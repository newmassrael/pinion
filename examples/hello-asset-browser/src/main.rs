//! `hello-asset-browser` — R927 §5.22 §5.23 §5.27: **async source-side
//! sort/filter** over a virtualized, out-of-memory 10 000-row source. The
//! Model/View-at-scale frontier the self-hosted editor's Content Browser needs
//! when the asset set never fits in memory.
//!
//! ## Why this is not `hello-lazy-list` with controls (the genuine new axis)
//!
//! `hello-lazy-list` (R924) virtualizes + lazy-loads a list whose row *order*
//! is fixed (row `i` is always source row `i`). The moment a real asset browser
//! grows a **Sort** and a **Filter**, the in-memory proxy family
//! (`GridSortState`, `ViewOrderState`, `GroupOrderState`) no longer applies:
//! those sort/filter a *materialized* `Vec` the client holds. Here the client
//! never holds the set — so sort and filter cannot be client-side proxies.
//!
//! The textbook model for an out-of-memory source is **server-side query
//! parameters**: sort and filter are pushed *down to the source*, exactly like
//! `SELECT … WHERE kind = ? ORDER BY ? LIMIT … OFFSET …` (or Unreal's Content
//! Browser, or a file explorer over a network share). Changing either:
//!
//! - **re-queries the source** — a page fetched under one ordering is never
//!   reused under another, so the page cache is keyed by the *full query*
//!   `(sort, filter, page)`, not the page index alone;
//! - **changes the row count** — the *filtered* count is itself a queried
//!   value (you cannot count rows you have not loaded), so a `Resource<usize>`
//!   keyed by the filter sizes the virtual list; until it resolves the list is
//!   empty and the status reads "Counting…";
//! - **resets the scroll** — a new ordering invalidates the old scroll
//!   position (clicking a column header jumps to the top, the universal idiom).
//!
//! ## Architecture (unidirectional, two reactive query stages)
//!
//! Two query parameters live as Signals — `sort: Signal<SortMode>` and
//! `filter: Signal<FilterKey>`. Two [`Effect`]s react to them:
//!
//! 1. **count loader** — subscribed to `filter` only (sorting never changes how
//!    many rows match): fetches the filtered count into a `Resource<usize>`.
//! 2. **page loader** — subscribed to `sort`, `filter`, the scroll offset *and*
//!    the count `Resource`'s state: once the count is `Ready(n)`, it computes
//!    the visible window over `n` and ensures every visible `(sort, filter,
//!    page)` page is fetched into a [`ResourceCache`](pinion_core::ResourceCache)
//!    — the keyed-async-carrier substrate lifted at R927 (lazy-list is its
//!    first consumer, this its second).
//!
//! The reducer mutates *only* the Signals (+ a scroll reset); the Effects own
//! all fetching, the view is pure (§6.3) — it reads `sort` / `filter` / the
//! count state / the scroll offset / each visible page's state and maps them to
//! sorted-filtered rows or skeletons. Both async stages drive through the
//! shell-polled [`LocalTaskPump`](pinion_core::LocalTaskPump) (R761.1 / R924).
//!
//! ## Latency model (ZERO-FLAKE)
//!
//! Every count + page fetch is a deterministic [`DeferredReady`] future
//! (`Pending` a fixed number of polls, then `Ready`). Each `scene/snapshot
//! from=paint` advances the pump one step, so the demo's own polling drives the
//! count → page → rows cascade with no wall-clock race (R923 SSOT).
//!
//! ## AI-first witness (§2 #7)
//!
//! `scene/snapshot` reports the windowed `assetbrowser#<pos>` rows (by visual
//! position, since the source index varies with the ordering), a `role=status`
//! count line ("10000 assets" / "1667 Texture assets" / "Counting…"), and the
//! Sort / Filter buttons. `scene/click` the buttons to re-query the source and
//! watch the count, the row order, and the membership change as data. See
//! `tools/demos/r927_asset_browser.py`.

use pinion_a11y::{windowed_list_nodes, AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::reactive::{
    batch, DeferredReady, Effect, Owner, Resource, ResourceCache, ResourceState, Signal,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::{compute_visible_range, VisibleWindow};
use pinion_core::{use_local_task_pump, Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::button::{
    button_a11y_state, button_scene, read_button_focused, read_button_state, ButtonColors,
    ButtonStyle,
};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::virtual_list::view_virtual_list;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloAssetBrowserRenderer, HelloAssetBrowserRendererError);

const WIN_W: u32 = 420;
const WIN_H: u32 = 560;
const THEME_TAG: &str = "app";

/// Total source size — large, while the client only ever holds the visible
/// pages and never materialises the whole set.
const N_TOTAL: usize = 10_000;
/// Rows fetched per async page.
const PAGE_SIZE: usize = 100;
/// Uniform per-row vertical slot (logical px).
const ROW_PITCH: u32 = 32;
/// Rows built above + below the strict visible window (fast-flick gap guard).
const OVERSCAN: usize = 4;
/// Scroll viewport width.
const VIEWPORT_W: u32 = 340;
/// Scroll viewport height — exactly 12 rows tall.
const VIEWPORT_H: u32 = 12 * ROW_PITCH;
/// `Pending` polls before a *count* query resolves (deterministic source
/// latency; keeps "Counting…" observable without a wall-clock sleep).
const COUNT_LATENCY_POLLS: u32 = 12;
/// `Pending` polls before a *page* query resolves (keeps the skeleton
/// observable across frames; ZERO-FLAKE — each `scene/snapshot from=paint`
/// advances the pump one step, matching the R924 lazy-list latency margin).
const FETCH_LATENCY_POLLS: u32 = 24;

/// Primary `ButtonExternal` (Sort) + a11y button tag.
const SORT_TAG: &str = "ab_sort";
/// Secondary `ButtonExternal` (Filter) + a11y button tag.
const FILTER_TAG: &str = "ab_filter";
/// Paint-root + a11y `list` container tag, and the windowed-row tag prefix.
const LIST_TAG: &str = "assetbrowser";
/// `role=status` count line tag.
const COUNT_TAG: &str = "ab_count";
/// Cache key for the scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "ab_scroll";
/// Paint + state tag for the interactive scrollbar peer.
const SCROLLBAR_TAG: &str = "ab_scrollbar";

const SORT_HOVER_KEY: &str = "ab_sort_hover";
const FILTER_HOVER_KEY: &str = "ab_filter_hover";

/// `Owner::cache` key for the `sort` query `Signal`.
const SORT_SIGNAL_KEY: &str = "ab.sort";
/// `Owner::cache` key for the `filter` query `Signal`.
const FILTER_SIGNAL_KEY: &str = "ab.filter";
/// `Owner::cache` key for the filtered-count `Resource`.
const COUNT_KEY: &str = "ab.count";
/// `Owner::cache` key for the per-`(sort, filter, page)` page cache.
const PAGE_CACHE_KEY: &str = "ab.pages";
/// `Owner::cache` key for the lifetime-held count-query `Effect`.
const COUNT_LOADER_KEY: &str = "ab.count_loader";
/// `Owner::cache` key for the lifetime-held page-prefetch `Effect`.
const PAGE_LOADER_KEY: &str = "ab.page_loader";

/// Asset kinds + extensions, rotated per source row.
const KINDS: [(&str, &str); 6] = [
    ("Texture", "png"),
    ("Mesh", "obj"),
    ("Audio", "wav"),
    ("Script", "rs"),
    ("Shader", "wgsl"),
    ("Scene", "pinion"),
];

/// Source-side ordering — a query parameter, not a client proxy. `Natural` is
/// source-index order; the size modes sort the *whole filtered set* by file
/// size (the source's `ORDER BY size`), so the visible page reflects a global
/// order the client never materialises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum SortMode {
    /// Source-index order (the backing-store order).
    Natural,
    /// Ascending file size, ties broken by source index.
    SizeAsc,
    /// Descending file size, ties broken by source index.
    SizeDesc,
}

/// Filter query parameter: `None` = all kinds, `Some(k)` = only `KINDS[k]`.
type FilterKey = Option<u8>;

/// A fetched page is identified by its *full query*: the ordering, the filter,
/// and the page position. Changing sort or filter yields fresh keys, so pages
/// are never reused across orderings (the source must be re-queried).
type PageKey = (SortMode, FilterKey, usize);

/// The per-query page cache — the R927 `ResourceCache` substrate (lazy-list's
/// page cache is its first consumer; this composite-keyed cache its second).
type PageCache = ResourceCache<PageKey, Vec<AssetRow>, String>;

/// The visible pages' resolved states, snapshotted once per frame.
type PageStates = HashMap<PageKey, ResourceState<Vec<AssetRow>, String>>;

/// One row of the (out-of-memory) source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct AssetRow {
    name: String,
    kind: String,
    size_kb: u32,
}

/// The human-readable descriptor — the SSOT for the painted row text (the
/// `listitem` accessible name is derived from it by `enrich_names_from_scene`,
/// the windowed-list a11y convention).
fn row_label(row: &AssetRow) -> String {
    format!("{} ({}, {} KB)", row.name, row.kind, row.size_kb)
}

// ─── scripted out-of-memory source (a stand-in for a server-side query) ─────

/// The kind index of source row `i` (which `KINDS` entry it rotates to).
fn kind_of(i: usize) -> u8 {
    u8::try_from(i % KINDS.len()).unwrap_or(0)
}

/// The file size (KB) of source row `i` — a deterministic spread the size sort
/// reorders.
fn size_kb_of(i: usize) -> u32 {
    u32::try_from((i * 37 + 11) % 900 + 8).unwrap_or(0)
}

/// Synthesise source row `i`.
fn row_at(i: usize) -> AssetRow {
    let (kind, ext) = KINDS[i % KINDS.len()];
    AssetRow {
        name: format!("asset_{i:05}.{ext}"),
        kind: kind.to_owned(),
        size_kb: size_kb_of(i),
    }
}

/// How many source rows match `filter` — the *count query* a real source
/// answers from an index without materialising the set. Closed-form (the kinds
/// rotate every `KINDS.len()` rows), so the client never scans the source.
fn matching_count(filter: FilterKey) -> usize {
    match filter {
        None => N_TOTAL,
        Some(k) => {
            let base = N_TOTAL / KINDS.len();
            let rem = N_TOTAL % KINDS.len();
            base + usize::from(usize::from(k) < rem)
        }
    }
}

/// All source indices matching `filter`, in source order. The size sort needs
/// the whole matching set to order it — this stands in for the source's own
/// indexed `WHERE kind = ?` scan; the client never sees it (only the page slice
/// the fetch returns).
fn matching_indices(filter: FilterKey) -> Vec<usize> {
    match filter {
        None => (0..N_TOTAL).collect(),
        Some(k) => (0..N_TOTAL).filter(|&i| kind_of(i) == k).collect(),
    }
}

/// The source index at visual position `pos` under a `Natural` ordering and
/// `filter` — closed-form (no scan): unfiltered it is `pos`; filtered to kind
/// `k` the matching rows are `k, k+6, k+12, …`, so the `pos`-th is
/// `k + pos·KINDS.len()`.
fn natural_index(filter: FilterKey, pos: usize) -> usize {
    match filter {
        None => pos,
        Some(k) => usize::from(k) + pos * KINDS.len(),
    }
}

/// The source indices occupying visual positions `start..end` under
/// `(sort, filter)`. `Natural` is closed-form; the size modes materialise the
/// filtered set (the source's work), sort it, and slice — so a page reflects a
/// *global* order, not a per-page sort.
fn ordered_indices_slice(sort: SortMode, filter: FilterKey, start: usize, end: usize) -> Vec<usize> {
    match sort {
        SortMode::Natural => (start..end).map(|pos| natural_index(filter, pos)).collect(),
        SortMode::SizeAsc | SortMode::SizeDesc => {
            let mut all = matching_indices(filter);
            let desc = matches!(sort, SortMode::SizeDesc);
            all.sort_by(|&a, &b| {
                let by_size = size_kb_of(a).cmp(&size_kb_of(b));
                let by_size = if desc { by_size.reverse() } else { by_size };
                // Stable tiebreak on source index (ascending) for a total order.
                by_size.then(a.cmp(&b))
            });
            let end = end.min(all.len());
            if start >= end {
                Vec::new()
            } else {
                all[start..end].to_vec()
            }
        }
    }
}

/// The rows on page `page` of the `(sort, filter)` view — the source's
/// `LIMIT PAGE_SIZE OFFSET page·PAGE_SIZE` answer. Only this slice is ever
/// materialised on the client.
fn page_rows(sort: SortMode, filter: FilterKey, page: usize) -> Vec<AssetRow> {
    let count = matching_count(filter);
    let start = page * PAGE_SIZE;
    if start >= count {
        return Vec::new();
    }
    let end = (start + PAGE_SIZE).min(count);
    ordered_indices_slice(sort, filter, start, end)
        .into_iter()
        .map(row_at)
        .collect()
}

/// The deferred *count query* for `filter` (a deterministic latency carrier).
fn fetch_count(filter: FilterKey) -> DeferredReady<Result<usize, String>> {
    DeferredReady::new(COUNT_LATENCY_POLLS, Ok(matching_count(filter)))
}

/// The deferred *page query* for `(sort, filter, page)`.
fn fetch_page(
    sort: SortMode,
    filter: FilterKey,
    page: usize,
) -> DeferredReady<Result<Vec<AssetRow>, String>> {
    DeferredReady::new(FETCH_LATENCY_POLLS, Ok(page_rows(sort, filter, page)))
}

// ─── reactive query layer (sort/filter Signals + count + page Effects) ──────

fn sort_signal() -> Rc<Signal<SortMode>> {
    Owner::current()
        .expect("sort_signal() requires an active Owner scope")
        .cache(SORT_SIGNAL_KEY, || Signal::new(SortMode::Natural))
}

fn filter_signal() -> Rc<Signal<FilterKey>> {
    Owner::current()
        .expect("filter_signal() requires an active Owner scope")
        .cache(FILTER_SIGNAL_KEY, || Signal::new(None))
}

/// The filtered-count carrier. Starts `Loading`; the count Effect's eager run
/// resolves the unfiltered count at boot.
fn count_resource() -> Rc<Resource<usize, String>> {
    Owner::current()
        .expect("count_resource() requires an active Owner scope")
        .cache(COUNT_KEY, Resource::loading)
}

fn page_cache() -> Rc<PageCache> {
    Owner::current()
        .expect("page_cache() requires an active Owner scope")
        .cache(PAGE_CACHE_KEY, PageCache::new)
}

/// The next sort in the toolbar cycle (Index → Size↑ → Size↓ → Index).
fn next_sort(sort: SortMode) -> SortMode {
    match sort {
        SortMode::Natural => SortMode::SizeAsc,
        SortMode::SizeAsc => SortMode::SizeDesc,
        SortMode::SizeDesc => SortMode::Natural,
    }
}

/// The next filter in the toolbar cycle (All → each kind → All).
fn next_filter(filter: FilterKey) -> FilterKey {
    match filter {
        None => Some(0),
        Some(k) if usize::from(k) + 1 < KINDS.len() => Some(k + 1),
        Some(_) => None,
    }
}

/// The 0-based page indices the window touches, or an empty iterator when
/// nothing is visible (so a `Loading`/zero count fetches no pages).
fn visible_pages(window: &VisibleWindow) -> impl Iterator<Item = usize> {
    let span = if window.is_empty() {
        None
    } else {
        let first = window.first / PAGE_SIZE;
        let last = (window.first + window.count - 1) / PAGE_SIZE;
        Some(first..=last)
    };
    span.into_iter().flatten()
}

/// Lifetime marker holding the count-query [`Effect`] (R665 retention).
struct CountLoaderMarker {
    _effect: Effect,
}

/// Install the count-query [`Effect`] once. Subscribed to `filter` only (the
/// match count is independent of the ordering), it refetches the filtered count
/// whenever the filter changes — including its eager run at boot (the
/// unfiltered count).
fn install_count_loader() -> Rc<CountLoaderMarker> {
    let owner = Owner::current().expect("install_count_loader() requires an active Owner scope");
    let filter = filter_signal();
    let count = count_resource();
    let pump = use_local_task_pump();
    let owner_for_effect = owner.clone();
    owner.cache(COUNT_LOADER_KEY, move || {
        let filter_e = filter.clone();
        let count_e = count.clone();
        let pump_e = pump.clone();
        let effect = Effect::new(&owner_for_effect, move || {
            let f = filter_e.get(); // subscribe to the filter query param
            count_e.fetch_with(&*pump_e, fetch_count(f));
        });
        CountLoaderMarker { _effect: effect }
    })
}

/// Lifetime marker holding the page-prefetch [`Effect`] (R665 retention).
struct PageLoaderMarker {
    _effect: Effect,
}

/// Install the page-prefetch [`Effect`] once. Subscribed to `sort`, `filter`,
/// the scroll offset, *and* the count `Resource`'s state, it waits until the
/// count is `Ready(n)` (the virtual extent is unknown before then) and ensures
/// every visible `(sort, filter, page)` page is fetched. A sort/filter change
/// yields fresh page keys, so the source is re-queried; the count change
/// re-fires this Effect through the count-state subscription.
fn install_page_loader() -> Rc<PageLoaderMarker> {
    let owner = Owner::current().expect("install_page_loader() requires an active Owner scope");
    let sort = sort_signal();
    let filter = filter_signal();
    let count = count_resource();
    let scroll = use_scroll_state(SCROLL_KEY);
    let cache = page_cache();
    let pump = use_local_task_pump();
    let owner_for_effect = owner.clone();
    owner.cache(PAGE_LOADER_KEY, move || {
        let sort_e = sort.clone();
        let filter_e = filter.clone();
        let count_e = count.clone();
        let scroll_e = scroll.clone();
        let cache_e = cache.clone();
        let pump_e = pump.clone();
        let effect = Effect::new(&owner_for_effect, move || {
            let sort = sort_e.get();
            let filter = filter_e.get();
            let offset = scroll_e.offset_y();
            // Subscribe to the count; the extent is unknown until it resolves.
            if let ResourceState::Ready(n) = count_e.state() {
                let window = compute_visible_range(offset, VIEWPORT_H, n, ROW_PITCH, OVERSCAN);
                for page in visible_pages(&window) {
                    cache_e.ensure((sort, filter, page), &*pump_e, || {
                        fetch_page(sort, filter, page)
                    });
                }
            }
        });
        PageLoaderMarker { _effect: effect }
    })
}

// ─── status / row text ──────────────────────────────────────────────────────

/// The `role=status` count line — the SSOT for the visible count text and the
/// live region's accessible name.
fn count_line(filter: FilterKey, count_state: &ResourceState<usize, String>) -> String {
    match count_state {
        ResourceState::Loading => "Counting\u{2026}".to_owned(),
        ResourceState::Ready(n) => match filter {
            None => format!("{n} assets"),
            Some(k) => format!("{n} {} assets", KINDS[usize::from(k)].0),
        },
        ResourceState::Error(err) => format!("count error: {err}"),
    }
}

/// The Sort button label.
fn sort_label(sort: SortMode) -> &'static str {
    match sort {
        SortMode::Natural => "Sort: Index",
        SortMode::SizeAsc => "Sort: Size asc",
        SortMode::SizeDesc => "Sort: Size desc",
    }
}

/// The Filter button label.
fn filter_label(filter: FilterKey) -> String {
    match filter {
        None => "Filter: All".to_owned(),
        Some(k) => format!("Filter: {}", KINDS[usize::from(k)].0),
    }
}

fn zebra_fill(index: usize, theme: &Theme) -> pinion_core::style::Color {
    if index % 2 == 0 {
        theme.resolve(ColorRole::SurfaceContainerLow)
    } else {
        theme.resolve(ColorRole::SurfaceContainer)
    }
}

/// One row's inner text node, by the visible page's resolved state.
fn row_text(
    page_state: Option<&ResourceState<Vec<AssetRow>, String>>,
    off: usize,
    theme: &Theme,
) -> Scene {
    let (content, role) = match page_state {
        Some(ResourceState::Ready(rows)) => match rows.get(off) {
            Some(row) => (row_label(row), ColorRole::OnSurface),
            None => ("Loading\u{2026}".to_owned(), ColorRole::OnSurfaceMuted),
        },
        Some(ResourceState::Error(_)) => ("Unavailable".to_owned(), ColorRole::Error),
        // Loading, or not-yet-requested (None) → skeleton placeholder.
        _ => ("Loading\u{2026}".to_owned(), ColorRole::OnSurfaceMuted),
    };
    Scene::Text(TextNode::styled(
        content,
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(theme.resolve(role)),
    ))
}

/// One virtualized row, tagged `assetbrowser#<pos>` by **visual position**
/// (the source index varies with the ordering, so the position is the stable
/// addressing key). Renders the page's row, or a skeleton.
fn build_row(
    sort: SortMode,
    filter: FilterKey,
    pos: usize,
    page_states: &PageStates,
    theme: &Theme,
) -> Scene {
    let page = pos / PAGE_SIZE;
    let off = pos % PAGE_SIZE;
    let state = page_states.get(&(sort, filter, page));
    Scene::Container(
        ContainerNode::new(vec![row_text(state, off, theme)])
            .with_tag(format!("{LIST_TAG}#{pos}"))
            .with_style(BoxStyle::filled(zebra_fill(pos, theme)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(VIEWPORT_W, ROW_PITCH))
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// The visible pages' resolved states, snapshotted once. Subscribes the view to
/// each visible `(sort, filter, page)` entry so a page resolution re-renders.
fn resolve_visible_pages(
    sort: SortMode,
    filter: FilterKey,
    window: &VisibleWindow,
    cache: &PageCache,
) -> PageStates {
    cache.snapshot(visible_pages(window).map(|page| (sort, filter, page)))
}

/// Render one toolbar button through the `pinion_widget_paint::button` SSOT.
fn toolbar_button(
    tag: &'static str,
    label: &str,
    state: ButtonState,
    focused: bool,
    hover_key: &'static str,
    theme: &Theme,
) -> Scene {
    button_scene(
        label,
        state,
        focused,
        hover_key,
        &ButtonColors::filled_tonal(theme),
        &ButtonStyle::m3_default(tag)
            .with_size(Size::px(160, 38))
            .with_label_font_size_px(14),
    )
}

// ─── view ─────────────────────────────────────────────────────────────────

/// Cached postures of the two toolbar buttons + their focus flags
/// (`[sort, filter]`). The query params + async data live in the reactive layer
/// the owner-scoped view + `access_node` read directly.
type AssetBrowserState = (ButtonState, ButtonState, [bool; 2]);

/// view-fn (§6.3): pure sync `(button postures) -> Scene`. Reads the sort/filter
/// query Signals, the count `Resource`, the scroll offset, and each visible
/// page's state; the data is virtual, out-of-memory, and source-side ordered.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: AssetBrowserState, _frame: &Frame) -> Scene {
    let (sort_posture, filter_posture, [sort_f, filter_f]) = state;
    let scroll = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let cache = page_cache();
    let sort = sort_signal().get();
    let filter = filter_signal().get();
    let count_state = count_resource().state();
    let item_count = if let ResourceState::Ready(n) = &count_state { *n } else { 0 };

    // Snapshot the visible pages' states once — subscribes the view to each.
    let window = compute_visible_range(scroll.offset_y(), VIEWPORT_H, item_count, ROW_PITCH, OVERSCAN);
    let page_states = resolve_visible_pages(sort, filter, &window, &cache);

    let title = Scene::Text(TextNode::styled(
        "Asset browser (10,000 rows, source-side sort + filter)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(15)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let toolbar = Scene::Container(
        ContainerNode::new(vec![
            toolbar_button(
                SORT_TAG,
                sort_label(sort),
                sort_posture,
                sort_f,
                SORT_HOVER_KEY,
                &theme,
            ),
            toolbar_button(
                FILTER_TAG,
                &filter_label(filter),
                filter_posture,
                filter_f,
                FILTER_HOVER_KEY,
                &theme,
            ),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_gap(12),
        ),
    );

    let count = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            count_line(filter, &count_state),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(COUNT_TAG)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center),
        ),
    );

    // The windowed list — `item_count == 0` while the count is loading yields an
    // empty window, so the list shows nothing until the filtered count resolves.
    let list = view_virtual_list(
        &scroll,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        item_count,
        ROW_PITCH,
        OVERSCAN,
        |pos| build_row(sort, filter, pos, &page_states, &theme),
    );

    let scrollbar_style = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual =
        view_vertical_scrollbar(&scroll, &theme, &scrollbar_style, scrollbar_interaction.get());

    let list_root = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_tag(LIST_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    Scene::Container(
        ContainerNode::new(vec![title, toolbar, count, list_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_gap(12),
            ),
    )
}

struct AssetBrowserView;

impl WidgetCore for AssetBrowserView {
    type State = AssetBrowserState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Install the count + page query Effects FIRST (boot eager runs → the
        // unfiltered count, then page 0 once it resolves), so the data layer is
        // live before the first paint and the side-effecting fetches never run
        // inside the pure view-fn.
        let _count = install_count_loader();
        let _pages = install_page_loader();
        vec![
            ExtraExternal::new(FILTER_TAG, Box::new(ButtonExternal::new())),
            scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG),
        ]
    }

    fn tag() -> &'static str {
        SORT_TAG
    }

    fn read_state(scene: &Scene) -> AssetBrowserState {
        (
            read_button_state(scene, SORT_TAG),
            read_button_state(scene, FILTER_TAG),
            [
                read_button_focused(scene, SORT_TAG),
                read_button_focused(scene, FILTER_TAG),
            ],
        )
    }

    fn view(state: AssetBrowserState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-asset-browser (R927 §5.22 §5.23 §5.27)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// The Sort + Filter buttons are static Tab stops.
    fn focusable_tags() -> Vec<&'static str> {
        vec![SORT_TAG, FILTER_TAG]
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        [SORT_TAG, FILTER_TAG]
            .into_iter()
            .any(|tag| apply_aria_activate(scene, focused, key, tag))
    }

    /// R927 reducer: a toolbar click mutates *only* the query Signal and resets
    /// the scroll (a re-query jumps to the top); the count + page Effects react
    /// and perform the fetches. The two Signal-and-scroll mutations are batched
    /// so the reactive flush sees one atomic query change.
    fn update(
        _state: AssetBrowserState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "ab_sort.click" => {
                let sort = sort_signal();
                let next = next_sort(sort.get());
                batch(|| {
                    sort.set(next);
                    use_scroll_state(SCROLL_KEY).scroll_to(0, 0);
                });
            }
            "ab_filter.click" => {
                let filter = filter_signal();
                let next = next_filter(filter.get());
                batch(|| {
                    filter.set(next);
                    use_scroll_state(SCROLL_KEY).scroll_to(0, 0);
                });
            }
            _ => {}
        }
        Vec::new()
    }
}

impl WidgetA11y for AssetBrowserView {
    /// WAI-ARIA virtualized `list` whose `aria-setsize` is the **filtered**
    /// count (0 while it loads), a `role=status` count line, and the Sort +
    /// Filter `button`s. A windowed `listitem`'s name comes from the same
    /// painted row text via `enrich_names_from_scene` (the windowed-list
    /// convention shared with `hello-lazy-list`).
    fn access_node(state: &AssetBrowserState, focused: Option<&str>) -> Vec<AccessNode> {
        let (sort_posture, filter_posture, _focus) = *state;
        let scroll = use_scroll_state(SCROLL_KEY);
        let filter = filter_signal().get();
        let count_state = count_resource().state();
        let item_count = if let ResourceState::Ready(n) = &count_state { *n } else { 0 };
        let window =
            compute_visible_range(scroll.offset_y(), VIEWPORT_H, item_count, ROW_PITCH, OVERSCAN);

        let mut nodes = windowed_list_nodes(
            LIST_TAG,
            "Assets",
            u32::try_from(item_count).unwrap_or(u32::MAX),
            &window,
        );
        nodes.push(
            AccessNode::new(COUNT_TAG, AriaRole::Status).with_name(count_line(filter, &count_state)),
        );
        nodes.push(
            AccessNode::new(SORT_TAG, AriaRole::Button)
                .with_state(button_a11y_state(sort_posture, focused == Some(SORT_TAG))),
        );
        nodes.push(
            AccessNode::new(FILTER_TAG, AriaRole::Button)
                .with_state(button_a11y_state(filter_posture, focused == Some(FILTER_TAG))),
        );
        nodes
    }
}

impl WidgetView for AssetBrowserView {
    type Renderer = HelloAssetBrowserRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<AssetBrowserView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> AssetBrowserState {
        (ButtonState::Idle, ButtonState::Idle, [false, false])
    }

    /// Drive the owner-scoped `LocalTaskPump` to completion — what the shell
    /// does each frame. The count resolves first, then the page it gates.
    fn drain_pump() {
        for _ in 0..(COUNT_LATENCY_POLLS + FETCH_LATENCY_POLLS + 8) {
            if !use_local_task_pump().poll() {
                break;
            }
        }
    }

    /// Install both query Effects (mirrors `create_extra_externals`).
    fn boot() {
        let _ = install_count_loader();
        let _ = install_page_loader();
    }

    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(tag.to_owned(), pinion_core::external::IntrospectValue::Null)
    }

    fn dispatch(tag: &str) {
        let _ = AssetBrowserView::update(idle(), &intent(tag));
    }

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_container(ch, tag)),
            Scene::Scroll(s) => find_container(s.content.as_ref(), tag),
            _ => None,
        }
    }

    fn row_text_of(scene: &Scene, pos: usize) -> Option<String> {
        let c = find_container(scene, &format!("{LIST_TAG}#{pos}"))?;
        c.children.iter().find_map(|ch| match ch {
            Scene::Text(t) => Some(t.content.clone()),
            _ => None,
        })
    }

    fn count_text(scene: &Scene) -> Option<String> {
        let c = find_container(scene, COUNT_TAG)?;
        c.children.iter().find_map(|ch| match ch {
            Scene::Text(t) => Some(t.content.clone()),
            _ => None,
        })
    }

    // ── pure source-query functions ────────────────────────────────────────

    #[test]
    fn r927_matching_count_agrees_with_indices_len() {
        // The closed-form count query must equal the materialised set's length
        // (the SSOT for "how many rows match"), for every filter.
        assert_eq!(matching_count(None), N_TOTAL);
        assert_eq!(matching_count(None), matching_indices(None).len());
        for k in 0..u8::try_from(KINDS.len()).unwrap() {
            assert_eq!(
                matching_count(Some(k)),
                matching_indices(Some(k)).len(),
                "count query disagrees with the set for kind {k}",
            );
        }
        // Kinds 0..3 get the remainder row (10000 % 6 == 4), 4..5 do not.
        assert_eq!(matching_count(Some(0)), 1667);
        assert_eq!(matching_count(Some(4)), 1666);
    }

    #[test]
    fn r927_natural_index_addresses_the_filtered_source() {
        assert_eq!(natural_index(None, 0), 0);
        assert_eq!(natural_index(None, 5), 5);
        // Filter to kind 1 (Mesh): matching rows are 1, 7, 13, …
        assert_eq!(natural_index(Some(1), 0), 1);
        assert_eq!(natural_index(Some(1), 1), 7);
        assert_eq!(natural_index(Some(1), 2), 13);
    }

    #[test]
    fn r927_page_under_filter_is_all_that_kind() {
        // Filter to Mesh (kind 1), Natural order: page 0 row 0 is asset_00001,
        // and every row in the page is a Mesh.
        let rows = page_rows(SortMode::Natural, Some(1), 0);
        assert_eq!(rows.len(), PAGE_SIZE);
        assert_eq!(rows[0].name, "asset_00001.obj");
        assert!(rows.iter().all(|r| r.kind == "Mesh"), "filtered page is all Mesh");
    }

    #[test]
    fn r927_size_sort_orders_the_page_globally() {
        // SizeAsc: the first page's sizes are non-decreasing AND start at the
        // global minimum size of the whole set (a global order, not per-page).
        let asc = page_rows(SortMode::SizeAsc, None, 0);
        assert_eq!(asc.len(), PAGE_SIZE);
        for w in asc.windows(2) {
            assert!(w[0].size_kb <= w[1].size_kb, "ascending size order");
        }
        let global_min = (0..N_TOTAL).map(size_kb_of).min().unwrap();
        assert_eq!(asc[0].size_kb, global_min, "page 0 starts at the global minimum");

        let desc = page_rows(SortMode::SizeDesc, None, 0);
        for w in desc.windows(2) {
            assert!(w[0].size_kb >= w[1].size_kb, "descending size order");
        }
        let global_max = (0..N_TOTAL).map(size_kb_of).max().unwrap();
        assert_eq!(desc[0].size_kb, global_max, "page 0 starts at the global maximum");
    }

    #[test]
    fn r927_toolbar_cycles_are_total() {
        assert_eq!(next_sort(SortMode::Natural), SortMode::SizeAsc);
        assert_eq!(next_sort(SortMode::SizeAsc), SortMode::SizeDesc);
        assert_eq!(next_sort(SortMode::SizeDesc), SortMode::Natural);
        assert_eq!(next_filter(None), Some(0));
        assert_eq!(next_filter(Some(4)), Some(5));
        assert_eq!(next_filter(Some(5)), None, "last kind wraps back to All");
    }

    // ── reactive query cascade ─────────────────────────────────────────────

    #[test]
    fn r927_boot_counts_then_loads_first_page() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            boot();
            // Before the pump runs, the count is still in flight → no rows.
            let loading = view(idle(), &Frame::default());
            assert_eq!(count_text(&loading).as_deref(), Some("Counting\u{2026}"));
            assert!(row_text_of(&loading, 0).is_none(), "no rows until the count resolves");
            drain_pump();
            view(idle(), &Frame::default())
        });
        // Count resolved to the unfiltered total, page 0 loaded its rows.
        assert_eq!(count_text(&scene).as_deref(), Some("10000 assets"));
        assert_eq!(
            row_text_of(&scene, 0).as_deref(),
            Some("asset_00000.png (Texture, 19 KB)"),
        );
    }

    #[test]
    fn r927_only_visible_pages_are_resident() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            // The boot window touches page 0 only → exactly one resident page.
            assert_eq!(page_cache().len(), 1, "only the visible page fetched");
            assert!(page_cache().contains(&(SortMode::Natural, None, 0)));
            assert!(!page_cache().contains(&(SortMode::Natural, None, 50)), "far pages not fetched");
        });
    }

    #[test]
    fn r927_filter_resets_scroll_and_requeries_count() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            // Scroll deep (set a bound first, as a direct view() skips layout).
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N_TOTAL).unwrap() * i32::try_from(ROW_PITCH).unwrap());
            scroll.scroll_to(0, 3000 * i32::try_from(ROW_PITCH).unwrap());
            assert!(scroll.offset_y() > 0, "scrolled away from the top");
            // Filter to Texture (kind 0): the reducer resets the scroll and the
            // count Effect re-queries.
            dispatch("ab_filter.click");
            assert_eq!(scroll.offset_y(), 0, "a re-query jumps back to the top");
            assert_eq!(filter_signal().get(), Some(0));
            // Count is re-fetching (Loading), then resolves to the filtered count.
            assert_eq!(count_resource().state(), ResourceState::Loading);
            drain_pump();
            assert_eq!(count_resource().state(), ResourceState::Ready(1667));
            // Page 0 under the new filter is resident; row 0 is the first Texture.
            let scene = view(idle(), &Frame::default());
            assert_eq!(count_text(&scene).as_deref(), Some("1667 Texture assets"));
            assert_eq!(
                row_text_of(&scene, 0).as_deref(),
                Some("asset_00000.png (Texture, 19 KB)"),
            );
        });
    }

    #[test]
    fn r927_sort_requeries_pages_keeping_count() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            let count_gen = count_resource().generation();
            // Sort by size: the count is unchanged (sorting does not change the
            // match count), but the page is re-queried in the new order.
            dispatch("ab_sort.click");
            assert_eq!(sort_signal().get(), SortMode::SizeAsc);
            assert_eq!(
                count_resource().generation(),
                count_gen,
                "sorting does not re-fetch the count",
            );
            assert!(
                page_cache().contains(&(SortMode::SizeAsc, None, 0)),
                "page 0 re-queried under the new ordering",
            );
            drain_pump();
            // The visible page now reflects the global ascending size order.
            let scene = view(idle(), &Frame::default());
            let r0 = row_text_of(&scene, 0).expect("row 0 present");
            assert!(r0.contains("KB)"), "row 0 loaded under the size sort: {r0}");
        });
    }

    #[test]
    fn r927_a11y_setsize_is_the_filtered_count() {
        let owner = Owner::new();
        let nodes = owner.run(|| {
            boot();
            drain_pump();
            // Filter to Audio (kind 2) and let it settle.
            dispatch("ab_filter.click"); // Texture
            dispatch("ab_filter.click"); // Mesh
            dispatch("ab_filter.click"); // Audio
            drain_pump();
            AssetBrowserView::access_node(&idle(), None)
        });
        let list = nodes.iter().find(|n| n.tag == LIST_TAG).expect("list node");
        assert_eq!(list.role, AriaRole::List);
        assert_eq!(
            list.size_of_set,
            Some(u32::try_from(matching_count(Some(2))).unwrap()),
            "aria-setsize is the filtered count",
        );
        assert!(
            nodes.iter().any(|n| n.tag == COUNT_TAG && n.role == AriaRole::Status),
            "count status node present",
        );
        assert_eq!(
            nodes.iter().filter(|n| n.role == AriaRole::Button).count(),
            2,
            "Sort + Filter buttons",
        );
    }

    #[test]
    fn r927_two_toolbar_buttons_are_focusable() {
        assert_eq!(AssetBrowserView::focusable_tags(), vec![SORT_TAG, FILTER_TAG]);
    }

    #[test]
    fn r927_row_label_is_single_ssot() {
        let row = AssetRow {
            name: "asset_00000.png".to_owned(),
            kind: "Texture".to_owned(),
            size_kb: 19,
        };
        assert_eq!(row_label(&row), "asset_00000.png (Texture, 19 KB)");
    }
}
