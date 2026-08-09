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
/// R1622 §5.28 — one band of a **stacked** area chart: a series' own curve
/// sitting on the cumulative total of everything below it.
///
/// A band is two curves, not one. That is the whole reason this type exists:
/// the crate's area fill takes a scalar baseline, so an application wanting a
/// stack had to compute every cumulative total itself and hand in a series
/// whose `y` was already a running sum — which meant the chart's tooltips,
/// legend and value axis all reported the SUM where the reader sees a band,
/// and the original measurement was gone by the time anything could ask.
///
/// Here the band keeps both: [`value`](Self::value) is what the series
/// measured, [`upper`](Self::upper) and [`lower`](Self::lower) are where it is
/// drawn. Nothing has to reconstruct one from the other.
#[derive(Debug, Clone, PartialEq)]
pub struct StackedBand {
    /// The series this band came from — its name, colour and visibility, with
    /// its **original** measurements in `points`.
    pub series: Series,
    /// Its index in the input slice, so a palette colour and a legend row
    /// still address it after hidden series are skipped.
    pub index: usize,
    /// The band's top edge: this series' value plus every visible one below.
    pub upper: Vec<DataPoint>,
    /// The band's bottom edge: the cumulative total below this series, which
    /// is a flat zero line for the bottom-most band.
    pub lower: Vec<DataPoint>,
}

impl StackedBand {
    /// This series' own measurement at sample `i` — what a tooltip should say,
    /// as distinct from the cumulative height the reader's eye lands on.
    #[must_use]
    pub fn value(&self, i: usize) -> Option<f64> {
        self.series.points.get(i).map(|p| p.y)
    }
}

/// R1622 §5.28 — **stack** a set of series: the composition an application had
/// to do for itself.
///
/// Each visible series is placed on the cumulative total of the visible series
/// before it, in slice order, so the first is the bottom band. Returns one
/// [`StackedBand`] per **visible** series; a hidden one contributes nothing to
/// the totals and produces no band, but the ones above it keep their palette
/// index, so toggling a series in a legend re-stacks without re-colouring the
/// rest ([`Series::visible`]'s existing rule, extended to the stack).
///
/// ## What it refuses to guess
///
/// Stacking is only defined when the series share an x grid: adding a value
/// sampled at 10:00 to one sampled at 10:07 requires deciding what the second
/// series was doing at 10:00, and every answer to that is an interpolation the
/// chart would be inventing. So the x values are taken from the **first
/// visible** series and every later one must match, position for position; a
/// series that does not is returned unstacked, sitting on the baseline, rather
/// than being silently resampled. The alternative — quietly interpolating —
/// produces a picture whose totals are not the data's totals, which is the one
/// failure a stacked chart must not have.
///
/// Negative values stack downward from the running total exactly as positives
/// stack upward, so a series that dips below zero pulls its band below the one
/// underneath instead of being clamped away.
#[must_use]
pub fn stack(series: &[Series]) -> Vec<StackedBand> {
    let mut running: Vec<f64> = Vec::new();
    let mut grid: Vec<f64> = Vec::new();
    let mut bands = Vec::new();
    for (index, s) in series.iter().enumerate() {
        if !s.visible {
            continue;
        }
        if running.is_empty() {
            running = vec![0.0; s.points.len()];
            grid = s.points.iter().map(|p| p.x).collect();
        }
        // The x grid must agree, position for position. `!=` on f64 is exact
        // here on purpose: these are the SAME numbers a caller put in, not
        // computed ones, so a tolerance would only let a genuinely different
        // grid through.
        #[expect(
            clippy::float_cmp,
            reason = "exact on purpose: these are the SAME f64s the caller put \
                      in, not computed ones, so a tolerance would only admit a \
                      genuinely different grid — and stacking against a grid \
                      that is not this series' is the invented-total failure \
                      this branch exists to refuse"
        )]
        let aligned =
            s.points.len() == grid.len() && s.points.iter().zip(&grid).all(|(p, &x)| p.x == x);
        let lower: Vec<DataPoint> = if aligned {
            grid.iter()
                .zip(&running)
                .map(|(&x, &y)| DataPoint::new(x, y))
                .collect()
        } else {
            s.points.iter().map(|p| DataPoint::new(p.x, 0.0)).collect()
        };
        let upper: Vec<DataPoint> = s
            .points
            .iter()
            .zip(&lower)
            .map(|(p, base)| DataPoint::new(p.x, base.y + p.y))
            .collect();
        if aligned {
            for (total, point) in running.iter_mut().zip(&s.points) {
                *total += point.y;
            }
        }
        bands.push(StackedBand {
            series: s.clone(),
            index,
            upper,
            lower,
        });
    }
    bands
}

/// R1622 §5.28 — the value bounds a **stacked** chart needs: the extent of the
/// cumulative totals, not of any one series.
///
/// Its own function because the difference is a real defect if missed. A stack
/// of three series each peaking at 40 reaches 120, and an axis scaled to the
/// per-series maximum would clip two thirds of the picture — so a caller that
/// stacked without rescaling would draw bands running off the top of the plot.
#[must_use]
pub fn stacked_value_bounds(bands: &[StackedBand]) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for band in bands {
        for point in band.upper.iter().chain(&band.lower) {
            lo = lo.min(point.y);
            hi = hi.max(point.y);
        }
    }
    (lo <= hi).then_some((lo, hi))
}

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

    /// R1622 §5.28 — the crate stacks. Each band sits on the cumulative total
    /// of the visible ones below it, and the ORIGINAL measurement survives.
    ///
    /// That second half is the point. An application stacking for itself had
    /// to hand in a series whose `y` was already a running sum, so a tooltip,
    /// a legend or an axis readout reported the SUM where the reader sees a
    /// band — the measurement was gone before anything could ask for it.
    #[expect(
        clippy::float_cmp,
        reason = "every number here is a small integer exactly representable in \
                  f64, and the sums of such are exact — a tolerance would let a \
                  genuinely wrong total pass as near enough, which is the one \
                  thing a stacked chart must not do"
    )]
    #[test]
    fn r1622_bands_sit_on_the_running_total_and_keep_their_own_values() {
        let s = |name: &str, ys: [f64; 3]| {
            Series::new(
                name,
                ys.iter()
                    .enumerate()
                    .map(|(i, &y)| DataPoint::new(f64::from(u32::try_from(i).unwrap_or(0)), y))
                    .collect(),
            )
        };
        let bands = stack(&[s("a", [1.0, 2.0, 3.0]), s("b", [10.0, 20.0, 30.0])]);
        assert_eq!(bands.len(), 2);
        // The bottom band rests on zero.
        assert_eq!(
            bands[0].lower.iter().map(|p| p.y).collect::<Vec<_>>(),
            vec![0.0, 0.0, 0.0]
        );
        assert_eq!(
            bands[0].upper.iter().map(|p| p.y).collect::<Vec<_>>(),
            vec![1.0, 2.0, 3.0]
        );
        // The second sits on the first's total, and its TOP is the sum.
        assert_eq!(
            bands[1].lower.iter().map(|p| p.y).collect::<Vec<_>>(),
            vec![1.0, 2.0, 3.0]
        );
        assert_eq!(
            bands[1].upper.iter().map(|p| p.y).collect::<Vec<_>>(),
            vec![11.0, 22.0, 33.0]
        );
        // ...while the series still reports what IT measured.
        assert_eq!(bands[1].value(0), Some(10.0), "not the cumulative 11");
        assert_eq!(bands[1].series.points[2].y, 30.0, "not 33");
        // The axis must be scaled to the total, or the top band runs off the
        // plot — the defect a caller hits the first time they stack.
        assert_eq!(stacked_value_bounds(&bands), Some((0.0, 33.0)));
        assert_eq!(
            data_bounds(&[s("a", [1.0, 2.0, 3.0]), s("b", [10.0, 20.0, 30.0])])
                .expect("both series have points")
                .y,
            (1.0, 30.0),
            "the UNSTACKED y bounds stop at 30 — scaling a stack to them clips \
             the top band clean off the plot, which is the defect a caller \
             meets the first time they stack",
        );
    }

    /// R1622 — a hidden series contributes nothing and produces no band, and
    /// the ones above it keep their palette index, so a legend toggle
    /// re-stacks without re-colouring the rest.
    #[expect(clippy::float_cmp, reason = "small integers, exact in f64")]
    #[test]
    fn r1622_a_hidden_series_leaves_the_stack_and_the_palette_alone() {
        let s = |name: &str, y: f64| Series::new(name, vec![DataPoint::new(0.0, y)]);
        let all = vec![s("a", 1.0), s("b", 2.0).with_visible(false), s("c", 4.0)];
        let bands = stack(&all);
        assert_eq!(bands.len(), 2, "the hidden one draws no band");
        assert_eq!(
            (bands[0].index, bands[1].index),
            (0, 2),
            "palette indices kept"
        );
        assert_eq!(
            bands[1].lower[0].y, 1.0,
            "and `c` rests on `a` alone — the hidden series adds nothing",
        );
        assert_eq!(bands[1].upper[0].y, 5.0);
    }

    /// R1622 — negatives stack DOWNWARD from the running total rather than
    /// being clamped, and a series whose x grid disagrees is left unstacked
    /// rather than silently resampled.
    #[expect(clippy::float_cmp, reason = "small integers, exact in f64")]
    #[test]
    fn r1622_negatives_stack_down_and_a_foreign_grid_is_refused() {
        let at = |xs: &[f64], ys: &[f64]| {
            Series::new(
                "s",
                xs.iter()
                    .zip(ys)
                    .map(|(&x, &y)| DataPoint::new(x, y))
                    .collect(),
            )
        };
        let bands = stack(&[at(&[0.0, 1.0], &[5.0, 5.0]), at(&[0.0, 1.0], &[-2.0, 3.0])]);
        assert_eq!(bands[1].lower[0].y, 5.0);
        assert_eq!(bands[1].upper[0].y, 3.0, "a negative pulls the band DOWN");
        assert_eq!(bands[1].upper[1].y, 8.0, "and a positive still pushes up");
        assert_eq!(stacked_value_bounds(&bands), Some((0.0, 8.0)));

        // A series sampled on a different grid cannot be added to the one
        // below without inventing what it was doing at the other's timestamps.
        // It is drawn on the baseline instead of being resampled: a picture
        // whose totals are not the data's totals is the one failure a stacked
        // chart must not have.
        let mixed = stack(&[at(&[0.0, 1.0], &[5.0, 5.0]), at(&[0.5, 1.5], &[1.0, 1.0])]);
        assert_eq!(mixed.len(), 2);
        assert_eq!(
            mixed[1].lower.iter().map(|p| p.y).collect::<Vec<_>>(),
            vec![0.0, 0.0],
            "the foreign grid sits on the baseline, unstacked",
        );
        assert_eq!(
            mixed[1].upper.iter().map(|p| p.y).collect::<Vec<_>>(),
            vec![1.0, 1.0],
            "carrying its own values, not a sum against a grid it is not on",
        );
        // NEGATIVE CONTROL — the SAME two series on a shared grid do stack, so
        // the refusal above is about the grid and not about these numbers.
        let shared = stack(&[at(&[0.0, 1.0], &[5.0, 5.0]), at(&[0.0, 1.0], &[1.0, 1.0])]);
        assert_eq!(shared[1].lower[0].y, 5.0);
    }

    /// R1622 — an empty or all-hidden input produces no bands and no bounds,
    /// rather than a zero-height axis a chart would divide by.
    #[test]
    fn r1622_nothing_to_stack_is_no_bands_and_no_bounds() {
        assert!(stack(&[]).is_empty());
        assert_eq!(stacked_value_bounds(&[]), None);
        let hidden = vec![Series::new("a", vec![DataPoint::new(0.0, 1.0)]).with_visible(false)];
        assert!(stack(&hidden).is_empty());
        assert_eq!(stacked_value_bounds(&stack(&hidden)), None);
    }
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
