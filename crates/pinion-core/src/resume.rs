//! ★★★★★ R1768 §5.4 — **putting a statechart back where it was, without
//! replaying the way it got there.**
//!
//! # What forced this module
//!
//! `pinion-rpc`'s `rewind` module has carried this sentence since it was
//! written: `scene/query` + `scene/rewind` are "the symbolic snapshot/restore
//! primitive that anchors §5.8 `dry_run` **once an engine-level hook is wired
//! in**". The hook did not exist. The engine published the two readers a
//! snapshot needs — `get_active_states` and `get_current_state` — and nothing
//! that could take their values back.
//!
//! The SCE revision this round pins adds the other half (`Engine::enter_at`,
//! and the `StatePolicy::set_active_states` the generator now emits beside the
//! reader). This module is pinion taking it.
//!
//! # Restore is not replay, and that is the whole point
//!
//! The obvious way to put a machine into a state is to construct it and drive
//! it there. That re-runs every `<onentry>` on the way in, and W3C SCXML 3.8
//! says entry actions are executed when the state is entered — so a chart whose
//! entry action sends, raises or invokes does all of it a second time, to a
//! world that already saw it once. A host restoring a persisted session wants
//! the configuration, not the side effects.
//!
//! [`resume_at`] enters the configuration and runs no entry action at all. The
//! test module below measures that rather than asserting it: the fixture chart
//! `self_answering_raise.scxml` raises an event from `<onentry>` whose own
//! transition raises it again, so **entering `spin` by driving costs a
//! truncated macrostep and entering it by resuming costs none.** The engine's
//! own truncation counter is the instrument, and it can tell the two apart.
//!
//! # What it refuses
//!
//! Every chain that is not a configuration of that document. The engine
//! validates before it mutates, so a refused resume leaves the machine exactly
//! as it was — "entered near the requested configuration" is the outcome that
//! must never happen, because a caller has no way to detect it afterwards. The
//! rejection is passed through rather than flattened: its arms name the
//! W3C SCXML 3.3 / 3.4 rule that was broken, and a caller that gets told
//! only "no" cannot fix its snapshot.
//!
//! ⚠ Those clause numbers are the W3C recommendation's and are deliberately
//! written without a `§`: in a tracked file of this repository that sigil
//! claims THIS store, which is the class R1766 found and repaired for the
//! generated modules. Here it is simply not written.
//!
//! # ⚠ Two senses of "configuration" in this tree
//!
//! [`Configuration`] here is the **SCXML** sense — the set of states a machine
//! is simultaneously in (W3C SCXML 3.3). It is unrelated to
//! `widgets::config_form`
//! and `ConfigDefect`, which are the *settings-form* sense. The spec's word is
//! kept because renaming a spec concept costs every reader who knows the spec;
//! the module name is what disambiguates at the use site.
//!
//! # What it does not do
//!
//! No datamodel restore. The engine declares `<data>` with its document
//! defaults and does not persist variable values, so a host with saved
//! variables puts them back itself. Saying so here rather than leaving it to be
//! discovered: a snapshot that silently omits half the state is worse than one
//! that says which half it is.

use sce_rust_runtime::helpers::hierarchy::StateChain;
use sce_rust_runtime::{ConfigurationRejection, Engine, StatePolicy};
use serde::{Deserialize, Serialize};

use crate::widget_core::WidgetStateName;

/// A configuration a machine was in, and can be put back into.
///
/// Holds **both** values the engine's restore door requires, because for a
/// machine with `<parallel>` states one does not determine the other: the
/// configuration says which states are active, and the current state says which
/// region the machine descended into, which is a fact about transition history
/// rather than about the configuration. For a machine without parallel states
/// the current state is the chain's leaf and carrying it costs nothing.
///
/// Opaque on purpose beyond the two accessors: a caller that could build one
/// field-by-field would be building chains the engine then has to refuse, and
/// the only chain worth having is one a real machine was actually in.
///
/// ★ R1769 — **it serialises, and that is what puts it on the wire.** The
/// statechart codegen already derives `Serialize` / `Deserialize` on every
/// generated state enum (the SCE-002/004 caller-injectable derives), so this
/// needed nothing new from the generator. A client therefore reads a
/// configuration and hands the same value back, rather than naming a state and
/// leaving the framework to invent the chain around it — which it cannot do
/// honestly, because the engine's ancestor walk publishes leaf-first and
/// `hierarchy::build_entry_chain` builds root-first, and choosing between them
/// would be this crate deciding a fact about somebody else's document.
///
/// ⚠ **This type carries no document identity, and deliberately does not.**
/// Two widgets generated from one statechart template have identical state
/// vocabularies, so one's configuration deserialises into the other's type and
/// validates against the other's document — measured on `button` and `toggle`.
/// Refusing that is a judgment about *which widget a snapshot came from*, which
/// this module cannot make and the wire layer can: `widget_core`'s
/// `widget_configuration` stamps the kind and its `resume_widget` checks it.
/// Keeping the split means this stays a statement about statecharts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Configuration<S> {
    states: StateChain<S>,
    current: S,
}

impl<S: Copy> Configuration<S> {
    /// The leaf the machine had descended to.
    #[must_use]
    pub fn current(&self) -> S {
        self.current
    }

    /// Every state the machine was simultaneously in, root-ward order as the
    /// engine publishes it.
    #[must_use]
    pub fn states(&self) -> &[S] {
        &self.states
    }
}

/// Take the configuration `engine` is in.
///
/// The pair this returns is exactly what [`resume_at`] takes, and taking it is
/// free of side effects — a snapshot never drives the machine.
pub fn configuration_of<P: StatePolicy>(engine: &Engine<P>) -> Configuration<P::State> {
    Configuration {
        states: engine.get_active_states(),
        current: engine.get_current_state(),
    }
}

/// Put `engine` back into `saved`, running no `<onentry>`.
///
/// The machine is left running and settled where it was; the caller drives it
/// on from there as it otherwise would. No macrostep is run, because the
/// configuration handed in was already a settled one and stepping here could
/// take an eventless transition the earlier run had no reason to take.
///
/// # Errors
///
/// Returns the engine's `ConfigurationRejection` when `saved` is not a
/// configuration of this document — passed through unflattened so the caller is
/// told *which* rule failed. Validation runs before any mutation, so a refused
/// call leaves `engine` untouched.
pub fn resume_at<P: StatePolicy>(
    engine: &mut Engine<P>,
    saved: &Configuration<P::State>,
) -> Result<(), ConfigurationRejection<P::State>> {
    engine.enter_at(&saved.states, saved.current)
}

/// ★★★★★ R1769 §5.15 — **why a resume was refused, as a sentence naming the
/// word the caller sent.**
///
/// The engine's rejection is a Rust value whose `Debug` prints GENERATED
/// identifiers, and this tree refuses a refusal spelled that way: R1699 fixed it
/// on the person's channel and R1720 on the agent's, and `Fault::DebugSpelling`
/// is the gate. So an operator who hands back a configuration this document
/// cannot hold gets a clause naming the rule and the state, not
/// `CurrentNotAtomic { current: Root }`.
///
/// ⚠ **It renders with `WidgetStateName::as_name`, deliberately NOT with the
/// engine's advice.** Upstream's own doc says to render a rejected state with
/// `StatePolicy::get_state_name`, "the vocabulary a host persisted it under" —
/// and for a host reading SCXML ids that is right. It is wrong HERE: this
/// framework's wire answers `query("state")` from `as_name`, so a caller's
/// vocabulary is `Pressed` while `get_state_name` would say `pressed`. A
/// refusal that names a word the caller never sent is a refusal they cannot act
/// on, so the rule is the caller's spelling wins.
///
/// The match is exhaustive on purpose: `ConfigurationRejection` is not
/// `#[non_exhaustive]`, so an arm upstream adds arrives here as a build error,
/// which is the moment to decide what it should say rather than a silent
/// fallback sentence.
pub fn refusal_sentence<S: WidgetStateName + Copy>(
    rejection: &ConfigurationRejection<S>,
) -> String {
    let n = |s: S| s.as_name();
    match *rejection {
        ConfigurationRejection::Empty => {
            "that configuration holds no states; a configuration always holds at least a root"
                .to_string()
        }
        ConfigurationRejection::Duplicate { state } => {
            format!("{} appears twice in that configuration", n(state))
        }
        ConfigurationRejection::AncestorMissing { state, parent } => format!(
            "{} is in that configuration and its parent {} is not, so the set is not closed upward",
            n(state),
            n(parent)
        ),
        ConfigurationRejection::RootCount { found } => format!(
            "that configuration closes on {found} states with no parent; a configuration has exactly one"
        ),
        ConfigurationRejection::CompoundChildCount { parent, found } => format!(
            "{} is a compound state and that configuration gives it {found} active children; it takes exactly one",
            n(parent)
        ),
        ConfigurationRejection::ParallelRegionMissing { parallel, region } => format!(
            "{} is a parallel state and that configuration omits its region {}; a parallel state is active in all of its regions at once",
            n(parallel),
            n(region)
        ),
        ConfigurationRejection::ParallelChildCount {
            parallel,
            found,
            regions,
        } => format!(
            "{} declares {regions} regions and that configuration gives it {found} children; a parallel state is entered with all of its regions and nothing else",
            n(parallel)
        ),
        ConfigurationRejection::AtomicHasChildren { state } => format!(
            "{} is atomic and that configuration gives it children",
            n(state)
        ),
        ConfigurationRejection::CurrentNotActive { current } => format!(
            "the configuration does not hold {}, which it names as the current state",
            n(current)
        ),
        ConfigurationRejection::CurrentNotAtomic { current } => format!(
            "{} is not atomic, so it is not a state a settled machine can be at; name one of its descendants instead",
            n(current)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{Configuration, configuration_of, refusal_sentence, resume_at};
    use crate::self_answering_raise::{
        SelfAnsweringRaiseEvent, SelfAnsweringRaisePolicy, SelfAnsweringRaiseState,
    };
    use sce_rust_runtime::{ConfigurationRejection, Engine};

    /// An engine over the entry-raising chart, already in `idle` — the same
    /// door `Widget::with_policy` uses, so this is a machine a widget runs.
    fn engine() -> Engine<SelfAnsweringRaisePolicy> {
        let mut e = Engine::new(SelfAnsweringRaisePolicy::new());
        e.initialize();
        e
    }

    /// A machine parked in `spin`, and the configuration it is in.
    ///
    /// Getting there costs one truncated macrostep — that is the premise the
    /// headline test below measures against, not an incidental detail.
    fn spun() -> (
        Engine<SelfAnsweringRaisePolicy>,
        Configuration<SelfAnsweringRaiseState>,
    ) {
        let mut e = engine();
        e.process_event(SelfAnsweringRaiseEvent::Start);
        let saved = configuration_of(&e);
        (e, saved)
    }

    /// ★★★★★ R1768 — **resuming into a state does not re-run its `<onentry>`,
    /// and the engine's own counter is what says so.**
    ///
    /// `spin`'s entry action raises an event whose transition raises it again,
    /// so *entering* `spin` is observable from outside: the macrostep cannot
    /// end itself and the engine truncates exactly one. Driving in costs one.
    /// Resuming in must cost none — and it lands in the same configuration, so
    /// looking at the state alone could not tell the two apart. That is the
    /// reason this assertion is written against the counter.
    #[test]
    fn r1768_a_resume_does_not_rerun_the_entry_action() {
        let (_driven, saved) = spun();

        let mut fresh = engine();
        assert_eq!(
            fresh.truncated_macrosteps(),
            0,
            "the premise: a fresh machine has entered nothing that runs away"
        );

        resume_at(&mut fresh, &saved).expect("a configuration a real machine was in");

        assert_eq!(
            fresh.get_current_state(),
            SelfAnsweringRaiseState::Spin,
            "it must actually be in the state that was saved"
        );
        assert_eq!(
            fresh.truncated_macrosteps(),
            0,
            "and it must have got there without the entry action firing — \
             driving to the same state costs one truncation, so a non-zero \
             count here would mean the resume replayed the way in"
        );
    }

    /// ★★★★★ R1768 — **the contrast, measured rather than asserted.**
    ///
    /// Without this the sibling above is satisfied by an engine whose counter
    /// simply never moves. Driving into the same state must cost one
    /// truncation, which is what makes zero a fact about the resume.
    #[test]
    fn r1768_driving_into_the_same_state_costs_what_resuming_does_not() {
        let (driven, _saved) = spun();

        assert_eq!(
            driven.get_current_state(),
            SelfAnsweringRaiseState::Spin,
            "both doors reach the same state"
        );
        assert_eq!(
            driven.truncated_macrosteps(),
            1,
            "and the door that runs the entry action pays for it"
        );
    }

    /// ★ R1768 — **a resumed machine is alive, not merely positioned.**
    ///
    /// A restore that set the state field and left the engine stopped would
    /// pass both tests above. `settle` is reachable only from `spin`, so
    /// answering it is the evidence that the machine is running from the
    /// configuration it was handed.
    #[test]
    fn r1768_a_resumed_machine_still_takes_events() {
        let (_driven, saved) = spun();
        let mut fresh = engine();
        resume_at(&mut fresh, &saved).expect("a configuration a real machine was in");

        fresh.process_event(SelfAnsweringRaiseEvent::Settle);

        assert_eq!(
            fresh.get_current_state(),
            SelfAnsweringRaiseState::Idle,
            "a transition out of the resumed state must be taken"
        );
    }

    /// ★★★ R1768 — **a chain this document cannot hold is refused, and the
    /// machine does not move.**
    ///
    /// The refusal matters more than the acceptance: a restore that entered
    /// "near" a bad configuration would leave a host with a machine that looks
    /// fine and is elsewhere, with nothing to detect it by. The empty chain is
    /// used because it is the one bad configuration every document rejects for
    /// the same reason, so this test says nothing about `spin` in particular.
    #[test]
    fn r1768_a_configuration_this_document_cannot_hold_is_refused() {
        let (_driven, saved) = spun();
        let mut fresh = engine();
        let before = fresh.get_current_state();

        // A saved value whose chain holds nothing — reachable only by taking a
        // real one apart, which is why `Configuration` does not let a caller
        // build one from the outside in ordinary code.
        let empty = Configuration {
            states: Vec::new(),
            current: saved.current(),
        };

        let refusal =
            resume_at(&mut fresh, &empty).expect_err("an empty chain is no configuration");

        assert!(
            matches!(refusal, ConfigurationRejection::Empty),
            "and the refusal must name the rule that failed, not merely say no: {refusal:?}"
        );
        assert_eq!(
            fresh.get_current_state(),
            before,
            "validation runs before any mutation, so a refused resume leaves \
             the machine exactly where it was"
        );
    }

    /// ★ R1768 — **the snapshot carries both readers' values.**
    ///
    /// `current` is not derivable from the chain for a machine with `<parallel>`
    /// states, so a snapshot that kept only the chain would be lossy for
    /// exactly the documents that most need restoring. This asserts the pair is
    /// carried; the parallel case itself is the sibling below.
    #[test]
    fn r1768_a_snapshot_carries_the_chain_and_the_leaf() {
        let (_driven, saved) = spun();

        assert_eq!(saved.current(), SelfAnsweringRaiseState::Spin);
        assert!(
            saved.states().contains(&SelfAnsweringRaiseState::Spin),
            "the leaf is in the chain: {:?}",
            saved.states()
        );
    }

    /// ★★★★★ R1768 — **the one chart whose emit this bump actually changed is
    /// the one chart that cannot use the door. Measured, and it is upstream's.**
    ///
    /// The pin bump moved fifteen generated modules and exactly one by more
    /// than its template hash: `multi_window_sm.rs` gained `set_active_states`,
    /// because a `<parallel>` root is the condition the generator emits it
    /// under. So the natural reading is that the parallel chart is the one that
    /// gained something. The opposite is true.
    ///
    /// `multi_window.scxml` is a `<parallel>` root whose three regions are
    /// **atomic**. Nothing descends below them, so the engine's
    /// `get_current_state` answers the parallel root — and `validate` requires
    /// the claimed current state to be atomic. The engine's own two readers
    /// therefore publish a pair its own validator refuses, and this generated
    /// `set_active_states` is unreachable in this tree.
    ///
    /// Upstream's acceptance test is titled *the configuration a run published
    /// is a configuration it can be put back into*. It does not close for this
    /// document shape, and the reason it was never seen is that upstream's
    /// parallel fixture gives every region a **compound** child, so its current
    /// state is an atomic leaf. One shape was tested; ours is the other legal
    /// one. See the sibling below for what a host can still do, and
    /// `memory/sce-upstream-debts.md` for the report.
    ///
    /// ⚠ This test asserts the DEFECT. It is meant to go red the day upstream
    /// closes the hole — that is the notification, not a regression.
    #[test]
    fn r1768_a_parallel_with_atomic_regions_publishes_a_pair_its_own_engine_refuses() {
        use crate::multi_window::MultiWindowPolicy;

        let mut ran = Engine::new(MultiWindowPolicy::new());
        ran.initialize();
        let saved = configuration_of(&ran);

        // Deliberately NOT asserting `HAS_ACTIVE_STATES` here. It is an
        // associated const, so a runtime `assert!` on it is optimised out and
        // clippy refuses it — but the deeper reason is that the fact belongs to
        // another gate: `tests/statechart_emit.rs` byte-compares the whole
        // generated tree against the pinned generator, so "the generator emits
        // the active-set pair for this policy" is already owned there. A second
        // account of one fact is what R1766 removed. What this test asserts is
        // the RUNTIME consequence below, which that gate cannot see.
        assert!(
            saved.states().len() > 1,
            "and the premise's other half: a parallel root is active together \
             with its regions, so there is a set to hand back: {:?}",
            saved.states()
        );

        let mut fresh = Engine::new(MultiWindowPolicy::new());
        fresh.initialize();
        let refusal = resume_at(&mut fresh, &saved)
            .expect_err("upstream refuses this pair — see the doc comment above");

        assert!(
            matches!(refusal, ConfigurationRejection::CurrentNotAtomic { .. }),
            "and the refusal names the rule: a parallel root is not atomic, so \
             the leaf the engine published cannot be claimed as one: {refusal:?}"
        );
    }

    /// ★★★★★ R1769 — **a refusal arrives as a sentence, in the caller's own
    /// spelling.**
    ///
    /// This is the one refusal in this tree that a wire client can actually
    /// provoke by handing back what the wire gave it (SCE-006), so it is the
    /// one that most needs to be readable. Two things are asserted and the
    /// second is the harder one: the sentence names the state, and it names it
    /// with the WIRE's word — upstream's own doc recommends
    /// `StatePolicy::get_state_name`, which for this document would say `root`,
    /// and `query("state")` answers `Root`. A refusal spelled the other way
    /// names a word the caller never sent.
    #[test]
    fn r1769_a_refusal_is_a_sentence_in_the_wire_vocabulary() {
        use crate::multi_window::MultiWindowPolicy;
        use sce_rust_runtime::StatePolicy;

        let mut ran = Engine::new(MultiWindowPolicy::new());
        ran.initialize();
        let saved = configuration_of(&ran);

        let mut fresh = Engine::new(MultiWindowPolicy::new());
        fresh.initialize();
        let refusal = resume_at(&mut fresh, &saved).expect_err("SCE-006 refuses this pair");
        let said = refusal_sentence(&refusal);

        assert!(
            said.contains("Root"),
            "it must name the state that tripped, in the vocabulary the wire \
             answers `state` with: {said}"
        );
        assert_ne!(
            <MultiWindowPolicy as StatePolicy>::get_state_name(saved.current()),
            "Root",
            "the premise of the assertion above: the engine's own recommended \
             spelling is a DIFFERENT word, so containing `Root` is evidence of \
             which vocabulary was used and not a coincidence"
        );
        assert!(
            !said.contains('{') && !said.contains("::"),
            "and it must not be a Debug spelling — `CurrentNotAtomic {{ .. }}` \
             is what R1699 and R1720 refuse on the two channels: {said}"
        );
    }

    /// ★★★ R1769 — **a configuration survives the wire round trip.**
    ///
    /// `Configuration` serialises so a client can hand back exactly what it
    /// read, rather than naming a state and leaving the framework to invent the
    /// chain around it. If this ever stopped holding, a client's saved session
    /// would restore into a machine that is somewhere else.
    #[test]
    fn r1769_a_configuration_survives_a_json_round_trip() {
        let (_driven, saved) = spun();

        let wire = serde_json::to_value(&saved).expect("a configuration serialises");
        let back: Configuration<SelfAnsweringRaiseState> =
            serde_json::from_value(wire).expect("and parses back");

        assert_eq!(
            back, saved,
            "the value a client hands back is the one it read"
        );

        let mut fresh = engine();
        resume_at(&mut fresh, &back).expect("and it is still a configuration the engine accepts");
        assert_eq!(fresh.get_current_state(), SelfAnsweringRaiseState::Spin);
        assert_eq!(
            fresh.truncated_macrosteps(),
            0,
            "and it still arrives without the entry action firing, which is the \
             property the wire leg exists to carry"
        );
    }

    /// ★★★ R1768 — **what a host can still do, and why it is not a fix.**
    ///
    /// Naming an atomic region as the current state passes validation and the
    /// generated `set_active_states` runs, so the restore door does work on
    /// this machine — the broken half is only the *published pair*. Measuring
    /// this is what turns the sibling above from "the feature is unusable here"
    /// into the narrower and true "the two readers disagree with the
    /// validator", which is the difference between a defect report upstream can
    /// act on and one it cannot.
    ///
    /// ⚠ It is not adopted as pinion's behaviour. Picking a region would be
    /// this framework inventing which window is "current" in a topology whose
    /// whole point is that all three are open at once — a semantic §5.17 does
    /// not have. `resume_at` passes the refusal through instead.
    #[test]
    fn r1768_naming_an_atomic_region_restores_the_parallel_machine() {
        use crate::multi_window::{MultiWindowApp, MultiWindowPolicy};

        let mut ran = Engine::new(MultiWindowPolicy::new());
        ran.initialize();
        let states = configuration_of(&ran);
        let region = MultiWindowApp::initial_window();

        let named = Configuration {
            states: states.states().to_vec(),
            current: region,
        };

        let mut fresh = Engine::new(MultiWindowPolicy::new());
        fresh.initialize();
        resume_at(&mut fresh, &named)
            .expect("an atomic region of the active configuration is a legal claim");

        assert_eq!(
            fresh.get_active_states(),
            named.states(),
            "the set the policy reports back is the one it was handed — this \
             reads through the `set_active_states` this bump emitted, so an \
             engine that wrote only `current_state` would differ here"
        );
        assert_eq!(fresh.get_current_state(), region);
    }
}
