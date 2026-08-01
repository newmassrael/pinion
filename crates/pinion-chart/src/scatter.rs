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

use crate::color_scale::{ColorScale, ValueEncoding};
use crate::draw::{
    CalloutRow, MUTED_ALPHA, absolute, box_node, callout, circle_commands, fill_parent, label_node,
    legend_band_color_bar, marker_node, stroke_path, to_f32, to_u32,
};
use crate::palette::CategoricalPalette;
use crate::plot::{
    AxisKinds, CartesianPlot, OffScale, Rescale, axis_format, axis_minor_ticks, axis_ticks,
    off_scale_points, resolve_focus, tick_pixels,
};
use crate::scale::AxisKind;
use crate::series::{DataPoint, Series, in_domain, value_bounds};
use crate::style::ChartStyle;
use crate::ticks::TickFormat;

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
    legend_tags: Option<Vec<String>>,
    color: ValueEncoding,
    kinds: AxisKinds,
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
            legend_tags: None,
            color: ValueEncoding::default(),
            kinds: AxisKinds::default(),
            tag_prefix: "chart".to_string(),
        }
    }

    /// R1438 — colour every mark by its [`DataPoint::value`] third channel on a
    /// SEQUENTIAL ramp, instead of by which series it belongs to.
    ///
    /// This is a different question than the categorical palette answers. A
    /// palette says *which* — series identity, a nominal channel where the
    /// colours must be tellable apart and rank means nothing. A colour scale
    /// says *how much* — an ordered channel where the reader is meant to
    /// compare two marks and conclude one is larger. Turning it on therefore
    /// also swaps the legend: a swatch row would claim the colours name the
    /// series, so the chart draws a colour bar over
    /// the value domain instead.
    ///
    /// The domain comes from the data ([`value_bounds`]) unless pinned with
    /// [`with_color_domain`](Self::with_color_domain). A point carrying no
    /// value keeps its series colour — the encoding covers the points that
    /// have the channel and does not invent a magnitude for those that do not.
    #[must_use]
    pub fn color_by(mut self, scale: ColorScale) -> Self {
        self.color.sequential(scale);
        self
    }

    /// R1438 — colour every mark by its [`DataPoint::value`] on a DIVERGING
    /// ramp anchored at `neutral` (a target, a baseline, zero).
    ///
    /// The sibling of [`color_by`](Self::color_by) for signed deviation, where
    /// the reader's first question is "which side of neutral, and how far" —
    /// each wing normalises on its own width (R1436), so on an asymmetric
    /// domain the neutral still lands on the ramp's centre colour instead of a
    /// third of the way up. The colour bar seats its neutral tick at the same
    /// fraction, so the legend reports the encoding rather than an even split.
    #[must_use]
    pub fn color_by_diverging(mut self, scale: ColorScale, neutral: f64) -> Self {
        self.color.diverging(scale, neutral);
        self
    }

    /// R1438 — pin the colour domain instead of deriving it from the data.
    ///
    /// Worth pinning whenever two charts must be comparable (the same colour
    /// has to mean the same magnitude across both) or when the scale should
    /// span a known operating range rather than the sample's own extremes.
    #[must_use]
    pub fn with_color_domain(mut self, lo: f64, hi: f64) -> Self {
        self.color.pin_domain(lo, hi);
        self
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

    /// Make the legend **interactive** (R1392): each entry becomes a focusable,
    /// hit-testable region tagged with the caller's `tags[i]` (one per series, in
    /// series order), so a click / press anywhere on entry `i` resolves — through
    /// the router's deepest-tagged-ancestor hit-test — to the
    /// [`External`](pinion_core::external::External) the caller binds to `tags[i]`,
    /// and a hidden series' entry renders muted (grey swatch + dimmed label). The
    /// scatter counterpart of [`LineChart::interactive_legend`](crate::LineChart::interactive_legend),
    /// sharing the one lifted `crate::draw::interactive_legend_row`, so a scatter
    /// can be a cross-filter SELECTOR that drives a DIFFERENT chart type (a
    /// scatter-legend click filtering a companion line chart — the "arbitrary
    /// chart-to-chart" cross-filter). The chart stays a pure scene producer: it
    /// emits the tagged focusable entries; the caller owns the tags and wires the
    /// toggles to [`Series::visible`](crate::Series::visible). Requires
    /// [`ChartStyle::legend`](crate::ChartStyle::legend) `= true`.
    #[must_use]
    pub fn interactive_legend(mut self, tags: Vec<String>) -> Self {
        self.legend_tags = Some(tags);
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

    /// Plot the **y**-axis logarithmically in base 10 (R1528) — the scatter
    /// chart's leg of Qt's `QLogValueAxis`; see
    /// [`LineChart::y_log`](crate::LineChart::y_log) for the whole contract.
    #[must_use]
    pub fn y_log(self) -> Self {
        self.y_log_base(crate::DEFAULT_LOG_BASE)
    }

    /// [`y_log`](Self::y_log) in an explicit `base`.
    #[must_use]
    pub fn y_log_base(mut self, base: f64) -> Self {
        self.kinds.y = AxisKind::Log(base);
        self
    }

    /// Plot the **x**-axis logarithmically in base 10 (R1528).
    ///
    /// The scatter chart is where a log *x*-axis earns its keep: a
    /// correlation between two measures that each span decades (request size
    /// against latency) is a straight line on log-log axes and an
    /// uninterpretable corner-hugging cloud on linear ones.
    #[must_use]
    pub fn x_log(self) -> Self {
        self.x_log_base(crate::DEFAULT_LOG_BASE)
    }

    /// [`x_log`](Self::x_log) in an explicit `base`.
    #[must_use]
    pub fn x_log_base(mut self, base: f64) -> Self {
        self.kinds.x = AxisKind::Log(base);
        self
    }

    /// Plot the **x**-axis as UTC time (R1529) — see
    /// [`LineChart::x_time`](crate::LineChart::x_time) for the whole
    /// contract. Sample `x` values are read as epoch milliseconds.
    #[must_use]
    pub fn x_time(mut self) -> Self {
        self.kinds.x = AxisKind::Time;
        self
    }

    /// Plot the **y**-axis as UTC time (R1529) — see
    /// [`LineChart::x_time`](crate::LineChart::x_time).
    #[must_use]
    pub fn y_time(mut self) -> Self {
        self.kinds.y = AxisKind::Time;
        self
    }

    /// Every point this chart's axes cannot place (R1528) — see
    /// [`LineChart::off_scale`](crate::LineChart::off_scale).
    #[must_use]
    pub fn off_scale(&self) -> Vec<OffScale> {
        off_scale_points(&self.series, self.kinds)
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
            Rescale::default(),
            self.kinds,
        );
        let x_ticks = axis_ticks(&plot.x, style.x_ticks);
        let y_ticks = axis_ticks(&plot.y, style.y_ticks);
        let steps = Steps {
            x: axis_format(&plot.x, &x_ticks),
            y: axis_format(&plot.y, &y_ticks),
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
        // A log axis's fainter per-decade subdivisions paint below the
        // labelled grid (R1528); a linear axis has none, so this is empty.
        children.extend(crate::draw::minor_gridlines(
            (plot.left, plot.right, plot.top, plot.bottom),
            &tick_pixels(&plot.x, &axis_minor_ticks(&plot.x)),
            &tick_pixels(&plot.y, &axis_minor_ticks(&plot.y)),
            style,
            &self.tag_prefix,
        ));
        children.extend(crate::draw::gridlines(
            (plot.left, plot.right, plot.top, plot.bottom),
            &tick_pixels(&plot.x, &x_ticks),
            &tick_pixels(&plot.y, &y_ticks),
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
            // R1438 — one colour legend, never two: a value encoding replaces
            // the series swatch row with the colour bar, because the swatches
            // would name a series-to-colour mapping that no longer holds.
            if self.color.is_set() {
                children.extend(self.color_bar_row(rect, style));
            } else {
                children.extend(self.legend(rect, style));
            }
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

    /// R1438 — the active colour domain: pinned, else measured off the data,
    /// else `None` when not one point carries the third channel (in which case
    /// there is nothing to encode and the chart stays categorical).
    fn resolved_color_domain(&self) -> Option<(f64, f64)> {
        self.color.domain(|| value_bounds(&self.series))
    }

    /// R1438 — the value-encoded colour for `point`, or `None` when this chart
    /// is not colouring by value, has no domain to map against, or the point
    /// carries no finite third channel.
    fn value_color(&self, point: DataPoint) -> Option<Color> {
        self.color
            .color_for(point.value, || value_bounds(&self.series))
    }

    /// R1438 — the colour bar: the value-encoding legend, seated in the same
    /// top band the categorical legend row uses. Emitted INSTEAD of that row
    /// (see [`color_by`](Self::color_by)) — showing both would assert that
    /// colour means series identity and magnitude at once.
    ///
    /// Ticks are the domain ends plus, for a diverging encoding, the neutral —
    /// at the fraction the encoding actually places it, which on an asymmetric
    /// domain is not the middle of the bar. The scatter has margins and a
    /// legend band, so its bar lies HORIZONTALLY across that band; the treemap,
    /// which has neither, stands its bar up the side instead.
    fn color_bar_row(&self, rect: Rect, style: &ChartStyle) -> Vec<Scene> {
        legend_band_color_bar(
            &self.color,
            self.resolved_color_domain(),
            rect,
            style,
            &self.tag_prefix,
        )
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
                // R1438 — value encoding wins over series identity when the
                // point carries the third channel; a point without one keeps
                // its series colour rather than being given an invented
                // magnitude.
                let color = self.value_color(*p).unwrap_or(color);
                // Cross-filter (R1391): a point outside the active brush range is
                // dimmed but still drawn (context), unlike the domain drop above.
                let mark_color = if self.is_muted(p.x) {
                    color.with_alpha(MUTED_ALPHA)
                } else {
                    color
                };
                let Some((px, py)) = plot.map_point(p) else {
                    continue;
                };
                out.push(marker_node(
                    px,
                    py,
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
        let y_pos = tick_pixels(&plot.y, y_ticks);
        let mut out =
            crate::draw::y_tick_labels(rect.x, y_ticks, &y_pos, steps.y, style, &self.tag_prefix);
        let slot = 60;
        for (k, &t) in x_ticks.iter().enumerate() {
            // (R1396) Clamp the box inside `rect` (see the line chart's twin) so
            // the last x-tick label no longer overhangs a docked neighbour.
            let Some(px) = plot.x.map(t) else { continue };
            let x = crate::draw::centered_label_x(px, slot, rect);
            out.push(label_node(
                steps.x.label(t),
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
        let start_x = rect.x + style.margin.left;
        let row_y = rect.y + 6;
        // (R1396) The row's width from `start_x` to the chart's right edge.
        let avail = rect.w.saturating_sub(style.margin.left);
        // R1392 — an interactive legend (focusable tagged entries) when the
        // caller opts in, else the static swatch+label row.
        if let Some(tags) = &self.legend_tags {
            let entries: Vec<(Color, String, bool)> = self
                .series
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    (
                        s.color.unwrap_or_else(|| self.palette.color(i)),
                        s.name.clone(),
                        s.visible,
                    )
                })
                .collect();
            return crate::draw::interactive_legend_row(
                &entries,
                tags,
                start_x,
                row_y,
                avail,
                style,
                &self.tag_prefix,
            );
        }
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
        crate::draw::legend_row(&entries, start_x, row_y, avail, style, &self.tag_prefix)
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
            Rescale::default(),
            self.kinds,
        );
        let x_step = axis_format(&plot.x, &axis_ticks(&plot.x, style.x_ticks));
        let y_step = axis_format(&plot.y, &axis_ticks(&plot.y, style.y_ticks));
        let (focus_x, hits) = resolve_focus(&self.series, self.inspect?, &plot, rect)?;
        // Same y-domain visibility filter the painted overlay applies, so the
        // readout names the SAME points the tooltip does (R1355 parity).
        let hits = Self::visible_hits(&plot, hits);
        let mut out = format!("x = {}", x_step.label(focus_x));
        for (i, p) in &hits {
            // `write!` to a String is infallible; the Result is discarded.
            let _ = write!(out, ", {} {}", self.series[*i].name, y_step.label(p.y));
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
        let focus_pixel = plot.x.map(focus_x)?;

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
            .filter_map(|(i, p)| {
                let (px, py) = plot.map_point(p)?;
                Some(ring_node(
                    px,
                    py,
                    point_r + 3,
                    ring_stroke,
                    format!("{}.inspect.ring.{i}", self.tag_prefix),
                ))
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
        let header = format!("x = {}", steps.x.readout(focus_x));
        let rows: Vec<CalloutRow> = hits
            .iter()
            .map(|(i, p)| CalloutRow {
                text: format!("{}  {}", self.series[*i].name, steps.y.label(p.y)),
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
    x: TickFormat,
    y: TickFormat,
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

    // ── R1392 interactive legend (scatter as a cross-filter selector) ─

    #[test]
    fn r1392_interactive_legend_emits_focusable_tagged_entries() {
        let tags = vec!["sc_0".to_string(), "sc_1".to_string()];
        let scene = ScatterChart::new(two_series())
            .interactive_legend(tags.clone())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        for tag in &tags {
            let Some(Scene::Container(entry)) = find(&scene, tag) else {
                panic!("legend entry {tag} is a focusable container")
            };
            assert!(
                entry.layout.focusable,
                "entry {tag} is a Tab / click target"
            );
        }
        // The static swatch tags are NOT emitted when the legend is interactive.
        assert!(
            find(&scene, "chart.legend.0.swatch").is_none(),
            "no static legend row when interactive",
        );
    }

    #[test]
    fn r1392_without_interactive_legend_the_static_row_is_used() {
        let scene = ScatterChart::new(two_series())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(
            find(&scene, "chart.legend.0.swatch").is_some(),
            "the static legend swatch",
        );
        assert!(find(&scene, "sc_0").is_none(), "no caller-tagged entry");
    }

    #[test]
    fn r1392_hidden_series_keeps_its_interactive_entry() {
        let mut series = two_series();
        series[1].visible = false;
        let tags = vec!["sc_0".to_string(), "sc_1".to_string()];
        let scene = ScatterChart::new(series)
            .interactive_legend(tags)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        // The hidden series draws no point marks...
        assert_eq!(
            count_prefix(&scene, "chart.point.1."),
            0,
            "a hidden series draws no points",
        );
        // ...but its legend entry stays (it is the toggle back on).
        assert!(
            find(&scene, "sc_1").is_some(),
            "the hidden series keeps its interactive legend entry",
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

    /// R1438 — points carrying the third channel, spanning an ASYMMETRIC
    /// deviation domain (-2 .. +8) so the diverging contract is testable.
    fn valued_series() -> Vec<Series> {
        vec![Series::new(
            "probe",
            vec![
                DataPoint::new(0.0, 1.0).with_value(-2.0),
                DataPoint::new(2.0, 3.0).with_value(0.0),
                DataPoint::new(4.0, 5.0).with_value(4.0),
                DataPoint::new(6.0, 7.0).with_value(8.0),
            ],
        )]
    }

    fn mark_fill(scene: &Scene, tag: &str) -> Color {
        match find(scene, tag).expect("mark present") {
            Scene::Path(p) => p.style.fill.expect("mark is filled"),
            other => panic!("unexpected mark node: {other:?}"),
        }
    }

    /// The encoding's whole point: colour ranks magnitude, so two marks from
    /// the SAME series differ, and equal values agree across DIFFERENT series.
    #[test]
    fn r1438_colour_ranks_value_not_series_identity() {
        let scene = ScatterChart::new(valued_series())
            .color_by(ColorScale::viridis())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let lowest = mark_fill(&scene, "chart.point.0.0");
        let highest = mark_fill(&scene, "chart.point.0.3");
        assert_ne!(
            lowest, highest,
            "same series, different value -> different colour"
        );

        // Two series, one shared value: colour must not encode which series.
        let shared = vec![
            Series::new("a", vec![DataPoint::new(0.0, 1.0).with_value(5.0)]),
            Series::new("b", vec![DataPoint::new(1.0, 2.0).with_value(5.0)]),
        ];
        let scene = ScatterChart::new(shared)
            .color_by(ColorScale::viridis())
            .with_color_domain(0.0, 10.0)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(
            mark_fill(&scene, "chart.point.0.0"),
            mark_fill(&scene, "chart.point.1.0"),
            "equal values -> equal colour across series"
        );
    }

    /// A point with no third channel keeps its series colour: the encoding
    /// covers the points that carry the channel and invents nothing for the
    /// rest. The counter-assertion pins that a valued point in the same
    /// series DOES move, so this is not just "the encoding did nothing".
    #[test]
    fn r1438_valueless_point_keeps_its_series_colour() {
        let mixed = vec![Series::new(
            "a",
            vec![
                DataPoint::new(0.0, 1.0),
                DataPoint::new(1.0, 2.0).with_value(9.0),
            ],
        )];
        let plain = ScatterChart::new(mixed.clone())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let encoded = ScatterChart::new(mixed)
            .color_by(ColorScale::viridis())
            .with_color_domain(0.0, 10.0)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(
            mark_fill(&plain, "chart.point.0.0"),
            mark_fill(&encoded, "chart.point.0.0"),
            "the valueless point is untouched by the encoding"
        );
        assert_ne!(
            mark_fill(&plain, "chart.point.0.1"),
            mark_fill(&encoded, "chart.point.0.1"),
            "the valued point in the same series DID take the ramp"
        );
    }

    /// The legend swaps rather than doubling: colour cannot mean series
    /// identity and magnitude at the same time.
    #[test]
    fn r1438_colour_bar_replaces_the_swatch_row() {
        let categorical = ScatterChart::new(valued_series())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(count_prefix(&categorical, "chart.legend.") > 0);
        assert_eq!(count_prefix(&categorical, "chart.colorbar"), 0);

        let encoded = ScatterChart::new(valued_series())
            .color_by(ColorScale::viridis())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(
            count_prefix(&encoded, "chart.legend."),
            0,
            "the swatch row is gone — it would claim colour names the series"
        );
        assert!(find(&encoded, "chart.colorbar.strip").is_some());
        assert!(find(&encoded, "chart.colorbar.tick.0").is_some());

        let off = ScatterChart::new(valued_series())
            .color_by(ColorScale::viridis())
            .build(Rect::new(0, 0, 400, 300), &zero_margin_no_legend());
        assert_eq!(
            count_prefix(&off, "chart.colorbar"),
            0,
            "legend=false suppresses the bar too"
        );
    }

    /// ★ The round's load-bearing claim: on an ASYMMETRIC domain the bar's
    /// neutral sits where the ENCODING puts it, not at the bar's midpoint —
    /// and the ramp the bar publishes is the ramp the marks were painted with.
    #[test]
    fn r1438_diverging_bar_reports_the_encoding_not_an_even_split() {
        let scale = ColorScale::blue_orange();
        let scene = ScatterChart::new(valued_series())
            .color_by_diverging(scale.clone(), 0.0)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());

        // The neutral tick exists and is NOT the midpoint: 0.0 sits a fifth of
        // the way along -2..+8, so a bar built on even spacing would be wrong.
        let strip = find(&scene, "chart.colorbar.strip").expect("strip");
        let Scene::Box(b) = strip else {
            panic!("strip is a box")
        };
        let stops = &b.style.gradient.as_ref().expect("gradient").stops;
        assert_eq!(stops.len(), 3, "blue_orange has three stops");
        assert!(
            (stops[1].offset - 0.2).abs() < 1e-3,
            "the neutral stop sits at the neutral's fraction of the domain, \
             not at 0.5 — got {}",
            stops[1].offset
        );

        // The published ramp agrees with the marks: the zero-valued point is
        // painted the centre stop's colour.
        assert_eq!(
            mark_fill(&scene, "chart.point.0.1"),
            stops[1].color,
            "the neutral mark IS the bar's neutral colour"
        );

        // Counterfactual: the sequential encoding over the same data does NOT
        // put a stop at 0.2, so the assertion above is discriminating.
        let sequential = ScatterChart::new(valued_series())
            .color_by(scale)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let Some(Scene::Box(seq)) = find(&sequential, "chart.colorbar.strip") else {
            panic!("strip")
        };
        let seq_stops = &seq.style.gradient.as_ref().expect("gradient").stops;
        assert!(
            (seq_stops[1].offset - 0.5).abs() < 1e-3,
            "a sequential ramp spaces its stops evenly — got {}",
            seq_stops[1].offset
        );
    }

    /// No third channel anywhere = nothing to encode: the chart stays
    /// categorical rather than drawing a bar over an empty domain.
    #[test]
    fn r1438_colour_by_without_values_draws_no_bar() {
        let scene = ScatterChart::new(two_series())
            .color_by(ColorScale::viridis())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(count_prefix(&scene, "chart.colorbar"), 0);
        assert_eq!(
            mark_fill(&scene, "chart.point.0.0"),
            mark_fill(&scene, "chart.point.0.1"),
            "valueless points all keep their one series colour"
        );
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
