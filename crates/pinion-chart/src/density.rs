//! R1626 — a kernel density estimate, and what the smoothing invented.
//!
//! # Why a violin needs its own type
//!
//! A box plot's five numbers are *order statistics*: every one of them is a
//! value the data actually took. A violin's outline is not. It is a smoothed
//! estimate, and three things about it are choices rather than facts — the
//! kernel, the bandwidth, and how wide to draw it beside its neighbours. A
//! reader shown one violin cannot recover any of the three from the picture.
//!
//! So the estimate is a value that carries them: [`Density`] publishes its
//! [`Kernel`], its resolved bandwidth, the sample count behind it, and
//! [`spill`](Density::spill) — the share of the estimated mass that landed
//! **outside the range the samples actually spanned**. That last one is the
//! same question R1625 asked of a spline, in the place it bites hardest: a
//! Gaussian kernel has infinite support, so a violin of a quantity that
//! cannot be negative shows density below zero unless something stops it.
//! [`Density::bounded`] is that something, and it makes `spill` exactly zero
//! by reflecting the kernel at the observed extremes rather than by clipping
//! a picture that was already wrong.
//!
//! The reference toolkit has no violin plot at all, so it has no answer to
//! compare — but its box plot's contract is the relevant precedent: a set of
//! pre-computed five numbers, from which no density can be estimated. That is
//! why [`crate::Distribution`] keeps its density as an **attachment** and a
//! summary-sourced distribution simply does not have one.

/// The kernel a [`Density`] smooths its samples with.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kernel {
    /// The standard normal kernel. Smooth everywhere, and its infinite
    /// support is why [`Density::spill`] is never zero unbounded.
    #[default]
    Gaussian,
    /// The Epanechnikov kernel — compactly supported, so its estimate stops
    /// exactly one bandwidth past the outermost sample. Minimises asymptotic
    /// mean integrated squared error, which is why it is the textbook default
    /// even though the Gaussian is the common one.
    Epanechnikov,
}

impl Kernel {
    /// Every kernel, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 2] = [Self::Gaussian, Self::Epanechnikov];

    /// Stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gaussian => "gaussian",
            Self::Epanechnikov => "epanechnikov",
        }
    }

    /// How many bandwidths past the data the kernel can still reach.
    /// Infinite in principle for the Gaussian; three bandwidths hold
    /// 99.7% of it, which is where the grid stops.
    #[must_use]
    pub const fn support(self) -> f64 {
        match self {
            Self::Gaussian => 3.0,
            Self::Epanechnikov => 1.0,
        }
    }

    /// The kernel's value at `u`, in units of the bandwidth.
    #[must_use]
    pub fn at(self, u: f64) -> f64 {
        match self {
            Self::Gaussian => (-0.5 * u * u).exp() / (2.0 * std::f64::consts::PI).sqrt(),
            Self::Epanechnikov => {
                if u.abs() < 1.0 {
                    0.75 * (1.0 - u * u)
                } else {
                    0.0
                }
            }
        }
    }
}

/// How the bandwidth is chosen.
///
/// It is the one parameter that decides whether a violin shows two modes or
/// one, so it is declared rather than buried: a chart that picked silently
/// would be making the reader's conclusion for them.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Bandwidth {
    /// Silverman's rule of thumb, `0.9 · min(σ, IQR/1.349) · n^(-1/5)`. The
    /// default: taking the smaller of the two spread measures is what stops a
    /// single far outlier from smoothing the whole estimate flat.
    #[default]
    Silverman,
    /// Scott's rule, `1.06 · σ · n^(-1/5)`. Wider than Silverman's on
    /// anything heavy-tailed, for the reason above.
    Scott,
    /// A bandwidth the caller chose. Must be finite and positive.
    Fixed(f64),
}

impl Bandwidth {
    /// Stable name, with the resolved value for the two rules.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Silverman => "silverman",
            Self::Scott => "scott",
            Self::Fixed(_) => "fixed",
        }
    }
}

/// Why a density could not be estimated.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DensityError {
    /// Fewer than two finite samples: there is no spread to smooth.
    TooFewSamples,
    /// Every sample is the same value, so the spread is zero and no rule of
    /// thumb yields a bandwidth. A point mass is not a violin, and drawing
    /// one at an arbitrary width would be inventing the whole shape.
    NoSpread,
    /// A [`Bandwidth::Fixed`] that is not finite and positive.
    BadBandwidth,
    /// A grid resolution below two points.
    BadResolution,
}

impl core::fmt::Display for DensityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::TooFewSamples => "a density needs at least two finite samples",
            Self::NoSpread => "every sample is the same value, so there is no spread to smooth",
            Self::BadBandwidth => "a fixed bandwidth must be finite and positive",
            Self::BadResolution => "a density grid needs at least two points",
        })
    }
}

impl std::error::Error for DensityError {}

/// The default number of points a density grid is evaluated on. 128 is enough
/// that a violin's outline reads as smooth at any plausible slot width, and
/// small enough that a chart of twenty categories stays cheap.
pub const DEFAULT_RESOLUTION: usize = 128;

/// A kernel density estimate over one sample set.
#[derive(Debug, Clone, PartialEq)]
pub struct Density {
    grid: Vec<(f64, f64)>,
    bandwidth: f64,
    kernel: Kernel,
    rule: Bandwidth,
    count: usize,
    observed: (f64, f64),
    spill: f64,
    bounded: bool,
}

impl Density {
    /// Estimate the density of `samples`.
    ///
    /// Non-finite samples are dropped before anything else, the way
    /// [`crate::Distribution::from_samples`] drops them — a `NaN` is a gap in
    /// the record, not a value at the middle of the axis.
    ///
    /// # Errors
    ///
    /// [`DensityError`] when the samples cannot support an estimate: fewer
    /// than two of them, no spread at all, a non-positive fixed bandwidth, or
    /// a grid of fewer than two points.
    pub fn from_samples(
        samples: &[f64],
        kernel: Kernel,
        rule: Bandwidth,
        resolution: usize,
    ) -> Result<Self, DensityError> {
        if resolution < 2 {
            return Err(DensityError::BadResolution);
        }
        let mut finite: Vec<f64> = samples.iter().copied().filter(|v| v.is_finite()).collect();
        if finite.len() < 2 {
            return Err(DensityError::TooFewSamples);
        }
        finite.sort_by(f64::total_cmp);
        let lo = finite[0];
        let hi = finite[finite.len() - 1];
        if (hi - lo).abs() < f64::EPSILON {
            return Err(DensityError::NoSpread);
        }
        let bandwidth = resolve_bandwidth(&finite, rule)?;

        let reach = kernel.support() * bandwidth;
        let (from, to) = (lo - reach, hi + reach);
        let grid = evaluate(&finite, kernel, bandwidth, from, to, resolution, None);
        let spill = mass_outside(&grid, lo, hi);
        Ok(Self {
            grid,
            bandwidth,
            kernel,
            rule,
            count: finite.len(),
            observed: (lo, hi),
            spill,
            bounded: false,
        })
    }

    /// Re-estimate with the kernel **reflected** at the observed extremes, so
    /// the density is exactly zero outside the range the samples spanned.
    ///
    /// The answer for a bounded quantity — a duration, a queue depth, a byte
    /// count — where the unbounded estimate puts mass below zero and a reader
    /// has no way to tell that from a measurement. Reflection is the
    /// principled form: clipping the picture would leave the visible mass
    /// short of one, while reflection folds the escaped mass back where it
    /// came from, so the outline still integrates to one and
    /// [`spill`](Self::spill) becomes zero rather than merely hidden.
    ///
    /// # Errors
    ///
    /// Only the errors [`from_samples`](Self::from_samples) can raise; a
    /// density that exists can always be bounded.
    pub fn bounded(&self, samples: &[f64]) -> Result<Self, DensityError> {
        let finite: Vec<f64> = samples.iter().copied().filter(|v| v.is_finite()).collect();
        if finite.len() < 2 {
            return Err(DensityError::TooFewSamples);
        }
        let (lo, hi) = self.observed;
        let grid = evaluate(
            &finite,
            self.kernel,
            self.bandwidth,
            lo,
            hi,
            self.grid.len(),
            Some((lo, hi)),
        );
        Ok(Self {
            grid,
            bandwidth: self.bandwidth,
            kernel: self.kernel,
            rule: self.rule,
            count: self.count,
            observed: self.observed,
            spill: 0.0,
            bounded: true,
        })
    }

    /// The estimate, as `(value, density)` in ascending value order.
    #[must_use]
    pub fn grid(&self) -> &[(f64, f64)] {
        &self.grid
    }

    /// The bandwidth actually used, whatever chose it.
    #[must_use]
    pub const fn bandwidth(&self) -> f64 {
        self.bandwidth
    }

    /// Which kernel smoothed the samples.
    #[must_use]
    pub const fn kernel(&self) -> Kernel {
        self.kernel
    }

    /// Which rule chose the bandwidth.
    #[must_use]
    pub const fn rule(&self) -> Bandwidth {
        self.rule
    }

    /// How many finite samples are behind the estimate.
    ///
    /// The number a reader needs and a violin's outline hides: five samples
    /// and five thousand draw the same confident shape.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.count
    }

    /// The range the samples actually spanned, `(min, max)`.
    #[must_use]
    pub const fn observed(&self) -> (f64, f64) {
        self.observed
    }

    /// The share of the estimated mass lying outside
    /// [`observed`](Self::observed) — what the smoothing invented.
    ///
    /// Never zero for an unbounded [`Kernel::Gaussian`] estimate, and exactly
    /// zero for a [`bounded`](Self::bounded) one.
    #[must_use]
    pub const fn spill(&self) -> f64 {
        self.spill
    }

    /// Whether this estimate was reflected at the observed extremes.
    #[must_use]
    pub const fn is_bounded(&self) -> bool {
        self.bounded
    }

    /// The largest density on the grid — the width a violin is scaled by.
    #[must_use]
    pub fn peak(&self) -> f64 {
        self.grid.iter().map(|&(_, d)| d).fold(0.0, f64::max)
    }

    /// The value range the estimate covers, which is wider than
    /// [`observed`](Self::observed) unless it is bounded.
    #[must_use]
    pub fn extent(&self) -> (f64, f64) {
        match (self.grid.first(), self.grid.last()) {
            (Some(&(lo, _)), Some(&(hi, _))) => (lo, hi),
            _ => self.observed,
        }
    }
}

/// A count as an `f64` weight. `pub(crate)` because the violin's count
/// scaling weighs the same counts this module resolves its bandwidth from,
/// and two spellings of that narrowing would be two places to get it wrong.
#[allow(
    clippy::cast_precision_loss,
    reason = "a sample count is a magnitude here, not an identity; f64 holds every count that fits in memory exactly up to 2^53"
)]
pub(crate) fn count_as_f64(n: usize) -> f64 {
    n as f64
}

/// Resolve `rule` against sorted finite `samples`.
fn resolve_bandwidth(sorted: &[f64], rule: Bandwidth) -> Result<f64, DensityError> {
    if let Bandwidth::Fixed(h) = rule {
        return if h.is_finite() && h > 0.0 {
            Ok(h)
        } else {
            Err(DensityError::BadBandwidth)
        };
    }
    let n = count_as_f64(sorted.len());
    let mean = sorted.iter().sum::<f64>() / n;
    let variance = sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let sigma = variance.max(0.0).sqrt();
    // Silverman takes the SMALLER of the two spread measures, which is what
    // stops one far outlier from smoothing the estimate flat; Scott takes the
    // standard deviation alone and is therefore wider on a heavy tail.
    let scott = rule == Bandwidth::Scott;
    let spread = if scott {
        sigma
    } else {
        let iqr = quantile(sorted, 0.75) - quantile(sorted, 0.25);
        let robust = iqr / 1.349;
        if robust > 0.0 {
            sigma.min(robust)
        } else {
            sigma
        }
    };
    let factor = if scott { 1.06 } else { 0.9 };
    let h = factor * spread * n.powf(-0.2);
    if h.is_finite() && h > 0.0 {
        Ok(h)
    } else {
        Err(DensityError::NoSpread)
    }
}

/// Linear-interpolating quantile over a sorted slice — the same reading
/// [`crate::QuantileMethod::Linear`] uses, kept local because the bandwidth
/// rule needs an IQR whatever quantile definition the box plot was built with.
fn quantile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let pos = p * count_as_f64(n - 1);
    let lo = pos.floor();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pos is in [0, n-1] by construction"
    )]
    let i = lo as usize;
    let frac = pos - lo;
    if i + 1 >= n {
        sorted[n - 1]
    } else {
        sorted[i] + frac * (sorted[i + 1] - sorted[i])
    }
}

/// Evaluate the estimate on `resolution` points spanning `[from, to]`.
///
/// `reflect` folds the kernel back at the given bounds — the mass that would
/// have escaped is added at its mirror image, which is what makes a bounded
/// estimate still integrate to one.
fn evaluate(
    samples: &[f64],
    kernel: Kernel,
    bandwidth: f64,
    from: f64,
    to: f64,
    resolution: usize,
    reflect: Option<(f64, f64)>,
) -> Vec<(f64, f64)> {
    let n = count_as_f64(samples.len());
    let step = (to - from) / count_as_f64(resolution - 1);
    (0..resolution)
        .map(|k| {
            let x = count_as_f64(k).mul_add(step, from);
            let mut sum = 0.0;
            for &s in samples {
                sum += kernel.at((x - s) / bandwidth);
                if let Some((lo, hi)) = reflect {
                    sum += kernel.at((x - (2.0f64.mul_add(lo, -s))) / bandwidth);
                    sum += kernel.at((x - (2.0f64.mul_add(hi, -s))) / bandwidth);
                }
            }
            (x, sum / (n * bandwidth))
        })
        .collect()
}

/// Trapezoidal mass of `grid` outside `[lo, hi]`.
fn mass_outside(grid: &[(f64, f64)], lo: f64, hi: f64) -> f64 {
    let mut outside = 0.0;
    let mut total = 0.0;
    for pair in grid.windows(2) {
        let (x0, d0) = pair[0];
        let (x1, d1) = pair[1];
        let area = (x1 - x0) * (d0 + d1) / 2.0;
        total += area;
        // A cell straddling a bound contributes its outside share, linearly.
        let width = x1 - x0;
        if width <= 0.0 {
            continue;
        }
        let out_lo = ((lo - x0).max(0.0)).min(width);
        let out_hi = ((x1 - hi).max(0.0)).min(width);
        outside += area * ((out_lo + out_hi) / width).min(1.0);
    }
    if total > 0.0 { outside / total } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spread(n: usize) -> Vec<f64> {
        // A deterministic bimodal set: two clusters, so a bandwidth that is
        // too wide visibly merges them.
        let mut s = 12_345u64;
        (0..n)
            .map(|i| {
                s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let jitter = f64::from(u32::try_from((s >> 40) % 100).expect("below 100")) / 100.0;
                if i % 2 == 0 {
                    10.0 + jitter
                } else {
                    30.0 + jitter
                }
            })
            .collect()
    }

    fn integral(d: &Density) -> f64 {
        d.grid()
            .windows(2)
            .map(|w| (w[1].0 - w[0].0) * (w[0].1 + w[1].1) / 2.0)
            .sum()
    }

    #[test]
    fn a_density_integrates_to_one() {
        for kernel in Kernel::ALL {
            let d = Density::from_samples(&spread(200), kernel, Bandwidth::Silverman, 512)
                .expect("estimable");
            assert!(
                (integral(&d) - 1.0).abs() < 0.02,
                "{kernel:?} integrates to {}",
                integral(&d),
            );
        }
    }

    /// ★ The report this module exists for: an unbounded Gaussian estimate
    /// ALWAYS puts mass where no sample was, and says how much.
    #[test]
    fn an_unbounded_gaussian_estimate_always_spills() {
        let samples = spread(200);
        let d = Density::from_samples(&samples, Kernel::Gaussian, Bandwidth::Silverman, 256)
            .expect("estimable");
        assert!(d.spill() > 0.0, "spill is {}", d.spill());
        assert!(!d.is_bounded());
        let (lo, hi) = d.observed();
        let (elo, ehi) = d.extent();
        assert!(elo < lo && ehi > hi, "the estimate reaches past the data");
    }

    /// ★ And the bounded form makes it exactly zero — by reflection, so the
    /// outline still integrates to one rather than being clipped short.
    #[test]
    fn a_bounded_estimate_stops_at_the_data_and_keeps_its_mass() {
        let samples = spread(200);
        let raw = Density::from_samples(&samples, Kernel::Gaussian, Bandwidth::Silverman, 512)
            .expect("estimable");
        let bounded = raw.bounded(&samples).expect("boundable");
        assert!(
            bounded.spill().abs() < f64::EPSILON,
            "exactly zero, not merely small"
        );
        assert!(bounded.is_bounded());
        assert_eq!(bounded.extent(), bounded.observed(), "it stops at the data");
        assert!(
            (integral(&bounded) - 1.0).abs() < 0.02,
            "reflection keeps the mass: {}",
            integral(&bounded),
        );
        // Reflection RAISES the density near the edges, which is the whole
        // difference from clipping.
        let edge = |d: &Density| d.grid().first().map_or(0.0, |&(_, v)| v);
        let raw_at_lo = raw
            .grid()
            .iter()
            .find(|&&(x, _)| x >= raw.observed().0)
            .map_or(0.0, |&(_, v)| v);
        assert!(
            edge(&bounded) > raw_at_lo,
            "bounded {} vs unbounded {raw_at_lo}",
            edge(&bounded),
        );
    }

    /// The compact kernel stops exactly one bandwidth past the data, which is
    /// the property that distinguishes it.
    #[test]
    fn the_compact_kernel_stops_one_bandwidth_past_the_data() {
        let samples = spread(200);
        let d = Density::from_samples(&samples, Kernel::Epanechnikov, Bandwidth::Silverman, 256)
            .expect("estimable");
        let (lo, hi) = d.observed();
        let (elo, ehi) = d.extent();
        assert!((elo - (lo - d.bandwidth())).abs() < 1e-9, "{elo} vs {lo}");
        assert!((ehi - (hi + d.bandwidth())).abs() < 1e-9, "{ehi} vs {hi}");
        assert!(d.spill() > 0.0, "it still reaches past the data");
        assert!(
            d.spill() < 0.5,
            "but far less than half the mass: {}",
            d.spill(),
        );
    }

    /// ★ Silverman's rule is ROBUST, and that is a stronger claim than
    /// "narrower than Scott's".
    ///
    /// FOUND BY A COUNTERFACTUAL: the first version of this test compared the
    /// two rules on outlier-laden data, and dropping the robust spread
    /// entirely still passed it — because the two rules also differ by a
    /// constant factor (0.9 against 1.06), so Silverman stayed narrower while
    /// having become exactly as fragile. The property is how little the
    /// bandwidth MOVES when one far outlier arrives.
    #[test]
    fn silvermans_bandwidth_barely_moves_when_one_outlier_arrives() {
        let clean = spread(200);
        let mut polluted = clean.clone();
        polluted.push(5_000.0);
        let h = |samples: &[f64], rule| {
            Density::from_samples(samples, Kernel::Gaussian, rule, 64)
                .expect("estimable")
                .bandwidth()
        };
        let sil_ratio = h(&polluted, Bandwidth::Silverman) / h(&clean, Bandwidth::Silverman);
        let scott_ratio = h(&polluted, Bandwidth::Scott) / h(&clean, Bandwidth::Scott);
        // Measured on this fixture: 1.48 against 35.0. The thresholds sit
        // either side of that gap rather than at a guessed round number,
        // because the fixture is bimodal and its IQR does move a little when
        // a 201st sample shifts the quantile positions — "robust" is a claim
        // about the ORDER of the two, not about Silverman holding perfectly
        // still.
        assert!(
            sil_ratio < 3.0,
            "one outlier barely moves silverman: ratio {sil_ratio}",
        );
        assert!(
            scott_ratio > 10.0,
            "and blows scott up, which is what robust MEANS here: ratio {scott_ratio}",
        );
        assert!(
            scott_ratio > sil_ratio * 5.0,
            "the gap is an order of magnitude: {sil_ratio} vs {scott_ratio}",
        );
        assert!(
            h(&polluted, Bandwidth::Silverman) < h(&polluted, Bandwidth::Scott),
            "so silverman stays the narrower of the two",
        );
        assert_eq!(
            Density::from_samples(&clean, Kernel::Gaussian, Bandwidth::Silverman, 64)
                .expect("estimable")
                .rule(),
            Bandwidth::Silverman,
        );
    }

    #[test]
    fn a_wider_bandwidth_merges_the_two_modes() {
        let samples = spread(200);
        let modes = |h: f64| {
            let d = Density::from_samples(&samples, Kernel::Gaussian, Bandwidth::Fixed(h), 512)
                .expect("estimable");
            d.grid()
                .windows(3)
                .filter(|w| w[1].1 > w[0].1 && w[1].1 > w[2].1)
                .count()
        };
        assert_eq!(
            modes(0.6),
            2,
            "a narrow kernel keeps the two clusters apart"
        );
        assert_eq!(modes(20.0), 1, "a wide one merges them");
    }

    #[test]
    fn an_inestimable_sample_set_is_refused_by_name() {
        assert_eq!(
            Density::from_samples(&[1.0], Kernel::Gaussian, Bandwidth::Silverman, 64),
            Err(DensityError::TooFewSamples),
        );
        assert_eq!(
            Density::from_samples(&[f64::NAN, 2.0], Kernel::Gaussian, Bandwidth::Silverman, 64),
            Err(DensityError::TooFewSamples),
            "a NaN is a gap in the record, not a value",
        );
        assert_eq!(
            Density::from_samples(&[3.0, 3.0, 3.0], Kernel::Gaussian, Bandwidth::Silverman, 64),
            Err(DensityError::NoSpread),
        );
        assert_eq!(
            Density::from_samples(&spread(20), Kernel::Gaussian, Bandwidth::Fixed(-1.0), 64),
            Err(DensityError::BadBandwidth),
        );
        assert_eq!(
            Density::from_samples(&spread(20), Kernel::Gaussian, Bandwidth::Silverman, 1),
            Err(DensityError::BadResolution),
        );
        assert!(DensityError::NoSpread.to_string().contains("spread"));
    }

    #[test]
    fn the_estimate_publishes_what_produced_it() {
        let samples = spread(50);
        let d = Density::from_samples(&samples, Kernel::Epanechnikov, Bandwidth::Scott, 64)
            .expect("estimable");
        assert_eq!(d.count(), samples.len());
        assert_eq!(d.kernel(), Kernel::Epanechnikov);
        assert_eq!(d.rule(), Bandwidth::Scott);
        assert!(d.bandwidth() > 0.0);
        assert_eq!(d.grid().len(), 64);
        assert!(d.peak() > 0.0);
        // The grid is ascending, which every consumer walking it assumes.
        assert!(d.grid().windows(2).all(|w| w[1].0 > w[0].0));
    }

    #[test]
    fn every_kernel_has_a_distinct_name_and_a_finite_support() {
        let mut names: Vec<&str> = Kernel::ALL.iter().map(|k| k.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Kernel::ALL.len());
        for k in Kernel::ALL {
            assert!(k.support() > 0.0 && k.support().is_finite());
            assert!(k.at(0.0) > 0.0, "{k:?} has mass at its centre");
            assert!(k.at(0.0) >= k.at(0.5), "{k:?} peaks at its centre");
        }
        assert!(
            Kernel::Epanechnikov.at(1.5).abs() < f64::EPSILON,
            "the compact kernel is exactly zero past its support",
        );
    }
}
