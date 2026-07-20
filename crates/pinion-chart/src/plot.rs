//! `CartesianPlot` — the resolved plotting geometry (the margin-inset plot rect
//! plus a value scale on each axis) a numeric-x / numeric-y chart maps its data
//! through.
//!
//! Lived as `line.rs`'s private `Plot` until R1377 gave the crate a THIRD
//! cartesian chart (`scatter`, after `line` and the categorical `bar`). At the
//! second numeric-x consumer a `crate::line::Plot` import would read as "the
//! scatter chart borrows the line chart's plot"; the neutral module makes it
//! shared substrate — the same reasoning that moved [`crate::style::ChartStyle`]
//! out of `line.rs` at the second chart. The categorical [`crate::bar`] keeps
//! its own `BarGeom` (a slot metric, not a numeric x-scale), so this is the
//! two-numeric-axis resolver, not a universal one.

use pinion_core::scene::Rect;

use crate::draw::{plot_rect, to_f32};
use crate::scale::LinearScale;
use crate::series::{
    DataPoint, Series, bounds_in_x_window, data_bounds, in_domain, nearest_point_in,
    visible_data_bounds,
};
use crate::style::ChartStyle;
use crate::ticks::nice_ticks;

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

/// The resolved plot rectangle (in float pixels) plus the two value scales.
#[derive(Debug, Clone, Copy)]
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
    pub x: LinearScale,
    /// Value -> pixel on the y-axis (INVERTED: domain-hi -> top / small pixel).
    pub y: LinearScale,
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
    pub(crate) fn resolve(
        rect: Rect,
        series: &[Series],
        x_domain: Option<(f64, f64)>,
        y_domain: Option<(f64, f64)>,
        style: &ChartStyle,
        rescale: Rescale,
    ) -> Self {
        let (left, right, top, bottom) = plot_rect(rect, style.margin);

        let bounds = if rescale.to_visible {
            visible_data_bounds(series).or_else(|| data_bounds(series))
        } else {
            data_bounds(series)
        };
        let raw_x = x_domain.or(bounds.map(|b| b.x)).unwrap_or((0.0, 1.0));
        // The x-window must be settled before the y-fit reads it, so resolve the
        // final x-domain first. A pinned domain is honoured verbatim; an auto one
        // snaps to the nice-tick extent so the outer gridlines land on the edges.
        let dom_x = domain_from_ticks(x_domain, raw_x, style.x_ticks);
        // y: a pinned domain wins; else an opt-in fit to the x-window's points
        // (R1397); else the whole measured extent. The window fit falls back to
        // `bounds` when it is empty, so a brush past the data keeps the grid.
        let raw_y = y_domain
            .or_else(|| {
                rescale
                    .y_to_x_window
                    .then(|| bounds_in_x_window(series, dom_x).map(|b| b.y))
                    .flatten()
            })
            .or(bounds.map(|b| b.y))
            .unwrap_or((0.0, 1.0));
        let dom_y = domain_from_ticks(y_domain, raw_y, style.y_ticks);

        Self {
            left,
            right,
            top,
            bottom,
            x: LinearScale::new(dom_x, (left, right)),
            // y is inverted: domain-hi maps to the top (small pixel).
            y: LinearScale::new(dom_y, (bottom, top)),
        }
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

/// Nice ticks for `domain`, clipped to it — the axis's own tick set (the ticks
/// that fall inside the visible range). Both the line and scatter charts derive
/// their tick labels and their gridline positions from it (R1377).
pub(crate) fn axis_ticks(domain: (f64, f64), target: usize) -> Vec<f64> {
    let (lo, hi) = domain;
    nice_ticks(lo, hi, target)
        .into_iter()
        .filter(|t| in_domain(*t, lo, hi))
        .collect()
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
    // chart-rect fraction -> plot fraction -> data x.
    let cursor_px = to_f32(rect.x) + fraction.clamp(0.0, 1.0) * to_f32(rect.w);
    let plot_frac = ((cursor_px - plot.left) / span).clamp(0.0, 1.0);
    let (x_lo, x_hi) = plot.x.domain();
    let data_x = x_lo + f64::from(plot_frac) * (x_hi - x_lo);

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
        if let Some(p) = nearest_point_in(s, data_x, x_lo, x_hi) {
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

    #[test]
    fn resolve_focus_skips_hidden_series() {
        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        let plot = CartesianPlot::resolve(rect, &two(), None, None, &style, Rescale::default());

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

        let default = CartesianPlot::resolve(rect, &series, None, None, &style, Rescale::default());
        let (_, hi_default) = default.y.domain();
        assert!(
            hi_default >= 4000.0,
            "default domain still spans the hidden big series (got hi={hi_default})"
        );

        let visible = Rescale {
            to_visible: true,
            ..Rescale::default()
        };
        let rescaled = CartesianPlot::resolve(rect, &series, None, None, &style, visible);
        let (_, hi_rescaled) = rescaled.y.domain();
        assert!(
            hi_rescaled < 100.0,
            "rescale_to_visible snaps the y-domain to the small series (got hi={hi_rescaled})"
        );

        // A pinned domain overrides rescale entirely.
        let pinned =
            CartesianPlot::resolve(rect, &series, None, Some((0.0, 9000.0)), &style, visible);
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

        let off = CartesianPlot::resolve(rect, &series, window, None, &style, Rescale::default());
        let (_, hi_off) = off.y.domain();
        assert!(
            hi_off >= 1000.0,
            "without the fit the y-axis still spans the off-window transient (got hi={hi_off})"
        );

        let fit = Rescale {
            y_to_x_window: true,
            ..Rescale::default()
        };
        let on = CartesianPlot::resolve(rect, &series, window, None, &style, fit);
        let (_, hi_on) = on.y.domain();
        assert!(
            hi_on < 200.0,
            "the fit snaps the y-axis to the window's ripples (got hi={hi_on})"
        );

        // A full-width x-domain includes every point, so the fit is a no-op:
        // the transient is back in-window and the y-axis spans it again.
        let full = CartesianPlot::resolve(rect, &series, Some((0.0, 8.0)), None, &style, fit);
        let (_, hi_full) = full.y.domain();
        assert!(
            hi_full >= 1000.0,
            "a full-width x-window fits nothing away (got hi={hi_full})"
        );

        // A window with no points leaves the y-domain on the full data rather
        // than collapsing the axis.
        let empty = CartesianPlot::resolve(rect, &series, Some((2.0, 4.0)), None, &style, fit);
        let (_, hi_empty) = empty.y.domain();
        assert!(
            hi_empty >= 1000.0,
            "an empty window keeps the full-data y-domain (got hi={hi_empty})"
        );

        // A pinned y-domain still wins over the window fit.
        let pinned = CartesianPlot::resolve(rect, &series, window, Some((0.0, 500.0)), &style, fit);
        assert_eq!(
            pinned.y.domain(),
            (0.0, 500.0),
            "a pinned y-domain overrides the window fit"
        );
    }
}
