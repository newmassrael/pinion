//! ★★★★★ R1751 §5.4 — **the engine's macrostep budget, observed from here.**
//!
//! # What forced this module
//!
//! R1750 moved the SCE pin onto a revision whose Rust runtime bounds a
//! macrostep at `MAX_MACROSTEP_MICROSTEPS` and counts each truncation. Nothing
//! in this repository asserted that the budget protects OUR machines: the
//! upstream fixture proves the engine, and pinion's widgets reach that engine
//! through `Widget`, which is a different claim.
//!
//! ⚠ **And the round before this one got that wrong in the loudest way.**
//! R1750 shipped a handoff document claiming the bound had NOT reached the Rust
//! backend, on a grep of `tools/codegen/templates/rust/` alone — Rust keeps the
//! loop in the runtime crate, not in generated code, so the search looked in
//! the one place it could not be. The claim was refuted the same day. This
//! module is what the refutation leaves behind: **an absence is proven by
//! counting every place the thing could be, and the cheapest way to stop
//! guessing about a runtime's behaviour is to run it.**
//!
//! # What it drives
//!
//! `fixtures/self_answering_raise.scxml` — a `<raise>` whose own transition
//! raises it again, so the internal queue never empties and the macrostep can
//! only end because the engine stops it.
//!
//! # What the tests assert, and what they deliberately do not
//!
//! They assert that `process_event` RETURNS, that the engine REPORTS the
//! truncation, and that the machine still takes an event afterwards. They do
//! not assert a duration: "it finished within N seconds" is the shape that goes
//! flaky on a slow runner, which [[zero-flake-policy]] forbids. The account is
//! the evidence; the wall clock is not.

// `unsafe_code` intentionally absent — see `widgets/button.rs` for the
// workspace `forbid` policy rationale. sce-build codegen output does
// not use `unsafe`.
#[allow(
    non_snake_case,
    unused_imports,
    dead_code,
    unused_variables,
    unused_mut,
    unused_labels,
    unreachable_patterns,
    unreachable_code,
    unused_assignments,
    clippy::style,
    clippy::complexity,
    clippy::pedantic,
    clippy::all
)]
mod sm {
    include!("../generated/self_answering_raise_sm.rs");
}

// R1768 — `crate::resume`'s tests drive this same chart, because an entry
// action that raises is the only thing in this tree that makes *entering* a
// state observable from outside. Re-exported rather than duplicated: a second
// runaway fixture would be a second account of one fact.
//
// ★ R1796 — gated on `cfg(test)`, which is what it has always been: the ONE
// consumer is `crate::resume`'s test module, so a lib build without
// `cfg(test)` has nothing using it and `-D warnings` says so. Stated rather
// than suppressed with an `allow`, because "only the tests drive this chart" is
// exactly the fact the gate was reporting.
#[cfg(test)]
pub(crate) use sm::{SelfAnsweringRaiseEvent, SelfAnsweringRaisePolicy, SelfAnsweringRaiseState};

#[cfg(test)]
mod tests {
    use super::sm::{SelfAnsweringRaiseEvent, SelfAnsweringRaisePolicy, SelfAnsweringRaiseState};
    use sce_rust_runtime::Engine;

    /// An engine over the runaway chart, already in `idle`.
    ///
    /// `initialize` is what enters the SCXML `initial` target, exactly as
    /// `Widget::with_policy` does it — a fresh engine has not entered anything
    /// yet, so skipping it would test a machine no widget ever runs.
    fn engine() -> Engine<SelfAnsweringRaisePolicy> {
        let mut e = Engine::new(SelfAnsweringRaisePolicy::new());
        e.initialize();
        e
    }

    /// ★★★★★ R1751 — **a macrostep an internal chain cannot end is ended by
    /// the engine, and the engine says so.**
    ///
    /// The chart raises its own event forever. Reaching the assertion at all is
    /// half the proof — a `process_event` that did not return would hang here
    /// rather than fail — and the truncation count is the other half, because
    /// "it returned" alone would also be true of a chart that simply stopped
    /// raising.
    #[test]
    fn r1751_a_self_answering_raise_is_stopped_by_the_engine() {
        let mut e = engine();
        assert_eq!(
            e.truncated_macrosteps(),
            0,
            "nothing has run yet, so nothing can have been truncated"
        );

        e.process_event(SelfAnsweringRaiseEvent::Start);

        assert_eq!(
            e.truncated_macrosteps(),
            1,
            "the chain cannot empty its own queue, so the engine must have \
             stopped exactly one macrostep"
        );
        assert_eq!(
            e.last_truncated_macrostep_state(),
            Some(SelfAnsweringRaiseState::Spin),
            "and it must name the state it stopped in"
        );
    }

    /// ★ R1751 — **a truncated macrostep leaves the machine alive.**
    ///
    /// Without this, the sibling above is satisfied by an engine that bounds
    /// the loop by wedging: the count would rise and the machine would take
    /// nothing further. `settle` is reachable only from outside the chain, so
    /// answering it is the evidence that the budget ended a macrostep rather
    /// than the machine.
    #[test]
    fn r1751_a_truncated_macrostep_does_not_wedge_the_machine() {
        let mut e = engine();
        e.process_event(SelfAnsweringRaiseEvent::Start);
        assert_eq!(e.truncated_macrosteps(), 1, "the premise: it truncated");

        e.process_event(SelfAnsweringRaiseEvent::Settle);

        assert_eq!(
            e.get_current_state(),
            SelfAnsweringRaiseState::Idle,
            "an event from outside the chain must still be taken"
        );
    }

    /// ★ R1751 — **the budget is per macrostep, not a fuse for the engine.**
    ///
    /// Entering the runaway state a second time must truncate a second time. An
    /// engine that armed the bound once and then either gave up or ran forever
    /// would pass both siblings above and fail here.
    #[test]
    fn r1751_the_budget_is_spent_per_macrostep_and_not_once() {
        let mut e = engine();
        e.process_event(SelfAnsweringRaiseEvent::Start);
        e.process_event(SelfAnsweringRaiseEvent::Settle);
        e.process_event(SelfAnsweringRaiseEvent::Start);

        assert_eq!(
            e.truncated_macrosteps(),
            2,
            "two entries into the chain are two truncated macrosteps"
        );
    }
}
