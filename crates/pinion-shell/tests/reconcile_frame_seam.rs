// Prose mentions of widget type names (TextGrid, …) read fine un-backticked in a
// test binding — the same looser doc-markdown lid the hello-* example bindings use.
#![allow(clippy::doc_markdown)]

//! R1047 §5.23 §5.22 §6.3 — `WidgetCore::reconcile_frame` seam, end-to-end forcing
//! consumer.
//!
//! A binding whose scroll authority is an **offset-projection** (the terminal
//! model: a `Scene::TextGrid` re-projecting scrollback by `offset_lines`, not a
//! `Scene::Scroll` pixel clip) whose content extent (`scrollback_len`) lives in an
//! **off-thread producer** with no reactive `Signal`. The post-layout
//! `update_scroll_state_bounds` reducer cannot reach it (no clip node, extent not
//! layout-derived) and no `Effect` can subscribe (no dep) — so the bound is
//! reconciled in `reconcile_frame`, the per-paint pre-view binding reducer.
//!
//! These tests drive the **live** paint path (`compute_paint_scene`, which runs
//! `reconcile_frame`) versus the **side-effect-free mirror**
//! (`compute_paint_scene_pure`, which must NOT) and assert the §6.3 purity
//! contract: a real paint reconciles the producer-derived bound; an introspection
//! / dry_run paint leaves it untouched.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::ContainerNode;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::virtual_list::{at_bottom, follow_tail};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{ShellCore, SizeStrategy, WidgetView};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

const SCROLL_KEY: &str = "reconcile_frame.scroll";

/// Serialises the file's tests: the producer length + call witness are
/// process-global statics the parallel test runner must not interleave.
static TEST_LOCK: Mutex<()> = Mutex::new(());
/// Simulated off-thread producer: the terminal's scrollback length in rows.
/// NOT a reactive `Signal` — exactly the substrate gap `reconcile_frame` fills.
static PRODUCER_LEN: AtomicUsize = AtomicUsize::new(0);
/// `reconcile_frame` invocation witness.
static RECONCILE_CALLS: AtomicUsize = AtomicUsize::new(0);

/// A no-op primary external: the seam is exercised through the binding's
/// `reconcile_frame` + a `use_scroll_state` authority, not this handle, but
/// `WidgetCore` requires a primary external to carry the state scene.
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

struct ScrollFollowView;

impl WidgetCore for ScrollFollowView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        "follow"
    }

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal)
    }

    fn read_state(_scene: &Scene) {}

    fn view(_state: (), _frame: &Frame) -> Scene {
        // Pure read of the already-reconciled bound — no Signal write here.
        let _bound = use_scroll_state(SCROLL_KEY).max().1;
        Scene::Container(ContainerNode::new(vec![]).with_tag("follow"))
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "reconcile_frame seam (R1047)"
    }

    fn reconcile_frame() {
        RECONCILE_CALLS.fetch_add(1, Ordering::SeqCst);
        // Read the off-thread producer length (non-reactive) and grow +
        // tail-follow the bound — the write that must live OFF the pure view fn.
        let len = PRODUCER_LEN.load(Ordering::SeqCst);
        let scroll = use_scroll_state(SCROLL_KEY);
        let was_following = at_bottom(scroll.offset().1, scroll.max().1);
        follow_tail(&scroll, len, 1, 0, was_following);
    }
}

impl WidgetA11y for ScrollFollowView {}

impl WidgetView for ScrollFollowView {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: 200,
            height: 200,
        }
    }
}

/// The reconciled max-y bound, read under the binding root owner (the same
/// `use_scroll_state` instance the view fn + reconcile_frame share).
fn bound(core: &ShellCore<ScrollFollowView>) -> i32 {
    core.root_owner()
        .run(|| use_scroll_state(SCROLL_KEY).max().1)
}

#[test]
fn real_paint_reconciles_but_pure_mirror_does_not() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PRODUCER_LEN.store(0, Ordering::SeqCst);
    RECONCILE_CALLS.store(0, Ordering::SeqCst);

    let mut core = ShellCore::<ScrollFollowView>::new();

    // (1) producer has 5 rows; a REAL paint runs reconcile_frame (pre-view)
    //     which grows the bound to the producer length.
    PRODUCER_LEN.store(5, Ordering::SeqCst);
    let _ = core.compute_paint_scene(200, 200);
    assert_eq!(
        RECONCILE_CALLS.load(Ordering::SeqCst),
        1,
        "a real paint runs reconcile_frame exactly once",
    );
    assert_eq!(
        bound(&core),
        5,
        "reconcile_frame grew the bound to producer len"
    );

    // (2) producer grows to 12 off-thread; a PURE mirror paint must NOT
    //     reconcile (§6.3 / §2 #3: an introspection / dry_run paint has no
    //     side effects), so the bound stays 5 and reconcile_frame is not
    //     called again.
    PRODUCER_LEN.store(12, Ordering::SeqCst);
    let _ = core.compute_paint_scene_pure(200, 200);
    assert_eq!(
        RECONCILE_CALLS.load(Ordering::SeqCst),
        1,
        "the side-effect-free mirror does NOT call reconcile_frame",
    );
    assert_eq!(
        bound(&core),
        5,
        "the pure mirror left the bound un-reconciled (no side effect)",
    );

    // (3) the next REAL paint reconciles to the grown producer.
    let _ = core.compute_paint_scene(200, 200);
    assert_eq!(RECONCILE_CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(
        bound(&core),
        12,
        "a real paint reconciles to the grown producer"
    );
}

#[test]
fn steady_producer_reconcile_is_idempotent() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    PRODUCER_LEN.store(7, Ordering::SeqCst);
    RECONCILE_CALLS.store(0, Ordering::SeqCst);

    let mut core = ShellCore::<ScrollFollowView>::new();
    let _ = core.compute_paint_scene(200, 200);
    assert_eq!(bound(&core), 7);

    // Two more paints at an UNCHANGED producer length: reconcile_frame runs
    // each paint, but set_max / scroll_to equality-skip, so the bound is
    // stable — a steady producer reconciles to a no-op (loop-safe).
    let _ = core.compute_paint_scene(200, 200);
    let _ = core.compute_paint_scene(200, 200);
    assert_eq!(
        RECONCILE_CALLS.load(Ordering::SeqCst),
        3,
        "ran every real paint"
    );
    assert_eq!(
        bound(&core),
        7,
        "a steady producer leaves the bound unchanged"
    );
}
