//! R1715 §5.41 §5.39 §5.35 §2 #6 — the terminal mirror of
//! `pinion-shell/tests/raw_edge_resolves_its_focus.rs`: a dispatch that ran
//! user code resolves the focus it leaves behind, even with no statechart tail.
//!
//! `ShellCoreTui::pointer_button` carries the Vello gap in the same shape —
//! a consumed raw edge `return`s before `handle_tail`, and `handle_tail`'s
//! last line is this backend's only `drain_focus_mailboxes` call. §2 #6 makes
//! the mirror mandatory rather than nice-to-have: both backends offer the edge
//! through the SAME `CoreShell::raw_pointer_button_for_window` seam but own
//! separate post-dispatch drains, and the comment on the TUI drain already
//! promises "identical focus AND identical modal stacks from identical input".
//! Fixing one backend alone makes that sentence false.
//!
//! One divergence is load-bearing for how these tests are written: click-to-
//! focus is Vello-only (`ShellCore::click_to_focus_for_window` has no terminal
//! peer), so on this backend the focus mailbox is not merely the raw sink's
//! *best* channel — it is the only way a pointer press moves focus at all.
//! Focus is therefore seeded through the `focus/set` RPC rather than a click.

use pinion_core::test_fixtures::{
    RAW_FOCUS_OTHER as OTHER, RAW_FOCUS_PANE as PANE, RawSinkFocusFixture, clear_raw_focus_edges,
    raw_focus_edges, raw_focus_gained_count, raw_focus_legacy_sends, set_raw_focus_redirect,
    set_raw_sink_redirect,
};
use pinion_core::{PointerButton, PointerEdge, modal_scope_request};
use pinion_tui::ShellCoreTui;
use std::sync::Mutex;

/// The fixture's two 40x40 focus stops, side by side — cells here.
const SURFACE: (u16, u16) = (80, 40);
/// A point inside the raw sink's rect (right half).
const IN_PANE: (f64, f64) = (60.0, 20.0);
/// A point inside the plain control's rect (left half).
const IN_OTHER: (f64, f64) = (20.0, 20.0);

/// The closed `(button, edge)` product the seam routes through ONE raw arm.
const EVERY_EDGE: [(PointerButton, PointerEdge); 6] = [
    (PointerButton::Left, PointerEdge::Down),
    (PointerButton::Left, PointerEdge::Up),
    (PointerButton::Middle, PointerEdge::Down),
    (PointerButton::Middle, PointerEdge::Up),
    (PointerButton::Right, PointerEdge::Down),
    (PointerButton::Right, PointerEdge::Up),
];

/// Serialises the file, matching the shell mirror's convention.
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Move focus without going through any pointer arm — on this backend the
/// `focus/set` RPC is the seed, because no press moves focus by itself.
fn put_focus_on(core: &mut ShellCoreTui<RawSinkFocusFixture>, tag: &str) {
    let request =
        format!(r#"{{"jsonrpc":"2.0","id":1,"method":"focus/set","params":{{"tag":"{tag}"}}}}"#);
    core.dispatch_rpc(&request)
        .expect("focus/set must produce a response");
    assert_eq!(core.focus().focused(), Some(tag), "focus seeded on {tag}");
}

/// Write the mailbox and run a dispatch that resolves it — the same path the
/// shell twin drives, so the two backends are asked the same question. A press
/// on the plain control reaches `handle_tail`, which is this backend's
/// post-dispatch resolution point.
fn request_focus_and_dispatch(core: &mut ShellCoreTui<RawSinkFocusFixture>, tag: &str) {
    pinion_core::focus_request::request(tag);
    core.pointer_button(
        IN_OTHER.0,
        IN_OTHER.1,
        PointerButton::Left,
        PointerEdge::Down,
    );
}

/// A booted terminal substrate with both stops painted and enumerated and
/// focus resting on the plain control, so every assertion reads a **move**.
fn boot() -> ShellCoreTui<RawSinkFocusFixture> {
    let _ = modal_scope_request::drain();
    let _ = pinion_core::focus_request::drain();
    clear_raw_focus_edges();
    set_raw_focus_redirect(None);
    set_raw_sink_redirect(None);
    let mut core = ShellCoreTui::<RawSinkFocusFixture>::new();
    let paint = core.compute_paint_scene(SURFACE.0, SURFACE.1);
    core.update_paint_scene(paint);

    // Harness self-check: the cursor point has to reach the sink, or every
    // assertion below would fail for a reason that is not the defect. Building
    // the shell mirror, a fixture whose hand-written rects the layout pass
    // overwrote produced exactly that — an empty edge log under red focus
    // assertions. A full click, not a lone press: a press opens the raw sink's
    // implicit grab, which would pin every later edge to the sink.
    core.pointer_button(IN_PANE.0, IN_PANE.1, PointerButton::Left, PointerEdge::Down);
    core.pointer_button(IN_PANE.0, IN_PANE.1, PointerButton::Left, PointerEdge::Up);
    assert_eq!(
        raw_focus_edges(),
        vec!["left:down", "left:up"],
        "the point named by IN_PANE resolves to the raw sink",
    );
    clear_raw_focus_edges();
    // The self-check is itself a dispatch, so whatever it left in the mailbox
    // must not leak into the measurement. Pre-fix that leak is the reported
    // defect in miniature: the undrained request rode along inside the NEXT
    // dispatch — here `focus/set` — and overrode it, so seeding focus on the
    // sibling landed on the pane instead. Post-fix the drain below is a no-op
    // because the self-check resolved its own request.
    let _ = pinion_core::focus_request::drain();

    put_focus_on(&mut core, OTHER);
    core
}

/// PR-89 acceptance #2 — the headline, mirrored. A raw sink that asks for the
/// keyboard inside `raw_pointer_button` has it by the time the seam returns.
#[test]
fn a_raw_edge_lands_the_focus_it_requested_before_the_seam_returns() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();

    core.pointer_button(IN_PANE.0, IN_PANE.1, PointerButton::Left, PointerEdge::Down);

    assert_eq!(
        core.focus().focused(),
        Some(PANE),
        "the terminal gives the pane the keyboard on its own click too; \
         a backend split here is the §2 #6 defect this mirror exists for",
    );
}

/// PR-89 acceptance #2, exhaustively — all six `(button, edge)` pairs.
#[test]
fn every_button_edge_pair_resolves_its_focus() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (button, edge) in EVERY_EDGE {
        let mut core = boot();
        core.pointer_button(IN_PANE.0, IN_PANE.1, button, edge);
        assert_eq!(
            core.focus().focused(),
            Some(PANE),
            "{button:?}/{edge:?} reached the raw sink but left its focus request undrained",
        );
    }
}

/// PR-89 acceptance #4 — the child still receives its report, in order.
#[test]
fn the_sink_still_receives_every_edge_it_focused_on() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();

    for (button, edge) in EVERY_EDGE {
        core.pointer_button(IN_PANE.0, IN_PANE.1, button, edge);
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
/// for the sink, mirrored. The shell twin carries the full rationale: the
/// round's counterfactual deleted the raw arm's early return and every other
/// assertion stayed green, so the legacy `send` wire is the observable that
/// tells "suppressed" apart from "ran and had nowhere to land".
#[test]
fn the_gui_default_is_still_suppressed_for_the_sink() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();

    for (button, edge) in EVERY_EDGE {
        core.pointer_button(IN_PANE.0, IN_PANE.1, button, edge);
    }

    assert!(
        raw_focus_legacy_sends().is_empty(),
        "the sink owns the raw stream, so the legacy PointerDown / PointerUp \
         wire must not ALSO fire at it — got {:?}",
        raw_focus_legacy_sends(),
    );
}

/// PR-89 acceptance #3 — a press on a NON-raw widget is unchanged. On this
/// backend that means it moves no focus at all (click-to-focus is Vello-only),
/// which is a pre-existing §2 #6 carry this round deliberately does not touch:
/// resolving the mailbox is not the same as growing a new focus source.
#[test]
fn a_press_on_a_plain_control_is_left_exactly_as_it_was() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();
    put_focus_on(&mut core, PANE);

    core.pointer_button(
        IN_OTHER.0,
        IN_OTHER.1,
        PointerButton::Left,
        PointerEdge::Down,
    );

    assert_eq!(
        core.focus().focused(),
        Some(PANE),
        "the terminal has no click-to-focus arc, and this round did not add one",
    );
    assert!(
        raw_focus_edges().is_empty(),
        "and no edge reached the sink — the cursor was never over it",
    );
}

/// PR-89 acceptance #5 — confinement still outranks the request. Moving WHEN
/// the mailbox is drained must not change WHO is allowed to win it.
#[test]
fn a_modal_trap_still_refuses_a_raw_sink_it_does_not_enumerate() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();
    modal_scope_request::open(vec![OTHER.to_owned()]);
    // A press on the plain control reaches `handle_tail`, which applies the
    // modal batch — the terminal's own post-dispatch sync point.
    core.pointer_button(
        IN_OTHER.0,
        IN_OTHER.1,
        PointerButton::Left,
        PointerEdge::Down,
    );
    assert_eq!(core.focus().modal_depth(), 1, "the trap is up");
    assert_eq!(core.focus().focused(), Some(OTHER));

    core.pointer_button(IN_PANE.0, IN_PANE.1, PointerButton::Left, PointerEdge::Down);

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

/// R1715 — the seam's post-condition, which licenses its single exit. Mirror
/// of the shell twin: `pointer_button` resolves on every path out, and that is
/// only safe because the resolution leaves both mailboxes empty.
#[test]
fn no_dispatch_leaves_anything_in_the_mailbox() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();

    for (button, edge) in EVERY_EDGE {
        core.pointer_button(IN_PANE.0, IN_PANE.1, button, edge);
        assert!(
            pinion_core::focus_request::drain().is_none(),
            "{button:?}/{edge:?} over the raw sink returned with a request still pending",
        );
        core.pointer_button(IN_OTHER.0, IN_OTHER.1, button, edge);
        assert!(
            pinion_core::focus_request::drain().is_none(),
            "{button:?}/{edge:?} over the plain control returned with a request still pending",
        );
    }
}

/// R1715 §2 #6 — a widget hands its focus on within the same frame, mirrored.
/// The shell twin carries the rationale; the mirror is what keeps the two
/// backends from disagreeing about when a delegated focus lands.
#[test]
fn a_widget_can_hand_its_focus_on_within_the_same_frame() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();
    put_focus_on(&mut core, PANE);
    set_raw_focus_redirect(Some(PANE));

    request_focus_and_dispatch(&mut core, OTHER);

    assert_eq!(
        core.focus().focused(),
        Some(PANE),
        "the control took focus and handed it on inside one resolution; \
         `Some(OTHER)` here is the hand-on still waiting for the next input",
    );
    assert!(
        pinion_core::focus_request::drain().is_none(),
        "the frame settled — nothing is left to land on a later input",
    );
}

/// R1715 §2 #6 — a frame that cannot settle is reported AND cleared, mirrored.
/// The shell twin carries why the unwind is caught rather than declared: the
/// clearing happens on the way to the panic, so a `should_panic` test would
/// check the loud half and never the clean half.
#[test]
fn a_focus_cycle_is_reported_and_the_mailbox_is_left_clean() {
    let _g = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut core = boot();
    set_raw_focus_redirect(Some(PANE));
    set_raw_sink_redirect(Some(OTHER));
    clear_raw_focus_edges();

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        request_focus_and_dispatch(&mut core, PANE);
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
    set_raw_focus_redirect(None);
    set_raw_sink_redirect(None);
    assert!(
        pinion_core::focus_request::drain().is_none(),
        "the unsettled frame left nothing behind to land on a later input",
    );
    // §2 #6 — the SAME literal the shell twin asserts. Two backends sharing
    // the mailboxes must re-enter user code the same number of times, or
    // identical input converges on one and is reported as a cycle on the other.
    assert_eq!(
        raw_focus_gained_count(),
        8,
        "an unsettled frame runs the focus observers once per settle pass",
    );
}
