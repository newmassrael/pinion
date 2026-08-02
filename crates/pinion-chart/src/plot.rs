//! `CartesianPlot` — the resolved plotting geometry (the margin-inset plot rect
//! plus a value scale on each axis) a numeric-x / numeric-y chart maps its data
//! through.
//!
//! Lived as `line.rs`'s private `Plot` until R1377 gave the crate a THIRD
//! cartesian chart (`scatter`, after `line` and the categorical `bar`). At the
//! second numeric-x consumer a `crate::line::Plot` import would read as "the
//! scatter chart borrows the line chart's plot"; the neutral module makes it
//! shared substrate — the same reasoning that moved [`crate::style::ChartStyle`]
//! out of `line.rs` at the second chart.
//!
//! R1545 made the categorical axis a [`ValueScale`] arm, so this resolver is
//! no longer numeric-only: `AxisKinds { x: Category(..), .. }` resolves a
//! [`CategoryScale`] here like any other kind, and a line or scatter chart
//! plots over named slots through it. [`crate::bar`] still keeps its own
//! `BarGeom` — a bar chart carries a baseline, a bar width and a gap
//! fraction that no other chart has — but the x inside it IS this crate's
//! category axis now, not a private slot metric.

use pinion_core::scene::Rect;

use crate::draw::{plot_rect, to_f32};
use crate::scale::{AxisKind, CategoryScale, LinearScale, LogScale, ValueScale, index_value};
use crate::series::{
    Bounds, DataPoint, Series, bounds_in_x_window, data_bounds, in_domain, nearest_point_in,
    visible_data_bounds,
};
use crate::style::ChartStyle;
use crate::ticks::{
    TickFormat, log_minor_ticks, log_ticks, nice_log_domain, nice_ticks, nice_time_domain,
    tick_step, time_ticks,
};

/// How the auto-domains are derived when they are not pinned — the two
/// orthogonal rescale opt-ins, bundled into one named-field value so the two
/// booleans can never be swapped at a call site (the R832-class silent-swap the
/// crate has been bitten by before).
///
/// `Default` is `{ to_visible: false, y_to_x_window: false }`: every series is
/// measured over its full x-extent, so the grid is stable across visibility
/// toggles and x-zooms (the R1379 dashboard default).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Rescale {
    /// Measure only the *visible* series (R1381) instead of all of them, so
    /// hiding the dominant series lets the axes rescale to reveal the rest.
    pub to_visible: bool,
    /// Fit the auto **y**-domain to the points inside the resolved x-window
    /// (R1397), so brushing to a narrow x-window zooms the y-axis in on just
    /// that window's values. Only bites when the x-domain is narrower than the
    /// data (a zoom); a full-width x-domain includes every point, so the y-fit
    /// is a no-op.
    pub y_to_x_window: bool,
}

/// Which KIND each axis of a cartesian plot is (R1528 log, R1529 time,
/// R1545 category).
///
/// A named-field value for the same reason [`Rescale`] is one: two
/// positional axis settings at a call site are a silent swap waiting to
/// happen. `Default` is linear on both axes.
#[derive(Debug, Clone, Default)]
pub(crate) struct AxisKinds {
    /// The x-axis's kind.
    pub x: AxisKind,
    /// The y-axis's kind.
    pub y: AxisKind,
}

/// The resolved plot rectangle (in float pixels) plus the two value scales.
#[derive(Debug, Clone)]
pub(crate) struct CartesianPlot {
    /// Plot-area left edge (px).
    pub left: f32,
    /// Plot-area right edge (px).
    pub right: f32,
    /// Plot-area top edge (px).
    pub top: f32,
    /// Plot-area bottom edge (px).
    pub bottom: f32,
    /// Value -> pixel on the x-axis (ascending: domain-lo -> left).
    pub x: ValueScale,
    /// Value -> pixel on the y-axis (INVERTED: domain-hi -> top / small pixel).
    pub y: ValueScale,
}

impl CartesianPlot {
    /// Resolve the plot geometry for `rect` under `style`: the margin-inset plot
    /// area, and an x / y [`LinearScale`] over each axis's domain (a pinned
    /// domain verbatim, else the data extent snapped to its nice-tick range so
    /// the outer gridlines land on the plot edges — Qt `applyNiceNumbers`).
    ///
    /// [`Rescale::to_visible`] (R1381) chooses the auto-domain source: `false`
    /// measures every series ([`data_bounds`] — hiding a series never rescales
    /// the axes, the R1379 default), `true` measures only the visible ones
    /// ([`visible_data_bounds`] — hiding the dominant series lets the axes
    /// rescale to reveal the rest), falling back to the all-series bounds when
    /// nothing is visible so hiding the last series leaves the grid put. A
    /// pinned `x_domain` / `y_domain` overrides either, so this only moves an
    /// auto axis.
    ///
    /// [`Rescale::y_to_x_window`] (R1397) then re-fits an *auto* y-domain to the
    /// points inside the resolved x-window ([`bounds_in_x_window`]): the x-window
    /// is settled first, so brushing to a narrow x-domain lets the y-axis zoom in
    /// on just that window's values. A window with no points, or a pinned
    /// `y_domain`, leaves the y-domain untouched.
    ///
    /// [`AxisKinds`] chooses each axis's *kind* (R1528 log, R1529 time, R1545
    /// category), which decides how an AUTO domain is snapped. A logarithmic
    /// axis takes its extent from the strictly positive part of the data
    /// ([`Bounds::positive_x`] / [`Bounds::positive_y`]) and snaps to whole
    /// decades ([`nice_log_domain`]); a time axis snaps to calendar
    /// boundaries ([`nice_time_domain`]); a linear axis to nice ticks; a
    /// categorical axis ignores the data entirely and takes the extent its
    /// category list declares ([`kind_extent`]). A pinned domain wins over
    /// all four — which is how a category WINDOW
    /// ([`CategoryWindow::domain`](crate::CategoryWindow::domain)) and a
    /// [`PlotWindow`](crate::PlotWindow) zoom reach a categorical axis
    /// through the one pinning path every other kind already used.
    pub(crate) fn resolve(
        rect: Rect,
        series: &[Series],
        x_domain: Option<(f64, f64)>,
        y_domain: Option<(f64, f64)>,
        style: &ChartStyle,
        rescale: Rescale,
        kinds: &AxisKinds,
    ) -> Self {
        let (left, right, top, bottom) = plot_rect(rect, style.margin);

        let bounds = if rescale.to_visible {
            visible_data_bounds(series).or_else(|| data_bounds(series))
        } else {
            data_bounds(series)
        };
        let raw_x = x_domain
            .or_else(|| bounds.and_then(|b| x_extent(b, &kinds.x)))
            .unwrap_or_else(|| kind_extent(&kinds.x));
        // The x-window must be settled before the y-fit reads it, so resolve the
        // final x-domain first. A pinned domain is honoured verbatim; an auto one
        // snaps to the nice-tick extent so the outer gridlines land on the edges.
        let dom_x = axis_domain(x_domain, raw_x, style.x_ticks, &kinds.x);
        // y: a pinned domain wins; else an opt-in fit to the x-window's points
        // (R1397); else the whole measured extent. The window fit falls back to
        // `bounds` when it is empty, so a brush past the data keeps the grid.
        let raw_y = y_domain
            .or_else(|| {
                rescale
                    .y_to_x_window
                    .then(|| bounds_in_x_window(series, dom_x).and_then(|b| y_extent(b, &kinds.y)))
                    .flatten()
            })
            .or_else(|| bounds.and_then(|b| y_extent(b, &kinds.y)))
            .unwrap_or_else(|| kind_extent(&kinds.y));
        let dom_y = axis_domain(y_domain, raw_y, style.y_ticks, &kinds.y);

        Self {
            left,
            right,
            top,
            bottom,
            x: axis_scale(dom_x, (left, right), &kinds.x),
            // y is inverted: domain-hi maps to the top (small pixel).
            y: axis_scale(dom_y, (bottom, top), &kinds.y),
        }
    }

    /// Whether both axes define a pixel for `point` — i.e. whether this plot
    /// can place it at all.
    pub(crate) fn defines(&self, point: &DataPoint) -> bool {
        self.x.defines(point.x) && self.y.defines(point.y)
    }

    /// `point` mapped to its pixel position, or `None` when an axis does not
    /// define one there. The single mapping entry point for the marks: a
    /// caller that filters and then maps through two separate predicates can
    /// desynchronise a series from its per-sample colours, so the filter and
    /// the map are one step.
    pub(crate) fn map_point(&self, point: &DataPoint) -> Option<(f32, f32)> {
        Some((self.x.map(point.x)?, self.y.map(point.y)?))
    }
}

/// One data point a chart could not place, and the series it came from
/// (R1528).
///
/// The counterpart of [`Mapped::unreadable`](crate::Mapped) on the other
/// input path: a value an axis does not define contributes no mark and is
/// *reported*, so a consumer can say "3 samples were zero and a log axis
/// has no zero" rather than have them drawn on the baseline — which would
/// be indistinguishable from real samples at the domain floor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffScale {
    /// Index of the series holding the point, in the chart's own order.
    pub series: usize,
    /// The point itself, with its original values.
    pub point: DataPoint,
}

/// Every point of `series` that a chart with these axis kinds cannot place,
/// in series order then point order.
///
/// Reports across ALL series, hidden ones included: a hidden series' points
/// are absent because the consumer hid them, which the consumer already
/// knows, whereas an off-scale point is absent because the *axis* cannot
/// carry it. Filter by [`OffScale::series`] if only the visible ones matter.
pub(crate) fn off_scale_points(series: &[Series], kinds: &AxisKinds) -> Vec<OffScale> {
    let mut out = Vec::new();
    for (i, s) in series.iter().enumerate() {
        for p in &s.points {
            if !kinds.x.defines(p.x) || !kinds.y.defines(p.y) {
                out.push(OffScale {
                    series: i,
                    point: *p,
                });
            }
        }
    }
    out
}

/// The x-extent one axis measures **from the data**: the whole one, or — when
/// the axis is logarithmic — only its strictly positive part, which is the
/// only part a logarithm is defined on. A time axis measures the whole extent:
/// an instant before 1970 is an ordinary negative value.
///
/// A categorical axis measures NOTHING here (`None`): its extent is the
/// category list it carries, not the values plotted on it, so it falls
/// through to [`kind_extent`]. That is the difference between a slot axis and
/// a value axis — a bar chart with one bar still has all of its categories.
const fn x_extent(b: Bounds, kind: &AxisKind) -> Option<(f64, f64)> {
    match kind {
        AxisKind::Log(_) => b.positive_x,
        AxisKind::Category(_) => None,
        AxisKind::Linear | AxisKind::Time => Some(b.x),
    }
}

/// [`x_extent`] for the y-axis.
const fn y_extent(b: Bounds, kind: &AxisKind) -> Option<(f64, f64)> {
    match kind {
        AxisKind::Log(_) => b.positive_y,
        AxisKind::Category(_) => None,
        AxisKind::Linear | AxisKind::Time => Some(b.y),
    }
}

/// The extent an axis kind DECLARES, independent of any data: `0..1` for a
/// linear axis, the unit decade for a logarithmic one (where `0` is not a
/// value the axis can carry), the epoch's first day for a time one (where
/// `0..1` would be a one-millisecond domain), and the whole category band run
/// for a categorical one.
///
/// For the three value kinds this is a *fallback*, reached only when nothing
/// measurable is present — a log chart whose data is entirely non-positive
/// lands here and draws a legible empty decade rather than collapsing. For
/// the categorical kind it is the extent itself: [`x_extent`] measures no
/// data, so this is always what a category axis resolves to before pinning.
fn kind_extent(kind: &AxisKind) -> (f64, f64) {
    match kind {
        AxisKind::Log(base) => (1.0, *base),
        AxisKind::Linear => (0.0, 1.0),
        AxisKind::Time => (0.0, 86_400_000.0),
        AxisKind::Category(c) => c.extent(),
    }
}

/// Resolve one axis's final domain — a pinned domain verbatim, else the raw
/// extent snapped the way that axis kind snaps: to nice ticks when linear,
/// to whole decades when logarithmic, to calendar boundaries when time, and
/// not at all when categorical (the band extent is already exact — snapping
/// it to nice numbers would put the axis edge half a slot off).
fn axis_domain(
    pinned: Option<(f64, f64)>,
    raw: (f64, f64),
    target: usize,
    kind: &AxisKind,
) -> (f64, f64) {
    if let Some(d) = pinned {
        return d;
    }
    match kind {
        AxisKind::Log(base) => nice_log_domain(raw.0, raw.1, *base),
        AxisKind::Time => nice_time_domain(raw.0, raw.1, target),
        AxisKind::Linear => domain_from_ticks(None, raw, target),
        AxisKind::Category(_) => raw,
    }
}

/// Build one axis's [`ValueScale`] over `domain` onto the pixel `range`.
fn axis_scale(domain: (f64, f64), range: (f32, f32), kind: &AxisKind) -> ValueScale {
    match kind {
        AxisKind::Log(base) => ValueScale::Log(LogScale::new(domain, range, *base)),
        AxisKind::Linear => ValueScale::Linear(LinearScale::new(domain, range)),
        AxisKind::Time => ValueScale::Time(LinearScale::new(domain, range)),
        AxisKind::Category(c) => ValueScale::Category(CategoryScale::new(c.clone(), domain, range)),
    }
}

/// Resolve a final domain: a pinned domain verbatim, else the nice-tick extent
/// of the raw data domain (falling back to the raw domain when ticks are
/// unavailable, e.g. a collapsed range).
pub(crate) fn domain_from_ticks(
    pinned: Option<(f64, f64)>,
    raw: (f64, f64),
    target: usize,
) -> (f64, f64) {
    if let Some(d) = pinned {
        return d;
    }
    let ticks = nice_ticks(raw.0, raw.1, target);
    match (ticks.first(), ticks.last()) {
        (Some(&lo), Some(&hi)) if (hi - lo).abs() > f64::EPSILON => (lo, hi),
        _ => raw,
    }
}

/// The **labelled** ticks of one axis, clipped to its domain — the ticks
/// that fall inside the visible range. Both the line and scatter charts
/// derive their tick labels and their gridline positions from it (R1377).
///
/// R1528 made the argument the scale rather than the bare domain: which
/// generator produces the ticks is a property of the axis kind, and taking
/// the scale means the tick set and the mapping that places it can never be
/// resolved from different axes. Both kinds reach the domain clip here, so
/// there is still one filter.
pub(crate) fn axis_ticks(scale: &ValueScale, target: usize) -> Vec<f64> {
    let (lo, hi) = scale.domain();
    let raw = match scale {
        ValueScale::Log(s) => log_ticks(lo, hi, s.base(), target),
        ValueScale::Time(_) => time_ticks(lo, hi, target),
        ValueScale::Linear(_) => nice_ticks(lo, hi, target),
        // R1545 — a categorical axis's ticks are its VISIBLE slots, and every
        // one of them is labelled: there is no nice-number ladder to snap to
        // and no reader expectation that a category be a round number. The
        // `target` count is deliberately ignored — thinning a category axis is
        // a decision about how many LABELS fit, which needs a measured text
        // width the scale does not have; the window is what bounds the count.
        ValueScale::Category(s) => s
            .visible()
            .map(|w| (w.lo()..=w.hi()).map(index_value).collect())
            .unwrap_or_default(),
    };
    raw.into_iter().filter(|t| in_domain(*t, lo, hi)).collect()
}

/// The pixel positions of `ticks` on one axis.
///
/// The `filter_map` is the [`ValueScale::map`] contract showing through
/// rather than a real filter: a tick comes from its own axis's generator,
/// so it is defined there by construction. Dropping an undefined one is
/// still the right total behaviour — a gridline at a pixel the axis did not
/// produce would be a line at an invented value.
pub(crate) fn tick_pixels(scale: &ValueScale, ticks: &[f64]) -> Vec<f32> {
    ticks.iter().filter_map(|&t| scale.map(t)).collect()
}

/// The label format one axis's ticks take: the constant step on a linear
/// axis (R1528), per-magnitude on a logarithmic one, which has no constant
/// step for a scalar to summarise, and per-calendar-field on a time one
/// (R1529), whose magnitude says nothing a reader wants.
pub(crate) fn axis_format(scale: &ValueScale, ticks: &[f64]) -> TickFormat {
    match scale.kind() {
        AxisKind::Log(_) => TickFormat::Log,
        AxisKind::Time => TickFormat::Time,
        AxisKind::Linear => TickFormat::Step(tick_step(ticks)),
        AxisKind::Category(c) => TickFormat::Category(c),
    }
}

/// The **unlabelled** minor ticks of one axis (R1528) — the per-decade
/// subdivisions on a logarithmic axis, and none on a linear or time one.
///
/// Drawn as fainter gridlines. A log axis NEEDS them: without the
/// subdivisions its evenly-spaced decade lines read as a linear axis with
/// odd labels, so they are a legibility requirement rather than a
/// decoration. A time axis has no such ambiguity — its labels name the
/// calendar directly — so it takes the linear arm's silence.
pub(crate) fn axis_minor_ticks(scale: &ValueScale) -> Vec<f64> {
    let (lo, hi) = scale.domain();
    match scale {
        ValueScale::Log(s) => log_minor_ticks(lo, hi, s.base())
            .into_iter()
            .filter(|t| in_domain(*t, lo, hi))
            .collect(),
        ValueScale::Linear(_) | ValueScale::Time(_) | ValueScale::Category(_) => Vec::new(),
    }
}

/// Resolve which data point an x-`fraction` scrub is focused on: the focus x
/// (the series point x nearest the cursor across every series) plus the nearest
/// visible point of each series. `fraction` is `0.0..=1.0` across `rect`'s
/// width; it maps through the plot margins + domain to a data x, then each
/// series contributes its [`nearest_point_in`] that x.
///
/// The single source both the line and scatter charts' scrub inspectors read
/// (R1377), so the painted overlay and the a11y readout can never point at
/// different data. Returns `None` when the plot is degenerate or no series has
/// a point in the visible x range.
pub(crate) fn resolve_focus(
    series: &[Series],
    fraction: f32,
    plot: &CartesianPlot,
    rect: Rect,
) -> Option<(f64, Vec<(usize, DataPoint)>)> {
    let span = plot.right - plot.left;
    if span <= 0.0 {
        return None;
    }
    // chart-rect fraction -> plot pixel -> data x. R1528 inverts through the
    // axis rather than interpolating the domain linearly: the two agree on a
    // linear axis (clamping the pixel is clamping the fraction), and only the
    // inversion is right on a log one, where the scrub is not affine in the
    // data. One scrub source keeps the ring and the readout on the same point.
    let cursor_px = to_f32(rect.x) + fraction.clamp(0.0, 1.0) * to_f32(rect.w);
    let (x_lo, x_hi) = plot.x.domain();
    let data_x = plot.x.invert(cursor_px.clamp(plot.left, plot.right));

    // Nearest point per series + the overall focus x (the series point nearest
    // the cursor across every series).
    let mut focus_x: Option<f64> = None;
    let mut hits: Vec<(usize, DataPoint)> = Vec::new();
    for (i, s) in series.iter().enumerate() {
        // A hidden series is not inspectable — skip it so the scrub never rings /
        // reports a point whose geometry was dropped (R1379; the R1377
        // visible_hits class, applied at the focus source).
        if !s.visible {
            continue;
        }
        if let Some(p) = nearest_point_in(s, data_x, x_lo, x_hi, |p| plot.defines(p)) {
            let better = focus_x.is_none_or(|fx| (p.x - data_x).abs() < (fx - data_x).abs());
            if better {
                focus_x = Some(p.x);
            }
            hits.push((i, p));
        }
    }
    Some((focus_x?, hits))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Vec<Series> {
        vec![
            Series::new(
                "a",
                vec![DataPoint::new(0.0, 0.0), DataPoint::new(10.0, 10.0)],
            ),
            Series::new(
                "b",
                vec![DataPoint::new(0.0, 5.0), DataPoint::new(10.0, 5.0)],
            ),
        ]
    }

    // ---- R1545: the categorical axis, resolved as one ---------------------

    fn category_kinds() -> AxisKinds {
        AxisKinds {
            x: AxisKind::Category(crate::Categories::new(["a", "b", "c", "d"])),
            y: AxisKind::default(),
        }
    }

    /// ★ A categorical axis's extent is its category list, not its data —
    /// the property that separates a slot axis from a value axis. The
    /// counterfactual is in the same test: the LINEAR axis over the identical
    /// data does move with it, so "ignore the data" is not something every
    /// kind does.
    #[test]
    fn r1545_a_category_axis_domains_from_its_list_not_its_data() {
        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        // One point, at category 1. A value axis would collapse onto it.
        let series = vec![Series::new("s", vec![DataPoint::new(1.0, 5.0)])];

        let plot = CartesianPlot::resolve(
            rect,
            &series,
            None,
            None,
            &style,
            Rescale::default(),
            &category_kinds(),
        );
        assert_eq!(plot.x.domain(), (-0.5, 3.5), "all four slots are in view");

        let linear = CartesianPlot::resolve(
            rect,
            &series,
            None,
            None,
            &style,
            Rescale::default(),
            &AxisKinds::default(),
        );
        assert_ne!(
            linear.x.domain(),
            (-0.5, 3.5),
            "a linear x DOES follow the data, so the category arm is doing work"
        );
    }

    /// ★ The ticks of a category axis are its visible slots and its labels
    /// are their names — resolved through the same `axis_ticks` / `axis_format`
    /// pair every other kind uses, so a chart cannot label a category axis
    /// with numbers by taking the wrong branch.
    #[test]
    fn r1545_category_ticks_are_the_visible_slots_labelled_by_name() {
        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        let plot = CartesianPlot::resolve(
            rect,
            &[],
            None,
            None,
            &style,
            Rescale::default(),
            &category_kinds(),
        );
        let ticks = axis_ticks(&plot.x, style.x_ticks);
        assert_eq!(ticks, vec![0.0, 1.0, 2.0, 3.0], "one tick per slot");
        let format = axis_format(&plot.x, &ticks);
        let labels: Vec<String> = ticks.iter().map(|&t| format.label(t)).collect();
        assert_eq!(labels, vec!["a", "b", "c", "d"]);
        assert_eq!(format.readout(2.0), "c", "a name is already self-contained");
        // A category axis has no minor subdivisions — a slot has no interior.
        assert!(axis_minor_ticks(&plot.x).is_empty());
        // The tick target is deliberately not applied: four slots survive a
        // target of one, because thinning a category axis is a label-width
        // decision the scale cannot make.
        assert_eq!(axis_ticks(&plot.x, 1).len(), 4);

        // Windowed: only the visible slots tick, and they keep their names.
        let windowed = CartesianPlot::resolve(
            rect,
            &[],
            Some((0.5, 2.5)),
            None,
            &style,
            Rescale::default(),
            &category_kinds(),
        );
        let ticks = axis_ticks(&windowed.x, style.x_ticks);
        assert_eq!(ticks, vec![1.0, 2.0]);
        let format = axis_format(&windowed.x, &ticks);
        assert_eq!(format.label(1.0), "b");
    }

    /// ★ A sample naming no slot is REPORTED, not drawn somewhere invented —
    /// the R1528 log-zero stance on the arm where "no such slot" is the
    /// failure. The counterfactual: the in-range samples of the same series
    /// are not reported, so this is not "a category axis reports everything".
    #[test]
    fn r1545_a_sample_naming_no_slot_is_reported() {
        let series = vec![Series::new(
            "s",
            vec![
                DataPoint::new(0.0, 1.0),
                DataPoint::new(3.0, 2.0),
                DataPoint::new(9.0, 3.0),
                DataPoint::new(-2.0, 4.0),
            ],
        )];
        let off = off_scale_points(&series, &category_kinds());
        assert_eq!(off.len(), 2, "only the two that name no slot: {off:?}");
        let xs: Vec<f64> = off.iter().map(|o| o.point.x).collect();
        assert_eq!(xs, vec![9.0, -2.0]);
        assert!(off.iter().all(|o| o.series == 0));
        // On a linear x the same samples are ordinary — off-scale is a
        // property of the AXIS, not of the data.
        assert!(off_scale_points(&series, &AxisKinds::default()).is_empty());
    }

    #[test]
    fn resolve_focus_skips_hidden_series() {
        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        let plot = CartesianPlot::resolve(
            rect,
            &two(),
            None,
            None,
            &style,
            Rescale::default(),
            &AxisKinds::default(),
        );

        // Both visible -> a hit per series.
        let (_fx, hits) = resolve_focus(&two(), 0.5, &plot, rect).expect("focus with two visible");
        assert_eq!(hits.len(), 2, "both visible series produce a hit");

        // Hide series 0 -> only series 1 hits, and it keeps its original index.
        let mut hidden = two();
        hidden[0].visible = false;
        let (_fx, hits) = resolve_focus(&hidden, 0.5, &plot, rect).expect("focus with one visible");
        assert_eq!(hits.len(), 1, "a hidden series is not a focus hit");
        assert_eq!(
            hits[0].0, 1,
            "the surviving hit keeps its original series index"
        );

        // All hidden -> no focus at all (the scrub overlay then paints nothing).
        let all_hidden: Vec<Series> = two().into_iter().map(|s| s.with_visible(false)).collect();
        assert!(
            resolve_focus(&all_hidden, 0.5, &plot, rect).is_none(),
            "no focus when every series is hidden"
        );
    }

    #[test]
    fn rescale_to_visible_moves_the_auto_domain_off_a_hidden_series() {
        // A dominant series to y=4000 plus a small one to y=8. With the big one
        // hidden, rescale_to_visible snaps the y-domain to the small series;
        // the default (all-series) keeps the full 0..4000 grid.
        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        let series = vec![
            Series::new(
                "big",
                vec![DataPoint::new(0.0, 0.0), DataPoint::new(3.0, 4000.0)],
            )
            .with_visible(false),
            Series::new(
                "small",
                vec![DataPoint::new(0.0, 2.0), DataPoint::new(3.0, 8.0)],
            ),
        ];

        let default = CartesianPlot::resolve(
            rect,
            &series,
            None,
            None,
            &style,
            Rescale::default(),
            &AxisKinds::default(),
        );
        let (_, hi_default) = default.y.domain();
        assert!(
            hi_default >= 4000.0,
            "default domain still spans the hidden big series (got hi={hi_default})"
        );

        let visible = Rescale {
            to_visible: true,
            ..Rescale::default()
        };
        let rescaled = CartesianPlot::resolve(
            rect,
            &series,
            None,
            None,
            &style,
            visible,
            &AxisKinds::default(),
        );
        let (_, hi_rescaled) = rescaled.y.domain();
        assert!(
            hi_rescaled < 100.0,
            "rescale_to_visible snaps the y-domain to the small series (got hi={hi_rescaled})"
        );

        // A pinned domain overrides rescale entirely.
        let pinned = CartesianPlot::resolve(
            rect,
            &series,
            None,
            Some((0.0, 9000.0)),
            &style,
            visible,
            &AxisKinds::default(),
        );
        assert_eq!(
            pinned.y.domain(),
            (0.0, 9000.0),
            "a pinned domain wins over rescale"
        );
    }

    #[test]
    fn rescale_y_to_x_window_fits_the_y_axis_to_the_brushed_window() {
        // A big transient at x=1 (y=1000) plus small ripples at x=5..8 (y in
        // 40..70). With the x-domain pinned to the ripple window, y_to_x_window
        // drops the transient from the y-fit so the ripples fill the axis;
        // without it the y-axis still spans the (now-off-window) transient.
        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
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
        let window = Some((5.0, 8.0));

        let off = CartesianPlot::resolve(
            rect,
            &series,
            window,
            None,
            &style,
            Rescale::default(),
            &AxisKinds::default(),
        );
        let (_, hi_off) = off.y.domain();
        assert!(
            hi_off >= 1000.0,
            "without the fit the y-axis still spans the off-window transient (got hi={hi_off})"
        );

        let fit = Rescale {
            y_to_x_window: true,
            ..Rescale::default()
        };
        let on = CartesianPlot::resolve(
            rect,
            &series,
            window,
            None,
            &style,
            fit,
            &AxisKinds::default(),
        );
        let (_, hi_on) = on.y.domain();
        assert!(
            hi_on < 200.0,
            "the fit snaps the y-axis to the window's ripples (got hi={hi_on})"
        );

        // A full-width x-domain includes every point, so the fit is a no-op:
        // the transient is back in-window and the y-axis spans it again.
        let full = CartesianPlot::resolve(
            rect,
            &series,
            Some((0.0, 8.0)),
            None,
            &style,
            fit,
            &AxisKinds::default(),
        );
        let (_, hi_full) = full.y.domain();
        assert!(
            hi_full >= 1000.0,
            "a full-width x-window fits nothing away (got hi={hi_full})"
        );

        // A window with no points leaves the y-domain on the full data rather
        // than collapsing the axis.
        let empty = CartesianPlot::resolve(
            rect,
            &series,
            Some((2.0, 4.0)),
            None,
            &style,
            fit,
            &AxisKinds::default(),
        );
        let (_, hi_empty) = empty.y.domain();
        assert!(
            hi_empty >= 1000.0,
            "an empty window keeps the full-data y-domain (got hi={hi_empty})"
        );

        // A pinned y-domain still wins over the window fit.
        let pinned = CartesianPlot::resolve(
            rect,
            &series,
            window,
            Some((0.0, 500.0)),
            &style,
            fit,
            &AxisKinds::default(),
        );
        assert_eq!(
            pinned.y.domain(),
            (0.0, 500.0),
            "a pinned y-domain overrides the window fit"
        );
    }
}
