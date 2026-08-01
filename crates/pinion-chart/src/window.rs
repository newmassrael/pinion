//! R1534 §5.38 — the **view window** over a plot's x-extent, and the two
//! gestures that move it: zoom about a point, and pan.
//!
//! ## What already existed, and what did not
//!
//! A window over the data has been here since the R1357 brush: [`Brush`]
//! resolves a `(low, high)` fraction pair onto the data extent and a consumer
//! re-domains the chart with it ([`LineChart::with_x_domain`]). But a brush is
//! an **overview strip** — a second widget below the plot, dragged by two
//! thumbs. Nothing could zoom or pan **the plot itself**, which is where
//! `QtCharts` puts it (`QChart::zoomIn` / `scroll` / `zoomReset`, driven from
//! `QChartView`) and where d3 puts it (`d3.zoom` on the plot area, then
//! `transform.rescaleX`).
//!
//! The difference is not only which pixels take the gesture. A strip cannot
//! zoom **about a point**: the reader's cursor is over the sample they care
//! about, and the whole feel of a wheel zoom is that that sample does not
//! move while the rest of the axis spreads around it. Two thumbs cannot
//! express "keep this fixed".
//!
//! ## The representation is the brush's
//!
//! [`PlotWindow`] holds the same normalised `(low, high)` pair a brush
//! resolves, and maps it with the same [`map_window`] the brush now calls —
//! one statement of "a normalised window means this data range". A plot and
//! its own overview strip disagreeing about that would be a visible bug, and
//! there is no second copy to drift.
//!
//! What [`PlotWindow`] adds is the arithmetic a gesture needs, and the
//! invariants that arithmetic has to keep: never wider than the extent, never
//! outside it, never narrower than [`PlotWindow::DEFAULT_MIN_SPAN`]. It keeps
//! them at every mutation, so — unlike the brush's raw thumb fractions, which
//! arrive from a range slider and may be reversed or collapsed — a
//! `PlotWindow` is never in an invalid state to be repaired at read time.
//!
//! [`Brush`]: crate::Brush
//! [`LineChart::with_x_domain`]: crate::LineChart::with_x_domain

use pinion_core::scene::Rect;

use crate::style::Margin;

/// R1534 §5.38 — the plotting area inside `rect`: the pixels the **axis**
/// occupies, after the [`Margin`] insets that hold the tick labels.
///
/// This is the geometry anything aligned to the axis needs, and until R1534 it
/// was `pub(crate)` — so consumers hand-derived it. `hello-chart` and
/// `hello-autoscale-y` each carry the identical
/// `rect.x + m.left` / `rect.w - (m.left + m.right)` to place their brush
/// strips under the data, and the R1534 wheel-zoom target would have been the
/// third copy: the anchor a zoom pivots about is a fraction **of the axis**,
/// so a target placed on the outer rect pivots about the wrong value by the
/// width of the y-label gutter.
///
/// [`crate::LineChart`]'s own painter is defined in terms of this function, so
/// an overlay placed here covers exactly the drawn axis.
#[must_use]
pub fn plot_area(rect: Rect, margin: Margin) -> Rect {
    let w = rect.w.saturating_sub(margin.left + margin.right).max(1);
    let h = rect.h.saturating_sub(margin.top + margin.bottom).max(1);
    Rect::new(rect.x + margin.left, rect.y + margin.top, w, h)
}

/// Map a normalised `(low, high)` window onto a data `extent`, the SSOT for
/// what a window fraction MEANS.
///
/// Called by both window owners — [`PlotWindow::domain`] and
/// [`Brush::domain`](crate::Brush::domain) (which clamps its raw thumb pair
/// first). `low`/`high` are fractions of the extent; the result is in the
/// extent's own units, ready for
/// [`LineChart::with_x_domain`](crate::LineChart::with_x_domain).
#[must_use]
pub fn map_window(window: (f32, f32), extent: (f64, f64)) -> (f64, f64) {
    let (x_min, x_max) = extent;
    let span = x_max - x_min;
    (
        x_min + f64::from(window.0) * span,
        x_min + f64::from(window.1) * span,
    )
}

/// R1534 §5.38 — a plot's view window, in fractions of its full x-extent.
///
/// `PlotWindow::full()` is the unzoomed plot. [`zoom_about`](Self::zoom_about)
/// and [`pan_by`](Self::pan_by) move the window and answer whether it
/// actually moved — the gate-by-effect return the whole codebase uses, and
/// what a wheel handler needs to decide between consuming the event and
/// handing it back (R1533: a wheel a widget cannot spend belongs to the
/// scroll container behind it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotWindow {
    low: f32,
    high: f32,
    min_span: f32,
}

impl PlotWindow {
    /// The narrowest window a gesture may reach — a 25x magnification ceiling.
    ///
    /// d3's `scaleExtent` defaults to unbounded zoom-in, which on a data
    /// domain eventually runs out of `f64` and shows a plot of nothing. This
    /// value is the R1357 brush's own minimum
    /// ([`Brush::DEFAULT_MIN_SPAN`](crate::Brush::DEFAULT_MIN_SPAN) is defined
    /// as this constant), so the two ways of windowing one axis cannot bottom
    /// out at different places. On a 25-sample series it is exactly one
    /// sample interval: "zoom until one sample fills the plot".
    pub const DEFAULT_MIN_SPAN: f32 = 0.04;

    /// The full extent — the unzoomed plot.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            low: 0.0,
            high: 1.0,
            min_span: Self::DEFAULT_MIN_SPAN,
        }
    }

    /// Override the magnification ceiling (see
    /// [`DEFAULT_MIN_SPAN`](Self::DEFAULT_MIN_SPAN)). A non-finite or
    /// non-positive `min_span` is ignored, and one above `1.0` is clamped
    /// there — a floor wider than the extent would make every window invalid.
    /// The current window is re-widened if it is now below the floor.
    #[must_use]
    pub fn with_min_span(mut self, min_span: f32) -> Self {
        if min_span.is_finite() && min_span > 0.0 {
            self.min_span = min_span.min(1.0);
            if self.span() < self.min_span {
                let mid = f32::midpoint(self.low, self.high);
                let half = self.min_span / 2.0;
                self.low = (mid - half).max(0.0);
                self.high = (self.low + self.min_span).min(1.0);
                self.low = self.high - self.min_span;
            }
        }
        self
    }

    /// The window's lower fraction.
    #[must_use]
    pub const fn low(&self) -> f32 {
        self.low
    }

    /// The window's upper fraction.
    #[must_use]
    pub const fn high(&self) -> f32 {
        self.high
    }

    /// The window's width in fractions of the extent. `1.0` is unzoomed.
    #[must_use]
    pub fn span(&self) -> f32 {
        self.high - self.low
    }

    /// Whether the window still covers the whole extent — what a consumer
    /// asks before deciding to show a "reset zoom" affordance, and before
    /// re-domaining a chart at all.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.span() >= 1.0
    }

    /// Magnify by `factor` while holding the data under `anchor` still.
    ///
    /// `anchor` is a fraction of the plot's x-axis (`0.0` = left edge, `1.0` =
    /// right), i.e. exactly the widget-relative coordinate
    /// [`External::wheel`](pinion_core::external::External::wheel) is handed.
    /// `factor > 1.0` zooms **in** (a narrower window), `< 1.0` zooms out.
    ///
    /// The anchor is what makes a wheel zoom feel right, and it is the thing
    /// an overview strip cannot express: the value under the cursor keeps its
    /// pixel while the axis spreads around it. At the extent's edges the
    /// window clamps and the anchor necessarily gives way — the same
    /// concession every map makes.
    ///
    /// Returns `false` (and changes nothing) when the window is already at a
    /// bound in the requested direction, or when `factor` is not a positive
    /// finite number.
    pub fn zoom_about(&mut self, anchor: f32, factor: f32) -> bool {
        if !factor.is_finite() || factor <= 0.0 || !anchor.is_finite() {
            return false;
        }
        let span = self.span();
        let new_span = (span / factor).clamp(self.min_span, 1.0);
        if (new_span - span).abs() < f32::EPSILON {
            return false;
        }
        let anchor = anchor.clamp(0.0, 1.0);
        // The data fraction under the anchor, which must keep its pixel.
        let pivot = self.low + anchor * span;
        let low = (pivot - anchor * new_span).clamp(0.0, 1.0 - new_span);
        self.low = low;
        self.high = low + new_span;
        true
    }

    /// Slide the window by `fraction` of its **own** width.
    ///
    /// Window-relative and not extent-relative because a pan comes from
    /// pixels: dragging (or wheeling) across a tenth of the plot should move a
    /// tenth of what is *visible*, at every zoom level. Positive moves the
    /// window toward higher values, so the plotted content travels left.
    ///
    /// Returns `false` when the window is already against that edge of the
    /// extent, or when `fraction` is not finite. An unzoomed window can never
    /// pan, and says so.
    pub fn pan_by(&mut self, fraction: f32) -> bool {
        if !fraction.is_finite() {
            return false;
        }
        let span = self.span();
        let low = (self.low + fraction * span).clamp(0.0, 1.0 - span);
        if (low - self.low).abs() < f32::EPSILON {
            return false;
        }
        self.low = low;
        self.high = low + span;
        true
    }

    /// Return to the full extent (`QtCharts`' `zoomReset`). `false` when it was
    /// already full.
    pub fn reset(&mut self) -> bool {
        if self.is_full() {
            return false;
        }
        self.low = 0.0;
        self.high = 1.0;
        true
    }

    /// This window on a data `extent`, ready for
    /// [`LineChart::with_x_domain`](crate::LineChart::with_x_domain).
    #[must_use]
    pub fn domain(&self, extent: (f64, f64)) -> (f64, f64) {
        map_window((self.low, self.high), extent)
    }
}

impl Default for PlotWindow {
    fn default() -> Self {
        Self::full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTENT: (f64, f64) = (100.0, 200.0);

    fn approx(got: f32, want: f32, what: &str) {
        assert!(
            (got - want).abs() < 1e-5,
            "{what}: expected {want}, got {got}"
        );
    }

    #[test]
    fn r1534_a_fresh_window_is_the_whole_extent() {
        let w = PlotWindow::full();
        assert!(w.is_full());
        approx(w.span(), 1.0, "span");
        let (lo, hi) = w.domain(EXTENT);
        assert!(
            (lo - 100.0).abs() < 1e-9 && (hi - 200.0).abs() < 1e-9,
            "{lo}..{hi}"
        );
    }

    #[test]
    fn r1534_zoom_about_the_middle_keeps_the_middle() {
        let mut w = PlotWindow::full();
        assert!(w.zoom_about(0.5, 2.0), "zooming in from full moves");
        approx(w.span(), 0.5, "2x magnification halves the span");
        approx(w.low(), 0.25, "centred");
        approx(w.high(), 0.75, "centred");
        let (lo, hi) = w.domain(EXTENT);
        assert!(
            (lo - 125.0).abs() < 1e-9 && (hi - 175.0).abs() < 1e-9,
            "{lo}..{hi}"
        );
    }

    #[test]
    fn r1534_the_value_under_the_anchor_does_not_move() {
        // The property an overview strip cannot express, asserted as the
        // invariant it is: whatever data value sits under the anchor before
        // the zoom sits under it after.
        for &anchor in &[0.1_f32, 0.25, 0.5, 0.75, 0.9] {
            let mut w = PlotWindow::full();
            let before = w.low() + anchor * w.span();
            assert!(w.zoom_about(anchor, 2.0));
            let after = w.low() + anchor * w.span();
            approx(after, before, &format!("pivot at anchor {anchor}"));
        }
    }

    #[test]
    fn r1534_zoom_out_returns_toward_the_full_extent() {
        let mut w = PlotWindow::full();
        assert!(w.zoom_about(0.5, 4.0));
        approx(w.span(), 0.25, "in");
        assert!(w.zoom_about(0.5, 0.5), "factor < 1 zooms out");
        approx(w.span(), 0.5, "out");
    }

    #[test]
    fn r1534_zooming_out_from_an_edge_stays_inside_the_extent() {
        // Where the clamp in `zoom_about` actually earns its place, and the
        // ONLY place: from a full window the arithmetic cannot escape the
        // extent on its own (`high = pivot + (1 - anchor) * new_span <= 1`
        // whenever the window started at `0..1`), so a test that only zooms
        // from full passes with the clamp deleted. Measured — that is exactly
        // what the counterfactual found.
        //
        // A window flush against an edge, zoomed OUT with the anchor away from
        // that edge, is the case that needs it: the anchor asks for a negative
        // low and has to give way.
        let mut w = PlotWindow::full();
        w.zoom_about(0.5, 4.0);
        w.pan_by(-10.0);
        approx(w.low(), 0.0, "premise: flush against the left edge");
        let span = w.span();
        assert!(w.zoom_about(0.9, 0.5), "zoom out, anchored near the right");
        // Both assertions are load-bearing: `low == 0.0` is also its unmoved
        // value, so alone it would pass for a zoom that never happened.
        approx(w.span(), span * 2.0, "the zoom-out happened");
        approx(w.low(), 0.0, "and the window did not leave the extent");

        let mut w = PlotWindow::full();
        w.zoom_about(0.5, 4.0);
        w.pan_by(10.0);
        approx(w.high(), 1.0, "premise: flush against the right edge");
        let span = w.span();
        assert!(w.zoom_about(0.1, 0.5), "zoom out, anchored near the left");
        approx(w.span(), span * 2.0, "the zoom-out happened");
        approx(w.high(), 1.0, "and stayed inside on that side too");
    }

    #[test]
    fn r1534_an_edge_anchored_zoom_from_full_lands_flush() {
        // The anchor at an extreme, from the full window. No clamp is involved
        // (see the test above) — what this pins is that the anchor is honoured
        // all the way to the edge rather than being pulled toward the centre.
        let mut w = PlotWindow::full();
        assert!(w.zoom_about(0.0, 2.0), "anchored at the left edge");
        approx(w.low(), 0.0, "flush left");
        approx(w.high(), 0.5, "and half a span wide");

        let mut w = PlotWindow::full();
        assert!(w.zoom_about(1.0, 2.0), "anchored at the right edge");
        approx(w.high(), 1.0, "flush right");
        approx(w.low(), 0.5, "");
    }

    #[test]
    fn r1534_zoom_stops_at_the_magnification_ceiling() {
        let mut w = PlotWindow::full();
        assert!(w.zoom_about(0.5, 1.0 / PlotWindow::DEFAULT_MIN_SPAN));
        approx(w.span(), PlotWindow::DEFAULT_MIN_SPAN, "at the floor");
        assert!(
            !w.zoom_about(0.5, 2.0),
            "a further zoom-in is declined, so a wheel handler can hand the \
             event back instead of swallowing it"
        );
        approx(w.span(), PlotWindow::DEFAULT_MIN_SPAN, "and nothing moved");
    }

    #[test]
    fn r1534_zoom_out_from_full_is_declined() {
        let mut w = PlotWindow::full();
        assert!(!w.zoom_about(0.5, 0.5), "already the whole extent");
        assert!(w.is_full());
    }

    #[test]
    fn r1534_a_custom_ceiling_is_honoured_and_re_widens_the_window() {
        let mut w = PlotWindow::full();
        assert!(w.zoom_about(0.5, 10.0));
        approx(w.span(), 0.1, "");
        let w = w.with_min_span(0.5);
        approx(w.span(), 0.5, "a wider floor re-widens the current window");
        approx(w.low(), 0.25, "about its own centre");
    }

    #[test]
    fn r1534_a_nonsense_ceiling_is_ignored() {
        let w = PlotWindow::full().with_min_span(f32::NAN);
        approx(w.span(), 1.0, "NaN ignored");
        let mut w = w.with_min_span(-1.0);
        assert!(w.zoom_about(0.5, 100.0), "still zoomable");
        approx(
            w.span(),
            PlotWindow::DEFAULT_MIN_SPAN,
            "the default floor still applies",
        );
    }

    #[test]
    fn r1534_pan_moves_a_fraction_of_what_is_visible() {
        let mut w = PlotWindow::full();
        w.zoom_about(0.5, 4.0);
        approx(w.low(), 0.375, "premise");
        assert!(w.pan_by(0.5), "half of the VISIBLE span");
        approx(w.low(), 0.375 + 0.125, "a quarter-wide window moves 0.125");
        approx(w.span(), 0.25, "and keeps its width");
    }

    #[test]
    fn r1534_pan_stops_at_the_edges() {
        let mut w = PlotWindow::full();
        w.zoom_about(0.5, 2.0);
        assert!(w.pan_by(-10.0), "a huge pan still moves as far as it can");
        approx(w.low(), 0.0, "flush against the left edge");
        assert!(!w.pan_by(-1.0), "and then declines");
        assert!(w.pan_by(10.0));
        approx(w.high(), 1.0, "flush right");
        assert!(!w.pan_by(1.0), "declines there too");
    }

    #[test]
    fn r1534_an_unzoomed_window_cannot_pan() {
        // There is nowhere to go, and saying so is what lets a plain
        // scroll-pan fall through to the page behind an unzoomed chart.
        let mut w = PlotWindow::full();
        assert!(!w.pan_by(0.5));
        assert!(!w.pan_by(-0.5));
        assert!(w.is_full());
    }

    #[test]
    fn r1534_non_finite_gestures_change_nothing() {
        let mut w = PlotWindow::full();
        w.zoom_about(0.5, 2.0);
        let before = w;
        assert!(!w.zoom_about(0.5, f32::NAN));
        assert!(!w.zoom_about(f32::NAN, 2.0));
        assert!(!w.zoom_about(0.5, 0.0));
        assert!(!w.pan_by(f32::INFINITY));
        assert_eq!(
            w, before,
            "a malformed wire payload leaves the window alone"
        );
    }

    #[test]
    fn r1534_reset_returns_to_full_and_says_whether_it_had_to() {
        let mut w = PlotWindow::full();
        assert!(!w.reset(), "already full");
        w.zoom_about(0.3, 5.0);
        assert!(w.reset(), "reset moved");
        assert!(w.is_full());
        let (lo, hi) = w.domain(EXTENT);
        assert!(
            (lo - 100.0).abs() < 1e-9 && (hi - 200.0).abs() < 1e-9,
            "{lo}..{hi}"
        );
    }

    #[test]
    fn r1534_map_window_is_the_brush_mapping() {
        // The brush's `domain` is defined in terms of this function, so the
        // plot and its own overview strip cannot disagree about what a
        // fraction means. Asserted against the brush itself, not against a
        // second copy of the arithmetic.
        let brush = crate::Brush::new("t", EXTENT);
        assert_eq!(brush.domain(0.25, 0.75), map_window((0.25, 0.75), EXTENT));
        let mut w = PlotWindow::full();
        w.zoom_about(0.5, 2.0);
        assert_eq!(w.domain(EXTENT), brush.domain(w.low(), w.high()));
    }
}
