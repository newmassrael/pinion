//! Value-to-pixel mapping — the arithmetic core every axis and series
//! share.
//!
//! A [`LinearScale`] maps a data `domain` (`f64` value range) onto a
//! pixel `range` (`f32` device coordinates) with an affine transform,
//! and inverts it. The range endpoints may be given in either order, so
//! a y-axis simply passes `(bottom_px, top_px)` — a descending pixel
//! range — to get the screen-space "larger value sits higher" mapping
//! without any special-casing at the call site.
//!
//! # Two layers (R1528)
//!
//! [`LinearScale`] is the **arithmetic**: a total affine transform, used
//! directly by the charts whose axis is linear by construction (the
//! categorical [`bar`](crate::BarChart) slot metric, the axis-less
//! [`Sparkline`](crate::Sparkline), the [`Timeline`](crate::Timeline)
//! ruler).
//!
//! [`ValueScale`] is the **axis** — Qt's `QAbstractAxis` distinction. It
//! is what a numeric-x / numeric-y chart plots through, and it is the
//! layer that knows a mapping can be *undefined*: a logarithmic axis has
//! no pixel for zero or for a negative value. So [`ValueScale::map`]
//! returns `Option<f32>` where [`LinearScale::map`] returns `f32`, and
//! every call site has to decide what a point with no pixel looks like
//! (the line chart breaks its polyline; the scatter chart omits the
//! mark). That is the whole reason the two layers are separate — a
//! partial map is the wrong contract for the affine core, and the right
//! one for an axis.
//!
//! # An axis kind is more than its arithmetic (R1529)
//!
//! The time axis is what shows this. It is *affine* — equal durations
//! occupy equal pixel spans — so it reuses [`LinearScale`] with no
//! transform at all, and if an axis were only its arithmetic there would
//! be nothing to add. What makes it a distinct [`ValueScale`] arm is that
//! an axis also decides **which ticks it lands on** and **how they are
//! labelled**, and on those a time axis differs from a linear one
//! completely (see [`crate::ticks`]). Carrying the kind in the scale is
//! what keeps a tick set and the mapping that places it from being
//! resolved off different axes.
//!
//! # The log scale is the linear scale (d3's `transformLog`)
//!
//! [`LogScale`] holds a [`LinearScale`] over the *log-transformed*
//! domain and composes: `map(v) = linear.map(log_b(v))`, `invert(px) =
//! b^linear.invert(px)`. This is how d3 builds `scaleLog` — a continuous
//! scale plus a transform pair — and it means the log axis inherits the
//! degenerate-domain and descending-range handling already proved here
//! rather than restating that arithmetic.

/// Affine `f64` domain to `f32` pixel-range mapping (and its inverse).
///
/// The transform is `pixel = range_lo + t * (range_hi - range_lo)` where
/// `t = (value - domain_lo) / (domain_hi - domain_lo)`. A degenerate
/// domain (`domain_lo == domain_hi`) maps every value to the pixel-range
/// midpoint rather than dividing by zero; a degenerate pixel range makes
/// [`LinearScale::invert`] return `domain_lo`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearScale {
    domain_lo: f64,
    domain_hi: f64,
    range_lo: f32,
    range_hi: f32,
}

impl LinearScale {
    /// Build a scale from a data `domain` (`(lo, hi)` values) onto a
    /// pixel `range` (`(lo, hi)` device coordinates). The range may be
    /// descending (`lo > hi`) for a y-axis.
    #[must_use]
    pub const fn new(domain: (f64, f64), range: (f32, f32)) -> Self {
        Self {
            domain_lo: domain.0,
            domain_hi: domain.1,
            range_lo: range.0,
            range_hi: range.1,
        }
    }

    /// Map a data `value` to its pixel coordinate. A degenerate domain
    /// returns the pixel-range midpoint.
    #[must_use]
    pub fn map(&self, value: f64) -> f32 {
        let span = self.domain_hi - self.domain_lo;
        let t = if span.abs() < f64::EPSILON {
            0.5
        } else {
            (value - self.domain_lo) / span
        };
        let lo = f64::from(self.range_lo);
        let hi = f64::from(self.range_hi);
        to_f32(lo + t * (hi - lo))
    }

    /// Invert a pixel coordinate back to a data value. A degenerate
    /// pixel range returns `domain_lo`.
    #[must_use]
    pub fn invert(&self, pixel: f32) -> f64 {
        let span = f64::from(self.range_hi) - f64::from(self.range_lo);
        if span.abs() < f64::EPSILON {
            return self.domain_lo;
        }
        let t = (f64::from(pixel) - f64::from(self.range_lo)) / span;
        self.domain_lo + t * (self.domain_hi - self.domain_lo)
    }

    /// The data domain `(lo, hi)` this scale was built with.
    #[must_use]
    pub const fn domain(&self) -> (f64, f64) {
        (self.domain_lo, self.domain_hi)
    }

    /// The pixel range `(lo, hi)` this scale was built with.
    #[must_use]
    pub const fn range(&self) -> (f32, f32) {
        (self.range_lo, self.range_hi)
    }
}

/// The default logarithm base — decades, what every log axis in a
/// monitoring or profiling tool uses (Qt's `QLogValueAxis::base` default).
pub const DEFAULT_LOG_BASE: f64 = 10.0;

/// Logarithmic `f64` domain to `f32` pixel-range mapping (and its inverse).
///
/// Built as a [`LinearScale`] over the log-transformed domain, so
/// `map(v) = linear.map(log_b(v))` and `invert(px) = b^linear.invert(px)`
/// — d3's `scaleLog` composition. Equal *ratios* therefore occupy equal
/// pixel distances, which is the whole point: a latency series spanning
/// `0.1 ms .. 1000 ms` shows its sub-millisecond structure instead of
/// collapsing onto the baseline.
///
/// # Partial by nature
///
/// `log_b(v)` is undefined for `v <= 0`, so [`LogScale::map`] returns
/// `None` there rather than inventing a pixel. Callers reach this through
/// [`ValueScale`], whose contract makes the possibility explicit at every
/// use site.
///
/// # Total construction
///
/// The domain must be strictly positive and the base greater than 1. Both
/// are *sanitised* rather than rejected, so the type has no failing
/// constructor: an out-of-contract base falls back to
/// [`DEFAULT_LOG_BASE`], and a non-positive or non-finite domain endpoint
/// falls back to the unit decade `(1, base)`. [`LogScale::domain`] then
/// reports the domain actually in use, so a chart built on a sanitised
/// scale is inspectable rather than silently wrong. The one production
/// caller (`crate::plot`) derives its domain from the positive data, so
/// it does not rely on the fallback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogScale {
    /// The affine scale over `(log_b(domain_lo), log_b(domain_hi))`.
    inner: LinearScale,
    base: f64,
    domain_lo: f64,
    domain_hi: f64,
}

impl LogScale {
    /// Build a log scale over a strictly-positive data `domain` onto a
    /// pixel `range` (descending for a y-axis, exactly as
    /// [`LinearScale::new`]). See the type doc for how an out-of-contract
    /// `base` or `domain` is sanitised.
    #[must_use]
    pub fn new(domain: (f64, f64), range: (f32, f32), base: f64) -> Self {
        let base = if base.is_finite() && base > 1.0 {
            base
        } else {
            DEFAULT_LOG_BASE
        };
        let (lo, hi) = if positive(domain.0) && positive(domain.1) {
            domain
        } else {
            (1.0, base)
        };
        Self {
            inner: LinearScale::new((log_base(lo, base), log_base(hi, base)), range),
            base,
            domain_lo: lo,
            domain_hi: hi,
        }
    }

    /// Map a positive data `value` to its pixel coordinate, or `None` when
    /// the logarithm is undefined there (`value <= 0`, or non-finite).
    ///
    /// Values outside the domain still map, by extrapolation, exactly as
    /// [`LinearScale::map`] does — clipping to the visible range is the
    /// chart's decision, not the scale's.
    #[must_use]
    pub fn map(&self, value: f64) -> Option<f32> {
        positive(value).then(|| self.inner.map(log_base(value, self.base)))
    }

    /// Invert a pixel coordinate back to a data value.
    #[must_use]
    pub fn invert(&self, pixel: f32) -> f64 {
        self.base.powf(self.inner.invert(pixel))
    }

    /// The data domain `(lo, hi)` in use — the constructed one, or the
    /// sanitised fallback described on the type.
    #[must_use]
    pub const fn domain(&self) -> (f64, f64) {
        (self.domain_lo, self.domain_hi)
    }

    /// The pixel range `(lo, hi)` this scale was built with.
    #[must_use]
    pub const fn range(&self) -> (f32, f32) {
        self.inner.range()
    }

    /// The logarithm base in use.
    #[must_use]
    pub const fn base(&self) -> f64 {
        self.base
    }
}

/// Which KIND one chart axis is — Qt's `QValueAxis` / `QLogValueAxis` /
/// `QDateTimeAxis` choice, without a domain or a pixel range attached.
///
/// A chart knows its axis kinds before a plot is resolved (it needs them
/// to report its off-scale points), and everything that depends only on
/// the kind — definedness, which tick generator runs, which label format
/// applies — is decided from here so those rules have one spelling.
///
/// R1529 made this a type. It was a `bool` (`is_log`) plus an
/// `Option<f64>` base while there were two kinds, which is what a boolean
/// discriminator always is: an encoding that fits exactly two arms. The
/// third arm is where it stops fitting — `is_log == false` would have had
/// to mean "linear or time", and every site that branched on it would
/// have silently taken the linear path for a time axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AxisKind {
    /// An affine axis over a plain number.
    #[default]
    Linear,
    /// A logarithmic axis in the given base.
    Log(f64),
    /// A time axis over epoch **milliseconds**, UTC — the unit Qt's
    /// `QDateTimeAxis` and d3's `scaleUtc` both carry. Affine in the
    /// instant, so it maps through the same [`LinearScale`] arithmetic a
    /// [`Linear`](Self::Linear) axis does; what differs is entirely which
    /// ticks it lands on and how they are labelled.
    Time,
}

impl AxisKind {
    /// Whether an axis of this kind defines a pixel for `value`.
    #[must_use]
    pub const fn defines(self, value: f64) -> bool {
        match self {
            Self::Log(_) => positive(value),
            Self::Linear | Self::Time => value.is_finite(),
        }
    }

    /// The logarithm base, for the one kind that has one.
    #[must_use]
    pub const fn log_base(self) -> Option<f64> {
        match self {
            Self::Log(base) => Some(base),
            _ => None,
        }
    }
}

/// The scale of one chart axis: linear, logarithmic, or time.
///
/// This is the axis-level abstraction (Qt's `QAbstractAxis` distinction),
/// distinct from the [`LinearScale`] arithmetic it is built on — see the
/// module doc. A closed enum rather than a trait object because the set of
/// axis scales is closed, and because staying `Copy` keeps
/// `crate::plot::CartesianPlot` a plain value.
///
/// [`map`](Self::map) is **partial**: a log axis has no pixel for a
/// non-positive value, and a linear or time axis none for a non-finite
/// one. There is deliberately no total variant — a caller that silently
/// substituted a pixel would be drawing a point the data does not contain,
/// which is the failure [`crate::Mapped::unreadable`] exists to avoid on
/// the other input path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueScale {
    /// An affine axis — equal value differences occupy equal pixel spans.
    Linear(LinearScale),
    /// A logarithmic axis — equal value *ratios* occupy equal pixel spans.
    Log(LogScale),
    /// A time axis over epoch milliseconds (R1529). Carries a
    /// [`LinearScale`] because time IS affine — equal *durations* occupy
    /// equal pixel spans — so unlike [`Log`](Self::Log) there is no
    /// transform to compose. That the arithmetic core is reused untouched
    /// by a third axis kind is the two-layer split of R1528 paying off.
    Time(LinearScale),
}

impl ValueScale {
    /// Map a data `value` to its pixel coordinate, or `None` when this
    /// axis does not define one there.
    #[must_use]
    pub fn map(&self, value: f64) -> Option<f32> {
        match self {
            Self::Linear(s) | Self::Time(s) => value.is_finite().then(|| s.map(value)),
            Self::Log(s) => s.map(value),
        }
    }

    /// Invert a pixel coordinate back to a data value.
    #[must_use]
    pub fn invert(&self, pixel: f32) -> f64 {
        match self {
            Self::Linear(s) | Self::Time(s) => s.invert(pixel),
            Self::Log(s) => s.invert(pixel),
        }
    }

    /// Whether this axis defines a pixel for `value` — the predicate form
    /// of [`map`](Self::map), for the call sites that partition data
    /// before mapping it.
    #[must_use]
    pub const fn defines(&self, value: f64) -> bool {
        self.kind().defines(value)
    }

    /// The data domain `(lo, hi)` of this axis.
    #[must_use]
    pub const fn domain(&self) -> (f64, f64) {
        match self {
            Self::Linear(s) | Self::Time(s) => s.domain(),
            Self::Log(s) => s.domain(),
        }
    }

    /// The pixel range `(lo, hi)` of this axis.
    #[must_use]
    pub const fn range(&self) -> (f32, f32) {
        match self {
            Self::Linear(s) | Self::Time(s) => s.range(),
            Self::Log(s) => s.range(),
        }
    }

    /// Which [`AxisKind`] this scale is — what the tick generator, the
    /// domain snapper and the label format branch on.
    #[must_use]
    pub const fn kind(&self) -> AxisKind {
        match self {
            Self::Linear(_) => AxisKind::Linear,
            Self::Log(s) => AxisKind::Log(s.base()),
            Self::Time(_) => AxisKind::Time,
        }
    }
}

/// Whether `v` is a value a logarithm is defined at: finite and `> 0`.
pub(crate) const fn positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// `log_base(v, b)` — the logarithm of `v` in base `b`. Callers guarantee
/// `v > 0` and `b > 1`.
pub(crate) fn log_base(v: f64, base: f64) -> f64 {
    v.ln() / base.ln()
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "f64 pixel arithmetic narrowed to the f32 PathPoint coordinate space; sub-pixel loss is expected and bounded by the device resolution"
)]
fn to_f32(v: f64) -> f32 {
    v as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }
    fn close64(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn maps_endpoints_and_midpoint() {
        let s = LinearScale::new((0.0, 100.0), (0.0, 200.0));
        assert!(close(s.map(0.0), 0.0));
        assert!(close(s.map(100.0), 200.0));
        assert!(close(s.map(50.0), 100.0));
    }

    #[test]
    fn descending_range_puts_larger_values_higher() {
        // y-axis: value 0 -> bottom (300px), value 10 -> top (20px).
        let s = LinearScale::new((0.0, 10.0), (300.0, 20.0));
        assert!(close(s.map(0.0), 300.0));
        assert!(close(s.map(10.0), 20.0));
        assert!(
            s.map(10.0) < s.map(0.0),
            "larger value maps higher (smaller px)"
        );
    }

    #[test]
    fn invert_round_trips() {
        let s = LinearScale::new((-5.0, 5.0), (10.0, 410.0));
        for v in [-5.0, -1.0, 0.0, 2.5, 5.0] {
            let px = s.map(v);
            assert!(close64(s.invert(px), v), "round-trip {v}");
        }
    }

    #[test]
    fn degenerate_domain_maps_to_range_midpoint() {
        let s = LinearScale::new((7.0, 7.0), (0.0, 100.0));
        assert!(close(s.map(7.0), 50.0));
        assert!(close(s.map(999.0), 50.0));
    }

    #[test]
    fn degenerate_range_inverts_to_domain_lo() {
        let s = LinearScale::new((3.0, 9.0), (50.0, 50.0));
        assert!(close64(s.invert(50.0), 3.0));
    }

    #[test]
    fn accessors_echo_construction() {
        let s = LinearScale::new((1.0, 2.0), (3.0, 4.0));
        assert_eq!(s.domain(), (1.0, 2.0));
        assert_eq!(s.range(), (3.0, 4.0));
    }

    // ---- R1528: the logarithmic axis -----------------------------------

    #[test]
    fn r1528_equal_ratios_occupy_equal_pixel_spans() {
        // Three decades over 300px: each decade is exactly 100px, which is
        // the defining property a linear axis cannot have.
        let s = LogScale::new((1.0, 1000.0), (0.0, 300.0), DEFAULT_LOG_BASE);
        assert!(close(s.map(1.0).unwrap(), 0.0));
        assert!(close(s.map(10.0).unwrap(), 100.0));
        assert!(close(s.map(100.0).unwrap(), 200.0));
        assert!(close(s.map(1000.0).unwrap(), 300.0));
        // The same RATIO anywhere in the domain spans the same pixels.
        let decade = s.map(20.0).unwrap() - s.map(2.0).unwrap();
        assert!(close(decade, 100.0), "2->20 spans a decade like 1->10");
    }

    #[test]
    fn r1528_descending_range_works_for_a_y_axis() {
        // y-axis: domain-lo -> bottom (300px), domain-hi -> top (20px).
        let s = LogScale::new((0.1, 100.0), (300.0, 20.0), DEFAULT_LOG_BASE);
        assert!(close(s.map(0.1).unwrap(), 300.0));
        assert!(close(s.map(100.0).unwrap(), 20.0));
        assert!(s.map(100.0).unwrap() < s.map(0.1).unwrap());
    }

    #[test]
    fn r1528_log_invert_round_trips() {
        let s = LogScale::new((0.01, 1000.0), (10.0, 410.0), DEFAULT_LOG_BASE);
        for v in [0.01, 0.5, 1.0, 42.0, 1000.0] {
            let px = s.map(v).expect("positive value maps");
            let back = s.invert(px);
            assert!(
                (back - v).abs() <= 1e-6 * v.abs().max(1.0),
                "round-trip {v} -> {px}px -> {back}"
            );
        }
    }

    /// ★ The reason [`ValueScale::map`] is partial. A log axis has no pixel
    /// for zero or a negative sample; returning one would draw a point the
    /// data does not contain.
    #[test]
    fn r1528_nonpositive_has_no_pixel_rather_than_the_baseline() {
        let s = LogScale::new((1.0, 1000.0), (300.0, 0.0), DEFAULT_LOG_BASE);
        for v in [0.0, -1.0, -0.0, f64::NEG_INFINITY, f64::NAN] {
            assert_eq!(s.map(v), None, "log has no pixel for {v}");
            assert!(!ValueScale::Log(s).defines(v));
        }
        // The counterfactual that makes the claim worth anything: the
        // baseline pixel IS a real, reachable pixel for a positive value,
        // so "map to the baseline" would have been indistinguishable from
        // a genuine sample at the domain floor.
        assert_eq!(s.map(1.0), Some(300.0));
    }

    #[test]
    fn r1528_linear_axis_is_partial_only_at_non_finite() {
        let s = ValueScale::Linear(LinearScale::new((0.0, 10.0), (0.0, 100.0)));
        assert_eq!(s.map(-5.0), Some(-50.0), "negatives are ordinary here");
        assert_eq!(s.map(f64::NAN), None);
        assert_eq!(s.map(f64::INFINITY), None);
        assert_eq!(s.kind(), AxisKind::Linear);
    }

    // ---- R1529: the time axis ------------------------------------------

    /// ★ A time axis reuses the affine core untouched — that is the claim
    /// the [`ValueScale::Time`] arm makes, and it is checkable: its mapping
    /// must agree with a [`LinearScale`] over the same domain everywhere.
    /// If it did not, the arm would be hiding arithmetic rather than policy.
    #[test]
    fn r1529_time_axis_maps_exactly_as_the_affine_core_does() {
        let domain = (1_772_582_400_000.0, 1_772_586_000_000.0); // one hour
        let inner = LinearScale::new(domain, (0.0, 300.0));
        let axis = ValueScale::Time(inner);
        for frac in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let v = domain.0 + frac * (domain.1 - domain.0);
            assert_eq!(
                axis.map(v),
                Some(inner.map(v)),
                "time is affine in the instant"
            );
        }
        assert_eq!(axis.domain(), domain);
        assert_eq!(axis.range(), (0.0, 300.0));
        assert!(close64(axis.invert(150.0), inner.invert(150.0)));
        assert_eq!(axis.kind(), AxisKind::Time);
    }

    /// ★ The reason [`AxisKind`] is a type rather than the `is_log` boolean
    /// it replaced: with three kinds, `!is_log` no longer names one of them.
    /// A time axis carries negative values (any instant before 1970) and
    /// must not be mistaken for a log axis, which cannot.
    #[test]
    fn r1529_axis_kind_names_three_arms_where_a_bool_named_two() {
        assert!(AxisKind::Time.defines(-86_400_000.0), "1969 is an instant");
        assert!(AxisKind::Linear.defines(-86_400_000.0));
        assert!(!AxisKind::Log(10.0).defines(-86_400_000.0));
        for k in [AxisKind::Linear, AxisKind::Log(10.0), AxisKind::Time] {
            assert!(!k.defines(f64::NAN), "{k:?} defines no pixel for NaN");
            assert!(!k.defines(f64::INFINITY));
        }
        assert_eq!(AxisKind::Log(2.0).log_base(), Some(2.0));
        assert_eq!(AxisKind::Time.log_base(), None);
        assert_eq!(AxisKind::Linear.log_base(), None);
        assert_eq!(AxisKind::default(), AxisKind::Linear);
    }

    #[test]
    fn r1528_out_of_contract_construction_is_sanitised_and_reported() {
        // A non-positive endpoint cannot be a log domain; the fallback is
        // the unit decade, and `domain()` reports what is actually in use
        // rather than echoing the impossible request.
        let s = LogScale::new((0.0, 100.0), (0.0, 100.0), DEFAULT_LOG_BASE);
        assert_eq!(s.domain(), (1.0, 10.0));
        assert!(close64(s.base(), 10.0));
        // A base of 1 has no logarithm; 0 and NaN neither.
        for bad in [1.0, 0.0, -2.0, f64::NAN] {
            let fixed = LogScale::new((1.0, 8.0), (0.0, 90.0), bad);
            assert!(
                close64(fixed.base(), 10.0),
                "base {bad} is not a logarithm base"
            );
        }
        // A legal non-decimal base is kept: base 2 puts 8 at three steps.
        let two = LogScale::new((1.0, 8.0), (0.0, 90.0), 2.0);
        assert!(close64(two.base(), 2.0));
        assert!(close(two.map(2.0).unwrap(), 30.0));
        assert!(close(two.map(4.0).unwrap(), 60.0));
    }

    #[test]
    fn r1528_value_scale_delegates_domain_and_range() {
        let lin = ValueScale::Linear(LinearScale::new((1.0, 2.0), (3.0, 4.0)));
        assert_eq!(lin.domain(), (1.0, 2.0));
        assert_eq!(lin.range(), (3.0, 4.0));
        let log = ValueScale::Log(LogScale::new((1.0, 100.0), (5.0, 6.0), 10.0));
        assert_eq!(log.domain(), (1.0, 100.0));
        assert_eq!(log.range(), (5.0, 6.0));
        assert_eq!(log.kind(), AxisKind::Log(10.0));
        assert!(close64(log.invert(5.0), 1.0));
    }
}
