//! ★★★★★ R1866 §5.2 — **what changed between two runs of one thing.**
//!
//! # What forced this, measured
//!
//! The analysis-tool census asks for *scenario diff and regression: two runs of
//! one graph compared on order and latency distribution*, and its row said the
//! substrate was already here — `Run::trace` is comparable and the chart crate
//! has the distributions, so the comparison is the application's. Re-measured
//! at R1866 before a line was written, **the middle was missing**: a run's
//! `Step` carries which node ran and what it produced and *nothing about when*,
//! and the lab's own value type is a locator string. Both named halves were
//! real and there was nothing to join them with — R1851's shape, one piece
//! further in, because here the join needs a fact neither half had.
//!
//! # Order and latency are one axis read at two scales
//!
//! The row names two comparisons and they look like two mechanisms. They are
//! not: an *order* is a sequence of marks at logical times (step 0, step 1) and
//! a *latency profile* is the same sequence at physical ones (0.0s, 8.2s).
//! Writing two comparators would be writing one rule twice — this crate's
//! standing finding — so [`Timeline`] carries its [`Scale`] and one
//! [`Regression::between`] answers both.
//!
//! ⚠ **And a timeline cannot be compared with one at another scale.** A shift
//! of `2` means two steps or two seconds and the difference is not recoverable
//! from the number, so mixing them produces a distribution whose samples are
//! not commensurable — a plausible answer to a question nobody asked.
//! [`Mismatch::DifferentScales`] refuses it, which is this project's rule about
//! designing a type so the wrong thing cannot be *said* rather than checking
//! for it afterwards.
//!
//! # What it hands the chart crate
//!
//! [`Regression::shifts`] is a `Vec<f64>` and
//! [`Distribution::from_samples`](../../pinion_chart/struct.Distribution.html)
//! takes one. That is the whole of the join the census row was asking for, and
//! it is deliberately a plain vector rather than a chart type: this crate does
//! not depend on the chart crate, and a regression is a fact about two runs
//! whether or not anybody draws it.
//!
//! # Floor
//!
//! Probed at 6.11.1 while writing this: that toolkit can *store* a sequence of
//! timed values and *animate* between them, and the count of members that
//! answer **what two such sequences do differently** — gained, lost, or moved,
//! with the amount — is **zero**. The nearest thing is a model that reports
//! rows inserted and removed between two states, which has no time axis at all,
//! so it cannot say a mark arrived later; and its own report is a stream of
//! change notifications rather than a value a caller can hold, compare again,
//! or hand to a summary. A regression here is a VALUE: it can be stored, sent
//! over the wire, and asked the same questions twice.

use std::collections::BTreeMap;
use std::fmt;

/// The units a [`Timeline`]'s marks are placed on.
///
/// Carried by the timeline rather than assumed by the caller, because a shift
/// of `2` is two steps or two seconds and the number does not say which. Two
/// timelines at different scales are refused rather than compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// Logical time: a run's own step ordinals. What `Run::trace` answers in.
    Steps,
    /// Physical time, in seconds. What a scenario clock answers in.
    Seconds,
}

impl Scale {
    /// The word this scale is published under.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Scale::Steps => "steps",
            Scale::Seconds => "seconds",
        }
    }

    /// The unit a shift on this scale is counted in, for a sentence a person
    /// reads.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        match self {
            Scale::Steps => "step(s)",
            Scale::Seconds => "s",
        }
    }
}

impl fmt::Display for Scale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire())
    }
}

/// One thing that happened, and when.
///
/// The name is what makes two runs comparable at all: a mark is matched with
/// the mark of the same name in the other run, so the name has to be the
/// identity of the *thing*, not of the occurrence. A node that ran twice is two
/// marks with the same name, and [`Regression::between`] pairs them in order —
/// see [`Timeline::place`] for why that is the only honest pairing.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    /// What happened.
    pub name: String,
    /// When, on the timeline's own scale.
    pub at: f64,
}

impl Mark {
    /// A mark, or `None` when `at` is not a position — a NaN or an infinity
    /// cannot be ordered against another, so a timeline holding one could not
    /// be walked.
    #[must_use]
    pub fn new(name: impl Into<String>, at: f64) -> Option<Self> {
        at.is_finite().then(|| Self {
            name: name.into(),
            at,
        })
    }
}

/// A run, as the sequence of marks it produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    scale: Scale,
    marks: Vec<Mark>,
}

impl Timeline {
    /// An empty timeline at `scale`.
    #[must_use]
    pub const fn new(scale: Scale) -> Self {
        Self {
            scale,
            marks: Vec::new(),
        }
    }

    /// Add a mark, keeping the timeline in time order.
    ///
    /// ⚠ **Stable for equal times, and that is load-bearing.** Two marks at one
    /// moment are two things that happened together, and which of them a reader
    /// is told about first is the order they were placed in — a sort that
    /// reordered them would make a run's own report depend on the sort, and
    /// then a rerun that changed nothing could report a reordering.
    ///
    /// ⚠ Takes `&mut self` rather than consuming, because a timeline is
    /// something a run *accumulates into* — the first consumer holds one in a
    /// `RefCell` and appends per crossing, and a consuming builder would have
    /// made every append a take-and-put-back.
    pub fn place(&mut self, mark: Mark) {
        let at = self.marks.partition_point(|m| m.at <= mark.at);
        self.marks.insert(at, mark);
    }

    /// Build a timeline from marks already in the order they happened.
    ///
    /// `None` when any position is not finite.
    #[must_use]
    pub fn of(scale: Scale, marks: impl IntoIterator<Item = (String, f64)>) -> Option<Self> {
        let mut out = Self::new(scale);
        for (name, at) in marks {
            out.place(Mark::new(name, at)?);
        }
        Some(out)
    }

    /// A run's own step ordinals as a timeline: the *order* half of the
    /// comparison.
    ///
    /// The convenience that keeps a caller from inventing its own ordinals and
    /// getting the scale wrong. `names` is walked once and each item is placed
    /// at its index.
    #[must_use]
    pub fn in_order(names: impl IntoIterator<Item = String>) -> Self {
        let mut out = Self::new(Scale::Steps);
        for (index, name) in names.into_iter().enumerate() {
            // An index is finite by construction, so `Mark::new` cannot refuse
            // and the timeline stays total.
            #[allow(
                clippy::cast_precision_loss,
                reason = "a step ordinal is far below 2^53"
            )]
            let at = index as f64;
            out.marks.push(Mark { name, at });
        }
        out
    }

    /// What scale these marks are on.
    #[must_use]
    pub const fn scale(&self) -> Scale {
        self.scale
    }

    /// The marks, in time order.
    #[must_use]
    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// How many marks there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.marks.len()
    }

    /// Whether nothing happened.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }
}

/// Why two timelines could not be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// One is in steps and the other in seconds.
    ///
    /// Refused rather than converted: there is no conversion. A shift computed
    /// across them would be a number with no unit, and a distribution of such
    /// numbers is a picture of nothing.
    DifferentScales {
        /// The earlier run's scale.
        was: Scale,
        /// The later run's scale.
        now: Scale,
    },
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mismatch::DifferentScales { was, now } => write!(
                f,
                "one run is measured in {was} and the other in {now}, and a \
                 shift between them would have no unit"
            ),
        }
    }
}

impl std::error::Error for Mismatch {}

/// One mark that is in both runs, at different times.
#[derive(Debug, Clone, PartialEq)]
pub struct Shift {
    /// What moved.
    pub name: String,
    /// Which occurrence of that name, counted from zero — a node that ran three
    /// times has three, and they shift independently.
    pub occurrence: usize,
    /// When it happened in the earlier run.
    pub was: f64,
    /// When it happened in the later run.
    pub now: f64,
}

impl Shift {
    /// How far it moved, signed: positive is later.
    #[must_use]
    pub fn by(&self) -> f64 {
        self.now - self.was
    }
}

/// What changed between two runs of one thing.
///
/// Four disjoint groups covering every mark of both runs, which is the property
/// that makes the totals checkable: `gained + shifted + held` is the later
/// run's mark count and `lost + shifted + held` the earlier one's.
#[derive(Debug, Clone, PartialEq)]
pub struct Regression {
    scale: Scale,
    gained: Vec<Mark>,
    lost: Vec<Mark>,
    shifted: Vec<Shift>,
    held: usize,
}

impl Regression {
    /// Compare two runs.
    ///
    /// # How marks are paired
    ///
    /// By name, and within a name by **occurrence order**: the first `router`
    /// of the earlier run is compared with the first `router` of the later one,
    /// the second with the second, and a run with more of them contributes the
    /// surplus to `gained` (or `lost`). That is the only pairing that does not
    /// need a rule nobody can check — pairing by nearest time would make the
    /// answer depend on how far apart things are, so a run where everything
    /// slipped past its neighbour would report no shift at all and a pile of
    /// gains and losses.
    ///
    /// # Errors
    ///
    /// [`Mismatch::DifferentScales`] when the two runs are not measured in the
    /// same units.
    pub fn between(was: &Timeline, now: &Timeline) -> Result<Self, Mismatch> {
        if was.scale != now.scale {
            return Err(Mismatch::DifferentScales {
                was: was.scale,
                now: now.scale,
            });
        }
        let mut before: BTreeMap<&str, Vec<&Mark>> = BTreeMap::new();
        for mark in &was.marks {
            before.entry(&mark.name).or_default().push(mark);
        }
        let mut after: BTreeMap<&str, Vec<&Mark>> = BTreeMap::new();
        for mark in &now.marks {
            after.entry(&mark.name).or_default().push(mark);
        }

        let mut gained = Vec::new();
        let mut lost = Vec::new();
        let mut shifted = Vec::new();
        let mut held = 0usize;

        let names: std::collections::BTreeSet<&str> =
            before.keys().chain(after.keys()).copied().collect();
        for name in names {
            let olds = before.get(name).map(Vec::as_slice).unwrap_or_default();
            let news = after.get(name).map(Vec::as_slice).unwrap_or_default();
            for (occurrence, pair) in olds.iter().zip(news.iter()).enumerate() {
                let (earlier, later) = pair;
                // Exact equality is the right relation and not a hazard: both
                // sides come from the same producer, so "the same moment" is
                // the same number rather than two roundings of one.
                if (earlier.at - later.at).abs() == 0.0 {
                    held += 1;
                } else {
                    shifted.push(Shift {
                        name: name.to_owned(),
                        occurrence,
                        was: earlier.at,
                        now: later.at,
                    });
                }
            }
            if olds.len() > news.len() {
                lost.extend(olds[news.len()..].iter().map(|m| (*m).clone()));
            } else {
                gained.extend(news[olds.len()..].iter().map(|m| (*m).clone()));
            }
        }
        shifted.sort_by(|a, b| {
            a.was
                .partial_cmp(&b.was)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(Self {
            scale: was.scale,
            gained,
            lost,
            shifted,
            held,
        })
    }

    /// The scale both runs were measured on.
    #[must_use]
    pub const fn scale(&self) -> Scale {
        self.scale
    }

    /// Marks the later run has and the earlier one did not.
    #[must_use]
    pub fn gained(&self) -> &[Mark] {
        &self.gained
    }

    /// Marks the earlier run had and the later one does not.
    #[must_use]
    pub fn lost(&self) -> &[Mark] {
        &self.lost
    }

    /// Marks both runs have, at different times.
    #[must_use]
    pub fn shifted(&self) -> &[Shift] {
        &self.shifted
    }

    /// How many marks are in both runs at the same time.
    #[must_use]
    pub const fn held(&self) -> usize {
        self.held
    }

    /// Whether the two runs are the same run.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.gained.is_empty() && self.lost.is_empty() && self.shifted.is_empty()
    }

    /// Every shift, as the samples a distribution summarises.
    ///
    /// The join the census row was asking for: this is what
    /// `Distribution::from_samples` takes. Signed, so a summary of it shows
    /// which way a run moved rather than only how far — a set of shifts that
    /// are all late and a set half early and half late have the same magnitudes
    /// and are opposite findings.
    ///
    /// ⚠ Empty when nothing shifted, and a distribution of no samples is
    /// refused by the chart crate rather than drawn as a flat line. That
    /// refusal is the correct one and this does not paper over it: *nothing
    /// moved* is a verdict ([`is_clean`](Self::is_clean)), not a picture.
    #[must_use]
    pub fn shifts(&self) -> Vec<f64> {
        self.shifted.iter().map(Shift::by).collect()
    }

    /// The largest move, in the direction it went.
    ///
    /// `None` when nothing shifted. Chosen by magnitude and reported signed,
    /// because the question a person asks first is *what moved most* and the
    /// answer they need includes which way.
    #[must_use]
    pub fn worst(&self) -> Option<&Shift> {
        self.shifted
            .iter()
            .max_by(|a, b| {
                a.by()
                    .abs()
                    .partial_cmp(&b.by().abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .filter(|s| s.by() != 0.0)
    }

    /// One sentence a person reads.
    #[must_use]
    pub fn sentence(&self) -> String {
        if self.is_clean() {
            return format!("{} mark(s) held, nothing changed", self.held);
        }
        let unit = self.scale.unit();
        let worst = self.worst().map_or_else(String::new, |s| {
            format!(", worst {} by {:+.3}{unit}", s.name, s.by())
        });
        format!(
            "{} gained, {} lost, {} shifted, {} held{worst}",
            self.gained.len(),
            self.lost.len(),
            self.shifted.len(),
            self.held,
        )
    }

    /// This regression as the value a running application publishes.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "scale": self.scale.wire(),
            "gained": self.gained.iter().map(|m| serde_json::json!({"name": m.name, "at": m.at})).collect::<Vec<_>>(),
            "lost": self.lost.iter().map(|m| serde_json::json!({"name": m.name, "at": m.at})).collect::<Vec<_>>(),
            "shifted": self.shifted.iter().map(|s| serde_json::json!({
                "name": s.name,
                "occurrence": s.occurrence,
                "was": s.was,
                "now": s.now,
                "by": s.by(),
            })).collect::<Vec<_>>(),
            "held": self.held,
            "clean": self.is_clean(),
            "sentence": self.sentence(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Mark, Mismatch, Regression, Scale, Timeline};

    /// Two sets of amounts, equal to the last place a shift can differ in.
    ///
    /// ⚠ Not `==`, and not because these numbers are uncertain — every one of
    /// them is a difference of two exact fixture positions. It is because a
    /// gate that compares floats exactly is a gate that starts failing the day
    /// a caller feeds it something measured, and the tests are the first reader
    /// of the contract.
    fn close(got: &[f64], want: &[f64]) -> bool {
        got.len() == want.len() && got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9)
    }

    fn line(scale: Scale, marks: &[(&str, f64)]) -> Timeline {
        Timeline::of(scale, marks.iter().map(|(n, a)| ((*n).to_owned(), *a)))
            .expect("the fixture's positions are finite")
    }

    /// ★ The identity: a run compared with itself changed nothing.
    #[test]
    fn r1866_a_run_compared_with_itself_is_clean() {
        let run = line(Scale::Seconds, &[("dial", 0.0), ("accept", 1.5)]);
        let diff = Regression::between(&run, &run).expect("one scale");
        assert!(diff.is_clean());
        assert_eq!(diff.held(), 2);
        assert!(diff.shifts().is_empty());
        assert_eq!(diff.sentence(), "2 mark(s) held, nothing changed");
    }

    /// ★★★★★ The four groups cover every mark of both runs, which is what makes
    /// the totals a check rather than a summary.
    #[test]
    fn r1866_every_mark_of_both_runs_lands_in_exactly_one_group() {
        let was = line(Scale::Seconds, &[("a", 0.0), ("b", 1.0), ("c", 2.0)]);
        let now = line(Scale::Seconds, &[("a", 0.0), ("b", 4.0), ("d", 5.0)]);
        let diff = Regression::between(&was, &now).expect("one scale");
        assert_eq!(
            diff.lost().len() + diff.shifted().len() + diff.held(),
            was.len(),
            "the earlier run's marks are not all accounted for",
        );
        assert_eq!(
            diff.gained().len() + diff.shifted().len() + diff.held(),
            now.len(),
            "the later run's marks are not all accounted for",
        );
        assert_eq!(diff.lost()[0].name, "c");
        assert_eq!(diff.gained()[0].name, "d");
        assert!(close(&diff.shifts(), &[3.0]), "{:?}", diff.shifts());
    }

    /// ★★★★★ Two scales cannot be compared, and the refusal is the type's
    /// rather than a caller's discipline.
    #[test]
    fn r1866_two_scales_cannot_be_compared() {
        let steps = line(Scale::Steps, &[("a", 0.0)]);
        let seconds = line(Scale::Seconds, &[("a", 0.0)]);
        assert_eq!(
            Regression::between(&steps, &seconds),
            Err(Mismatch::DifferentScales {
                was: Scale::Steps,
                now: Scale::Seconds,
            }),
        );
        // And the sentence says which is which, so a reader can fix it.
        let said = Mismatch::DifferentScales {
            was: Scale::Steps,
            now: Scale::Seconds,
        }
        .to_string();
        assert!(said.contains("steps") && said.contains("seconds"), "{said}");
    }

    /// ★★★★★ Occurrences are paired in order, not by nearest time.
    ///
    /// The rule that keeps the answer from depending on how far things moved: a
    /// run where every occurrence slipped past its neighbour would otherwise
    /// report no shift and a pile of gains and losses.
    #[test]
    fn r1866_repeated_names_are_paired_in_occurrence_order() {
        let was = line(Scale::Steps, &[("loop", 0.0), ("loop", 1.0), ("loop", 2.0)]);
        let now = line(Scale::Steps, &[("loop", 0.0), ("loop", 5.0)]);
        let diff = Regression::between(&was, &now).expect("one scale");
        assert_eq!(diff.held(), 1, "the first occurrence did not move");
        assert_eq!(diff.shifted().len(), 1);
        assert_eq!(diff.shifted()[0].occurrence, 1);
        assert!(close(&[diff.shifted()[0].by()], &[4.0]));
        assert_eq!(diff.lost().len(), 1, "the third occurrence is gone");
    }

    /// ★ Order IS the same mechanism at another scale — `in_order` places each
    /// name at its ordinal, and a reordering shows up as shifts.
    #[test]
    fn r1866_an_order_is_a_timeline_in_steps() {
        let was = Timeline::in_order(["a", "b", "c"].map(str::to_owned));
        let now = Timeline::in_order(["a", "c", "b"].map(str::to_owned));
        assert_eq!(was.scale(), Scale::Steps);
        let diff = Regression::between(&was, &now).expect("one scale");
        assert_eq!(diff.held(), 1, "only `a` kept its place");
        let mut moved: Vec<(&str, f64)> = diff
            .shifted()
            .iter()
            .map(|s| (s.name.as_str(), s.by()))
            .collect();
        moved.sort_by(|a, b| a.0.cmp(b.0));
        assert_eq!(
            moved.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            vec!["b", "c"],
        );
        assert!(close(
            &moved.iter().map(|(_, by)| *by).collect::<Vec<_>>(),
            &[1.0, -1.0],
        ));
        // The unit reaches the sentence, so a reader is not left guessing.
        assert!(diff.sentence().contains("step(s)"), "{}", diff.sentence());
    }

    /// ★★ A mark at a position that is not a position is refused at
    /// construction, so a timeline cannot hold one.
    #[test]
    fn r1866_a_mark_needs_a_position() {
        assert!(Mark::new("a", f64::NAN).is_none());
        assert!(Mark::new("a", f64::INFINITY).is_none());
        assert!(Timeline::of(Scale::Seconds, [("a".to_owned(), f64::NAN)]).is_none());
    }

    /// ★★ Equal times keep placement order, so a rerun that changed nothing
    /// cannot report a reordering.
    #[test]
    fn r1866_marks_at_one_moment_keep_the_order_they_were_placed_in() {
        let mut line = Timeline::new(Scale::Seconds);
        line.place(Mark::new("first", 1.0).expect("finite"));
        line.place(Mark::new("second", 1.0).expect("finite"));
        line.place(Mark::new("third", 1.0).expect("finite"));
        let names: Vec<&str> = line.marks().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["first", "second", "third"]);
    }

    /// ★★★★★ What this type SAYS, in every state it can be in.
    ///
    /// Demanded by `every_speaking_type_in_this_crate_is_driven_by_the_speech_
    /// gate`, which caught `Regression` the moment it grew a `sentence` — an
    /// arm nobody drives is an arm that can say anything. The three states are
    /// the ones a reader meets: nothing changed, something moved, and something
    /// came or went without anything moving.
    #[test]
    fn r1866_every_verdict_this_type_reaches_says_something_different() {
        use crate::test_fixtures::speech::assert_speaks;

        let clean = line(Scale::Seconds, &[("a", 0.0)]);
        let moved = line(Scale::Seconds, &[("a", 2.0)]);
        let other = line(Scale::Seconds, &[("b", 0.0)]);
        let said = [
            (
                "clean",
                Regression::between(&clean, &clean)
                    .expect("one scale")
                    .sentence(),
            ),
            (
                "shifted",
                Regression::between(&clean, &moved)
                    .expect("one scale")
                    .sentence(),
            ),
            (
                "gained and lost",
                Regression::between(&clean, &other)
                    .expect("one scale")
                    .sentence(),
            ),
        ];
        assert_speaks("Regression", 3, &said, &[]);
        // ★ And they are three DIFFERENT sentences: a verdict that read the
        // same in two states would be a verdict nobody can act on.
        let mut distinct: Vec<&str> = said.iter().map(|(_, s)| s.as_str()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 3, "two states read the same: {said:?}");
        // ★★ The one that moved names the amount AND its unit; the one that
        // only gained and lost has no worst move to name.
        assert!(said[1].1.contains("+2.000s"), "{}", said[1].1);
        assert!(!said[2].1.contains("worst"), "{}", said[2].1);
    }

    /// ★★ The worst move is by MAGNITUDE and reported SIGNED.
    #[test]
    fn r1866_the_worst_move_is_the_largest_either_way() {
        let was = line(Scale::Seconds, &[("a", 10.0), ("b", 10.0)]);
        let now = line(Scale::Seconds, &[("a", 12.0), ("b", 4.0)]);
        let diff = Regression::between(&was, &now).expect("one scale");
        let worst = diff.worst().expect("two things moved");
        assert_eq!(worst.name, "b");
        assert!(close(&[worst.by()], &[-6.0]));
        assert!(diff.sentence().contains("-6.000s"), "{}", diff.sentence());
    }
}
