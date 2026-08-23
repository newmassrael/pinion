//! ★★★★★ R1796 §5.4 §3 — **an act the host performs, and the ORDER of what it
//! answers with**, observed from here.
//!
//! # What forced this module
//!
//! R1796 moved the SCE pin 22 commits. Measured before anything was written:
//!
//! * the regenerated emit is **byte-identical** — the 15 committed
//!   `{chart}_sm.rs` modules did not move at all, because the host-served path
//!   is emitted only for a type the build DECLARES and this build declared
//!   none;
//! * the Rust door is **not new** — `host_processor.rs` existed at the previous
//!   pin (265 lines, now 297). What the 22 commits did was give the same door
//!   to the C++, C11, Go, Python and Kotlin engines;
//! * and the Rust-side change is a **widening**: a handler answered with
//!   `Option<HostSendResponse>` and now answers with `Vec<…>` — a list of
//!   events, in the order the document should see them.
//!
//! So the honest reading is that pinion could have opened this door before the
//! bump and did not, and that what the bump actually brought is an **ordering
//! guarantee nothing exercised**. This module exercises it.
//!
//! # What it drives
//!
//! `fixtures/host_served_send.scxml`. Its `asking` state sends to
//! `pinion.fixture.host` and accepts only `first`; `heard_first` accepts only
//! `second`. So the order is asserted by **where the document ended up**, not
//! by inspecting a queue — a machine that saw the two replies the other way
//! round sits in `asking` and says so by its configuration.
//!
//! # ⚠ Why this does NOT open a third escape hatch
//!
//! §3 fixes `Effect(opaque)` and `External(opaque)` as the **only** two ways
//! out of the structured model. A host-served `<send>` is a third path by
//! construction — the document names an act and something outside performs it —
//! and whether an APPLICATION may register a handler is therefore a §3
//! question, not an implementation detail.
//!
//! This round does not answer it. The type served here is the **framework's
//! own**, declared in one place (`statechart_emit`'s `HOST_PROCESSOR_TYPES`)
//! and handled by framework code, which is an internal path rather than a new
//! boundary. Opening registration to consumers needs a spec round, and
//! CLAUDE.md's rule for exactly this case is to propose one rather than work
//! around: *"If a new feature requires breaking any of these, STOP and propose
//! a new spec round."*
//!
//! # What the tests assert, and what they deliberately do not
//!
//! They assert that the send REACHED the host at all (an undeclared type would
//! have raised `error.execution` and left the machine in `asking`), that both
//! replies arrived, and that they arrived in the handler's order. They do not
//! assert a duration or a queue length: the account is where the machine is.

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
    include!("../generated/host_served_send_sm.rs");
}

/// The Event I/O Processor type this framework serves.
///
/// The same string the build declares. Written once here and asserted against
/// the build's list by the emit gate, so a rename cannot leave a registration
/// pointing at a type codegen never opened — which registers as inert rather
/// than as an error, and is the failure mode SCE's own doc warns about.
pub(crate) const FIXTURE_PROCESSOR: &str = "pinion.fixture.host";

#[cfg(test)]
mod tests {
    use super::sm::{HostServedSendEvent, HostServedSendPolicy, HostServedSendState};
    use sce_rust_runtime::Engine;
    use sce_rust_runtime::host_processor::{HostSendRequest, HostSendResponse};

    /// An engine over the fixture, already in `idle`.
    ///
    /// `initialize` is what enters the SCXML `initial` target, exactly as
    /// `Widget::with_policy` does — a fresh engine has entered nothing, so
    /// skipping it would test a machine no widget ever runs.
    fn engine() -> Engine<HostServedSendPolicy> {
        let mut e = Engine::new(HostServedSendPolicy::new());
        e.initialize();
        e
    }

    fn reply(event: &str) -> HostSendResponse {
        HostSendResponse {
            event_name: event.to_owned(),
            event_data: String::new(),
        }
    }

    /// ★★★★★ R1796 — **the send reaches the host, and the two events it answers
    /// with arrive in the order the host wrote.**
    ///
    /// The order is read off the CONFIGURATION rather than off a queue: the
    /// chart accepts `first` only in `asking` and `second` only in
    /// `heard_first`, so `Settled` is reachable by exactly one ordering. A
    /// handler that answered the other way round leaves the machine in
    /// `asking`, which the sibling test drives.
    #[test]
    fn r1796_a_host_served_send_answers_with_events_in_order() {
        let mut e = engine();
        let mut seen: Vec<String> = Vec::new();
        // `move` into the closure, so what the handler saw is read back after
        // the run rather than through a shared cell — the engine holds the
        // handler for as long as it lives.
        let (tx, rx) = std::sync::mpsc::channel();
        e.register_event_processor(super::FIXTURE_PROCESSOR, move |req: HostSendRequest| {
            let _ = tx.send(req.event_name.clone());
            vec![reply("first"), reply("second")]
        });

        e.process_event(HostServedSendEvent::Go);

        while let Ok(name) = rx.try_recv() {
            seen.push(name);
        }
        assert_eq!(
            seen,
            vec!["ask".to_owned()],
            "★★ the act reached the host exactly once, carrying the event name \
             the document wrote. An UNDECLARED type would have raised \
             `error.execution` instead and this would be empty"
        );
        assert_eq!(
            e.get_current_state(),
            HostServedSendState::Settled,
            "★★★★★ and both replies arrived in the handler's order — `Settled` \
             is reachable only through `first` then `second`, so the \
             configuration IS the ordering assertion"
        );
    }

    /// ★★★★★ R1796 — **and the order is the HOST's, not a set.**
    ///
    /// The counterfactual the test above needs to mean anything: without it,
    /// "it reached `Settled`" would also be true of an engine that delivered
    /// replies in any order it liked.
    ///
    /// ★ **The predicted resting place was wrong and running it said so, and
    /// what it measured is stronger than the prediction.** I expected `Asking`
    /// — the machine never leaving, because the first reply it can act on never
    /// comes. It rests in `HeardFirst`: both events reach the external queue in
    /// the handler's order, `asking` cannot take `second` so that event is
    /// DROPPED, and `first` then moves the machine one step with nothing left
    /// to carry it further.
    ///
    /// So the reversed order does not merely fail to arrive — it costs an event
    /// outright, and the machine's resting state names which one. That is a
    /// sharper discriminator than "it did not move": an engine that reordered
    /// replies would land on `Settled` here, and one that delivered them as a
    /// SET would land on `Settled` too.
    #[test]
    fn r1796_the_replies_are_delivered_in_the_order_the_handler_gave_them() {
        let mut e = engine();
        e.register_event_processor(super::FIXTURE_PROCESSOR, |_req: HostSendRequest| {
            vec![reply("second"), reply("first")]
        });

        e.process_event(HostServedSendEvent::Go);

        assert_eq!(
            e.get_current_state(),
            HostServedSendState::HeardFirst,
            "★★★★★ reversing the handler's list does NOT reach `Settled` — the \
             out-of-order reply is dropped by the state that cannot take it, so \
             the list is an ORDER and not a bag, and the machine's resting \
             state says which event was lost"
        );
    }

    /// ★★ R1796 — **a declared type with nothing registered is refused, and the
    /// machine says so by standing still.**
    ///
    /// SCE's own doc states this: *"a type dispatched with nothing registered
    /// raises `error.execution` exactly as an undeclared one would, because
    /// from the document's point of view nothing performed the act either
    /// way"*. Asserted here rather than trusted, because the two halves — the
    /// build's declaration and the run-time registration — are edited in
    /// different files and nothing else checks that both are present.
    #[test]
    fn r1796_a_declared_type_with_no_handler_performs_nothing() {
        let mut e = engine();
        e.process_event(HostServedSendEvent::Go);
        assert_eq!(
            e.get_current_state(),
            HostServedSendState::Asking,
            "the act was dispatched and nothing served it, so no reply exists \
             to move the machine on"
        );
    }
}
