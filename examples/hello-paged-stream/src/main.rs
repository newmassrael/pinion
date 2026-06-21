//! `hello-paged-stream` — R1005 §5.22 §5.23 §5.27: **paged out-of-memory
//! streaming** (Model/View-at-scale slice ①b). The combination
//! `hello-streaming-log` (R996, slice ①a) named as its successor: a log whose
//! row count **grows at runtime** *and* whose rows are **paged behind an
//! LRU-bounded async cache**, so memory stays flat while the stream is
//! unbounded — a GB DLT log, a Wireshark capture, a `tail -f` on a file too
//! large to hold in RAM.
//!
//! ## The two halves it merges
//!
//! - **Streaming-append** (R996): the total row count is a runtime-growable
//!   [`Signal<usize>`]; an Emit batch bumps it, and the windowing math is
//!   count-parametric so the window + scroll extent grow with no new substrate.
//!   Tail-follow is **stateless** — the R1005-lifted [`at_bottom`] /
//!   [`follow_tail`] windowing substrate (this is the 2nd consumer R996
//!   anticipated, so they moved into `pinion-core`).
//! - **Paged LRU** (R934): rows are **not** held in memory — only the count is.
//!   Each page is fetched on demand into a [`ResourceCache::with_capacity`]
//!   bounded to [`CACHE_PAGES`] resident slices; pages that scroll away evict.
//!   So both the rendered node count (virtualization) and the resident page
//!   count (LRU) stay small whether the stream is 50 rows or 5,000,000.
//!
//! ## The new substrate this slice needs (`ResourceCache::invalidate`)
//!
//! The streaming twist a fixed-count paged source never faces: the **last page
//! is partial**, and the stream appends *onto it*. Its cached slice is then
//! stale. R1005 adds [`ResourceCache::invalidate`] — drop a page so the next
//! `ensure` re-fetches it. On Emit the reducer invalidates the partial tail
//! page **before** bumping the count; the count change re-runs the prefetch
//! `Effect`, which re-fetches that page (now larger) and any brand-new pages.
//! Re-keying by an embedded version would leak a dead carrier per append;
//! in-place invalidation re-fetches the same key.
//!
//! ## AI-first witness (§2 #7)
//!
//! `scene/snapshot` reports a small window of `pagedstream#<i>` rows (loaded or
//! "Loading…"), a `role=status` line (live count + Following/Paused), and a
//! `pagedstream_cacheinfo` line ("Resident pages: k/4") — the out-of-memory
//! bound as queryable data. `scene/click pagedstream_emit` appends a batch;
//! the partial tail page re-fetches with the grown rows. See
//! `tools/demos/r1005_paged_stream.py`.

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y, windowed_list_nodes};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::intent::Intent;
use pinion_core::reactive::{DeferredReady, Effect, Owner, ResourceCache, ResourceState, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::widgets::scroll::{ScrollState, use_scroll_state};
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::{
    at_bottom, compute_visible_range, follow_tail, pages_in_window,
};
use pinion_core::{
    Command, Frame, LocalTaskPump, Scene, WidgetCore, WidgetStateName, use_local_task_pump,
};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::scrollbar::{VerticalScrollbarStyle, view_vertical_scrollbar};
use pinion_widget_paint::virtual_list::view_virtual_list;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloPagedStreamRenderer, HelloPagedStreamRendererError);

const WIN_W: u32 = 440;
const WIN_H: u32 = 540;
const THEME_TAG: &str = "app";

/// Rows per async page. Smaller than [`BATCH`] so an Emit fills the partial tail
/// page *and* spills into a fresh page — exercising both the tail-page
/// invalidation and a brand-new-page fetch.
const PAGE_SIZE: usize = 64;
/// Maximum resident pages — the LRU bound. Headroom over the (≤2-page) visible
/// window so the window is never the eviction victim (the *Capacity invariant*).
const CACHE_PAGES: NonZeroUsize = NonZeroUsize::new(4).expect("4 is non-zero");
/// Lines appended per Emit — one batch arriving from the producer.
const BATCH: usize = 50;
/// Uniform per-row vertical slot. Uniform pitch → exact window math.
const ROW_PITCH: u32 = 24;
/// Rows built above + below the strict visible window.
const OVERSCAN: usize = 4;
/// Scroll viewport width.
const VIEWPORT_W: u32 = 400;
/// Scroll viewport height — exactly 14 rows tall.
const VIEWPORT_H: u32 = 14 * ROW_PITCH;
/// `Pending` polls before a page fetch resolves — a deterministic source-latency
/// stand-in keeping the skeleton observable across frames (ZERO-FLAKE; each
/// `scene/snapshot from=paint` advances the pump one step).
const FETCH_LATENCY_POLLS: u32 = 16;

/// Paint-root + a11y `list` container tag, and the `pagedstream#<i>` row prefix.
const LIST_TAG: &str = "pagedstream";
/// `role=status` line tag (live count + Following/Paused).
const STATUS_TAG: &str = "pagedstream_status";
/// Resident-page-count witness line tag (the LRU bound as queryable data).
const CACHEINFO_TAG: &str = "pagedstream_cacheinfo";
/// The Emit button's paint + External tag (the §5.20 routing handle).
const EMIT_TAG: &str = "pagedstream_emit";
/// Cache key for the scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "pagedstream_scroll";
/// Paint + state tag for the interactive scrollbar peer.
const SCROLLBAR_TAG: &str = "pagedstream_scrollbar";
/// `Owner::cache` key for the shared streaming source (the count only).
const SOURCE_KEY: &str = "pagedstream.source";
/// `Owner::cache` key for the per-page LRU-bounded `Resource` cache.
const PAGE_CACHE_KEY: &str = "pagedstream.page_cache";
/// `Owner::cache` key for the lifetime-held prefetch `Effect`.
const LOADER_KEY: &str = "pagedstream.loader";

/// Fully-prefixed wire tag for the Emit button's `"click"` intent (§5.20 R22).
const EMIT_INTENT_TAG_FULL: &str = pinion_core::intent_tag!("pagedstream_emit", "click");

/// Log levels rotated per line — cosmetic, keeps the synthesized lines readable.
const LEVELS: [&str; 4] = ["INFO ", "WARN ", "ERROR", "DEBUG"];

// ─── streaming source: the COUNT only (rows are paged / out-of-memory) ──────

/// The producer's streaming source. Unlike `hello-streaming-log` (which holds
/// every line in a `Vec`), this holds **only the reactive row count** — the
/// rows themselves are synthesized a page at a time by [`fetch_page`] and never
/// all resident, the out-of-memory contract. The reducer bumps the count, the
/// view + a11y + prefetch `Effect` read it, all sharing one `Rc` via
/// [`use_source`].
struct StreamSource {
    count: Signal<usize>,
}

impl StreamSource {
    fn new() -> Self {
        Self {
            count: Signal::new(0),
        }
    }

    /// Total rows produced so far (reactive read).
    fn count(&self) -> usize {
        self.count.get()
    }

    /// Append `n` rows by advancing the count — the one observable change the
    /// view + prefetch `Effect` subscribe to. The rows are *not* materialized
    /// here; they are synthesized on demand when their page is fetched.
    fn emit(&self, n: usize) {
        self.count.set(self.count.get() + n);
    }
}

fn use_source() -> Rc<StreamSource> {
    Owner::current()
        .expect("use_source() requires an active Owner scope")
        .cache(SOURCE_KEY, StreamSource::new)
}

/// The page index holding source row `row`.
fn page_of(row: usize) -> usize {
    row / PAGE_SIZE
}

// ─── scripted out-of-memory paged source ───────────────────────────────────

/// One synthesized log line — deterministic, stable for a given index, so a
/// re-fetched page reproduces the same rows.
fn synth_line(i: usize) -> String {
    let level = LEVELS[i % LEVELS.len()];
    format!("[{i:06}] {level} worker: event {i} processed")
}

/// The rows resident on `page` given the current total `count`. The **last
/// page is partial**: it holds only `[page*PAGE_SIZE, count)`, which is why it
/// must be re-fetched (invalidated) when the stream appends onto it.
fn page_lines(page: usize, count: usize) -> Vec<String> {
    let base = page * PAGE_SIZE;
    let end = ((page + 1) * PAGE_SIZE).min(count);
    (base..end).map(synth_line).collect()
}

/// Fetch `page` behind a deterministic latency ([`DeferredReady`]), snapshotting
/// the `count` at fetch time so the partial tail page reflects the rows present
/// when it was requested.
fn fetch_page(page: usize, count: usize) -> DeferredReady<Result<Vec<String>, String>> {
    DeferredReady::new(FETCH_LATENCY_POLLS, Ok(page_lines(page, count)))
}

// ─── per-page LRU cache + count/scroll-driven prefetch Effect ───────────────

type PageCache = ResourceCache<usize, Vec<String>, String>;

fn page_cache() -> Rc<PageCache> {
    Owner::current()
        .expect("page_cache() requires an active Owner scope")
        .cache(PAGE_CACHE_KEY, || PageCache::with_capacity(CACHE_PAGES))
}

/// Ensure every page the visible window touches has a fetch in flight (or is
/// resident), for the current `count`. Owner-scoped side effect, run only from
/// the prefetch `Effect` (never the view).
fn ensure_pages_loaded(offset_y: i32, count: usize, cache: &PageCache, pump: &LocalTaskPump) {
    let window = compute_visible_range(offset_y, VIEWPORT_H, count, ROW_PITCH, OVERSCAN);
    for page in pages_in_window(&window, PAGE_SIZE) {
        cache.ensure(page, pump, || fetch_page(page, count));
    }
}

/// Lifetime marker holding the prefetch [`Effect`] (R665).
struct LoaderMarker {
    _effect: Effect,
}

/// Install the prefetch `Effect` once. It subscribes to the scroll offset **and
/// the row count**, so it re-fetches when the user scrolls *or* the stream
/// grows — the latter is what re-fetches the invalidated tail page and the
/// brand-new pages after an Emit.
fn install_loader() -> Rc<LoaderMarker> {
    let owner = Owner::current().expect("install_loader() requires an active Owner scope");
    let scroll = use_scroll_state(SCROLL_KEY);
    let src = use_source();
    let cache = page_cache();
    let pump = use_local_task_pump();
    let owner_for_effect = owner.clone();
    owner.cache(LOADER_KEY, move || {
        let scroll_e = scroll.clone();
        let src_e = src.clone();
        let cache_e = cache.clone();
        let pump_e = pump.clone();
        let effect = Effect::new(&owner_for_effect, move || {
            let offset = scroll_e.offset_y(); // subscribe to scroll
            let count = src_e.count(); // subscribe to the growing count
            ensure_pages_loaded(offset, count, &cache_e, &pump_e);
        });
        LoaderMarker { _effect: effect }
    })
}

// ─── streaming append + stateless tail-follow ──────────────────────────────

/// Append one batch and apply tail-follow. The streaming-tail-page step (the
/// slice ①b twist over ①a): **before** the count grows, invalidate the partial
/// tail page so the count-change re-fetches it with the appended rows; then the
/// shared [`follow_tail`] reducer shape grows the bound + pins to the new
/// bottom when following.
fn apply_emit(src: &StreamSource, scroll: &ScrollState, cache: &PageCache) {
    let measured_h = scroll.measured_viewport().1;
    let viewport_h = if measured_h == 0 {
        VIEWPORT_H
    } else {
        measured_h
    };
    let was_following = at_bottom(scroll.offset_y(), scroll.max().1);

    // The page holding the last row gains rows iff it was partial; a full page
    // is unchanged, and brand-new pages are absent so `ensure` fetches them
    // fresh. Invalidate the partial tail BEFORE bumping the count, so the
    // count-change re-run of the prefetch Effect re-fetches it.
    let old_count = src.count();
    if old_count > 0 && old_count % PAGE_SIZE != 0 {
        cache.invalidate(&page_of(old_count - 1));
    }
    src.emit(BATCH);
    follow_tail(scroll, src.count(), ROW_PITCH, viewport_h, was_following);
}

// ─── view ──────────────────────────────────────────────────────────────────

type PageStates = HashMap<usize, ResourceState<Vec<String>, String>>;

fn zebra_fill(index: usize, theme: &Theme) -> pinion_core::style::Color {
    if index % 2 == 0 {
        theme.resolve(ColorRole::SurfaceContainerLow)
    } else {
        theme.resolve(ColorRole::SurfaceContainer)
    }
}

/// One row's content, by its page's resolved state: the loaded line, or a
/// skeleton when the page is loading / not-yet-fetched / evicted.
fn row_content(page_states: &PageStates, index: usize) -> (String, ColorRole) {
    let page = page_of(index);
    let off = index % PAGE_SIZE;
    match page_states.get(&page) {
        Some(ResourceState::Ready(rows)) => match rows.get(off) {
            Some(line) => (line.clone(), ColorRole::OnSurface),
            None => ("\u{2026}".to_owned(), ColorRole::OnSurfaceMuted),
        },
        Some(ResourceState::Error(_)) => ("Unavailable".to_owned(), ColorRole::Error),
        _ => ("Loading\u{2026}".to_owned(), ColorRole::OnSurfaceMuted),
    }
}

/// One virtualized row, tagged `pagedstream#<index>` so the windowed-list a11y
/// nodes + `enrich_names_from_scene` attach.
fn build_row(index: usize, page_states: &PageStates, theme: &Theme) -> Scene {
    let (content, role) = row_content(page_states, index);
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            content,
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(role)),
        ))])
        .with_tag(format!("{LIST_TAG}#{index}"))
        .with_style(BoxStyle::filled(zebra_fill(index, theme)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(VIEWPORT_W, ROW_PITCH))
                .with_padding(Rect::new(10, 0, 10, 0)),
        ),
    )
}

/// The `role=status` line text — the live count + derived follow state.
fn status_line(count: usize, following: bool) -> String {
    if count == 0 {
        return "0 lines \u{00b7} click Emit to stream".to_owned();
    }
    let mode = if following {
        "Following (newest)"
    } else {
        "Paused (scroll to bottom to follow)"
    };
    format!("{count} lines \u{00b7} {mode}")
}

/// The resident-page-count witness line — the LRU bound as queryable scene data
/// (§2 #7). The out-of-memory proof: this stays ≤ [`CACHE_PAGES`] however large
/// the stream grows.
fn cache_info_line(cache: &PageCache) -> String {
    format!("Resident pages: {}/{}", cache.len(), CACHE_PAGES.get())
}

fn emit_button(state: ButtonState, theme: &Theme) -> Scene {
    let base = theme.resolve(ColorRole::Accent);
    let fill = match state {
        ButtonState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
        ButtonState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), 0.08),
        ButtonState::Disabled => base.lerp(theme.resolve(ColorRole::Surface), 0.5),
        ButtonState::Idle => base,
    };
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            format!("Emit {BATCH} lines"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnAccent)),
        ))])
        .with_tag(EMIT_TAG)
        .with_aria_label("Emit log lines")
        .with_style(BoxStyle::filled(fill).with_corner_radius(8))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(160, 36))
                .with_focusable(true),
        ),
    )
}

fn centered_text(content: String, tag: &str, size: u32, role: ColorRole, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            content,
            Rect::default(),
            TextStyle::new()
                .with_size_px(size)
                .with_fg(theme.resolve(role)),
        ))])
        .with_tag(tag.to_owned())
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center),
        ),
    )
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let src = use_source();
    let cache = page_cache();

    let count = src.count();
    let offset = scroll.offset_y();
    let (_, max_y) = scroll.max();
    let following = at_bottom(offset, max_y);

    // Snapshot the visible pages' states once (subscribes + promotes them →
    // they stay resident under LRU), then map each windowed row to its line.
    let window = compute_visible_range(offset, VIEWPORT_H, count, ROW_PITCH, OVERSCAN);
    let page_states = cache.snapshot(pages_in_window(&window, PAGE_SIZE));

    let title = Scene::Text(TextNode::styled(
        "Paged streaming log (out-of-memory, LRU-bounded, tail-follow)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(15)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let status = centered_text(
        status_line(count, following),
        STATUS_TAG,
        13,
        ColorRole::OnSurfaceMuted,
        &theme,
    );
    let cache_info = centered_text(
        cache_info_line(&cache),
        CACHEINFO_TAG,
        12,
        ColorRole::OnSurfaceMuted,
        &theme,
    );

    let list = view_virtual_list(
        &scroll,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        count,
        ROW_PITCH,
        OVERSCAN,
        |index| build_row(index, &page_states, &theme),
    );

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
        ContainerNode::new(vec![
            title,
            emit_button(state, &theme),
            status,
            cache_info,
            list_root,
        ])
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

struct PagedStreamView;

impl WidgetCore for PagedStreamView {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Install the prefetch Effect FIRST (boot eager run → fetch the
        // initially visible pages) so the data layer is live before first paint.
        let _loader = install_loader();
        vec![scrollbar_extra_external(
            use_scroll_state(SCROLL_KEY),
            SCROLLBAR_TAG,
        )]
    }

    fn tag() -> &'static str {
        EMIT_TAG
    }

    fn read_state(scene: &Scene) -> ButtonState {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    return ButtonState::from_name_or_default(&name);
                }
            }
        }
        ButtonState::Idle
    }

    fn view(state: ButtonState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    /// R1005 — the Emit button's `"click"` intent appends one batch, invalidates
    /// the partial tail page, and applies stateless tail-follow ([`apply_emit`]).
    fn update(_state: ButtonState, intent: &Intent) -> Vec<Command> {
        if intent.tag.as_ref() == EMIT_INTENT_TAG_FULL {
            apply_emit(&use_source(), &use_scroll_state(SCROLL_KEY), &page_cache());
        }
        Vec::new()
    }

    fn event_name(event: ButtonEvent) -> &'static str {
        match event {
            ButtonEvent::PointerEnter => "PointerEnter",
            ButtonEvent::PointerLeave => "PointerLeave",
            ButtonEvent::PointerDown => "PointerDown",
            ButtonEvent::PointerUp => "PointerUp",
            ButtonEvent::PointerCancel => "PointerCancel",
            ButtonEvent::KeyboardActivate => "KeyboardActivate",
            ButtonEvent::Disable => "Disable",
            ButtonEvent::Enable => "Enable",
            _ => "__internal__",
        }
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    fn title() -> &'static str {
        "pinion hello-paged-stream (R1005 §5.22 §5.23 §5.27)"
    }

    fn fmt_state_log(state: &ButtonState) -> String {
        format!("emit button: {}", state.as_name())
    }
}

impl WidgetA11y for PagedStreamView {
    /// The Emit `button` (the tab stop) + a `role=status` live region (live
    /// count + follow state) + the virtualized `list` (`aria-setsize` = the
    /// current growing count, one windowed `listitem` per rendered row).
    fn access_node(state: &ButtonState, focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let count = use_source().count();
        let window =
            compute_visible_range(scroll.offset_y(), VIEWPORT_H, count, ROW_PITCH, OVERSCAN);

        let button = AccessNode::new(EMIT_TAG, AriaRole::Button)
            .with_name("Emit log lines")
            .with_state(AccessState {
                focused: focused == Some(EMIT_TAG),
                ..AccessState::from_interaction(*state, None)
            });
        let following = at_bottom(scroll.offset_y(), scroll.max().1);
        let status =
            AccessNode::new(STATUS_TAG, AriaRole::Status).with_name(status_line(count, following));

        let mut nodes = vec![button, status];
        nodes.extend(windowed_list_nodes(
            LIST_TAG,
            "Paged streaming log",
            u32::try_from(count).unwrap_or(u32::MAX),
            &window,
        ));
        nodes
    }
}

impl WidgetView for PagedStreamView {
    type Renderer = HelloPagedStreamRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<PagedStreamView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::widgets::scroll::max_scroll_offset;
    use pinion_core::widgets::virtual_list::content_height;

    /// Boot the cache slots (prefetch Effect) + measure the viewport (the layout
    /// pass does this in the real shell; a direct call skips layout).
    fn boot() -> (Rc<StreamSource>, Rc<ScrollState>, Rc<PageCache>) {
        let _loader = install_loader();
        let src = use_source();
        let scroll = use_scroll_state(SCROLL_KEY);
        scroll.set_measured_viewport(VIEWPORT_W, VIEWPORT_H);
        (src, scroll, page_cache())
    }

    /// Drive the pump to completion — what the shell does each frame.
    fn drain() {
        for _ in 0..(FETCH_LATENCY_POLLS + 8) {
            if !use_local_task_pump().poll() {
                break;
            }
        }
    }

    fn emit(src: &StreamSource, scroll: &ScrollState, cache: &PageCache) {
        apply_emit(src, scroll, cache);
        drain();
    }

    fn count_row_tags(scene: &Scene) -> usize {
        fn walk(scene: &Scene, n: &mut usize) {
            match scene {
                Scene::Container(c) => {
                    if c.tag
                        .as_deref()
                        .is_some_and(|t| t.starts_with("pagedstream#"))
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
    fn page_lines_partial_tail_then_full_after_more_count() {
        // Page 0 with only 50 rows produced is partial; with 100 it is full.
        assert_eq!(page_lines(0, 50).len(), 50, "partial tail page");
        assert_eq!(
            page_lines(0, 100).len(),
            PAGE_SIZE,
            "page 0 full once count passes it"
        );
        assert_eq!(page_lines(0, 50)[0], synth_line(0));
        assert_eq!(
            page_lines(1, 100)[0],
            synth_line(PAGE_SIZE),
            "page 1 starts at row 64"
        );
    }

    #[test]
    fn emit_grows_count_and_loads_only_a_bounded_window() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll, _cache) = boot();
            drain();
            assert_eq!(src.count(), 0, "starts empty");
            for _ in 0..40 {
                emit(&src, &scroll, &page_cache()); // 2000 rows
            }
            assert_eq!(src.count(), 40 * BATCH);
            let rendered = count_row_tags(&view(ButtonState::Idle, &Frame::default()));
            assert!(
                rendered <= 14 + 2 * OVERSCAN + 1,
                "virtualized window stays bounded: {rendered}"
            );
            assert!(rendered >= 14, "covers the 14-row viewport");
        });
    }

    #[test]
    fn invalidating_the_partial_tail_page_refetches_it_with_appended_rows() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll, cache) = boot();
            // First batch: 50 rows on the partial page 0. Row 49 loads.
            emit(&src, &scroll, &cache);
            assert_eq!(src.count(), 50);
            let scene = view(ButtonState::Idle, &Frame::default());
            // Following → window is at the tail; row 49 is the newest.
            assert_eq!(
                row_text_of(&scene, 49).as_deref(),
                Some(synth_line(49).as_str())
            );
            // Row 50 does not exist yet.
            assert_eq!(page_lines(0, 50).get(50), None);
            // Second batch: count 100. Page 0 is now full (64 rows) — row 50
            // (previously absent) appears ON page 0 only because the partial
            // tail page was invalidated + re-fetched.
            emit(&src, &scroll, &cache);
            assert_eq!(src.count(), 100);
            assert_eq!(page_of(50), 0, "row 50 lives on page 0");
            // Bring page 0 back into the window so it is ensured + resident, drain.
            scroll.scroll_to(0, 48 * i32::try_from(ROW_PITCH).unwrap());
            drain();
            // Direct cache witness: page 0 (formerly a 50-row partial tail) was
            // invalidated + re-fetched, so it now holds the full 64 rows incl 50.
            match page_cache().state(&0) {
                Some(ResourceState::Ready(rows)) => {
                    assert_eq!(
                        rows.len(),
                        PAGE_SIZE,
                        "partial tail page re-fetched to full"
                    );
                    assert_eq!(rows[50], synth_line(50), "row 50 now present on page 0");
                }
                other => panic!("page 0 should be Ready after refetch, got {other:?}"),
            }
            // And it renders in the now-visible window.
            let scene2 = view(ButtonState::Idle, &Frame::default());
            assert_eq!(
                row_text_of(&scene2, 50).as_deref(),
                Some(synth_line(50).as_str()),
                "the partial tail page grew to include row 50 after invalidate+refetch",
            );
        });
    }

    #[test]
    fn resident_pages_stay_lru_bounded_while_the_stream_is_unbounded() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll, cache) = boot();
            for _ in 0..200 {
                emit(&src, &scroll, &cache); // 10,000 rows ≈ 157 pages
            }
            assert_eq!(src.count(), 200 * BATCH);
            // Scroll across far bands so many distinct pages are touched. Row 0
            // (page 0) is touched first, then four far bands.
            for row in [0usize, 2_000, 5_000, 8_000, 9_900] {
                scroll.scroll_to(
                    0,
                    i32::try_from(row).unwrap() * i32::try_from(ROW_PITCH).unwrap(),
                );
                drain();
            }
            // `len() <= cap` alone is structurally guaranteed by the LRU; the
            // real witness that eviction HAPPENS (memory stays flat over an
            // unbounded stream) is that page 0 — touched first, then five bands
            // ago — is actually gone, not merely that the count is capped.
            assert!(
                cache.len() <= CACHE_PAGES.get(),
                "resident pages capped at {}",
                CACHE_PAGES.get()
            );
            assert!(
                !cache.contains(&0),
                "page 0 was evicted after far scrolling (eviction happens, not just the cap holds)",
            );
        });
    }

    #[test]
    fn following_pins_to_the_new_bottom_paused_stays_put() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll, cache) = boot();
            emit(&src, &scroll, &cache); // 50 rows, followed to bottom
            let bottom = max_scroll_offset(content_height(src.count(), ROW_PITCH), VIEWPORT_H);
            assert_eq!(scroll.offset_y(), bottom, "followed to the new bottom");
            assert!(bottom > 0, "50 rows exceed the 14-row viewport");
            // Scroll up to read history → paused.
            scroll.scroll_to(0, 0);
            drain();
            let paused = scroll.offset_y();
            emit(&src, &scroll, &cache); // 100 rows now
            assert_eq!(scroll.offset_y(), paused, "paused viewport stays put");
            assert_eq!(
                scroll.max().1,
                max_scroll_offset(content_height(src.count(), ROW_PITCH), VIEWPORT_H),
                "the extent grew underneath the paused viewport",
            );
        });
    }

    #[test]
    fn status_and_cacheinfo_witness_the_count_and_bound() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll, cache) = boot();
            drain();
            let empty = view(ButtonState::Idle, &Frame::default());
            assert_eq!(
                text_of(&empty, STATUS_TAG).as_deref(),
                Some("0 lines \u{00b7} click Emit to stream")
            );
            for _ in 0..3 {
                emit(&src, &scroll, &cache); // 150 rows
            }
            let scene = view(ButtonState::Idle, &Frame::default());
            assert_eq!(
                text_of(&scene, STATUS_TAG).as_deref(),
                Some("150 lines \u{00b7} Following (newest)"),
            );
            // Resident pages are bounded — the out-of-memory witness.
            let info = text_of(&scene, CACHEINFO_TAG).unwrap();
            assert!(
                info.starts_with("Resident pages: "),
                "cacheinfo present: {info}"
            );
            assert!(cache.len() <= CACHE_PAGES.get());
        });
    }

    #[test]
    fn a11y_setsize_grows_and_items_are_windowed() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll, cache) = boot();
            for _ in 0..4 {
                emit(&src, &scroll, &cache); // 200 rows
            }
            let nodes = PagedStreamView::access_node(&ButtonState::Idle, Some(EMIT_TAG));
            assert_eq!(nodes[0].role, AriaRole::Button);
            assert!(nodes[0].state.focused);
            assert_eq!(nodes[1].role, AriaRole::Status);
            assert_eq!(nodes[2].role, AriaRole::List);
            assert_eq!(
                nodes[2].size_of_set,
                Some(u32::try_from(4 * BATCH).unwrap())
            );
            assert!(
                nodes.len() - 3 <= 14 + 2 * OVERSCAN + 1,
                "only the window has listitems"
            );
            for item in &nodes[3..] {
                assert_eq!(item.role, AriaRole::ListItem);
                assert_eq!(item.size_of_set, Some(u32::try_from(4 * BATCH).unwrap()));
            }
        });
    }
}
