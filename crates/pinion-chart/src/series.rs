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

/// A named sequence of samples — rendered as one line (and, optionally, a
/// filled area) by [`crate::LineChart`], or as discrete point marks by
/// [`crate::ScatterChart`]. `color == None` defers to the chart palette by
/// series index; `Some` pins an explicit colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// Legend label.
    pub name: String,
    /// Samples in draw order (x is expected monotonic for a line, but
    /// this is not enforced — a non-monotonic series simply back-tracks).
    pub points: Vec<DataPoint>,
    /// Explicit colour override; `None` uses the palette by index.
    pub color: Option<Color>,
    /// Whether this series' geometry is drawn (R1379). A hidden series still
    /// occupies its palette index and legend slot, so hiding it never re-colours
    /// or re-indexes the others; it only drops the series' own marks / polyline.
    /// By default the **auto-domain is unaffected** — [`data_bounds`] measures
    /// every series regardless, so toggling visibility never rescales the axes
    /// (a hidden series that later returns lands on the same grid). A chart
    /// built with `rescale_to_visible` (R1381) instead measures only the
    /// visible series ([`visible_data_bounds`]), so hiding one *does* rescale.
    pub visible: bool,
}

impl Series {
    /// A series with the given name and samples, palette-coloured, visible.
    #[must_use]
    pub fn new(name: impl Into<String>, points: Vec<DataPoint>) -> Self {
        Self {
            name: name.into(),
            points,
            color: None,
            visible: true,
        }
    }

    /// Pin an explicit colour instead of the palette default.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Set whether this series' geometry is drawn (default `true`). Hiding a
    /// series drops only its own marks / polyline — the palette, legend indices,
    /// and auto-domain are unchanged (R1379).
    #[must_use]
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
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

/// The `(x, y)` extent across every point of `series`, or `None` when not a
/// single finite point is present. The shared scan behind [`data_bounds`] and
/// [`visible_data_bounds`] — the only difference between them is which series
/// the caller feeds in.
fn bounds_of<'a>(series: impl IntoIterator<Item = &'a Series>) -> Option<Bounds> {
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
    seen.then_some(Bounds {
        x: (min_x, max_x),
        y: (min_y, max_y),
    })
}

/// The combined data bounds across ALL `series` (visible or not), or `None`
/// when there is not a single point to measure. Non-finite coordinates are
/// skipped. This is the default auto-domain source, so hiding a series never
/// rescales the axes — see [`visible_data_bounds`] for the opt-in that does.
#[must_use]
pub fn data_bounds(series: &[Series]) -> Option<Bounds> {
    bounds_of(series)
}

/// The data bounds across only the VISIBLE series (R1381) — the auto-domain a
/// chart built with `rescale_to_visible` snaps to, so hiding the dominant
/// series lets the axes rescale to reveal the rest. `None` when no visible
/// series has a finite point (the caller then falls back to the all-series
/// [`data_bounds`], so hiding the last series leaves the grid put rather than
/// collapsing it).
#[must_use]
pub fn visible_data_bounds(series: &[Series]) -> Option<Bounds> {
    bounds_of(series.iter().filter(|s| s.visible))
}

/// The series point whose x is nearest `data_x`, restricted to the visible x
/// range `[lo, hi]` so a zoomed chart never inspects a point outside the plot.
/// Non-finite points are skipped. Shared by the two numeric-x charts that scrub
/// a per-series nearest point — the line chart and the scatter chart (R1377);
/// it lives here, on the data model, rather than in either chart.
#[must_use]
pub(crate) fn nearest_point_in(
    series: &Series,
    data_x: f64,
    lo: f64,
    hi: f64,
) -> Option<DataPoint> {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    series
        .points
        .iter()
        .filter(|p| p.x.is_finite() && p.y.is_finite() && p.x >= lo && p.x <= hi)
        .copied()
        .min_by(|a, b| {
            (a.x - data_x)
                .abs()
                .partial_cmp(&(b.x - data_x).abs())
                .unwrap_or(core::cmp::Ordering::Equal)
        })
}

/// Whether `value` lies within the domain `[lo, hi]` (order-agnostic, with an
/// epsilon slack so a tick exactly on the domain edge counts). The line and
/// scatter charts both filter their nice-ticks to the domain with it (R1377).
#[must_use]
pub(crate) fn in_domain(value: f64, lo: f64, hi: f64) -> bool {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    value >= lo - f64::EPSILON && value <= hi + f64::EPSILON
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

    #[test]
    fn visible_bounds_measure_only_visible_series() {
        // A dominant series [0, 4000] plus a small one [0, 8]: the all-series
        // bounds span the big one, but with the big one hidden the visible
        // bounds collapse to the small one — the rescale-to-visible domain.
        let big = Series::new(
            "big",
            vec![DataPoint::new(0.0, 0.0), DataPoint::new(3.0, 4000.0)],
        );
        let small = Series::new(
            "small",
            vec![DataPoint::new(0.0, 2.0), DataPoint::new(3.0, 8.0)],
        );
        let all = vec![big.clone().with_visible(false), small.clone()];

        assert_eq!(
            data_bounds(&all).expect("has points").y,
            (0.0, 4000.0),
            "all-series bounds still span the hidden big series"
        );
        assert_eq!(
            visible_data_bounds(&all).expect("one visible").y,
            (2.0, 8.0),
            "visible-only bounds collapse to the small series"
        );
    }

    #[test]
    fn visible_bounds_none_when_all_hidden() {
        let all: Vec<Series> = sample()
            .into_iter()
            .map(|s| s.with_visible(false))
            .collect();
        assert!(
            visible_data_bounds(&all).is_none(),
            "no visible series -> None (caller falls back to all-series bounds)"
        );
    }
}
