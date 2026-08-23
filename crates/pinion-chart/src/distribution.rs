//! R1553 — the crate's first datum that is **not a point**.
//!
//! Every value this crate could plot until now resolved to one position: a
//! [`DataPoint`](crate::DataPoint) is `(x, y)`, a [`Bar`](crate::Bar) is a
//! label and a magnitude, a [`Slice`](crate::Slice) is a share. A
//! [`Distribution`] is the summary of *many* samples at one slot — five
//! landmarks with extent along the value axis, plus the samples that fall
//! outside them. It is what a box plot draws (Tukey, *Exploratory Data
//! Analysis*, 1977).
//!
//! R1553 wrote here that a candlestick was "a different reading of the same
//! geometry", and R1567 found that wrong when it built one: a candle's `open`
//! and `close` are **not ordered against each other**, and this type's whole
//! invariant is that its landmarks are. See [`Candle`](crate::Candle) for the
//! argument. What the two share is the *paint* — a band across a slot with
//! landmarks mapped through the value axis — which is where it is now shared.
//!
//! # Why the derivation lives here rather than in the caller
//!
//! The toolkit's box set takes exactly five numbers — `LowerExtreme`, `LowerQuartile`, `Median`, `UpperQuartile`, `UpperExtreme`
//! — and `the toolkit's charting module` computes none of them. Its own box-plot example ships a `findMedian()`
//! helper in the *example*, because the library has no quantile API at all.
//! Three consequences follow, and this module exists to remove all three:
//!
//! * **Which definition produced the box is unrecorded.** There is no single
//!   quartile: [`QuantileMethod`] carries three standard ones that disagree
//!   for small `n`, and a box drawn without naming its method is not
//!   reproducible. Here the method is part of the value
//!   ([`DistributionSource::Samples`]).
//! * **Outliers are inexpressible.** Tukey's box plot stops each whisker at
//!   the most extreme sample still *inside* the `k * IQR` fence and draws
//!   everything beyond it as an individual point — that fence is the
//!   defining half of the form. box set has five slots and no per-outlier
//!   geometry, so a toolkit box plot cannot show one. [`Distribution::outliers`]
//!   is a `Vec`.
//! * **The sample count is lost**, and with it the notch. `McGill`, Tukey &
//!   Larsen (*Variations of Box Plots*, 1978) put a waist at
//!   `median +- 1.58 * IQR / sqrt(n)`, so two boxes whose notches do not
//!   overlap have significantly different medians at roughly 95%. box set
//!   does not carry `n`, so the toolkit could not offer a notch even as a paint
//!   option. [`Distribution::notch`] answers it — and answers `None` for a
//!   summary handed in without samples, because the statistic does not
//!   exist there.
//!
//! The pre-computed path is still supported, because a consumer whose
//! million rows were summarised in the database has five numbers and no
//! samples: [`Distribution::from_summary`] is the toolkit's contract, with the
//! ordering *checked* rather than assumed (`setValue` will accept an
//! upper quartile below the lower one and paint an inverted box in silence).

use std::fmt::Write as _;

use crate::density::Density;
use crate::ticks::format_axis_tick;

/// Tukey's conventional fence multiplier: a whisker reaches the most extreme
/// sample within `1.5 * IQR` of the box, and samples beyond are outliers.
///
/// The value is a convention, not a derivation — Tukey chose it so that
/// roughly 0.7% of a normal sample is flagged. [`Distribution::from_samples_fenced`]
/// takes another; `3.0` is the common "far out" second fence.
pub const DEFAULT_FENCE: f64 = 1.5;

/// The notch half-width coefficient of `McGill`, Tukey & Larsen (1978): the
/// waist spans `median +- NOTCH_COEFFICIENT * IQR / sqrt(n)`, an approximate
/// 95% confidence interval for the median.
const NOTCH_COEFFICIENT: f64 = 1.58;

/// How the quartiles are derived from a sorted sample.
///
/// These are not interchangeable roundings of one answer: for `n = 4` over
/// `[1, 2, 3, 4]` the lower quartile is `1.5`, `1.75` and `1.25`
/// respectively. All three agree on the median. Which one a box was drawn
/// with is therefore part of what the box *means*, which is why
/// [`DistributionSource::Samples`] records it rather than leaving it to a
/// caller's comment.
///
/// The names in parentheses are the types of Hyndman & Fan (*Sample
/// Quantiles in Statistical Packages*, 1996), the standard enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuantileMethod {
    /// Tukey's **hinges** — the median of each half of the data, including
    /// the overall median in both halves when `n` is odd (R's `fivenum`).
    ///
    /// The default, because it is what Tukey defined the box plot on: a
    /// hinge is always a datum or the midpoint of two, so the box edges are
    /// values the sample actually contains.
    #[default]
    Tukey,
    /// Linear interpolation at `(n - 1) * p` (Hyndman & Fan type 7) — the
    /// default of R's `quantile()`, `NumPy`, and Julia.
    Linear,
    /// Linear interpolation at `(n + 1) * p` (Hyndman & Fan type 6) — the spreadsheet's
    /// `PERCENTILE.EXC`, SPSS, and Minitab.
    ///
    /// Spreads the quartiles further apart than [`Linear`](Self::Linear) at
    /// small `n`, so a box built this way is wider and flags fewer outliers.
    Exclusive,
}

impl QuantileMethod {
    /// This method's name, for a readout or a wire field.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tukey => "tukey",
            Self::Linear => "linear",
            Self::Exclusive => "exclusive",
        }
    }

    /// `(q1, median, q3)` of a **non-empty, ascending, all-finite** slice.
    ///
    /// The three methods share this one entry point so a caller cannot reach
    /// one of them by a different route: the sort, the finiteness filter and
    /// the emptiness check all happen once, in
    /// [`Distribution::from_samples_fenced`].
    ///
    /// ★ R1797 — the arithmetic moved to [`crate::Quantiles`], which answers
    /// an ARBITRARY proportion under the same three definitions. This is now
    /// the special case (three fixed proportions) of the general question, and
    /// there is one implementation of it in the crate rather than three.
    fn quartiles(self, sorted: &[f64]) -> (f64, f64, f64) {
        crate::quantile::Quantiles::from_sorted(sorted, self).quartiles()
    }
}

// ★ R1797 — the quantile arithmetic that stood here (`hf7_depth`, `hf6_depth`,
// `interpolate_at`, `tukey_hinges`, `hinge_at`) moved to [`crate::quantile`],
// which answers an ARBITRARY proportion under these same three definitions.
// It moved rather than being copied a fourth time: two other modules in this
// crate had grown their own private versions of the same interpolation, and
// three mechanical copies is this project's lift threshold. What the copies
// were hiding is recorded there.

/// Where the five numbers of a summary sit, in ascending order — the names
/// of the toolkit's `ValuePositions`, so an error can say *which* landmark
/// broke the ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryPosition {
    /// The lower whisker end.
    LowerExtreme,
    /// The bottom of the box.
    LowerQuartile,
    /// The line inside the box.
    Median,
    /// The top of the box.
    UpperQuartile,
    /// The upper whisker end.
    UpperExtreme,
}

impl SummaryPosition {
    /// This landmark's name, for a message or a wire field.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::LowerExtreme => "lower_extreme",
            Self::LowerQuartile => "lower_quartile",
            Self::Median => "median",
            Self::UpperQuartile => "upper_quartile",
            Self::UpperExtreme => "upper_extreme",
        }
    }
}

impl SummaryPosition {
    /// The word a **person** hears, as distinct from
    /// [`name`](Self::name)'s wire spelling (R1634).
    ///
    /// Two audiences, so two strings: a client matches `lower_extreme` and a
    /// screen reader announces "lower whisker", and running the wire spelling
    /// through a reader would have it say an underscore. The same split R1628
    /// drew between a measurement's shape and a scene's.
    ///
    /// **"Whisker", not "minimum"** — and that is the reason this is a table
    /// rather than a substitution. A whisker end is the smallest sample inside
    /// the fence; a distribution with outliers has values below it, so
    /// announcing it as the minimum would state something false about the data
    /// to precisely the reader who cannot check it against the picture.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LowerExtreme => "lower whisker",
            Self::LowerQuartile => "lower quartile",
            Self::Median => "median",
            Self::UpperQuartile => "upper quartile",
            Self::UpperExtreme => "upper whisker",
        }
    }
}

impl std::fmt::Display for SummaryPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a [`Distribution`] could not be built.
///
/// Every arm names the input that was wrong, because these are all *caller*
/// errors and a caller cannot fix one it cannot locate. The toolkit reports
/// none of them: box set accepts any five doubles in any order, including NaN,
/// and paints whatever geometry they imply.
///
/// **No arm carries a non-finite number.** An error is a value a caller
/// compares — and `NaN != NaN`, so an arm holding one would not equal
/// itself, making `err == DistributionError::Fence(f64::NAN)` false forever.
/// So the two non-finite cases ([`FenceNotFinite`](Self::FenceNotFinite),
/// [`NotFinite`](Self::NotFinite)) name the *position* and drop the value,
/// while the arms that do carry numbers are only reachable once finiteness
/// has been established. That ordering is load-bearing in
/// [`Distribution::from_summary`]: it checks all five for finiteness before
/// it checks any for order, so [`OutOfOrder`](Self::OutOfOrder)'s two `f64`s
/// are always comparable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistributionError {
    /// Not one finite sample was supplied, so there is nothing to summarise.
    /// Distinct from an out-of-order summary: nothing was *wrong*, there was
    /// simply no data (an endpoint with no traffic in the window).
    NoFiniteSamples,
    /// The fence multiplier is NaN or infinite. Usually an upstream
    /// computation that produced no answer, rather than a typed-in constant.
    FenceNotFinite,
    /// The fence multiplier is negative, which would put the whisker limits
    /// *inside* the box and make every sample outside the middle 50% an
    /// outlier — a plot that looks right and means nothing.
    FenceNegative(f64),
    /// A pre-computed summary carried a non-finite number at this position.
    NotFinite {
        /// Which of the five it was.
        at: SummaryPosition,
    },
    /// A pre-computed summary is not non-decreasing: `at` holds `value`,
    /// which is below the `previous` landmark.
    OutOfOrder {
        /// The first position that went backwards.
        at: SummaryPosition,
        /// What that position held.
        value: f64,
        /// What the position below it held.
        previous: f64,
    },
    /// R1626 — a density was attached to a summary-sourced distribution.
    /// Five pre-computed numbers cannot have produced one, so the estimate
    /// describes some other sample set.
    DensityWithoutSamples,
    /// R1626 — the samples summarised fine but could not support a density.
    Density(crate::DensityError),
}

impl std::fmt::Display for DistributionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFiniteSamples => f.write_str("no finite samples to summarise"),
            Self::FenceNotFinite => f.write_str("fence multiplier is not finite"),
            Self::FenceNegative(k) => write!(f, "fence multiplier {k} is negative"),
            Self::NotFinite { at } => write!(f, "summary position {at} is not finite"),
            Self::OutOfOrder {
                at,
                value,
                previous,
            } => write!(f, "summary position {at} is {value}, below {previous}"),
            Self::DensityWithoutSamples => f.write_str(
                "a pre-computed summary carries no samples, so no density can belong to it",
            ),
            Self::Density(e) => write!(f, "the samples support no density estimate: {e}"),
        }
    }
}

impl std::error::Error for DistributionError {}

/// Where a [`Distribution`]'s numbers came from — and therefore which
/// statistics are answerable at all.
///
/// This is the part the toolkit has no room for. A box set is five doubles:
/// nothing records whether they were computed or typed in, by what definition,
/// or over how many samples. Carrying the provenance *in the value* is what
/// makes [`Distribution::notch`] total — it returns `None` for a summary because the confidence
/// interval is not merely unknown there, it does not exist.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistributionSource {
    /// Derived from raw samples by this crate.
    Samples {
        /// How many finite samples were summarised. The `n` of the notch.
        count: usize,
        /// Which quantile definition produced the quartiles.
        method: QuantileMethod,
        /// The fence multiplier that classified the outliers and limited the
        /// whiskers.
        fence: f64,
    },
    /// Handed in already summarised — the toolkit's box set contract. No
    /// sample count, so no notch; no samples, so no outliers.
    Summary,
}

/// The summary of a sample at one slot: a box, a median, two whiskers, and
/// the individual samples beyond the fence.
///
/// Build it from data with [`from_samples`](Self::from_samples) — which is
/// what makes the whiskers Tukey's rather than the plain extremes — or from
/// five pre-computed numbers with [`from_summary`](Self::from_summary).
///
/// The five landmarks are non-decreasing by construction on both paths:
/// derivation cannot produce an inverted box, and
/// [`from_summary`](Self::from_summary) rejects one.
///
/// **The fields are private, and R1567 made them so.** That sentence above
/// is the type's whole claim, and while a caller could write
/// `d.q1 = 100.0` after construction it was false — the rejection in
/// `from_summary` was defeatable by assignment, which is the same shape as a
/// checked constructor with a public setter. The comparison that surfaced it
/// is [`Candle`](crate::Candle), whose invariant is likewise the only reason
/// the type exists.
#[derive(Debug, Clone, PartialEq)]
pub struct Distribution {
    label: String,
    q1: f64,
    median: f64,
    q3: f64,
    lower_whisker: f64,
    upper_whisker: f64,
    outliers: Vec<f64>,
    source: DistributionSource,
    /// R1626 — the kernel density estimate behind a violin, when the caller
    /// attached one. An **attachment** rather than a field the constructors
    /// fill, for two reasons: a summary-sourced distribution has no samples
    /// to estimate from and so can never have one, and a box plot that never
    /// draws a violin should not pay for a density it will not use.
    density: Option<Density>,
}

impl Distribution {
    /// The five landmarks in ascending order — the enumeration
    /// [`at`](Self::at) is the accessor for (R1634).
    ///
    /// The peer of [`Candle::positions`](crate::Candle::positions), and here
    /// for the same reason: a summary IS its five numbers, so "which numbers
    /// does a distribution carry" is a fact about this type rather than
    /// something each consumer re-lists. `off_scale` listed them once and the
    /// accessible projection would have listed them again.
    #[must_use]
    pub const fn positions() -> [SummaryPosition; 5] {
        use SummaryPosition as P;
        [
            P::LowerExtreme,
            P::LowerQuartile,
            P::Median,
            P::UpperQuartile,
            P::UpperExtreme,
        ]
    }

    /// The value at one landmark (R1634).
    #[must_use]
    pub const fn at(&self, at: SummaryPosition) -> f64 {
        match at {
            SummaryPosition::LowerExtreme => self.lower_whisker,
            SummaryPosition::LowerQuartile => self.q1,
            SummaryPosition::Median => self.median,
            SummaryPosition::UpperQuartile => self.q3,
            SummaryPosition::UpperExtreme => self.upper_whisker,
        }
    }

    /// The slot label — a category name on the x-axis.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Bottom of the box (the lower quartile / hinge).
    #[must_use]
    pub const fn q1(&self) -> f64 {
        self.q1
    }

    /// The line inside the box.
    #[must_use]
    pub const fn median(&self) -> f64 {
        self.median
    }

    /// Top of the box (the upper quartile / hinge).
    #[must_use]
    pub const fn q3(&self) -> f64 {
        self.q3
    }

    /// The lower whisker end: the most extreme sample still inside the fence
    /// when derived, or the caller's lower extreme when handed in.
    #[must_use]
    pub const fn lower_whisker(&self) -> f64 {
        self.lower_whisker
    }

    /// The upper whisker end, mirroring [`lower_whisker`](Self::lower_whisker).
    #[must_use]
    pub const fn upper_whisker(&self) -> f64 {
        self.upper_whisker
    }

    /// The samples outside the fence, ascending. Always empty for a
    /// [`Summary`](DistributionSource::Summary) — not because there were
    /// none, but because a summary has no samples to classify.
    #[must_use]
    pub fn outliers(&self) -> &[f64] {
        &self.outliers
    }

    /// How these numbers came to be, and hence which statistics exist.
    #[must_use]
    pub const fn source(&self) -> DistributionSource {
        self.source
    }
    /// Summarise `samples` under `method`, with Tukey's conventional
    /// [`DEFAULT_FENCE`].
    ///
    /// Non-finite samples are dropped before anything is computed, so a NaN
    /// in the input cannot poison a quartile; `Err(NoFiniteSamples)` when
    /// that leaves nothing.
    ///
    /// # Errors
    ///
    /// [`DistributionError::NoFiniteSamples`] when no sample is finite.
    pub fn from_samples(
        label: impl Into<String>,
        samples: &[f64],
        method: QuantileMethod,
    ) -> Result<Self, DistributionError> {
        Self::from_samples_fenced(label, samples, method, DEFAULT_FENCE)
    }

    /// [`from_samples`](Self::from_samples) with an explicit fence
    /// multiplier — `3.0` is the conventional "far out" second fence, and
    /// `0.0` makes every sample outside the box an outlier.
    ///
    /// # Errors
    ///
    /// [`DistributionError::NoFiniteSamples`] when no sample is finite,
    /// [`DistributionError::FenceNotFinite`] when `fence` is NaN or
    /// infinite, and [`DistributionError::FenceNegative`] when it is below
    /// zero.
    pub fn from_samples_fenced(
        label: impl Into<String>,
        samples: &[f64],
        method: QuantileMethod,
        fence: f64,
    ) -> Result<Self, DistributionError> {
        if !fence.is_finite() {
            return Err(DistributionError::FenceNotFinite);
        }
        if fence < 0.0 {
            return Err(DistributionError::FenceNegative(fence));
        }
        let mut sorted: Vec<f64> = samples.iter().copied().filter(|v| v.is_finite()).collect();
        if sorted.is_empty() {
            return Err(DistributionError::NoFiniteSamples);
        }
        // `total_cmp` rather than `partial_cmp().unwrap()`: the filter above
        // already removed every NaN, so the two agree here — but a sort that
        // cannot panic is the one to write down, because the invariant it
        // relies on lives in a different statement.
        sorted.sort_unstable_by(f64::total_cmp);

        let (q1, median, q3) = method.quartiles(&sorted);
        // Tukey's fence. `q3 - q1` is non-negative because the quartiles come
        // from an ascending slice at non-decreasing depths, so the fence
        // always contains the box and the whisker sets below are non-empty.
        let reach = fence * (q3 - q1);
        let (lo_fence, hi_fence) = (q1 - reach, q3 + reach);

        let mut outliers = Vec::new();
        let mut lower_whisker = f64::INFINITY;
        let mut upper_whisker = f64::NEG_INFINITY;
        for &v in &sorted {
            if v < lo_fence || v > hi_fence {
                outliers.push(v);
            } else {
                lower_whisker = lower_whisker.min(v);
                upper_whisker = upper_whisker.max(v);
            }
        }
        // At least one sample is inside the fence: the maximum is `>= q3 >=
        // q1 >= lo_fence` and the minimum is `<= q1 <= q3 <= hi_fence`, so
        // the two cannot both fall outside. The fallback keeps the type total
        // rather than asserting that reasoning at runtime.
        if !lower_whisker.is_finite() || !upper_whisker.is_finite() {
            lower_whisker = q1;
            upper_whisker = q3;
        }

        Ok(Self {
            label: label.into(),
            q1,
            median,
            q3,
            lower_whisker,
            upper_whisker,
            outliers,
            source: DistributionSource::Samples {
                count: sorted.len(),
                method,
                fence,
            },
            density: None,
        })
    }

    /// Take five pre-computed numbers — the toolkit's
    /// `box set(le, lq, m, uq, ue)` — for a consumer whose data was
    /// summarised upstream (a database `percentile_cont`, a metrics
    /// pipeline) and who therefore has no samples to hand.
    ///
    /// The whiskers are the caller's extremes verbatim: nothing here knows
    /// whether a fence was applied upstream, so nothing here claims one was.
    /// There are no outliers and no [`notch`](Self::notch) on this path.
    ///
    /// # Errors
    ///
    /// [`DistributionError::NotFinite`] naming the first non-finite landmark, or [`DistributionError::OutOfOrder`] naming the first
    /// one that goes backwards. The toolkit performs neither check and paints
    /// an inverted box in silence.
    pub fn from_summary(
        label: impl Into<String>,
        lower_extreme: f64,
        q1: f64,
        median: f64,
        q3: f64,
        upper_extreme: f64,
    ) -> Result<Self, DistributionError> {
        use SummaryPosition as P;
        let ladder = [
            (P::LowerExtreme, lower_extreme),
            (P::LowerQuartile, q1),
            (P::Median, median),
            (P::UpperQuartile, q3),
            (P::UpperExtreme, upper_extreme),
        ];
        for (at, value) in ladder {
            if !value.is_finite() {
                return Err(DistributionError::NotFinite { at });
            }
        }
        for window in ladder.windows(2) {
            let [(_, previous), (at, value)] = *window else {
                continue;
            };
            if value < previous {
                return Err(DistributionError::OutOfOrder {
                    at,
                    value,
                    previous,
                });
            }
        }
        Ok(Self {
            label: label.into(),
            q1,
            median,
            q3,
            lower_whisker: lower_extreme,
            upper_whisker: upper_extreme,
            outliers: Vec::new(),
            source: DistributionSource::Summary,
            density: None,
        })
    }

    /// R1626 — attach the kernel density estimate a violin is drawn from.
    ///
    /// Refused for a summary-sourced distribution: five pre-computed numbers
    /// cannot have produced a density, so an estimate arriving beside them
    /// describes *some other* sample set and the two would disagree about the
    /// same category with nothing to say so. That is the one error this
    /// attachment can make, so it is the one it rejects.
    ///
    /// # Errors
    ///
    /// [`DistributionError::DensityWithoutSamples`] when this distribution
    /// was built by [`from_summary`](Self::from_summary).
    pub fn with_density(mut self, density: Density) -> Result<Self, DistributionError> {
        if matches!(self.source, DistributionSource::Summary) {
            return Err(DistributionError::DensityWithoutSamples);
        }
        self.density = Some(density);
        Ok(self)
    }

    /// The attached density, when there is one.
    #[must_use]
    pub const fn density(&self) -> Option<&Density> {
        self.density.as_ref()
    }

    /// R1626 — summarise `samples` **and** estimate their density in one
    /// call, so the two derivations cannot describe different data.
    ///
    /// The reason this exists rather than leaving the caller to compose
    /// [`from_samples`](Self::from_samples) with
    /// [`Density::estimate`](crate::Density::estimate) and
    /// [`with_density`](Self::with_density): that composition takes the
    /// sample slice twice, and nothing would notice if the second one were a
    /// different slice.
    ///
    /// # Errors
    ///
    /// Whatever [`from_samples`](Self::from_samples) raises, or
    /// [`DistributionError::Density`] carrying the estimate's own refusal.
    ///
    /// R1628 — `spec` is one value rather than a widening argument list, and
    /// it carries the bounded choice, because reflecting the kernel is part of
    /// estimating rather than something done to an estimate afterwards.
    pub fn from_samples_with_density(
        label: impl Into<String>,
        samples: &[f64],
        method: QuantileMethod,
        spec: crate::DensitySpec,
    ) -> Result<Self, DistributionError> {
        let summary = Self::from_samples(label, samples, method)?;
        let density = Density::estimate(samples, spec).map_err(DistributionError::Density)?;
        summary.with_density(density)
    }

    /// The interquartile range `q3 - q1` — the height of the box, and the
    /// unit both the fence and the notch are measured in.
    #[must_use]
    pub fn iqr(&self) -> f64 {
        self.q3 - self.q1
    }

    /// How many finite samples this summary was derived from, or `None` when
    /// it was handed in pre-computed.
    #[must_use]
    pub const fn count(&self) -> Option<usize> {
        match self.source {
            DistributionSource::Samples { count, .. } => Some(count),
            DistributionSource::Summary => None,
        }
    }

    /// Which quantile definition produced the quartiles, or `None` for a
    /// pre-computed summary — where the definition is the *upstream's* and
    /// this crate would be inventing it.
    #[must_use]
    pub const fn method(&self) -> Option<QuantileMethod> {
        match self.source {
            DistributionSource::Samples { method, .. } => Some(method),
            DistributionSource::Summary => None,
        }
    }

    /// The `median +- 1.58 * IQR / sqrt(n)` confidence interval of `McGill`,
    /// Tukey & Larsen (1978) — the waist of a notched box plot. Two boxes
    /// whose notches do not overlap have significantly different medians at
    /// roughly 95%.
    ///
    /// `None` for a [`Summary`](DistributionSource::Summary): the interval is a
    /// function of `n`, and a summary has none. That is the whole reason [`DistributionSource`]
    /// is part of the value rather than a builder argument — the toolkit's box
    /// set carries no `n`, so a notch could not be offered there even as a
    /// paint option.
    ///
    /// The interval is **not** clamped to the box. A small sample with a
    /// wide IQR yields a notch taller than the box itself, which R draws as
    /// a self-crossing "bowtie" and warns about; folding it into the box
    /// would hide exactly the case a reader most needs to see.
    #[must_use]
    pub fn notch(&self) -> Option<(f64, f64)> {
        let DistributionSource::Samples { count, .. } = self.source else {
            return None;
        };
        if count == 0 {
            return None;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "a sample count is a display-scale cardinality; f64 is exact to 2^53"
        )]
        let half = NOTCH_COEFFICIENT * self.iqr() / (count as f64).sqrt();
        Some((self.median - half, self.median + half))
    }

    /// The full extent this distribution occupies on the value axis —
    /// whiskers **and** outliers, because an outlier is drawn and an axis
    /// that does not reach it would place the mark outside the plot.
    #[must_use]
    pub fn extent(&self) -> (f64, f64) {
        let mut lo = self.lower_whisker.min(self.q1);
        let mut hi = self.upper_whisker.max(self.q3);
        for &v in &self.outliers {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        (lo, hi)
    }

    /// [`extent`](Self::extent) restricted to the strictly positive part, or
    /// `None` when nothing here is positive — the auto-domain source for a
    /// **logarithmic** value axis (R1528's rule, applied to a datum with
    /// extent rather than to a point).
    #[must_use]
    pub fn positive_extent(&self) -> Option<(f64, f64)> {
        let mut span: Option<(f64, f64)> = None;
        let mut widen = |v: f64| {
            if v > 0.0 {
                span = Some(match span {
                    Some((lo, hi)) => (lo.min(v), hi.max(v)),
                    None => (v, v),
                });
            }
        };
        for v in self.landmarks() {
            widen(v);
        }
        for &v in &self.outliers {
            widen(v);
        }
        span
    }

    /// The five landmarks in ascending order — the box, its median, and the
    /// two whisker ends. The one enumeration both extents and the readout
    /// walk, so a landmark cannot be added to the type and forgotten by a
    /// measurement.
    fn landmarks(&self) -> [f64; 5] {
        [
            self.lower_whisker,
            self.q1,
            self.median,
            self.q3,
            self.upper_whisker,
        ]
    }

    /// The summary as one line, at the precision `step` implies — the text a
    /// scrub tooltip and an assistive-technology readout both take, so the
    /// two can never state different numbers.
    ///
    /// States the sample count and the outlier count when they exist, and
    /// says so when they do not: a box over 6 samples and a box over 60,000
    /// are the same picture, and which one a reader is looking at is the
    /// first thing they need.
    #[must_use]
    pub fn readout(&self, step: f64) -> String {
        let q = |v: f64| format_axis_tick(v, step);
        let mut out = format!(
            "{}: median {}, IQR {}\u{2013}{}, whiskers {}\u{2013}{}",
            self.label,
            q(self.median),
            q(self.q1),
            q(self.q3),
            q(self.lower_whisker),
            q(self.upper_whisker),
        );
        match self.source {
            DistributionSource::Samples { count, method, .. } => {
                // `write!` into the buffer rather than `push_str(&format!(..))`:
                // the same text, one allocation instead of two per clause.
                let _ = write!(out, ", n {count} ({})", method.name());
                if !self.outliers.is_empty() {
                    let _ = write!(out, ", {} outliers", self.outliers.len());
                }
            }
            DistributionSource::Summary => out.push_str(", pre-computed summary"),
        }
        out
    }
}

/// The combined value extent across `distributions`, or `None` when the slice
/// is empty — the auto-domain source for a box plot's value axis.
#[must_use]
pub fn distribution_bounds(distributions: &[Distribution]) -> Option<(f64, f64)> {
    distributions
        .iter()
        .map(Distribution::extent)
        .reduce(|(alo, ahi), (blo, bhi)| (alo.min(blo), ahi.max(bhi)))
}

/// [`distribution_bounds`] restricted to strictly positive values — the
/// auto-domain source when the value axis is logarithmic.
#[must_use]
pub fn positive_distribution_bounds(distributions: &[Distribution]) -> Option<(f64, f64)> {
    distributions
        .iter()
        .filter_map(Distribution::positive_extent)
        .reduce(|(alo, ahi), (blo, bhi)| (alo.min(blo), ahi.max(bhi)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The textbook quartet: every quantile definition disagrees here.
    const QUARTET: [f64; 4] = [1.0, 2.0, 3.0, 4.0];

    /// ★ The three methods are not roundings of one answer — they give three
    /// different boxes over the same four samples, which is why naming the
    /// method is load-bearing rather than documentation. The toolkit computes
    /// none of them, so a box set cannot record which one built it.
    ///
    /// The counterfactual is inside the test: all three agree on the MEDIAN,
    /// so this is not "the methods differ everywhere" (which would also pass
    /// if `quartiles` simply returned the method's discriminant).
    #[test]
    fn r1553_the_three_quantile_methods_disagree_on_the_box() {
        let q1_of = |m| {
            Distribution::from_samples("q", &QUARTET, m)
                .expect("four finite samples")
                .q1()
        };
        assert!((q1_of(QuantileMethod::Tukey) - 1.5).abs() < 1e-12);
        assert!((q1_of(QuantileMethod::Linear) - 1.75).abs() < 1e-12);
        assert!((q1_of(QuantileMethod::Exclusive) - 1.25).abs() < 1e-12);

        // ...and agree on the median, which is what makes the disagreement a
        // property of the quartile definition and not of the whole summary.
        for m in [
            QuantileMethod::Tukey,
            QuantileMethod::Linear,
            QuantileMethod::Exclusive,
        ] {
            let d = Distribution::from_samples("q", &QUARTET, m).expect("four finite samples");
            assert!(
                (d.median() - 2.5).abs() < 1e-12,
                "{m:?} median {} is not the ordinary median",
                d.median
            );
        }
    }

    /// ★ Tukey's hinges over an odd sample reproduce R's `fivenum(1:5)`.
    #[test]
    fn r1553_tukey_hinges_match_fivenum() {
        let d =
            Distribution::from_samples("odd", &[1.0, 2.0, 3.0, 4.0, 5.0], QuantileMethod::Tukey)
                .expect("five finite samples");
        assert!((d.q1 - 2.0).abs() < 1e-12, "lower hinge {}", d.q1);
        assert!((d.median() - 3.0).abs() < 1e-12);
        assert!((d.q3 - 4.0).abs() < 1e-12, "upper hinge {}", d.q3);
    }

    /// ★ The whisker stops at the fence and the sample beyond it becomes an
    /// individual outlier — the half of the box plot the toolkit's five-slot
    /// box set cannot express at all.
    ///
    /// The counterfactual is the second assertion: with the fence widened the
    /// SAME sample is inside, so the classification is the fence's doing and
    /// not "the largest sample is always an outlier".
    #[test]
    fn r1553_the_fence_limits_the_whisker_and_names_the_outlier() {
        // Nine tight samples plus one far value. Under Tukey's hinges the box
        // is 3..7, IQR 4, so the fence reaches 13 — and 40 is well past it.
        let samples = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 40.0];
        let d = Distribution::from_samples("fenced", &samples, QuantileMethod::Tukey)
            .expect("ten finite samples");
        assert_eq!(
            d.outliers(),
            vec![40.0],
            "the far sample is the only outlier"
        );
        assert!(
            (d.upper_whisker() - 9.0).abs() < 1e-12,
            "the whisker stops at the most extreme sample INSIDE the fence, got {}",
            d.upper_whisker
        );
        assert!((d.lower_whisker() - 1.0).abs() < 1e-12);
        // The extent still reaches the outlier: it is drawn, so the axis must
        // hold it.
        assert!((d.extent().1 - 40.0).abs() < 1e-12);

        // Counterfactual: a wide fence swallows it, so nothing here is
        // "the maximum is an outlier".
        let wide =
            Distribution::from_samples_fenced("fenced", &samples, QuantileMethod::Tukey, 9.0)
                .expect("ten finite samples");
        assert!(wide.outliers().is_empty(), "a wide fence classifies none");
        assert!((wide.upper_whisker() - 40.0).abs() < 1e-12);
    }

    /// ★ The notch exists only where `n` does. A derived distribution answers
    /// the McGill-Tukey-Larsen interval; a pre-computed summary answers
    /// `None`, because the statistic is a function of a sample count the
    /// summary never carried. The toolkit's box set is always the second case.
    #[test]
    fn r1553_the_notch_needs_the_sample_count() {
        let d = Distribution::from_samples("n", &[1.0, 2.0, 3.0, 4.0], QuantileMethod::Tukey)
            .expect("four finite samples");
        let (lo, hi) = d.notch().expect("a derived summary carries n");
        // median 2.5, IQR 3.5 - 1.5 = 2, n = 4 -> half = 1.58 * 2 / 2 = 1.58.
        assert!((lo - (2.5 - 1.58)).abs() < 1e-12, "notch lo {lo}");
        assert!((hi - (2.5 + 1.58)).abs() < 1e-12, "notch hi {hi}");
        assert_eq!(d.count(), Some(4));
        assert_eq!(d.method(), Some(QuantileMethod::Tukey));

        let s = Distribution::from_summary("s", 0.0, 1.5, 2.5, 3.5, 5.0).expect("ordered");
        assert!(s.notch().is_none(), "a summary has no n, so no notch");
        assert_eq!(s.count(), None);
        assert_eq!(s.method(), None);
        assert!(s.outliers().is_empty());
    }

    /// ★ An out-of-order summary is rejected and the rejection NAMES the
    /// landmark. `setValue` accepts the same input and paints an
    /// inverted box.
    #[test]
    fn r1553_an_inverted_summary_is_rejected_by_position() {
        let err = Distribution::from_summary("bad", 0.0, 9.0, 2.5, 3.5, 5.0)
            .expect_err("q1 above the median");
        assert_eq!(
            err,
            DistributionError::OutOfOrder {
                at: SummaryPosition::Median,
                value: 2.5,
                previous: 9.0,
            }
        );
        assert!(err.to_string().contains("median"));

        let err = Distribution::from_summary("bad", 0.0, 1.0, f64::NAN, 3.5, 5.0)
            .expect_err("a NaN median");
        assert_eq!(
            err,
            DistributionError::NotFinite {
                at: SummaryPosition::Median
            }
        );

        // Counterfactual: the ordered version of the same five is accepted,
        // so this is not "from_summary always fails".
        assert!(Distribution::from_summary("ok", 0.0, 1.0, 2.5, 3.5, 5.0).is_ok());
        // Equal landmarks are ordered (a constant sample), not an inversion.
        assert!(Distribution::from_summary("flat", 2.0, 2.0, 2.0, 2.0, 2.0).is_ok());
    }

    /// ★ Non-finite samples are dropped before anything is computed, so one
    /// NaN cannot poison a quartile; an all-NaN input is the distinct
    /// "nothing to summarise" error rather than a box at NaN.
    #[test]
    fn r1553_non_finite_samples_are_dropped_not_propagated() {
        let d = Distribution::from_samples(
            "mixed",
            &[f64::NAN, 1.0, 2.0, f64::INFINITY, 3.0, 4.0],
            QuantileMethod::Tukey,
        )
        .expect("four finite samples survive");
        assert_eq!(d.count(), Some(4), "only the finite samples are counted");
        assert!((d.median() - 2.5).abs() < 1e-12);

        assert_eq!(
            Distribution::from_samples("none", &[f64::NAN], QuantileMethod::Tukey),
            Err(DistributionError::NoFiniteSamples)
        );
        assert_eq!(
            Distribution::from_samples("none", &[], QuantileMethod::Tukey),
            Err(DistributionError::NoFiniteSamples)
        );
    }

    /// ★ A negative fence is refused rather than silently clamped: it would
    /// place the whisker limits INSIDE the box, making every sample outside
    /// the middle 50% an outlier — a plot that looks fine and means nothing.
    ///
    /// The two refusals are distinct arms, and the non-finite one carries no
    /// value: an error holding NaN would not compare equal to itself, so a
    /// caller could never match on it. That is why this assertion can be an
    /// `assert_eq!` at all.
    #[test]
    fn r1553_a_nonsense_fence_is_refused() {
        assert_eq!(
            Distribution::from_samples_fenced("f", &QUARTET, QuantileMethod::Tukey, -1.0),
            Err(DistributionError::FenceNegative(-1.0))
        );
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                Distribution::from_samples_fenced("f", &QUARTET, QuantileMethod::Tukey, bad),
                Err(DistributionError::FenceNotFinite),
                "fence {bad} must be refused"
            );
        }
        // The comparability this arm exists for: the error equals itself.
        let err = Distribution::from_samples_fenced("f", &QUARTET, QuantileMethod::Tukey, f64::NAN)
            .expect_err("a NaN fence is refused");
        assert_eq!(
            err, err,
            "an error a caller cannot match on is not an error"
        );
        // Zero is legal and meaningful: every sample outside the box is an
        // outlier, the "no whiskers" reading.
        let d = Distribution::from_samples_fenced("f", &QUARTET, QuantileMethod::Tukey, 0.0)
            .expect("a zero fence is legal");
        assert_eq!(d.outliers(), vec![1.0, 4.0]);
    }

    /// ★ A single sample summarises to a degenerate but total distribution —
    /// no division by zero, no empty whisker set.
    #[test]
    fn r1553_one_sample_is_a_degenerate_distribution() {
        let d = Distribution::from_samples("one", &[7.0], QuantileMethod::Tukey)
            .expect("one finite sample");
        assert_eq!(
            (d.q1(), d.median, d.q3(), d.lower_whisker, d.upper_whisker),
            (7.0, 7.0, 7.0, 7.0, 7.0)
        );
        assert!(d.outliers().is_empty());
        assert!(
            d.iqr().abs() < f64::EPSILON,
            "a constant sample has no spread"
        );
        let (lo, hi) = d.notch().expect("a zero-width notch, not None");
        assert!(
            (lo - 7.0).abs() < 1e-12 && (hi - 7.0).abs() < 1e-12,
            "a zero-width notch, not NaN: {lo}..{hi}"
        );
        assert_eq!(d.extent(), (7.0, 7.0));

        for m in [QuantileMethod::Linear, QuantileMethod::Exclusive] {
            let d = Distribution::from_samples("one", &[7.0], m).expect("one finite sample");
            assert_eq!((d.q1(), d.median, d.q3), (7.0, 7.0, 7.0), "{m:?}");
        }
    }

    /// ★ The positive-only extent is the log axis's domain source, and it
    /// measures the OUTLIERS too — a mark the axis cannot reach would be
    /// drawn outside the plot.
    #[test]
    fn r1553_positive_extent_measures_marks_not_just_the_box() {
        let d = Distribution::from_samples(
            "log",
            &[0.0, -3.0, 1.0, 2.0, 3.0, 4.0, 900.0],
            QuantileMethod::Tukey,
        )
        .expect("finite samples");
        assert!(
            d.outliers().contains(&900.0),
            "the far sample is an outlier: {:?}",
            d.outliers
        );
        let (lo, hi) = d.positive_extent().expect("some positive value");
        assert!((hi - 900.0).abs() < 1e-12, "the outlier sets the top: {hi}");
        assert!(lo > 0.0, "the non-positive samples are excluded: {lo}");

        // Nothing positive -> nothing to domain a log axis from.
        let none =
            Distribution::from_summary("neg", -9.0, -5.0, -4.0, -3.0, -1.0).expect("ordered");
        assert!(none.positive_extent().is_none());
    }

    /// ★ The combined bounds span every distribution, and the positive-only
    /// pair skips a wholly non-positive one instead of dragging the domain
    /// to zero.
    #[test]
    fn r1553_bounds_combine_every_distribution() {
        let a = Distribution::from_summary("a", 1.0, 2.0, 3.0, 4.0, 5.0).expect("ordered");
        let b = Distribution::from_summary("b", -8.0, -6.0, -5.0, -4.0, -2.0).expect("ordered");
        assert_eq!(
            distribution_bounds(&[a.clone(), b.clone()]),
            Some((-8.0, 5.0))
        );
        assert_eq!(
            positive_distribution_bounds(&[a, b]),
            Some((1.0, 5.0)),
            "the wholly negative distribution contributes nothing"
        );
        assert_eq!(distribution_bounds(&[]), None);
        assert_eq!(positive_distribution_bounds(&[]), None);
    }

    /// ★ The readout states the sample count and the method, and says
    /// "pre-computed" when it cannot — the difference between a box a reader
    /// can weigh and a picture. The toolkit's charts announce nothing at all.
    #[test]
    fn r1553_the_readout_states_its_provenance() {
        let d = Distribution::from_samples(
            "api",
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 40.0],
            QuantileMethod::Linear,
        )
        .expect("ten finite samples");
        let text = d.readout(1.0);
        assert!(text.starts_with("api: median "), "{text}");
        assert!(text.contains("n 10 (linear)"), "{text}");
        assert!(text.contains("1 outliers"), "{text}");

        let s = Distribution::from_summary("db", 0.0, 1.0, 2.0, 3.0, 4.0).expect("ordered");
        let text = s.readout(1.0);
        assert!(text.contains("pre-computed summary"), "{text}");
        assert!(!text.contains(" n "), "a summary states no count: {text}");
    }
}
