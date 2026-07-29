//! `hello-answer-origin` — R1482 §5.12 §2 #7 §2 #2.
//!
//! A binding whose three introspectable surfaces answer `scene/query`
//! identically, and mean three different things.
//!
//! `scene/query` resolves against the retained **state scene** first and, when
//! that scene has nothing at the path, retries against the **last painted
//! frame** (R828, so a live game-loop driver is readable at all). The retry is
//! not a detail an agent can ignore, because what it reaches is not one kind of
//! thing:
//!
//!   * `model` lives in the state scene. That scene is not rebuilt per frame,
//!     so its answer is the model's current value.
//!   * `sim` is a [`Scene::ImmediateModeNode`] the view fn emits. Its handle is
//!     the same `Rc` the tick loop drives, so its answer is current simulation
//!     state — the case the fallback exists for.
//!   * `probe` is a plain [`Scene::External`] the view fn **rebuilds every
//!     paint**. Its answer is the value that node carried *as of the last
//!     painted frame*.
//!
//! Before R1482 all three produced a bare `{"result": <value>}`, so a client
//! holding an answer could not tell which contract it had — and the third one
//! stops tracking the app the moment painting stops. Asking for
//! `params.with_origin` puts the answer and its provenance in the *same*
//! reply, which a second round-trip could not do: between two calls the painted
//! frame can be replaced, so a separately-fetched provenance may describe a
//! frame the value never came from.
//!
//! The origin word is checkable against something independent: it predicts the
//! **write** contract R1481 settled. `state` and `paint_driver` name surfaces a
//! write reaches; `paint_frame` names a `Box` the next paint discards, which
//! `scene/intervene` refuses rather than silently dropping. An agent that reads
//! the origin therefore knows whether acting on what it just read is possible,
//! without trying it.
//!
//! Run it: `cargo run -p hello-answer-origin`. The window states each surface's
//! origin and whether it is writable; the RPC half is the interesting one.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, SchemaField,
    query_proxy_external_impl, read_only_or_unknown,
};
use pinion_core::reactive::Owner;
use pinion_core::scene::{
    ContainerNode, ExternalNode, ImmediateMode, ImmediateModeNode, ImmediatePainter, Rect, Scene,
    TextNode,
};
use pinion_core::style::{BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextStyle};
use pinion_core::widgets::button::{ButtonEvent, ButtonState};
use pinion_core::{Frame as PaintFrame, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(AnswerOriginRenderer, AnswerOriginRendererError);

const WIN_W: u32 = 760;
const WIN_H: u32 = 400;

/// The binding's own tag — [`WidgetCore::tag`], so this is the node
/// `CoreShell` puts in the **state scene**.
const MODEL_TAG: &str = "model";
/// The immediate-mode subtree the view fn emits, reachable only in a
/// painted frame.
const SIM_TAG: &str = "sim";
/// The retained node the view fn rebuilds every paint, likewise reachable
/// only in a painted frame — and a different kind of thing from `sim`.
const PROBE_TAG: &str = "probe";

const SIM_CACHE_KEY: &str = "answer_origin.sim_driver";
const CANVAS_W: u32 = 300;
const CANVAS_H: u32 = 48;

// ---------------------------------------------------------------------------
// state scene — the retained model
// ---------------------------------------------------------------------------

/// The binding's authoritative counter. Reached through the state scene, so
/// its answer is never as-of-a-frame: this node outlives every paint.
#[derive(Debug, Default)]
struct Model {
    ticks: i64,
}

query_proxy_external_impl!(Model);

impl ExternalIntrospect for Model {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(const { &[SchemaField::new("ticks", "int")] })
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        (path == "ticks").then_some(IntrospectValue::Int(self.ticks))
    }

    /// Writable — which is half of what the `state` origin word promises.
    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match (path, value) {
            ("ticks", IntrospectValue::Int(n)) => {
                self.ticks = n;
                Ok(())
            }
            ("ticks", _) => Err(InterveneError::TypeMismatch),
            _ => Err(read_only_or_unknown(&self.schema(), path)),
        }
    }
}

// ---------------------------------------------------------------------------
// painted frame — a live driver
// ---------------------------------------------------------------------------

/// A minimal game-loop driver. Nothing here is about the simulation; what
/// matters is that the view fn hands out the **same** `Rc` every paint, so a
/// read through the painted frame observes the object the tick loop is
/// advancing rather than a copy of it.
#[derive(Debug, Default)]
struct Sim {
    /// Advanced by [`ImmediateMode::tick`], so it moves without any RPC.
    frames: i64,
    /// Writable, so the demo can prove a `paint_driver` answer names a
    /// surface a write actually reaches.
    speed: i64,
    fill: Color,
}

impl ImmediateMode for Sim {
    fn tick(&mut self, _dt: Duration) {
        self.frames = self.frames.saturating_add(1);
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    fn paint(&mut self, painter: &mut dyn ImmediatePainter) {
        let (w, h) = painter.viewport_size();
        if w == 0 || h == 0 {
            return;
        }
        // Cast is exact for any GUI surface (well under 2^24 px).
        #[allow(clippy::cast_precision_loss)]
        let (w, h) = (w as f32, h as f32);
        painter.fill_rect(0.0, h * 0.4, w, h * 0.2, self.fill);
    }
}

impl ExternalIntrospect for Sim {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("frames", "int"),
                    SchemaField::new("speed", "int"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "frames" => Some(IntrospectValue::Int(self.frames)),
            "speed" => Some(IntrospectValue::Int(self.speed)),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match (path, value) {
            ("speed", IntrospectValue::Int(n)) => {
                self.speed = n;
                Ok(())
            }
            ("speed", _) => Err(InterveneError::TypeMismatch),
            _ => Err(read_only_or_unknown(&self.schema(), path)),
        }
    }
}

/// [`Owner::cache`] hook for the driver, so every paint emits the same handle.
fn use_sim() -> Rc<RefCell<Sim>> {
    let owner = Owner::current().expect("use_sim must run inside an Owner::run wrap (view fn)");
    owner.cache(SIM_CACHE_KEY, || RefCell::new(Sim::default()))
}

// ---------------------------------------------------------------------------
// painted frame — a per-frame copy
// ---------------------------------------------------------------------------

/// A retained surface the view fn **constructs fresh on every paint**. It is
/// reachable exactly like the other two and answers exactly like them, and its
/// value is a property of the frame it was built into — which is the fact the
/// wire could not previously state.
#[derive(Debug)]
struct Probe {
    /// The value the view fn stamped in when it built this node.
    stamped: i64,
}

query_proxy_external_impl!(Probe);

impl ExternalIntrospect for Probe {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(const { &[SchemaField::new("stamped", "int")] })
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        (path == "stamped").then_some(IntrospectValue::Int(self.stamped))
    }

    /// Declared read-only, but that is not what makes it unwritable over the
    /// wire: the R1481 fallback refuses a painted retained node before this
    /// method is ever consulted, because a write into a `Box` the next paint
    /// discards would be silently lost. Refusing at the surface too keeps the
    /// declaration honest for any caller holding the node directly.
    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(read_only_or_unknown(&self.schema(), path))
    }
}

// ---------------------------------------------------------------------------
// view
// ---------------------------------------------------------------------------

fn line(text: &str, size: u32, fg: Color) -> Scene {
    let mut style = TextStyle::new();
    style.fg_color = fg;
    style.font_size_px = size;
    Scene::Text(TextNode::styled(text, Rect::default(), style))
}

/// view-fn (§6.3). Emits the two painted-frame surfaces and reports, on screen,
/// what each of the three answers *is* — so the binding states its own
/// introspection contract without a client attached (§2 #7).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: ButtonState, _frame: &PaintFrame) -> Scene {
    let sim = use_sim();
    {
        let mut s = sim.borrow_mut();
        s.fill = Color::rgb(0x1B, 0x5E, 0x20);
    }
    // The stamp is a property of THIS frame: the value is fixed when the node
    // is built and cannot change until the next paint rebuilds it.
    let stamped = sim.borrow().frames;

    let handle: Rc<RefCell<dyn ImmediateMode>> = sim;

    Scene::Container(
        ContainerNode::new(vec![
            line("answer origin", 22, Color::rgb(0x14, 0x14, 0x14)),
            line(
                "three surfaces, one wire shape — ask with_origin to tell them apart",
                14,
                Color::rgb(0x44, 0x44, 0x44),
            ),
            line(
                "model/external/ticks    origin=state         writable",
                14,
                Color::rgb(0x1B, 0x5E, 0x20),
            ),
            line(
                "sim/external/frames     origin=paint_driver  writable",
                14,
                Color::rgb(0x1B, 0x5E, 0x20),
            ),
            line(
                "probe/external/stamped  origin=paint_frame   as of this frame",
                14,
                Color::rgb(0x88, 0x50, 0x00),
            ),
            Scene::ImmediateModeNode(
                ImmediateModeNode::new(handle, Rect::default())
                    .with_tag(SIM_TAG)
                    .with_layout(LayoutStyle::new().with_size(Size::px(CANVAS_W, CANVAS_H))),
            ),
            Scene::External(
                ExternalNode::new(Box::new(Probe { stamped })).with_tag(PROBE_TAG.to_owned()),
            ),
        ])
        .with_style(BoxStyle::filled(Color::rgb(0xFA, 0xFA, 0xFA)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_padding(Rect::new(24, 24, 24, 24))
                .with_gap(10),
        ),
    )
}

struct AnswerOrigin;

impl WidgetCore for AnswerOrigin {
    type State = ButtonState;
    type Event = ButtonEvent;
    fn create_external() -> Box<dyn External> {
        Box::new(Model::default())
    }
    fn tag() -> &'static str {
        MODEL_TAG
    }
    fn read_state(_scene: &Scene) -> Self::State {
        ButtonState::Idle
    }
    fn view(state: Self::State, frame: &PaintFrame) -> Scene {
        view(state, frame)
    }
    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }
    fn title() -> &'static str {
        "pinion answer origin demo"
    }
}

impl WidgetA11y for AnswerOrigin {}

impl WidgetView for AnswerOrigin {
    type Renderer = AnswerOriginRenderer;
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
    fn windows() -> Vec<WindowSpec> {
        vec![WindowSpec::new(
            "main",
            "pinion answer origin demo",
            SizeStrategy::Fixed {
                width: WIN_W,
                height: WIN_H,
            },
        )]
    }
}

fn main() {
    pinion_shell::run::<AnswerOrigin>();
}

#[cfg(test)]
mod tests {
    use super::{MODEL_TAG, Model, PROBE_TAG, Probe, SIM_TAG, Sim};
    use pinion_core::external::{ExternalIntrospect, IntrospectValue};
    use pinion_core::scene::{
        ContainerNode, ExternalNode, ImmediateMode, ImmediateModeNode, Rect, Scene,
    };
    use pinion_rpc::{AnswerOrigin, SceneSource, query_from};

    /// The primary external is the state scene's ROOT, so the `/external/...`
    /// short-circuit is its address; a tagged root is not reachable by walking
    /// to its own tag, which the demo depends on knowing.
    const MODEL_PATH: &str = "/external/ticks";
    use std::cell::RefCell;
    use std::rc::Rc;

    /// The state scene `CoreShell` composes for this binding — measured
    /// against the running app rather than assumed: with no extra externals
    /// the root IS the primary `Scene::External`, not a `Container` wrapping
    /// it. A fixture that wrapped it would still pass every origin assertion
    /// below while testing a shape this binding never produces.
    fn state_scene() -> Scene {
        Scene::External(
            ExternalNode::new(Box::new(Model::default())).with_tag(MODEL_TAG.to_owned()),
        )
    }

    /// The painted frame's two introspectable surfaces, in the same shape the
    /// view fn emits them.
    fn paint_scene(frames: i64) -> Scene {
        let sim = Sim {
            frames,
            ..Sim::default()
        };
        let handle: Rc<RefCell<dyn ImmediateMode>> = Rc::new(RefCell::new(sim));
        Scene::Container(ContainerNode::new(vec![
            Scene::ImmediateModeNode(
                ImmediateModeNode::new(handle, Rect::default()).with_tag(SIM_TAG),
            ),
            Scene::External(
                ExternalNode::new(Box::new(Probe { stamped: frames }))
                    .with_tag(PROBE_TAG.to_owned()),
            ),
        ]))
    }

    #[test]
    fn r1482_the_three_surfaces_report_three_origins() {
        // The claim in one assertion: same method, same wire shape, three
        // different contracts — now distinguishable.
        assert_eq!(
            query_from(&state_scene(), SceneSource::State, MODEL_PATH)
                .expect("the model answers")
                .1,
            AnswerOrigin::State,
        );
        let paint = paint_scene(3);
        assert_eq!(
            query_from(&paint, SceneSource::Paint, "/sim/external/frames")
                .expect("the driver answers")
                .1,
            AnswerOrigin::PaintDriver,
        );
        assert_eq!(
            query_from(&paint, SceneSource::Paint, "/probe/external/stamped")
                .expect("the probe answers")
                .1,
            AnswerOrigin::PaintFrame,
        );
    }

    #[test]
    fn r1482_only_the_model_is_in_the_state_scene() {
        // The premise the whole demo rests on: the other two are absent from
        // the state scene, which is what makes the RPC fallback fire for them.
        // If a future edit put them there, every origin assertion would still
        // pass while testing nothing.
        let state = state_scene();
        for path in ["/sim/external/frames", "/probe/external/stamped"] {
            assert!(
                query_from(&state, SceneSource::State, path).is_err(),
                "{path} must not resolve in the state scene",
            );
        }
    }

    #[test]
    fn r1482_the_origin_word_predicts_the_write_contract() {
        // Checked against the surfaces' own declarations, so the demo's wire
        // assertion has an in-crate counterpart: the two writable origins
        // accept a write, the as-of-frame one does not.
        let mut model = Model::default();
        assert!(model.intervene("ticks", IntrospectValue::Int(5)).is_ok());
        let mut sim = Sim::default();
        assert!(sim.intervene("speed", IntrospectValue::Int(5)).is_ok());
        let mut probe = Probe { stamped: 0 };
        assert!(probe.intervene("stamped", IntrospectValue::Int(5)).is_err());
    }

    #[test]
    fn r1482_a_stamp_cannot_change_without_a_repaint() {
        // What `paint_frame` means, pinned on the type: the node owns a plain
        // value with no path back to the driver, so nothing but building a new
        // node can change what it answers.
        let probe = Probe { stamped: 7 };
        assert_eq!(probe.query("stamped"), Some(IntrospectValue::Int(7)));
        assert_eq!(
            probe.query("stamped"),
            Some(IntrospectValue::Int(7)),
            "a second read of the same node answers the same",
        );
    }

    #[test]
    fn r1482_a_driver_read_sees_the_tick_loop() {
        // The other half: the driver's value moves without any RPC, which is
        // why lumping it in with the per-frame copy would understate it.
        let mut sim = Sim::default();
        let before = sim.query("frames");
        sim.tick(std::time::Duration::from_millis(16));
        assert_ne!(before, sim.query("frames"), "tick must be observable");
    }

    #[test]
    fn r1482_every_declared_slot_answers() {
        // A schema that names a path the surface cannot answer would make the
        // demo's discovery step report a contract that is not real (§2 #7).
        let model = Model::default();
        for f in model.schema().fields {
            assert!(model.query(f.path).is_some(), "model/{}", f.path);
        }
        let sim = Sim::default();
        for f in sim.schema().fields {
            assert!(sim.query(f.path).is_some(), "sim/{}", f.path);
        }
        let probe = Probe { stamped: 0 };
        for f in probe.schema().fields {
            assert!(probe.query(f.path).is_some(), "probe/{}", f.path);
        }
    }
}
