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
    #[must_use]
    pub const fn verdict(&self) -> &'static str {
        match self.met {
            Some(true) => "met",
            Some(false) => "failed",
            None => "waiting",
        }
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
    if from <= 0.0 && by > 0.0 {
        state.checks.borrow_mut().clear();
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
        done.push(serde_json::json!({
            "lane": lane,
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
        "lanes": lanes,
        "conflicts": conflicts(&plan),
        "checks": checks_wire(state),
    })
}

/// A track's refusal, as a sentence on the invoke channel.
fn refusal(why: Misplaced) -> InvokeError {
    InvokeError::rejected(why.to_string())
}
