//! `hello-viewport-question` — R1468 §5.23 §5.22 §2 #3 §2 #7.
//!
//! An agent can ask a pinion binding a hypothetical: `scene/layout {viewport}`
//! means *lay yourself out as if the window were W×H*. This binding is built so
//! that answering that question badly is visible, because it draws the SAME
//! quantity — half the window's height — twice, by the two routes a binding has:
//!
//!   * `by_engine` is `height: 50%`, resolved by the layout engine against
//!     whatever root extent it is handed;
//!   * `by_seam` is `use_viewport_size().1 / 2`, read through the R1006 reactive
//!     seam a producer needs because a reflow is a side effect (a PTY's winsize
//!     ioctl) and so must be reachable from an `Effect`, not only from the pure
//!     view.
//!
//! Both routes are legitimate and a real app mixes them. Pre-R1468 they answered
//! a hypothetical differently — taffy got the hypothetical extent, the seam kept
//! reporting the live one — so a question about a 400×1200 window came back
//! describing two different windows at once.
//!
//! The third row is the one that makes the fix non-trivial. `reflows` counts a
//! side-effecting `Effect` subscribed to the seam. Republishing the hypothetical
//! is a `Signal::set`, and a `Signal::set` re-runs subscribers — so the naive fix
//! would drive a real reflow at a size no window has. The mirror therefore runs
//! inside `pinion_runtime::IntrospectionPaint`, which restores the live extent
//! and suppresses those re-runs, and this counter is how an agent checks that
//! over the wire: ask any number of hypotheticals and it must not move.
//!
//! Run it: `cargo run -p hello-viewport-question`. Resize the window and the
//! bars stay equal while `reflows` ticks — that is a REAL resize doing what it
//! should. Then ask a hypothetical over RPC and watch it not tick.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, ExternalNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonState};
use pinion_core::{Effect, Frame, Owner, WidgetCore, use_viewport_size};
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl};
use std::cell::Cell;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(ViewportQuestionRenderer, ViewportQuestionRendererError);

const WIN_W: u32 = 640;
const WIN_H: u32 = 420;
const BY_ENGINE: &str = "by_engine";
const BY_SEAM: &str = "by_seam";
const REFLOW_TAG: &str = "reflow_meter";
const OWNER_KEY: &str = "hello-viewport-question/reflow";

thread_local! {
    /// How many times the seam-subscribed `Effect` has actually run. A
    /// thread-local rather than a field on the External because the Effect is
    /// owned by the reactive scope and the External is owned by the scene; the
    /// two are rebuilt on different schedules and this is the value both read.
    static REFLOWS: Cell<u64> = const { Cell::new(0) };
}

/// The side-effecting consumer the R1006 seam exists for, stood up once per
/// binding through the `Owner` cache (`Effect` handles must be retained —
/// dropping one unsubscribes it, the R665 retention rule).
fn install_reflow_effect(owner: &Owner) {
    // Resolve the seam's `Signal` HANDLE here, outside the cache factory below,
    // and read it directly in the Effect body. Two reasons, both load-bearing:
    //
    //  * `use_viewport_size()` resolves a `ProviderSlot` through `Owner::cache`,
    //    and the Effect's eager first run happens INSIDE the factory — the
    //    re-entrancy `Owner::cache` panics on
    //    ([[owner-cache-no-nested-factory]]);
    //  * an Effect re-run pushes the subscriber stack, not the owner-handle
    //    stack, so a body calling the hook would resolve nothing on any wake
    //    that did not originate under an owner scope. R1006 documents the
    //    captured-handle shape as the one a reflow Effect uses.
    let viewport = pinion_core::VIEWPORT_SIZE.resolve(owner);
    let owner_for_effect = owner.clone();
    owner.cache(OWNER_KEY, move || {
        Effect::new(&owner_for_effect, move || {
            // A real consumer would push this size into a producer (resize a
            // PTY, re-tile a canvas). Counting is the introspectable stand-in:
            // an agent cannot see an ioctl, but it can read this.
            let _ = viewport.get();
            REFLOWS.with(|c| c.set(c.get() + 1));
        })
    });
}

/// Publishes the reflow count on the §5.15 introspect channel, so the claim
/// "asking a hypothetical fires no side effect" is answerable over RPC instead
/// of by reading the source.
#[derive(Debug, Default)]
struct ReflowMeter;

impl External for ReflowMeter {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
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

impl ExternalIntrospect for ReflowMeter {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(const { &[SchemaField::new("reflows", "int")] })
    }
    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "reflows" => Some(IntrospectValue::Int(
                i64::try_from(REFLOWS.with(Cell::get)).unwrap_or(i64::MAX),
            )),
            _ => None,
        }
    }
    /// Read-only: the count is a measurement of the world, not a knob on it.
    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(InterveneError::ReadOnly)
    }
}

fn bar(tag: &str, height: SizeValue, fill: Color, children: Vec<Scene>) -> Scene {
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_owned())
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_padding(Rect::new(8, 8, 8, 8))
                    .with_size(
                        Size::auto()
                            .with_width(SizeValue::Percent(45))
                            .with_height(height),
                    ),
            ),
    )
}

fn caption(text: &str) -> Scene {
    let mut style = TextStyle::new();
    style.fg_color = Color::rgb(0xFF, 0xFF, 0xFF);
    style.font_size_px = 13;
    Scene::Text(TextNode::styled(text, Rect::default(), style))
}

/// view-fn (§6.3): two bars that must always be the same height, and a meter
/// that must not move when a question is asked.
///
/// The root is the bars' own row — deliberately, and it is the whole point of
/// the fixture. `by_engine` is a percentage, and a percentage resolves against
/// its PARENT, while the seam reports the WINDOW. Wrapping the row in a padded
/// column with a caption above it (the first shape of this example) made the
/// two quantities incomparable for a reason that had nothing to do with the
/// round: the row was simply shorter than the window. So the captions live
/// INSIDE the bars, where they cannot change what is being measured.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: ButtonState, _frame: &Frame) -> Scene {
    let owner = Owner::current().expect("a view fn runs inside the root owner scope");
    // Read the seam BEFORE installing the Effect. `use_viewport_size` resolves
    // its `ProviderSlot` through `Owner::cache`, and the Effect's eager first
    // run would otherwise reach that resolve from INSIDE the cache factory —
    // the re-entrancy `Owner::cache` panics on
    // ([[owner-cache-no-nested-factory]]).
    let (_, vh) = use_viewport_size();
    install_reflow_effect(&owner);

    Scene::Container(
        ContainerNode::new(vec![
            // Half the window, via the layout engine.
            bar(
                BY_ENGINE,
                SizeValue::Percent(50),
                Color::rgb(0x2E, 0x6B, 0xE6),
                vec![caption("50% by layout")],
            ),
            // Half the window, via the R1006 seam. `max(1)` keeps the bar a
            // legal box before the first paint, when R1006 defines the seam as
            // the honest `(0, 0)` "viewport unknown".
            bar(
                BY_SEAM,
                SizeValue::Px((vh / 2).max(1)),
                Color::rgb(0xE6, 0x7E, 0x2E),
                vec![caption("half by viewport seam")],
            ),
            Scene::External(
                ExternalNode::new(Box::new(ReflowMeter)).with_tag(REFLOW_TAG.to_owned()),
            ),
        ])
        .with_style(BoxStyle::filled(Color::rgb(0xF7, 0xF7, 0xF7)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Start)
                .with_gap(16),
        ),
    )
}

struct ViewportQuestion;

impl WidgetCore for ViewportQuestion {
    type State = ButtonState;
    type Event = ButtonEvent;
    fn create_external() -> Box<dyn External> {
        Box::new(ReflowMeter)
    }
    fn tag() -> &'static str {
        REFLOW_TAG
    }
    fn read_state(_scene: &Scene) -> Self::State {
        ButtonState::Idle
    }
    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, frame)
    }
    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }
    fn title() -> &'static str {
        "pinion viewport question demo"
    }
}

impl WidgetA11y for ViewportQuestion {}

impl WidgetView for ViewportQuestion {
    type Renderer = ViewportQuestionRenderer;
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
    fn windows() -> Vec<WindowSpec> {
        vec![WindowSpec::new(
            "main",
            "pinion viewport question demo",
            SizeStrategy::Fixed {
                width: WIN_W,
                height: WIN_H,
            },
        )]
    }
}

fn main() {
    pinion_shell::run::<ViewportQuestion>();
}
