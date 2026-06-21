// Prose mentions of widget type names read fine un-backticked in a test binding.
#![allow(clippy::doc_markdown)]

//! R1024 §5.39 — a pointer click that moves keyboard focus must request a
//! redraw, end-to-end forcing consumer (sprag PR-13).
//!
//! The sprag multi-pane terminal: clicking a different pane moves focus
//! immediately (so the next key routes there), but the focus *ring* lagged on
//! the old pane until some unrelated event (a keypress' PTY echo) happened to
//! repaint. Root cause: `click_to_focus_for_window` mutated the `FocusManager`
//! but — unlike the programmatic `drain_focus_request` path — never paired the
//! mutation with `revision.bump()` + `request_redraw()`. The focus ring is
//! injected at paint time (`apply_focus_ring`) and no reactive owner is dirtied
//! by a focus mutation, so without an explicit request the next frame is never
//! scheduled.
//!
//! These tests drive the real pointer path (`cursor_moved` + `mouse_pressed`)
//! against two focusable panes at fixed rects and assert the binding-wide
//! redraw flag (`take_redraw_request`) is raised exactly when — and only when —
//! the click actually moves focus:
//!   * a click that moves focus (None -> pane, pane -> other pane, or a
//!     background click that clears focus) requests a redraw (R13.1 / R13.2);
//!   * a click that does NOT move focus (re-click the focused pane, background
//!     click while already cleared) requests nothing (no spurious frame).
//!
//! The "no spurious redraw" controls are load-bearing: they prove the asserted
//! redraw is attributable to the focus change and not to some unconditional
//! side effect of the press itself.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{FlexDirection, LayoutStyle, Size};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_runtime::PointerId;
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{ShellCore, SizeStrategy, WidgetView};
use std::sync::Mutex;

const PANE0: &str = "pane#0";
const PANE1: &str = "pane#1";
/// A tagged but NON-focusable node (no `.with_focusable`) — the W3C
/// decoration: clicking it leaves focus unchanged, so it must request no
/// redraw. Exercises the `resolve_focusable -> None` arm of the fix.
const DECO: &str = "decoration";

const W: u32 = 640;
const H: u32 = 384;

/// Serialises the file's tests — the shared `TestRenderer` fixture makes
/// interleaving undesirable (the R1020 focus-test precedent).
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// A no-op primary external — `WidgetCore` requires one to carry the state
/// scene; the click-to-focus behaviour is exercised through the painted panes.
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

struct ClickFocusView;

impl WidgetCore for ClickFocusView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        "click_focus_root"
    }

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal)
    }

    fn read_state(_scene: &Scene) {}

    // Two focus stops then a non-focusable decoration, side by side at fixed
    // 100x100 rects (flex row, no grow), so a cursor coordinate hits a known
    // node deterministically: pane#0 x[0,100), pane#1 x[100,200), decoration
    // x[200,300). The root is intentionally UNtagged: a click past all three
    // (x>=300) resolves to no target, exercising the `focus_clear` arm (a
    // tagged-but-non-focusable root would instead leave focus unchanged).
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
        Scene::Container(
            ContainerNode::new(vec![
                cell(PANE0, true),
                cell(PANE1, true),
                cell(DECO, false),
            ])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
        )
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion click-to-focus redraw (R1024)"
    }
}

impl WidgetA11y for ClickFocusView {}

impl WidgetView for ClickFocusView {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: W,
            height: H,
        }
    }
}

/// Boot a shell with the panes painted (focus enumeration derived) and the
/// router primed for hit-testing.
fn booted() -> ShellCore<ClickFocusView> {
    let mut core = ShellCore::<ClickFocusView>::new();
    // `compute_paint_scene` derives the scene focus enumeration (so the panes
    // are valid focus targets); `finalize_frame` publishes the same geometry to
    // the input router so `cursor_moved` can hit-test against it.
    let paint = core.compute_paint_scene(W, H);
    core.finalize_frame(paint);
    core
}

/// Move the cursor to `(x, y)` and click (press + release), returning whether
/// the *press* requested a redraw. The redraw flag is drained around the move
/// and the release so the returned value isolates the press' own effect (the
/// PR-13 probe's isolation trick).
fn click_at(core: &mut ShellCore<ClickFocusView>, x: f64, y: f64) -> bool {
    core.cursor_moved(PointerId::MOUSE, x, y);
    let _ = core.take_redraw_request(); // drop any hover/move-driven redraw
    core.mouse_pressed(PointerId::MOUSE);
    let pressed = core.take_redraw_request();
    core.mouse_released(PointerId::MOUSE);
    let _ = core.take_redraw_request(); // drop any release-driven redraw
    pressed
}

#[test]
fn click_moving_focus_from_none_requests_a_redraw() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    assert_eq!(core.focus().focused(), None, "boot focus is unset");

    let redraw = click_at(&mut core, 50.0, 50.0);
    assert_eq!(
        core.focus().focused(),
        Some(PANE0),
        "click landed focus on pane#0"
    );
    assert!(
        redraw,
        "a click that moves focus None -> pane#0 must request a redraw (R13.1)",
    );
}

#[test]
fn click_moving_focus_between_panes_requests_a_redraw() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    click_at(&mut core, 50.0, 50.0);
    assert_eq!(core.focus().focused(), Some(PANE0));

    let redraw = click_at(&mut core, 150.0, 50.0);
    assert_eq!(core.focus().focused(), Some(PANE1), "focus moved to pane#1");
    assert!(
        redraw,
        "a click that moves focus pane#0 -> pane#1 must request a redraw (R13.1)",
    );
}

#[test]
fn reclicking_the_focused_pane_requests_no_redraw() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    click_at(&mut core, 150.0, 50.0);
    assert_eq!(core.focus().focused(), Some(PANE1));

    // Control: a click on the already-focused pane changes nothing. If the
    // press requested a redraw unconditionally this would (wrongly) be true,
    // so this guards the attribution of the asserts above.
    let redraw = click_at(&mut core, 150.0, 50.0);
    assert_eq!(core.focus().focused(), Some(PANE1), "focus unchanged");
    assert!(
        !redraw,
        "re-clicking the focused pane moves no focus, so it requests no redraw",
    );
}

#[test]
fn background_click_clearing_focus_requests_a_redraw() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    click_at(&mut core, 150.0, 50.0);
    assert_eq!(core.focus().focused(), Some(PANE1));

    // A click past both panes resolves to no target -> focus_clear.
    let redraw = click_at(&mut core, 500.0, 50.0);
    assert_eq!(
        core.focus().focused(),
        None,
        "background click cleared focus"
    );
    assert!(
        redraw,
        "a background click that clears focus must request a redraw (R13.2)",
    );
}

#[test]
fn clicking_a_nonfocusable_decoration_requests_no_redraw() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    click_at(&mut core, 50.0, 50.0);
    assert_eq!(core.focus().focused(), Some(PANE0));

    // Control: a click on a tagged-but-non-focusable node resolves to a hover
    // target that `resolve_focusable` rejects (`None`), so focus is unchanged
    // and no redraw is requested. Directly exercises the `None => false` arm.
    let redraw = click_at(&mut core, 250.0, 50.0);
    assert_eq!(
        core.focus().focused(),
        Some(PANE0),
        "a decoration click leaves focus on pane#0 (W3C: only focusable nodes focus)",
    );
    assert!(
        !redraw,
        "a click on a non-focusable decoration moves no focus, so requests no redraw",
    );
}

#[test]
fn background_click_while_unfocused_requests_no_redraw() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    assert_eq!(core.focus().focused(), None);

    // Control: clearing focus that is already clear changes nothing.
    let redraw = click_at(&mut core, 500.0, 50.0);
    assert_eq!(core.focus().focused(), None);
    assert!(
        !redraw,
        "a background click while already unfocused requests no redraw",
    );
}
