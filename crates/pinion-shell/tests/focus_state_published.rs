// Prose mentions of widget type names read fine un-backticked in a test binding.
#![allow(clippy::doc_markdown)]

//! R1327 §5.39 — a binding can READ the focused tag from the paint path, and
//! what it reads is what the focus manager holds (sprag PR-53).
//!
//! The focused TAG reached a binding only inside `apply_key`'s `focused` argument
//! (while a key is being pressed) or the a11y tree builder's. Caching the former
//! goes stale the moment focus moves without a key — a click, Tab, a
//! `focus_request`, a modal opening — so display state DERIVED from focus (the
//! sprag terminal's window title naming the active pane, the tmux convention)
//! could not be held correctly. (`External::on_focus_change` carries a
//! per-External *boolean*, not the binding-wide tag, and only to focus stops that
//! are Externals.)
//!
//! `pinion_core::focus_state::focused()` is the read. These tests drive the
//! REAL shell dispatch paths — a pointer press, a Tab traversal, an RPC
//! `focus/set`, a drained programmatic `focus_request` — and assert each one
//! lands in the mirror a binding reads:
//!
//! * the pointer and RPC cases are the ones no `apply_key` cache can see (NO
//!   key is ever pressed in them);
//! * `mirror_agrees_with_the_apply_key_argument` is the SSOT criterion — the
//!   value a view reads and the value `apply_key` is handed are compared inside
//!   the same dispatch, so the two doors cannot disagree;
//! * `a_click_that_moves_no_focus_publishes_nothing` is the control: it proves
//!   the assertions above are attributable to the focus change and not to some
//!   unconditional publish on every press.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, RepaintOwner, ThreadOwnership,
};
use pinion_core::input::Modifiers;
use pinion_core::scene::ContainerNode;
use pinion_core::style::{FlexDirection, LayoutStyle, Size};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_runtime::PointerId;
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{ShellCore, SizeStrategy, WidgetView};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

const PANE0: &str = "pane0";
const PANE1: &str = "pane1";
/// Tagged but NOT focusable — clicking it moves no focus (the W3C decoration
/// convention), so it must publish nothing.
const DECO: &str = "decoration";

const W: u32 = 640;
const H: u32 = 384;

/// Serialises the file's tests: the mirror is a thread-local, and the shared
/// `TestRenderer` fixture makes interleaving undesirable regardless (the R1020
/// focus-test precedent).
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// What `WidgetCore::apply_key` was handed, paired with what
/// `focus_state::focused()` reported at the same instant — the SSOT probe.
static APPLY_KEY_FOCUS: Mutex<Option<(Option<String>, Option<String>)>> = Mutex::new(None);

/// Whether the view still paints `pane1`. Cleared to model the focused widget's
/// view branch going away (a pane closing) — the one focus mutation that happens
/// INSIDE the paint pass.
static PANE1_PAINTED: AtomicBool = AtomicBool::new(true);

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

struct FocusPublishView;

impl WidgetCore for FocusPublishView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        "focus_publish_root"
    }

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal)
    }

    fn read_state(_scene: &Scene) {}

    /// Two focus stops then a non-focusable decoration, at fixed 100x100 rects
    /// in a flex row, so a cursor coordinate hits a known node: pane0 x[0,100),
    /// pane1 x[100,200), decoration x[200,300). The root is untagged so a click
    /// past all three resolves to no target (the focus-clear arm).
    fn view(_state: (), _frame: &Frame) -> Scene {
        let cell = |tag: &'static str, focusable: bool| {
            Scene::Container(
                ContainerNode::new(Vec::new()).with_tag(tag).with_layout(
                    LayoutStyle::new()
                        .with_focusable(focusable)
                        .with_size(Size::px(100, 100)),
                ),
            )
        };
        let mut children = vec![cell(PANE0, true)];
        if PANE1_PAINTED.load(Ordering::SeqCst) {
            children.push(cell(PANE1, true));
        }
        children.push(cell(DECO, false));
        Scene::Container(
            ContainerNode::new(children).with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
        )
    }

    /// Records the framework-supplied `focused` argument next to the mirror the
    /// binding could have read instead, so the test can compare the two doors.
    fn apply_key(_scene: &mut Scene, focused: Option<&str>, _key: &str, _mods: Modifiers) -> bool {
        *APPLY_KEY_FOCUS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((
            focused.map(str::to_owned),
            pinion_core::focus_state::focused(),
        ));
        false
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion published focus state (R1327)"
    }
}

impl WidgetA11y for FocusPublishView {}

impl WidgetView for FocusPublishView {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: W,
            height: H,
        }
    }
}

/// The tag a binding reads on the paint path.
fn mirror() -> Option<String> {
    pinion_core::focus_state::focused()
}

/// Boot a shell with the panes painted (so the scene-derived focus enumeration
/// exists) and the router primed for hit-testing.
fn booted() -> ShellCore<FocusPublishView> {
    PANE1_PAINTED.store(true, Ordering::SeqCst);
    let mut core = ShellCore::<FocusPublishView>::new();
    let paint = core.compute_paint_scene(W, H);
    core.finalize_frame(paint);
    core
}

/// Press and release at `(x, y)` — the real pointer path, no key involved.
fn click_at(core: &mut ShellCore<FocusPublishView>, x: f64, y: f64) {
    core.cursor_moved(PointerId::MOUSE, x, y);
    core.mouse_pressed(PointerId::MOUSE);
    core.mouse_released(PointerId::MOUSE);
}

/// Dispatch a JSON-RPC frame (also the drain point for the programmatic
/// focus-request mailbox).
fn rpc(core: &mut ShellCore<FocusPublishView>, request: &str) {
    let mut no_resize = |_: u32, _: u32| {};
    let _ = core.dispatch_rpc(request, &mut no_resize);
}

#[test]
fn a_click_publishes_the_focused_tag() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    assert_eq!(mirror(), None, "boot: nothing focused");

    // NO key is pressed here — this is precisely the path an `apply_key`-cache
    // workaround cannot observe.
    click_at(&mut core, 50.0, 50.0);
    assert_eq!(core.focus().focused(), Some(PANE0));
    assert_eq!(
        mirror().as_deref(),
        Some(PANE0),
        "a click that moves focus publishes it to the binding",
    );

    click_at(&mut core, 150.0, 50.0);
    assert_eq!(
        mirror().as_deref(),
        Some(PANE1),
        "…and a click onto another pane republishes",
    );
}

#[test]
fn a_background_click_publishes_the_cleared_focus() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    click_at(&mut core, 50.0, 50.0);
    assert_eq!(mirror().as_deref(), Some(PANE0));

    // Past all three cells → no target → focus clears.
    click_at(&mut core, 400.0, 50.0);
    assert_eq!(core.focus().focused(), None);
    assert_eq!(
        mirror(),
        None,
        "the cleared focus is published too (a binding must stop naming a dead tag)",
    );
}

#[test]
fn a_click_that_moves_no_focus_publishes_nothing() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    click_at(&mut core, 50.0, 50.0);
    assert_eq!(mirror().as_deref(), Some(PANE0));

    // A tagged but non-focusable decoration: focus stays where it is, so the
    // mirror must not churn (the control that makes the tests above
    // attributable to the focus change itself).
    click_at(&mut core, 250.0, 50.0);
    assert_eq!(core.focus().focused(), Some(PANE0));
    assert_eq!(mirror().as_deref(), Some(PANE0));
}

#[test]
fn tab_traversal_publishes_the_focused_tag() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();

    assert!(core.handle_focus_traverse(false), "Tab advances focus");
    assert_eq!(mirror().as_deref(), Some(PANE0), "Tab publishes");

    assert!(core.handle_focus_traverse(false));
    assert_eq!(mirror().as_deref(), Some(PANE1));

    assert!(core.handle_focus_traverse(true), "Shift+Tab steps back");
    assert_eq!(mirror().as_deref(), Some(PANE0));
}

#[test]
fn rpc_focus_set_publishes_the_focused_tag() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();

    // The AI-primary path (§2 #2) — also key-less.
    rpc(
        &mut core,
        r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":"pane1"},"id":1}"#,
    );
    assert_eq!(core.focus().focused(), Some(PANE1));
    assert_eq!(mirror().as_deref(), Some(PANE1), "focus/set publishes");

    rpc(
        &mut core,
        r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":null},"id":2}"#,
    );
    assert_eq!(mirror(), None, "focus/set null publishes the clear");
}

#[test]
fn a_drained_focus_request_publishes_the_focused_tag() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();

    // The programmatic path a widget body takes (`External::invoke`, a reducer,
    // an Effect): request → the shell drains it at its post-dispatch sync point.
    pinion_core::focus_request::request(PANE1);
    rpc(
        &mut core,
        r#"{"jsonrpc":"2.0","method":"focus/get","id":1}"#,
    );

    assert_eq!(core.focus().focused(), Some(PANE1));
    assert_eq!(
        mirror().as_deref(),
        Some(PANE1),
        "a drained focus_request publishes — the write direction's own move is readable",
    );
}

#[test]
fn mirror_agrees_with_the_apply_key_argument() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    *APPLY_KEY_FOCUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

    click_at(&mut core, 150.0, 50.0);
    core.apply_key("a");

    let recorded = APPLY_KEY_FOCUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .expect("apply_key ran");
    let (argument, published) = recorded;
    assert_eq!(
        argument.as_deref(),
        Some(PANE1),
        "the framework handed apply_key the focused pane",
    );
    assert_eq!(
        published, argument,
        "★ the tag a binding READS on the paint path is the tag the framework HANDS \
         apply_key — one focus, one SSOT (the PR-53 acceptance criterion)",
    );
}

#[test]
fn focus_dropped_by_the_paint_pass_schedules_the_correcting_frame() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    rpc(
        &mut core,
        r#"{"jsonrpc":"2.0","method":"focus/set","params":{"tag":"pane1"},"id":1}"#,
    );
    assert_eq!(mirror().as_deref(), Some(PANE1));

    // The focused pane's view branch goes away. The NEXT paint drops focus — but it
    // does so AFTER `V::view` has already run and painted "pane1 is active", so that
    // frame is stale by the time it is presented.
    PANE1_PAINTED.store(false, Ordering::SeqCst);
    let _ = core.take_redraw_request();
    let paint = core.compute_paint_scene(W, H);
    core.finalize_frame(paint);

    assert_eq!(
        core.focus().focused(),
        None,
        "the focused tag left the scene, so focus dropped",
    );
    assert_eq!(mirror(), None, "…and the binding is told");
    assert!(
        core.take_redraw_request(),
        "★ a focus drop INSIDE the paint pass must schedule the correcting frame: the \
         reactive dirty flag it raises is cleared by the end of that same paint, so \
         without this request nothing repaints and the stale name sits on screen until \
         an unrelated event happens to redraw",
    );
}
