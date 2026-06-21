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
//!     press and release, and clears on release;
//!   * a press off any hit-target captures nothing (the diagnostic that
//!     distinguishes a hover-miss from a capture failure).
//!
//! Each single-window read is cross-checked against its per-window form.

use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, RepaintOwner, ThreadOwnership,
};
use pinion_core::scene::ContainerNode;
use pinion_core::style::{LayoutStyle, Size};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_a11y::WidgetA11y;
use pinion_runtime::{PointerId, DEFAULT_WINDOW};
use pinion_shell::test_fixtures::TestRenderer;
use pinion_shell::{ShellCore, SizeStrategy, WidgetView};
use std::sync::Mutex;

const SPLITTER: &str = "splitter";
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

    // One hittable node tagged `splitter` pinned to 200x200 at the origin; the
    // root is UNtagged so a cursor past x=200 resolves to no target (hover
    // `None`). The painted tag matches the primary External's tag (`tag()`), so
    // the capture path's state-scene lookup finds the capturing External.
    fn view(_state: (), _frame: &Frame) -> Scene {
        Scene::Container(ContainerNode::new(vec![Scene::Container(
            ContainerNode::new(Vec::new())
                .with_tag(SPLITTER)
                .with_layout(LayoutStyle::new().with_size(Size::px(200, 200))),
        )]))
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
        SizeStrategy::Fixed { width: W, height: H }
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
    let _g = TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();
    assert_eq!(core.hover_target(PointerId::MOUSE), None, "no cursor moved yet");

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
    let _g = TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

    core.mouse_released(PointerId::MOUSE);
    assert_eq!(
        core.captured_target(PointerId::MOUSE),
        None,
        "release freed the capture lock",
    );
}

#[test]
fn press_off_target_captures_nothing() {
    let _g = TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = booted();

    // The diagnostic the seam unblocks: a press that hits no widget captures
    // nothing, so a binding can tell a hover-miss from a capture failure.
    core.cursor_moved(PointerId::MOUSE, 300.0, 50.0);
    assert_eq!(core.hover_target(PointerId::MOUSE), None, "cursor hit nothing");

    core.mouse_pressed(PointerId::MOUSE);
    assert_eq!(
        core.captured_target(PointerId::MOUSE),
        None,
        "a press off any hit-target engages no capture",
    );
    core.mouse_released(PointerId::MOUSE);
}
