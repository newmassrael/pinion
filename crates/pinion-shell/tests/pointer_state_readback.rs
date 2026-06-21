// Prose mentions of widget type names read fine un-backticked in a test binding.
#![allow(clippy::doc_markdown)]

//! R1025 §5.35 — ShellCore exposes pointer hover/capture READ accessors
//! (sprag PR-14), grounding pointer-driven interaction tests in data.
//!
//! sprag verifies interaction "as data, not pixels": keyboard focus through
//! `ShellCore::focus()`, redraw through `redraw_requested()`. The pointer axis
//! had the WRITE side (`cursor_moved` / `mouse_pressed` / `mouse_released`) but
//! no READ side, so a drag (splitter / pan / capture gesture) could be driven
//! but not diagnosed — when a drag does nothing you cannot tell hover-miss from
//! capture-miss from move-not-delivered.
//!
//! These tests drive the real pointer path against a single capturing widget (a
//! `wants_pointer_capture` External tagged `splitter`) and read back the hover
//! and capture state the new accessors expose:
//!   * `hover_target` tracks the cursor onto and off the widget;
//!   * `captured_target` is empty while merely hovering, holds the tag between
//!     press and release, and survives a cursor move while held (the drag);
//!   * the two failure modes are distinguishable in data: a press off any
//!     hit-target captures nothing (hover-miss), and a press on a hovered but
//!     NON-capturing widget also captures nothing (capture-miss) while
//!     `hover_target` still reports the widget.
//!
//! Each single-window read is cross-checked against its per-window form.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{LayoutStyle, Size};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_runtime::{DEFAULT_WINDOW, PointerId};
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{ShellCore, SizeStrategy, WidgetView};
use std::sync::Mutex;

const SPLITTER: &str = "splitter";
/// A second, NON-capturing focus target (no matching capturing External in the
/// state scene), so a press on it hovers but captures nothing — the capture-miss
/// contrast to the hover-miss `press_off_target` case.
const PANE: &str = "pane";
const W: u32 = 640;
const H: u32 = 384;

/// Serialises the file's tests — the shared `TestRenderer` fixture makes
/// interleaving undesirable (the R1020 focus-test precedent).
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// A primary External that grabs the pointer on press — the splitter / slider /
/// pan-canvas class. `wants_pointer_capture` is the only behaviour the capture
/// path reads; everything else is the inert default.
#[derive(Debug, Default)]
struct CapturingExternal;

impl External for CapturingExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn wants_pointer_capture(&self) -> bool {
        true
    }
}

struct DragView;

impl WidgetCore for DragView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        SPLITTER
    }

    fn create_external() -> Box<dyn External> {
        Box::new(CapturingExternal)
    }

    fn read_state(_scene: &Scene) {}

    // Two hittable 200x200 nodes stacked by block flow: `splitter` at (0,0) and
    // `pane` below it at (0,200). The root is UNtagged so a cursor outside both
    // resolves to no target (hover `None`). Only `splitter` matches the primary
    // External's tag (`tag()`), so the capture path's state-scene lookup finds a
    // capturing External for `splitter` but none for `pane` (capture-miss case).
    fn view(_state: (), _frame: &Frame) -> Scene {
        let cell = |tag: &'static str| {
            Scene::Container(
                ContainerNode::new(Vec::new())
                    .with_tag(tag)
                    .with_layout(LayoutStyle::new().with_size(Size::px(200, 200))),
            )
        };
        Scene::Container(ContainerNode::new(vec![cell(SPLITTER), cell(PANE)]))
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion pointer hover/capture readback (R1025)"
    }
}

impl WidgetA11y for DragView {}

impl WidgetView for DragView {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: W,
            height: H,
        }
    }
}

/// Boot a shell with the widget painted and the router primed for hit-testing.
fn booted() -> ShellCore<DragView> {
    let mut core = ShellCore::<DragView>::new();
    let paint = core.compute_paint_scene(W, H);
    core.finalize_frame(paint);
    core
}

#[test]
fn hover_target_tracks_the_cursor() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    assert_eq!(
        core.hover_target(PointerId::MOUSE),
        None,
        "no cursor moved yet"
    );

    core.cursor_moved(PointerId::MOUSE, 50.0, 50.0);
    assert_eq!(
        core.hover_target(PointerId::MOUSE),
        Some(SPLITTER),
        "cursor over the widget resolves to its tag",
    );
    // The single-window read is the DEFAULT_WINDOW per-window read.
    assert_eq!(
        core.hover_target_for_window(DEFAULT_WINDOW, PointerId::MOUSE),
        Some(SPLITTER),
        "per-window hover accessor agrees with the single-window one",
    );

    core.cursor_moved(PointerId::MOUSE, 300.0, 50.0);
    assert_eq!(
        core.hover_target(PointerId::MOUSE),
        None,
        "cursor past the widget (x>200) resolves to no target",
    );
}

#[test]
fn captured_target_holds_between_press_and_release() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();

    core.cursor_moved(PointerId::MOUSE, 50.0, 50.0);
    assert_eq!(
        core.captured_target(PointerId::MOUSE),
        None,
        "hovering a capture widget does not capture — only a press does",
    );

    core.mouse_pressed(PointerId::MOUSE);
    assert_eq!(
        core.captured_target(PointerId::MOUSE),
        Some(SPLITTER),
        "press on the wants_pointer_capture widget engaged the capture lock",
    );
    assert_eq!(
        core.captured_target_for_window(DEFAULT_WINDOW, PointerId::MOUSE),
        Some(SPLITTER),
        "per-window capture accessor agrees with the single-window one",
    );

    // The drag: the cursor moves while the button is held. Capture pins to the
    // splitter through the move (the held-pointer branch bypasses hover
    // refresh), which is exactly the state a splitter-drag test reads back.
    core.cursor_moved(PointerId::MOUSE, 80.0, 130.0);
    assert_eq!(
        core.captured_target(PointerId::MOUSE),
        Some(SPLITTER),
        "capture survives a cursor move while the button is held",
    );

    core.mouse_released(PointerId::MOUSE);
    assert_eq!(
        core.captured_target(PointerId::MOUSE),
        None,
        "release freed the capture lock",
    );
}

#[test]
fn press_off_target_captures_nothing() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();

    // Hover-miss: a press that hits no widget captures nothing.
    core.cursor_moved(PointerId::MOUSE, 500.0, 300.0);
    assert_eq!(
        core.hover_target(PointerId::MOUSE),
        None,
        "cursor hit nothing"
    );

    core.mouse_pressed(PointerId::MOUSE);
    assert_eq!(
        core.captured_target(PointerId::MOUSE),
        None,
        "a press off any hit-target engages no capture",
    );
    core.mouse_released(PointerId::MOUSE);
}

#[test]
fn pressing_a_noncapturing_widget_hovers_but_captures_nothing() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();

    // Capture-miss (distinct from hover-miss): the cursor IS over a widget, but
    // that widget's External does not want capture. `hover_target` reports it,
    // `captured_target` stays empty after a press — the readback that tells a
    // missed hit-target apart from an un-capturing widget.
    core.cursor_moved(PointerId::MOUSE, 50.0, 300.0);
    assert_eq!(
        core.hover_target(PointerId::MOUSE),
        Some(PANE),
        "cursor is over the (non-capturing) pane",
    );

    core.mouse_pressed(PointerId::MOUSE);
    assert_eq!(
        core.captured_target(PointerId::MOUSE),
        None,
        "a press on a hovered but non-capturing widget engages no capture",
    );
    core.mouse_released(PointerId::MOUSE);
}
