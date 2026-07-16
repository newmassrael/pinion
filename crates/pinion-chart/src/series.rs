//! The chart data model: [`DataPoint`], [`Series`], and the
//! [`data_bounds`] helper that derives an auto-domain from one or more
//! series.

use pinion_core::style::Color;

/// A single `(x, y)` sample in data space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DataPoint {
    /// Independent-axis value (time, index, bucket centre, ...).
    pub x: f64,
    /// Dependent-axis value (the measured quantity).
    pub y: f64,
}

impl DataPoint {
    /// Construct a data point.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A named ordered sequence of samples rendered as one line (and,
/// optionally, one filled area). `color == None` defers to the chart
/// palette by series index; `Some` pins an explicit colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// Legend label.
    pub name: String,
    /// Samples in draw order (x is expected monotonic for a line, but
    /// this is not enforced — a non-monotonic series simply back-tracks).
    pub points: Vec<DataPoint>,
    /// Explicit colour override; `None` uses the palette by index.
    pub color: Option<Color>,
}

impl Series {
    /// A series with the given name and samples, palette-coloured.
    #[must_use]
    pub fn new(name: impl Into<String>, points: Vec<DataPoint>) -> Self {
        Self {
            name: name.into(),
            points,
            color: None,
        }
    }

    /// Pin an explicit colour instead of the palette default.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

/// The `(lo, hi)` extent of a set of series on each axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// `(min_x, max_x)` across every point.
    pub x: (f64, f64),
    /// `(min_y, max_y)` across every point.
    pub y: (f64, f64),
}

/// The combined data bounds across `series`, or `None` when there is not
/// a single point to measure. Non-finite coordinates are skipped.
#[must_use]
pub fn data_bounds(series: &[Series]) -> Option<Bounds> {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut seen = false;
    for s in series {
        for p in &s.points {
            if !p.x.is_finite() || !p.y.is_finite() {
                continue;
            }
            seen = true;
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_y = min_y.min(p.y);
            max_y = max_y.max(p.y);
        }
    }
    if seen {
        Some(Bounds {
            x: (min_x, max_x),
            y: (min_y, max_y),
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Series> {
        vec![
            Series::new(
                "a",
                vec![DataPoint::new(0.0, 1.0), DataPoint::new(10.0, 5.0)],
            ),
            Series::new(
                "b",
                vec![DataPoint::new(-2.0, 3.0), DataPoint::new(4.0, -1.0)],
            ),
        ]
    }

    #[test]
    fn bounds_span_every_series() {
        let b = data_bounds(&sample()).expect("has points");
        assert_eq!(b.x, (-2.0, 10.0));
        assert_eq!(b.y, (-1.0, 5.0));
    }

    #[test]
    fn empty_series_have_no_bounds() {
        assert!(data_bounds(&[]).is_none());
        assert!(data_bounds(&[Series::new("empty", Vec::new())]).is_none());
    }

    #[test]
    fn non_finite_points_are_skipped() {
        let s = vec![Series::new(
            "x",
            vec![
                DataPoint::new(f64::NAN, 2.0),
                DataPoint::new(1.0, 2.0),
                DataPoint::new(3.0, f64::INFINITY),
            ],
        )];
        let b = data_bounds(&s).expect("one finite point");
        assert_eq!(b.x, (1.0, 1.0));
        assert_eq!(b.y, (2.0, 2.0));
    }

    #[test]
    fn with_color_pins_override() {
        let s = Series::new("c", vec![]).with_color(Color::rgb(1, 2, 3));
        assert_eq!(s.color, Some(Color::rgb(1, 2, 3)));
    }
}
