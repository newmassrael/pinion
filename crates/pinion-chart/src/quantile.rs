//! R1797 §5.29 — **an arbitrary quantile, under a method that says so, from a
//! type that holds the invariant the arithmetic needs.**
//!
//! ## How this gap was found
//!
//! The analyzer dashboard's latency card, read off the behaviour reference,
//! puts three numbers above the bars: **p50, p95 and max**. The crate could
//! answer two of them. [`QuantileMethod`] has carried three standard quantile
//! definitions since R1553, and it hard-codes the three proportions a box plot
//! needs — `0.25`, `0.50`, `0.75` — in a **private** method. There was no way
//! to ask for `0.95` at all, from anywhere, under any of the three.
//!
//! ## ★★★★★ The rule of three, and what it was hiding
//!
//! Measured before a line was written, the *same* interpolating quantile was
//! written out **three times** in this crate:
//!
//! * `distribution.rs` — `hf7_depth` + `interpolate_at`, the named methods.
//! * `density.rs` — a private `quantile()`, for Silverman's IQR term.
//! * `histogram.rs` — a private `quantile()`, for Freedman–Diaconis's.
//!
//! Three mechanical copies is this project's immediate-lift threshold, and the
//! lift is not the interesting part. The interesting part is what the copies
//! *disagreed* about: the two private ones are Hyndman & Fan **type 7** with no
//! way to say so, so the bandwidth rule and the bin rule were silently reading
//! a different quartile definition from the box plot drawn beside them
//! whenever that box was built with the default [`QuantileMethod::Tukey`]. The
//! numbers are close and the disagreement is real, and nothing said which was
//! which. Now one type answers, and it carries its method.
//!
//! ## Why a type rather than a function
//!
//! An interpolating quantile is only meaningful on an **ascending, finite**
//! slice, and every one of the three copies took `&[f64]` and trusted its
//! caller. A public function of that shape is a footgun: hand it unsorted
//! samples and it returns a number, quietly wrong. [`Quantiles`] sorts once,
//! drops non-finite samples once, refuses an empty set once, and then cannot be
//! asked a question it would answer wrongly — the same argument
//! [`crate::Distribution`] makes for holding its five landmarks ordered.
//!
//! ## ★★★★★ Tukey does not define p95, and this says so
//!
//! [`QuantileMethod::Tukey`] is the default because it is what Tukey defined
//! the box plot on. A **hinge** is the median of a half, so hinges exist at the
//! quartiles and at the extremes and *nowhere else*: there is no Tukey p95.
//! The tempting shapes are both wrong — silently interpolating (which returns a
//! number the named method did not define) and silently substituting type 7
//! (which returns a number a *different* method defined, under this one's
//! name). [`Quantiles::at`] refuses with [`QuantileError::HingesOnly`], and
//! naming the proportion it could not answer is what lets a caller switch
//! method deliberately.
//!
//! ⚠ **Floor, measured by running it** (R1797, the reference toolkit's charting
//! module at 6.11.1, built from source for this round): it publishes
//! **no quantile of any kind**. Its box set is five positional numbers the
//! caller computed, and its own box-plot example ships a median helper in the
//! *example* for that reason. So this module is not "better than" a floor —
//! there is nothing at that position to be better than, and the comparison it
//! is actually held to is the internal one above.

use crate::distribution::QuantileMethod;

/// Why a quantile could not be answered.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QuantileError {
    /// No sample was finite. A quantile of nothing is not a number, and
    /// returning one would invent it.
    NoFiniteSamples,
    /// The proportion was not in `0.0 ..= 1.0`, or was not finite. Carries what
    /// was asked, because a caller computing a proportion is usually the one
    /// that produced the bad value.
    NotAProportion(f64),
    /// [`QuantileMethod::Tukey`] was asked for a proportion that is not a
    /// hinge. Tukey's hinges are defined at the quartiles and the extremes —
    /// `0.0`, `0.25`, `0.5`, `0.75`, `1.0` — and at no other proportion.
    /// Carries the proportion that has no hinge.
    ///
    /// ★ The repair is to name a method that defines it:
    /// [`QuantileMethod::Linear`] (R, `NumPy`) or
    /// [`QuantileMethod::Exclusive`].
    HingesOnly(f64),
}

impl core::fmt::Display for QuantileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoFiniteSamples => f.write_str("a quantile needs at least one finite sample"),
            Self::NotAProportion(p) => {
                write!(f, "a quantile proportion must be in 0..=1, got {p}")
            }
            Self::HingesOnly(p) => write!(
                f,
                "Tukey's hinges are defined at 0, 0.25, 0.5, 0.75 and 1 only, so there is no \
                 hinge at {p}; name `linear` or `exclusive` to interpolate one"
            ),
        }
    }
}

impl std::error::Error for QuantileError {}

/// The proportions [`QuantileMethod::Tukey`] defines a landmark at.
///
/// Public because a caller choosing a proportion at runtime — from a
/// configuration field, say — needs to be able to ask whether the method it
/// also holds can answer, rather than discovering it from a refusal.
pub const TUKEY_HINGES: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

/// Finite samples, sorted once, answering quantiles under one named method.
///
/// The invariant — ascending and all-finite — is established by the
/// constructor and is why [`Quantiles::at`] can be a plain read.
#[derive(Debug, Clone, PartialEq)]
pub struct Quantiles {
    sorted: Vec<f64>,
    method: QuantileMethod,
}

impl Quantiles {
    /// Sort `samples`, drop the non-finite ones, and answer under `method`.
    ///
    /// A `NaN` is a gap in the record rather than a value on the axis — the
    /// same reading [`crate::Distribution::from_samples`] and
    /// [`crate::density::Density`] take — so it is removed before anything
    /// else and does not count towards [`Quantiles::n`].
    ///
    /// # Errors
    ///
    /// [`QuantileError::NoFiniteSamples`] when nothing survives that filter.
    pub fn of(samples: &[f64], method: QuantileMethod) -> Result<Self, QuantileError> {
        let mut sorted: Vec<f64> = samples.iter().copied().filter(|v| v.is_finite()).collect();
        if sorted.is_empty() {
            return Err(QuantileError::NoFiniteSamples);
        }
        // `total_cmp` rather than `partial_cmp().unwrap()`: the filter above
        // removed every NaN so the two agree, and a sort that cannot panic is
        // the one to write down when the invariant it relies on is a statement
        // away.
        sorted.sort_unstable_by(f64::total_cmp);
        Ok(Self { sorted, method })
    }

    /// Take an already-ascending, already-finite slice.
    ///
    /// Crate-internal: the callers that have one have it because they just did
    /// the filter and the sort themselves, and re-doing both to cross this
    /// boundary would make the shared codec cost more than the copies it
    /// replaced.
    pub(crate) fn from_sorted(sorted: &[f64], method: QuantileMethod) -> Self {
        debug_assert!(
            sorted.windows(2).all(|w| w[0] <= w[1]),
            "from_sorted was handed an unsorted slice"
        );
        Self {
            sorted: sorted.to_vec(),
            method,
        }
    }

    /// The quantile at proportion `p`, under this value's method.
    ///
    /// # Errors
    ///
    /// [`QuantileError::NotAProportion`] when `p` is outside `0.0 ..= 1.0` or
    /// is not finite, and [`QuantileError::HingesOnly`] when the method is
    /// [`QuantileMethod::Tukey`] and `p` is not one of [`TUKEY_HINGES`].
    pub fn at(&self, p: f64) -> Result<f64, QuantileError> {
        if !p.is_finite() || !(0.0..=1.0).contains(&p) {
            return Err(QuantileError::NotAProportion(p));
        }
        match self.method {
            QuantileMethod::Tukey => self.hinge(p),
            QuantileMethod::Linear => Ok(interpolate_at(
                &self.sorted,
                hf7_depth(self.sorted.len(), p),
            )),
            QuantileMethod::Exclusive => Ok(interpolate_at(
                &self.sorted,
                hf6_depth(self.sorted.len(), p),
            )),
        }
    }

    /// Tukey's landmark at `p`, or the refusal that says there is not one.
    ///
    /// The comparison is exact, and deliberately: `0.25` and `1.0 / 4.0` are
    /// the same `f64`, and a tolerance here would answer `0.2500001` with a
    /// hinge — which is precisely the silent substitution this method exists
    /// to refuse.
    #[expect(
        clippy::float_cmp,
        reason = "the exactness IS the contract: a margin here would answer a \
                  proportion Tukey does not define with a hinge, which is the \
                  silent substitution `HingesOnly` exists to refuse"
    )]
    fn hinge(&self, p: f64) -> Result<f64, QuantileError> {
        let (q1, median, q3) = tukey_hinges(&self.sorted);
        if p == 0.0 {
            Ok(self.sorted[0])
        } else if p == 0.25 {
            Ok(q1)
        } else if p == 0.5 {
            Ok(median)
        } else if p == 0.75 {
            Ok(q3)
        } else if p == 1.0 {
            Ok(self.sorted[self.sorted.len() - 1])
        } else {
            Err(QuantileError::HingesOnly(p))
        }
    }

    /// `(q1, median, q3)` under this value's method.
    ///
    /// The box plot's three landmarks, and the one entry point the three
    /// methods share — so a caller cannot reach one of them by a route that
    /// skips the sort or the finiteness filter.
    #[must_use]
    pub fn quartiles(&self) -> (f64, f64, f64) {
        match self.method {
            QuantileMethod::Tukey => tukey_hinges(&self.sorted),
            QuantileMethod::Linear | QuantileMethod::Exclusive => {
                // Every proportion here is a hinge, so the refusal arm is
                // unreachable — but it is answered rather than unwrapped,
                // because a method added later might not define all three and
                // an `expect` here would be the panic nobody predicted.
                let read = |p: f64| self.at(p).unwrap_or(f64::NAN);
                (read(0.25), read(0.5), read(0.75))
            }
        }
    }

    /// The interquartile range under this value's method.
    ///
    /// ★ The number both [`crate::density::Bandwidth::Silverman`] and
    /// [`crate::BinRule::FreedmanDiaconis`] read, from the one place that can
    /// say which definition produced it.
    #[must_use]
    pub fn iqr(&self) -> f64 {
        let (q1, _, q3) = self.quartiles();
        q3 - q1
    }

    /// Which definition answers.
    #[must_use]
    pub const fn method(&self) -> QuantileMethod {
        self.method
    }

    /// How many finite samples are behind the answers.
    #[must_use]
    pub fn n(&self) -> usize {
        self.sorted.len()
    }

    /// The smallest finite sample.
    #[must_use]
    pub fn min(&self) -> f64 {
        self.sorted[0]
    }

    /// The largest finite sample.
    #[must_use]
    pub fn max(&self) -> f64 {
        self.sorted[self.sorted.len() - 1]
    }

    /// The ascending samples themselves.
    #[must_use]
    pub fn sorted(&self) -> &[f64] {
        &self.sorted
    }
}

/// The 0-indexed fractional position of quantile `p` under Hyndman & Fan
/// type 7: `h = (n - 1) * p`.
#[allow(
    clippy::cast_precision_loss,
    reason = "a sample count is a display-scale cardinality; f64 is exact to 2^53"
)]
pub(crate) fn hf7_depth(n: usize, p: f64) -> f64 {
    (n as f64 - 1.0) * p
}

/// The 0-indexed fractional position of quantile `p` under Hyndman & Fan
/// type 6: `h = (n + 1) * p`, one-indexed, hence `- 1.0` here.
#[allow(
    clippy::cast_precision_loss,
    reason = "a sample count is a display-scale cardinality; f64 is exact to 2^53"
)]
pub(crate) fn hf6_depth(n: usize, p: f64) -> f64 {
    (n as f64 + 1.0) * p - 1.0
}

/// `sorted` sampled at the 0-indexed fractional position `depth`, linearly
/// interpolating between the two neighbouring order statistics and clamping
/// to the ends (type 6 runs off both edges at small `n`).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "`depth` is clamped into `0.0 ..= n - 1` immediately above the cast"
)]
pub(crate) fn interpolate_at(sorted: &[f64], depth: f64) -> f64 {
    let last = sorted.len() - 1;
    #[allow(
        clippy::cast_precision_loss,
        reason = "a sample count is a display-scale cardinality; f64 is exact to 2^53"
    )]
    let depth = depth.clamp(0.0, last as f64);
    let floor = depth.floor();
    let lo = floor as usize;
    if lo >= last {
        return sorted[last];
    }
    sorted[lo] + (depth - floor) * (sorted[lo + 1] - sorted[lo])
}

/// Tukey's five-number summary of a non-empty ascending slice, reduced to the
/// three landmarks a box needs — R's `fivenum`.
///
/// The hinge *depth* is `floor((n + 3) / 2) / 2` counted inward from each
/// end; a fractional depth averages the two order statistics it falls
/// between. The median's depth is `(n + 1) / 2`, which is the ordinary
/// median, so all three methods agree there.
#[allow(
    clippy::cast_precision_loss,
    reason = "a sample count is a display-scale cardinality; f64 is exact to 2^53"
)]
pub(crate) fn tukey_hinges(sorted: &[f64]) -> (f64, f64, f64) {
    let n = sorted.len();
    let depth = usize::midpoint(n, 3) as f64 / 2.0;
    let n = n as f64;
    (
        hinge_at(sorted, depth),
        hinge_at(sorted, f64::midpoint(n, 1.0)),
        hinge_at(sorted, n + 1.0 - depth),
    )
}

/// `sorted` at the **1-indexed** depth `d`, averaging the two order
/// statistics a half-integer depth falls between (R's
/// `0.5 * (x[floor(d)] + x[ceiling(d)])`).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "both bounds are clamped into `1 ..= n` before the cast"
)]
pub(crate) fn hinge_at(sorted: &[f64], d: f64) -> f64 {
    #[allow(
        clippy::cast_precision_loss,
        reason = "a sample count is a display-scale cardinality; f64 is exact to 2^53"
    )]
    let n = sorted.len() as f64;
    let lo = d.floor().clamp(1.0, n) as usize;
    let hi = d.ceil().clamp(1.0, n) as usize;
    f64::midpoint(sorted[lo - 1], sorted[hi - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `0.0 ..= 100.0` in unit steps — 101 samples, so every method has an
    /// exact answer to compare and the disagreements below are real rather
    /// than rounding.
    fn ramp() -> Vec<f64> {
        (0..=100).map(f64::from).collect()
    }

    #[test]
    fn r1797_an_arbitrary_proportion_is_answerable_at_last() {
        let q = Quantiles::of(&ramp(), QuantileMethod::Linear).expect("finite");
        // Hyndman & Fan type 7 on 101 points: depth = 100 * p, exactly.
        assert!((q.at(0.95).expect("linear defines p95") - 95.0).abs() < 1e-9);
        assert!((q.at(0.5).expect("and p50") - 50.0).abs() < 1e-9);
        assert_eq!(q.n(), 101);
    }

    #[test]
    fn r1797_tukey_refuses_p95_by_name_rather_than_inventing_one() {
        // ★★★★★ The refusal this module exists for. A hinge is the median of a
        // half; there is no Tukey p95, and both silent repairs are wrong --
        // interpolating returns a number the method did not define, and
        // substituting type 7 returns a number a DIFFERENT method defined
        // under this one's name.
        let q = Quantiles::of(&ramp(), QuantileMethod::Tukey).expect("finite");
        assert_eq!(q.at(0.95), Err(QuantileError::HingesOnly(0.95)));
        assert!(
            format!("{}", QuantileError::HingesOnly(0.95)).contains("linear"),
            "and the refusal names the method that would answer"
        );
        for hinge in TUKEY_HINGES {
            assert!(
                q.at(hinge).is_ok(),
                "but every hinge it DOES define is answerable: {hinge}"
            );
        }
    }

    #[test]
    fn r1797_the_three_methods_disagree_and_each_says_which_it_is() {
        // Six samples is where the definitions come apart -- the case R1553
        // named and could only exercise through a box plot.
        let samples = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let read = |m: QuantileMethod| {
            Quantiles::of(&samples, m)
                .expect("finite")
                .at(0.25)
                .expect("a quartile is a hinge under every method")
        };
        let (tukey, linear, exclusive) = (
            read(QuantileMethod::Tukey),
            read(QuantileMethod::Linear),
            read(QuantileMethod::Exclusive),
        );
        assert!(
            (tukey - linear).abs() > 1e-9,
            "tukey {tukey} vs linear {linear}"
        );
        assert!(
            exclusive < linear,
            "the exclusive definition spreads the quartiles further apart: \
             {exclusive} vs {linear}"
        );
        assert_eq!(
            Quantiles::of(&samples, QuantileMethod::Exclusive)
                .expect("finite")
                .method()
                .name(),
            "exclusive",
            "and the value carries which one answered"
        );
    }

    #[test]
    fn r1797_a_proportion_outside_the_unit_interval_is_refused() {
        let q = Quantiles::of(&ramp(), QuantileMethod::Linear).expect("finite");
        assert_eq!(q.at(1.5), Err(QuantileError::NotAProportion(1.5)));
        assert_eq!(q.at(-0.1), Err(QuantileError::NotAProportion(-0.1)));
        match q.at(f64::NAN) {
            Err(QuantileError::NotAProportion(p)) => assert!(p.is_nan()),
            other => panic!("a NaN proportion must be refused, got {other:?}"),
        }
    }

    #[test]
    fn r1797_construction_sorts_and_drops_the_gaps_in_the_record() {
        // ★ The invariant is the type's, not the caller's: the samples arrive
        // unsorted and with two NaNs, and every answer below is still right.
        let q = Quantiles::of(
            &[5.0, f64::NAN, 1.0, 3.0, f64::INFINITY, 2.0, 4.0],
            QuantileMethod::Linear,
        )
        .expect("five finite");
        assert_eq!(q.n(), 5, "the non-finite samples are not counted");
        assert!((q.min() - 1.0).abs() < 1e-9);
        assert!((q.max() - 5.0).abs() < 1e-9);
        assert!((q.at(0.5).expect("median") - 3.0).abs() < 1e-9);
        assert_eq!(
            Quantiles::of(&[f64::NAN], QuantileMethod::Linear),
            Err(QuantileError::NoFiniteSamples)
        );
    }

    #[test]
    fn r1797_the_iqr_the_two_rules_read_now_says_which_definition_made_it() {
        // ★★★★★ The disagreement the three private copies were hiding. The
        // density bandwidth and the histogram bin rule both read an IQR, both
        // computed it as Hyndman & Fan type 7, and the box plot beside them
        // defaults to Tukey -- so the same samples had two interquartile
        // ranges in one crate and nothing could say so. Here they differ by a
        // measurable amount and each carries its name.
        // Six samples, not nine: at n = 9 the hinge depth is an integer and
        // both definitions land on the same order statistics, so the two IQRs
        // agree exactly and the assertion below would have passed for the wrong
        // reason. Its own first draft did that.
        let samples = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let tukey = Quantiles::of(&samples, QuantileMethod::Tukey).expect("finite");
        let linear = Quantiles::of(&samples, QuantileMethod::Linear).expect("finite");
        assert!(
            (tukey.iqr() - linear.iqr()).abs() > 1e-9,
            "tukey iqr {} vs linear iqr {}",
            tukey.iqr(),
            linear.iqr()
        );
        assert_eq!(tukey.method().name(), "tukey");
        assert_eq!(linear.method().name(), "linear");
    }
}
