//! R1789 — **the scenario**: what happens to this graph, and when.
//!
//! The analysis-tool census asks for *a scenario timeline: warmup, sequential
//! and concurrent tasks, and killing a node at a given time*, and recorded that
//! a graph which stops a node at eight seconds had **no authoring surface**.
//! Re-measured before this round wrote a line, that reason held — the first
//! census reason in four rounds to survive re-measurement. This screen had a
//! `run` that is a **boolean** and no clock at all, so there was nowhere to put
//! the eight seconds.
//!
//! # What is the framework's and what is this screen's
//!
//! The framework owns the shape: [`Schedule`] is named lanes of timed entries,
//! its `place` refuses a second entry at one moment instead of swallowing it,
//! and `due` answers **which entries a step just crossed** — the query an
//! interpolating keyframe API has no equivalent of, and the one a scenario is
//! made of.
//!
//! This screen owns the **taxonomy**, which the framework deliberately does
//! not: what an entry MEANS. [`Act`] is that vocabulary, and it is four words
//! because those are the four things this tool's reference does to a running
//! graph.
//!
//! # ★★ The clock is explicit, and that is R1600's division rather than a
//! shortcut
//!
//! A scenario advances because somebody advanced it — [`advance`] takes the
//! seconds and answers what it crossed. `run` does not turn a clock; a tick
//! does. A wall-clock driver here would make every assertion about this screen
//! depend on how fast the machine is, which this project's zero-flake rule
//! forbids, and it would hide the one fact a person wants: the entries a step
//! passed, in order.

use std::rc::Rc;

use pinion_core::external::{ArgCase, InvokeError, SchemaArg};
use pinion_core::regression::{Mark, Regression, Timeline};
use pinion_core::scene::Rect;
use pinion_core::theme::StateTone;
use pinion_core::widgets::track::{Misplaced, Schedule, Seconds};
use serde_json::Value;

use crate::{LabState, ROOT};

/// What a scenario entry does to the graph.
///
/// The taxonomy is this screen's, not the framework's — a track holds whatever
/// its consumer puts in it, exactly as `pinion-node-graph` holds a node kind it
/// does not name. Four words because those are the four things the reference
/// does to a running graph: bring one up, take one down, take one down for
/// good, and wait.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    /// Let the graph settle before anything is measured.
    Warmup,
    /// Switch a card on.
    Start,
    /// Switch a card off.
    Stop,
    /// Take a card down as a fault, rather than as an operator would.
    ///
    /// ★ The same effect on the graph as [`Stop`](Self::Stop) and a different
    /// **meaning**, which is why it is a word of its own: a scenario that
    /// injects a fault at eight seconds and one that shuts a node down cleanly
    /// are testing different things, and a reader of the plan has to be able to
    /// tell them apart. The reference groups them under one heading and this
    /// does not.
    Kill,
    /// ★★★★★ R1844 — **assert that a card is up, and wait a stated while for
    /// it.** The census's `lab.t1.9`, *checkpoints and assertions with a
    /// timeout*.
    ///
    /// The fifth word, and the first that CHANGES NOTHING. Every act above
    /// tells the graph to be different; this one asks whether it is, and the
    /// scenario is worth less than it looks without one — a plan that starts a
    /// node at two seconds and kills it at eight has, until now, no way to say
    /// *and it should have been running in between*. A person reading the
    /// timeline could see what was commanded and never what was expected.
    ///
    /// ⚠ **The timeout is what makes it an assertion rather than a sample.**
    /// Checked only at the instant it is crossed, a checkpoint asserts
    /// something about one moment of a discrete clock, which is a fact about
    /// the step size a caller happened to advance by. With a deadline it
    /// asserts something about an INTERVAL: true if the card comes up at any
    /// point before it expires, failed when the playhead passes it still
    /// unmet. That is the difference between a scenario that can be replayed
    /// at a different step and one that cannot.
    Check,
}

impl Act {
    /// Every arm, in wire order.
    pub const ALL: &'static [Self] = &[
        Self::Warmup,
        Self::Start,
        Self::Stop,
        Self::Kill,
        Self::Check,
    ];

    /// Every arm's wire name, in [`ALL`](Self::ALL) order — the closed
    /// vocabulary the `schedule` action's argument domain is drawn from.
    pub const WIRE_NAMES: &'static [&'static str] = &["warmup", "start", "stop", "kill", "check"];

    /// The wire name of this act.
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Kill => "kill",
            Self::Check => "check",
        }
    }

    /// Whether this act carries a deadline — true for [`Check`](Self::Check)
    /// alone.
    ///
    /// ★ R1844 — this is what the `schedule` action's CONDITIONAL argument is
    /// derived from, so the wire's case table and the dispatcher cannot
    /// disagree about which word brings a timeout. R1630's ratchet applied to
    /// a mapping rather than to a count: a hand-written case table can be
    /// wrong about what a value implies, not merely about how many there are.
    #[must_use]
    pub const fn needs_timeout(self) -> bool {
        matches!(self, Self::Check)
    }

    /// The act `name` names, or `None`.
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.as_wire_name() == name)
    }

    /// Whether this act needs a card to happen to.
    ///
    /// A warmup is about the graph and not about any one node, so demanding a
    /// target for it would make every scenario carry a meaningless one.
    #[must_use]
    pub const fn needs_target(self) -> bool {
        !matches!(self, Self::Warmup)
    }
}

/// The `act` argument's case table: every word, and what choosing it adds.
///
/// ★★★★★ R1844 — DERIVED from [`Act`] rather than spelled at the call site,
/// which is R1630's ratchet applied to a mapping. A hand-written table can
/// disagree with the dispatcher about what a value IMPLIES, not merely about
/// how many values there are — and this table's whole content is an
/// implication: `check` brings a timeout and the other four do not.
pub const ACT_CASES: [ArgCase; Act::ALL.len()] = {
    let mut cases = [ArgCase::EMPTY; Act::ALL.len()];
    let mut n = 0;
    while n < Act::ALL.len() {
        let act = Act::ALL[n];
        cases[n] = ArgCase::new(
            act.as_wire_name(),
            if act.needs_timeout() {
                const { &[SchemaArg::open("timeout", "number")] }
            } else {
                &[]
            },
        );
        n += 1;
    }
    cases
};

/// One scheduled thing: an act, the card it happens to, and how long it will
/// wait.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// What happens.
    pub act: Act,
    /// The card it happens to, or empty for an act that needs none.
    pub target: String,
    /// How long a [`Check`](Act::Check) keeps waiting, and `None` for every
    /// act that does not wait.
    ///
    /// ★ [`Seconds`] and not `f32`, for the reason [`conflicts`] gives about
    /// its own key: the framework hands back a VALIDATED moment, finite by
    /// construction, so `Eq` on it is a real equality rather than the float
    /// comparison a lint rightly refuses. An `Entry` that derived `Eq` around a
    /// raw `f32` could not exist.
    pub timeout: Option<Seconds>,
}

/// This screen's scenario: named lanes of [`Entry`].
pub type Plan = Schedule<Entry>;

/// One checkpoint the playhead has raised, and what it has decided.
///
/// ★★★★★ R1844 — the verdict is a THREE-valued thing and the third value is
/// the point. `Some(true)` is met, `Some(false)` is failed, and `None` is
/// *still waiting* — a checkpoint whose card is not up yet but whose deadline
/// has not passed. Collapsing that into `false` would make every assertion
/// depend on the step the caller advanced by, which is precisely what the
/// timeout exists to stop.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Checkpoint {
    /// The moment the checkpoint was scheduled at.
    pub at: Seconds,
    /// The card it asks about.
    pub target: String,
    /// The moment after which waiting stops.
    pub deadline: Seconds,
    /// Met, failed, or still waiting.
    pub met: Option<bool>,
}

impl Checkpoint {
    /// The word the wire reports this verdict as.
    ///
    /// ★★★★★ R2024 — [`MarkKind`]'s, not a second list. The band paints this
    /// verdict and names it in the accessibility tree, and a screen whose word
    /// and a wire whose word came from two `match`es is this project's
    /// commonest defect. One function, two readers.
    #[must_use]
    pub const fn verdict(&self) -> &'static str {
        MarkKind::raised(self.met).word()
    }
}

/// The lane a scheduled act goes on when the caller does not say.
pub const DEFAULT_LANE: &str = "main";

/// Put `act` on `lane` at `at`, targeting `target`.
///
/// # Errors
/// A refusal naming what was wrong: an unknown act, an act that needs a card
/// with no card or a card this graph does not have, a time no track can hold,
/// or a moment already spoken for on that lane.
pub fn schedule(
    state: &Rc<LabState>,
    lane: &str,
    at: f32,
    act: &str,
    target: &str,
    timeout: Option<f32>,
) -> Result<String, InvokeError> {
    let Some(act) = Act::from_wire_name(act) else {
        return Err(InvokeError::rejected(format!(
            "no act named {act:?}; this scenario does {}",
            pinion_core::external::one_of_phrase(Act::WIRE_NAMES.iter().copied())
        )));
    };
    let target = target.trim();
    if act.needs_target() {
        if target.is_empty() {
            return Err(InvokeError::rejected(format!(
                "{} needs a card to happen to",
                act.as_wire_name()
            )));
        }
        if state.node_of(target).is_none() {
            return Err(InvokeError::rejected(format!(
                "there is no card called {target:?} to {}",
                act.as_wire_name()
            )));
        }
    } else if !target.is_empty() {
        return Err(InvokeError::rejected(format!(
            "{} is about the whole graph, so it takes no card — {target:?} was given",
            act.as_wire_name()
        )));
    }
    // ★★★★★ R1844 — the timeout is REQUIRED by the act that waits and REFUSED
    // by every act that does not, and both directions are refusals rather than
    // one being quietly ignored.
    //
    // A `check` with no deadline would be a sample dressed as an assertion —
    // true or false about one instant of whatever step the caller happened to
    // advance by. A `kill` carrying one would be a number a reader can see in
    // the plan and nothing will ever consult, which is the worse of the two:
    // an ignored argument is a lie the surface tells once and keeps telling.
    let timeout = match (timeout, act.needs_timeout()) {
        (Some(secs), true) => Some(Seconds::new(secs).map_err(refusal)?),
        (None, true) => {
            return Err(InvokeError::rejected(format!(
                "{} waits, so it needs a timeout in seconds",
                act.as_wire_name()
            )));
        }
        (Some(_), false) => {
            return Err(InvokeError::rejected(format!(
                "{} happens at its moment and does not wait, so it takes no timeout",
                act.as_wire_name()
            )));
        }
        (None, false) => None,
    };
    let entry = Entry {
        act,
        target: target.to_owned(),
        timeout,
    };
    state
        .scenario
        .borrow_mut()
        .track(lane)
        .place(at, entry)
        .map(|when| format!("{} at {when} on {lane}", act.as_wire_name()))
        .map_err(refusal)
}

/// Take the entry at `at` off `lane`.
///
/// # Errors
/// A refusal naming the moment when nothing is there, or the time itself when
/// it is not one a track can hold.
pub fn unschedule(state: &Rc<LabState>, lane: &str, at: f32) -> Result<String, InvokeError> {
    let mut plan = state.scenario.borrow_mut();
    let entry = plan.track(lane).remove(at).map_err(refusal)?;
    Ok(format!("{} at {at}s off {lane}", entry.act.as_wire_name()))
}

/// Advance the playhead by `by` seconds, doing everything it crosses.
///
/// Answers the entries it crossed, in the order they happen — which is the
/// whole reason a scenario is a track and not a curve. Advancing by zero or
/// backwards crosses nothing and says so rather than refusing: a scrub that
/// went nowhere is a legitimate thing to have asked for.
///
/// # Errors
/// A refusal when `by` is not a number of seconds a clock can move by.
pub fn advance(state: &Rc<LabState>, by: f32) -> Result<Value, InvokeError> {
    if !by.is_finite() {
        return Err(InvokeError::rejected(format!(
            "{by} is not a number of seconds"
        )));
    }
    let from = state.playhead.get();
    let to = (from + by).max(0.0);
    // ★ The FIRST advance has to include an entry at zero, and `due` is open
    // below on purpose (see its doc — that is what stops a boundary entry being
    // delivered twice). `from_start` is the window that has no lower bound.
    let crossed: Vec<(String, f32, Entry)> = {
        let plan = state.scenario.borrow();
        let upto = Seconds::new(to).map_err(refusal)?;
        let window = if from <= 0.0 && by > 0.0 {
            plan.from_start(upto)
        } else {
            let after = Seconds::new(from).map_err(refusal)?;
            plan.due(after, upto)
        };
        window
            .into_iter()
            // ★ R1844 — the entry's OWN moment is carried through, because a
            // checkpoint's deadline is measured from when it was scheduled and
            // not from where the playhead happens to have stopped. A step that
            // crosses a checkpoint and runs on would otherwise give it a
            // deadline that grows with the caller's step size.
            .map(|(lane, key)| (lane.to_owned(), key.at().secs(), key.value().clone()))
            .collect()
    };
    // ★ R1844 — a restart forgets the previous run's verdicts. The window
    // above already treats `from <= 0` as the beginning; a checkpoint that
    // survived it would report on a graph state that no longer exists.
    //
    // ★★★★★ R1866 — **and REWINDING is a restart too, which it was not.** The
    // condition was *at the beginning and going forward*, so a rewind landed
    // the playhead at zero and left the previous run's verdicts standing: a
    // reader who scrubbed back to the start was shown checkpoints decided about
    // a run that, as far as the playhead was concerned, had not happened. Found
    // by R1866's demo, which rewound and then asked whether there was a run to
    // keep — and there was one, from before the rewind.
    //
    // The rule is one sentence and it covers both: **a playhead at the start
    // means this run has not happened.**
    if to <= 0.0 || (from <= 0.0 && by > 0.0) {
        state.checks.borrow_mut().clear();
        // The tape goes with them: a mark from the previous run of a plan would
        // make the regression below compare two runs that were never separate.
        *state.tape.borrow_mut() = Timeline::new(pinion_core::regression::Scale::Seconds);
    }
    state.playhead.set(to);

    let mut done: Vec<Value> = Vec::new();
    for (lane, at, entry) in crossed {
        // ★★★★★ R1844 — a check RAISES rather than does. Every other act is
        // finished when it is crossed; this one starts waiting, and `done` is
        // false for it because nothing about the graph moved. Reporting a
        // check as `done: true` would make the crossing log say the scenario
        // changed something when its whole purpose is that it did not.
        if entry.act == Act::Check {
            let wait = entry.timeout.map_or(0.0, Seconds::secs);
            let (Ok(raised), Ok(deadline)) = (Seconds::new(at), Seconds::new(at + wait)) else {
                continue;
            };
            state.checks.borrow_mut().push(Checkpoint {
                at: raised,
                target: entry.target.clone(),
                deadline,
                met: None,
            });
        }
        let outcome = apply(state, entry.act, &entry.target);
        // ★★★★★ R1866 — the crossing goes on the tape, named by WHAT it did to
        // WHICH card and placed at its own moment.
        //
        // The name is the identity a regression pairs on, so it has to name the
        // thing rather than the occurrence: `stop router` is the same event in
        // both runs whether it happened at 8 seconds or at 12, and that is
        // exactly the difference a regression exists to report. The lane is not
        // in it — a lane is where an author put an entry, and moving an entry
        // between lanes does not change what happened to the graph.
        if let Some(mark) = Mark::new(
            format!("{} {}", entry.act.as_wire_name(), entry.target),
            f64::from(at),
        ) {
            state.tape.borrow_mut().place(mark);
        }
        done.push(serde_json::json!({
            "lane": lane,
            // ★ R1866 — the MOMENT, which was missing. `crossed` said what
            // happened and to what and left out when, so a reader of the wire
            // could not rebuild the run it had just watched.
            "at": at,
            "act": entry.act.as_wire_name(),
            "target": entry.target,
            "done": outcome,
        }));
    }
    settle_checks(state, to);
    Ok(serde_json::json!({
        "playhead": to,
        "crossed": done,
        "checks": checks_wire(state),
    }))
}

/// Decide every checkpoint the playhead can now decide.
///
/// ★ Runs AFTER the acts of this step, so a checkpoint scheduled at the same
/// moment as the `start` it is waiting for sees the started card. The
/// alternative — deciding before — would make a plan that starts and checks at
/// one second depend on lane order, which is exactly the defect [`conflicts`]
/// exists to report rather than one to introduce.
fn settle_checks(state: &Rc<LabState>, now: f32) {
    let mut checks = state.checks.borrow_mut();
    for check in checks.iter_mut().filter(|c| c.met.is_none()) {
        if is_running(state, &check.target) {
            check.met = Some(true);
        } else if now > check.deadline.secs() {
            check.met = Some(false);
        }
    }
}

/// Whether `target` names a card that is currently up.
fn is_running(state: &Rc<LabState>, target: &str) -> bool {
    state.node_of(target).is_some_and(|node| {
        state
            .doc
            .borrow()
            .tree(ROOT)
            .and_then(|tree| tree.node(node))
            .is_some_and(|slot| !slot.disabled)
    })
}

/// Every checkpoint raised so far, as the wire reads it.
fn checks_wire(state: &LabState) -> Vec<Value> {
    state
        .checks
        .borrow()
        .iter()
        .map(|check| {
            serde_json::json!({
                "at": check.at.secs(),
                "target": check.target,
                "deadline": check.deadline.secs(),
                "verdict": check.verdict(),
            })
        })
        .collect()
}

/// Do one act, and say whether the graph moved.
///
/// A `Kill` or a `Stop` on a card that is already off is reported as `false`
/// rather than refused: a scenario is a script, and a script that stops when
/// the world is already in the state it asked for is one nobody can replay.
fn apply(state: &Rc<LabState>, act: Act, target: &str) -> bool {
    // ★ R1844 — a check moves nothing, so it reports `false` here and its
    // verdict is carried by `checks` instead. Folding it into this boolean
    // would put an assertion's outcome in a field that means "the graph
    // changed", and a reader would have no way to tell a met checkpoint from a
    // node that was switched on.
    if act == Act::Check {
        return false;
    }
    let Some(node) = state.node_of(target) else {
        return false;
    };
    let want_off = matches!(act, Act::Stop | Act::Kill);
    let is_off = state
        .doc
        .borrow()
        .tree(ROOT)
        .and_then(|tree| tree.node(node))
        .is_some_and(|slot| slot.disabled);
    if is_off == want_off {
        return false;
    }
    state
        .doc
        .borrow_mut()
        .set_disabled(ROOT, node, want_off)
        .is_ok()
}

/// Whether this act leaves its target running.
///
/// The pair `Start` / (`Stop` | `Kill`) is what makes two acts at one moment
/// contradictory; `Warmup` has no target and cannot contradict anything.
const fn leaves_running(act: Act) -> Option<bool> {
    match act {
        // ★ R1844 — a `check` answers `None` for the same reason `warmup`
        // does, arrived at from the opposite direction: warmup has no target
        // to contradict anything about, and a check has one but commands it
        // nothing. Two acts contradict when they tell one card to be two
        // things; asking is not telling, so a check beside a kill at one
        // moment is a plan that asserts and then acts, which is legitimate.
        Act::Warmup | Act::Check => None,
        Act::Start => Some(true),
        Act::Stop | Act::Kill => Some(false),
    }
}

/// ★★★★★ R1789 — moments where one card is told **two opposite things at once**.
///
/// Found by driving the thing rather than by reading it: scheduling `kill P-01`
/// on one lane and `start P-01` on another at the same second runs both, and
/// which one survives is decided by lane order — stable, stated, and not
/// something the author asked for. That is the shape this round is about, so it
/// is REPORTED rather than left to be discovered from a card that did not end
/// up where the scenario said.
///
/// **Reported and not refused**, which is the opposite call to R1788's shared
/// name, and the difference is when the fact becomes true. A duplicate node
/// name breaks the artifact at the moment it is written. A scenario is authored
/// one entry at a time, and the second half of a pair a person is midway
/// through placing is not an error yet — refusing it would make the tool refuse
/// the ordinary act of moving an entry out of the way.
#[must_use]
pub fn conflicts(plan: &Plan) -> Vec<Value> {
    /// One card's acts at one moment, while they are being gathered.
    ///
    /// ★ Keyed by [`Seconds`] and not by a raw `f32`. The framework hands back a
    /// **validated** moment — finite by construction, so `Eq` on it is a real
    /// equality rather than the float comparison a lint rightly refuses — and
    /// reaching past it to the number would have been re-deriving the very
    /// thing the type exists to have decided once.
    struct Moment<'a> {
        at: Seconds,
        target: &'a str,
        acts: Vec<(&'a str, Act)>,
    }

    let mut rows: Vec<Moment<'_>> = Vec::new();
    for lane in plan.lanes() {
        let Some(track) = plan.get(lane) else {
            continue;
        };
        for key in track.keys() {
            let entry = key.value();
            if entry.target.is_empty() {
                continue;
            }
            let at = key.at();
            match rows
                .iter_mut()
                .find(|row| row.at == at && row.target == entry.target)
            {
                Some(row) => row.acts.push((lane, entry.act)),
                None => rows.push(Moment {
                    at,
                    target: &entry.target,
                    acts: vec![(lane, entry.act)],
                }),
            }
        }
    }
    rows.retain(|row| {
        let mut running = row.acts.iter().filter_map(|(_, act)| leaves_running(*act));
        let first = running.next();
        running.any(|later| Some(later) != first)
    });
    rows.sort_by(|a, b| a.at.cmp(&b.at));
    rows.into_iter()
        .map(|row| {
            serde_json::json!({
                "at": row.at.secs(),
                "target": row.target,
                "acts": row
                    .acts
                    .iter()
                    .map(|(lane, act)| serde_json::json!({
                        "lane": lane,
                        "act": act.as_wire_name(),
                    }))
                    .collect::<Vec<_>>(),
                "why": format!(
                    "{} is told {} things at {}, and which one lasts is decided \
                     by lane order rather than by the scenario",
                    row.target,
                    row.acts.len(),
                    row.at
                ),
            })
        })
        .collect()
}

/// The scenario as the wire reads it: the lanes, what is on them, how long it
/// is, where the playhead stands, and any moment that contradicts itself.
#[must_use]
pub fn wire(state: &LabState) -> Value {
    let plan = state.scenario.borrow();
    let lanes: Vec<Value> = plan
        .lanes()
        .into_iter()
        .map(|lane| {
            let track = plan.get(lane).expect("a lane the schedule just named");
            serde_json::json!({
                "lane": lane,
                "duration": track.duration().secs(),
                "entries": track
                    .keys()
                    .iter()
                    .map(|key| serde_json::json!({
                        "at": key.at().secs(),
                        "act": key.value().act.as_wire_name(),
                        "target": key.value().target,
                        // ★ R1844 — null for an act that does not wait, rather
                        // than absent. A reader diffing two entries can then
                        // see that one has no deadline, where a missing key
                        // reads as "this surface did not say".
                        "timeout": key.value().timeout.map(Seconds::secs),
                    }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::json!({
        "playhead": state.playhead.get(),
        "duration": plan.duration().secs(),
        "acts": Act::WIRE_NAMES,
        // ★★★★★ R2024 — the closed set of words the BAND can show, built from
        // the enum rather than written here, so what an agent is told the
        // screen can say cannot drift from what it says. R2001's rule for
        // `classify_pin`'s classes, applied to a read: it is wider than
        // `checks[].verdict` on purpose, because two of the five are states a
        // checkpoint is in before it is raised or is not a checkpoint at all,
        // and a client rendering this band needs the whole vocabulary.
        "row_states": MarkKind::ALL.map(MarkKind::word),
        "lanes": lanes,
        "conflicts": conflicts(&plan),
        "checks": checks_wire(state),
    })
}

/// ★★★★★ R1866 — **keep this run, so the next one can be compared with it.**
///
/// The act that makes a regression possible at all. It takes what the playhead
/// has crossed so far and puts it aside as the run to measure against — a
/// deliberate choice rather than an automatic one, because a baseline nobody
/// picked is a comparison with an accident.
///
/// # Errors
///
/// Refuses an empty tape: a baseline of nothing would report every later run as
/// pure gain, which reads like a finding and is an artefact of when somebody
/// pressed this.
pub fn record(state: &Rc<LabState>) -> Result<Value, InvokeError> {
    let tape = state.tape.borrow().clone();
    if tape.is_empty() {
        return Err(InvokeError::rejected(
            "nothing has been crossed yet, so there is no run to keep — advance \
             the playhead first"
                .to_owned(),
        ));
    }
    let kept = tape.len();
    *state.baseline.borrow_mut() = Some(tape);
    Ok(serde_json::json!({ "kept": kept }))
}

/// ★★★★★ R1866 — **what this run did differently from the one that was kept.**
///
/// The census asks for *two runs of one graph compared on order and latency
/// distribution*, and this is that comparison published. Both halves come out
/// of one mechanism, because they are one axis at two scales: the marks carry
/// seconds, so `shifted` is the latency half and the ORDER is recoverable from
/// the same numbers. See [`pinion_core::regression`] for why writing two
/// comparators would have been writing one rule twice.
///
/// `baseline: null` until a reader records one — stated rather than empty, so a
/// client can tell *nothing to compare with* from *nothing changed*, which are
/// the two answers a summary of zero shifts could otherwise mean.
pub fn regression_wire(state: &LabState) -> Value {
    let tape = state.tape.borrow();
    let Some(baseline) = state.baseline.borrow().clone() else {
        return serde_json::json!({
            "baseline": Value::Null,
            "now": tape.len(),
            "why": "no run has been kept to compare with",
        });
    };
    match Regression::between(&baseline, &tape) {
        Ok(diff) => {
            let mut out = diff.to_json();
            out["baseline"] = serde_json::json!(baseline.len());
            out["now"] = serde_json::json!(tape.len());
            // ★★★★★ R1866 — the LATENCY DISTRIBUTION half of the row, as five
            // landmarks over the shifts.
            //
            // `null` when nothing moved, and that is the chart crate's refusal
            // carried through rather than papered over: a summary of no samples
            // is a picture of nothing, and *nothing moved* is already said by
            // `clean`. A reader who gets a null here and `clean: false` is
            // looking at a run that only gained or lost marks — which is a
            // different finding from one that slipped.
            out["distribution"] = distribution_wire(&diff.shifts());
            out
        }
        // Unreachable while both timelines are this screen's, and answered
        // rather than unwrapped: the refusal is a fact the wire can carry, and
        // a panic here would take the whole screen down for a comparison.
        Err(why) => serde_json::json!({
            "baseline": baseline.len(),
            "now": tape.len(),
            "why": why.to_string(),
        }),
    }
}

/// ★★★★★ R1866 — the five landmarks of a set of shifts, or why there are none.
///
/// The join the census row names: `pinion_core::regression` says what moved and
/// by how much, `pinion_chart::Distribution` turns amounts into a summary, and
/// this is the one place that puts them together. Published as the numbers
/// rather than as a drawing — a client that wants a box plot has the five, and
/// one that wants the sentence has `sentence`.
fn distribution_wire(shifts: &[f64]) -> Value {
    use pinion_chart::{Distribution, QuantileMethod};

    match Distribution::from_samples("shift", shifts, QuantileMethod::Linear) {
        Ok(summary) => serde_json::json!({
            "lower": summary.lower_whisker(),
            "q1": summary.q1(),
            "median": summary.median(),
            "q3": summary.q3(),
            "upper": summary.upper_whisker(),
            "outliers": summary.outliers(),
            "samples": shifts.len(),
        }),
        // The refusal is a fact, and the reason is carried rather than
        // flattened to null: "no samples" and "no finite samples" are different
        // things to have found.
        Err(why) => serde_json::json!({ "why": why.to_string() }),
    }
}

/// A track's refusal, as a sentence on the invoke channel.
fn refusal(why: Misplaced) -> InvokeError {
    InvokeError::rejected(why.to_string())
}

// ── The strip ───────────────────────────────────────────────────────────────
//
// ★★★★★ R2024 — **what a person watching this screen can see of all of the
// above**, which until this round was nothing at all.
//
// R1844 gave the scenario an assertion whose verdict is three-valued, and every
// one of the three rides `scenario` and `advance`. R1844's own debt recorded
// that none of it is painted; re-measured at the open of R2024, the finding is
// larger than the debt's sentence — `state.checks` had exactly three readers
// and `state.playhead` two, all of them in this file, and NOT ONE PAINTER. The
// lanes, the entries, the playhead and the verdicts were all wire-only, so the
// debt's own prescription (*draw the checkpoints on the scenario lane*)
// presumed a lane that did not exist.
//
// ⚠ And the behaviour canon has no scenario at all: measured against it,
// `scenario`, `playhead`, `checkpoint` and `deadline` appear ZERO times, and
// its five *timeline* matches are a sequence-gap sparkline and a handshake
// list on two other screens. So this is second-pass work under rule (4) — the
// floor's editors show a transport — and NOT reproduction, which the debt's
// justification claimed it was.

/// What one scheduled entry has come to, as a reader of the band meets it.
///
/// ★★★★★ FIVE UNIT ARMS and not three-with-a-payload. The first draft was
/// `Raised(Option<bool>)`, mirroring [`Checkpoint::met`] — and the speech
/// census refused it, correctly: `assert_speaks` counts ARMS, so a tri-state
/// hidden inside one arm would have let two of the three verdict words go
/// undriven while the gate read as satisfied. It is also this tree's own rule
/// (*when the answer is one of three, it is a type*) applied to the view's
/// vocabulary, which is wider than the model's: the model has three verdicts
/// and the band has five things to say, because *the playhead has not reached
/// this yet* and *this is still waiting* are different states and *this act
/// happened* is neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum MarkKind {
    /// An act that happens at its moment and is over.
    Act,
    /// A checkpoint the playhead has not raised. Its interval is known from the
    /// plan, so the deadline is visible BEFORE the run — which is what lets a
    /// person read what a scenario is going to assert.
    Pending,
    /// A raised checkpoint whose card came up before its deadline.
    Met,
    /// A raised checkpoint whose deadline passed with its card still down.
    Failed,
    /// A raised checkpoint whose card is not up yet and whose deadline has not
    /// passed. R1844's third value, and the reason the timeout exists.
    Waiting,
}

impl MarkKind {
    /// Every arm, in the order a reader meets them.
    pub const ALL: [Self; 5] = [
        Self::Act,
        Self::Pending,
        Self::Met,
        Self::Failed,
        Self::Waiting,
    ];

    /// What a checkpoint the playhead HAS raised has decided.
    ///
    /// The one crossing between the model's tri-state and the view's
    /// vocabulary, so nothing else has to know that `Some(true)` is *met*.
    #[must_use]
    pub const fn raised(met: Option<bool>) -> Self {
        match met {
            Some(true) => Self::Met,
            Some(false) => Self::Failed,
            None => Self::Waiting,
        }
    }

    /// The state this mark is in, on the framework's own scale, or [`None`] for
    /// a mark that is not in a state at all.
    ///
    /// ★★★★★ The one place a verdict becomes a colour. The word a reader is
    /// told ([`Self::word`]) and the ground the mark is painted on come off two
    /// `match`es over one closed set, so a screen cannot paint *met* and
    /// announce *failed* — this tree's recurring defect, and the reason R2020
    /// put the four states behind [`StateTone`] rather than leaving them as
    /// colours a painter picks.
    #[must_use]
    pub const fn tone(self) -> Option<StateTone> {
        match self {
            // Not a state: an act is a thing that happened, and a checkpoint
            // nothing has reached has decided nothing. Painting either in a
            // state colour would tell a reader something the model does not
            // know.
            Self::Act | Self::Pending => None,
            Self::Met => Some(StateTone::Success),
            Self::Failed => Some(StateTone::Error),
            // ★ `Info` and not `Warning`: a checkpoint still inside its
            // deadline is a fact the reader is being told and asks nothing of
            // them, which is what `StateTone::Info` is for. Warning would say
            // *this wants care*, and a deadline that has not passed does not.
            // ⚠ It is also this vocabulary's FIRST painter — R2020 landed
            // `info` / `on_info` / `info_container` / `on_info_container` and
            // recorded that no screen drew them.
            Self::Waiting => Some(StateTone::Info),
        }
    }

    /// The word this mark is announced and labelled with.
    ///
    /// ★★★★★ [`Checkpoint::verdict`] — the WIRE's word — is this function, so
    /// the sentence an agent reads off `checks` and the sentence painted into
    /// the band are the same string rather than two lists that agree today.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Act => "done",
            Self::Pending => "not reached",
            Self::Met => "met",
            Self::Failed => "failed",
            Self::Waiting => "waiting",
        }
    }
}

/// One row of the strip: one scheduled entry, what it decided, and where the
/// three parts of its row go.
///
/// ★★★★★ ONE ROW PER ENTRY, which is a decision the smear gate made rather
/// than a preference. Packing several entries onto one lane row is the natural
/// first draft and it cannot be made safe: a track refuses two entries at one
/// MOMENT but a checkpoint's interval reaches past the next entry's moment, so
/// two marks on a shared row can overlap — and once a word rides a mark, two
/// overlapping marks are two words painted over each other, which is
/// `r1653_no_two_text_runs_are_painted_on_top_of_each_other`. A row apiece
/// makes that unrepresentable instead of checked for, and it is also the right
/// picture: an assertion over an interval is a bar, and bars that mean
/// different things belong on different lines.
#[derive(Clone, PartialEq, Debug)]
pub struct StripRow {
    /// The paint tag, `lab.scenario.<lane>.<at>`.
    pub tag: String,
    /// Which lane the entry is on.
    pub lane: String,
    /// The moment it happens at.
    pub at: f32,
    /// What it does.
    pub act: &'static str,
    /// The card it is about, empty for an act about the whole graph.
    pub target: String,
    /// Where waiting stops, and [`None`] for every act that does not wait.
    pub until: Option<f32>,
    /// What it has decided.
    pub kind: MarkKind,
    /// The whole row, in the band's own space.
    pub row: Rect,
    /// Where the row's words go.
    pub words: Rect,
    /// The bar: a checkpoint's interval, or an act's moment.
    pub bar: Rect,
}

impl StripRow {
    /// What the row reads, on the screen and in the tree — one string, so a
    /// reader who sees the band and one who does not are told the same thing.
    ///
    /// ★ The verdict is IN THE WORDS and not only in the colour. A band that
    /// carried the three verdicts as three fills alone would be telling a
    /// reader who cannot distinguish them nothing at all, and this screen's
    /// own inks are what the colour would have to come from — so the word is
    /// the carrier and the fill is the reinforcement.
    #[must_use]
    pub fn reads(&self, lanes: usize) -> String {
        let mut out = String::new();
        if lanes > 1 {
            out.push_str(&self.lane);
            out.push_str(" · ");
        }
        out.push_str(self.act);
        if !self.target.is_empty() {
            out.push(' ');
            out.push_str(&self.target);
        }
        if self.until.is_some() {
            out.push_str(" — ");
            out.push_str(self.kind.word());
        }
        out
    }

    /// The sentence a reader who cannot see the band is told, which says the
    /// moments the drawing shows as positions.
    #[must_use]
    pub fn sentence(&self, lanes: usize) -> String {
        match self.until {
            Some(until) => format!("{}, {}s to {until}s", self.reads(lanes), self.at),
            None => format!("{}, at {}s", self.reads(lanes), self.at),
        }
    }
}

/// The strip, as one derivation both the painter and the describing reader ask.
///
/// ★★★★★ R2022's rule, applied at the moment a surface is BUILT rather than
/// after a gate catches it: the marks a reader is told about are the marks a
/// painter was given seats for, because they are the same `Vec`. A count each
/// side keeps is what produced thirteen announced-and-undrawn rows one screen
/// over.
#[derive(Clone, PartialEq, Debug)]
pub struct Strip {
    /// The whole band, in the band's own space (origin at its top-left).
    pub band: Rect,
    /// Every row that fits, in the order they happen.
    pub rows: Vec<StripRow>,
    /// The entries there was no room for, counted rather than dropped (R1690).
    pub hidden: usize,
    /// How many lanes the plan holds, which is what decides whether a row names
    /// its own.
    pub lanes: usize,
    /// The playhead's rule, spanning the rows.
    pub playhead: Rect,
    /// The last moment the band spans.
    pub span: f32,
}

/// The band's inner padding.
const STRIP_PAD: u32 = 8;
/// The height of one row.
pub const STRIP_ROW_H: u32 = 18;
/// The width a row's words are given at the band's left edge.
pub const STRIP_GUTTER: u32 = 150;
/// The narrowest a bar is drawn, so an act that takes no time is still
/// something a person can see.
const STRIP_BAR_MIN_W: u32 = 5;
/// The type size the band is set at.
pub const STRIP_TEXT_PX: u32 = 9;
/// What a row's word-run adds to its row's tag.
///
/// ★ Published rather than spelled at the painter, because the walk that
/// judges the band has to be able to tell a row's BAR from a row's WORDS and a
/// second spelling of the suffix is how the two come to disagree.
pub const ROW_WORDS: &str = ".reads";

/// Where every part of the strip goes, or [`None`] when there is nothing to
/// show.
///
/// `room` is the rectangle the band may occupy; every rectangle answered is
/// relative to `room`'s origin.
///
/// ★ [`None`] rather than an empty band: a transport with no plan on it is
/// chrome that says nothing, and this screen's opening frame — the one the
/// reference comparison judges — is left exactly as it was.
#[must_use]
pub fn strip(state: &LabState, room: Rect) -> Option<Strip> {
    let plan = state.scenario.borrow();
    let lane_names = plan.lanes();
    if lane_names.is_empty() {
        return None;
    }
    let checks = state.checks.borrow();
    let now = state.playhead.get();
    // ★ The span is the whole plan OR wherever the playhead has got to,
    // whichever is further: a scrub past the end must not push the playhead off
    // the right-hand edge of the picture of it. Floored at one second so a plan
    // whose entries all sit at zero still has a track to sit on.
    let span = plan.duration().secs().max(now).max(1.0);

    // Every entry, in the order they happen — a lane is where an author put an
    // entry, and the picture is of the run rather than of the authoring.
    let mut entries: Vec<(&str, f32, &Entry)> = Vec::new();
    for lane in &lane_names {
        let Some(track) = plan.get(lane) else {
            continue;
        };
        for key in track.keys() {
            entries.push((lane, key.at().secs(), key.value()));
        }
    }
    entries.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(b.0)));

    let room_rows =
        usize::try_from(room.h.saturating_sub(STRIP_PAD * 2) / STRIP_ROW_H).unwrap_or(0);
    let shown = entries.len().min(room_rows);
    if shown == 0 {
        return None;
    }
    let band_h = STRIP_PAD * 2 + u32::try_from(shown).unwrap_or(1) * STRIP_ROW_H;
    let band = Rect::new(0, 0, room.w, band_h);
    let track_x = STRIP_GUTTER;
    let track_w = room.w.saturating_sub(STRIP_GUTTER + STRIP_PAD).max(1);
    // A moment as an x inside the track. Clamped rather than wrapped: a plan
    // whose duration is the span puts its last entry exactly at the right edge.
    let at_x = |secs: f32| -> u32 { track_x + seat_offset((secs / span).clamp(0.0, 1.0), track_w) };

    let rows = entries
        .iter()
        .take(shown)
        .enumerate()
        .map(|(n, (lane, at, entry))| {
            let until = entry.timeout.map(|wait| at + wait.secs());
            let kind = if entry.act == Act::Check {
                // ★ Matched on the pair the checkpoint was RAISED with, which is
                // the pair `advance` pushes: a plan may hold two checks about
                // one card at different moments, and matching on the card alone
                // would give the second one the first one's verdict.
                match checks
                    .iter()
                    .find(|c| c.target == entry.target && (c.at.secs() - at).abs() < f32::EPSILON)
                {
                    Some(check) => MarkKind::raised(check.met),
                    None => MarkKind::Pending,
                }
            } else {
                MarkKind::Act
            };
            let top = STRIP_PAD + u32::try_from(n).unwrap_or(0) * STRIP_ROW_H;
            let left = at_x(*at);
            let right = until.map_or(left, at_x);
            StripRow {
                tag: format!("lab.scenario.{lane}.{at}"),
                lane: (*lane).to_owned(),
                at: *at,
                act: entry.act.as_wire_name(),
                target: entry.target.clone(),
                until,
                kind,
                row: Rect::new(0, top, room.w, STRIP_ROW_H),
                words: Rect::new(
                    STRIP_PAD,
                    top + 2,
                    STRIP_GUTTER.saturating_sub(STRIP_PAD * 2).max(1),
                    STRIP_ROW_H.saturating_sub(4),
                ),
                bar: Rect::new(
                    left,
                    top + 4,
                    right.saturating_sub(left).max(STRIP_BAR_MIN_W),
                    STRIP_ROW_H.saturating_sub(8),
                ),
            }
        })
        .collect();

    Some(Strip {
        band,
        rows,
        hidden: entries.len() - shown,
        lanes: lane_names.len(),
        playhead: Rect::new(
            at_x(now),
            STRIP_PAD,
            1,
            band_h.saturating_sub(STRIP_PAD * 2),
        ),
        span,
    })
}

/// A ratio of a width, as whole pixels.
///
/// Its own function so the one cast lives at one site: a ratio the caller has
/// already clamped into `0.0..=1.0` times a `u32` width cannot leave `u32`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a clamped ratio of a window-bounded width is a window-bounded offset"
)]
fn seat_offset(ratio: f32, width: u32) -> u32 {
    (ratio * width as f32) as u32
}
