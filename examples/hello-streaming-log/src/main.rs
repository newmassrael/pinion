//! `hello-streaming-log` — R996 §5.22 §5.23 §5.27: **Model-View
//! streaming-append**. The Model/View-at-scale gap the self-hosted editor's
//! terminal scrollback / packet capture / log tail all share: a virtualized
//! list whose **row count grows at runtime** as a producer appends new lines.
//!
//! ## What this is (R924 virtualization × a growable count)
//!
//! `hello-lazy-list` (R924) virtualizes a list of a *fixed* size `N` over a
//! paged async source. R996 keeps the same windowing backbone but makes the
//! row count a runtime-growable [`Signal<usize>`]: an Emit button appends a
//! batch of lines to a shared append-only [`LogSource`], the count Signal
//! bumps, and the view re-renders against the larger count. The windowing math
//! ([`compute_visible_range`] / [`view_virtual_list`]) is **stateless and
//! count-parametric** (R927 proved a reactive count flows through it), so the
//! visible window *and* the scroll extent grow with no refetch and no new
//! windowing substrate.
//!
//! ## Tail-follow is STATELESS (`tail -f`)
//!
//! A terminal autoscrolls to the newest line while you watch — unless you have
//! scrolled up to read history. That is modelled without any `following` flag or
//! Effect: tail-follow is **derived from the scroll position**. The emit reducer
//! reads "is the viewport at the bottom *now*" ([`at_bottom`]) *before*
//! appending; the shared [`follow_tail`] reducer shape then grows the scroll
//! bound to the new extent and, if it was following, pins to the new bottom.
//! Scroll up and the next batch appends silently below (paused); scroll back and
//! follow resumes — no stored state. The view shows the derived
//! "Following" / "Paused" status from the same predicate.
//!
//! [`at_bottom`] + [`follow_tail`] are the **windowing substrate** (R1005 lift):
//! R996 wrote them here as the 1st streaming consumer; the paged out-of-memory
//! view (`hello-paged-stream`) is the 2nd ([[abstraction-needs-second-consumer]]),
//! so they moved into `pinion_core::widgets::virtual_list` and both consumers
//! share them. They grow the bound themselves because [`ScrollState::scroll_to`]
//! clamps to the *current* `max_y`, which the layout pass only grows next frame
//! — so the autoscroll lands on the same frame as the append, deterministically.
//!
//! ## Scope
//!
//! This is the **in-memory** streaming slice (①a): the producer's lines live
//! in an unbounded `Vec` and the row count is a `Signal`. That covers a bounded
//! scrollback (a terminal, a capped log). The **out-of-memory** case —
//! millions of streaming rows paged behind an LRU cache with a tail-page
//! re-fetch (a GB DLT log, a analyser capture) — is slice ①b, demonstrated in
//! `hello-paged-stream` (R1005) via [`ResourceCache::invalidate`]. The tail-follow + windowing structure here is the
//! shared part both slices use.
//!
//! [`follow_tail`]: pinion_core::widgets::virtual_list::follow_tail
//! [`ResourceCache::invalidate`]: pinion_core::reactive::ResourceCache::invalidate
//!
//! ## AI-first witness (§2 #7)
//!
//! `scene/snapshot` reports a small window of `streamlog#<i>` rows (not the
//! whole log), a `role=status` line with the live count + Following/Paused
//! state, and `aria-setsize` tracking the growing total. `scene/click` the
//! `stream_emit` button to append; `scene/scroll` to pause / resume follow.
//! See `tools/demos/r996_streaming_log.py`.

use pinion_a11y::{AccessNode, AccessState, AriaRole, WidgetA11y, windowed_list_nodes};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::intent::Intent;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::widgets::scroll::{ScrollState, use_scroll_state};
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::{at_bottom, compute_visible_range, follow_tail};
use pinion_core::{Command, Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::scrollbar::{VerticalScrollbarStyle, view_vertical_scrollbar};
use pinion_widget_paint::virtual_list::view_virtual_list;
use std::cell::RefCell;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloStreamingLogRenderer, HelloStreamingLogRendererError);

const WIN_W: u32 = 440;
const WIN_H: u32 = 540;
const THEME_TAG: &str = "app";

/// Uniform per-row vertical slot (logical px). Uniform pitch → exact window math.
const ROW_PITCH: u32 = 24;
/// Rows built above + below the strict visible window (fast-flick gap guard).
const OVERSCAN: usize = 4;
/// Scroll viewport width.
const VIEWPORT_W: u32 = 400;
/// Scroll viewport height — exactly 14 rows tall.
const VIEWPORT_H: u32 = 14 * ROW_PITCH;
/// Lines appended per Emit click — one "batch" arriving from the producer.
const BATCH: usize = 50;

/// Paint-root + a11y `list` container tag, and the `streamlog#<i>` row prefix.
const LOG_TAG: &str = "streamlog";
/// `role=status` line tag (live count + Following/Paused).
const STATUS_TAG: &str = "stream_status";
/// The Emit button's paint + External tag (the §5.20 routing handle).
const EMIT_TAG: &str = "stream_emit";
/// Cache key for the scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "stream_scroll";
/// Paint + state tag for the interactive scrollbar peer.
const SCROLLBAR_TAG: &str = "stream_scrollbar";
/// `Owner::cache` key for the shared append-only [`LogSource`].
const SOURCE_KEY: &str = "stream.source";

/// Fully-prefixed wire tag for the Emit button's `"click"` intent (§5.20 R22).
const EMIT_INTENT_TAG_FULL: &str = pinion_core::intent_tag!("stream_emit", "click");

/// Log levels rotated per line — purely cosmetic, keeps the synthesized lines
/// readable as log output.
const LEVELS: [&str; 4] = ["INFO ", "WARN ", "ERROR", "DEBUG"];

// ─── shared append-only source (the reactive holder) ───────────────────────

/// The producer's append-only log: the lines plus a reactive `count` the view
/// subscribes to. The canonical [[reactive-holder-for-shared-external-view-state]]
/// shape — the reducer mutates it, the view (and a11y, and RPC query) read it,
/// shared by one `Rc` through [`use_source`].
///
/// Lines are append-only, so a row index, once emitted, is stable; the view
/// re-renders on `count` change (and on scroll) and reads the lines then.
struct LogSource {
    lines: RefCell<Vec<String>>,
    count: Signal<usize>,
}

impl LogSource {
    fn new() -> Self {
        Self {
            lines: RefCell::new(Vec::new()),
            count: Signal::new(0),
        }
    }

    /// Append `n` freshly-synthesized lines at the tail and publish the new
    /// total through the `count` Signal (the one observable change the view
    /// subscribes to).
    fn emit(&self, n: usize) {
        let mut lines = self.lines.borrow_mut();
        let base = lines.len();
        for k in 0..n {
            let seq = base + k;
            let level = LEVELS[seq % LEVELS.len()];
            lines.push(format!("[{seq:06}] {level} worker: event {seq} processed"));
        }
        let total = lines.len();
        drop(lines);
        self.count.set(total);
    }
}

fn use_source() -> Rc<LogSource> {
    Owner::current()
        .expect("use_source() requires an active Owner scope")
        .cache(SOURCE_KEY, LogSource::new)
}

// ─── tail-follow (stateless) ───────────────────────────────────────────────

/// Append one batch and apply stateless tail-follow. Extracted from the reducer
/// so the streaming behaviour is unit-testable without constructing an Intent.
/// The was-following decision ([`at_bottom`]) + the grow-bound-then-pin reducer
/// shape ([`follow_tail`]) are the shared windowing substrate (R1005 lift);
/// only the domain-specific `emit` (an in-memory `Vec` push) lives here.
fn apply_emit(src: &LogSource, scroll: &ScrollState) {
    // The measured viewport (layout-written); fall back to the const before the
    // first layout pass has run (e.g. an emit before first paint, or a unit
    // test that has not measured). The const == the measured height for this
    // fixed-size window, which is load-bearing: `follow_tail`'s bound below must
    // equal the bound the layout pass will write from `s.viewport.h`.
    let measured_h = scroll.measured_viewport().1;
    let viewport_h = if measured_h == 0 {
        VIEWPORT_H
    } else {
        measured_h
    };
    // Decide BEFORE appending: was the viewport already at the newest line?
    let was_following = at_bottom(scroll.offset_y(), scroll.max().1);
    src.emit(BATCH);
    follow_tail(
        scroll,
        src.count.get(),
        ROW_PITCH,
        viewport_h,
        was_following,
    );
}

// ─── view ──────────────────────────────────────────────────────────────────

fn zebra_fill(index: usize, theme: &Theme) -> pinion_core::style::Color {
    if index % 2 == 0 {
        theme.resolve(ColorRole::SurfaceContainerLow)
    } else {
        theme.resolve(ColorRole::SurfaceContainer)
    }
}

/// One virtualized row, tagged `streamlog#<index>` so the windowed-list a11y
/// nodes + `enrich_names_from_scene` attach. `lines` is the live (borrowed)
/// append-only buffer; an index inside the window always resolves (count ==
/// `lines.len()`), so a missing entry is only the defensive ellipsis.
fn build_row(index: usize, lines: &[String], theme: &Theme) -> Scene {
    let content = lines
        .get(index)
        .cloned()
        .unwrap_or_else(|| "\u{2026}".to_owned());
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            content,
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(format!("{LOG_TAG}#{index}"))
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

/// The `role=status` line text — the SSOT for the live count + the derived
/// follow state. Reads as `"<N> lines \u{00b7} Following"` /
/// `"\u{00b7} Paused"`; an empty log prompts the first emit.
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

/// The Emit button — a tagged, accent-filled control routed to the
/// [`ButtonExternal`] primary. Its fill reflects the interaction `state`.
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

/// view-fn (§6.3): pure sync `(ButtonState) -> Scene`. Reads the reactive
/// count and the scroll offset / bound; the dataset is virtualized and the
/// count grows at runtime as the producer appends.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let src = use_source();

    // Subscribe to the count (re-render on append) + the scroll bound/offset
    // (re-render on extent change / user scroll). `following` is derived.
    let count = src.count.get();
    let offset = scroll.offset_y();
    let (_, max_y) = scroll.max();
    let following = at_bottom(offset, max_y);

    let title = Scene::Text(TextNode::styled(
        "Streaming log (append-only, virtualized, tail-follow)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(15)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let status = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            status_line(count, following),
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

    // The windowed list — build only the visible indices, reading the live
    // append-only buffer once (borrow held across the windowed build).
    let lines = src.lines.borrow();
    let list = view_virtual_list(
        &scroll,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        count,
        ROW_PITCH,
        OVERSCAN,
        |index| build_row(index, &lines, &theme),
    );

    // Scrollbar peer — sized against the full extent the layout pass wrote into
    // `ScrollState::max_y`, sharing the same `Rc`.
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
            .with_tag(LOG_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    Scene::Container(
        ContainerNode::new(vec![title, emit_button(state, &theme), status, list_root])
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

struct StreamingLogView;

impl WidgetCore for StreamingLogView {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Touch the source + scroll state once at boot so their cache slots
        // exist before the first paint / dispatch (the view + reducer both
        // resolve the same `Rc`). The scrollbar peer shares the scroll state.
        let _src = use_source();
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
                if let Ok(IntrospectValue::Text(name)) = intro.query("state") {
                    return ButtonState::from_name_or_default(&name);
                }
            }
        }
        ButtonState::Idle
    }

    fn view(state: ButtonState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    /// R996 — the Emit button's `"click"` intent appends one batch to the
    /// shared source and applies stateless tail-follow ([`apply_emit`]). The
    /// reducer runs in an Owner scope, so it resolves the same `use_source` /
    /// `use_scroll_state` slots the view does.
    fn update(_state: ButtonState, intent: &Intent) -> Vec<Command> {
        if intent.tag.as_ref() == EMIT_INTENT_TAG_FULL {
            apply_emit(&use_source(), &use_scroll_state(SCROLL_KEY));
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

    /// ARIA button keyboard activation: Space / Enter on the focused Emit
    /// button fires `KeyboardActivate` (→ `"click"` → the emit reducer), in
    /// parity with a pointer click.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    fn title() -> &'static str {
        "pinion hello-streaming-log (R996 §5.22 §5.23 §5.27)"
    }

    fn fmt_state_log(state: &ButtonState) -> String {
        format!("emit button: {}", state.as_name())
    }
}

impl WidgetA11y for StreamingLogView {
    /// The Emit `button` (the tab stop) plus the virtualized `list`: one
    /// `listitem` per rendered row with its absolute `aria-posinset`, and
    /// `aria-setsize` = the *current* (growing) line count. The windowed list
    /// topology is the shared `pinion_a11y::windowed_list_nodes` SSOT (same
    /// builder as `hello-lazy-list`); the set size grows as the producer emits.
    fn access_node(state: &ButtonState, focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let count = use_source().count.get();
        let window =
            compute_visible_range(scroll.offset_y(), VIEWPORT_H, count, ROW_PITCH, OVERSCAN);

        let button = AccessNode::new(EMIT_TAG, AriaRole::Button)
            .with_name("Emit log lines")
            .with_state(AccessState {
                focused: focused == Some(EMIT_TAG),
                ..AccessState::from_interaction(*state, None)
            });

        // The live count + follow state as a `role=status` live region — so an
        // AT (and an AI agent) is announced the growing line count and whether
        // the view is following the tail, not just the focused button + rows.
        let following = at_bottom(scroll.offset_y(), scroll.max().1);
        let status =
            AccessNode::new(STATUS_TAG, AriaRole::Status).with_name(status_line(count, following));

        let mut nodes = vec![button, status];
        nodes.extend(windowed_list_nodes(
            LOG_TAG,
            "Streaming log",
            u32::try_from(count).unwrap_or(u32::MAX),
            &window,
        ));
        nodes
    }
}

impl WidgetView for StreamingLogView {
    type Renderer = HelloStreamingLogRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<StreamingLogView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    // The expected-bottom math the tests assert against (the same SSOTs
    // `follow_tail` uses internally), now that the reducer delegates the lift.
    use pinion_core::widgets::scroll::max_scroll_offset;
    use pinion_core::widgets::virtual_list::content_height;

    /// Boot the cache slots + measure the viewport (the layout pass does this
    /// in the real shell; a direct `view()` / reducer call skips layout).
    fn boot() -> (Rc<LogSource>, Rc<ScrollState>) {
        let src = use_source();
        let scroll = use_scroll_state(SCROLL_KEY);
        scroll.set_measured_viewport(VIEWPORT_W, VIEWPORT_H);
        (src, scroll)
    }

    /// Count `streamlog#<i>` row containers anywhere in the scene.
    fn count_row_tags(scene: &Scene) -> usize {
        fn walk(scene: &Scene, n: &mut usize) {
            match scene {
                Scene::Container(c) => {
                    if c.tag
                        .as_deref()
                        .is_some_and(|t| t.starts_with("streamlog#"))
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

    fn row_text_of(scene: &Scene, index: usize) -> Option<String> {
        let c = find_container(scene, &format!("{LOG_TAG}#{index}"))?;
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
    fn emit_grows_the_count_and_appends_at_the_tail() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            assert_eq!(src.count.get(), 0, "starts empty");
            apply_emit(&src, &scroll);
            assert_eq!(src.count.get(), BATCH, "one batch appended");
            apply_emit(&src, &scroll);
            assert_eq!(
                src.count.get(),
                2 * BATCH,
                "second batch appends at the tail"
            );
            // Line 0 is stable across appends; line 50 is the second batch's head.
            assert_eq!(
                src.lines.borrow()[0],
                "[000000] INFO  worker: event 0 processed",
            );
            // LEVELS[BATCH % 4] = LEVELS[50 % 4 = 2] = "ERROR".
            assert_eq!(
                src.lines.borrow()[BATCH],
                format!("[{BATCH:06}] ERROR worker: event {BATCH} processed"),
            );
        });
    }

    #[test]
    fn following_pins_the_viewport_to_the_new_bottom() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            // At the empty bottom → following.
            apply_emit(&src, &scroll);
            let bottom = max_scroll_offset(content_height(src.count.get(), ROW_PITCH), VIEWPORT_H);
            assert_eq!(scroll.offset_y(), bottom, "followed to the new bottom");
            assert!(bottom > 0, "50 rows exceed the 14-row viewport");
            // Still at the bottom → the next batch follows again.
            apply_emit(&src, &scroll);
            assert_eq!(
                scroll.offset_y(),
                max_scroll_offset(content_height(src.count.get(), ROW_PITCH), VIEWPORT_H)
            );
        });
    }

    #[test]
    fn scrolling_up_pauses_follow_and_appends_stay_below() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            apply_emit(&src, &scroll); // 50 rows, followed to bottom
            // The user scrolls up to read history.
            scroll.scroll_to(0, 100);
            assert!(!at_bottom(scroll.offset_y(), scroll.max().1), "now paused");
            let paused_offset = scroll.offset_y();
            apply_emit(&src, &scroll); // 100 rows now
            assert_eq!(src.count.get(), 2 * BATCH, "rows still appended");
            assert_eq!(
                scroll.offset_y(),
                paused_offset,
                "paused: viewport stays put"
            );
            // The extent grew underneath the paused viewport.
            assert_eq!(
                scroll.max().1,
                max_scroll_offset(content_height(src.count.get(), ROW_PITCH), VIEWPORT_H)
            );
        });
    }

    #[test]
    fn scrolling_back_to_the_bottom_resumes_follow() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            apply_emit(&src, &scroll);
            scroll.scroll_to(0, 100); // pause
            apply_emit(&src, &scroll); // append below (paused)
            // Return to the bottom → following resumes on the next batch.
            scroll.scroll_to(0, scroll.max().1);
            assert!(at_bottom(scroll.offset_y(), scroll.max().1));
            apply_emit(&src, &scroll);
            assert_eq!(
                scroll.offset_y(),
                max_scroll_offset(content_height(src.count.get(), ROW_PITCH), VIEWPORT_H),
                "follow resumed"
            );
        });
    }

    #[test]
    fn view_renders_a_small_window_not_the_whole_log() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            for _ in 0..20 {
                apply_emit(&src, &scroll); // 1000 rows
            }
            let scene = view(ButtonState::Idle, &Frame::default());
            let rendered = count_row_tags(&scene);
            // 14-row viewport + OVERSCAN above & below + a straddling partial.
            assert!(
                rendered <= 14 + 2 * OVERSCAN + 1,
                "virtualized window stays bounded, got {rendered} of 1000"
            );
            assert!(rendered >= 14, "must cover the 14-row viewport");
        });
    }

    #[test]
    fn view_window_shows_the_tail_when_following() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            apply_emit(&src, &scroll); // 50 rows, followed to the bottom
            let scene = view(ButtonState::Idle, &Frame::default());
            // The last line (49) is in the window; the first (0) scrolled off.
            assert!(
                row_text_of(&scene, BATCH - 1).is_some(),
                "newest line is visible when following",
            );
            assert!(
                row_text_of(&scene, 0).is_none(),
                "oldest line is off the top"
            );
            assert_eq!(
                status_text(&scene).as_deref(),
                Some("50 lines \u{00b7} Following (newest)"),
            );
        });
    }

    #[test]
    fn status_reports_count_and_paused_when_scrolled_up() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            assert_eq!(
                status_text(&view(ButtonState::Idle, &Frame::default())).as_deref(),
                Some("0 lines \u{00b7} click Emit to stream"),
            );
            apply_emit(&src, &scroll);
            scroll.scroll_to(0, 0); // top of a 50-row log → paused
            let scene = view(ButtonState::Idle, &Frame::default());
            assert_eq!(
                status_text(&scene).as_deref(),
                Some("50 lines \u{00b7} Paused (scroll to bottom to follow)"),
            );
        });
    }

    #[test]
    fn a11y_setsize_grows_with_the_log_and_items_are_windowed() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            for _ in 0..4 {
                apply_emit(&src, &scroll); // 200 rows
            }
            let nodes = StreamingLogView::access_node(&ButtonState::Idle, Some(EMIT_TAG));
            // Node 0 is the focused Emit button.
            assert_eq!(nodes[0].role, AriaRole::Button);
            assert!(nodes[0].state.focused, "Emit button carries focus");
            // Node 1 is the role=status live region (count + follow state).
            assert_eq!(nodes[1].role, AriaRole::Status);
            assert_eq!(
                nodes[1].name.as_deref(),
                Some(status_line(4 * BATCH, true).as_str()),
                "status live region announces the current count + follow state",
            );
            // Node 2 is the list; its setsize is the current count.
            assert_eq!(nodes[2].role, AriaRole::List);
            assert_eq!(
                nodes[2].size_of_set,
                Some(u32::try_from(4 * BATCH).unwrap())
            );
            // Only the window is materialized as listitems (button + status + list + items).
            assert!(
                nodes.len() - 3 <= 14 + 2 * OVERSCAN + 1,
                "only the rendered window has listitems"
            );
            for item in &nodes[3..] {
                assert_eq!(item.role, AriaRole::ListItem);
                assert_eq!(item.size_of_set, Some(u32::try_from(4 * BATCH).unwrap()));
            }
        });
    }
}
