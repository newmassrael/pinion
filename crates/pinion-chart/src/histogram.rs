//! R1793 §5.29 — a **histogram that chooses its own bins and says which rule
//! chose them**.
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
//! their spread, and the interquartile range. A reader looking at a surprising
//! histogram has two hypotheses, *the data is like that* and *the binning did
//! that*, and no published histogram anywhere in this tree could tell them
//! apart. Now the chart says which rule ran and what it read.
//!
//! ⚠ **No floor was measured for this item.** The reference toolkit's charting
//! is a separate module and this install does not carry it, so there is nothing
//! here to compare against and no superiority is claimed on that axis. The
//! comparison this module is actually held to is INTERNAL and stricter: the
//! crate's own KDE already does this, and the histogram did not.

use crate::bar::Bar;

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
        }
    }
}

/// Why a histogram could not be binned.
///
/// The variants of [`crate::density::DensityError`] that apply, and no others:
/// this is the same refusal about the same samples.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinError {
    /// Fewer than two finite samples: there is no spread to divide.
    TooFewSamples,
    /// Every sample is the same value. A point mass is not a distribution, and
    /// drawing one bar of an arbitrary width would be inventing the shape.
    NoSpread,
    /// A [`BinRule::Fixed`] of zero bins.
    BadBinCount,
}

impl core::fmt::Display for BinError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::TooFewSamples => "a histogram needs at least two finite samples",
            Self::NoSpread => "every sample is the same value, so there is no spread to divide",
            Self::BadBinCount => "a fixed bin count must be at least one",
        })
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
    /// How many finite samples were binned.
    pub n: usize,
    /// The smallest sample.
    pub min: f64,
    /// The largest sample.
    pub max: f64,
    /// The population standard deviation.
    pub sigma: f64,
    /// The interquartile range, by linear interpolation between order
    /// statistics — the same convention `Distribution` uses for its box.
    pub iqr: f64,
}

/// Samples divided into bins, with the rule that divided them.
#[derive(Debug, Clone, PartialEq)]
pub struct Binned {
    edges: Vec<f64>,
    counts: Vec<u32>,
    rule: BinRule,
    basis: Basis,
}

impl Binned {
    /// Bin `samples` by `rule`.
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
    /// finite ones, no spread at all, or a fixed count of zero.
    pub fn of(samples: &[f64], rule: BinRule) -> Result<Self, BinError> {
        let mut finite: Vec<f64> = samples.iter().copied().filter(|s| s.is_finite()).collect();
        if finite.len() < 2 {
            return Err(BinError::TooFewSamples);
        }
        finite.sort_by(f64::total_cmp);
        let n = finite.len();
        let (min, max) = (finite[0], finite[n - 1]);
        // `>=` rather than `==`, and not to satisfy a lint: the slice is sorted,
        // so `min >= max` is exactly "no spread" and says so without an equality
        // on floats. A margin here would be WRONG — it would refuse a genuinely
        // narrow spread that bins perfectly well.
        if min >= max {
            return Err(BinError::NoSpread);
        }
        let basis = Basis {
            n,
            min,
            max,
            sigma: sigma_of(&finite),
            iqr: iqr_of(&finite),
        };
        let count = bin_count(rule, &basis)?;
        let width = (max - min) / f64::from(count);
        let edges: Vec<f64> = (0..=count).map(|k| min + width * f64::from(k)).collect();
        let mut counts = vec![0_u32; count as usize];
        for sample in &finite {
            let slot = which_bin(*sample, min, width, count);
            counts[slot] += 1;
        }
        Ok(Self {
            edges,
            counts,
            rule,
            basis,
        })
    }

    /// The bin boundaries, `bins() + 1` of them, ascending.
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

    /// The uniform bin width.
    ///
    /// Read off the stored edges rather than recomputed from the span and the
    /// count, so the width and the edges cannot disagree — the same reason
    /// `select_x_range` reads the extent a bar carries instead of deriving one.
    #[must_use]
    pub fn width(&self) -> f64 {
        self.edges[1] - self.edges[0]
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

    /// The bars a [`crate::BarChart`] draws, each carrying its own numeric
    /// extent.
    ///
    /// ★ Through [`crate::Bar::with_bin`], which is what makes the result
    /// cross-filterable by [`crate::BarChart::select_x_range`] the moment it is
    /// drawn — the half of the histogram that R1395 already built, and the half
    /// this module exists to feed. `label` is the bin's lower edge, which is
    /// what a reader matches against an axis tick.
    #[must_use]
    pub fn bars(&self) -> Vec<Bar> {
        self.counts
            .iter()
            .enumerate()
            .map(|(k, count)| {
                let (lo, hi) = (self.edges[k], self.edges[k + 1]);
                Bar::new(format!("{lo:.3}"), f64::from(*count)).with_bin(lo, hi)
            })
            .collect()
    }
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

/// Which bin a sample falls in, with the top bin CLOSED at its upper edge.
fn which_bin(sample: f64, min: f64, width: f64, count: u32) -> usize {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the quotient is non-negative here and clamped to the last bin"
    )]
    let slot = ((sample - min) / width).floor() as usize;
    slot.min(count as usize - 1)
}

/// The population standard deviation of an ascending slice.
fn sigma_of(sorted: &[f64]) -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "a sample count as a divisor")]
    let n = sorted.len() as f64;
    let mean = sorted.iter().sum::<f64>() / n;
    (sorted.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n).sqrt()
}

/// The interquartile range of an ascending slice, by linear interpolation.
fn iqr_of(sorted: &[f64]) -> f64 {
    quantile(sorted, 0.75) - quantile(sorted, 0.25)
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "a sample index as a position")]
    let pos = q * (sorted.len() - 1) as f64;
    let lo = pos.floor();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pos is within 0..len-1 by construction"
    )]
    let i = lo as usize;
    let frac = pos - lo;
    if i + 1 >= sorted.len() {
        return sorted[i];
    }
    sorted[i] + (sorted[i + 1] - sorted[i]) * frac
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Twelve samples with one far outlier — the case the two rules disagree on
    /// and the reason the default is the robust one.
    fn skewed() -> Vec<f64> {
        vec![1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 1.9, 2.0, 40.0]
    }

    #[test]
    fn r1793_a_histogram_says_which_rule_chose_its_bins() {
        let binned = Binned::of(&skewed(), BinRule::Scott).expect("spread");
        assert_eq!(binned.rule().name(), "scott");
        assert_eq!(binned.basis().n, 12);
        assert!(binned.basis().iqr > 0.0, "and what it could have read");
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
        ] {
            let binned = Binned::of(&skewed(), rule).expect("spread");
            let total: u32 = binned.counts().iter().sum();
            assert_eq!(total as usize, 12, "{}: {total} of 12 counted", rule.name());
            assert_eq!(binned.edges().len(), binned.bins() + 1);
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
}
