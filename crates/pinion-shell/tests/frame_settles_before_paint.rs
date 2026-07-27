//! R1458 §5.45 §5.27 — a frame runs to a fixed point *before* it is painted.
//!
//! A layout pass writes state the view reads back — the scroll bound, the
//! measured-row table, a pane's measured rect — so the scene it just laid out
//! can already be out of date by the time it returns. Pre-R1458 the paint path
//! answered that with exactly one re-pass and threw the re-pass's own dirty bit
//! away: a chain that needed a third pass was presented mid-settle, and since
//! the paint ends by clearing the reactive dirty flag and the event loop is
//! `ControlFlow::Wait`, nothing asked for the frame that would have finished
//! it. The user kept looking at the half-settled picture until the next
//! keystroke (the tide field report: "the pin lands, but the paint that shows
//! it never comes").
//!
//! Both halves are pinned here:
//!   * a chain that converges within the budget is painted CONVERGED — the
//!     scene handed to the renderer is the one the fixed point produced, not
//!     an earlier pass of it;
//!   * a chain that does not converge is bounded and arms a catch-up redraw,
//!     so the loop stays responsive instead of hanging inside one paint or
//!     stalling until the next input.
//!
//! The fixtures make the settling chain explicit: content height is a function
//! of the bound the previous pass wrote, so pass N's bound is pass N+1's input.
//! That is the same feedback the real consumers have (a scrollbar reading
//! `max`, a measured list windowing against harvested heights) with the
//! arithmetic in plain sight.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, ScrollNode};
use pinion_core::style::{LayoutStyle, Size};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_runtime::SETTLE_PASS_BUDGET;
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{ShellCore, SizeStrategy, WidgetView};

const W: u32 = 400;
const H: u32 = 300;
const VIEWPORT_H: u32 = 100;
/// How much taller each pass makes the content than the bound it read.
const STEP: u32 = 100;
/// Where the converging fixture stops growing: reached on pass 3, confirmed
/// clean by pass 4 — one pass inside [`SETTLE_PASS_BUDGET`].
const TARGET: u32 = 3 * STEP;

#[derive(Debug, Default)]
struct StubExternal;

impl External for StubExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
}

/// Build a scroll whose content height is `VIEWPORT_H + f(bound this state
/// currently publishes)`. The layout pass then writes `max = content -
/// viewport = f(previous max)`, so `f` *is* the settling chain and the fixed
/// point is wherever `f(x) == x`.
fn feedback_scroll(key: &'static str, next_extent: impl Fn(u32) -> u32) -> Scene {
    let scroll = use_scroll_state(key);
    let current = u32::try_from(scroll.max().1).unwrap_or(0);
    let content = Scene::Container(ContainerNode::new(Vec::new()).with_layout(
        LayoutStyle::new().with_size(Size::px(200, VIEWPORT_H + next_extent(current))),
    ));
    Scene::Scroll(ScrollNode::from_state(
        scroll,
        Rect::new(0, 0, 200, VIEWPORT_H),
        content,
    ))
}

/// The bound the binding's `ScrollState` currently publishes, read out of the
/// same root-owner slot the view resolves.
fn bound_of<V: WidgetView>(core: &ShellCore<V>, key: &'static str) -> i32 {
    core.root_owner().run(|| use_scroll_state(key)).max().1
}

/// The height the painted scene actually laid its scrolled content out to —
/// the witness for "which pass produced the scene we are looking at".
fn painted_content_height(scene: &Scene) -> Option<u32> {
    match scene {
        Scene::Scroll(s) => Some(s.content.rect().h),
        Scene::Container(c) => c.children.iter().find_map(painted_content_height),
        _ => None,
    }
}

macro_rules! feedback_view {
    ($name:ident, $tag:literal, $key:literal, $f:expr) => {
        struct $name;

        impl WidgetCore for $name {
            type State = ();
            type Event = ();

            fn tag() -> &'static str {
                $tag
            }

            fn create_external() -> Box<dyn External> {
                Box::new(StubExternal)
            }

            fn read_state(_scene: &Scene) {}

            fn view(_state: (), _frame: &Frame) -> Scene {
                Scene::Container(ContainerNode::new(vec![feedback_scroll($key, $f)]))
            }

            fn event_name(_event: ()) -> &'static str {
                "__internal__"
            }

            fn title() -> &'static str {
                $tag
            }
        }

        impl WidgetA11y for $name {}

        impl WidgetView for $name {
            type Renderer = TestRenderer;

            fn initial_size_strategy() -> SizeStrategy {
                SizeStrategy::Fixed {
                    width: W,
                    height: H,
                }
            }
        }
    };
}

// Converges: grows by STEP per pass until TARGET, then re-affirms it.
feedback_view!(
    ConvergingView,
    "settle_converging",
    "settle_converging_scroll",
    |bound: u32| (bound + STEP).min(TARGET)
);
// Never converges: every pass makes the content taller than the bound it read.
// A bug in the binding — the budget's whole job is to keep it from being a
// hang.
feedback_view!(
    RunawayView,
    "settle_runaway",
    "settle_runaway_scroll",
    |bound: u32| bound + STEP
);

#[test]
fn the_painted_frame_is_the_one_the_chain_settled_on() {
    let mut core = ShellCore::<ConvergingView>::new();
    let scene = core.compute_paint_scene(W, H);

    // Three passes to reach TARGET, a fourth to find it unchanged. One re-pass
    // — the pre-R1458 shape — would have stopped at `STEP` with the state two
    // steps ahead of the pixels.
    assert_eq!(
        bound_of(&core, "settle_converging_scroll"),
        i32::try_from(TARGET).unwrap(),
        "the chain reached its fixed point inside one paint",
    );
    assert_eq!(
        painted_content_height(&scene),
        Some(VIEWPORT_H + TARGET),
        "and the scene handed to the renderer is the fixed point's own, not \
         an earlier pass laid out against a smaller bound",
    );
    assert!(
        !core.take_redraw_request(),
        "a converged frame asks for nothing further",
    );
}

#[test]
fn a_frame_that_cannot_settle_is_bounded_and_asks_for_another() {
    let mut core = ShellCore::<RunawayView>::new();
    let _ = core.compute_paint_scene(W, H);

    assert_eq!(
        bound_of(&core, "settle_runaway_scroll"),
        i32::try_from(SETTLE_PASS_BUDGET * STEP).unwrap(),
        "the loop spent exactly its budget — it neither stopped at one \
         re-pass nor ran away inside the paint",
    );
    assert!(
        core.take_redraw_request(),
        "and it asked for the frame that continues, instead of presenting a \
         half-settled picture into a `ControlFlow::Wait` sleep",
    );
}

#[test]
fn a_settled_binding_asks_for_no_extra_frames() {
    // The control for both tests above: the catch-up redraw must be
    // attributable to the unsettled chain, not to something every paint does.
    let mut core = ShellCore::<ConvergingView>::new();
    let _ = core.compute_paint_scene(W, H);
    let _ = core.take_redraw_request();
    for _ in 0..3 {
        let _ = core.compute_paint_scene(W, H);
        assert!(
            !core.take_redraw_request(),
            "a steady-state paint settles on pass 1 and requests nothing",
        );
    }
}
