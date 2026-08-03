//! R1549.3 §5.41 §5.35 §5.38 — the GUI's **live paint clock** reaches a
//! held press.
//!
//! ## Why this file exists
//!
//! R1549 gave the shell two clock arms for press-and-hold auto-repeat: the
//! injected one (`scene/tick`, in the deferred-input drain) and the LIVE
//! one (the wall-clock delta inside `compute_paint_scene_internal`). Only
//! the injected arm had coverage. Measured while auditing the round: with
//! the live arm **deleted outright**, every unit test in the workspace AND
//! the round's own 58-assertion demo still passed — because the demo
//! pauses the window (`scene/set_fps 0`) to make its fire counts exact,
//! and a paused window is precisely the case where the live arm
//! contributes zero. The one path a real mouse in a real window takes was
//! the one path nothing exercised.
//!
//! That is the [[r1506-guard-renders-production]] shape: a guard aimed at
//! a copy of the thing. The fix is a test that runs the real
//! `compute_paint_scene` and asserts the hold advanced.
//!
//! ## Why it does not sleep
//!
//! A wall-clock assertion would either flake or prove nothing
//! ([[zero-flake-policy]]), and `clamp_frame_dt` caps a frame's delta
//! anyway, so no single paint can be relied on to cross a 300 ms delay.
//! The assertion is therefore on `held_secs` — the accumulator the hold
//! advances on **every** armed frame with `dt > 0`, before any threshold
//! is involved. Two paints suffice: the first has no previous instant
//! (`dt == 0`), the second measures real elapsed time. Nothing here
//! depends on how long a paint takes, only that it takes some time.

use pinion_core::WidgetCore;
use pinion_core::test_fixtures::{EchoButtonFixture, RepeatingButtonFixture};
use pinion_runtime::{DEFAULT_WINDOW, PointerId};
use pinion_shell::ShellCore;

/// The fixture button's painted rect is `(0, 0, 32, 48)`; press its middle.
const HIT: (f64, f64) = (8.0, 8.0);
const W: u32 = 200;
const H: u32 = 200;

/// Boot, paint once so the router has a hit-test scene, and hold the
/// button down. Returns the shell with a press in flight.
fn booted_and_held<V>() -> ShellCore<V>
where
    V: pinion_shell::WidgetView,
{
    let mut core = ShellCore::<V>::new();
    let paint = core.compute_paint_scene(W, H);
    core.finalize_frame(paint);
    core.cursor_moved(PointerId::MOUSE, HIT.0, HIT.1);
    core.mouse_pressed(PointerId::MOUSE);
    core
}

/// Paint once through the real live path (the same fn the winit
/// `RedrawRequested` arm calls), so the frame's wall-clock delta reaches
/// whatever the paint drives.
fn live_paint<V: pinion_shell::WidgetView>(core: &mut ShellCore<V>) {
    let paint = core.compute_paint_scene(W, H);
    core.finalize_frame(paint);
}

#[test]
fn r1549_3_the_live_paint_clock_advances_a_held_press() {
    let mut core = booted_and_held::<RepeatingButtonFixture>();
    let holds = core.auto_repeat_holds_for_window(DEFAULT_WINDOW);
    assert_eq!(holds.len(), 1, "a press is in flight");
    assert!(holds[0].repeating, "on a button that declares a cadence");
    assert_eq!(
        holds[0].target,
        <RepeatingButtonFixture as WidgetCore>::tag(),
    );
    assert!(
        (holds[0].held_secs - 0.0).abs() < f32::EPSILON,
        "nothing has been painted since the press",
    );

    // Two live paints. The first seeds `last_paint_instant` (dt == 0), the
    // second measures real elapsed time — the only thing this test needs
    // from the clock is that it moved at all.
    live_paint(&mut core);
    live_paint(&mut core);

    let holds = core.auto_repeat_holds_for_window(DEFAULT_WINDOW);
    assert_eq!(holds.len(), 1, "still held");
    assert!(
        holds[0].held_secs > 0.0,
        "the paint's own clock reached the hold; got held_secs = {}",
        holds[0].held_secs,
    );
}

#[test]
fn r1549_3_a_released_press_stops_advancing_on_the_live_clock() {
    let mut core = booted_and_held::<RepeatingButtonFixture>();
    live_paint(&mut core);
    live_paint(&mut core);
    assert!(core.auto_repeat_holds_for_window(DEFAULT_WINDOW)[0].held_secs > 0.0);

    core.mouse_released(PointerId::MOUSE);
    assert!(
        core.auto_repeat_holds_for_window(DEFAULT_WINDOW).is_empty(),
        "the release removed the press record",
    );
    for _ in 0..5 {
        live_paint(&mut core);
    }
    assert!(
        core.auto_repeat_holds_for_window(DEFAULT_WINDOW).is_empty(),
        "and painting does not resurrect it",
    );
}

/// The negative control — the SAME button (`EchoButtonFixture` wraps the
/// very `ButtonExternal::new()` that `RepeatingButtonFixture` decorates),
/// differing in exactly one thing: it declares no cadence. Without it the
/// test above would pass on a shell that advanced *every* press, and the
/// whole point of the audit that produced this file is that a guard which
/// cannot fail is not a guard.
#[test]
fn r1549_3_an_undeclared_button_never_advances_on_the_live_clock() {
    let mut core = booted_and_held::<EchoButtonFixture>();
    for _ in 0..5 {
        live_paint(&mut core);
    }
    let holds = core.auto_repeat_holds_for_window(DEFAULT_WINDOW);
    assert_eq!(holds.len(), 1, "the press IS in flight");
    assert!(!holds[0].repeating, "it just declares no cadence");
    assert!(
        (holds[0].held_secs - 0.0).abs() < f32::EPSILON,
        "so no amount of painting accrues hold time",
    );
}
