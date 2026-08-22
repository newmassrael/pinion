//! R1774 §5.32 §5.45 — **the gate that asks whether a sweep reaches both sides
//! of every clamp a screen has.**
//!
//! Its own module for the reason [`screen_ink`](super::screen_ink),
//! [`speech`](super::speech) and [`surface`](super::surface) have theirs: this
//! is a HARNESS vocabulary rather than a widget stand-in. The rule — every
//! observable must be seen clamped at least once and unclamped at least once —
//! is the framework's; *which* observables a screen has, and how to read one
//! off a painted frame, is the screen's and stays there.
//!
//! # What a clamp is, and why a gate has to ask about it
//!
//! Painters in this tree guard their own output: a row that will not fit is not
//! drawn, a column too narrow to say anything is dropped, a pane below its
//! minimum is omitted entirely. Every such guard has two sides, and a sweep
//! that only ever visits one of them is worth nothing on that guard:
//!
//! * never CLAMPED — the guard is there and nothing exercises it, so deleting
//!   it would change nothing and no gate would say so;
//! * never FULL — the screen is always truncated in that state, so *always
//!   truncated* would pass every check above it.
//!
//! # What it caught
//!
//! Screen C of this tree's analysis tool asked the question first (R1669) and
//! found, on the first run, a real defect — a card whose size could not hold
//! its own specification — plus one structural exemption worth stating. Two
//! rounds later the debt that recorded it observed that the other two screens
//! carry guards of the same shape and **nobody asks them the question**, and
//! that the axis is not *is there a guard* but *does the sweep reach its true
//! branch*. This is that question, lifted so a screen supplies only its own
//! observations.

use std::collections::BTreeMap;

/// What a sweep saw of one observable: its full form, and its clamped one.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sides {
    full: bool,
    clamped: bool,
}

impl Sides {
    /// Whether the sweep ever saw this observable unclamped.
    #[must_use]
    pub const fn full(self) -> bool {
        self.full
    }

    /// Whether the sweep ever saw it clamped.
    #[must_use]
    pub const fn clamped(self) -> bool {
        self.clamped
    }
}

/// Every clamp outcome a sweep observed, keyed by the observable's own name.
///
/// The name is the screen's word for the thing that can be cut — `"decode:
/// rows"`, `"filter: stat tiles"` — because the failure a reader has to act on
/// is *which* guard is unexercised, and a count cannot say that.
#[derive(Default, Debug)]
pub struct ClampCensus {
    seen: BTreeMap<String, Sides>,
}

impl ClampCensus {
    /// A census with nothing observed yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold in one observation: this observable, on this frame, was clamped or
    /// was not.
    ///
    /// Called once per observable per swept state. A screen that reads an
    /// observable only in the states where it is clamped will fail the `full`
    /// half below, which is the point — the reading has to happen on every
    /// state the sweep visits, not only the interesting ones.
    pub fn note(&mut self, what: impl Into<String>, clamped: bool) {
        let side = self.seen.entry(what.into()).or_default();
        if clamped {
            side.clamped = true;
        } else {
            side.full = true;
        }
    }

    /// What was observed, by name.
    #[must_use]
    pub fn seen(&self) -> &BTreeMap<String, Sides> {
        &self.seen
    }

    /// How many distinct observables the sweep produced.
    #[must_use]
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    /// Whether the sweep produced nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    /// The observables whose clamped side no swept state reached.
    #[must_use]
    pub fn never_clamped(&self) -> Vec<&str> {
        self.side(|side| !side.clamped)
    }

    /// The observables the sweep never saw unclamped.
    #[must_use]
    pub fn never_full(&self) -> Vec<&str> {
        self.side(|side| !side.full)
    }

    fn side(&self, wanted: impl Fn(&Sides) -> bool) -> Vec<&str> {
        self.seen
            .iter()
            .filter(|(_, side)| wanted(side))
            .map(|(what, _)| what.as_str())
            .collect()
    }

    /// ★★★★★ The whole rule, as one call: the population is not empty, every
    /// observable was reached on its clamped side, and every one was seen
    /// unclamped.
    ///
    /// `floor` is the smallest population the caller's sweep is expected to
    /// produce. It is required rather than defaulted because **a derivation
    /// that quietly yields nothing is the failure this check exists to
    /// prevent**: a census that observed zero observables would pass both of
    /// the other two clauses vacuously, and read as *this screen has no clamp
    /// nobody exercises*.
    ///
    /// # Panics
    ///
    /// With the observables named, on any of the three counts.
    pub fn assert_both_sides_reached(&self, screen: &str, floor: usize) {
        assert!(
            self.len() >= floor,
            "{screen}: the sweep observed only {} clamp outcome(s), fewer than \
             the {floor} this screen has: {:?}. A derivation that quietly \
             yields nothing would pass every clause below it",
            self.len(),
            self.seen.keys().collect::<Vec<_>>(),
        );
        let unreached = self.never_clamped();
        assert!(
            unreached.is_empty(),
            "{screen}: no swept state reaches the clamped side of {unreached:?} \
             — the guard is there and nothing exercises it, so deleting it \
             would change nothing and no gate would say so",
        );
        let never_full = self.never_full();
        assert!(
            never_full.is_empty(),
            "{screen}: the sweep never sees {never_full:?} unclamped, so \
             'always truncated' would pass every check above it",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ClampCensus;

    fn both(what: &str) -> ClampCensus {
        let mut census = ClampCensus::new();
        census.note(what, true);
        census.note(what, false);
        census
    }

    #[test]
    fn an_observable_seen_both_ways_satisfies_the_rule() {
        let census = both("rows");
        assert_eq!(census.len(), 1);
        assert!(census.never_clamped().is_empty());
        assert!(census.never_full().is_empty());
        census.assert_both_sides_reached("fixture", 1);
    }

    #[test]
    fn an_observable_never_clamped_is_named() {
        let mut census = ClampCensus::new();
        census.note("rows", false);
        assert_eq!(census.never_clamped(), vec!["rows"]);
        assert!(census.never_full().is_empty());
    }

    #[test]
    fn an_observable_never_full_is_named() {
        let mut census = ClampCensus::new();
        census.note("rows", true);
        assert_eq!(census.never_full(), vec!["rows"]);
        assert!(census.never_clamped().is_empty());
    }

    /// ★★★★★ The clause the other two cannot cover: an EMPTY census satisfies
    /// both of them vacuously, and would read as a screen whose every clamp is
    /// exercised. The floor is what makes the population a claim.
    #[test]
    fn an_empty_census_is_refused_by_the_floor_and_by_nothing_else() {
        let census = ClampCensus::new();
        assert!(census.is_empty());
        assert!(census.never_clamped().is_empty(), "vacuously");
        assert!(census.never_full().is_empty(), "vacuously");
        let refused = std::panic::catch_unwind(|| census.assert_both_sides_reached("fixture", 1));
        assert!(refused.is_err(), "an empty population must be refused");
    }

    /// Repeated observations of the same side do not manufacture the other.
    #[test]
    fn seeing_one_side_many_times_is_still_one_side() {
        let mut census = ClampCensus::new();
        for _ in 0..5 {
            census.note("rows", true);
        }
        assert_eq!(census.never_full(), vec!["rows"]);
    }
}
