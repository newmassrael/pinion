//! R1715 §5.39 §5.35 §5.15 — a dispatch that ran user code resolves the focus
//! it leaves behind, even when it carried no statechart tail.
//!
//! ## The gap this file closes
//!
//! `ShellCore::pointer_button_for_window` offers every mouse-button edge to a
//! raw multi-button sink FIRST. A consumed edge bumps the revision, requests a
//! redraw and returns early — and that early return skips `handle_tail`, whose
//! last line is the *only* call to `drain_focus_mailboxes` in the product. So
//! a `focus_request::request` written inside `External::raw_pointer_button`
//! sat in the mailbox until some later dispatch happened to drain it. The
//! symptom reads as "nothing happened"; the truth is "it happens one input
//! late, and swallows that input" — and a terminal that eats a keystroke is
//! not a cosmetic defect.
//!
//! The reasoning that produced the gap was half right: a raw edge really has
//! no `DispatchTail`. But `handle_tail` carries TWO responsibilities — drain
//! the tail, and resolve the frame's focus — and "there is no tail" only
//! retires the first.
//!
//! ## Why nothing saw it
//!
//! Every raw-sink fixture lives one layer down, at `pinion-runtime`'s router
//! (`deliver_raw_pointer_button`), where there is no focus drain to skip. The
//! layer that OWNS the drain — this shell seam — had no test that drove it
//! with a raw sink in the scene at all. The defect lived in the seam between
//! the two, which neither layer's tests crossed. Hence this file drives the
//! public shell seam and reads `focus()`, and deliberately never calls
//! `drain_focus_mailboxes` itself: a gate that calls what the product does not
//! call is green about nothing.
//!
//! Reported by sprag (PINION-PR89) — a pane whose child program enabled xterm
//! mouse reporting could not be given the keyboard by clicking it, because
//! owning the raw stream suppresses `click_to_focus` and the mailbox is the
//! only channel left.

use pinion_core::test_fixtures::{
    RAW_FOCUS_OTHER as OTHER, RAW_FOCUS_PANE as PANE, RawSinkFocusFixture, clear_raw_focus_edges,
    raw_focus_edges, raw_focus_gained_count, raw_focus_legacy_sends, set_raw_focus_redirect,
    set_raw_sink_redirect,
};
use pinion_core::{PointerButton, PointerEdge, modal_scope_request};
use pinion_runtime::{DEFAULT_WINDOW, PointerId};
use pinion_shell::ShellCore;
use std::sync::Mutex;

/// The fixture's two 40x40 focus stops, side by side.
const SURFACE: (u32, u32) = (80, 40);
/// A point inside the raw sink's rect (right half).
const IN_PANE: (f64, f64) = (60.0, 20.0);
/// A point inside the plain control's rect (left half).
const IN_OTHER: (f64, f64) = (20.0, 20.0);

/// The closed `(button, edge)` product the seam routes through ONE raw arm.
/// Sweeping it is what makes this gate exhaustive for the arm rather than
/// anecdotal about left-press.
const EVERY_EDGE: [(PointerButton, PointerEdge); 6] = [
    (PointerButton::Left, PointerEdge::Down),
    (PointerButton::Left, PointerEdge::Up),
    (PointerButton::Middle, PointerEdge::Down),
    (PointerButton::Middle, PointerEdge::Up),
    (PointerButton::Right, PointerEdge::Down),
    (PointerButton::Right, PointerEdge::Up),
];

/// Serialises the file: the focus-request mailbox and the raw-edge log are
/// per-thread, but the shared test renderer makes interleaving undesirable.
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// One paint + finalize cycle, the pair the winit loop runs. `finalize_frame`
/// is the post-paint sync point that resolves whatever the frame requested.
fn frame(core: &mut ShellCore<RawSinkFocusFixture>) {
    let scene = core.compute_paint_scene(SURFACE.0, SURFACE.1);
    core.finalize_frame(scene);
}

/// Put the cursor at `at`, then send `button`/`edge` through the ONE seam the
/// native `MouseInput` path and the `scene/pointer_button` RPC drain share.
fn press(
    core: &mut ShellCore<RawSinkFocusFixture>,
    at: (f64, f64),
    button: PointerButton,
    edge: PointerEdge,
) {
    core.cursor_moved_for_window(DEFAULT_WINDOW, PointerId::MOUSE, at.0, at.1);
    core.pointer_button_for_window(DEFAULT_WINDOW, button, edge);
}

/// Move focus without going through any pointer arm, so a test can position
/// the ring before measuring what a press does to it.
fn put_focus_on(core: &mut ShellCore<RawSinkFocusFixture>, tag: &str) {
    pinion_core::focus_request::request(tag);
    frame(core);
    assert_eq!(core.focus().focused(), Some(tag), "focus seeded on {tag}");
}

/// A booted shell with both stops painted and enumerated and focus resting on
/// the plain control, so every assertion below reads a focus **move** rather
/// than a ring that was already where it wanted to be.
fn boot() -> ShellCore<RawSinkFocusFixture> {
    let _ = modal_scope_request::drain();
    let _ = pinion_core::focus_request::drain();
    clear_raw_focus_edges();
    // Both hand-on switches inert unless a test arms them: a fixture that
    // delegates by default would make every other assertion here about
    // delegation instead of about the arm under test.
    set_raw_focus_redirect(None);
    set_raw_sink_redirect(None);
    let mut core = ShellCore::<RawSinkFocusFixture>::new();
    // A real frame: paint publishes the hit-testable scene to the router and
    // re-derives the focus enumeration.
    frame(&mut core);

    // Harness self-check: the cursor point has to reach the sink, or every
    // assertion below would fail for a reason that is not the defect. Building
    // this file, a fixture whose hand-written rects the layout pass overwrote
    // produced exactly that — an empty edge log under red focus assertions,
    // indistinguishable at a glance from the gap under test. A full click, not
    // a lone press: a press opens the raw sink's implicit grab, which would
    // pin every later edge to the sink no matter where the cursor went.
    press(&mut core, IN_PANE, PointerButton::Left, PointerEdge::Down);
    press(&mut core, IN_PANE, PointerButton::Left, PointerEdge::Up);
    assert_eq!(
        raw_focus_edges(),
        vec!["left:down", "left:up"],
        "the point named by IN_PANE resolves to the raw sink",
    );
    clear_raw_focus_edges();
    // The self-check is itself a dispatch, so whatever it left in the mailbox
    // must not leak into the measurement. Pre-fix that leak IS the reported
    // defect in miniature — the undrained request rides along inside the next
    // dispatch and overrides it. Post-fix this drain is a no-op because the
    // self-check resolved its own request.
    let _ = pinion_core::focus_request::drain();

    put_focus_on(&mut core, OTHER);
    core
}

/// PR-89 acceptance #1 — the headline. A raw sink that asks for the keyboard
/// from inside `raw_pointer_button` has it by the time the seam returns, with
/// no intervening dispatch and no paint.
#[test]
fn a_raw_edge_lands_the_focus_it_requested_before_the_seam_returns() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();

    press(&mut core, IN_PANE, PointerButton::Left, PointerEdge::Down);

    assert_eq!(
        core.focus().focused(),
        Some(PANE),
        "the pane owns the keyboard the instant its own click reached it; \
         `Some(OTHER)` here is the request still sitting in the mailbox, \
         which is not 'nothing happened' but 'happens one input late'",
    );
}

/// PR-89 acceptance #1, exhaustively — all six `(button, edge)` pairs cross
/// the same raw arm, so all six must resolve. A gate that asked only about
/// left-press would leave five arms of a closed product unmeasured.
#[test]
fn every_button_edge_pair_resolves_its_focus() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (button, edge) in EVERY_EDGE {
        let mut core = boot();
        press(&mut core, IN_PANE, button, edge);
        assert_eq!(
            core.focus().focused(),
            Some(PANE),
            "{button:?}/{edge:?} reached the raw sink but left its focus request undrained",
        );
    }
}

/// PR-89 acceptance #4 — the child still receives its report. Focus is not
/// bought by swallowing the click: a fix that stole the edge for the focus
/// ring would break every mouse-driven program running inside a pane, which is
/// precisely what the multiplexer this came from must not do.
#[test]
fn the_sink_still_receives_every_edge_it_focused_on() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();

    for (button, edge) in EVERY_EDGE {
        press(&mut core, IN_PANE, button, edge);
    }

    assert_eq!(
        raw_focus_edges(),
        vec![
            "left:down",
            "left:up",
            "middle:down",
            "middle:up",
            "right:down",
            "right:up",
        ],
        "select AND forward — the raw stream is delivered verbatim, in order",
    );
}

/// PR-89 acceptance #4, the other half — the GUI default is still SUPPRESSED
/// for the sink. Resolving the focus a raw dispatch left behind must not turn
/// the raw arm into a pass-through that runs both dispatches.
///
/// This test exists because the round's counterfactual said it had to. Deleting
/// the raw arm's `return` — so every edge is delivered raw AND runs the GUI
/// arc — left every other assertion in this file green, because the double
/// dispatch happened to land focus on the same tag either way. The legacy
/// `send` wire is the observable that tells them apart: a raw sink trades that
/// wire for the raw stream, so anything arriving on it is the suppressed
/// default running anyway.
#[test]
fn the_gui_default_is_still_suppressed_for_the_sink() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();

    for (button, edge) in EVERY_EDGE {
        press(&mut core, IN_PANE, button, edge);
    }

    assert!(
        raw_focus_legacy_sends().is_empty(),
        "the sink owns the raw stream, so the legacy PointerDown / PointerUp \
         wire must not ALSO fire at it — got {:?}",
        raw_focus_legacy_sends(),
    );
}

/// PR-89 acceptance #3 (control) — the non-raw arms are untouched. This is
/// also the harness's own control: it proves the fixture CAN move the ring
/// through the ordinary `click_to_focus` path, so a stuck ring in the tests
/// above is the raw arm's fault and not the harness's.
#[test]
fn a_plain_control_still_takes_focus_through_the_ordinary_arm() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();
    put_focus_on(&mut core, PANE);

    press(&mut core, IN_OTHER, PointerButton::Left, PointerEdge::Down);

    assert_eq!(
        core.focus().focused(),
        Some(OTHER),
        "a press on a non-raw widget still focuses it via click_to_focus",
    );
    assert!(
        raw_focus_edges().is_empty(),
        "and no edge reached the sink — the cursor was never over it",
    );
}

/// PR-89 acceptance #5 — confinement still outranks the request. Moving WHEN
/// the mailbox is drained must not change WHO is allowed to win it: a raw sink
/// outside an open modal's enumeration is refused exactly as before.
#[test]
fn a_modal_trap_still_refuses_a_raw_sink_it_does_not_enumerate() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();
    modal_scope_request::open(vec![OTHER.to_owned()]);
    frame(&mut core);
    assert_eq!(core.focus().modal_depth(), 1, "the trap is up");

    press(&mut core, IN_PANE, PointerButton::Left, PointerEdge::Down);

    assert_eq!(
        core.focus().focused(),
        Some(OTHER),
        "the trap does not enumerate the pane, so its request is refused — \
         this round moves the drain point, not the precedence",
    );
    assert_eq!(
        raw_focus_edges().len(),
        1,
        "the refusal is about focus only; the pane still got its edge",
    );
}

/// R1715 — the seam's post-condition, which is what licenses its single exit.
///
/// `pointer_button_for_window` resolves on EVERY path out, including the six
/// arms that already resolved through `handle_tail`. That is only safe because
/// `drain_focus_mailboxes` leaves both mailboxes empty, so the second call is a
/// no-op *by construction* rather than by anyone's judgement. Read the mailbox
/// directly here: if a dispatch ever returned with something still in it, that
/// residue is precisely the "one input late" defect, waiting for an unrelated
/// input to land on.
#[test]
fn no_dispatch_leaves_anything_in_the_mailbox() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();

    for (button, edge) in EVERY_EDGE {
        press(&mut core, IN_PANE, button, edge);
        assert!(
            pinion_core::focus_request::drain().is_none(),
            "{button:?}/{edge:?} over the raw sink returned with a request still pending",
        );
        press(&mut core, IN_OTHER, button, edge);
        assert!(
            pinion_core::focus_request::drain().is_none(),
            "{button:?}/{edge:?} over the plain control returned with a request still pending",
        );
    }
}

/// R1715 — a widget can hand its focus ON, and it lands in the SAME frame.
///
/// "I was told I have focus; the real target is my child" is how a container
/// delegates (the toolkit's focus proxy, the web's `delegatesFocus`), and it is
/// written by requesting from `on_focus_change` — user code that runs *inside*
/// the resolution. A single-pass resolution left that request in the mailbox
/// for the next dispatch, so delegation worked one input late and ate that
/// input: PINION-PR89's own symptom, one layer in. The fixed point lands it now.
///
/// Measured at R1715, none of the tree's 9 `on_focus_change` bodies writes a
/// mailbox, so without `set_raw_focus_redirect` the settle loop would be a
/// mechanism nothing drives and this file would not notice it being deleted.
#[test]
fn a_widget_can_hand_its_focus_on_within_the_same_frame() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();
    put_focus_on(&mut core, PANE);
    // The control now delegates to the pane the moment it is told it has focus.
    set_raw_focus_redirect(Some(PANE));

    pinion_core::focus_request::request(OTHER);
    frame(&mut core);

    assert_eq!(
        core.focus().focused(),
        Some(PANE),
        "the control took focus and handed it on inside one resolution; \
         `Some(OTHER)` here is the hand-on still waiting for the next input",
    );
    assert!(
        pinion_core::focus_request::drain().is_none(),
        "and the frame settled — nothing is left to land on a later input",
    );
}

/// R1715 — a frame that CANNOT settle is reported AND cleared, not looped on
/// forever and not left pending.
///
/// Two widgets each handing focus to the other from `on_focus_change` is a
/// binding-side cycle, and a fixed point over user code has to bound itself or
/// it is a hang.
///
/// The unwind is caught rather than declared with `#[should_panic]` so the
/// SECOND half is checkable: a `should_panic` test ends at the panic, and the
/// clearing happens on the way there. Leaving the requests in the mailbox
/// would put them on the next unrelated input — the silent-late defect this
/// whole round is about, wearing a different hat — and nothing would have
/// noticed, because the loud half would still have been loud.
#[test]
fn a_focus_cycle_is_reported_and_the_mailbox_is_left_clean() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();
    set_raw_focus_redirect(Some(PANE));
    set_raw_sink_redirect(Some(OTHER));
    clear_raw_focus_edges();

    // The report fires a `debug_assert`, which is a panic in a test build.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pinion_core::focus_request::request(PANE);
        frame(&mut core);
    }));
    std::panic::set_hook(previous);

    let panic = outcome.expect_err("a frame that cannot settle must be reported");
    let message = panic
        .downcast_ref::<String>()
        .map_or("<non-string panic>", String::as_str);
    assert!(
        message.contains("focus did not settle"),
        "the report names what happened; got {message:?}",
    );
    // Disarm before the next dispatch so the cycle does not restart.
    set_raw_focus_redirect(None);
    set_raw_sink_redirect(None);
    assert!(
        pinion_core::focus_request::drain().is_none(),
        "the unsettled frame left nothing behind to land on a later input",
    );
    // The bound is SMALL, not merely finite — asserted against a literal on
    // purpose. A bound whose only job is to terminate is satisfied by any
    // number, so raising it from 8 to 200 was invisible to every other test
    // here (measured: that counterfactual passed). This is the reader that
    // makes the constant a decision somebody has to take twice; a cycling
    // frame re-enters user code this many times and no more.
    assert_eq!(
        raw_focus_gained_count(),
        8,
        "an unsettled frame runs the focus observers once per settle pass",
    );
}
