//! R827 §2 #4 §5.20 — first consumer of the immediate-mode -> retained
//! **intent bridge**.
//!
//! This is the bidirectional half of the §2 #4 dual-execution model.
//! [`hello-immediate-mode-canvas`](../hello-immediate-mode-canvas) (R681)
//! proved the *retained -> immediate* direction: a retained widget tree
//! hosts a [`Scene::ImmediateModeNode`] whose driver `tick`s + `paint`s
//! every per-window paint cycle. This binding proves the *immediate ->
//! retained* direction: the immediate-mode driver **emits §5.20 intents**
//! that the retained `V::update` reducer consumes — so the game loop
//! drives retained application state, the canonical thing a game loop
//! must do (a goal scored, an enemy spawned, a level cleared).
//!
//! ## The bridge
//!
//! - The [`BouncingBallDriver`] advances a 1-D position every
//!   [`ImmediateMode::tick`]. On each wall reflection it buffers one
//!   `bounce` [`Intent`]; [`ImmediateMode::drain_intents`] hands the
//!   buffer to the runtime.
//! - The substrate's per-window paint cycle, immediately after
//!   [`Scene::tick_immediate_mode`], runs
//!   `pinion_runtime::walk_scene_and_drain_immediate`, which prefixes
//!   each drained intent with the node's §5.20 tag (`ball` + `bounce` ->
//!   `ball.bounce`, the R22 `<widget>.<kind>` convention) and routes it
//!   through `V::update` — the same reducer arc retained widget intents
//!   flow through.
//! - [`BallView::update`] matches `ball.bounce` and increments a
//!   reactive [`Signal<u32>`] held in [`Owner::cache`]. The view fn reads
//!   the same Signal and renders `"Bounces: N"`, so the retained text
//!   readout grows as the game loop runs.
//!
//! ## AI-first observability (§2 #2 + §2 #7)
//!
//! The retained `Bounces:` readout is plain scene-as-data: an AI client
//! reads it through `scene/snapshot` and watches the count climb as the
//! immediate-mode driver bounces — observing game-loop-driven state
//! change entirely through the headless RPC surface. See
//! `tools/demos/r827_immediate_intent.py`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use pinion_core::external::{ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue};
use pinion_core::scene::{
    ContainerNode, ImmediateMode, ImmediateModeNode, ImmediatePainter, Rect, TextNode,
};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Frame, Intent, Owner, Scene, Signal, WidgetCore};
#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output. Defines `HelloImmediateIntentRenderer`
// + its error enum + async `new` + sync `render` / `resize`. R46.3.3
// emit template uses fully-qualified `::vello::*` paths so the include
// is bare.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 bridge from the inherent renderer methods to the
// `pinion_shell::VelloRenderer` trait.
vello_renderer_impl!(HelloImmediateIntentRenderer, HelloImmediateIntentRendererError);

/// (R827 §2 #4) Window size — header + immediate-mode track + bounce
/// readout + dismiss trigger.
const WIN_W: u32 = 340;
const WIN_H: u32 = 360;

/// (R827 §2 #4) Immediate-mode track viewport (logical pixels).
const CANVAS_VIEWPORT_W: u32 = 300;
const CANVAS_VIEWPORT_H: u32 = 140;

const HEADER_FONT_PX: u32 = 16;
const BODY_FONT_PX: u32 = 14;
const BTN_W: u32 = 160;
const BTN_H: u32 = 36;
const ROW_GAP: u32 = 14;
const OUTER_PAD: u32 = 20;

/// (R827 §2 #4) Ball speed in track-fractions per second. At `2.5` the
/// ball crosses the full track (one wall reflection) every `~0.4s`, so
/// the continuous ~60fps game loop produces a bounce a couple of times
/// a second — fast enough that the RPC demo observes the retained
/// bounce count climbing within a short wall-clock window.
const BALL_SPEED: f32 = 2.5;

/// Shared `ThemeProvider` cache tag matching the `"app"` convention.
const THEME_TAG: &str = "app";

/// (R827 §2 #4) `Owner::cache` slot key for the bouncing-ball driver.
const BALL_DRIVER_CACHE_KEY: &str = "hello_immediate_intent.driver";

/// (R827 §2 #4) `Owner::cache` slot key for the retained bounce counter
/// the reducer increments and the view fn reads.
const BOUNCE_COUNT_CACHE_KEY: &str = "hello_immediate_intent.bounce_count";

/// (R830 §5.20) `Owner::cache` slot key for the retained *click* counter
/// — the reducer increments it from `ball.click` intents the driver emits
/// on [`ImmediateMode::on_pointer_down`] (the R830 input channel). It
/// exercises the R830 paused-window intent-drain path: a click on a
/// paused window must still route its intent to `V::update`.
const CLICK_COUNT_CACHE_KEY: &str = "hello_immediate_intent.click_count";

/// (R827 §5.20) The unprefixed intent kind the driver emits on a wall
/// reflection. The runtime walk prefixes it with [`BALL_NODE_TAG`].
const BOUNCE_EVENT: &str = "bounce";

/// (R830 §5.20) The unprefixed intent kind the driver emits on a pointer
/// press, prefixed with [`BALL_NODE_TAG`] → `ball.click`.
const CLICK_EVENT: &str = "click";

/// (R827 §5.20 R22) Tags on the paint tree. [`BALL_NODE_TAG`] is the
/// immediate-mode node's §5.20 intent tag (so its drained `bounce`
/// becomes `ball.bounce`); [`DISMISS_BTN_TAG`] is the retained Button's
/// tag (`<BallView as WidgetCore>::tag()`); [`COUNT_READOUT_TAG`] /
/// [`CLICK_READOUT_TAG`] tag the retained text the demo reads;
/// [`VIEW_TAG`] tags the root.
const BALL_NODE_TAG: &str = "ball";
const DISMISS_BTN_TAG: &str = "dismiss_btn";
const COUNT_READOUT_TAG: &str = "bounce_readout";
const CLICK_READOUT_TAG: &str = "click_readout";
const VIEW_TAG: &str = "ball_view";

/// (R827 §5.20 R22) The fully-prefixed wire tag the reducer matches —
/// `BALL_NODE_TAG` + `.` + `BOUNCE_EVENT`. Built through the
/// `intent_tag!` SSOT so the dotted form cannot drift from the node tag
/// / event-kind constants. [[intent-tag-dotted-wire-form]].
const BOUNCE_INTENT_TAG: &str = pinion_core::intent_tag!("ball", "bounce");

/// (R830 §5.20 R22) The fully-prefixed `ball.click` wire tag.
const CLICK_INTENT_TAG: &str = pinion_core::intent_tag!("ball", "click");

const HEADER_TEXT: &str = "Bouncing ball (R827 immediate -> retained intent)";
const BTN_LABEL: &str = "Dismiss";

/// R827 §2 #4 §5.20 — the binding's [`ImmediateMode`] driver. A 1-D
/// bouncing value: `pos` advances by `vel * dt` each [`tick`], reflects
/// at both walls, and buffers one `bounce` [`Intent`] per reflection for
/// [`drain_intents`] to hand back to the retained reducer.
///
/// [`tick`]: ImmediateMode::tick
/// [`drain_intents`]: ImmediateMode::drain_intents
#[derive(Debug)]
struct BouncingBallDriver {
    /// Position along the track, `[0.0, 1.0]` (`0` = left wall,
    /// `1` = right wall).
    pos: f32,
    /// Velocity in track-fractions per second. Sign is direction; the
    /// magnitude is fixed at [`BALL_SPEED`] (reflections flip the sign,
    /// never the speed).
    vel: f32,
    /// Cumulative wall reflections this driver has performed — its own
    /// simulation-side bounce tally, surfaced through the R828
    /// [`ImmediateMode::introspect`] channel (`scene/query
    /// "/ball/external/bounces"`). Distinct from the *retained* bounce
    /// count: the reducer derives that independently by counting the
    /// `ball.bounce` intents drained from this driver, so the two must
    /// converge — a live cross-check that the R827 write bridge and the
    /// R828 read channel observe the same events.
    bounces: u32,
    /// R830 §5.15 — viewport-local position of the most recent pointer
    /// press the framework forwarded to [`ImmediateMode::on_pointer_down`]
    /// (the player → game input channel), or `None` before the first
    /// click. Surfaced through the introspect channel so the demo
    /// verifies the coordinate translation, and used by `on_pointer_down`
    /// to flip the ball's direction (a click reverses travel).
    last_click: Option<(f32, f32)>,
    /// §5.20 intents emitted during the current frame's [`tick`], drained
    /// by the runtime immediately after via [`drain_intents`]. One
    /// `bounce` per wall reflection.
    ///
    /// [`tick`]: ImmediateMode::tick
    /// [`drain_intents`]: ImmediateMode::drain_intents
    pending: Vec<Intent>,
    /// Ball fill colour, published by the view fn from the live theme
    /// each frame. Default (black) is the pre-first-view bootstrap.
    fill: Color,
    /// Track-line colour, published by the view fn from the live theme.
    track: Color,
}

impl Default for BouncingBallDriver {
    fn default() -> Self {
        Self {
            pos: 0.0,
            vel: BALL_SPEED,
            bounces: 0,
            last_click: None,
            pending: Vec::new(),
            fill: Color::default(),
            track: Color::default(),
        }
    }
}

impl ImmediateMode for BouncingBallDriver {
    fn tick(&mut self, dt: Duration) {
        let dt_secs = dt.as_secs_f32();
        self.pos += self.vel * dt_secs;
        // Reflect at each wall, emitting one `bounce` §5.20 intent per
        // reflection. A `while` (not `if`) keeps `pos` inside `[0, 1]`
        // even when a large clamped `dt` (debugger-pause resume) crosses
        // a wall more than once; each reflection strictly reduces the
        // overshoot magnitude, so the loop always terminates.
        while !(0.0..=1.0).contains(&self.pos) {
            if self.pos < 0.0 {
                self.pos = -self.pos;
                self.vel = self.vel.abs();
            } else {
                self.pos = 2.0 - self.pos;
                self.vel = -self.vel.abs();
            }
            self.bounces = self.bounces.saturating_add(1);
            self.pending
                .push(Intent::new_static(BOUNCE_EVENT, IntrospectValue::Null));
        }
    }

    // R828 §2 #4 §5.12 — opt in to the §5.15 item 8 introspection
    // channel so an AI client reads live simulation state
    // (`scene/query "/ball/external/{pos,velocity,bounces}"`) through the
    // same surface it queries `Scene::External` widgets, preserving the
    // §2 #2 + §2 #7 scene-as-data invariant across the retained /
    // immediate boundary. First consumer of the R681-deferred channel.
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    fn paint(&mut self, painter: &mut dyn ImmediatePainter) {
        let (w_u, h_u) = painter.viewport_size();
        if w_u == 0 || h_u == 0 {
            return;
        }
        // Cast viewport size to f32 for geometry. Within the GUI surface
        // range (well under 2^24 px) the cast is exact; the `#[allow]`
        // documents the intentional precision waiver.
        #[allow(clippy::cast_precision_loss)]
        let w = w_u as f32;
        #[allow(clippy::cast_precision_loss)]
        let h = h_u as f32;
        let cy = h * 0.5;
        let margin = w * 0.08;
        let track_w = w - 2.0 * margin;
        // Horizontal track the ball travels along.
        painter.stroke_line((margin, cy), (margin + track_w, cy), 2.0, self.track);
        // Ball as a square centred on the track at the current position.
        let ball = w.min(h) * 0.18;
        let cx = margin + self.pos * track_w;
        painter.fill_rect(cx - ball * 0.5, cy - ball * 0.5, ball, ball, self.fill);
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        for intent in self.pending.drain(..) {
            sink(intent);
        }
    }

    // R830 §2 #4 §5.15 — player → game pointer input. A click reverses
    // the ball's travel direction (a trivially-observable game response)
    // and records the viewport-local press point so the demo can verify
    // the framework forwarded the coordinates correctly. `(x, y)` are
    // viewport-local logical pixels — the same space `paint` draws in.
    //
    // R830 §5.20 — it ALSO emits a `click` §5.20 intent: a press is a
    // second intent producer independent of `tick`, so this exercises the
    // paused-window intent-drain path (a click on a paused window must
    // still route its intent to the retained `V::update`).
    fn on_pointer_down(&mut self, x: f32, y: f32) {
        self.vel = -self.vel;
        self.last_click = Some((x, y));
        self.pending
            .push(Intent::new_static(CLICK_EVENT, IntrospectValue::Null));
    }
}

/// R828 §2 #4 §5.12 — the driver's §5.15 item 8 introspection surface.
/// Read-only: `pos` / `velocity` / `bounces` are all owned by the
/// `tick` simulation (the game loop is the single writer), so `intervene`
/// rejects every write with [`InterveneError::ReadOnly`] — a raw poke
/// would desync state from the simulation (same contract as the R823
/// tree / R810 snackbar read-only introspects, whose state is event- /
/// timer-driven rather than RPC-driven).
impl ExternalIntrospect for BouncingBallDriver {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("pos", "float"),
            ("velocity", "float"),
            ("bounces", "int"),
            ("clicked", "bool"),
            ("last_click_x", "float"),
            ("last_click_y", "float"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "pos" => Some(IntrospectValue::Float(f64::from(self.pos))),
            "velocity" => Some(IntrospectValue::Float(f64::from(self.vel))),
            "bounces" => Some(IntrospectValue::Int(i64::from(self.bounces))),
            // R830 — the forwarded player-input state (Null before the
            // first click, so the parametric paths are still schema-
            // declared but resolve to "no value yet").
            "clicked" => Some(IntrospectValue::Bool(self.last_click.is_some())),
            "last_click_x" => Some(
                self.last_click
                    .map_or(IntrospectValue::Null, |(x, _)| IntrospectValue::Float(f64::from(x))),
            ),
            "last_click_y" => Some(
                self.last_click
                    .map_or(IntrospectValue::Null, |(_, y)| IntrospectValue::Float(f64::from(y))),
            ),
            _ => None,
        }
    }

    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(InterveneError::ReadOnly)
    }
}

/// (R827 §2 #4) [`Owner::cache`] hook for the ball driver. Pulls the
/// same `Rc<RefCell<BouncingBallDriver>>` from the cache slot on every
/// view-fn call so the per-frame tick advances the same state instance
/// across the binding lifetime.
fn use_ball_driver() -> Rc<RefCell<BouncingBallDriver>> {
    let owner = Owner::current().expect("use_ball_driver must run inside Owner::run wrap (view fn)");
    owner.cache(BALL_DRIVER_CACHE_KEY, || {
        RefCell::new(BouncingBallDriver::default())
    })
}

/// (R827 §2 #4) [`Owner::cache`] hook for the retained bounce counter.
/// The reducer ([`BallView::update`]) increments it when a `ball.bounce`
/// intent arrives from the immediate-mode driver; the view fn reads it
/// to render the `Bounces:` readout. Both run under the same
/// `root_owner` scope so they resolve the same Signal — the immediate ->
/// retained bridge's shared retained state.
fn use_bounce_count() -> Rc<Signal<u32>> {
    let owner =
        Owner::current().expect("use_bounce_count must run inside Owner::run wrap (view / reducer)");
    owner.cache(BOUNCE_COUNT_CACHE_KEY, || Signal::new(0_u32))
}

/// (R830 §5.20) [`Owner::cache`] hook for the retained *click* counter.
/// The reducer increments it from `ball.click` intents the driver emits
/// on a pointer press (R830 input → R827 intent bridge), proving the
/// paused-window intent-drain path: a click on a paused window still
/// routes its intent to `V::update`.
fn use_click_count() -> Rc<Signal<u32>> {
    let owner =
        Owner::current().expect("use_click_count must run inside Owner::run wrap (view / reducer)");
    owner.cache(CLICK_COUNT_CACHE_KEY, || Signal::new(0_u32))
}

/// view-fn (§6.3) — pure sync mapping `(ButtonState) -> Scene`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);
    let accent = theme.resolve(ColorRole::Accent);

    // Publish the live palette into the driver so the next paint uses
    // theme colours. Mutates behind the `RefCell`; the substrate's tick
    // walker takes the `borrow_mut` immediately after this fn returns.
    let driver = use_ball_driver();
    {
        let mut d = driver.borrow_mut();
        d.fill = accent;
        d.track = on_surface_muted;
    }

    // Read the retained bounce count the reducer has accumulated from
    // immediate-mode `ball.bounce` intents — the bridge's observable.
    let bounces = use_bounce_count().get();
    // R830 — the retained click count, accumulated from `ball.click`
    // intents the driver emits on a pointer press.
    let clicks = use_click_count().get();

    let header = Scene::Text(TextNode::styled(
        HEADER_TEXT,
        Rect::default(),
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_fg(on_surface),
    ));

    // ImmediateModeNode — the §2 #4 immediate-mode subtree. The
    // §5.20 tag makes its drained `bounce` intents arrive as
    // `ball.bounce` at the reducer.
    let handle: Rc<RefCell<dyn ImmediateMode>> = driver;
    let canvas = Scene::ImmediateModeNode(
        ImmediateModeNode::new(handle, Rect::default())
            .with_tag(BALL_NODE_TAG)
            .with_layout(LayoutStyle::new().with_size(Size::px(CANVAS_VIEWPORT_W, CANVAS_VIEWPORT_H))),
    );

    let readout = Scene::Text(
        TextNode::styled(
            format!("Bounces: {bounces}"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(BODY_FONT_PX)
                .with_fg(on_surface),
        )
        .with_tag(COUNT_READOUT_TAG),
    );

    let click_readout = Scene::Text(
        TextNode::styled(
            format!("Clicks: {clicks}"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(BODY_FONT_PX)
                .with_fg(on_surface_muted),
        )
        .with_tag(CLICK_READOUT_TAG),
    );

    // Dismiss Button — `ButtonState` mapped to M3 surface tier +
    // state-layer overlay (same convention hello-button uses).
    let btn_fill: Color = match state {
        ButtonState::Idle | ButtonState::Hover => theme.resolve(ColorRole::SurfaceContainerHighest),
        ButtonState::Pressed => theme
            .resolve(ColorRole::SurfaceContainerHighest)
            .lerp(on_surface, 0.12),
        ButtonState::Disabled => theme
            .resolve(ColorRole::SurfaceContainerHighest)
            .lerp(surface, 0.38),
    };
    let btn_label = Scene::Text(TextNode::styled(
        BTN_LABEL,
        Rect::default(),
        TextStyle::new()
            .with_size_px(BODY_FONT_PX)
            .with_fg(on_surface),
    ));
    let button = Scene::Container(
        ContainerNode::new(vec![btn_label])
            .with_tag(DISMISS_BTN_TAG)
            .with_style(BoxStyle::filled(btn_fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(BTN_W, BTN_H)),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![header, canvas, readout, click_readout, button])
            .with_tag(VIEW_TAG)
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP)
                    .with_padding(Rect::new(OUTER_PAD, OUTER_PAD, OUTER_PAD, OUTER_PAD))
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

/// `WidgetView` binding. The `#[widget]` attribute lifts the mechanical
/// trait wiring (`WidgetCore` + `WidgetA11y` + `WidgetView` forwarding
/// trio + the R27 `update` reducer hook); only the view body, keybinding
/// shim, ARIA apply_key, and the bounce reducer remain inherent.
#[widget(
    tag = "dismiss_btn",
    state = ButtonState,
    event = ButtonEvent,
    title = "pinion hello-immediate-intent (R827 §2 #4 §5.20)",
    renderer = HelloImmediateIntentRenderer,
    initial_size = (WIN_W, WIN_H),
    external = ButtonExternal::new,
    role = Button,
    state_flags(
        hovered = Hover,
        pressed = Pressed,
        disabled = Disabled,
    ),
    apply_key,
    keybinding,
    update,
    state_name_derive,
)]
struct BallView;

impl BallView {
    /// R641 inherent forward for [`WidgetCore::view`].
    fn view(state: ButtonState, frame: Frame) -> Scene {
        view(state, &frame)
    }

    /// Keybinding shim mirroring hello-button.
    fn keybinding(key: &str) -> Option<ButtonEvent> {
        match key {
            "d" => Some(ButtonEvent::Disable),
            "e" => Some(ButtonEvent::Enable),
            _ => None,
        }
    }

    /// ARIA Button keyboard activation — Space / Enter on the focused
    /// button fires the SCXML `KeyboardActivate` transition.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        pinion_core::widgets::aria::apply_aria_activate(scene, focused, key, Self::tag())
    }

    /// R827 / R830 §2 #4 §5.20 — the immediate -> retained reducer. Each
    /// `ball.bounce` intent (driver wall reflection) bumps the retained
    /// bounce counter; each `ball.click` intent (driver pointer press,
    /// R830 input) bumps the retained click counter. Both are drained by
    /// the runtime and routed here through `route_intent_through_update`.
    /// Runs under `root_owner`, so the `use_*_count` Signals resolve the
    /// same instances the view fn reads. Returns no `Command`s — the side
    /// effect is the reactive Signal mutation, which marks the owner dirty
    /// so the next frame re-renders the updated readouts.
    fn update(_state: ButtonState, intent: &Intent) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            BOUNCE_INTENT_TAG => {
                let count = use_bounce_count();
                count.set(count.get() + 1);
            }
            CLICK_INTENT_TAG => {
                let count = use_click_count();
                count.set(count.get() + 1);
            }
            _ => {}
        }
        Vec::new()
    }
}

fn main() {
    pinion_shell::run::<BallView>();
}

#[cfg(test)]
mod r827_immediate_intent_tests {
    use super::*;
    use pinion_core::scene::Scene as PinionScene;

    #[test]
    fn r827_widget_tag_matches_paint_tag() {
        assert_eq!(<BallView as WidgetCore>::tag(), DISMISS_BTN_TAG);
    }

    #[test]
    fn r827_bounce_intent_tag_is_node_tag_dot_event() {
        // The reducer-matched wire tag must equal the runtime-prefixed
        // form: node tag + `.` + emitted event kind.
        assert_eq!(BOUNCE_INTENT_TAG, "ball.bounce");
        assert_eq!(BOUNCE_INTENT_TAG, format!("{BALL_NODE_TAG}.{BOUNCE_EVENT}"));
    }

    #[test]
    fn r827_access_node_is_aria_button() {
        let nodes = <BallView as WidgetA11y>::access_node(&ButtonState::Idle, None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Button);
        assert_eq!(nodes[0].tag, DISMISS_BTN_TAG);
    }

    #[test]
    fn r827_view_root_carries_composite_paint_root_tag() {
        let owner = Owner::new();
        let scene = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        assert!(scene.contains_tag(DISMISS_BTN_TAG));
    }

    #[test]
    fn r827_view_contains_immediate_mode_subtree() {
        let owner = Owner::new();
        let scene = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        assert!(
            scene.has_immediate_mode_subtree(),
            "view fn must emit a Scene::ImmediateModeNode",
        );
        assert!(scene.contains_tag(BALL_NODE_TAG));
    }

    #[test]
    fn r827_driver_emits_one_bounce_per_wall_reflection() {
        // Start at the left wall moving right. A tick large enough to
        // cross the whole track reflects once at the right wall and
        // buffers exactly one bounce intent.
        let mut driver = BouncingBallDriver::default();
        assert_eq!(driver.pos.to_bits(), 0.0_f32.to_bits());
        // dt = 1/BALL_SPEED seconds advances pos by exactly 1.0 -> sits
        // on the right wall (no reflection yet at the boundary).
        driver.tick(Duration::from_secs_f32(1.0 / BALL_SPEED));
        let mut drained: Vec<Intent> = Vec::new();
        driver.drain_intents(&mut |i| drained.push(i));
        assert!(
            drained.is_empty(),
            "pos exactly at the wall does not reflect; got {drained:?}",
        );
        // One more small step crosses the right wall -> one reflection,
        // one bounce intent, velocity now negative, pos back inside.
        driver.tick(Duration::from_secs_f32(0.1));
        let mut drained2: Vec<Intent> = Vec::new();
        driver.drain_intents(&mut |i| drained2.push(i));
        assert_eq!(drained2.len(), 1, "one reflection -> one bounce");
        assert_eq!(drained2[0].tag_str(), BOUNCE_EVENT);
        assert!(driver.vel < 0.0, "velocity reversed after right-wall bounce");
        assert!(
            (0.0..=1.0).contains(&driver.pos),
            "pos stays inside the track after reflection; got {}",
            driver.pos,
        );
    }

    #[test]
    fn r827_driver_huge_dt_stays_in_bounds_and_emits_multiple_bounces() {
        let mut driver = BouncingBallDriver::default();
        // A huge dt crosses many walls; the while-loop reflects each
        // crossing and keeps pos inside [0, 1].
        driver.tick(Duration::from_secs(10));
        let mut drained: Vec<Intent> = Vec::new();
        driver.drain_intents(&mut |i| drained.push(i));
        assert!(
            drained.len() >= 2,
            "10s at BALL_SPEED crosses several walls; got {} bounces",
            drained.len(),
        );
        assert!(
            (0.0..=1.0).contains(&driver.pos),
            "pos stays bounded under a huge dt; got {}",
            driver.pos,
        );
    }

    #[test]
    fn r828_driver_introspect_exposes_pos_velocity_bounces() {
        let mut driver = BouncingBallDriver::default();
        // Advance past the right wall to register one bounce.
        driver.tick(Duration::from_secs_f32(1.0 / BALL_SPEED + 0.1));
        assert!(driver.bounces >= 1);
        let intro = driver.introspect().expect("driver opts into introspection");
        // `bounces` mirrors the driver's own simulation tally.
        assert_eq!(
            intro.query("bounces"),
            Some(IntrospectValue::Int(i64::from(driver.bounces))),
        );
        // pos / velocity surface as floats; unknown paths are None.
        assert!(matches!(intro.query("pos"), Some(IntrospectValue::Float(_))));
        assert!(matches!(intro.query("velocity"), Some(IntrospectValue::Float(_))));
        assert_eq!(intro.query("ghost"), None);
        // Schema declares the simulation fields plus the R830 forwarded-
        // input fields.
        assert_eq!(
            intro.schema().fields,
            &[
                ("pos", "float"),
                ("velocity", "float"),
                ("bounces", "int"),
                ("clicked", "bool"),
                ("last_click_x", "float"),
                ("last_click_y", "float"),
            ],
        );
    }

    #[test]
    fn r828_driver_introspect_intervene_is_read_only() {
        let mut driver = BouncingBallDriver::default();
        let intro = driver.introspect_mut().expect("driver opts into introspection");
        assert!(matches!(
            intro.intervene("pos", IntrospectValue::Float(0.5)),
            Err(InterveneError::ReadOnly),
        ));
    }

    #[test]
    fn r830_on_pointer_down_reverses_direction_and_records_click() {
        let mut driver = BouncingBallDriver::default();
        let v0 = driver.vel;
        assert!(driver.last_click.is_none(), "no click before first press");
        driver.on_pointer_down(12.5, 7.0);
        // A click reverses travel: vel == -v0 (so vel + v0 == 0).
        assert!((driver.vel + v0).abs() < 1e-6, "click must reverse velocity");
        let (lx, ly) = driver.last_click.expect("click recorded");
        assert!((lx - 12.5).abs() < 1e-6 && (ly - 7.0).abs() < 1e-6, "viewport-local coords stored");
    }

    #[test]
    fn r830_on_pointer_down_emits_click_intent() {
        // R830 — a press is an intent producer independent of `tick`; the
        // emitted `click` (prefixed `ball.click`) is what the paused
        // intent-drain fix routes to the reducer.
        let mut driver = BouncingBallDriver::default();
        driver.on_pointer_down(1.0, 1.0);
        let mut drained: Vec<Intent> = Vec::new();
        driver.drain_intents(&mut |i| drained.push(i));
        assert_eq!(drained.len(), 1, "one press -> one click intent");
        assert_eq!(drained[0].tag_str(), CLICK_EVENT);
        assert_eq!(CLICK_INTENT_TAG, "ball.click");
    }

    #[test]
    fn r830_introspect_surfaces_forwarded_click() {
        let mut driver = BouncingBallDriver::default();
        // Null / false before the first press (borrow scoped so the
        // mutating press below can take `&mut driver`).
        {
            let intro = driver.introspect().expect("opt-in");
            assert_eq!(intro.query("clicked"), Some(IntrospectValue::Bool(false)));
            assert_eq!(intro.query("last_click_x"), Some(IntrospectValue::Null));
        }
        driver.on_pointer_down(40.0, 18.0);
        let intro = driver.introspect().expect("opt-in");
        assert_eq!(intro.query("clicked"), Some(IntrospectValue::Bool(true)));
        assert_eq!(intro.query("last_click_x"), Some(IntrospectValue::Float(40.0)));
        assert_eq!(intro.query("last_click_y"), Some(IntrospectValue::Float(18.0)));
    }

    #[test]
    fn r827_reducer_counts_bounce_intent() {
        // Seed the cache (view fn) then drive the reducer with a
        // `ball.bounce` intent under the same Owner — the bounce count
        // must climb by one per matching intent and ignore others.
        let owner = Owner::new();
        owner.run(|| {
            let _ = view(ButtonState::Idle, &Frame::new());
            assert_eq!(use_bounce_count().get(), 0);

            let bounce = Intent::new_static(BOUNCE_INTENT_TAG, IntrospectValue::Null);
            let _ = BallView::update(ButtonState::Idle, &bounce);
            assert_eq!(use_bounce_count().get(), 1, "ball.bounce increments");

            let _ = BallView::update(ButtonState::Idle, &bounce);
            assert_eq!(use_bounce_count().get(), 2, "second bounce");

            // A non-bounce intent must NOT touch the counter.
            let other = Intent::new_static("dismiss_btn.click", IntrospectValue::Null);
            let _ = BallView::update(ButtonState::Idle, &other);
            assert_eq!(use_bounce_count().get(), 2, "unrelated intent ignored");
        });
    }

    #[test]
    fn r827_use_ball_driver_returns_same_handle_across_view_runs() {
        let owner = Owner::new();
        let scene_a = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        let scene_b = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        let a = first_immediate_handle(&scene_a).expect("scene_a has ImmediateModeNode");
        let b = first_immediate_handle(&scene_b).expect("scene_b has ImmediateModeNode");
        assert!(
            Rc::ptr_eq(&a, &b),
            "Owner::cache must return the same driver Rc across view-fn re-runs",
        );
    }

    #[test]
    fn r827_painted_scene_layout_runs_through_immediate_node_viewport() {
        let owner = Owner::new();
        let mut scene = owner.run(|| view(ButtonState::Idle, &Frame::new()));
        let mut text_cache = pinion_text::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut text_cache, WIN_W, WIN_H);
        let node = walk_first_immediate_node(&scene).expect("painted scene has ImmediateModeNode");
        assert!(
            node.viewport.w > 0 && node.viewport.h > 0,
            "post-layout viewport must be non-zero; got {:?}",
            node.viewport,
        );
        assert!(node.viewport.w <= CANVAS_VIEWPORT_W);
        assert!(node.viewport.h <= CANVAS_VIEWPORT_H);
    }

    fn first_immediate_handle(scene: &PinionScene) -> Option<Rc<RefCell<dyn ImmediateMode>>> {
        walk_first_immediate_node(scene).map(|n| n.handle.clone())
    }

    fn walk_first_immediate_node(scene: &PinionScene) -> Option<&ImmediateModeNode> {
        match scene {
            PinionScene::ImmediateModeNode(n) => Some(n),
            PinionScene::Container(c) => c.children.iter().find_map(walk_first_immediate_node),
            PinionScene::Scroll(s) => walk_first_immediate_node(&s.content),
            _ => None,
        }
    }
}
