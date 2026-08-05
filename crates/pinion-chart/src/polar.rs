//! R1568 — the crate's first **non-cartesian coordinate system**.
//!
//! Every chart in this crate until now shared one unexamined assumption: a
//! value maps to a pixel on ONE axis, x horizontally and y vertically, and
//! the two compose by sitting at right angles. [`ValueScale`] is that
//! assumption's type — `f64 -> f32`, a single number on a single line — and
//! four axis KINDS (linear, log, time, category) all fit inside it because
//! each is still a map onto one line.
//!
//! A polar plot does not fit. Its two axes are an **angle** and a
//! **radius**, and neither is a pixel: a point's screen position is a
//! function of *both* together. What the coordinate system needs from an
//! angular axis is not "where on a line" but "how far round", and that
//! difference is not a fifth `AxisKind` — it is a different composition.
//!
//! # What is actually new: the angular axis is PERIODIC
//!
//! On a line, `0` and `360` are two places. On a circle drawn over a
//! `0 .. 360` period they are the **same** place, and that single fact is
//! the whole of what [`AngularScale`] adds:
//!
//! * A value outside the period is **placed**, not dropped — `370` on a
//!   compass is `10`, and a chart that discards it is answering a question
//!   nobody asked.
//! * A series **closes on itself**. The segment from the last sample back to
//!   the first is derived from the axis, so a radar polygon needs no
//!   duplicated datum.
//! * The tick at the period's end is the tick at its start, so a closed axis
//!   must emit **one** of them or two labels stack at 12 o'clock.
//!
//! Qt's `QPolarChart` has none of this, and for a locatable reason: its
//! angular axis is an ordinary `QValueAxis` — the same class a cartesian
//! x-axis uses — so periodicity is not modelled anywhere. A value outside
//! the range behaves exactly as it does on a cartesian axis, and closing a
//! radar loop is the caller's job (append the first point again, which puts
//! a duplicate in the data the model does not contain).
//!
//! # And the period cannot be inferred
//!
//! [`PolarChart`](crate::PolarChart) **requires** its angular axis rather
//! than auto-scaling one from the data, which is the opposite of what every
//! cartesian chart here does. The reason is that a period is a fact about
//! the *quantity* — a compass has 360 degrees, a week has 7 days, a clock
//! has 24 hours — and no sample reveals it. Qt auto-scales the angular axis
//! like any other, so a wind rose whose samples happen to span `10 .. 350`
//! silently gets a period of 340: every bearing then means something
//! different, and adding one sample changes what all the others mean.
//!
//! # Which conventions are declarations here
//!
//! Qt hard-codes two: the angular minimum sits at 12 o'clock and values
//! increase clockwise. That is the compass convention and it is the right
//! default, so it is the default here — but a mathematician's polar plot
//! (zero at 3 o'clock, increasing counter-clockwise) is a normal form that
//! `QPolarChart` cannot draw at all, so [`origin`](AngularScale::with_origin)
//! and [`winding`](AngularScale::with_winding) are declarations.
//!
//! A **sector** is the third thing Qt cannot do: `QPolarChart` is always a
//! full circle, while a gauge over half a turn is an ordinary professional
//! form. The sweep is therefore a declaration too — and it is what decides
//! periodicity, by derivation rather than by a second flag: an axis wraps
//! **iff its sweep closes the circle**, because wrapping a sector would fold
//! data onto the gap it deliberately leaves.

use core::f32::consts::{PI, TAU};

use crate::scale::ValueScale;

/// How close a sweep must be to a full turn to count as closing the circle.
///
/// Radians, so this is about a thousandth of a degree — tight enough that a
/// deliberate sector is never mistaken for a full turn, loose enough that a
/// sweep computed as `360.0_f64.to_radians()` and narrowed to `f32` still
/// closes.
const CLOSURE_EPSILON: f32 = 1e-5;

/// Which way angles increase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Winding {
    /// Increasing values run clockwise — the compass convention, and what
    /// `QPolarChart` hard-codes.
    #[default]
    Clockwise,
    /// Increasing values run counter-clockwise — the mathematical
    /// convention. `QPolarChart` cannot draw this.
    CounterClockwise,
}

impl Winding {
    /// This winding's name, for a readout or a wire field.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Clockwise => "clockwise",
            Self::CounterClockwise => "counter-clockwise",
        }
    }

    /// The sign this winding applies to an angle measured clockwise.
    const fn sign(self) -> f32 {
        match self {
            Self::Clockwise => 1.0,
            Self::CounterClockwise => -1.0,
        }
    }
}

/// The angular axis of a polar plot: a value interval mapped onto a sweep of
/// angle.
///
/// Angles are radians measured **clockwise from 12 o'clock**, which is the
/// frame this crate's own arc builder already draws in, so a grid ring and a
/// data point are placed by one convention rather than two that must agree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngularScale {
    /// The value interval one sweep represents — the **period**.
    period: (f64, f64),
    /// Where `period.0` sits, radians clockwise from 12 o'clock.
    origin: f32,
    /// How much angle the period occupies. `TAU` closes the circle.
    sweep: f32,
    winding: Winding,
}

impl AngularScale {
    /// A full-turn axis over `period`, in Qt's convention: `period.0` at 12
    /// o'clock, increasing clockwise.
    ///
    /// A degenerate or non-finite period is widened to `0.0 ..= 1.0` rather
    /// than left to divide by zero — a scale that could fail mid-axis would
    /// have to invent a position anyway, and this is the one every other
    /// scale in the crate takes.
    #[must_use]
    pub fn new(period: (f64, f64)) -> Self {
        let (lo, hi) = period;
        let period = if lo.is_finite() && hi.is_finite() && (hi - lo).abs() > f64::EPSILON {
            (lo, hi)
        } else {
            (0.0, 1.0)
        };
        Self {
            period,
            origin: 0.0,
            sweep: TAU,
            winding: Winding::Clockwise,
        }
    }

    /// Move `period.0` to `origin` radians clockwise from 12 o'clock.
    ///
    /// `QPolarChart` fixes this at 12 o'clock with no accessor.
    #[must_use]
    pub const fn with_origin(mut self, origin: f32) -> Self {
        self.origin = origin;
        self
    }

    /// Put `period.0` at the 3 o'clock position and run counter-clockwise —
    /// the mathematical convention, in one call.
    #[must_use]
    pub fn mathematical(self) -> Self {
        self.with_origin(PI / 2.0)
            .with_winding(Winding::CounterClockwise)
    }

    /// Occupy `sweep` radians rather than a full turn.
    ///
    /// **This is what decides periodicity**, by derivation: an axis wraps iff
    /// its sweep closes the circle ([`closes`](Self::closes)). Wrapping a
    /// sector would fold values onto the gap the sector deliberately leaves,
    /// so a value outside a sector's period is genuinely off-scale where the
    /// same value on a full turn is merely on the other side.
    ///
    /// `QPolarChart` is always a full circle, so a gauge is not expressible
    /// there.
    #[must_use]
    pub const fn with_sweep(mut self, sweep: f32) -> Self {
        self.sweep = sweep;
        self
    }

    /// Set which way values increase.
    #[must_use]
    pub const fn with_winding(mut self, winding: Winding) -> Self {
        self.winding = winding;
        self
    }

    /// The value interval one sweep represents.
    #[must_use]
    pub const fn period(&self) -> (f64, f64) {
        self.period
    }

    /// The width of the period in value units — always positive.
    #[must_use]
    pub fn span(&self) -> f64 {
        (self.period.1 - self.period.0).abs()
    }

    /// Where `period.0` sits, radians clockwise from 12 o'clock.
    #[must_use]
    pub const fn origin(&self) -> f32 {
        self.origin
    }

    /// How much angle the period occupies, in radians.
    #[must_use]
    pub const fn sweep(&self) -> f32 {
        self.sweep
    }

    /// Which way values increase.
    #[must_use]
    pub const fn winding(&self) -> Winding {
        self.winding
    }

    /// Whether the sweep closes the circle — and therefore whether this axis
    /// is **periodic**.
    ///
    /// Derived rather than declared, so "wraps" and "is a full turn" cannot
    /// be set to disagree. Everything periodicity implies reads this one
    /// answer: whether a value outside the period is placed or reported,
    /// whether a series closes on itself, and whether the tick at the end of
    /// the period is the tick at its start.
    #[must_use]
    pub fn closes(&self) -> bool {
        (self.sweep.abs() - TAU).abs() < CLOSURE_EPSILON
    }

    /// `value`'s position as a fraction of the period, **wrapped into
    /// `0.0 .. 1.0` when this axis closes**.
    ///
    /// `None` when the value is not finite, or when a *sector* axis does not
    /// reach it.
    #[must_use]
    pub fn fraction(&self, value: f64) -> Option<f64> {
        if !value.is_finite() {
            return None;
        }
        let t = (value - self.period.0) / (self.period.1 - self.period.0);
        if self.closes() {
            return Some(t.rem_euclid(1.0));
        }
        (0.0..=1.0).contains(&t).then_some(t)
    }

    /// Whether `value` needed wrapping to be placed — i.e. it lies outside
    /// the period and this axis carried it anyway.
    ///
    /// The report a periodic axis owes its caller. Qt cannot answer it
    /// because it does not place such a value at all, and a consumer that
    /// pre-wrapped its own data cannot tell either, having already destroyed
    /// the evidence.
    #[must_use]
    pub fn wrapped(&self, value: f64) -> bool {
        if !self.closes() || !value.is_finite() {
            return false;
        }
        let t = (value - self.period.0) / (self.period.1 - self.period.0);
        !(0.0..1.0).contains(&t)
    }

    /// `value`'s angle, radians clockwise from 12 o'clock, or `None` when
    /// this axis does not reach it.
    #[must_use]
    pub fn angle(&self, value: f64) -> Option<f32> {
        let t = self.fraction(value)?;
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a period fraction is bounded to 0..=1 for a closed axis and by the sector test otherwise"
        )]
        let t = t as f32;
        Some(self.origin + self.winding.sign() * self.sweep * t)
    }

    /// Whether this axis can place `value` at all.
    #[must_use]
    pub fn defines(&self, value: f64) -> bool {
        self.fraction(value).is_some()
    }

    /// The value at `fraction` of the period — the inverse of
    /// [`fraction`](Self::fraction), for a hit-test.
    #[must_use]
    pub fn value_at(&self, fraction: f64) -> f64 {
        self.period.0 + fraction * (self.period.1 - self.period.0)
    }
}

/// A resolved polar plotting frame: a centre, an outer radius, the angular
/// axis and the radial one.
///
/// The counterpart of [`CartesianPlot`](crate::plot::CartesianPlot), and the
/// reason it cannot be one: the radial axis is an ordinary [`ValueScale`]
/// over `0 .. radius` (so a **logarithmic** radial axis is R1528's, not a
/// second implementation), while the angular axis is an [`AngularScale`],
/// and a point's pixel is a function of both together rather than one per
/// screen dimension.
#[derive(Debug, Clone)]
pub(crate) struct PolarPlot {
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
    pub angular: AngularScale,
    pub radial: ValueScale,
}

impl PolarPlot {
    /// The pixel at `angle` (radians clockwise from 12 o'clock) and `radius`
    /// pixels from this plot's centre.
    ///
    /// The one place the coordinate convention is written down. It matches
    /// [`crate::draw`]'s arc builder — angle 0 at the top, increasing
    /// clockwise — so a grid ring and a data point cannot be drawn in two
    /// frames that disagree.
    pub(crate) fn at(&self, angle: f32, radius: f32) -> (f32, f32) {
        (
            self.cx + radius * angle.sin(),
            self.cy - radius * angle.cos(),
        )
    }

    /// `(angular_value, radial_value)` mapped to its pixel, or `None` when
    /// either axis does not reach it.
    ///
    /// One step rather than two predicates, for [`CartesianPlot::map_point`](crate::plot::CartesianPlot::map_point)'s
    /// reason: a caller that filters and then maps can desynchronise a series
    /// from its per-sample colours.
    pub(crate) fn map(&self, angular: f64, radial: f64) -> Option<(f32, f32)> {
        let angle = self.angular.angle(angular)?;
        let r = self.radial.map(radial)?;
        Some(self.at(angle, r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A compass: 0..360 degrees, full turn, north at the top.
    fn compass() -> AngularScale {
        AngularScale::new((0.0, 360.0))
    }

    /// ★ The round's claim, and the one thing a line cannot do: on a closed
    /// axis a value outside the period is the SAME PLACE as its wrapped
    /// twin, not a value the axis fails to reach.
    ///
    /// Qt's angular axis is an ordinary `QValueAxis`, so 370 there behaves as
    /// it would on a cartesian x-axis: out of range, not drawn.
    #[test]
    fn r1568_a_closed_axis_places_a_value_outside_its_period() {
        let a = compass();
        assert!(a.closes());
        let ten = a.angle(10.0).expect("in period");
        let three_seventy = a.angle(370.0).expect("PAST QT: wrapped, not dropped");
        assert!(
            (ten - three_seventy).abs() < 1e-5,
            "370 degrees is 10 degrees: {ten} vs {three_seventy}"
        );
        // ...and going the other way.
        let minus = a.angle(-350.0).expect("wrapped");
        assert!((ten - minus).abs() < 1e-5, "-350 is 10: {ten} vs {minus}");

        // The wrap is REPORTED, which neither Qt nor a caller that pre-modded
        // its own data can answer.
        assert!(!a.wrapped(10.0));
        assert!(a.wrapped(370.0));
        assert!(a.wrapped(-350.0));
    }

    /// ★ A SECTOR does not wrap, and that is derived from the sweep rather
    /// than declared beside it — wrapping a gauge would fold values onto the
    /// gap it deliberately leaves.
    #[test]
    fn r1568_a_sector_does_not_wrap_and_says_so() {
        let gauge = compass().with_sweep(PI); // half a turn
        assert!(!gauge.closes());
        assert!(gauge.defines(0.0) && gauge.defines(360.0));
        assert_eq!(gauge.angle(370.0), None, "a sector does not reach it");
        assert!(!gauge.defines(370.0));
        assert!(
            !gauge.wrapped(370.0),
            "an unplaced value is off-scale, not wrapped"
        );

        // The same value on the full turn IS placed, so this is a property of
        // the SWEEP and not of the value.
        assert!(compass().defines(370.0));
    }

    /// ★ The compass convention is the default and it is Qt's — north at the
    /// top, increasing clockwise — while the mathematical convention, which
    /// `QPolarChart` cannot draw at all, is one call away.
    #[test]
    fn r1568_the_convention_is_a_declaration_not_a_hard_coded_frame() {
        let a = compass();
        assert!(
            a.angle(0.0).expect("origin").abs() < 1e-6,
            "0 is at 12 o'clock"
        );
        let east = a.angle(90.0).expect("placed");
        assert!(
            (east - PI / 2.0).abs() < 1e-5,
            "90 runs CLOCKWISE to 3 o'clock: {east}"
        );

        let m = compass().mathematical();
        assert_eq!(m.winding(), Winding::CounterClockwise);
        assert!(
            (m.angle(0.0).expect("origin") - PI / 2.0).abs() < 1e-5,
            "0 is at 3 o'clock"
        );
        // ...and 90 runs the other way, back to 12 o'clock.
        assert!(
            m.angle(90.0).expect("placed").abs() < 1e-5,
            "{:?}",
            m.angle(90.0)
        );
    }

    /// ★ The two ends of a closed period are ONE place. That is what forces a
    /// polar chart to drop the duplicate tick, and what lets a series close
    /// on itself without a repeated datum.
    #[test]
    fn r1568_the_periods_two_ends_are_one_place() {
        let a = compass();
        let start = a.angle(0.0).expect("placed");
        let end = a.angle(360.0).expect("placed");
        assert!((start - end).abs() < 1e-5, "{start} vs {end}");
        assert!(
            (a.fraction(360.0).expect("placed")).abs() < 1e-9,
            "wrapped to 0"
        );

        // On a SECTOR they are two places, which is the counterfactual.
        let s = compass().with_sweep(PI);
        let (s0, s1) = (s.angle(0.0).expect("p"), s.angle(360.0).expect("p"));
        assert!((s1 - s0 - PI).abs() < 1e-5, "{s0} vs {s1}");
    }

    /// ★ The plot's pixel frame agrees with the arc builder's: angle 0 is the
    /// top and angles increase clockwise. Written once, so a grid ring and a
    /// data point cannot be placed by two conventions.
    #[test]
    fn r1568_the_pixel_frame_is_clockwise_from_twelve() {
        let plot = PolarPlot {
            cx: 100.0,
            cy: 100.0,
            radius: 50.0,
            angular: compass(),
            radial: ValueScale::Linear(crate::scale::LinearScale::new((0.0, 1.0), (0.0, 50.0))),
        };
        let near =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
        assert!(near(plot.at(0.0, 50.0), (100.0, 50.0)), "12 o'clock");
        assert!(near(plot.at(PI / 2.0, 50.0), (150.0, 100.0)), "3 o'clock");
        assert!(near(plot.at(PI, 50.0), (100.0, 150.0)), "6 o'clock");
        assert!(near(plot.at(0.0, 0.0), (100.0, 100.0)), "the centre");
    }

    /// ★ A degenerate period is widened rather than left to divide by zero —
    /// a scale that could fail mid-axis would have to invent a position.
    #[test]
    fn r1568_a_degenerate_period_is_widened_not_divided_by() {
        for bad in [(5.0, 5.0), (f64::NAN, 1.0), (0.0, f64::INFINITY)] {
            let a = AngularScale::new(bad);
            assert_eq!(a.period(), (0.0, 1.0), "{bad:?}");
            assert!(a.angle(0.5).expect("placed").is_finite());
        }
        // A non-finite VALUE is still refused: the axis is total, the input
        // is not.
        assert_eq!(compass().angle(f64::NAN), None);
        assert!(!compass().wrapped(f64::NAN));
    }

    /// ★ `value_at` inverts `fraction`, which is what a hit-test rides.
    #[test]
    fn r1568_fraction_and_value_at_are_inverses() {
        let a = compass();
        for v in [0.0, 45.0, 180.0, 359.0] {
            let t = a.fraction(v).expect("placed");
            assert!((a.value_at(t) - v).abs() < 1e-9, "{v} -> {t}");
        }
        // A wrapped value inverts to its representative in the period, which
        // is the honest answer rather than the caller's original number.
        let t = a.fraction(370.0).expect("wrapped");
        assert!((a.value_at(t) - 10.0).abs() < 1e-9);
    }
}
