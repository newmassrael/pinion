//! `hello-tail-reveal` — R1458 §5.45 §5.27: **the reveal lands in the frame it
//! was asked for**, when the list that has to move is virtualized.
//!
//! ## The gap this closes
//!
//! [`hello-transcript`] already arms
//! [`ScrollState::follow_measured_tail`](pinion_core::widgets::scroll::ScrollState::follow_measured_tail)
//! on a transcript of wrapped prose — the binding names no bound, and the
//! layout pass pins the viewport to the extent it measured. Its transcript is a
//! plain column, though, so *every* pass lays out *every* entry: the first
//! bound the frame publishes is already the true one.
//!
//! Virtualize the same transcript and that stops being true. A windowed list
//! only lays out the rows it materialized, so the harvest can only measure
//! those; the rest are still counted at the estimate. The bound the first pass
//! publishes is therefore **provisional** — and it is exactly the pass that a
//! one-shot pin would have spent its arming on. The viewport landed on the
//! estimate-derived tail, the next pass measured the rows that arrival had
//! brought into the window, the bound grew past where the reader now sat, and
//! nothing was left armed to carry them the rest of the way. The newest entry
//! stayed off the bottom of the screen, by exactly the refinement.
//!
//! R1458 makes both halves converge instead:
//!
//! - the arming survives a pass whose pin still **moved** something, and is
//!   spent by the first pass that has nothing left to move;
//! - the paint runs view + layout to that fixed point **before** presenting,
//!   and — if a binding cannot converge inside the budget — asks for another
//!   frame rather than leaving a half-settled picture on a `ControlFlow::Wait`
//!   event loop.
//!
//! The binding below is unchanged from `hello-transcript`'s: append, arm, name
//! no number. That is the point — the consumer never learns that its list is
//! windowed.
//!
//! ## AI-first witness (§2 #7)
//!
//! `scene/scroll_state` publishes the offset, the bound and the standing
//! arming; `scene/snapshot` carries every *windowed* row slot's measured rect;
//! and the primary External publishes the measurement table itself
//! (`total_height` / `measured_count` / `is_fully_measured` /
//! `measured_height.<row>`). So "the reveal landed" is checkable without
//! pixels, and so is the precondition that makes it hard — a tail row whose
//! measured height is far past the estimate the first pass counted it at. See
//! `tools/demos/r1458_tail_reveal.py`.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y, windowed_list_nodes};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, RepaintOwner, SchemaArg, SchemaField, ThreadOwnership,
    int_of,
};
use pinion_core::intent::Intent;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::measured_rows::{MeasuredRowState, use_measured_rows};
use pinion_core::widgets::scroll::{ScrollState, use_scroll_state};
use pinion_core::widgets::virtual_list::at_bottom;
use pinion_core::widgets::virtual_list::compute_visible_range_variable;
use pinion_core::{Command, Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, button_scene, read_button_focused,
    read_button_state,
};
use pinion_widget_paint::virtual_list::view_measured_list;
use std::cell::RefCell;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTailRevealRenderer, HelloTailRevealRendererError);

const WIN_W: u32 = 520;
const WIN_H: u32 = 620;
const THEME_TAG: &str = "app";

/// Scroll viewport (logical px). Fixed, so the demo's expectations are
/// geometry-derived rather than window-manager-derived.
const VIEWPORT_W: u32 = 460;
const VIEWPORT_H: u32 = 300;
/// Wrap width for one entry — the viewport minus its horizontal padding. THIS
/// is the only geometry the binding states, and it says nothing about height.
const ENTRY_W: u32 = VIEWPORT_W - 2 * 12;
/// Rows rendered beyond the strict window on each side. `0` keeps the windowed
/// set — and therefore what the harvest can measure — as tight as the real
/// thing gets, which is what makes the first pass's bound provisional.
const OVERSCAN: usize = 0;
/// The per-row estimate the windowing starts from. Deliberately far below every
/// real entry: an un-materialized row counted at this is the whole reason the
/// first pass's bound is short.
const EST: u32 = 24;

/// Cache key + paint tag for the transcript's `ScrollState`.
const SCROLL_TAG: &str = "reveal_scroll";
/// Cache key for the reactive `MeasuredRowState` (the measurement table).
const MEASURED_KEY: &str = "reveal_measured";
/// The primary (display-only, queryable) External: the measurement table's
/// AI-first read surface, and the a11y `log` container.
const LIST_TAG: &str = "reveal_list";
/// `role=status` line: entry count + the derived follow state.
const STATUS_TAG: &str = "reveal_status";
/// The Reply button (extra External) — always reveals its entry.
const REPLY_TAG: &str = "reveal_reply";
/// `Owner::cache` key for the shared append-only transcript.
const SOURCE_KEY: &str = "reveal.source";

/// Fully-prefixed wire tag for the Reply control's `"click"` intent (§5.20).
const REPLY_INTENT: &str = pinion_core::intent_tag!("reveal_reply", "click");

/// Reply bodies, rotated per reply — deliberately different lengths, so each
/// entry wraps to a different number of lines and the extent is genuinely
/// unknowable without laying it out. The long ones matter most: they are the
/// rows whose estimate-counted stand-in is furthest from the truth.
const BODIES: [&str; 4] = [
    "The frame runs to a fixed point before it is painted, so the offset you \
     see is the one the last pass agreed on.",
    "A windowed list measures only what it materialized. That is not a defect \
     to work around: it is why the bound the first pass publishes is a \
     provisional one, and why an intent to reach the tail has to survive long \
     enough to be told where the tail actually is. Nothing here computes a \
     height; the binding says where it wants to be and the layout pass says \
     how far that is.",
    "Short reply.",
    "Wrapped prose has no pitch to multiply, so the arming carries no number \
     at all — the one thing the caller genuinely cannot supply. What it does \
     carry is the intent, and the intent outlives any single pass.",
];

/// The seeded backlog. Large enough that the estimate-derived window covers
/// only a fraction of it, so most rows — including every one near the tail —
/// are still counted at [`EST`] when the reader is at the top. That is the
/// precondition: a bound computed from that table is provisional.
const SEED: usize = 60;

// ─── the source ────────────────────────────────────────────────────────────

/// Append-only transcript. `count` is the reactive handle the view subscribes
/// to; the text itself lives behind a `RefCell` (append-only, so a snapshot
/// clone per frame would be pure waste).
#[derive(Debug)]
struct Transcript {
    entries: RefCell<Vec<String>>,
    count: Signal<usize>,
}

impl Transcript {
    fn new() -> Self {
        let src = Self {
            entries: RefCell::new(Vec::new()),
            count: Signal::new(0),
        };
        for _ in 0..SEED {
            src.push();
        }
        src
    }

    /// Append one entry, rotating the body pool by the running index so a demo
    /// can predict every entry's text without predicting its height.
    fn push(&self) {
        let mut entries = self.entries.borrow_mut();
        let nth = entries.len();
        entries.push(entry_text(nth));
        let total = entries.len();
        drop(entries);
        self.count.set(total);
    }

    fn text(&self, index: usize) -> String {
        self.entries
            .borrow()
            .get(index)
            .cloned()
            .unwrap_or_default()
    }
}

/// One entry's rendered text. A free fn so the demo mirrors it exactly (the
/// data witness for "the entry that appended is the one on screen").
fn entry_text(nth: usize) -> String {
    format!("{nth}: {}", BODIES[nth % BODIES.len()])
}

fn use_transcript() -> Rc<Transcript> {
    Owner::current()
        .expect("use_transcript() requires an active Owner scope")
        .cache(SOURCE_KEY, Transcript::new)
}

// ─── the reducer (unchanged from the un-windowed sibling) ──────────────────

/// A reply is the answer to what the reader just pressed, so it is revealed
/// wherever they were reading. No bound is named, and nothing here knows that
/// the list is virtualized: the arming defers the pin to the passes that
/// measure the newly-wrapped entry.
fn apply_reply(src: &Transcript, scroll: &ScrollState, measured: &MeasuredRowState) {
    src.push();
    measured.set_count(src.count.get());
    scroll.follow_measured_tail();
}

// ─── view ──────────────────────────────────────────────────────────────────

/// The `role=status` text — SSOT for the entry count + the derived follow
/// state.
fn status_line(count: usize, following: bool) -> String {
    let mode = if following {
        "at the tail"
    } else {
        "scrolled back"
    };
    format!("{count} entries \u{00b7} {mode}")
}

/// One entry: a wrapped text leaf inside a height-auto row. Width is declared,
/// height is whatever the line breaker produces — the property that makes this
/// transcript's extent layout-measured. The slot wrapper
/// (`view_measured_list`) tags it `<scroll>/measured-row:<index>` and leaves
/// its height free so the harvest reads the true content height.
fn build_entry(text: &str, theme: &Theme) -> Scene {
    let leaf = Scene::Text(
        TextNode::styled(
            text.to_owned(),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_layout(LayoutStyle::new().with_size(Size::width_px(ENTRY_W))),
    );
    Scene::Container(
        ContainerNode::new(vec![leaf])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainer)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_padding(Rect::new(12, 6, 12, 6)),
            ),
    )
}

fn control(label: &str, state: ButtonState, focused: bool, theme: &Theme) -> Scene {
    button_scene(
        label,
        state,
        focused,
        REPLY_TAG, // hover-spring key: the tag is already unique
        &ButtonColors::accent(theme),
        &ButtonStyle::m3_default(REPLY_TAG)
            .with_size(Size::px(190, 38))
            .with_corner_radius(19)
            .with_label_font_size_px(15),
    )
}

/// The Reply control's interaction posture, read back from its External.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Controls {
    reply: ButtonState,
    reply_focused: bool,
}

/// view-fn (§6.3): pure sync `(Controls) -> Scene`. Subscribes to the entry
/// count (re-render on append) and the scroll offset / bound (re-render when
/// the reader moves, or when the pin lands).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: Controls, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let scroll = use_scroll_state(SCROLL_TAG);
    let src = use_transcript();
    let count = src.count.get();
    let measured = use_measured_rows(MEASURED_KEY, count, EST);
    let following = at_bottom(scroll.offset_y(), scroll.max().1);

    let title = Scene::Text(TextNode::styled(
        "Windowed transcript (the tail is measured, never estimated)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(15)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let controls = Scene::Container(
        ContainerNode::new(vec![control(
            "Reply",
            state.reply,
            state.reply_focused,
            &theme,
        )])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center),
        ),
    );

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

    // The windowed measured list. Builds ONLY the rows in the current window,
    // and carries `measured` so the layout pass finds the harvest target.
    let list = view_measured_list(
        &scroll,
        &measured,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        OVERSCAN,
        |index| build_entry(&src.text(index), &theme),
    );
    let list_root = Scene::Container(
        ContainerNode::new(vec![list])
            .with_tag(LIST_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    );

    Scene::Container(
        ContainerNode::new(vec![title, controls, status, list_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_gap(14),
            ),
    )
}

// ─── the queryable anchor ──────────────────────────────────────────────────

/// R1458 §5.27 — the primary, display-only External. Its `query` channel is
/// the AI-first witness for the claim this example exists to make: it holds
/// the same owner-cached [`MeasuredRowState`] the view windows against, so an
/// agent reads the live measurement table — including which rows are still
/// counted at the estimate — without pixels.
#[derive(Debug)]
struct TailRevealExternal {
    measured: Rc<MeasuredRowState>,
}

impl External for TailRevealExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(
            &[Backend::Gui, Backend::Tui, Backend::Rpc],
            BackendFallback::Skip,
        )
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for TailRevealExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("item_count", "int"),
                    SchemaField::new("estimated", "int"),
                    SchemaField::new("viewport_h", "int"),
                    SchemaField::new("measured_count", "int"),
                    SchemaField::new("is_fully_measured", "bool"),
                    SchemaField::new("total_height", "int"),
                    SchemaField::parametric(
                        "measured_height.<row>",
                        "int",
                        const { &[SchemaArg::index("row", "item_count")] },
                    ),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "item_count" => Some(IntrospectValue::Int(int_of(self.measured.item_count()))),
            "estimated" => Some(IntrospectValue::Int(i64::from(EST))),
            "viewport_h" => Some(IntrospectValue::Int(i64::from(VIEWPORT_H))),
            "measured_count" => Some(IntrospectValue::Int(int_of(self.measured.measured_count()))),
            "is_fully_measured" => Some(IntrospectValue::Bool(self.measured.is_fully_measured())),
            "total_height" => Some(IntrospectValue::Int(i64::from(
                self.measured.total_height(),
            ))),
            // `measured_height.<row>` — one row's harvested height, or null
            // while it is still counted at the estimate. Guarded against
            // `item_count` so the declared index domain is true (R1353.1: an
            // unguarded parametric path fabricates answers for rows that do
            // not exist).
            _ => {
                let row = path
                    .strip_prefix("measured_height.")
                    .and_then(|seg| seg.parse::<usize>().ok())?;
                if row >= self.measured.item_count() {
                    return None;
                }
                Some(match self.measured.measured_height(row) {
                    Some(h) => IntrospectValue::Int(i64::from(h)),
                    None => IntrospectValue::Null,
                })
            }
        }
    }

    /// Display-only: no writable state.
    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(InterveneError::UnknownPath)
    }
}

struct TailRevealView;

impl WidgetCore for TailRevealView {
    type State = Controls;
    /// Every state change arrives as the Reply button's `"click"` intent
    /// (pointer) or through `apply_key` (keyboard); the shell's enum-typed
    /// keybinding channel is unused, so `()` satisfies the trait's `Copy`
    /// bound without an inhabited-but-dead event variant.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        // Touch the shared slots once at boot so the view, the reducer and this
        // External resolve the same `Rc`s before the first paint.
        let src = use_transcript();
        let _scroll = use_scroll_state(SCROLL_TAG);
        Box::new(TailRevealExternal {
            measured: use_measured_rows(MEASURED_KEY, src.count.get(), EST),
        })
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            REPLY_TAG,
            Box::new(ButtonExternal::new()),
        )]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    fn read_state(scene: &Scene) -> Controls {
        Controls {
            reply: read_button_state(scene, REPLY_TAG),
            reply_focused: read_button_focused(scene, REPLY_TAG),
        }
    }

    fn view(state: Controls, frame: &Frame) -> Scene {
        view(state, frame)
    }

    /// R1458 — the append policy. Runs in an Owner scope, so it resolves the
    /// same `use_transcript` / `use_scroll_state` / `use_measured_rows` slots
    /// the view does; it computes no scroll bound.
    fn update(_state: Controls, intent: &Intent) -> Vec<Command> {
        if intent.tag_str() == REPLY_INTENT {
            let src = use_transcript();
            let count = src.count.get();
            apply_reply(
                &src,
                &use_scroll_state(SCROLL_TAG),
                &use_measured_rows(MEASURED_KEY, count, EST),
            );
        }
        Vec::new()
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// ARIA button activation: Space / Enter on the focused control, in parity
    /// with a pointer click.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, REPLY_TAG)
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn title() -> &'static str {
        "pinion hello-tail-reveal (R1458 §5.45 §5.27)"
    }

    fn fmt_state_log(state: &Controls) -> String {
        format!("reply={:?}", state.reply)
    }
}

impl WidgetA11y for TailRevealView {
    /// The transcript's rendered children are a *window* onto the dataset, so
    /// each row carries its dataset position rather than its position among
    /// the painted nodes (`windowed_list_nodes`, the measured list's a11y
    /// contract): an AT reading "entry 12 of 13" is reading the dataset, not
    /// the window. The Reply control and the `role=status` line ride alongside,
    /// so an AT is told the tail moved, not just that a button was pressed.
    fn access_node(state: &Controls, focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_TAG);
        let src = use_transcript();
        let count = src.count.get();
        let measured = use_measured_rows(MEASURED_KEY, count, EST);
        let window = compute_visible_range_variable(
            scroll.offset_y(),
            VIEWPORT_H,
            &measured.offsets(),
            OVERSCAN,
        );
        let mut nodes = windowed_list_nodes(
            LIST_TAG,
            "Windowed transcript",
            u32::try_from(count).unwrap_or(u32::MAX),
            &window,
        );
        nodes.push(
            AccessNode::new(REPLY_TAG, AriaRole::Button)
                .with_name("Reply")
                .with_state(button_a11y_state(state.reply, focused == Some(REPLY_TAG))),
        );
        nodes.push(
            AccessNode::new(STATUS_TAG, AriaRole::Status).with_name(status_line(
                count,
                at_bottom(scroll.offset_y(), scroll.max().1),
            )),
        );
        nodes
    }
}

impl WidgetView for TailRevealView {
    type Renderer = HelloTailRevealRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TailRevealView>();
}
