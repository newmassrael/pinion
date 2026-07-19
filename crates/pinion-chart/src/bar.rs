//! The bar chart builder — projects a list of labelled [`Bar`]s into a
//! retained [`Scene`] of a value y-axis (nice ticks, gridlines, labels), a
//! category x-axis, and one filled box per bar.
//!
//! # Why a second builder rather than a `LineChart` mode
//!
//! A bar chart's x-axis is CATEGORICAL (evenly spaced discrete slots), not
//! the numeric [`LinearScale`] a line chart interpolates along — so the bar
//! layout genuinely diverges. It shares the crate's leaf + scaling core with
//! [`crate::line`]: the value [`LinearScale`], the Heckbert nice ticks, the
//! [`plot_rect`](crate::draw) margin math, the categorical palette, and the
//! retained draw primitives ([`crate::draw`] — a bar IS a `box_node`, an axis a
//! `stroke_path`, a tick a `label_node`). It still re-derives the mid-level
//! ASSEMBLY that sits on that core — the horizontal gridlines, the two axes,
//! and the y-tick-label loop are near-identical to [`crate::line`]'s; that
//! duplication is deliberately deferred, not lifted, until a third chart type
//! shows the right axis-furniture API (an axis helper fitted to only line + bar
//! risks being wrong for a scatter / donut). Both builders emit the same
//! tagged, layout-native `Scene`, so a bar chart docks / flexes / resizes and
//! reads back over §2 #7 introspection exactly as a line chart does.
//!
//! # Introspection
//!
//! Every node carries a tag under `tag_prefix` (default `"chart"`):
//! `chart.bg`, `chart.grid.y.{k}`, `chart.axis.x` / `chart.axis.y`,
//! `chart.bar.{i}`, `chart.label.y.{k}` (the numeric y-tick labels, the same
//! tag [`crate::line`] uses), and `chart.xlabel.{i}` (the one-per-CATEGORY x
//! labels — deliberately NOT line's `label.x.{k}`, which are one-per-numeric-
//! tick: different cardinality, different meaning). A bar with a non-finite
//! value emits no `chart.bar.{i}` node (present-if-finite).
//!
//! # Coordinate contract
//!
//! Identical to [`crate::line`]: [`BarChart::build_fill`] is the
//! **layout-native** entry point (fill-parent root, children in the chart's
//! own `(0,0)..(w,h)` frame), [`BarChart::build`] pins to a caller rect. Read
//! the crate-level "Known limitations": the `build_fill` measured-rect seam is
//! Vello-only, and `Scene::Path` (the axes / gridlines) does not render on TUI.

use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{Color, Stroke, TextAlign};

use crate::draw::{absolute, box_node, fill_parent, label_node, plot_rect, stroke_path, to_u32};
use crate::palette::CategoricalPalette;
use crate::scale::LinearScale;
use crate::style::ChartStyle;
use crate::ticks::{format_axis_tick, nice_ticks, tick_step};

/// The fraction of each bar's slot left empty as the inter-bar gap (so
/// adjacent bars read as distinct). `0.2` = a bar fills 80% of its slot.
const BAR_GAP_FRAC: f32 = 0.2;

/// One bar: a category `label`, its `value`, and an optional per-bar `color`
/// override (else the chart's palette colour). The override is what lets a
/// histogram paint its over-budget bins in a distinct colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    /// The category label, centred under the bar on the x-axis.
    pub label: String,
    /// The bar's value on the y-axis.
    pub value: f64,
    /// An optional colour override for THIS bar.
    pub color: Option<Color>,
}

impl Bar {
    /// A bar with the default (palette) colour.
    #[must_use]
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        Self {
            label: label.into(),
            value,
            color: None,
        }
    }

    /// Override this bar's colour (e.g. an over-budget histogram bin).
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

/// A vertical bar chart over labelled [`Bar`]s, sharing the crate's
/// scale / ticks / palette / draw core with [`crate::line::LineChart`].
pub struct BarChart {
    bars: Vec<Bar>,
    palette: CategoricalPalette,
    y_domain: Option<(f64, f64)>,
    tag_prefix: String,
}

impl BarChart {
    /// A bar chart over `bars`, using the default palette, an auto y-domain
    /// (baseline `0` to the data max, nice-tick-snapped), and the `"chart"`
    /// tag prefix.
    #[must_use]
    pub fn new(bars: Vec<Bar>) -> Self {
        Self {
            bars,
            palette: CategoricalPalette::default(),
            y_domain: None,
            tag_prefix: "chart".to_string(),
        }
    }

    /// Override the default bar-colour palette (a bar's own
    /// [`with_color`](Bar::with_color) still wins).
    #[must_use]
    pub fn with_palette(mut self, palette: CategoricalPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Pin the y-axis domain instead of deriving it from the data. Pinning
    /// `(0, hi)` keeps the baseline at zero regardless of the data minimum.
    #[must_use]
    pub fn with_y_domain(mut self, lo: f64, hi: f64) -> Self {
        self.y_domain = Some((lo, hi));
        self
    }

    /// Override the intent/introspection tag prefix (default `"chart"`).
    #[must_use]
    pub fn with_tag_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.tag_prefix = prefix.into();
        self
    }

    /// The bars this chart was built with.
    #[must_use]
    pub fn bars(&self) -> &[Bar] {
        &self.bars
    }

    /// Build the chart PINNED to `rect` (the caller states the geometry). See
    /// [`crate::line::LineChart::build`] — same contract; prefer
    /// [`Self::build_fill`] for anything layout-placed.
    #[must_use]
    pub fn build(&self, rect: Rect, style: &ChartStyle) -> Scene {
        Scene::Container(
            self.build_body(Rect::new(0, 0, rect.w, rect.h), style)
                .with_layout(absolute(rect)),
        )
    }

    /// Build the chart as a **layout-native** subtree (R1360 contract): the
    /// root fills its slot; children are authored in the chart's own
    /// `(0,0)..(w,h)` frame. `(0,0)` returns an empty tagged root that still
    /// measures, so the size feeds back on the next paint (the `build_fill`
    /// bootstrap). See [`crate::line::LineChart::build_fill`].
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

    /// The chart body, authored in `rect`'s frame — the ONE builder both
    /// entry points wrap (the `build`/`build_fill` split of R1360.4).
    #[allow(
        clippy::cast_precision_loss,
        reason = "the bar index / count is a small display count, well under 2^24 where f32 is exact"
    )]
    fn build_body(&self, rect: Rect, style: &ChartStyle) -> ContainerNode {
        let (left, right, top, bottom) = plot_rect(rect, style.margin);

        // Auto domain: baseline 0 to the data max (a bar reads against zero),
        // snapped to the nice-tick extent so the outer gridlines land on the
        // plot edges; a pinned domain is honoured verbatim.
        let (y_lo, y_hi) = self.y_domain_snapped(style.y_ticks);
        let y = LinearScale::new((y_lo, y_hi), (bottom, top));
        let y_ticks = nice_ticks(y_lo, y_hi, style.y_ticks);
        let y_step = tick_step(&y_ticks);
        // The baseline the bars grow from — 0 when it is in the domain, else
        // the nearer domain edge.
        let baseline_y = y.map(0.0_f64.clamp(y_lo, y_hi));

        let mut children: Vec<Scene> = Vec::new();
        if let Some(bg) = style.background {
            children.push(box_node(rect, bg, format!("{}.bg", self.tag_prefix)));
        }
        // Horizontal gridlines, one per y-tick.
        let grid = Stroke::new(style.grid, 1);
        for (k, &t) in y_ticks.iter().enumerate() {
            let gy = y.map(t);
            children.push(stroke_path(
                &[(left, gy), (right, gy)],
                grid,
                format!("{}.grid.y.{k}", self.tag_prefix),
            ));
        }
        // Axes.
        let axis = Stroke::new(style.axis, 1);
        children.push(stroke_path(
            &[(left, top), (left, bottom)],
            axis,
            format!("{}.axis.y", self.tag_prefix),
        ));
        children.push(stroke_path(
            &[(left, bottom), (right, bottom)],
            axis,
            format!("{}.axis.x", self.tag_prefix),
        ));

        // Bars — evenly spaced slots across the plot width, each bar centred
        // in its slot and grown from the baseline to its value.
        let n = self.bars.len();
        if n > 0 {
            let slot = (right - left).max(1.0) / n as f32;
            let bar_w = (slot * (1.0 - BAR_GAP_FRAC)).max(1.0);
            let size = style.label_size_px.max(1);
            for (i, bar) in self.bars.iter().enumerate() {
                let slot_x = left + (i as f32) * slot;
                if bar.value.is_finite() {
                    // A bar defaults to its palette colour BY INDEX (a categorical
                    // chart's distinct-per-bar colouring); a per-bar override wins
                    // (a histogram paints every bin uniform + its over-budget bins
                    // in the jank colour).
                    let color = bar.color.unwrap_or_else(|| self.palette.color(i));
                    let bx = slot_x + (slot - bar_w) / 2.0;
                    // Clamp the bar top into the plot: a value outside a PINNED
                    // `y_domain` would otherwise map past the plot edge and — a
                    // `Scene::Box` is not rect-clipped by its container — paint over
                    // the axis / gridlines / neighbours (the R1356 non-containment
                    // the line chart clips series against). The auto domain always
                    // brackets the data, so this only guards a pinned domain.
                    let top_y = y.map(bar.value).clamp(top, bottom);
                    // A bar above the baseline grows up (top = value, height =
                    // baseline - value); a negative bar grows down.
                    let (rect_top, rect_h) = if top_y <= baseline_y {
                        (top_y, baseline_y - top_y)
                    } else {
                        (baseline_y, top_y - baseline_y)
                    };
                    // `max(1.0)`: a zero-value bar (an empty histogram bin) still
                    // emits a 1px baseline stub, so its slot stays addressable
                    // (`chart.bar.{i}` present) and slot-aligned — distinct from a
                    // non-finite bar, which is omitted entirely. A reader tells 0
                    // from a real count by the stub's 1px height vs the y-scale.
                    children.push(box_node(
                        Rect::new(
                            to_u32(bx),
                            to_u32(rect_top),
                            to_u32(bar_w),
                            to_u32(rect_h.max(1.0)),
                        ),
                        color,
                        format!("{}.bar.{i}", self.tag_prefix),
                    ));
                }
                // Category label centred under the slot (always, even for a
                // non-finite bar, so the axis stays legible).
                children.push(label_node(
                    bar.label.clone(),
                    to_u32(slot_x),
                    to_u32(bottom) + 4,
                    to_u32(slot),
                    TextAlign::Center,
                    style.label,
                    size,
                    format!("{}.xlabel.{i}", self.tag_prefix),
                ));
            }
        }

        // Right-aligned y-axis tick labels in the left gutter.
        let size = style.label_size_px.max(1);
        let gutter = style.margin.left.saturating_sub(6).max(1);
        for (k, &t) in y_ticks.iter().enumerate() {
            let ly = to_u32(y.map(t)).saturating_sub(size / 2 + 1);
            children.push(label_node(
                format_axis_tick(t, y_step),
                rect.x + 2,
                ly,
                gutter,
                TextAlign::End,
                style.label,
                size,
                format!("{}.label.y.{k}", self.tag_prefix),
            ));
        }

        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
    }

    /// The final y-domain: a pinned domain verbatim, else `[min(0, data_min),
    /// max(0, data_max)]` snapped to its nice-tick extent (falling back to the
    /// raw range when ticks collapse). Zero is always included so a bar reads
    /// against a true baseline.
    fn y_domain_snapped(&self, target: usize) -> (f64, f64) {
        if let Some(d) = self.y_domain {
            return d;
        }
        let mut lo = 0.0_f64;
        let mut hi = 0.0_f64;
        for b in &self.bars {
            if b.value.is_finite() {
                lo = lo.min(b.value);
                hi = hi.max(b.value);
            }
        }
        if (hi - lo).abs() <= f64::EPSILON {
            hi = lo + 1.0;
        }
        let ticks = nice_ticks(lo, hi, target);
        match (ticks.first(), ticks.last()) {
            (Some(&t_lo), Some(&t_hi)) if (t_hi - t_lo).abs() > f64::EPSILON => (t_lo, t_hi),
            _ => (lo, hi),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::Rect;

    fn tags(scene: &Scene) -> Vec<String> {
        let mut out = Vec::new();
        collect(scene, &mut out);
        out
    }
    fn collect(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Container(c) => {
                if let Some(t) = c.tag.as_deref() {
                    out.push(t.to_string());
                }
                for ch in &c.children {
                    collect(ch, out);
                }
            }
            other => {
                if let Some(t) = other.tag() {
                    out.push(t.to_string());
                }
            }
        }
    }
    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(scene);
                }
                c.children.iter().find_map(|ch| find(ch, tag))
            }
            other => (other.tag() == Some(tag)).then_some(scene),
        }
    }

    fn three() -> Vec<Bar> {
        vec![
            Bar::new("a", 10.0),
            Bar::new("b", 40.0),
            Bar::new("c", 20.0),
        ]
    }

    #[test]
    fn one_bar_node_per_bar_plus_axes_and_labels() {
        let scene = BarChart::new(three()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let t = tags(&scene);
        for i in 0..3 {
            assert!(t.contains(&format!("chart.bar.{i}")), "bar {i} emitted");
            assert!(
                t.contains(&format!("chart.xlabel.{i}")),
                "x label {i} emitted"
            );
        }
        assert!(t.contains(&"chart.axis.x".to_string()));
        assert!(t.contains(&"chart.axis.y".to_string()));
        assert!(
            !t.contains(&"chart.bar.3".to_string()),
            "no phantom 4th bar"
        );
    }

    #[test]
    fn taller_value_makes_a_taller_bar() {
        let scene = BarChart::new(three()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let h = |i: usize| {
            let Scene::Box(b) = find(&scene, &format!("chart.bar.{i}")).unwrap() else {
                panic!("bar is a box")
            };
            b.rect.h
        };
        // b (40) is the max, a (10) the min -> taller value, taller bar.
        assert!(
            h(1) > h(2) && h(2) > h(0),
            "bar height tracks value (b>c>a)"
        );
    }

    #[test]
    fn a_per_bar_colour_override_wins_over_the_palette() {
        let red = Color::rgb(0xE0, 0x40, 0x40);
        let bars = vec![Bar::new("a", 10.0), Bar::new("b", 20.0).with_color(red)];
        let scene = BarChart::new(bars).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let fill = |i: usize| {
            let Scene::Box(b) = find(&scene, &format!("chart.bar.{i}")).unwrap() else {
                panic!("bar is a box")
            };
            b.style.fill
        };
        assert_eq!(fill(1), red, "the overridden bar carries its own colour");
        assert_ne!(fill(0), red, "the default bar does not");
    }

    #[test]
    fn a_non_finite_bar_emits_a_label_but_no_bar() {
        let bars = vec![Bar::new("ok", 10.0), Bar::new("nan", f64::NAN)];
        let scene = BarChart::new(bars).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let t = tags(&scene);
        assert!(t.contains(&"chart.bar.0".to_string()));
        assert!(
            !t.contains(&"chart.bar.1".to_string()),
            "no bar for a NaN value"
        );
        assert!(
            t.contains(&"chart.xlabel.1".to_string()),
            "but its label stays"
        );
    }

    #[test]
    fn build_fill_zero_size_is_an_empty_but_tagged_root() {
        let scene = BarChart::new(three()).build_fill((0, 0), &ChartStyle::default());
        let Scene::Container(root) = &scene else {
            panic!("the fill-parent root is a container")
        };
        assert_eq!(
            root.tag.as_deref(),
            Some("chart"),
            "the root still measures"
        );
        assert!(root.children.is_empty(), "no body until a size feeds back");
    }

    #[test]
    fn a_pinned_tag_prefix_renames_every_node() {
        let scene = BarChart::new(three())
            .with_tag_prefix("hist")
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let t = tags(&scene);
        assert!(t.contains(&"hist.bar.0".to_string()));
        assert!(
            t.iter().all(|s| !s.starts_with("chart.")),
            "no default prefix leaks"
        );
    }

    #[test]
    fn a_pinned_y_domain_is_honoured() {
        // Pin [0, 100]; a value of 50 lands at the vertical middle of the plot.
        let scene = BarChart::new(vec![Bar::new("m", 50.0)])
            .with_y_domain(0.0, 100.0)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let Scene::Box(bar) = find(&scene, "chart.bar.0").unwrap() else {
            panic!()
        };
        let m = ChartStyle::default().margin;
        let plot_h = 300 - m.top - m.bottom;
        // value 50 of [0,100] -> bar top at plot mid; height ~= half the plot.
        let expected_h = plot_h / 2;
        assert!(
            bar.rect.h.abs_diff(expected_h) <= 2,
            "50 of [0,100] fills ~half the plot height, got {} vs {expected_h}",
            bar.rect.h
        );
    }
}
