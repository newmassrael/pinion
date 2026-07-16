//! Linear value-to-pixel mapping — the arithmetic core every axis and
//! series share.
//!
//! A [`LinearScale`] maps a data `domain` (`f64` value range) onto a
//! pixel `range` (`f32` device coordinates) with an affine transform,
//! and inverts it. The range endpoints may be given in either order, so
//! a y-axis simply passes `(bottom_px, top_px)` — a descending pixel
//! range — to get the screen-space "larger value sits higher" mapping
//! without any special-casing at the call site.

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
}
