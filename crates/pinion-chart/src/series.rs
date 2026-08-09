//! The chart data model: [`DataPoint`], [`Series`], and the
//! [`data_bounds`] helper that derives an auto-domain from one or more
//! series.

use pinion_core::style::Color;

/// A single `(x, y)` sample in data space, optionally carrying a third
/// magnitude channel for value-encoded colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DataPoint {
    /// Independent-axis value (time, index, bucket centre, ...).
    pub x: f64,
    /// Dependent-axis value (the measured quantity).
    pub y: f64,
    /// R1438 — the optional THIRD channel a value-encoded colour scale reads
    /// (`ScatterChart::color_by`): a magnitude that is neither axis, e.g. an
    /// error rate over a latency/throughput plane. `None` (the default, and
    /// what [`Self::new`] builds) means "no third channel", and a chart
    /// colouring by value leaves such a point on its series colour.
    ///
    /// Carrying the magnitude ON the point rather than in a parallel
    /// index-keyed side table (the toolkit `setPointConfiguration`
    /// shape) makes misalignment unrepresentable: there is no second array to
    /// fall out of step with this one.
    pub value: Option<f64>,
}

impl DataPoint {
    /// Construct a data point with no third channel.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y, value: None }
    }

    /// R1438 — attach the magnitude a value-encoded colour scale reads.
    #[must_use]
    pub const fn with_value(mut self, value: f64) -> Self {
        self.value = Some(value);
        self
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
    /// `(min_x, max_x)` across only the points whose x is strictly
    /// **positive**, or `None` when there is no such point — the auto-domain
    /// source for a logarithmic x-axis (R1528).
    pub positive_x: Option<(f64, f64)>,
    /// `(min_y, max_y)` across only the points whose y is strictly
    /// **positive**, or `None` when there is no such point — the auto-domain
    /// source for a logarithmic y-axis (R1528).
    pub positive_y: Option<(f64, f64)>,
}

/// The `(x, y)` extent across every point of `series`, or `None` when not a
/// single finite point is present. The shared scan behind [`data_bounds`],
/// [`visible_data_bounds`], and [`bounds_in_x_window`] — the only differences
/// between them are which series the caller feeds in and whether points are
/// restricted to an x-window.
///
/// `x_window` (`Some((lo, hi))`, order-agnostic) restricts the scan to points
/// whose x falls inside it — the auto-y-fit source for a brush-zoomed chart
/// ([`bounds_in_x_window`]). `None` measures every finite point.
///
/// The **positive-only** extents ([`Bounds::positive_x`] /
/// [`Bounds::positive_y`], R1528) come out of this same scan rather than a
/// second one, so a logarithmic axis and a linear one over the same chart
/// can never disagree about which points they measured — the selection
/// rules (visible series, x-window, finiteness) are applied once.
///
/// Note each axis's positive extent is filtered by that axis's own
/// coordinate only: a point at `(5, -1)` is off-scale on a log **y**-axis
/// yet still carries an ordinary x, so it contributes to `positive_x`.
/// Dropping it from the x-extent would make a time axis jump whenever a
/// sample dipped to zero.
fn bounds_of<'a>(
    series: impl IntoIterator<Item = &'a Series>,
    x_window: Option<(f64, f64)>,
) -> Option<Bounds> {
    let in_window = |x: f64| {
        x_window.is_none_or(|(lo, hi)| {
            let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
            x >= lo && x <= hi
        })
    };
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut pos_x: Option<(f64, f64)> = None;
    let mut pos_y: Option<(f64, f64)> = None;
    let mut seen = false;
    let widen = |slot: &mut Option<(f64, f64)>, v: f64| {
        if v > 0.0 {
            *slot = Some(match *slot {
                Some((lo, hi)) => (lo.min(v), hi.max(v)),
                None => (v, v),
            });
        }
    };
    for s in series {
        for p in &s.points {
            if !p.x.is_finite() || !p.y.is_finite() || !in_window(p.x) {
                continue;
            }
            seen = true;
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_y = min_y.min(p.y);
            max_y = max_y.max(p.y);
            widen(&mut pos_x, p.x);
            widen(&mut pos_y, p.y);
        }
    }
    seen.then_some(Bounds {
        x: (min_x, max_x),
        y: (min_y, max_y),
        positive_x: pos_x,
        positive_y: pos_y,
    })
}

/// The combined data bounds across ALL `series` (visible or not), or `None`
/// when there is not a single point to measure. Non-finite coordinates are
/// skipped. This is the default auto-domain source, so hiding a series never
/// rescales the axes — see [`visible_data_bounds`] for the opt-in that does.
#[must_use]
pub fn data_bounds(series: &[Series]) -> Option<Bounds> {
    bounds_of(series, None)
}

/// R1438 — the `(min, max)` of the third channel across ALL `series`, or
/// `None` when not one point carries a finite [`DataPoint::value`]. This is
/// the auto colour-domain a value-encoded chart maps against, and it measures
/// every series for the same reason [`data_bounds`] does: hiding a series must
/// not silently re-colour the rest.
///
/// A single distinct value yields a degenerate `(v, v)` span; the colour scale
/// itself is total over such a domain (it maps to the low end) rather than
/// dividing by zero here.
#[must_use]
pub fn value_bounds(series: &[Series]) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut seen = false;
    for v in series
        .iter()
        .flat_map(|s| s.points.iter())
        .filter_map(|p| p.value)
        .filter(|v| v.is_finite())
    {
        seen = true;
        lo = lo.min(v);
        hi = hi.max(v);
    }
    seen.then_some((lo, hi))
}

/// The data bounds across only the VISIBLE series (R1381) — the auto-domain a
/// chart built with `rescale_to_visible` snaps to, so hiding the dominant
/// series lets the axes rescale to reveal the rest. `None` when no visible
/// series has a finite point (the caller then falls back to the all-series
/// [`data_bounds`], so hiding the last series leaves the grid put rather than
/// collapsing it).
#[must_use]
pub fn visible_data_bounds(series: &[Series]) -> Option<Bounds> {
    bounds_of(series.iter().filter(|s| s.visible), None)
}

/// The data bounds across only the points whose x falls inside `window`
/// (`(lo, hi)`, order-agnostic), across ALL `series` (R1397) — the auto-y-fit
/// source a chart built with
/// [`LineChart::rescale_y_to_x_window`](crate::LineChart::rescale_y_to_x_window)
/// snaps its y-axis to, so brushing to a narrow x-window lets the y-axis zoom in
/// on just that window's values (a large transient elsewhere no longer flattens
/// the detail). `None` when no point falls inside the window (the caller then
/// keeps the full-data y-domain rather than collapsing the axis).
///
/// This measures every series, in parallel with [`data_bounds`] (the stable-grid
/// default) — the x-window axis is orthogonal to the visible-series axis of
/// [`visible_data_bounds`]; composing the two (a windowed *and* visible-only
/// fit) is deferred until a consumer needs both at once.
#[must_use]
pub fn bounds_in_x_window(series: &[Series], window: (f64, f64)) -> Option<Bounds> {
    bounds_of(series, Some(window))
}

/// The series point whose x is nearest `data_x`, restricted to the visible x
/// range `[lo, hi]` so a zoomed chart never inspects a point outside the plot.
/// Non-finite points are skipped. Shared by the two numeric-x charts that scrub
/// a per-series nearest point — the line chart and the scatter chart (R1377);
/// it lives here, on the data model, rather than in either chart.
#[must_use]
///
/// `keep` (R1528) narrows the candidates to the points the chart can
/// actually place — on a log axis a zero sample has no pixel. It is applied
/// *inside* the nearest-point search rather than to its result, so a series
/// whose nearest sample is off-scale still contributes the nearest one that
/// is drawn, instead of dropping out of the scrub entirely. This is the
/// R1379 rule ("the scrub never rings a point whose geometry was dropped")
/// applied at the same place: the focus source.
pub(crate) fn nearest_point_in(
    series: &Series,
    data_x: f64,
    lo: f64,
    hi: f64,
    keep: impl Fn(&DataPoint) -> bool,
) -> Option<DataPoint> {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    series
        .points
        .iter()
        .filter(|p| p.x.is_finite() && p.y.is_finite() && p.x >= lo && p.x <= hi && keep(p))
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
    fn windowed_bounds_fit_y_to_the_x_window() {
        // A big transient at x=1 (y=1000) plus small ripples at x=5..8 (y in
        // 40..70). The full bounds span the transient; windowing to x>=5 drops
        // it, so the y-extent collapses to the ripples — the auto-y-fit a
        // brush-zoom relies on.
        let series = vec![Series::new(
            "signal",
            vec![
                DataPoint::new(0.0, 60.0),
                DataPoint::new(1.0, 1000.0),
                DataPoint::new(5.0, 40.0),
                DataPoint::new(6.0, 70.0),
                DataPoint::new(8.0, 55.0),
            ],
        )];
        assert_eq!(
            data_bounds(&series).expect("has points").y,
            (40.0, 1000.0),
            "full bounds span the transient"
        );
        let win = bounds_in_x_window(&series, (5.0, 8.0)).expect("points in window");
        assert_eq!(win.y, (40.0, 70.0), "windowed y fits the ripples only");
        assert_eq!(win.x, (5.0, 8.0), "windowed x is the in-window extent");
    }

    #[test]
    fn windowed_bounds_are_order_agnostic_and_inclusive() {
        let series = vec![Series::new(
            "s",
            vec![
                DataPoint::new(0.0, 1.0),
                DataPoint::new(5.0, 9.0),
                DataPoint::new(10.0, 2.0),
            ],
        )];
        // Reversed window == forward window; the x=5 point sits on/inside it.
        let fwd = bounds_in_x_window(&series, (4.0, 6.0)).expect("in window");
        let rev = bounds_in_x_window(&series, (6.0, 4.0)).expect("in window");
        assert_eq!(fwd, rev, "the window is order-agnostic");
        assert_eq!(fwd.y, (9.0, 9.0), "only the x=5 point is inside");
    }

    #[test]
    fn windowed_bounds_none_when_the_window_is_empty() {
        let series = vec![Series::new(
            "s",
            vec![DataPoint::new(0.0, 1.0), DataPoint::new(10.0, 2.0)],
        )];
        assert!(
            bounds_in_x_window(&series, (3.0, 7.0)).is_none(),
            "a window with no points -> None (caller keeps the full-data y)"
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
