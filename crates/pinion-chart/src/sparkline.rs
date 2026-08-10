//! The sparkline builder — a compact, axis-less trend line for inline display
//! (R1385). A sparkline is a word-sized graphic: no axes, gridlines, ticks,
//! or legend, just the shape of a value sequence, sized to fit inside a table
//! cell or a stat tile. It is the glanceable counterpart to [`crate::line`]'s
//! full chart, and the visualization a KPI / stat-tile row needs.
//!
//! # Why a distinct builder rather than a `LineChart` mode
//!
//! [`crate::line::LineChart`] is furniture-heavy on purpose — axes, nice
//! ticks, gridlines, a legend, tooltips. A sparkline is defined by the
//! ABSENCE of all of that plus inline affordances a full chart does not have:
//! an end-cap dot marking the latest value, and optional min / max reference
//! dots. Rather than thread a dozen "off" flags through the line chart, a
//! sparkline is its own small builder over the shared leaf + scale core it
//! reuses verbatim: the value [`LinearScale`], and the retained
//! [`crate::draw`] primitives (`stroke_path` / `area_path` / `marker_node` /
//! `box_node`). Its input is a bare `Vec<f64>` (x is the implicit index) — the
//! natural shape of a trend, not a series of `(x, y)` points.
//!
//! # Introspection
//!
//! Every node carries a tag under `tag_prefix` (default `"spark"`): `spark.bg`
//! (only when a background is set), `spark.area` (only when [`filled`]), the
//! `spark.line` polyline, and — only when [`with_markers`] — `spark.min` /
//! `spark.max` (the extremes, in the subdued axis tone) and `spark.end` (the
//! latest value, in the accent colour, drawn last so it sits on top). A single
//! finite value draws a lone `spark.end` dot and no line. **A row of
//! sparklines MUST give each one a distinct [`with_tag_prefix`] (e.g.
//! `spark_0`) — the default `spark` tag would otherwise collide.**
//!
//! # Coordinate contract
//!
//! Identical to [`crate::line`] / [`crate::bar`]: [`Sparkline::build_fill`] is
//! the layout-native entry point (fill-parent root, children in the chart's
//! own `(0,0)..(w,h)` frame), [`Sparkline::build`] pins to a caller rect. As
//! with every chart type in this crate the geometry is `Scene::Path`, so a
//! sparkline does not render on the TUI backend (§2 #6, crate-level "Known
//! limitations").
//!
//! [`filled`]: Sparkline::filled
//! [`with_markers`]: Sparkline::with_markers
//! [`with_tag_prefix`]: Sparkline::with_tag_prefix

use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{Color, Stroke};

use crate::draw::{absolute, area_path, box_node, fill_parent, marker_node, stroke_path, to_f32};
use crate::palette::CategoricalPalette;
use crate::scale::LinearScale;
use crate::style::ChartStyle;

/// The sparkline polyline width in pixels.
const SPARK_STROKE_W: u32 = 2;

/// The alpha the area fill is drawn at (a faint wash under the line, so the
/// line stays the dominant mark).
const SPARK_FILL_ALPHA: u8 = 0x3D;

/// A compact, axis-less trend line over a `Vec<f64>` value sequence (x is the
/// implicit index), sharing the crate's scale + draw core with
/// [`crate::line::LineChart`].
pub struct Sparkline {
    values: Vec<f64>,
    color: Option<Color>,
    fill_area: bool,
    markers: bool,
    tag_prefix: String,
}

impl Sparkline {
    /// A sparkline over `values` (x = index): a bare line, no fill, no markers,
    /// the first palette colour, and the `"spark"` tag prefix.
    #[must_use]
    pub fn new(values: Vec<f64>) -> Self {
        Self {
            values,
            color: None,
            fill_area: false,
            markers: false,
            tag_prefix: "spark".to_string(),
        }
    }

    /// Set the line (and end-cap) colour. Defaults to the first categorical
    /// palette colour when unset.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Paint a faint area fill under the line (an area sparkline).
    #[must_use]
    pub fn filled(mut self, fill: bool) -> Self {
        self.fill_area = fill;
        self
    }

    /// Draw the reference dots: `spark.min` / `spark.max` at the extremes (in
    /// the subdued axis tone) and `spark.end` at the latest value (in the line
    /// colour). Off by default — a bare sparkline is just the line.
    #[must_use]
    pub fn with_markers(mut self, markers: bool) -> Self {
        self.markers = markers;
        self
    }

    /// Override the introspection tag prefix (default `"spark"`). REQUIRED to
    /// be distinct per sparkline when several share one scene (a stat-tile
    /// row), else their `spark.*` tags collide.
    #[must_use]
    pub fn with_tag_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.tag_prefix = prefix.into();
        self
    }

    /// The values this sparkline was built with.
    #[must_use]
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// Build the sparkline PINNED to `rect`. See [`crate::line::LineChart::build`].
    #[must_use]
    pub fn build(&self, rect: Rect, style: &ChartStyle) -> Scene {
        Scene::Container(
            self.build_body(Rect::new(0, 0, rect.w, rect.h), style)
                .with_layout(absolute(rect)),
        )
    }

    /// Build the sparkline as a layout-native subtree (R1360 contract): the
    /// root fills its slot; `(0,0)` returns an empty tagged root that still
    /// measures. See [`crate::line::LineChart::build_fill`].
    #[must_use]
    pub fn build_fill(&self, size: (u32, u32), style: &ChartStyle) -> Scene {
        let (w, h) = size;
        let body = if w == 0 || h == 0 {
            ContainerNode::new(Vec::new()).with_tag(self.tag_prefix.clone())
        } else {
            self.build_body(Rect::new(0, 0, w, h), style)
        };
        Scene::Container(body.with_layout(fill_parent()))
    }

    /// The sparkline body, authored in `rect`'s frame — the ONE builder both
    /// entry points wrap.
    fn build_body(&self, rect: Rect, style: &ChartStyle) -> ContainerNode {
        let mut children: Vec<Scene> = Vec::new();
        if let Some(bg) = style.background {
            children.push(box_node(rect, bg, format!("{}.bg", self.tag_prefix)));
        }

        if let Some(g) = self.geom(rect, style) {
            let color = self
                .color
                .unwrap_or_else(|| CategoricalPalette::default().color(0));

            // A faint area wash under the line (an area sparkline).
            if self.fill_area && g.points.len() >= 2 {
                children.push(area_path(
                    &g.points,
                    g.baseline_y,
                    // R1628 — a sparkline states no interpolation, so it takes
                    // the one that invents nothing. Explicit rather than
                    // defaulted, so adding the choice here is a decision
                    // someone makes rather than one that happens.
                    crate::Interpolation::Linear,
                    color.with_alpha(SPARK_FILL_ALPHA),
                    format!("{}.area", self.tag_prefix),
                ));
            }
            // The trend polyline (needs at least two points).
            if g.points.len() >= 2 {
                children.push(stroke_path(
                    &g.points,
                    Stroke::new(color, SPARK_STROKE_W),
                    format!("{}.line", self.tag_prefix),
                ));
            }

            // Reference dots. `min` / `max` in the subdued axis tone (they are
            // context), `end` in the accent colour drawn LAST (it is the point
            // the eye should land on). A lone value still shows its end dot so
            // the sparkline is not blank.
            let r = style.marker_radius.max(1);
            if self.markers {
                if let Some(&(mx, my)) = g.points.get(g.min_idx) {
                    children.push(marker_node(
                        mx,
                        my,
                        r,
                        style.axis,
                        format!("{}.min", self.tag_prefix),
                    ));
                }
                if let Some(&(mx, my)) = g.points.get(g.max_idx) {
                    children.push(marker_node(
                        mx,
                        my,
                        r,
                        style.axis,
                        format!("{}.max", self.tag_prefix),
                    ));
                }
                if let Some(&(ex, ey)) = g.points.last() {
                    children.push(marker_node(
                        ex,
                        ey,
                        r,
                        color,
                        format!("{}.end", self.tag_prefix),
                    ));
                }
            } else if g.points.len() == 1 {
                if let Some(&(ex, ey)) = g.points.last() {
                    children.push(marker_node(
                        ex,
                        ey,
                        r,
                        color,
                        format!("{}.end", self.tag_prefix),
                    ));
                }
            }
        }

        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
    }

    /// Resolve the pixel points + baseline + extreme indices, or `None` when
    /// there is no finite value to plot.
    #[allow(
        clippy::cast_precision_loss,
        reason = "the value index is a small display count, exact in f32"
    )]
    fn geom(&self, rect: Rect, style: &ChartStyle) -> Option<SparkGeom> {
        let n = self.values.len();
        if n == 0 {
            return None;
        }
        // Inset by the marker radius so the end / min / max dots never clip the
        // edge; a bare (marker-less) sparkline still keeps 1px of breathing room.
        let pad = if self.markers {
            to_f32(style.marker_radius.max(1)) + 1.0
        } else {
            1.0
        };
        let left = to_f32(rect.x) + pad;
        let right = (to_f32(rect.x + rect.w) - pad).max(left);
        let top = to_f32(rect.y) + pad;
        let bottom = (to_f32(rect.y + rect.h) - pad).max(top);

        // The finite value range. A flat series (all equal, or a single value)
        // is centred vertically by widening the domain symmetrically.
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &v in &self.values {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        if !lo.is_finite() {
            return None;
        }
        if (hi - lo).abs() <= f64::EPSILON {
            lo -= 0.5;
            hi += 0.5;
        }
        let y = LinearScale::new((lo, hi), (bottom, top));

        let span = right - left;
        let denom = if n > 1 { (n - 1) as f32 } else { 1.0 };
        let mut points: Vec<(f32, f32)> = Vec::new();
        let (mut min_idx, mut max_idx) = (0usize, 0usize);
        let (mut min_v, mut max_v) = (f64::INFINITY, f64::NEG_INFINITY);
        for (i, &v) in self.values.iter().enumerate() {
            if !v.is_finite() {
                continue;
            }
            let px = if n > 1 {
                left + (i as f32 / denom) * span
            } else {
                f32::midpoint(left, right)
            };
            points.push((px, y.map(v)));
            if v < min_v {
                min_v = v;
                min_idx = points.len() - 1;
            }
            if v > max_v {
                max_v = v;
                max_idx = points.len() - 1;
            }
        }
        if points.is_empty() {
            return None;
        }
        Some(SparkGeom {
            points,
            baseline_y: bottom,
            min_idx,
            max_idx,
        })
    }
}

/// The resolved sparkline geometry: the pixel points, the area baseline, and
/// which point is the min / max (for the reference dots).
struct SparkGeom {
    points: Vec<(f32, f32)>,
    baseline_y: f32,
    min_idx: usize,
    max_idx: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_probe::find;
    use pinion_core::scene::Rect;

    const RECT: Rect = Rect::new(0, 0, 120, 40);

    fn path_rect(scene: &Scene, tag: &str) -> Rect {
        match find(scene, tag).unwrap_or_else(|| panic!("{tag} present")) {
            Scene::Path(p) => p.rect,
            other => panic!("{tag} is a path, got {other:?}"),
        }
    }

    fn ramp() -> Vec<f64> {
        vec![10.0, 30.0, 20.0, 45.0, 25.0, 60.0]
    }

    #[test]
    fn a_multi_point_series_draws_one_polyline() {
        let scene = Sparkline::new(ramp()).build(RECT, &ChartStyle::default());
        assert!(
            find(&scene, "spark.line").is_some(),
            "the trend line is drawn"
        );
    }

    #[test]
    fn a_bare_sparkline_has_no_axes_ticks_or_legend() {
        // The whole point of a sparkline: none of the line chart's furniture.
        let scene = Sparkline::new(ramp()).build(RECT, &ChartStyle::default());
        for absent in [
            "chart.axis.x",
            "chart.axis.y",
            "spark.legend",
            "spark.grid.y.0",
        ] {
            assert!(find(&scene, absent).is_none(), "{absent} must be absent");
        }
    }

    #[test]
    fn markers_off_by_default() {
        let scene = Sparkline::new(ramp()).build(RECT, &ChartStyle::default());
        for tag in ["spark.end", "spark.min", "spark.max"] {
            assert!(
                find(&scene, tag).is_none(),
                "{tag} absent without with_markers"
            );
        }
    }

    #[test]
    fn with_markers_draws_end_min_and_max_dots() {
        let scene = Sparkline::new(ramp())
            .with_markers(true)
            .build(RECT, &ChartStyle::default());
        for tag in ["spark.end", "spark.min", "spark.max"] {
            assert!(find(&scene, tag).is_some(), "{tag} drawn with markers");
        }
    }

    #[test]
    fn the_end_dot_sits_at_the_rightmost_point() {
        // The end cap marks the LATEST value -> the last index -> the right edge.
        let scene = Sparkline::new(ramp())
            .with_markers(true)
            .build(RECT, &ChartStyle::default());
        let end = path_rect(&scene, "spark.end");
        let line = path_rect(&scene, "spark.line");
        // The end dot's centre is near the line bbox's right edge.
        let end_cx = end.x + end.w / 2;
        assert!(
            end_cx + 6 >= line.x + line.w,
            "the end dot is at the right edge (end_cx {end_cx} vs line right {})",
            line.x + line.w
        );
    }

    #[test]
    fn the_max_dot_is_higher_on_screen_than_the_min_dot() {
        // Screen y grows downward, so the MAX value (highest) has the SMALLER y.
        let scene = Sparkline::new(ramp())
            .with_markers(true)
            .build(RECT, &ChartStyle::default());
        let max = path_rect(&scene, "spark.max");
        let min = path_rect(&scene, "spark.min");
        assert!(
            max.y < min.y,
            "the max dot ({}) sits above the min dot ({})",
            max.y,
            min.y
        );
    }

    #[test]
    fn area_only_when_filled() {
        let bare = Sparkline::new(ramp()).build(RECT, &ChartStyle::default());
        assert!(find(&bare, "spark.area").is_none(), "no area by default");
        let filled = Sparkline::new(ramp())
            .filled(true)
            .build(RECT, &ChartStyle::default());
        assert!(
            find(&filled, "spark.area").is_some(),
            "filled draws an area"
        );
    }

    #[test]
    fn a_single_value_draws_a_dot_not_a_line() {
        let scene = Sparkline::new(vec![42.0]).build(RECT, &ChartStyle::default());
        assert!(
            find(&scene, "spark.line").is_none(),
            "one point is not a line"
        );
        assert!(
            find(&scene, "spark.end").is_some(),
            "one point shows its dot"
        );
    }

    #[test]
    fn empty_is_an_empty_tagged_root() {
        let scene = Sparkline::new(vec![]).build(RECT, &ChartStyle::default());
        let Scene::Container(root) = &scene else {
            panic!("the root is a container")
        };
        assert_eq!(root.tag.as_deref(), Some("spark"));
        assert!(root.children.is_empty(), "nothing to draw with no data");
    }

    #[test]
    fn a_flat_series_still_draws_a_horizontal_line() {
        // All-equal values must not divide-by-zero or vanish — they centre.
        let scene = Sparkline::new(vec![7.0, 7.0, 7.0]).build(RECT, &ChartStyle::default());
        assert!(
            find(&scene, "spark.line").is_some(),
            "a flat series still draws"
        );
    }

    #[test]
    fn a_distinct_tag_prefix_renames_every_node() {
        let scene = Sparkline::new(ramp())
            .filled(true)
            .with_markers(true)
            .with_tag_prefix("spark_2")
            .build(RECT, &ChartStyle::default());
        for tag in ["spark_2", "spark_2.line", "spark_2.area", "spark_2.end"] {
            assert!(
                find(&scene, tag).is_some(),
                "{tag} present under the prefix"
            );
        }
        assert!(
            find(&scene, "spark.line").is_none(),
            "no default prefix leaks (row-collision guard)"
        );
    }
}
