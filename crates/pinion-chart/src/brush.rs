//! A **brush** — a draggable x-window over a chart, wired as a sibling (or
//! primary) [`RangeSliderExternal`].
//!
//! The brush is the shared substrate behind every brush-driven consumer. The
//! *wiring* — a range external, reading its `(low, high)` fractions off the
//! scene, mapping them onto a data x-extent (with a minimum span so the
//! window never collapses), and painting the overview strip — was copied
//! verbatim across `hello-chart` (R1357 brush-zoom) and `hello-scatter`
//! (R1391 numeric cross-filter) before this lift; `hello-linked-brush` (R1394
//! cross-type cross-filter) is the third consumer that triggered it.
//!
//! What a consumer *does* with the resolved `(x_lo, x_hi)` stays in the
//! consumer's view fn — re-domain the chart ([`LineChart::with_x_domain`])
//! for a zoom, or mute the marks outside it
//! ([`LineChart::select_x_range`](crate::LineChart::select_x_range) /
//! [`ScatterChart::select_x_range`](crate::ScatterChart::select_x_range)) for
//! a cross-filter. The brush owns only the window selection.
//!
//! [`LineChart::with_x_domain`]: crate::LineChart::with_x_domain

use pinion_core::Scene;
use pinion_core::scene::{BoxNode, ContainerNode, Rect};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::range_slider::RangeSliderExternal;

/// The three colours the overview [`strip`](Brush::strip) paints with,
/// resolved by the consumer from its theme so the chart crate stays
/// theme-independent (like [`ChartStyle`](crate::ChartStyle)). The selected
/// span is the accent at a translucent alpha; the thumbs are the solid
/// accent; the track sits on `track_bg`.
#[derive(Debug, Clone, Copy)]
pub struct BrushStripColors {
    /// The inactive track behind the selected span.
    pub track_bg: Color,
    /// The accent — solid for the thumbs, translucent for the selected span.
    pub accent: Color,
}

/// A brush over a chart's x-axis, dispatched by `tag` (the sibling-external
/// idiom): its [`RangeSliderExternal`] selects a `0.0..=1.0` fraction window
/// that [`domain`](Self::domain) maps onto the data's x-extent.
#[derive(Debug, Clone)]
pub struct Brush {
    tag: &'static str,
    x_extent: (f64, f64),
    min_span: f32,
}

impl Brush {
    /// The default narrowest window, as a fraction of the x-extent — keeps
    /// the mapped domain non-degenerate even when the two thumbs coincide.
    ///
    /// R1534 — defined as [`PlotWindow::DEFAULT_MIN_SPAN`], because a plot and
    /// its own overview strip window the SAME axis and must bottom out at the
    /// same place.
    ///
    /// [`PlotWindow::DEFAULT_MIN_SPAN`]: crate::PlotWindow::DEFAULT_MIN_SPAN
    pub const DEFAULT_MIN_SPAN: f32 = crate::PlotWindow::DEFAULT_MIN_SPAN;

    /// The alpha the selected span is drawn at (the accent, translucent so
    /// the track reads through it).
    const SPAN_ALPHA: u8 = 0x66;

    /// The thumb width in pixels (each thumb straddles a span boundary).
    const THUMB_W: u32 = 4;

    /// A brush tagged `tag` over the data x-extent `x_extent`
    /// (`(x_min, x_max)`, the [`data_bounds`](crate::data_bounds) `.x`),
    /// with the [`DEFAULT_MIN_SPAN`](Self::DEFAULT_MIN_SPAN) minimum window.
    #[must_use]
    pub const fn new(tag: &'static str, x_extent: (f64, f64)) -> Self {
        Self {
            tag,
            x_extent,
            min_span: Self::DEFAULT_MIN_SPAN,
        }
    }

    /// Override the minimum window fraction (default
    /// [`DEFAULT_MIN_SPAN`](Self::DEFAULT_MIN_SPAN)).
    #[must_use]
    pub const fn with_min_span(mut self, min_span: f32) -> Self {
        self.min_span = min_span;
        self
    }

    /// This brush's dispatch tag — the `Container::tag` the [`strip`](Self::strip)
    /// carries and the external the router routes each drag to.
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        self.tag
    }

    /// The brush as a sibling [`ExtraExternal`] (the R1249 slot): one
    /// [`RangeSliderExternal`] under this brush's tag, selecting the full
    /// span at boot (so nothing is filtered until the user drags). Consumers
    /// that make the brush their *primary* external skip this and declare the
    /// range external directly.
    #[must_use]
    pub fn extras(&self) -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            self.tag,
            Box::new(RangeSliderExternal::new()),
        )]
    }

    /// Read the brush window `(low, high)` fractions from the external tagged
    /// [`tag`](Self::tag). A missing external (or one that does not
    /// introspect) falls back to the full span `(0.0, 1.0)` — no filter.
    #[must_use]
    pub fn read(&self, scene: &Scene) -> (f32, f32) {
        let Some(node) = scene.find_external_with_tag(self.tag) else {
            return (0.0, 1.0);
        };
        let Some(intro) = node.handle.introspect() else {
            return (0.0, 1.0);
        };
        let read = |field: &str, fallback: f32| {
            intro
                .query(field)
                .ok()
                .and_then(|v| v.as_f32())
                .unwrap_or(fallback)
        };
        (read("low", 0.0), read("high", 1.0))
    }

    /// Map the window fractions onto the data x-extent, normalising a
    /// reversed pair and widening a collapsed one to
    /// [`min_span`](Self::with_min_span) so the mapped domain never degenerates.
    #[must_use]
    pub fn domain(&self, low: f32, high: f32) -> (f64, f64) {
        let (low, high) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        let high = high.max(low + self.min_span).min(1.0);
        let low = low.min(high - self.min_span).max(0.0);
        // R1534 — the mapping itself lives in `window::map_window`, the one
        // statement of what a window fraction means, shared with the plot's
        // own `PlotWindow::domain`. Only the clamping above is the brush's,
        // because raw thumb fractions can arrive reversed or collapsed.
        crate::window::map_window((low, high), self.x_extent)
    }

    /// The overview strip: a [`tag`](Self::tag)-carrying track (the
    /// [`RangeSliderExternal`] capture basis) holding the selected span and
    /// two thumbs, laid out to fill `track`. Align `track` to the plot's x
    /// range (its inner margin, if any) so the strip reads as an overview of
    /// the full x-axis; `low`/`high` are the raw window fractions.
    #[must_use]
    pub fn strip(
        &self,
        track: Rect,
        low: f32,
        high: f32,
        colors: BrushStripColors,
        aria_label: &'static str,
    ) -> Scene {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "fraction * track width -> px offset; both are display-bounded"
        )]
        let (span_x, span_w) = {
            let w = track.w as f32;
            let lo = (low.clamp(0.0, 1.0) * w).round() as u32;
            let hi = (high.clamp(0.0, 1.0) * w).round() as u32;
            (lo, hi.saturating_sub(lo).max(2))
        };

        let selected = Scene::Box(
            BoxNode::new(
                Rect::default(),
                BoxStyle::filled(colors.accent.with_alpha(Self::SPAN_ALPHA)),
            )
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(span_x, 0)
                    .with_size(Size::px(span_w, track.h)),
            ),
        );
        let thumb = |x: u32| {
            Scene::Box(
                BoxNode::new(
                    Rect::default(),
                    BoxStyle::filled(colors.accent).with_corner_radius(2),
                )
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(x.saturating_sub(Self::THUMB_W / 2), 0)
                        .with_size(Size::px(Self::THUMB_W, track.h)),
                ),
            )
        };

        Scene::Container(
            ContainerNode::new(vec![selected, thumb(span_x), thumb(span_x + span_w)])
                .with_tag(self.tag)
                .with_aria_label(aria_label)
                .with_style(BoxStyle::filled(colors.track_bg).with_corner_radius(track.h / 2))
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(track.x, track.y)
                        .with_size(Size::px(track.w, track.h)),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    const TAG: &str = "test_brush";

    fn brush() -> Brush {
        // A brush is tagged with a `&'static str`; the test const is one.
        Brush::new("test_brush", (0.0, 10.0))
    }

    #[test]
    fn domain_maps_fractions_onto_the_x_extent() {
        // The full span covers the whole extent; a half window covers half.
        assert_eq!(brush().domain(0.0, 1.0), (0.0, 10.0));
        assert_eq!(brush().domain(0.0, 0.5), (0.0, 5.0));
        assert_eq!(brush().domain(0.5, 1.0), (5.0, 10.0));
    }

    #[test]
    fn domain_normalises_a_reversed_pair() {
        // low > high is the same window as high < low.
        assert_eq!(brush().domain(0.8, 0.2), brush().domain(0.2, 0.8));
    }

    #[test]
    fn domain_widens_a_collapsed_window_to_min_span() {
        // Two coincident thumbs never map to a zero-width domain.
        let (lo, hi) = brush().domain(0.5, 0.5);
        assert!(hi > lo, "collapsed window widened, got {lo}..{hi}");
        let expected = f64::from(Brush::DEFAULT_MIN_SPAN) * 10.0;
        assert!(
            (hi - lo - expected).abs() < 1e-4,
            "widened to about min_span * extent, got {}",
            hi - lo
        );
    }

    #[test]
    fn read_falls_back_to_full_span_without_an_external() {
        // No `test_brush` external in the scene -> no filter.
        let empty = Scene::Container(ContainerNode::new(vec![]));
        assert_eq!(brush().read(&empty), (0.0, 1.0));
    }

    #[test]
    fn read_recovers_the_window_from_a_tagged_range_external() {
        // A real RangeSliderExternal seeded to a sub-range surfaces through
        // introspect exactly, proving the wiring the migrated consumers rely on.
        let node = Scene::External(
            ExternalNode::new(Box::new(RangeSliderExternal::with_values(0.25, 0.75))).with_tag(TAG),
        );
        let (low, high) = brush().read(&node);
        assert!((low - 0.25).abs() < 1e-6, "low read back, got {low}");
        assert!((high - 0.75).abs() < 1e-6, "high read back, got {high}");
    }

    #[test]
    fn extras_registers_one_range_external_under_the_tag() {
        let extras = brush().extras();
        assert_eq!(extras.len(), 1, "exactly one sibling external");
        assert_eq!(extras[0].tag, TAG, "under the brush tag");
    }

    #[test]
    fn strip_is_a_tagged_announced_track_with_a_span_and_two_thumbs() {
        let colors = BrushStripColors {
            track_bg: Color::rgba(0x20, 0x20, 0x20, 0xFF),
            accent: Color::rgba(0x33, 0x99, 0xFF, 0xFF),
        };
        let strip = brush().strip(
            Rect::new(10, 100, 200, 14),
            0.25,
            0.75,
            colors,
            "Test brush",
        );
        let Scene::Container(c) = &strip else {
            panic!("the strip is a Container, got {strip:?}");
        };
        assert_eq!(
            c.tag.as_deref(),
            Some(TAG),
            "the strip carries the brush tag"
        );
        assert_eq!(
            c.aria_label.as_deref(),
            Some("Test brush"),
            "the strip announces itself"
        );
        assert_eq!(
            c.children.len(),
            3,
            "the selected span + two thumbs, got {}",
            c.children.len()
        );
    }
}
