//! `hello-million-row` — R934 §5.22 §5.23 §5.27: **unbounded million-row
//! virtualized lazy-load**. The Model/View-at-scale backbone the self-hosted
//! editor's asset browser / scene outliner needs when the store is *truly*
//! unbounded: 1,000,000 rows, windowed *and* fetched a page at a time, with a
//! page cache that stays bounded no matter how deep you scroll.
//!
//! ## What this is (R924 lazy-load × R934 LRU eviction)
//!
//! `hello-lazy-list` (R924) virtualizes + lazy-loads a 10,000-row source whose
//! key space is *bounded* (100 pages), so its [`ResourceCache`] retains every
//! fetched page for the app lifetime. That does not scale to a million-asset
//! store (10,000 pages): retaining every page you ever scrolled past grows
//! memory without bound. R934 backs the page cache with
//! [`ResourceCache::with_capacity`](pinion_core::ResourceCache::with_capacity)
//! — an **LRU-bounded** cache of [`CACHE_PAGES`] resident slices. The visible
//! window is touched (ensured + snapshotted) every frame, so it stays
//! most-recently-used and resident; pages that scroll far away become
//! least-recently-used and are **evicted**. Scroll back to an evicted page and
//! it re-fetches — skeleton, then data — exactly as it did the first time.
//!
//! So the rendered node count stays small (virtualization), *and* the resident
//! page count stays small (LRU eviction): memory is flat whether you are at row
//! 0 or row 999,999.
//!
//! ## Architecture (unidirectional, Effect-driven prefetch)
//!
//! - A per-page `Resource` cache — the [`ResourceCache`](pinion_core::ResourceCache)
//!   keyed-async-carrier substrate, here built **bounded** via
//!   [`with_capacity`](pinion_core::ResourceCache::with_capacity) — in
//!   `Owner::cache`. A page is materialised only when it first scrolls into
//!   view; resident pages are capped at [`CACHE_PAGES`], the least-recently-used
//!   being evicted when a new page is fetched past the cap.
//! - An [`Effect`] subscribed to the **scroll offset** [`Signal`] computes the
//!   visible page range and kicks off a [`Resource::fetch_with`] for every page
//!   not yet in the cache, through the shell-polled
//!   [`LocalTaskPump`](pinion_core::LocalTaskPump). Re-running it on every
//!   scroll also promotes the visible pages (ensure-on-hit), keeping them hot.
//! - The view is **pure** (§6.3): it reads `scroll.offset_y()` + each visible
//!   page's `resource.state()` (which promotes them) and maps `(loaded rows |
//!   skeletons)` into the windowed scene. No fetching in the view.
//!
//! ## Capacity sizing
//!
//! The visible window spans at most 2 pages (12 rows + overscan ≈ 20 rows of a
//! 100-row page); [`CACHE_PAGES`] = 4 leaves two pages of headroom so the
//! window is never the eviction victim (the cache-thrash the substrate's
//! *Capacity invariant* warns of). Small enough that a few far jumps evict the
//! pages left behind — making the bound observable.
//!
//! ## Latency model (ZERO-FLAKE)
//!
//! Each page fetch is a deterministic [`DeferredReady`] future (`Pending`
//! `FETCH_LATENCY_POLLS` times, then `Ready`). Each `scene/snapshot from=paint`
//! advances the pump one step, so the demo's own polling drives a freshly
//! scrolled-to (or re-fetched-after-eviction) page from skeleton → rows with no
//! wall-clock race.
//!
//! ## AI-first witness (§2 #7)
//!
//! `scene/snapshot` reports ~16 `millionrow#<i>` row nodes (loaded descriptor
//! or "Loading…"), a `role=status` band line, and a `million_cacheinfo` line
//! reporting "Resident pages: k/4" — the LRU bound as queryable data.
//! `aria-setsize` on the list stays 1,000,000. Scroll deep, then back: a page
//! that has been evicted re-appears as a skeleton before resolving — the
//! observable signature of eviction. See `tools/demos/r934_million_row.py`.

use pinion_a11y::{AccessNode, WidgetA11y, windowed_list_nodes};
use pinion_core::external::{External, StubExternal};
use pinion_core::reactive::{DeferredReady, Effect, Owner, ResourceCache, ResourceState};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::{VisibleWindow, compute_visible_range, pages_in_window};
use pinion_core::{Frame, LocalTaskPump, Scene, WidgetCore, use_local_task_pump};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::scrollbar::{VerticalScrollbarStyle, view_vertical_scrollbar};
use pinion_widget_paint::virtual_list::view_virtual_list;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloMillionRowRenderer, HelloMillionRowRendererError);

const WIN_W: u32 = 380;
const WIN_H: u32 = 520;
const THEME_TAG: &str = "app";

/// Total dataset size — a *truly unbounded* store stand-in: a million rows,
/// 10,000 pages. Both the rendered node count and the resident page count stay
/// small.
const N: usize = 1_000_000;
/// Rows fetched per async page. `N % PAGE_SIZE == 0` so every page is full.
const PAGE_SIZE: usize = 100;
/// Maximum resident pages — the LRU bound. Two pages of headroom over the
/// (≤2-page) visible window so the window is never the eviction victim.
const CACHE_PAGES: NonZeroUsize = NonZeroUsize::new(4).expect("4 is non-zero");
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
const LIST_TAG: &str = "millionrow";
/// `role=status` band line tag.
const STATUS_TAG: &str = "million_status";
/// Resident-page-count witness line tag (the LRU bound as queryable data).
const CACHEINFO_TAG: &str = "million_cacheinfo";
/// Cache key for the scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "million_scroll";
/// Paint + state tag for the interactive scrollbar peer.
const SCROLLBAR_TAG: &str = "million_scrollbar";
/// `Owner::cache` key for the per-page `Resource` cache.
const PAGE_CACHE_KEY: &str = "million.page_cache";
/// `Owner::cache` key for the lifetime-held scroll-driven prefetch `Effect`.
const LOADER_KEY: &str = "million.loader";

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
                name: format!("asset_{i:07}.{ext}"),
                kind: kind.to_owned(),
                size_kb: u32::try_from(size).unwrap_or(0),
            }
        })
        .collect()
}

// ─── per-page LRU-bounded Resource cache + scroll-driven prefetch Effect ───

/// Per-page async carrier cache, **LRU-bounded** to [`CACHE_PAGES`] resident
/// slices (R934). Keyed by page index; only fetched pages are resident, and
/// far-away pages are evicted as new ones load. The keyed-async-carrier
/// substrate (`pinion_core::ResourceCache`) owns the idempotent get-or-fetch +
/// LRU eviction + state snapshot; each page's `Resource` carries its own
/// reactive state, and the view subscribes to the visible ones.
type PageCache = ResourceCache<usize, Vec<AssetRow>, String>;

fn page_cache() -> Rc<PageCache> {
    Owner::current()
        .expect("page_cache() requires an active Owner scope")
        .cache(PAGE_CACHE_KEY, || PageCache::with_capacity(CACHE_PAGES))
}

/// Ensure every page the window touches has a `Resource` in flight: create +
/// `fetch_with` the missing ones, promoting any already-resident ones. Owner-
/// scoped side effect, run only from the prefetch [`Effect`] (never the view).
fn ensure_pages_loaded(offset_y: i32, cache: &PageCache, pump: &LocalTaskPump) {
    let window = compute_visible_range(offset_y, VIEWPORT_H, N, ROW_PITCH, OVERSCAN);
    for page in pages_in_window(&window, PAGE_SIZE) {
        cache.ensure(page, pump, || fetch_page(page));
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
/// each `Resource::state()` here subscribes the view to it (so a page
/// resolution re-renders) and promotes it to most-recently-used — which is what
/// keeps the visible window resident under LRU eviction.
type PageStates = HashMap<usize, ResourceState<Vec<AssetRow>, String>>;

fn resolve_visible_pages(window: &VisibleWindow, cache: &PageCache) -> PageStates {
    cache.snapshot(pages_in_window(window, PAGE_SIZE))
}

/// The `role=status` band line — the SSOT for the visible-band text + the live
/// region's accessible name.
fn status_line(window: &VisibleWindow, page_states: &PageStates) -> String {
    // "Loading" if ANY visible page is not yet `Ready` — the window can
    // straddle two pages, and a skeleton anywhere in the band means the band
    // is still loading (not just the top page).
    let loading = pages_in_window(window, PAGE_SIZE)
        .any(|p| !matches!(page_states.get(&p), Some(ResourceState::Ready(_))));
    let last = window.first + window.count.saturating_sub(1);
    if loading {
        format!(
            "Loading rows {}\u{2013}{}\u{2026}",
            window.first + 1,
            last + 1
        )
    } else {
        format!("Rows {}\u{2013}{} of {N}", window.first + 1, last + 1)
    }
}

/// The resident-page-count witness line — the LRU bound surfaced as queryable
/// scene data (§2 #7). Reading `len()` is a plain (non-reactive) probe; it is
/// re-evaluated whenever the view re-runs (every scroll), which is exactly when
/// eviction happens, so it tracks the bound in practice.
fn cache_info_line(cache: &PageCache) -> String {
    format!("Resident pages: {}/{}", cache.len(), CACHE_PAGES.get())
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
        // Loading, or not-yet-requested / evicted (None) → skeleton placeholder.
        _ => ("Loading\u{2026}".to_owned(), ColorRole::OnSurfaceMuted),
    };
    Scene::Text(TextNode::styled(
        content,
        Rect::default(),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(role)),
    ))
}

/// One virtualized row, tagged `millionrow#<index>` so the windowed-list a11y
/// nodes + `enrich_names_from_scene` attach. Renders the loaded descriptor or
/// a skeleton, depending on whether the row's page is currently resident.
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
/// visible page's `Resource` state; the dataset is virtual *and* lazily fetched
/// *and* LRU-bounded.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let cache = page_cache();

    // Snapshot the visible pages' states once (1-2 pages) — subscribes the view
    // to each + promotes them, avoids a per-row page-Vec clone.
    let window = compute_visible_range(scroll.offset_y(), VIEWPORT_H, N, ROW_PITCH, OVERSCAN);
    let page_states = resolve_visible_pages(&window, &cache);

    let title = Scene::Text(TextNode::styled(
        "Asset store (1,000,000 rows, LRU-bounded page cache)",
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

    let cache_info = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            cache_info_line(&cache),
            Rect::default(),
            TextStyle::new()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(CACHEINFO_TAG)
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
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    let list_root = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_tag(LIST_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    Scene::Container(
        ContainerNode::new(vec![title, status, cache_info, list_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_gap(10),
            ),
    )
}

struct MillionRowView;

impl WidgetCore for MillionRowView {
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
        vec![scrollbar_extra_external(
            use_scroll_state(SCROLL_KEY),
            SCROLLBAR_TAG,
        )]
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

    fn title() -> &'static str {
        "pinion hello-million-row (R934 §5.22 §5.23 §5.27)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (no widget state)".to_string()
    }
}

impl WidgetA11y for MillionRowView {
    /// WAI-ARIA virtualized `list`: `aria-setsize = N`, one `listitem` per
    /// rendered row with its absolute `aria-posinset`. The windowed topology is
    /// the shared `pinion_a11y::windowed_list_nodes` SSOT; a skeleton row
    /// (loading, or evicted-and-reloading) is still item `k` of `N` to AT —
    /// load state is conveyed by the rendered text, not the set position.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let window = compute_visible_range(scroll.offset_y(), VIEWPORT_H, N, ROW_PITCH, OVERSCAN);
        windowed_list_nodes(
            LIST_TAG,
            "Asset store",
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
        )
    }
}

impl WidgetView for MillionRowView {
    type Renderer = HelloMillionRowRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<MillionRowView>();
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

    /// Set the scroll bound the real layout pass would write, so `scroll_to`
    /// does not clamp to 0 in a direct (layout-skipping) test call.
    fn set_scroll_bound() {
        use_scroll_state(SCROLL_KEY).set_max(
            0,
            i32::try_from(N).unwrap() * i32::try_from(ROW_PITCH).unwrap(),
        );
    }

    /// Scroll so `row` is at the window top, then drain the pump.
    fn scroll_to_row(row: usize) {
        use_scroll_state(SCROLL_KEY).scroll_to(
            0,
            i32::try_from(row).unwrap() * i32::try_from(ROW_PITCH).unwrap(),
        );
        drain_pump();
    }

    /// Count `millionrow#<i>` row containers anywhere in the scene.
    fn count_row_tags(scene: &Scene) -> usize {
        fn walk(scene: &Scene, n: &mut usize) {
            match scene {
                Scene::Container(c) => {
                    if c.tag
                        .as_deref()
                        .is_some_and(|t| t.starts_with("millionrow#"))
                    {
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

    fn text_of(scene: &Scene, tag: &str) -> Option<String> {
        let c = find_container(scene, tag)?;
        c.children.iter().find_map(|ch| match ch {
            Scene::Text(t) => Some(t.content.clone()),
            _ => None,
        })
    }

    fn row_text_of(scene: &Scene, index: usize) -> Option<String> {
        text_of(scene, &format!("{LIST_TAG}#{index}"))
    }

    #[test]
    fn r934_renders_a_small_window_not_the_whole_dataset() {
        let owner = Owner::new();
        let rendered = owner.run(|| {
            boot();
            drain_pump();
            count_row_tags(&view((), &Frame::default()))
        });
        assert!(
            rendered < 30,
            "virtualized: small window, got {rendered} of {N}"
        );
        assert!(rendered >= 12, "must cover the 12-row viewport");
    }

    #[test]
    fn r934_boot_loads_first_page_through_pump() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            boot();
            // Before the pump runs, page 0 is in flight → row 0 is a skeleton.
            let loading = view((), &Frame::default());
            assert_eq!(row_text_of(&loading, 0).as_deref(), Some("Loading\u{2026}"));
            drain_pump();
            view((), &Frame::default())
        });
        assert_eq!(
            row_text_of(&scene, 0).as_deref(),
            Some("asset_0000000.png (Texture, 19 KB)"),
        );
        assert_eq!(
            text_of(&scene, STATUS_TAG).as_deref(),
            Some(format!("Rows 1\u{2013}16 of {N}").as_str())
        );
    }

    #[test]
    fn r934_cache_is_lru_bounded() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            assert_eq!(
                page_cache().capacity(),
                Some(CACHE_PAGES),
                "page cache is bounded"
            );
            set_scroll_bound();
            // Scroll through many far bands — far more distinct pages than the cap.
            for row in [150_000, 300_000, 450_000, 600_000, 750_000, 900_000] {
                scroll_to_row(row);
            }
            assert!(
                page_cache().len() <= CACHE_PAGES.get(),
                "resident pages stay bounded: {} <= {}",
                page_cache().len(),
                CACHE_PAGES.get(),
            );
        });
    }

    #[test]
    fn r934_far_scrolling_evicts_early_pages() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            assert!(page_cache().contains(&0), "page 0 resident after boot");
            set_scroll_bound();
            // Jump to three distinct far regions; each touches 1-2 fresh pages,
            // pushing page 0 (untouched since boot) out of a 4-page cache.
            scroll_to_row(300_000);
            scroll_to_row(600_000);
            scroll_to_row(900_000);
            assert!(
                !page_cache().contains(&0),
                "page 0 evicted after far scrolling"
            );
        });
    }

    #[test]
    fn r934_scroll_back_to_evicted_page_refetches() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            set_scroll_bound();
            scroll_to_row(300_000);
            scroll_to_row(600_000);
            scroll_to_row(900_000);
            assert!(!page_cache().contains(&0), "precondition: page 0 evicted");
            // Scroll back to the top: page 0 is re-requested (a fresh fetch).
            use_scroll_state(SCROLL_KEY).scroll_to(0, 0);
            assert!(
                page_cache().contains(&0),
                "page 0 re-requested on scroll-back"
            );
            assert_eq!(
                page_cache().state(&0),
                Some(ResourceState::Loading),
                "the evicted page re-fetches → skeleton again",
            );
            // Row 0 renders as a skeleton mid-refetch, then resolves to data.
            let reloading = view((), &Frame::default());
            assert_eq!(
                row_text_of(&reloading, 0).as_deref(),
                Some("Loading\u{2026}")
            );
            drain_pump();
            let reloaded = view((), &Frame::default());
            assert_eq!(
                row_text_of(&reloaded, 0).as_deref(),
                Some("asset_0000000.png (Texture, 19 KB)"),
            );
        });
    }

    #[test]
    fn r934_cache_info_line_reports_the_bound() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            boot();
            drain_pump();
            view((), &Frame::default())
        });
        // Boot window touches page 0 only → exactly 1 resident, bound 4.
        assert_eq!(
            text_of(&scene, CACHEINFO_TAG).as_deref(),
            Some("Resident pages: 1/4")
        );
    }

    #[test]
    fn r934_deep_scroll_loads_band_and_advances_window() {
        let owner = Owner::new();
        owner.run(|| {
            boot();
            drain_pump();
            set_scroll_bound();
            use_scroll_state(SCROLL_KEY).scroll_to(0, 500_000 * i32::try_from(ROW_PITCH).unwrap());
            // Mid-flight: the new page (5000) is Loading.
            assert!(
                page_cache().contains(&5000),
                "page 5000 requested on scroll"
            );
            let loading_scene = view((), &Frame::default());
            assert_eq!(
                row_text_of(&loading_scene, 500_000).as_deref(),
                Some("Loading\u{2026}")
            );
            drain_pump();
            let scene = view((), &Frame::default());
            // i=500000 → KINDS[500000 % 6 = 2] = Audio/wav; size=(500000*37+11)%900+8=519.
            assert_eq!(
                row_text_of(&scene, 500_000).as_deref(),
                Some("asset_0500000.wav (Audio, 519 KB)"),
            );
            assert!(
                row_text_of(&scene, 0).is_none(),
                "row 0 outside the window after scroll"
            );
        });
    }

    #[test]
    fn r934_a11y_full_setsize_with_windowed_items() {
        use pinion_a11y::AriaRole;
        let owner = Owner::new();
        let nodes = owner.run(|| {
            boot();
            drain_pump();
            MillionRowView::access_node(&(), None)
        });
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(
            nodes[0].size_of_set,
            Some(u32::try_from(N).unwrap()),
            "aria-setsize=N"
        );
        assert!(
            nodes.len() - 1 < 30,
            "only the rendered window has listitems"
        );
        assert_eq!(
            nodes[1].position_in_set,
            Some(1),
            "top window starts at posinset 1"
        );
        for item in &nodes[1..] {
            assert_eq!(item.role, AriaRole::ListItem);
            assert_eq!(item.size_of_set, Some(u32::try_from(N).unwrap()));
        }
    }

    #[test]
    fn r934_page_rows_are_indexed_and_stable() {
        let rows = page_rows(0);
        assert_eq!(rows.len(), PAGE_SIZE);
        assert_eq!(row_label(&rows[0]), "asset_0000000.png (Texture, 19 KB)");
        let rows5000 = page_rows(5000);
        assert_eq!(rows5000[0].name, "asset_0500000.wav");
    }
}
