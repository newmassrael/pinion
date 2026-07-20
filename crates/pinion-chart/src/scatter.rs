//! The scatter chart builder — projects [`Series`] data into a retained
//! [`Scene`] of axes, gridlines, tick labels, one filled circle per point, and
//! a legend.
//!
//! # The crate's third cartesian chart
//!
//! A scatter chart is a numeric-x / numeric-y chart like the [`crate::line`]
//! chart, but it plots each sample as an isolated *point* rather than joining a
//! series into a polyline — the canonical form for a *correlation* view (is y a
//! function of x?) where the samples have no order to connect. Being the third
//! cartesian chart (after `line` and the categorical `bar`) is what let the
//! shared cartesian substrate finally factor out, so this builder authors almost
//! none of its furniture itself. The plot geometry is the shared
//! [`CartesianPlot`]; the gridlines, L-frame axes, and y-tick labels are the
//! shared [`crate::draw`] primitives the line AND bar charts also call; the
//! legend row it shares with line and donut; the circle geometry (a filled
//! point mark, a stroked inspect ring) with the line chart's inspect marker.
//! What stays scatter's own is the point marks, the inspector's ring, and — a
//! two-consumer duplicate deferred from the lift, shared only with the line
//! chart — its numeric x-tick-label loop.
//!
//! # Coordinate contract
//!
//! Identical to the line chart's (see its module docs): [`ScatterChart::build`]
//! pins the chart to a window-absolute rect; [`ScatterChart::build_fill`] is the
//! layout-native entry point. Every child is authored in the chart's own
//! `(0, 0)..(w, h)` frame and the root is placed by the caller's policy, so the
//! same body serves both placements.
//!
//! # Introspection
//!
//! Every node carries a tag under the chart's `tag_prefix` (default `"chart"`):
//! `chart.bg`, `chart.grid.y.{k}` / `chart.grid.x.{k}`, `chart.axis.x` /
//! `chart.axis.y`, `chart.point.{i}.{j}` (series `i`, point `j`),
//! `chart.label.y.{k}` / `chart.label.x.{k}`, `chart.legend.{i}.swatch` /
//! `chart.legend.{i}.label`, and — when [`inspect`](ScatterChart::inspect) is
//! set — `chart.inspect.crosshair`, `chart.inspect.ring.{i}`,
//! `chart.inspect.tooltip`, `chart.inspect.header`, `chart.inspect.value.{i}`.
//! A point outside the (pinned / zoomed) domain emits no `chart.point` node, so
//! a consumer treats these tags as present-if-visible.

use core::fmt::Write as _;

use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, PathNode, Rect};
use pinion_core::style::{Color, PathStyle, Stroke, TextAlign};

use crate::draw::{
    CalloutRow, MUTED_ALPHA, absolute, box_node, callout, circle_commands, fill_parent, label_node,
    marker_node, stroke_path, to_f32, to_u32,
};
use crate::palette::CategoricalPalette;
use crate::plot::{CartesianPlot, axis_ticks, resolve_focus};
use crate::series::{DataPoint, Series, in_domain};
use crate::style::ChartStyle;
use crate::ticks::{format_axis_tick, tick_step};

/// A scatter chart: one or more [`Series`] drawn as filled point marks with
/// nice axes, gridlines, labels, and a legend, plus an optional scrub inspector.
#[derive(Debug, Clone)]
pub struct ScatterChart {
    series: Vec<Series>,
    palette: CategoricalPalette,
    x_domain: Option<(f64, f64)>,
    y_domain: Option<(f64, f64)>,
    inspect: Option<f32>,
    select_x_range: Option<(f64, f64)>,
    tag_prefix: String,
}

impl ScatterChart {
    /// A scatter chart over `series`, using the default palette, auto domains,
    /// no inspector, and the `"chart"` tag prefix.
    #[must_use]
    pub fn new(series: Vec<Series>) -> Self {
        Self {
            series,
            palette: CategoricalPalette::default(),
            x_domain: None,
            y_domain: None,
            inspect: None,
            select_x_range: None,
            tag_prefix: "chart".to_string(),
        }
    }

    /// Override the categorical series palette.
    #[must_use]
    pub fn with_palette(mut self, palette: CategoricalPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Pin the x-axis domain instead of deriving it from the data.
    #[must_use]
    pub fn with_x_domain(mut self, lo: f64, hi: f64) -> Self {
        self.x_domain = Some((lo, hi));
        self
    }

    /// Pin the y-axis domain instead of deriving it from the data.
    #[must_use]
    pub fn with_y_domain(mut self, lo: f64, hi: f64) -> Self {
        self.y_domain = Some((lo, hi));
        self
    }

    /// Show the scrub inspector (a vertical crosshair, a ring around each
    /// series' nearest point, and a value tooltip) at `fraction` — the cursor's
    /// position as a fraction `0.0..=1.0` across the chart `rect` width, exactly
    /// as [`LineChart::inspect`](crate::LineChart::inspect). `None` (the
    /// default) draws no overlay.
    #[must_use]
    pub fn inspect(mut self, fraction: Option<f32>) -> Self {
        self.inspect = fraction;
        self
    }

    /// Cross-filter the point marks to a numeric x-range: with `Some((lo, hi))`,
    /// every point whose x falls OUTSIDE `[lo, hi]` (inclusive) renders muted
    /// (its fill dimmed to a low alpha) while the in-range points stay full — the
    /// numeric-range twin of the categorical [`BarChart::select`](crate::BarChart::select)
    /// (R1384), the mark-level peer a numeric BRUSH drives (R1391). Muted points
    /// are still DRAWN (unlike a pinned [`with_x_domain`](Self::with_x_domain),
    /// which drops out-of-domain points): the out-of-range marks stay on screen as
    /// context, so a brush on one widget highlights the corresponding points here.
    /// `None` (the default) is "no filter" and every point renders full — the
    /// crossfilter convention that no selection = all data. Render-only; the
    /// caller owns the range (e.g. wired from a `RangeSlider` brush).
    #[must_use]
    pub fn select_x_range(mut self, range: Option<(f64, f64)>) -> Self {
        self.select_x_range = range;
        self
    }

    /// Override the intent/introspection tag prefix (default `"chart"`).
    #[must_use]
    pub fn with_tag_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.tag_prefix = prefix.into();
        self
    }

    /// The series this chart was built with.
    #[must_use]
    pub fn series(&self) -> &[Series] {
        &self.series
    }

    /// Build the chart PINNED to `rect` (see [`LineChart::build`](crate::LineChart::build)
    /// for the placement contract this shares).
    #[must_use]
    pub fn build(&self, rect: Rect, style: &ChartStyle) -> Scene {
        Scene::Container(
            self.build_body(Rect::new(0, 0, rect.w, rect.h), style)
                .with_layout(absolute(rect)),
        )
    }

    /// Build the chart as a **layout-native** subtree — the root fills its slot
    /// (see [`LineChart::build_fill`](crate::LineChart::build_fill), whose seam
    /// and TUI limitation this shares).
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

    /// The chart body, authored in the frame `rect` describes — the ONE builder
    /// both entry points wrap (the R1360.4 shape the line chart also uses).
    fn build_body(&self, rect: Rect, style: &ChartStyle) -> ContainerNode {
        let plot = CartesianPlot::resolve(
            rect,
            &self.series,
            self.x_domain,
            self.y_domain,
            style,
            false,
        );
        let x_ticks = axis_ticks(plot.x.domain(), style.x_ticks);
        let y_ticks = axis_ticks(plot.y.domain(), style.y_ticks);
        let steps = Steps {
            x: tick_step(&x_ticks),
            y: tick_step(&y_ticks),
        };

        // Inspect overlay, split so the crosshair paints behind the points, the
        // rings over them, and the tooltip above everything.
        let (crosshair, rings, tooltip) = match self.resolve_inspect(&plot, rect, style, steps) {
            Some(i) => (Some(i.crosshair), i.rings, i.tooltip),
            None => (None, Vec::new(), Vec::new()),
        };

        let mut children: Vec<Scene> = Vec::new();
        if let Some(bg) = style.background {
            children.push(box_node(rect, bg, format!("{}.bg", self.tag_prefix)));
        }
        // Gridlines (both axes numeric) + crosshair behind the points.
        let x_grid: Vec<f32> = x_ticks.iter().map(|&t| plot.x.map(t)).collect();
        let y_grid: Vec<f32> = y_ticks.iter().map(|&t| plot.y.map(t)).collect();
        children.extend(crate::draw::gridlines(
            (plot.left, plot.right, plot.top, plot.bottom),
            &x_grid,
            &y_grid,
            style,
            &self.tag_prefix,
        ));
        if let Some(crosshair) = crosshair {
            children.push(crosshair);
        }
        children.extend(crate::draw::axes(
            (plot.left, plot.right, plot.top, plot.bottom),
            style,
            &self.tag_prefix,
        ));
        children.extend(self.point_marks(&plot, style));
        children.extend(rings);
        children.extend(self.tick_labels(&plot, rect, &x_ticks, &y_ticks, style, steps));
        if style.legend {
            children.extend(self.legend(rect, style));
        }
        children.extend(tooltip);

        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
    }

    /// One filled circle per finite, in-domain point of every series. A point
    /// outside the (pinned) domain is dropped so it never paints past the axes.
    /// Whether a point at x `px` falls OUTSIDE the active cross-filter range (so
    /// its mark mutes). No range set = nothing muted (every point renders full).
    /// Boundaries are inclusive: a point exactly on `lo` or `hi` stays selected.
    fn is_muted(&self, px: f64) -> bool {
        matches!(self.select_x_range, Some((lo, hi)) if px < lo || px > hi)
    }

    fn point_marks(&self, plot: &CartesianPlot, style: &ChartStyle) -> Vec<Scene> {
        let radius = style.marker_radius.max(1);
        let (x_lo, x_hi) = plot.x.domain();
        let (y_lo, y_hi) = plot.y.domain();
        let mut out = Vec::new();
        for (i, s) in self.series.iter().enumerate() {
            // A hidden series draws no point marks (R1379); it keeps its palette
            // index `i` so the visible series never re-colour.
            if !s.visible {
                continue;
            }
            let color = s.color.unwrap_or_else(|| self.palette.color(i));
            for (j, p) in s.points.iter().enumerate() {
                if !p.x.is_finite()
                    || !p.y.is_finite()
                    || !in_domain(p.x, x_lo, x_hi)
                    || !in_domain(p.y, y_lo, y_hi)
                {
                    continue;
                }
                // Cross-filter (R1391): a point outside the active brush range is
                // dimmed but still drawn (context), unlike the domain drop above.
                let mark_color = if self.is_muted(p.x) {
                    color.with_alpha(MUTED_ALPHA)
                } else {
                    color
                };
                out.push(marker_node(
                    plot.x.map(p.x),
                    plot.y.map(p.y),
                    radius,
                    mark_color,
                    format!("{}.point.{i}.{j}", self.tag_prefix),
                ));
            }
        }
        out
    }

    /// Right-aligned y-axis labels (the shared [`crate::draw::y_tick_labels`])
    /// and centred numeric x-axis labels. The numeric x-label loop is the
    /// deferred twin of the line chart's (two consumers — the categorical bar
    /// chart labels its slots instead; R1377 leaves the two-consumer numeric
    /// x-label lift for a third numeric-x chart).
    fn tick_labels(
        &self,
        plot: &CartesianPlot,
        rect: Rect,
        x_ticks: &[f64],
        y_ticks: &[f64],
        style: &ChartStyle,
        steps: Steps,
    ) -> Vec<Scene> {
        let size = style.label_size_px.max(1);
        let y_pos: Vec<f32> = y_ticks.iter().map(|&t| plot.y.map(t)).collect();
        let mut out =
            crate::draw::y_tick_labels(rect.x, y_ticks, &y_pos, steps.y, style, &self.tag_prefix);
        let slot = 60;
        for (k, &t) in x_ticks.iter().enumerate() {
            let cx = to_u32(plot.x.map(t));
            let x = cx.saturating_sub(slot / 2);
            out.push(label_node(
                format_axis_tick(t, steps.x),
                x,
                to_u32(plot.bottom) + 4,
                slot,
                TextAlign::Center,
                style.label,
                size,
                format!("{}.label.x.{k}", self.tag_prefix),
            ));
        }
        out
    }

    /// The legend row (fixed-width slots) in the top margin band — the shared
    /// [`crate::draw::legend_row`], resolving each series' colour + name.
    fn legend(&self, rect: Rect, style: &ChartStyle) -> Vec<Scene> {
        let entries: Vec<(Color, String)> = self
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| {
                (
                    s.color.unwrap_or_else(|| self.palette.color(i)),
                    s.name.clone(),
                )
            })
            .collect();
        let start_x = rect.x + style.margin.left;
        let row_y = rect.y + 6;
        crate::draw::legend_row(&entries, start_x, row_y, style, &self.tag_prefix)
    }

    /// The inspect readout as one line of text — the same focus and values the
    /// tooltip paints (`x = 10, ingress 2.6k, egress 1.6k`), or `None` when
    /// nothing is being inspected. A consumer wires this into its `WidgetA11y`
    /// node so the readout reaches a screen reader (the R1355 parity). Its
    /// rows duplicate the line chart's readout format — two consumers of the
    /// same `"name value"` row; a shared numeric-x scrub-readout helper is
    /// deferred to a third consumer (R1377).
    #[must_use]
    pub fn inspect_readout(&self, rect: Rect, style: &ChartStyle) -> Option<String> {
        let plot = CartesianPlot::resolve(
            rect,
            &self.series,
            self.x_domain,
            self.y_domain,
            style,
            false,
        );
        let x_step = tick_step(&axis_ticks(plot.x.domain(), style.x_ticks));
        let y_step = tick_step(&axis_ticks(plot.y.domain(), style.y_ticks));
        let (focus_x, hits) = resolve_focus(&self.series, self.inspect?, &plot, rect)?;
        // Same y-domain visibility filter the painted overlay applies, so the
        // readout names the SAME points the tooltip does (R1355 parity).
        let hits = Self::visible_hits(&plot, hits);
        let mut out = format!("x = {}", format_axis_tick(focus_x, x_step));
        for (i, p) in &hits {
            // `write!` to a String is infallible; the Result is discarded.
            let _ = write!(
                out,
                ", {} {}",
                self.series[*i].name,
                format_axis_tick(p.y, y_step)
            );
        }
        Some(out)
    }

    /// The `resolve_focus` hits restricted to points inside the y-domain — the
    /// ones that actually have a drawn marker. The shared `resolve_focus` filters
    /// on x only (the line chart's polyline is y-unbounded), so scatter, whose
    /// marks ARE y-clipped, applies this so its ring + tooltip + a11y readout all
    /// reference only visible points (R1377).
    fn visible_hits(
        plot: &CartesianPlot,
        hits: Vec<(usize, DataPoint)>,
    ) -> Vec<(usize, DataPoint)> {
        let (y_lo, y_hi) = plot.y.domain();
        hits.into_iter()
            .filter(|(_, p)| in_domain(p.y, y_lo, y_hi))
            .collect()
    }

    /// Resolve the inspect overlay for the current `inspect` fraction, or `None`
    /// when inspection is off, the plot is degenerate, or nothing is in range.
    fn resolve_inspect(
        &self,
        plot: &CartesianPlot,
        rect: Rect,
        style: &ChartStyle,
        steps: Steps,
    ) -> Option<Inspect> {
        let (focus_x, hits) = resolve_focus(&self.series, self.inspect?, plot, rect)?;
        // Only inspect points that are actually DRAWN. `resolve_focus` (shared
        // with the line chart, whose polyline is y-unbounded) restricts hits to
        // the x-domain only; a scatter point is `point_marks`-clipped on the y
        // domain too, so a pinned y-domain could otherwise ring a point that has
        // no marker, off the plot (R1377).
        let hits = Self::visible_hits(plot, hits);
        let focus_pixel = plot.x.map(focus_x);

        let crosshair = stroke_path(
            &[(focus_pixel, plot.top), (focus_pixel, plot.bottom)],
            Stroke::new(style.crosshair, 1),
            format!("{}.inspect.crosshair", self.tag_prefix),
        );

        // A ring one gap wider than the point marks, so it frames the focused
        // point rather than repainting it (the marks are already filled dots).
        let point_r = style.marker_radius.max(1);
        let ring_stroke = Stroke::new(style.crosshair, 2);
        let rings: Vec<Scene> = hits
            .iter()
            .map(|(i, p)| {
                ring_node(
                    plot.x.map(p.x),
                    plot.y.map(p.y),
                    point_r + 3,
                    ring_stroke,
                    format!("{}.inspect.ring.{i}", self.tag_prefix),
                )
            })
            .collect();

        let tooltip = self.inspect_tooltip(plot, focus_pixel, focus_x, &hits, style, steps);
        Some(Inspect {
            crosshair,
            rings,
            tooltip,
        })
    }

    /// The inspect tooltip: a rounded box, an `x = …` header, and one
    /// series-coloured value line per hit series. Assembles the rows and hands
    /// them to the shared [`callout`] placement (the box geometry every chart's
    /// tooltip shares). The row-building loop here is byte-identical to the line
    /// chart's, but it stays per-chart deliberately: the row CONTENT is an
    /// OPINIONATED presentation choice that diverges across chart types (a bar
    /// row is just `"{value}"`, a donut row `"{value} ({pct}%)"`), so this
    /// `"{name}  {value}"` format is a two-consumer duplicate (line + scatter)
    /// deferred until a third chart shares the identical format — unlike the
    /// mechanical focus geometry the round DID lift at two consumers (R1377).
    fn inspect_tooltip(
        &self,
        plot: &CartesianPlot,
        focus_pixel: f32,
        focus_x: f64,
        hits: &[(usize, DataPoint)],
        style: &ChartStyle,
        steps: Steps,
    ) -> Vec<Scene> {
        let header = format!("x = {}", format_axis_tick(focus_x, steps.x));
        let rows: Vec<CalloutRow> = hits
            .iter()
            .map(|(i, p)| CalloutRow {
                text: format!(
                    "{}  {}",
                    self.series[*i].name,
                    format_axis_tick(p.y, steps.y)
                ),
                color: self.series[*i]
                    .color
                    .unwrap_or_else(|| self.palette.color(*i)),
                tag: format!("{}.inspect.value.{i}", self.tag_prefix),
            })
            .collect();
        callout(
            focus_pixel,
            plot.right,
            plot.top,
            &header,
            format!("{}.inspect.header", self.tag_prefix),
            &rows,
            style,
            format!("{}.inspect.tooltip", self.tag_prefix),
        )
    }
}

/// A stroked ring of radius `r` at `(cx, cy)` — the scatter inspector's
/// highlight around the focused point. The stroked sibling of the filled
/// [`marker_node`], over the shared [`circle_commands`] geometry; a one-consumer
/// helper, so it stays here rather than in the shared `draw` module.
fn ring_node(cx: f32, cy: f32, r: u32, stroke: Stroke, tag: String) -> Scene {
    let outer = r + stroke.width;
    let bbox = Rect::new(
        to_u32(cx - to_f32(outer)),
        to_u32(cy - to_f32(outer)),
        outer * 2 + 1,
        outer * 2 + 1,
    );
    let commands = circle_commands(cx - to_f32(bbox.x), cy - to_f32(bbox.y), to_f32(r));
    Scene::Path(
        PathNode::new(bbox, commands, PathStyle::stroked(stroke))
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    )
}

/// Per-axis tick step — the precision the axis labels and the inspect tooltip
/// format at (bundled so the inspect helpers stay under the argument limit,
/// mirroring the line chart's `Steps`). A trivial two-field bundle kept
/// per-chart: a shared cross-module `Steps` type would not earn its coupling.
#[derive(Debug, Clone, Copy)]
struct Steps {
    x: f64,
    y: f64,
}

/// The inspect overlay layers, kept separate so `build_body` interleaves them at
/// the right paint depths (crosshair behind the points, rings over them, tooltip
/// above everything).
struct Inspect {
    crosshair: Scene,
    rings: Vec<Scene>,
    tooltip: Vec<Scene>,
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn count_prefix(scene: &Scene, prefix: &str) -> usize {
        let mut n = 0;
        if scene.tag().is_some_and(|t| t.starts_with(prefix)) {
            n += 1;
        }
        if let Scene::Container(c) = scene {
            for ch in &c.children {
                n += count_prefix(ch, prefix);
            }
        }
        n
    }

    fn two_series() -> Vec<Series> {
        vec![
            Series::new(
                "a",
                vec![
                    DataPoint::new(0.0, 1.0),
                    DataPoint::new(5.0, 8.0),
                    DataPoint::new(10.0, 4.0),
                ],
            ),
            Series::new(
                "b",
                vec![DataPoint::new(2.0, 6.0), DataPoint::new(8.0, 2.0)],
            ),
        ]
    }

    fn zero_margin_no_legend() -> ChartStyle {
        ChartStyle {
            margin: crate::Margin::uniform(0),
            legend: false,
            background: None,
            ..ChartStyle::default()
        }
    }

    #[test]
    fn root_and_points_are_tagged() {
        let scene = ScatterChart::new(two_series())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "chart").is_some(), "the chart root");
        // 3 + 2 = 5 points, each tagged chart.point.{i}.{j}.
        assert_eq!(count_prefix(&scene, "chart.point."), 5);
        assert!(find(&scene, "chart.point.0.0").is_some());
        assert!(find(&scene, "chart.point.1.1").is_some());
    }

    #[test]
    fn axes_and_gridlines_present() {
        let scene = ScatterChart::new(two_series())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "chart.axis.x").is_some(), "x-axis line");
        assert!(find(&scene, "chart.axis.y").is_some(), "y-axis line");
        // Numeric x-axis (unlike the categorical bar chart) => x-gridlines exist.
        assert!(count_prefix(&scene, "chart.grid.x.") > 0, "x-gridlines");
        assert_eq!(count_prefix(&scene, "chart.grid.y."), 5);
    }

    #[test]
    fn a_point_marker_is_a_closed_filled_circle() {
        use pinion_core::scene::PathCommand;
        let scene = ScatterChart::new(two_series())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let Scene::Path(p) = find(&scene, "chart.point.0.0").expect("a point") else {
            panic!("a point mark is a path")
        };
        // The shared circle: MoveTo + 4 CurveTo + Close.
        assert_eq!(p.commands.len(), 6);
        assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(p.commands[1], PathCommand::CurveTo { .. }));
        assert!(matches!(p.commands.last(), Some(PathCommand::Close)));
        assert!(p.style.fill.is_some(), "a point is FILLED");
    }

    #[test]
    fn points_outside_a_pinned_domain_are_dropped() {
        // Pin y to 0..5; series "a"'s (5, 8) and (10, 4)->x out too; "b"'s (2, 6)
        // is above the y-domain. Only in-domain points emit a node.
        let scene = ScatterChart::new(two_series())
            .with_x_domain(0.0, 6.0)
            .with_y_domain(0.0, 5.0)
            .build(Rect::new(0, 0, 400, 300), &zero_margin_no_legend());
        // In-domain: a(0,1) yes; a(5,8) no (y); b(2,6) no (y). => only 1.
        assert_eq!(count_prefix(&scene, "chart.point."), 1);
        assert!(find(&scene, "chart.point.0.0").is_some());
    }

    // ── R1391 numeric brush-range cross-filter (select_x_range) ───────

    /// A point mark's fill colour (each point is a filled circle `Scene::Path`).
    fn point_fill(scene: &Scene, tag: &str) -> Color {
        let Scene::Path(p) = find(scene, tag).expect("a point") else {
            panic!("a point mark is a path")
        };
        p.style.fill.expect("a point is filled")
    }

    #[test]
    fn r1391_select_x_range_mutes_points_outside_and_keeps_them_drawn() {
        // two_series: a=[(0,1),(5,8),(10,4)], b=[(2,6),(8,2)]. Brush x in [3,9]
        // selects a(5,8)=0.1 and b(8,2)=1.1; the other three mute.
        let scene = ScatterChart::new(two_series())
            .select_x_range(Some((3.0, 9.0)))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        // Muting DIMS, it does not DROP: all five points still emit a node
        // (unlike a pinned domain, which drops out-of-domain points).
        assert_eq!(
            count_prefix(&scene, "chart.point."),
            5,
            "every point stays drawn"
        );
        let full = CategoricalPalette::default().color(0);
        assert_eq!(
            point_fill(&scene, "chart.point.0.1"),
            full,
            "an in-range point keeps its full colour",
        );
        assert_eq!(
            point_fill(&scene, "chart.point.0.0"),
            full.with_alpha(MUTED_ALPHA),
            "an out-of-range point is its own colour dimmed to MUTED_ALPHA",
        );
        assert_eq!(
            point_fill(&scene, "chart.point.1.0"),
            CategoricalPalette::default()
                .color(1)
                .with_alpha(MUTED_ALPHA),
            "series b's out-of-range point mutes in its own colour",
        );
    }

    #[test]
    fn r1391_select_x_range_none_leaves_every_point_full() {
        let scene = ScatterChart::new(two_series())
            .select_x_range(None)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let full = CategoricalPalette::default().color(0);
        for tag in ["chart.point.0.0", "chart.point.0.1", "chart.point.0.2"] {
            assert_eq!(point_fill(&scene, tag), full, "no range = every point full");
        }
    }

    #[test]
    fn r1391_select_x_range_boundary_is_inclusive() {
        // Brush exactly [5, 8]: a(5,8)=0.1 and b(8,2)=1.1 sit ON the boundary and
        // stay selected; a(0,1) and b(2,6) fall outside and mute.
        let scene = ScatterChart::new(two_series())
            .select_x_range(Some((5.0, 8.0)))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let a = CategoricalPalette::default().color(0);
        let b = CategoricalPalette::default().color(1);
        assert_eq!(
            point_fill(&scene, "chart.point.0.1"),
            a,
            "x==lo stays selected"
        );
        assert_eq!(
            point_fill(&scene, "chart.point.1.1"),
            b,
            "x==hi stays selected"
        );
        assert_eq!(
            point_fill(&scene, "chart.point.0.0"),
            a.with_alpha(MUTED_ALPHA),
            "x below lo mutes",
        );
    }

    #[test]
    fn inspect_does_not_ring_a_y_clipped_point() {
        // Series with (0,1) inside a pinned 0..5 y-domain and (5,9) above it.
        // The shared `resolve_focus` restricts hits to the x-domain only, so
        // without the y-visibility filter the (5,9) focus would ring a point
        // that `point_marks` clipped away — off the plot, no marker under it.
        let series = vec![Series::new(
            "a",
            vec![DataPoint::new(0.0, 1.0), DataPoint::new(5.0, 9.0)],
        )];
        let base = ScatterChart::new(series)
            .with_x_domain(0.0, 5.0)
            .with_y_domain(0.0, 5.0);
        // Scrub right -> nearest point is (5,9), y-clipped -> NO ring.
        let right = base
            .clone()
            .inspect(Some(1.0))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(
            count_prefix(&right, "chart.inspect.ring."),
            0,
            "a y-clipped focus point draws no ring (it has no marker)"
        );
        // The crosshair still marks the scrubbed x.
        assert!(find(&right, "chart.inspect.crosshair").is_some());
        // Scrub left -> nearest point is (0,1), visible -> one ring.
        let left = base
            .inspect(Some(0.0))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(
            count_prefix(&left, "chart.inspect.ring."),
            1,
            "a visible focus rings"
        );
    }

    #[test]
    fn inspect_rings_a_point_per_series_and_shows_a_tooltip() {
        let scene = ScatterChart::new(two_series())
            .inspect(Some(0.2))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(
            find(&scene, "chart.inspect.crosshair").is_some(),
            "crosshair"
        );
        assert!(find(&scene, "chart.inspect.tooltip").is_some(), "tooltip");
        // One ring + one value row per series with a point in range.
        assert_eq!(count_prefix(&scene, "chart.inspect.ring."), 2);
        assert_eq!(count_prefix(&scene, "chart.inspect.value."), 2);
    }

    #[test]
    fn inspect_ring_is_a_stroked_circle() {
        use pinion_core::scene::PathCommand;
        let scene = ScatterChart::new(two_series())
            .inspect(Some(0.2))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let Scene::Path(p) = find(&scene, "chart.inspect.ring.0").expect("a ring") else {
            panic!("a ring is a path")
        };
        assert_eq!(p.commands.len(), 6);
        assert!(matches!(p.commands.last(), Some(PathCommand::Close)));
        assert!(p.style.stroke.is_some(), "a ring is STROKED, not filled");
        assert!(p.style.fill.is_none());
    }

    #[test]
    fn inspect_readout_names_x_and_each_series_value() {
        let readout = ScatterChart::new(two_series())
            .inspect(Some(0.0))
            .inspect_readout(Rect::new(0, 0, 400, 300), &ChartStyle::default())
            .expect("an inspected readout");
        assert!(
            readout.starts_with("x = "),
            "readout leads with x: {readout:?}"
        );
        assert!(
            readout.contains('a') && readout.contains('b'),
            "names both series"
        );
    }

    #[test]
    fn scrubbing_moves_the_focus() {
        // A left-edge scrub and a right-edge scrub focus different x's, so the
        // readout header differs.
        let chart = |f: f32| {
            ScatterChart::new(two_series())
                .inspect(Some(f))
                .inspect_readout(Rect::new(0, 0, 400, 300), &ChartStyle::default())
                .expect("readout")
        };
        assert_ne!(chart(0.0), chart(1.0), "the scrub picks a different point");
    }

    #[test]
    fn r1379_hidden_series_draws_no_marks_but_keeps_legend() {
        let mut series = two_series();
        series[0].visible = false;
        let scene =
            ScatterChart::new(series).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(
            count_prefix(&scene, "chart.point.0."),
            0,
            "hidden series 0: no marks"
        );
        assert!(
            count_prefix(&scene, "chart.point.1.") > 0,
            "visible series 1 still marks"
        );
        assert_eq!(
            count_prefix(&scene, "chart.legend.0"),
            2,
            "hidden series keeps its legend slot"
        );
    }

    #[test]
    fn r1379_hidden_series_is_not_scrub_inspectable() {
        // Every series hidden -> no visible point -> the scrub resolves no focus,
        // so no inspect overlay paints (the resolve_focus visibility skip).
        let series: Vec<Series> = two_series()
            .into_iter()
            .map(|s| s.with_visible(false))
            .collect();
        let scene = ScatterChart::new(series)
            .inspect(Some(0.5))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(
            count_prefix(&scene, "chart.inspect"),
            0,
            "no focus overlay when every series is hidden"
        );
    }

    #[test]
    fn legend_tracks_series_and_toggles_off() {
        let on = ScatterChart::new(two_series())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(count_prefix(&on, "chart.legend.0"), 2); // swatch + label
        assert_eq!(count_prefix(&on, "chart.legend.1"), 2);
        let off = ScatterChart::new(two_series())
            .build(Rect::new(0, 0, 400, 300), &zero_margin_no_legend());
        assert_eq!(count_prefix(&off, "chart.legend."), 0);
    }

    #[test]
    fn build_fill_zero_size_is_an_empty_but_tagged_root() {
        let scene = ScatterChart::new(two_series()).build_fill((0, 0), &ChartStyle::default());
        assert!(find(&scene, "chart").is_some(), "root still tagged");
        assert_eq!(
            count_prefix(&scene, "chart.point."),
            0,
            "no points at 0 size"
        );
    }
}
