//! `hello-lazy-list` — R924 §5.22 §5.23 §5.27: **infinite-scroll virtualized
//! lazy-load**. The Model/View-at-scale backbone the self-hosted editor's
//! asset browser / scene outliner needs: 10 000 rows, windowed *and* fetched a
//! page at a time as the viewport moves.
//!
//! ## What this is (R744 virtualization × R923 async data)
//!
//! `hello-virtual-list` (R744) windows a 10 000-row list that is already fully
//! in memory. `hello-async-data` (R923) renders a *paged* view over an async
//! out-of-memory source, but only one page at a time (no scroll). R924 unifies
//! them: the list is **both** virtualized (the scene holds ~16 row nodes, not
//! 10 000) **and** lazy-loaded (each 100-row page is fetched asynchronously the
//! first time it scrolls into view; until it resolves the rows render as
//! skeletons).
//!
//! ## Architecture (unidirectional, Effect-driven prefetch)
//!
//! - A per-page `Resource` cache (`HashMap<page, Rc<Resource<Vec<AssetRow>>>>`
//!   in `Owner::cache`) — only fetched pages are ever resident.
//! - An [`Effect`] subscribed to the **scroll offset** [`Signal`] computes the
//!   visible page range (`compute_visible_range`) and kicks off a
//!   [`Resource::fetch_with`] for every page not yet in the cache, through the
//!   shell-polled [`LocalTaskPump`](pinion_core::LocalTaskPump). Boot loads the
//!   first visible pages via the Effect's eager run; scrolling re-runs it.
//! - The view is **pure** (§6.3): it reads `scroll.offset_y()` + each visible
//!   page's `resource.state()` and maps `(loaded rows | skeletons)` into the
//!   windowed scene. No fetching in the view.
//!
//! The fetch never happens in the view, so view-fn purity holds; the Effect is
//! installed once in `create_extra_externals`. This is the same Resource +
//! Effect + `DeferredReady` + pump substrate R923 introduced, now driven by the
//! scroll position instead of a button.
//!
//! ## Latency model (ZERO-FLAKE)
//!
//! Each page fetch is a deterministic [`DeferredReady`] future (`Pending`
//! `FETCH_LATENCY_POLLS` times, then `Ready`). Each `scene/snapshot from=paint`
//! advances the pump one step, so the demo's own polling drives a freshly
//! scrolled-to page from skeleton → rows with no wall-clock race.
//!
//! ## AI-first witness (§2 #7)
//!
//! `scene/snapshot` reports ~16 `lazylist#<i>` row nodes whose text is either
//! the loaded descriptor or "Loading…", plus a `role=status` band line —
//! `aria-setsize` on the list stays 10 000. Scroll via `scene/wheel`, and the
//! present rows shift to a higher band that starts as skeletons and resolves to
//! data. See `tools/demos/r924_lazy_list.py`.

use pinion_a11y::{windowed_list_nodes, AccessNode, WidgetA11y};
use pinion_core::external::{External, StubExternal};
use pinion_core::reactive::{DeferredReady, Effect, Owner, Resource, ResourceState};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::{compute_visible_range, VisibleWindow};
use pinion_core::{use_local_task_pump, Frame, LocalTaskPump, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::virtual_list::view_virtual_list;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloLazyListRenderer, HelloLazyListRendererError);

const WIN_W: u32 = 380;
const WIN_H: u32 = 500;
const THEME_TAG: &str = "app";

/// Total dataset size — large, while the rendered node count + resident page
/// count both stay small.
const N: usize = 10_000;
/// Rows fetched per async page. `N % PAGE_SIZE == 0` so every page is full.
const PAGE_SIZE: usize = 100;
/// Uniform per-row vertical slot (logical px). Uniform pitch → exact window math.
const ROW_PITCH: u32 = 32;
/// Rows built above + below the strict visible window (fast-flick gap guard).
const OVERSCAN: usize = 4;
/// Scroll viewport width.
const VIEWPORT_W: u32 = 300;
/// Scroll viewport height — exactly 12 rows tall.
const VIEWPORT_H: u32 = 12 * ROW_PITCH;
/// `Pending` polls before a page fetch resolves — a deterministic stand-in for
/// source latency that keeps the skeleton observable across frames (ZERO-FLAKE;
/// each `scene/snapshot from=paint` advances the pump one step).
const FETCH_LATENCY_POLLS: u32 = 24;

/// Paint-root + a11y `list` container tag, and the `StubExternal` anchor tag.
const LIST_TAG: &str = "lazylist";
/// `role=status` band line tag.
const STATUS_TAG: &str = "lazy_status";
/// Cache key for the scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "lazy_scroll";
/// Paint + state tag for the interactive scrollbar peer.
const SCROLLBAR_TAG: &str = "lazy_scrollbar";
/// `Owner::cache` key for the per-page `Resource` cache.
const PAGE_CACHE_KEY: &str = "lazy.page_cache";
/// `Owner::cache` key for the lifetime-held scroll-driven prefetch `Effect`.
const LOADER_KEY: &str = "lazy.loader";

/// Asset kinds + extensions, rotated per row.
const KINDS: [(&str, &str); 6] = [
    ("Texture", "png"),
    ("Mesh", "obj"),
    ("Audio", "wav"),
    ("Script", "rs"),
    ("Shader", "wgsl"),
    ("Scene", "pinion"),
];

/// One row of the (out-of-memory) dataset.
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

// ─── scripted out-of-memory paged source ──────────────────────────────────

/// Synthesise page `page`'s rows behind a deterministic fetch latency
/// ([`DeferredReady`], the R923 SSOT). Only fetched pages are ever
/// materialised — the out-of-memory contract.
fn fetch_page(page: usize) -> DeferredReady<Result<Vec<AssetRow>, String>> {
    DeferredReady::new(FETCH_LATENCY_POLLS, Ok(page_rows(page)))
}

/// The rows that live on page `page` of the backing store.
fn page_rows(page: usize) -> Vec<AssetRow> {
    let base = page * PAGE_SIZE;
    (0..PAGE_SIZE)
        .map(|r| {
            let i = base + r;
            let (kind, ext) = KINDS[i % KINDS.len()];
            let size = (i * 37 + 11) % 900 + 8;
            AssetRow {
                name: format!("asset_{i:05}.{ext}"),
                kind: kind.to_owned(),
                size_kb: u32::try_from(size).unwrap_or(0),
            }
        })
        .collect()
}

// ─── per-page Resource cache + scroll-driven prefetch Effect ───────────────

/// Per-page async carrier cache. Keyed by page index; only fetched pages are
/// resident. A `RefCell<HashMap>` (not a `Signal`): the *structure* need not be
/// reactive — each page's `Resource` carries its own reactive state, and the
/// view subscribes to the visible ones.
type PageCache = RefCell<HashMap<usize, Rc<Resource<Vec<AssetRow>, String>>>>;

fn page_cache() -> Rc<PageCache> {
    Owner::current()
        .expect("page_cache() requires an active Owner scope")
        .cache(PAGE_CACHE_KEY, || RefCell::new(HashMap::new()))
}

/// The 0-based page span the window touches (inclusive).
fn visible_pages(window: &VisibleWindow) -> std::ops::RangeInclusive<usize> {
    let first = window.first / PAGE_SIZE;
    let last = (window.first + window.count.saturating_sub(1)) / PAGE_SIZE;
    first..=last
}

/// Ensure every page the window touches has a `Resource` in flight: create +
/// `fetch_with` the missing ones. Owner-scoped side effect, run only from the
/// prefetch [`Effect`] (never the view).
fn ensure_pages_loaded(offset_y: i32, cache: &PageCache, pump: &LocalTaskPump) {
    let window = compute_visible_range(offset_y, VIEWPORT_H, N, ROW_PITCH, OVERSCAN);
    for page in visible_pages(&window) {
        let missing = !cache.borrow().contains_key(&page);
        if missing {
            let res = Rc::new(Resource::loading());
            cache.borrow_mut().insert(page, Rc::clone(&res));
            res.fetch_with(pump, fetch_page(page));
        }
    }
}

/// Lifetime marker holding the scroll-driven prefetch [`Effect`] (R665).
struct LoaderMarker {
    _effect: Effect,
}

/// Install the prefetch [`Effect`] once. It subscribes to the scroll-offset
/// `Signal` and, whenever it changes (including its eager initial run at boot),
/// fetches every newly-visible page that is not already cached. Pre-resolves
/// dependent cache slots before the factory (R666 nested-cache guard).
fn install_loader() -> Rc<LoaderMarker> {
    let owner = Owner::current().expect("install_loader() requires an active Owner scope");
    let scroll = use_scroll_state(SCROLL_KEY);
    let cache = page_cache();
    let pump = use_local_task_pump();
    let owner_for_effect = owner.clone();
    owner.cache(LOADER_KEY, move || {
        let scroll_e = scroll.clone();
        let cache_e = cache.clone();
        let pump_e = pump.clone();
        let effect = Effect::new(&owner_for_effect, move || {
            let offset = scroll_e.offset_y(); // subscribe to vertical scroll
            ensure_pages_loaded(offset, &cache_e, &pump_e);
        });
        LoaderMarker { _effect: effect }
    })
}

// ─── view ─────────────────────────────────────────────────────────────────

/// The visible pages' resolved states, snapshotted once per frame. Reading
/// each `Resource::state()` here subscribes the view to it, so a page
/// resolution re-renders — and avoids cloning a full page `Vec` per row.
type PageStates = HashMap<usize, ResourceState<Vec<AssetRow>, String>>;

fn resolve_visible_pages(window: &VisibleWindow, cache: &PageCache) -> PageStates {
    let mut out = PageStates::new();
    let c = cache.borrow();
    for page in visible_pages(window) {
        if let Some(res) = c.get(&page) {
            out.insert(page, res.state());
        }
    }
    out
}

/// The `role=status` band line — the SSOT for the visible-band text + the live
/// region's accessible name.
fn status_line(window: &VisibleWindow, page_states: &PageStates) -> String {
    let first_page = window.first / PAGE_SIZE;
    let loading = !matches!(page_states.get(&first_page), Some(ResourceState::Ready(_)));
    let last = window.first + window.count.saturating_sub(1);
    if loading {
        format!("Loading rows {}\u{2013}{}\u{2026}", window.first + 1, last + 1)
    } else {
        format!("Rows {}\u{2013}{} of {N}", window.first + 1, last + 1)
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
fn row_text(page_state: Option<&ResourceState<Vec<AssetRow>, String>>, off: usize, theme: &Theme) -> Scene {
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

/// One virtualized row, tagged `lazylist#<index>` so the windowed-list a11y
/// nodes + `enrich_names_from_scene` attach. Renders the loaded descriptor or
/// a skeleton, depending on whether the row's page has resolved.
fn build_row(index: usize, page_states: &PageStates, theme: &Theme) -> Scene {
    let page = index / PAGE_SIZE;
    let off = index % PAGE_SIZE;
    Scene::Container(
        ContainerNode::new(vec![row_text(page_states.get(&page), off, theme)])
            .with_tag(format!("{LIST_TAG}#{index}"))
            .with_style(BoxStyle::filled(zebra_fill(index, theme)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(VIEWPORT_W, ROW_PITCH))
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// view-fn (§6.3): pure sync `() -> Scene`. Reads the scroll offset + each
/// visible page's `Resource` state; the dataset is virtual *and* lazily fetched.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let cache = page_cache();

    // Snapshot the visible pages' states once (1-2 pages) — subscribes the view
    // to each, avoids a per-row page-Vec clone.
    let window = compute_visible_range(scroll.offset_y(), VIEWPORT_H, N, ROW_PITCH, OVERSCAN);
    let page_states = resolve_visible_pages(&window, &cache);

    let title = Scene::Text(TextNode::styled(
        "Lazy-loaded asset index (10,000 rows, paged async)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(15)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let status = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            status_line(&window, &page_states),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(STATUS_TAG)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center),
        ),
    );

    // The windowed list — `view_virtual_list` builds only the windowed indices.
    let list = view_virtual_list(
        &scroll,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        N,
        ROW_PITCH,
        OVERSCAN,
        |index| build_row(index, &page_states, &theme),
    );

    // Scrollbar peer — sized against the full extent (the sizer height the
    // layout pass wrote into `ScrollState::max_y`), sharing the same `Rc`.
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
        ContainerNode::new(vec![title, status, list_root])
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

struct LazyListView;

impl WidgetCore for LazyListView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Install the scroll-driven prefetch Effect FIRST (boot eager run →
        // fetch the initially visible pages) so the data layer is live before
        // the first paint, and the side-effecting fetch never runs in `view`.
        let _loader = install_loader();
        vec![scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG)]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Display-only list: nothing is a keyboard tab stop (the scrollbar is
    /// pointer / RPC driven).
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-lazy-list (R924 §5.22 §5.23 §5.27)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for LazyListView {
    /// WAI-ARIA virtualized `list`: `aria-setsize = N`, one `listitem` per
    /// rendered row with its absolute `aria-posinset`. The windowed topology is
    /// the shared `pinion_a11y::windowed_list_nodes` SSOT (same builder as
    /// `hello-virtual-list`); a skeleton row is still item `k` of `N` to AT —
    /// load state is conveyed by the rendered text, not the set position.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let window = compute_visible_range(scroll.offset_y(), VIEWPORT_H, N, ROW_PITCH, OVERSCAN);
        windowed_list_nodes(LIST_TAG, "Asset index", u32::try_from(N).unwrap_or(u32::MAX), &window)
    }
}

impl WidgetView for LazyListView {
    type Renderer = HelloLazyListRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<LazyListView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the owner-scoped `LocalTaskPump` to completion — what the shell
    /// does once per frame. A fetch is deferred `FETCH_LATENCY_POLLS` polls.
    fn drain_pump() {
        for _ in 0..(FETCH_LATENCY_POLLS + 8) {
            if !use_local_task_pump().poll() {
                break;
            }
        }
    }

    fn boot() {
        let _ = install_loader();
    }

    /// Count `lazylist#<i>` row containers anywhere in the scene.
    fn count_row_tags(scene: &Scene) -> usize {
        fn walk(scene: &Scene, n: &mut usize) {
            match scene {
                Scene::Container(c) => {
                    if c.tag.as_deref().is_some_and(|t| t.starts_with("lazylist#")) {
                        *n += 1;
                    }
                    for child in &c.children {
                        walk(child, n);
                    }
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), n),
                _ => {}
            }
        }
        let mut n = 0;
        walk(scene, &mut n);
        n
    }

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_container(ch, tag)),
            Scene::Scroll(s) => find_container(s.content.as_ref(), tag),
            _ => None,
        }
    }

    fn row_text_of(scene: &Scene, index: usize) -> Option<String> {
        let c = find_container(scene, &format!("{LIST_TAG}#{index}"))?;
        c.children.iter().find_map(|ch| match ch {
            Scene::Text(t) => Some(t.content.clone()),
            _ => None,
        })
    }

    fn status_text(scene: &Scene) -> Option<String> {
        let c = find_container(scene, STATUS_TAG)?;
        c.children.iter().find_map(|ch| match ch {
            Scene::Text(t) => Some(t.content.clone()),
            _ => None,
        })
    }

    #[test]
    fn r924_renders_a_small_window_not_the_whole_dataset() {
        let owner = Owner::new();
        let rendered = owner.run(|| {
            boot();
            drain_pump();
            count_row_tags(&view((), &Frame::default()))
        });
        assert!(rendered < 30, "virtualized: small window, got {rendered} of {N}");
        assert!(rendered >= 12, "must cover the 12-row viewport");
    }

    #[test]
    fn r924_boot_loads_first_page_through_pump() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            boot();
            // Before the pump runs, page 0 is in flight → row 0 is a skeleton.
            let loading = view((), &Frame::default());
            assert_eq!(row_text_of(&loading, 0).as_deref(), Some("Loading\u{2026}"));
            drain_pump();
            view((), &Frame::default())
        });
        // After the pump drains, row 0 shows real data.
        assert_eq!(
            row_text_of(&scene, 0).as_deref(),
            Some("asset_00000.png (Texture, 19 KB)"),
        );
        assert_eq!(status_text(&scene).as_deref(), Some(format!("Rows 1\u{2013}16 of {N}").as_str()));
    }

    #[test]
    fn r924_only_visible_pages_are_fetched() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            // Boot window touches page 0 only (rows 0..15) → exactly 1 resident.
            assert_eq!(page_cache().borrow().len(), 1, "only the visible page fetched");
            assert!(page_cache().borrow().contains_key(&0));
            assert!(!page_cache().borrow().contains_key(&50), "far pages not fetched");
        });
    }

    #[test]
    fn r924_scroll_fetches_new_band_and_advances_window() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            // The real layout pass writes the scroll bound (sizer height) into
            // `ScrollState::max_y`; a direct `view()` call skips layout, so set
            // a bound here or `scroll_to` clamps to 0.
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_PITCH).unwrap());
            // Scroll far down to row ~5000 (page 50). The Effect (subscribed to
            // the scroll offset) fetches the newly-visible page.
            scroll.scroll_to(0, 5000 * i32::try_from(ROW_PITCH).unwrap());
            // Mid-flight: the new page is Loading.
            assert!(page_cache().borrow().contains_key(&50), "page 50 requested on scroll");
            let loading_scene = view((), &Frame::default());
            assert_eq!(row_text_of(&loading_scene, 5000).as_deref(), Some("Loading\u{2026}"));
            drain_pump();
            let scene = view((), &Frame::default());
            // i=5000 → KINDS[5000 % 6 = 2] = Audio/wav; size=(5000*37+11)%900+8=519.
            assert_eq!(
                row_text_of(&scene, 5000).as_deref(),
                Some("asset_05000.wav (Audio, 519 KB)"),
            );
            // Window advanced: top-of-dataset rows are no longer rendered.
            assert!(row_text_of(&scene, 0).is_none(), "row 0 outside the window after scroll");
        });
    }

    #[test]
    fn r924_error_page_renders_unavailable() {
        // A page whose fetch resolves to Err renders the error placeholder.
        let owner = Owner::new();
        owner.run(|| {
            let cache = page_cache();
            let res: Rc<Resource<Vec<AssetRow>, String>> = Rc::new(Resource::loading());
            res.fetch_with(
                &*use_local_task_pump(),
                DeferredReady::new(0, Err::<Vec<AssetRow>, String>("boom".to_owned())),
            );
            cache.borrow_mut().insert(0, res);
            drain_pump();
            let states = resolve_visible_pages(
                &VisibleWindow { first: 0, count: 4 },
                &cache,
            );
            // Row 0 (page 0) is in the Error arm → "Unavailable".
            let scene = build_row(0, &states, &use_theme(THEME_TAG).theme_animated());
            assert_eq!(
                match &scene {
                    Scene::Container(c) => c.children.iter().find_map(|ch| match ch {
                        Scene::Text(t) => Some(t.content.clone()),
                        _ => None,
                    }),
                    _ => None,
                }
                .as_deref(),
                Some("Unavailable"),
            );
        });
    }

    #[test]
    fn r924_a11y_full_setsize_with_windowed_items() {
        use pinion_a11y::AriaRole;
        let owner = Owner::new();
        let nodes = owner.run(|| {
            boot();
            drain_pump();
            LazyListView::access_node(&(), None)
        });
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(nodes[0].size_of_set, Some(u32::try_from(N).unwrap()), "aria-setsize=N");
        assert!(nodes.len() - 1 < 30, "only the rendered window has listitems");
        assert_eq!(nodes[1].position_in_set, Some(1), "top window starts at posinset 1");
        for item in &nodes[1..] {
            assert_eq!(item.role, AriaRole::ListItem);
            assert_eq!(item.size_of_set, Some(u32::try_from(N).unwrap()));
        }
    }

    #[test]
    fn r924_page_rows_are_indexed_and_stable() {
        let rows = page_rows(0);
        assert_eq!(rows.len(), PAGE_SIZE);
        assert_eq!(row_label(&rows[0]), "asset_00000.png (Texture, 19 KB)");
        let rows50 = page_rows(50);
        assert_eq!(rows50[0].name, "asset_05000.wav");
    }
}
