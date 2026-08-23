//! R1793 §5.29 — a **histogram that chooses its own bins and says which rule
//! chose them**. R1797 gave it the three things a *latency* distribution needs
//! and none of the four original rules could express.
//!
//! ## The gap, and how it was found
//!
//! The analysis-tool census records `dashboard.t1.6` — *a latency distribution:
//! a histogram* — as a gap, with the reason *"NO BINNING IN THE CRATE:
//! `Density` (KDE), box and violin exist, and a histogram is assembled by the
//! application out of `BarChart` bins it computes itself"*. Re-measured before a
//! line was written, that reason is **true and precise**: [`crate::Bar`] has
//! carried a numeric bin extent since R1395 ([`crate::Bar::with_bin`]) and
//! [`crate::BarChart::select_x_range`] cross-filters on it, so a bar can *hold*
//! an interval — but nothing computes the intervals. `hello-histogram-brush`
//! states `N_BINS` by hand and buckets its own samples.
//!
//! ## ★★★★★ The asymmetry that decides the shape
//!
//! This crate already knows how to choose a smoothing width from the data. It
//! is [`crate::density::Bandwidth`], and the argument on it is the argument for
//! this module, word for word:
//!
//! > *It is the one parameter that decides whether a violin shows two modes or
//! > one, so it is declared rather than buried: a chart that picked silently
//! > would be making the reader's conclusion for them.*
//!
//! Bin width decides exactly that, and more brutally — a histogram of the same
//! samples is unimodal at six bins and bimodal at twenty. So the same question
//! was answered for the continuous estimator and left unanswered for the
//! discrete one, in one crate. This module closes that, in the same vocabulary:
//! a named rule, a `Fixed` escape, a `name()` for the wire, and a refusal type
//! whose variants say what about the samples made the estimate impossible.
//!
//! ## What it publishes that a chart normally does not
//!
//! [`Binned::basis`] — the numbers the rule computed from: how many samples,
//! their spread, the interquartile range, and (R1797) **which quantile
//! definition produced that range**. A reader looking at a surprising
//! histogram has two hypotheses, *the data is like that* and *the binning did
//! that*, and no published histogram anywhere in this tree could tell them
//! apart. Now the chart says which rule ran and what it read.
//!
//! # ★★★★★ R1797 — what a latency distribution forced
//!
//! The reference's dashboard draws round-trip time as eight buckets:
//! `<1`, `1-2`, `2-4`, `4-8`, `8-16`, `16-32`, `32-64`, `>64` milliseconds.
//! Re-measured against R1793's module before this round wrote anything, **that
//! card could not be drawn**, for three separate reasons:
//!
//! * The ladder is **geometric**. Every rule here divided the span into equal
//!   parts, and `width()` was a single scalar — so a doubling ladder was not
//!   merely absent, it was unrepresentable. That is the wrong default for the
//!   shape latency actually has: Freedman–Diaconis on a heavy tail spends its
//!   bins on the empty right-hand side and collapses the body into one bar,
//!   which is asserted below rather than claimed.
//! * Two of the eight buckets are **unbounded**. Every edge was finite by
//!   construction, and there was no way to say *"and everything above 64"*
//!   without inventing a top that the data does not have.
//! * Nothing said which bins are the **tail**. The reference hard-codes the
//!   index (`i >= 6`); a tail that is *derived* from the distribution is the
//!   claim a reader can check, and it moves when the data does.
//!
//! [`BinRule::Ratio`], [`BinEnds`] and [`Binned::tail_from`] are those three, and
//! [`Binned::over`] is the constructor for boundaries a caller states rather
//! than derives — because a bucket ladder chosen for an SLA is a *decision*,
//! not an estimate, and a rule that re-derived it would be overruling it.
//!
//! ## ⚠ The floor, measured by running it
//!
//! R1793 recorded *"no floor was measured for this item"* because the charting
//! module was not in this install. R1797 **built it from source** and probed it
//! through its own runtime type information — enumerating every property and
//! method the relevant classes publish, so that "no such capability" is a
//! measurement rather than a reading. What that probe reports:
//!
//! * The bar surface publishes 33 + 30 names and **not one names a bin, a
//!   bucket, a sample, a rule or a basis**. Bars are counts the caller already
//!   computed; the toolkit stores them verbatim.
//! * A bar set carries **no per-bar numeric interval at all**, so no bar there
//!   has an extent that could be unbounded — the bucket labels are strings on a
//!   category axis and nothing connects them to a number.
//! * Handed `+inf` as an axis maximum, the numeric axis **refuses it**
//!   (`"Attempting to set invalid range"`) and keeps the previous value. An
//!   unbounded end is not expressible there even as a display.
//! * There **is** a logarithmic *axis*, at a settable base — and it names
//!   nothing to do with binning. A log axis is a display concern; log bins are
//!   a derivation, and having the first does not give you the second.
//! * The box surface answers **no quantile of any kind** (see
//!   [`crate::quantile`]).
//!
//! So on this item the floor is not a lower bar, it is an absence — and the
//! comparison this module is actually held to stays the internal one: the
//! crate's own KDE already declared its width rule, and the histogram did not.

use crate::bar::Bar;
use crate::distribution::QuantileMethod;
use crate::quantile::Quantiles;

/// How the bin width is chosen.
///
/// Mirrors [`crate::density::Bandwidth`] deliberately — same shape, same
/// `Fixed` escape, same `name()` — because they answer the same question about
/// the same samples and a reader who has met one should not have to learn a
/// second vocabulary for the other.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BinRule {
    /// Freedman–Diaconis, `2 · IQR · n^(-1/3)`. The default, for the reason
    /// [`crate::density::Bandwidth`] defaults to Silverman: it reads the
    /// INTERQUARTILE range rather than the standard deviation, so one far
    /// outlier widens the bins by a little instead of stretching them across
    /// the whole range and collapsing the shape into a single bar.
    #[default]
    FreedmanDiaconis,
    /// Scott's rule, `3.49 · σ · n^(-1/3)`. Narrower than Freedman–Diaconis on
    /// well-behaved data and much wider on anything heavy-tailed — the same
    /// trade Scott makes against Silverman one module over.
    Scott,
    /// Sturges' rule, `ceil(log2(n)) + 1` bins. Classic, and it assumes the
    /// samples are roughly normal: on skewed or large samples it produces too
    /// few bins and smooths real structure away. Offered because it is what a
    /// reader may be comparing against, and named so a chart can say it used it.
    Sturges,
    /// A bin count the caller chose. Must be at least one.
    Fixed(u32),
    /// ★ R1797 — a **geometric** ladder: each bin is `base` times as wide as
    /// the one below it, anchored on the powers of `base` that bracket the
    /// samples.
    ///
    /// The rule for anything measured as a *duration* or a *magnitude*, where
    /// the interesting structure is spread over decades and an equal-width
    /// ladder spends every bin on the widest decade. `base` must be greater
    /// than one, and every sample must be positive — the logarithm of zero is
    /// not a bin boundary, and pretending otherwise is how a latency chart ends
    /// up with a first bucket nobody can read.
    Ratio(f64),
    /// ★ R1797 — the boundaries came from the caller, through
    /// [`Binned::over`]. Not usable with [`Binned::of`], which exists to
    /// *derive* boundaries; passing it there is [`BinError::StatedNeedsEdges`].
    ///
    /// It is a variant rather than an absence so that [`Binned::rule`] stays
    /// total and a wire reading a histogram always gets an answer to *how were
    /// these bins chosen* — including the answer "they were not chosen here".
    Stated,
}

impl BinRule {
    /// Stable name, for a wire that has to say which rule ran.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FreedmanDiaconis => "freedman-diaconis",
            Self::Scott => "scott",
            Self::Sturges => "sturges",
            Self::Fixed(_) => "fixed",
            Self::Ratio(_) => "ratio",
            Self::Stated => "stated",
        }
    }
}

/// ★ R1797 — what happens to a sample outside the outermost boundary.
///
/// `BinEnds` rather than `Ends`, and not for tidiness:
/// `pinion_core::widgets::roving::Ends` already exists and means something
/// entirely different — whether a focus ring wraps at its last member. Both
/// names read perfectly in isolation and the collision only surfaced when one
/// screen imported both, which is the moment a generic name in a shared tree
/// stops being free. The `Bin` prefix is this module's own ([`BinRule`],
/// [`BinError`]), so the type is discoverable beside the ones it belongs with.
///
/// Only meaningful for [`Binned::over`], where the boundaries are the caller's
/// and the data is under no obligation to fit inside them. [`Binned::of`]
/// derives boundaries that cover the samples by construction, so it is always
/// [`BinEnds::Closed`] and nothing ever falls out.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BinEnds {
    /// The outer boundaries are hard. A sample outside them is **not binned**,
    /// and [`Binned::outside`] reports how many fell each way.
    ///
    /// The default, because silently widening a stated ladder would overrule
    /// the decision that stated it — and because a drop that is *reported* is
    /// the same shape as [`crate::widgets`]-adjacent losses elsewhere in this
    /// tree: the number is available, so a screen can show it instead of a
    /// reader wondering where the samples went.
    ///
    /// [`crate::widgets`]: crate
    #[default]
    Closed,
    /// An unbounded bin below the first boundary and another above the last.
    /// Nothing is dropped, and those two bins have **no finite width** —
    /// [`Binned::extent`] answers `None` for the side that runs off.
    ///
    /// This is the reference latency card's `<1` and `>64`, and it is what the
    /// floor's numeric axis measurably cannot hold.
    Open,
}

/// Why a histogram could not be binned.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinError {
    /// Fewer than two finite samples: there is no spread to divide.
    ///
    /// [`Binned::over`] needs only **one**, and refuses at zero: it was handed
    /// the boundaries, so it has nothing to derive and nothing to derive it
    /// from. The two constructors differ here for that reason and no other.
    TooFewSamples,
    /// Every sample is the same value. A point mass is not a distribution, and
    /// drawing one bar of an arbitrary width would be inventing the shape.
    ///
    /// [`Binned::over`] does not raise this either — identical samples fall
    /// perfectly well into a bucket somebody else chose.
    NoSpread,
    /// A [`BinRule::Fixed`] of zero bins.
    BadBinCount,
    /// ★ R1797 — a [`BinRule::Ratio`] whose base is not greater than one, or is
    /// not finite. A ratio of one never advances and a ratio below one runs
    /// backwards; neither is a ladder.
    BadRatio(f64),
    /// ★ R1797 — a [`BinRule::Ratio`] over samples that are not all positive.
    /// Carries the offending sample. There is no logarithm of zero to anchor a
    /// geometric ladder on, and clamping to some small positive value would
    /// invent a first bucket rather than report the problem.
    NonPositiveSample(f64),
    /// ★ R1797 — [`Binned::over`] was given fewer than two boundaries, or ones
    /// that are not strictly ascending and finite. Two boundaries is one bin,
    /// which is the least that can be drawn.
    BadEdges,
    /// ★ R1797 — [`BinRule::Stated`] reached [`Binned::of`]. It names
    /// boundaries the caller supplies; [`Binned::over`] is the constructor that
    /// takes them.
    StatedNeedsEdges,
}

impl core::fmt::Display for BinError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooFewSamples => f.write_str("a histogram needs at least two finite samples"),
            Self::NoSpread => {
                f.write_str("every sample is the same value, so there is no spread to divide")
            }
            Self::BadBinCount => f.write_str("a fixed bin count must be at least one"),
            Self::BadRatio(base) => {
                write!(
                    f,
                    "a geometric bin ladder needs a base above one, got {base}"
                )
            }
            Self::NonPositiveSample(sample) => write!(
                f,
                "a geometric bin ladder has no logarithm of {sample}; every sample must be positive"
            ),
            Self::BadEdges => {
                f.write_str("stated bin boundaries must be at least two, finite and ascending")
            }
            Self::StatedNeedsEdges => {
                f.write_str("`stated` names boundaries the caller supplies; use `Binned::over`")
            }
        }
    }
}

impl std::error::Error for BinError {}

/// What the rule read off the samples to choose the bins.
///
/// ★★★★★ The half a histogram normally does not publish. A surprising shape has
/// two explanations — *the data is like that* and *the binning did that* — and
/// without these numbers a reader cannot tell them apart. `iqr` is here even
/// when the rule that ran did not use it, because the point is to let a reader
/// ask what the OTHER rule would have done.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Basis {
    /// How many finite samples were read.
    ///
    /// ⚠ For [`Binned::over`] under [`BinEnds::Closed`] this counts every finite
    /// sample, **including the ones that fell outside the boundaries and were
    /// therefore not binned**. The two numbers are different questions and
    /// [`Binned::outside`] answers the other one; folding them together would
    /// make a spread statistic disagree with the samples it was computed from.
    pub n: usize,
    /// The smallest sample.
    pub min: f64,
    /// The largest sample.
    pub max: f64,
    /// The population standard deviation.
    pub sigma: f64,
    /// The interquartile range, under [`Basis::quantile_method`].
    pub iqr: f64,
    /// ★ R1797 — which quantile definition produced `iqr`.
    ///
    /// It is here because the answer was **not the obvious one**. Before this
    /// round the interquartile range read by this rule, the one read by
    /// [`crate::density::Bandwidth::Silverman`], and the one drawn by a box
    /// plot beside them were computed by three separate private copies, of
    /// which the first two were Hyndman & Fan type 7 with no way to say so
    /// while the box defaults to Tukey's hinges. The numbers differ. Nothing
    /// said which was which; now the histogram does.
    pub quantile_method: QuantileMethod,
}

/// Samples divided into bins, with the rule that divided them.
#[derive(Debug, Clone, PartialEq)]
pub struct Binned {
    /// The **finite** boundaries, ascending. Under [`BinEnds::Open`] there is one
    /// more bin than these describe at each end.
    edges: Vec<f64>,
    counts: Vec<u32>,
    rule: BinRule,
    basis: Basis,
    ends: BinEnds,
    below: u32,
    above: u32,
}

impl Binned {
    /// Bin `samples` by `rule`, deriving boundaries that cover them.
    ///
    /// Non-finite samples are dropped before anything else, the way
    /// [`crate::Distribution::from_samples`] and [`crate::density::Density`]
    /// drop them: a `NaN` is a gap in the record, not a value in the middle of
    /// the axis.
    ///
    /// The last bin is CLOSED at the top (`[lo, hi]` where every other bin is
    /// `[lo, hi)`), so the largest sample lands in a bin instead of falling off
    /// the end. Every histogram has to make this choice and most make it
    /// silently; it is asserted in this module's tests.
    ///
    /// # Errors
    ///
    /// [`BinError`] when the samples cannot support a histogram: fewer than two
    /// finite ones, no spread at all, a fixed count of zero, a
    /// [`BinRule::Ratio`] that is not a ladder or meets a non-positive sample,
    /// or [`BinRule::Stated`], which belongs to [`Binned::over`].
    pub fn of(samples: &[f64], rule: BinRule) -> Result<Self, BinError> {
        let finite = finite_ascending(samples);
        if finite.len() < 2 {
            return Err(BinError::TooFewSamples);
        }
        let n = finite.len();
        let (min, max) = (finite[0], finite[n - 1]);
        // `>=` rather than `==`, and not to satisfy a lint: the slice is sorted,
        // so `min >= max` is exactly "no spread" and says so without an equality
        // on floats. A margin here would be WRONG — it would refuse a genuinely
        // narrow spread that bins perfectly well.
        if min >= max {
            return Err(BinError::NoSpread);
        }
        let basis = basis_of(&finite);
        let edges = match rule {
            BinRule::Stated => return Err(BinError::StatedNeedsEdges),
            BinRule::Ratio(base) => ratio_edges(base, &finite)?,
            other => {
                let count = bin_count(other, &basis)?;
                let width = (max - min) / f64::from(count);
                (0..=count).map(|k| min + width * f64::from(k)).collect()
            }
        };
        Ok(Self::tally(edges, &finite, rule, basis, BinEnds::Closed))
    }

    /// ★ R1797 — bin `samples` into boundaries the **caller states**, with
    /// `ends` deciding what happens to samples outside them.
    ///
    /// `edges` are the finite interior boundaries: `n` of them describe `n - 1`
    /// bins under [`BinEnds::Closed`], and `n + 1` under [`BinEnds::Open`], the two
    /// extra ones running to negative and positive infinity.
    ///
    /// This is the constructor for a ladder that is a **decision** rather than
    /// an estimate — a service-level bucket set, a doubling ladder a team reads
    /// every day, an axis shared with another tool. A rule that re-derived
    /// those boundaries from the current sample would be overruling the reason
    /// they exist.
    ///
    /// # Errors
    ///
    /// [`BinError::BadEdges`] when there are fewer than two boundaries or they
    /// are not finite and strictly ascending, and [`BinError::TooFewSamples`]
    /// when no sample is finite. Unlike [`Binned::of`] this accepts a **single**
    /// sample and accepts samples with no spread: it was handed the boundaries,
    /// so it has nothing to derive.
    pub fn over(samples: &[f64], edges: &[f64], ends: BinEnds) -> Result<Self, BinError> {
        if edges.len() < 2
            || !edges.iter().all(|e| e.is_finite())
            || edges.windows(2).any(|w| w[0] >= w[1])
            || u32::try_from(edges.len()).is_err()
        {
            return Err(BinError::BadEdges);
        }
        let finite = finite_ascending(samples);
        if finite.is_empty() {
            return Err(BinError::TooFewSamples);
        }
        let basis = basis_of(&finite);
        Ok(Self::tally(
            edges.to_vec(),
            &finite,
            BinRule::Stated,
            basis,
            ends,
        ))
    }

    /// Count `sorted` into `edges` under `ends`. The one place a sample is
    /// assigned to a bin, so the two constructors cannot disagree about it.
    fn tally(edges: Vec<f64>, sorted: &[f64], rule: BinRule, basis: Basis, ends: BinEnds) -> Self {
        let open = ends == BinEnds::Open;
        let interior = edges.len() - 1;
        let bins = if open { interior + 2 } else { interior };
        let mut counts = vec![0_u32; bins];
        let (mut below, mut above) = (0_u32, 0_u32);
        for sample in sorted {
            match slot(&edges, *sample, open) {
                Slot::Bin(k) => counts[k] += 1,
                Slot::Below => below += 1,
                Slot::Above => above += 1,
            }
        }
        Self {
            edges,
            counts,
            rule,
            basis,
            ends,
            below,
            above,
        }
    }

    /// The **finite** bin boundaries, ascending.
    ///
    /// Under [`BinEnds::Closed`] there are `bins() + 1` of them. Under
    /// [`BinEnds::Open`] there are `bins() - 1`, because the outermost bins have
    /// no finite boundary on their outer side — [`Binned::extent`] is the
    /// accessor that can say so.
    #[must_use]
    pub fn edges(&self) -> &[f64] {
        &self.edges
    }

    /// How many samples fell in each bin.
    #[must_use]
    pub fn counts(&self) -> &[u32] {
        &self.counts
    }

    /// How many bins there are.
    #[must_use]
    pub fn bins(&self) -> usize {
        self.counts.len()
    }

    /// Whether the outermost bins are unbounded.
    #[must_use]
    pub const fn ends(&self) -> BinEnds {
        self.ends
    }

    /// ★ R1797 — bin `k`'s interval, `None` on a side that runs to infinity.
    ///
    /// Returns `None` for an index past the last bin, which is what makes this
    /// safe to walk beside a painter's own loop.
    #[must_use]
    pub fn extent(&self, k: usize) -> Option<(Option<f64>, Option<f64>)> {
        if k >= self.bins() {
            return None;
        }
        if self.ends == BinEnds::Open {
            if k == 0 {
                return Some((None, Some(self.edges[0])));
            }
            if k == self.bins() - 1 {
                return Some((Some(self.edges[self.edges.len() - 1]), None));
            }
            return Some((Some(self.edges[k - 1]), Some(self.edges[k])));
        }
        Some((Some(self.edges[k]), Some(self.edges[k + 1])))
    }

    /// ★ R1797 — bin `k` in the notation a bucket ladder is read in:
    /// `<1`, `1-2`, `>64`.
    ///
    /// Distinct from the label [`Binned::bars`] puts on a bar, which is the
    /// lower edge alone because that is what a reader matches against an axis
    /// tick. Both conventions are legitimate and they are for different
    /// drawings: a bar under a numeric axis wants the tick, a bucket column
    /// with no axis at all wants the interval.
    #[must_use]
    pub fn label(&self, k: usize) -> String {
        match self.extent(k) {
            None => String::new(),
            Some((None, Some(hi))) => format!("<{}", compact(hi)),
            Some((Some(lo), None)) => format!(">{}", compact(lo)),
            Some((Some(lo), Some(hi))) => format!("{}-{}", compact(lo), compact(hi)),
            // Both sides unbounded is one bin covering everything, which
            // `over` cannot produce (it needs two ascending edges) and `of`
            // never produces. Answered rather than unwrapped.
            Some((None, None)) => "all".to_owned(),
        }
    }

    /// ★ R1797 — how many finite samples fell below the first boundary and
    /// above the last **without being binned**.
    ///
    /// Always `(0, 0)` under [`BinEnds::Open`], where those samples have bins to
    /// fall into, and always `(0, 0)` for [`Binned::of`], whose boundaries
    /// cover the samples by construction. Non-zero only when a caller stated
    /// hard boundaries and the data did not respect them — which is exactly
    /// when a chart that stayed silent would be lying about its total.
    #[must_use]
    pub const fn outside(&self) -> (u32, u32) {
        (self.below, self.above)
    }

    /// The uniform bin width, or `None` when the bins are not all the same
    /// finite width.
    ///
    /// ⚠ R1797 changed this from `f64`. A geometric ladder and an unbounded
    /// outer bin both make "the width" a question with no answer, and the
    /// previous signature could only answer it wrongly. Derived from the stored
    /// edges rather than from the span and the count, so the width and the
    /// edges cannot disagree — the same reason `select_x_range` reads the
    /// extent a bar carries instead of computing one.
    #[must_use]
    pub fn uniform_width(&self) -> Option<f64> {
        if self.ends == BinEnds::Open {
            return None;
        }
        let first = self.edges[1] - self.edges[0];
        let uniform = self
            .edges
            .windows(2)
            .all(|w| ((w[1] - w[0]) - first).abs() <= first.abs() * 1e-9);
        uniform.then_some(first)
    }

    /// Which rule chose the bins.
    #[must_use]
    pub const fn rule(&self) -> BinRule {
        self.rule
    }

    /// What that rule read off the samples.
    #[must_use]
    pub const fn basis(&self) -> Basis {
        self.basis
    }

    /// ★ R1797 — the contiguous run of top bins lying entirely at or above
    /// `cut`.
    ///
    /// The distribution's **tail**, as a range a painter can emphasise. The
    /// reference's own latency card hard-codes the index of its first amber
    /// bar; a cut derived from the samples — a p95, a service-level threshold —
    /// is a claim a reader can check, and it moves when the data does.
    ///
    /// A bin qualifies when its **lower** bound is at or above `cut`, so a bin
    /// straddling the cut stays out of the tail: half a bin's samples are below
    /// the threshold, and colouring it as though none were would overstate the
    /// case. An unbounded lower bound never qualifies. The result is empty when
    /// no bin does, and `bins()..bins()` is the empty range returned.
    #[must_use]
    pub fn tail_from(&self, cut: f64) -> core::ops::Range<usize> {
        let mut start = self.bins();
        for k in (0..self.bins()).rev() {
            match self.extent(k) {
                Some((Some(lo), _)) if lo >= cut => start = k,
                _ => break,
            }
        }
        start..self.bins()
    }

    /// The bars a [`crate::BarChart`] draws, each carrying its own numeric
    /// extent.
    ///
    /// ★ Through [`crate::Bar::with_bin`], which is what makes the result
    /// cross-filterable by [`crate::BarChart::select_x_range`] the moment it is
    /// drawn — the half of the histogram that R1395 already built, and the half
    /// this module exists to feed. `label` is the bin's lower edge, which is
    /// what a reader matches against an axis tick; an **unbounded** bin has no
    /// edge to match, so it takes [`Binned::label`]'s interval form instead.
    ///
    /// ★ R1797 — an unbounded side is handed to `with_bin` as an infinity
    /// rather than as a clamp, and it works: `select_x_range` mutes a bar whose
    /// bin does not overlap the window, and `-inf >= hi` and `+inf <= lo` are
    /// both false, so an open bin is selected exactly when the window reaches
    /// into it. The floor cannot do this — its numeric axis refuses `+inf`
    /// outright, measured this round.
    #[must_use]
    pub fn bars(&self) -> Vec<Bar> {
        (0..self.bins())
            .map(|k| {
                let (lo, hi) = self.extent(k).unwrap_or((None, None));
                // ★ EITHER side unbounded takes the interval form. The first
                // draft matched on the lower bound alone and the top open bin
                // -- which has a finite lower edge and no upper one -- came out
                // labelled `64.000`, a tick that promises a bin ending
                // somewhere. Its own test caught it.
                let label = match (lo, hi) {
                    (Some(edge), Some(_)) => format!("{edge:.3}"),
                    _ => self.label(k),
                };
                let count = f64::from(self.counts[k]);
                Bar::new(label, count)
                    .with_bin(lo.unwrap_or(f64::NEG_INFINITY), hi.unwrap_or(f64::INFINITY))
            })
            .collect()
    }
}

/// Which bin a sample belongs in.
enum Slot {
    Bin(usize),
    Below,
    Above,
}

/// Assign `sample` to a bin of `edges`, with the top interior bin CLOSED at its
/// upper edge.
///
/// A binary search rather than a division, because R1797's geometric ladder has
/// no single width to divide by — and because the search is correct for the
/// uniform case too, so there is one assignment rule rather than two.
fn slot(edges: &[f64], sample: f64, open: bool) -> Slot {
    let last = edges.len() - 1;
    if sample < edges[0] {
        return if open { Slot::Bin(0) } else { Slot::Below };
    }
    if sample > edges[last] {
        return if open {
            Slot::Bin(last + 1)
        } else {
            Slot::Above
        };
    }
    // `partition_point` counts the edges at or below the sample; one less is
    // the interior bin, and the top bin absorbs a sample sitting exactly on the
    // final edge.
    let interior = edges.partition_point(|e| *e <= sample).saturating_sub(1);
    let interior = interior.min(last - 1);
    Slot::Bin(interior + usize::from(open))
}

/// Finite samples, ascending. The filter and the sort both constructors need.
fn finite_ascending(samples: &[f64]) -> Vec<f64> {
    let mut finite: Vec<f64> = samples.iter().copied().filter(|s| s.is_finite()).collect();
    // `total_cmp` rather than `partial_cmp().unwrap()`: the filter above
    // already removed every NaN, so the two agree here — but a sort that
    // cannot panic is the one to write down, because the invariant it relies
    // on lives in a different statement.
    finite.sort_by(f64::total_cmp);
    finite
}

/// What the rules get to read, from a non-empty ascending slice.
fn basis_of(sorted: &[f64]) -> Basis {
    // ★ R1797 — through the crate's one quantile type rather than a private
    // copy. `Linear` preserves the number R1793's private helper produced
    // (Hyndman & Fan type 7) and is now the answer to *which definition*, which
    // nothing could give before.
    const METHOD: QuantileMethod = QuantileMethod::Linear;
    let quantiles = Quantiles::from_sorted(sorted, METHOD);
    Basis {
        n: sorted.len(),
        min: sorted[0],
        max: sorted[sorted.len() - 1],
        sigma: sigma_of(sorted),
        iqr: quantiles.iqr(),
        quantile_method: METHOD,
    }
}

/// The geometric ladder bracketing an ascending, all-positive slice.
fn ratio_edges(base: f64, sorted: &[f64]) -> Result<Vec<f64>, BinError> {
    if !base.is_finite() || base <= 1.0 {
        return Err(BinError::BadRatio(base));
    }
    if let Some(bad) = sorted.iter().copied().find(|s| *s <= 0.0) {
        return Err(BinError::NonPositiveSample(bad));
    }
    let (min, max) = (sorted[0], sorted[sorted.len() - 1]);
    let log = base.ln();
    let lo_step = (min.ln() / log).floor();
    let hi_step = (max.ln() / log).ceil();
    // A sample landing exactly on a power of the base makes floor and ceil
    // agree, which would be zero bins. One step up is the bin that contains it.
    let hi_step = if hi_step <= lo_step {
        lo_step + 1.0
    } else {
        hi_step
    };
    let wanted = hi_step - lo_step;
    // `f64::from` rather than `as`: a `u32` converts to `f64` losslessly, so
    // there is no precision to lose and nothing to suppress. The first draft
    // carried an `expect` here and clippy reported it as UNFULFILLED, which is
    // the lint doing its second job — a suppression nobody needs is a claim
    // about the code that is not true.
    let bound = f64::from(MAX_BINS);
    if wanted <= bound {
        // The anchored ladder: edges are exact powers of the base, which is
        // what makes `1, 2, 4, 8` come out as those numbers rather than as
        // whatever the sample minimum happens to be.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "checked against the bound immediately above"
        )]
        let steps = (wanted as u32).max(1);
        return Ok((0..=steps)
            .map(|k| (log * (lo_step + f64::from(k))).exp())
            .collect());
    }
    // ★★★★★ Past the bound, the ladder is the geometric division of the
    // OBSERVED span rather than the anchored power ladder. Its own test caught
    // why this matters: the first draft clamped the step COUNT and kept the
    // anchor, so a base barely above one over sixty decades produced 4,096
    // steps covering a millionth of the data and the largest sample fell off
    // the end -- silently, because `of` derives closed ends and a closed end
    // drops. A derived histogram that does not cover its own samples is the one
    // thing this constructor promises not to be, so coverage wins over anchoring
    // and the equal-width rules make the same trade (`from_width` clamps the
    // count and recomputes the width from the span).
    let ratio = (max / min).ln() / bound;
    let mut edges: Vec<f64> = (0..=MAX_BINS)
        .map(|k| (min.ln() + ratio * f64::from(k)).exp())
        .collect();
    // ★★★★★ The endpoints are ASSIGNED, not computed. `exp(ln(min))` is not
    // `min` — it is within an ulp or two, and over sixty decades that lands on
    // the wrong side about half the time. Its own test caught the consequence
    // and it was worse than the defect it replaced: with a computed first edge
    // a hair above `min`, the smallest sample falls BELOW the ladder, and with
    // a computed last edge a hair below `max` the largest falls above it, so a
    // derived histogram dropped BOTH of its extremes and counted zero of two.
    // A round trip through a logarithm is not an identity, and the two values
    // that have to be exact are the two we already hold.
    edges[0] = min;
    edges[MAX_BINS as usize] = max;
    Ok(edges)
}

/// How many bins `rule` asks for, given what was read off the samples.
///
/// Separated so the arithmetic each rule is named for is readable next to its
/// name, and so a test can drive a rule without building a histogram.
fn bin_count(rule: BinRule, basis: &Basis) -> Result<u32, BinError> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "n is a sample count; the rules are cube and log of it"
    )]
    let n = basis.n as f64;
    let span = basis.max - basis.min;
    let from_width = |width: f64| -> u32 {
        if width <= 0.0 || !width.is_finite() {
            // A degenerate spread measure (every sample inside one quartile,
            // say) leaves the rule with nothing to say. Falling back to
            // Sturges is what a reader gets rather than a refusal, because the
            // samples DO have a spread — it is the rule that ran out.
            return sturges(n);
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the ratio is positive and finite here, and clamped below"
        )]
        let raw = (span / width).ceil() as u32;
        raw.clamp(1, MAX_BINS)
    };
    Ok(match rule {
        BinRule::FreedmanDiaconis => from_width(2.0 * basis.iqr * n.powf(-1.0 / 3.0)),
        BinRule::Scott => from_width(3.49 * basis.sigma * n.powf(-1.0 / 3.0)),
        BinRule::Sturges => sturges(n),
        BinRule::Fixed(0) => return Err(BinError::BadBinCount),
        BinRule::Fixed(k) => k.min(MAX_BINS),
        // Both are handled before `bin_count` is reached: `Ratio` builds its
        // own edges and `Stated` is refused. Answered rather than unreachable,
        // so a variant added later fails visibly instead of panicking.
        BinRule::Ratio(base) => return Err(BinError::BadRatio(base)),
        BinRule::Stated => return Err(BinError::StatedNeedsEdges),
    })
}

/// The most bins a rule may ask for.
///
/// A bound rather than a hope: `n^(-1/3)` against a tiny spread measure can ask
/// for millions of bins, and a chart that allocated them would hang on data
/// rather than say anything about it. 4096 is far past any histogram a person
/// reads and small enough to allocate without thought.
pub const MAX_BINS: u32 = 4096;

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "log2 of a sample count, clamped to at least one"
)]
fn sturges(n: f64) -> u32 {
    (n.log2().ceil() as u32 + 1).clamp(1, MAX_BINS)
}

/// The population standard deviation of an ascending slice.
fn sigma_of(sorted: &[f64]) -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "a sample count as a divisor")]
    let n = sorted.len() as f64;
    let mean = sorted.iter().sum::<f64>() / n;
    (sorted.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n).sqrt()
}

/// A boundary as a bucket label reads it: no trailing zeros, and no exponent
/// for the magnitudes a bucket ladder is written in.
fn compact(value: f64) -> String {
    let rounded = format!("{value:.3}");
    let trimmed = rounded.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Twelve samples with one far outlier — the case the two rules disagree on
    /// and the reason the default is the robust one.
    fn skewed() -> Vec<f64> {
        vec![1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9, 2.0, 40.0]
    }

    /// The reference latency card's ladder: doubling from one millisecond to
    /// sixty-four, with everything outside falling off both ends.
    const LADDER: [f64; 7] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0];

    #[test]
    fn r1793_a_histogram_says_which_rule_chose_its_bins() {
        let binned = Binned::of(&skewed(), BinRule::Scott).expect("spread");
        assert_eq!(binned.rule().name(), "scott");
        assert_eq!(binned.basis().n, 12);
        assert!(binned.basis().iqr > 0.0, "and what it could have read");
    }

    #[test]
    fn r1797_the_basis_says_which_quantile_definition_made_its_iqr() {
        // ★★★★★ The half that was missing. Two rules in this crate read an
        // interquartile range and a third drawing showed one, and all three
        // computed it separately -- so the number was reproducible only by
        // reading the source. Now it carries its definition.
        let binned = Binned::of(&skewed(), BinRule::FreedmanDiaconis).expect("spread");
        assert_eq!(binned.basis().quantile_method.name(), "linear");
        let stated = crate::Quantiles::of(&skewed(), QuantileMethod::Linear)
            .expect("finite")
            .iqr();
        assert!(
            (binned.basis().iqr - stated).abs() < 1e-12,
            "and the number is that definition's: {} vs {stated}",
            binned.basis().iqr
        );
    }

    #[test]
    fn r1793_the_robust_rule_and_the_sigma_rule_disagree_on_an_outlier() {
        // ★★★★★ The whole reason a rule is declared rather than picked
        // silently. One far sample stretches sigma, so Scott's bins get wide
        // and the eleven clustered samples collapse into one bar; the
        // interquartile range barely moves, so Freedman-Diaconis keeps them
        // apart. Same data, two shapes, and a reader can only tell which they
        // are looking at because the chart says.
        let samples = skewed();
        let scott = Binned::of(&samples, BinRule::Scott).expect("spread");
        let fd = Binned::of(&samples, BinRule::FreedmanDiaconis).expect("spread");
        assert!(
            fd.bins() > scott.bins(),
            "the robust rule keeps more structure: fd {} vs scott {}",
            fd.bins(),
            scott.bins()
        );
        assert_eq!(
            BinRule::default(),
            BinRule::FreedmanDiaconis,
            "and it is the default, for the reason `Bandwidth` defaults to Silverman"
        );
    }

    #[test]
    fn r1793_every_sample_lands_in_exactly_one_bin() {
        for rule in [
            BinRule::FreedmanDiaconis,
            BinRule::Scott,
            BinRule::Sturges,
            BinRule::Fixed(7),
            BinRule::Ratio(2.0),
        ] {
            let binned = Binned::of(&skewed(), rule).expect("spread");
            let total: u32 = binned.counts().iter().sum();
            assert_eq!(total as usize, 12, "{}: {total} of 12 counted", rule.name());
            assert_eq!(binned.edges().len(), binned.bins() + 1);
            assert_eq!(
                binned.outside(),
                (0, 0),
                "{}: derived bins cover",
                rule.name()
            );
        }
    }

    #[test]
    fn r1793_the_largest_sample_is_in_a_bin_and_not_past_the_end() {
        // Every histogram has to close its top bin and most do it silently.
        let binned = Binned::of(&[0.0, 1.0, 2.0, 3.0, 4.0], BinRule::Fixed(4)).expect("spread");
        assert_eq!(*binned.counts().last().expect("four bins"), 2, "3 and 4");
        assert!(
            (binned.edges()[binned.bins()] - 4.0).abs() < 1e-9,
            "the top edge IS the largest sample: {:?}",
            binned.edges()
        );
    }

    #[test]
    fn r1793_the_refusals_name_what_about_the_samples_was_wrong() {
        assert_eq!(
            Binned::of(&[], BinRule::default()),
            Err(BinError::TooFewSamples)
        );
        assert_eq!(
            Binned::of(&[f64::NAN, 1.0], BinRule::default()),
            Err(BinError::TooFewSamples),
            "a NaN is a gap in the record, not a value"
        );
        assert_eq!(
            Binned::of(&[2.0, 2.0, 2.0], BinRule::default()),
            Err(BinError::NoSpread)
        );
        assert_eq!(
            Binned::of(&[1.0, 2.0], BinRule::Fixed(0)),
            Err(BinError::BadBinCount)
        );
    }

    #[test]
    fn r1797_the_new_refusals_name_what_a_ladder_could_not_do() {
        assert_eq!(
            Binned::of(&[1.0, 2.0], BinRule::Ratio(1.0)),
            Err(BinError::BadRatio(1.0)),
            "a ratio of one never advances"
        );
        assert_eq!(
            Binned::of(&[1.0, 2.0], BinRule::Ratio(0.5)),
            Err(BinError::BadRatio(0.5)),
            "and one below runs backwards"
        );
        assert_eq!(
            Binned::of(&[0.0, 1.0, 2.0], BinRule::Ratio(2.0)),
            Err(BinError::NonPositiveSample(0.0)),
            "★ there is no logarithm of zero to anchor a ladder on, and the \
             refusal carries the sample rather than clamping it into a first \
             bucket nobody could read"
        );
        assert_eq!(
            Binned::of(&[1.0, 2.0], BinRule::Stated),
            Err(BinError::StatedNeedsEdges),
            "`stated` names boundaries `of` was never given"
        );
        assert_eq!(
            Binned::over(&[1.0], &[1.0], BinEnds::Closed),
            Err(BinError::BadEdges),
            "one boundary is not a bin"
        );
        assert_eq!(
            Binned::over(&[1.0], &[2.0, 1.0], BinEnds::Closed),
            Err(BinError::BadEdges),
            "and boundaries must ascend"
        );
        assert_eq!(
            Binned::over(&[1.0], &[1.0, f64::INFINITY], BinEnds::Closed),
            Err(BinError::BadEdges),
            "★ an unbounded end is `BinEnds::Open`, not an infinite EDGE -- the \
             two are different statements and only one of them is checkable"
        );
    }

    #[test]
    fn r1797_a_geometric_ladder_keeps_a_heavy_tail_that_equal_width_flattens() {
        // ★★★★★ The measurement that decided `Ratio` exists. Latency is
        // heavy-tailed, and the four original rules all divide the SPAN into
        // equal parts -- so the tail sample dictates the width and the body of
        // the distribution collapses. Both histograms below are of the same
        // samples; only one of them is readable.
        let mut samples: Vec<f64> = Vec::new();
        for k in 0..200 {
            samples.push(1.0 + f64::from(k) * 0.01); // a dense body near 1-3 ms
        }
        samples.push(900.0); // and one long reply

        let equal = Binned::of(&samples, BinRule::Sturges).expect("spread");
        let ladder = Binned::of(&samples, BinRule::Ratio(2.0)).expect("positive");

        let equal_body = equal.counts()[0];
        assert_eq!(
            equal_body as usize,
            200,
            "★ equal width puts the entire body in ONE bar: {:?}",
            equal.counts()
        );
        let occupied = ladder.counts().iter().filter(|c| **c > 0).count();
        assert!(
            occupied >= 3,
            "★ the geometric ladder keeps it apart across {occupied} bins: {:?}",
            ladder.counts()
        );
        assert_eq!(ladder.rule().name(), "ratio");
        assert!(
            ladder.uniform_width().is_none(),
            "★★ and it honestly has no single width -- the accessor R1793 \
             returned an f64 from could only have answered this wrongly"
        );
        assert!(
            equal.uniform_width().is_some(),
            "while an equal-width rule still answers"
        );
    }

    #[test]
    fn r1797_an_open_ended_ladder_holds_a_tail_with_no_top() {
        // ★★★★★ The reference latency card, and the capability the floor
        // measurably lacks: its numeric axis refuses +inf outright.
        let samples = vec![0.4, 0.9, 1.5, 3.0, 5.0, 12.0, 30.0, 70.0, 400.0];
        let binned = Binned::over(&samples, &LADDER, BinEnds::Open).expect("stated");

        assert_eq!(binned.bins(), 8, "six interior bins plus two open ends");
        assert_eq!(
            binned.counts().iter().sum::<u32>() as usize,
            samples.len(),
            "★ nothing is dropped: an open end is a BIN, not a discard"
        );
        assert_eq!(binned.outside(), (0, 0));
        assert_eq!(binned.label(0), "<1");
        assert_eq!(binned.label(1), "1-2");
        assert_eq!(binned.label(7), ">64");
        assert_eq!(binned.extent(0), Some((None, Some(1.0))));
        assert_eq!(binned.extent(7), Some((Some(64.0), None)));
        assert_eq!(binned.extent(8), None, "and there is no ninth bin");
        assert_eq!(binned.counts()[0], 2, "0.4 and 0.9 are below one");
        assert_eq!(binned.counts()[7], 2, "70 and 400 are above sixty-four");
        assert_eq!(binned.rule().name(), "stated");
    }

    #[test]
    fn r1797_a_closed_ladder_reports_what_it_could_not_bin() {
        // The other half of the choice. Hard boundaries are a legitimate
        // decision -- but a chart that dropped samples silently would be lying
        // about its total, so the drop is a number the caller can show.
        let samples = vec![0.4, 0.9, 1.5, 3.0, 70.0, 400.0];
        let binned = Binned::over(&samples, &LADDER, BinEnds::Closed).expect("stated");
        assert_eq!(binned.bins(), 6, "six bins, no open ends");
        assert_eq!(
            binned.outside(),
            (2, 2),
            "0.4 and 0.9 below; 70 and 400 above"
        );
        assert_eq!(
            binned.counts().iter().sum::<u32>(),
            2,
            "only 1.5 and 3.0 were binned"
        );
        assert_eq!(
            binned.basis().n,
            6,
            "★ and the BASIS still counts every finite sample -- a spread \
             statistic that disagreed with the samples it was computed from \
             would be a third number nobody could reconcile"
        );
    }

    #[test]
    fn r1797_the_tail_is_derived_from_the_distribution_and_moves_with_it() {
        // ★★★★★ The reference hard-codes which bars are amber. A cut derived
        // from the samples is a claim a reader can check -- and this asserts it
        // MOVES, which a hard-coded index cannot.
        //
        // ★★★★★ And it moves in the direction that is easy to get backwards.
        // The first draft of this test asserted that a WORSE distribution has a
        // LONGER tail and failed, correctly: `tail_from` marks the bins lying
        // entirely beyond what is typical FOR THESE SAMPLES, and a percentile
        // is relative to its own distribution. Make everything slow and almost
        // nothing is beyond typical any more. The number that grows when things
        // get worse is the cut itself, and it is on the card as a tile.
        let cut_of = |samples: &[f64]| {
            crate::Quantiles::of(samples, QuantileMethod::Linear)
                .expect("finite")
                .at(0.95)
                .expect("linear defines p95")
        };
        let tail_of = |samples: &[f64]| {
            Binned::over(samples, &LADDER, BinEnds::Open)
                .expect("stated")
                .tail_from(cut_of(samples))
        };

        // A body near 1-2 ms with a tenth of the replies slow -- the shape a
        // healthy capture has. A TENTH, not a twentieth: the first draft put
        // ten slow samples in two hundred, so the 95th percentile landed on the
        // interpolation between the ramp's top and the first slow reply, moved
        // by less than one bucket when the tail tripled, and the assertion
        // below failed on a real distribution that really had degraded. A
        // percentile has to sit INSIDE the part of the distribution the test is
        // moving, or the test is measuring the part it is not.
        let mut healthy: Vec<f64> = (0..180).map(|k| 1.0 + f64::from(k) * 0.005).collect();
        healthy.extend([
            9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0, 20.0, 22.0, 24.0,
            26.0, 28.0, 30.0, 35.0, 40.0, 70.0,
        ]);
        // The same capture after the far end degrades: the slow replies get
        // slower, so the 95th percentile rises.
        let mut degraded = healthy.clone();
        for slow in degraded.iter_mut().filter(|s| **s > 8.0) {
            *slow *= 3.0;
        }

        assert!(
            cut_of(&degraded) > cut_of(&healthy),
            "the cut rises: {} -> {}",
            cut_of(&healthy),
            cut_of(&degraded)
        );
        let (healthy_tail, degraded_tail) = (tail_of(&healthy), tail_of(&degraded));
        assert!(
            degraded_tail.start > healthy_tail.start,
            "★ so FEWER bins are entirely beyond typical: healthy {healthy_tail:?} vs \
             degraded {degraded_tail:?}"
        );
        assert_eq!(healthy_tail.end, 8, "both runs reach the last bin");
        assert_eq!(degraded_tail.end, 8);
    }

    #[test]
    fn r1797_a_cut_inside_the_unbounded_bin_leaves_an_empty_tail() {
        // ★★★★★ The stated limit of the rule, asserted rather than discovered.
        // When the cut lands INSIDE the top open bin, no bin lies entirely
        // above it -- the open bin straddles, and the straddle rule excludes
        // it. So the tail is empty, and that is a statement about the LADDER:
        // it is too coarse to isolate this distribution's tail. A caller that
        // needs an emphasis in that case has the cut and can say so; what this
        // must not do is silently mark a bin most of whose samples are below
        // the threshold.
        let everything_slow: Vec<f64> = (0..100).map(|k| 80.0 + f64::from(k)).collect();
        let binned = Binned::over(&everything_slow, &LADDER, BinEnds::Open).expect("stated");
        let cut = crate::Quantiles::of(&everything_slow, QuantileMethod::Linear)
            .expect("finite")
            .at(0.95)
            .expect("linear defines p95");
        assert!(cut > 64.0, "the cut is inside the unbounded bin: {cut}");
        assert!(
            binned.tail_from(cut).is_empty(),
            "and no bin lies entirely above it"
        );
        assert_eq!(
            binned.counts()[7] as usize,
            everything_slow.len(),
            "while every sample really is in that bin"
        );
    }

    #[test]
    fn r1797_a_bin_straddling_the_cut_stays_out_of_the_tail() {
        // The judgement call, asserted so it cannot drift: half a straddling
        // bin's samples are BELOW the threshold, and colouring it as though
        // none were would overstate the case.
        let samples = vec![1.0, 2.0, 5.0, 9.0, 20.0, 40.0];
        let binned = Binned::over(&samples, &LADDER, BinEnds::Open).expect("stated");
        // A cut of 20 sits inside bin `16-32`, whose lower bound is 16.
        let tail = binned.tail_from(20.0);
        assert_eq!(
            binned.label(tail.start),
            "32-64",
            "the straddled bin is excluded: tail starts at {:?}",
            binned.label(tail.start)
        );
        // On the boundary the bin IS in the tail: its lower bound equals the cut.
        assert_eq!(binned.label(binned.tail_from(16.0).start), "16-32");
        assert!(
            binned.tail_from(f64::INFINITY).is_empty(),
            "and a cut nothing reaches has an empty tail"
        );
    }

    #[test]
    fn r1793_the_bars_carry_their_own_extent_so_a_brush_can_reach_them() {
        // ★ The half R1395 already built. Without `with_bin` a histogram is a
        // bar chart of strings and `select_x_range` has nothing to select on.
        let binned = Binned::of(&skewed(), BinRule::Fixed(4)).expect("spread");
        let bars = binned.bars();
        assert_eq!(bars.len(), 4);
        for (k, bar) in bars.iter().enumerate() {
            let (lo, hi) = bar.bin.expect("every histogram bar has an extent");
            assert!((lo - binned.edges()[k]).abs() < 1e-9);
            assert!((hi - binned.edges()[k + 1]).abs() < 1e-9);
            assert!(hi > lo, "and it is a real interval");
        }
    }

    #[test]
    fn r1797_an_open_bin_is_brushable_through_an_infinite_extent() {
        // ★★★★★ The floor's numeric axis REFUSES +inf, measured this round by
        // running it. Here an infinity is the honest extent and the existing
        // cross-filter reads it without a special case: `select_x_range` mutes
        // a bar whose bin does not overlap the window, and neither
        // `-inf >= hi` nor `+inf <= lo` is ever true.
        use crate::{BarChart, ChartStyle};
        use pinion_core::scene::Rect;

        let samples = vec![0.4, 1.5, 3.0, 70.0, 400.0];
        let binned = Binned::over(&samples, &LADDER, BinEnds::Open).expect("stated");
        let bars = binned.bars();
        assert_eq!(bars[0].bin, Some((f64::NEG_INFINITY, 1.0)));
        assert_eq!(bars[7].bin, Some((64.0, f64::INFINITY)));
        assert_eq!(bars[0].label, "<1", "an unbounded bin has no tick to name");
        assert_eq!(bars[7].label, ">64");
        assert_eq!(bars[1].label, "1.000", "a bounded one still names its tick");

        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        let all = BarChart::new(binned.bars()).build(rect, &style);
        let tail_only = BarChart::new(binned.bars())
            .select_x_range(Some((64.0, 1000.0)))
            .build(rect, &style);
        assert_ne!(
            format!("{all:?}"),
            format!("{tail_only:?}"),
            "★ brushing INTO the unbounded tail changes the drawing"
        );
    }

    #[test]
    fn r1793_a_rule_that_runs_out_falls_back_rather_than_refusing() {
        // A spread measure can be zero while the samples DO have a spread —
        // here every sample inside the quartiles is identical, so the
        // interquartile range is 0 and Freedman-Diaconis has nothing to divide
        // by. Refusing would be wrong: the samples are perfectly binnable.
        let flat_middle = vec![0.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, 10.0];
        let binned = Binned::of(&flat_middle, BinRule::FreedmanDiaconis).expect("binnable");
        assert!(
            binned.basis().iqr.abs() < 1e-9,
            "the rule really did run out"
        );
        assert!(binned.bins() >= 1);
        assert_eq!(
            binned.counts().iter().sum::<u32>() as usize,
            10,
            "and every sample is still counted"
        );
    }

    #[test]
    fn r1797_a_single_sample_bins_into_stated_boundaries_and_not_into_derived_ones() {
        // ★ The two constructors' thresholds differ, and this is the reason:
        // `of` must derive a width from a spread, `over` was handed the
        // boundaries. One sample is a histogram under the second and not under
        // the first.
        assert_eq!(
            Binned::of(&[3.0], BinRule::default()),
            Err(BinError::TooFewSamples)
        );
        let one = Binned::over(&[3.0], &LADDER, BinEnds::Open).expect("boundaries were given");
        assert_eq!(one.counts().iter().sum::<u32>(), 1);
        assert_eq!(one.basis().n, 1);
        // And samples with no spread at all, which `of` refuses as a point mass.
        assert_eq!(
            Binned::of(&[3.0, 3.0, 3.0], BinRule::default()),
            Err(BinError::NoSpread)
        );
        let flat = Binned::over(&[3.0, 3.0, 3.0], &LADDER, BinEnds::Open).expect("stated");
        assert_eq!(flat.counts()[2], 3, "all three in `2-4`");
        assert_eq!(
            Binned::over(&[f64::NAN], &LADDER, BinEnds::Open),
            Err(BinError::TooFewSamples),
            "but nothing finite is still nothing"
        );
    }

    /// ★★★★★ R1793 — **samples to a drawn, brushable chart, through the public
    /// API only.** This is the census proof for `dashboard.t1.6` (*a latency
    /// distribution: a histogram*), and it is written end to end on purpose:
    /// the gap that round closed was never "no bar chart" — it was that the
    /// step from SAMPLES to bins had no home, so every claim about a histogram
    /// in this tree started after the interesting part.
    ///
    /// Measured while writing it: `hello-histogram-brush`, the one screen in
    /// the tree named for a histogram, does not bin anything either — it starts
    /// from counts somebody already aggregated. So this path had no consumer at
    /// all before now.
    #[test]
    fn r1793_latency_samples_become_a_chart_a_brush_can_cross_filter() {
        use crate::{BarChart, ChartStyle};
        use pinion_core::scene::Rect;

        // Round-trip times in milliseconds, with the long tail a latency
        // distribution always has and a box plot's fence would call an outlier.
        let latencies = vec![
            0.9, 1.0, 1.1, 1.1, 1.2, 1.2, 1.3, 1.3, 1.4, 1.5, 1.6, 1.8, 2.1, 2.6, 3.4, 9.7,
        ];
        let binned = Binned::of(&latencies, BinRule::default()).expect("real samples");
        assert_eq!(binned.rule().name(), "freedman-diaconis");
        assert_eq!(
            binned.counts().iter().sum::<u32>() as usize,
            latencies.len(),
            "every measurement is somewhere on the chart"
        );

        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        let all = BarChart::new(binned.bars()).build(rect, &style);
        assert!(
            crate::scene_probe::find(&all, "chart.bar.0").is_some(),
            "the first bin is drawn"
        );

        // ★ And the brush reaches it WITHOUT the caller re-deriving anything:
        // the bars carry the extents `Binned` computed, which is what
        // `select_x_range` filters on.
        let window = (binned.edges()[0], binned.edges()[1]);
        let brushed = BarChart::new(binned.bars())
            .select_x_range(Some(window))
            .build(rect, &style);
        assert_ne!(
            format!("{brushed:?}"),
            format!("{all:?}"),
            "★★ brushing the first bin changes the drawing, so the extents the \
             histogram derived are the ones the cross-filter reads"
        );
    }

    #[test]
    fn r1793_no_rule_can_ask_for_more_bins_than_the_bound() {
        // `n^(-1/3)` against a tiny spread measure asks for millions. A chart
        // that allocated them would hang on the data rather than say anything
        // about it.
        let mut samples: Vec<f64> = (0..200).map(|k| f64::from(k) * 1e-9).collect();
        samples.push(1e6);
        let binned = Binned::of(&samples, BinRule::FreedmanDiaconis).expect("spread");
        assert!(
            binned.bins() <= MAX_BINS as usize,
            "{} bins asked for",
            binned.bins()
        );
    }

    #[test]
    fn r1797_a_ratio_ladder_over_many_decades_is_bounded_too() {
        // The same bound, on the new rule: a base barely above one over a wide
        // span asks for an unbounded number of steps.
        let samples = vec![1e-30, 1e30];
        let binned = Binned::of(&samples, BinRule::Ratio(1.000_001)).expect("positive");
        assert!(
            binned.bins() <= MAX_BINS as usize,
            "{} bins asked for",
            binned.bins()
        );
        assert_eq!(
            binned.counts().iter().sum::<u32>(),
            2,
            "and both are counted"
        );
    }
}
