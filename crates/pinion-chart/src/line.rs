//! The line / area chart builder — projects [`Series`] data into a
//! retained [`Scene`] of axes, gridlines, tick labels, series polylines,
//! optional area fills, and a legend.
//!
//! # Coordinate contract — a remaining limitation, no longer a primitive one
//!
//! [`LineChart::build`] takes the [`Rect`] the chart will occupy and must
//! be given it *before* the layout pass runs, so the chart is still not a
//! layout citizen: it cannot flex, dock, tab, or resize.
//!
//! Until R1358 that was **unfixable from this crate**: the `Scene::Path`
//! primitive painted `commands` at literal device coordinates and never
//! offset them by the rect layout assigned, so a path's geometry could not
//! participate in layout at all. R1358 made path commands relative to the
//! node's own rect (the basis the R722 gradient UV already used), so a
//! path is now placed by its rect like any other node. Every path here
//! declares `absolute_position` + size, which is why that migration was
//! pixel-identical for this crate.
//!
//! What remains is a **chart-side** redesign — no longer blocked by the
//! primitive, but not done either. Precisely:
//!
//! * Every child is still resolved against the *absolute* `rect` handed to
//!   [`LineChart::build`] and pinned with an `absolute_position` carrying
//!   those absolute coordinates, and the chart's root
//!   [`Container`](pinion_core::Scene) declares no layout of its own. So
//!   the chart still only lands correctly under a root at the window
//!   origin. Emitting children relative to a placed chart root is the work.
//! * `build` also needs its *size* while the view fn runs, which is the
//!   measured-rect reactive seam's job, not the path primitive's.
//!
//! `build`'s signature is expected to change with that follow-up — do not
//! design against it yet.
//!
//! # Introspection
//!
//! Every emitted node carries a tag under the chart's `tag_prefix`
//! (default `"chart"`): `chart.bg`, `chart.series.{i}`, `chart.area.{i}`,
//! `chart.grid.y.{k}` / `chart.grid.x.{k}`, `chart.axis.x` /
//! `chart.axis.y`, `chart.label.y.{k}` / `chart.label.x.{k}`,
//! `chart.legend.{i}.swatch` / `chart.legend.{i}.label`, and — when
//! [`inspect`](LineChart::inspect) is set — `chart.inspect.crosshair`,
//! `chart.inspect.marker.{i}`, `chart.inspect.tooltip`,
//! `chart.inspect.header`, `chart.inspect.value.{i}`.
//! This makes the whole chart queryable as data (§2 #1 / #7) — an AI
//! client reads the series geometry without sampling pixels.
//!
//! A series with no point inside the x-domain emits no `chart.series.{i}`
//! node at all (its legend entry remains), so a consumer must treat these
//! tags as present-if-visible rather than one-per-series.

use core::fmt::Write as _;

use pinion_core::Scene;
use pinion_core::scene::{
    BoxNode, ContainerNode, PathCommand, PathNode, PathPoint, Rect, TextNode,
};
use pinion_core::style::{
    BoxStyle, Color, LayoutStyle, PathStyle, Size, Stroke, StrokeCap, TextAlign, TextStyle,
};

use crate::palette::CategoricalPalette;
use crate::series::{DataPoint, Series, data_bounds};
use crate::ticks::{format_axis_tick, nice_ticks};

/// Pixel insets between the chart `rect` and its plotting area, leaving
/// room for the axis tick labels and (top) the legend row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Margin {
    /// Left inset — the y-axis label gutter.
    pub left: u32,
    /// Top inset — the legend row.
    pub top: u32,
    /// Right inset.
    pub right: u32,
    /// Bottom inset — the x-axis label row.
    pub bottom: u32,
}

impl Margin {
    /// Explicit per-side margins.
    #[must_use]
    pub const fn new(left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The same inset on every side.
    #[must_use]
    pub const fn uniform(value: u32) -> Self {
        Self::new(value, value, value, value)
    }
}

impl Default for Margin {
    fn default() -> Self {
        Self::new(52, 28, 16, 28)
    }
}

/// Resolved colours, sizes, and layout knobs for a chart render. The
/// colour fields are plain [`Color`]s so this crate stays decoupled from
/// the theme system — the consumer resolves its theme roles (e.g.
/// `ColorRole::Outline` for the grid) into these fields.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartStyle {
    /// Axis line colour.
    pub axis: Color,
    /// Gridline colour (usually a low-alpha outline).
    pub grid: Color,
    /// Tick-label and legend-label text colour.
    pub label: Color,
    /// Optional plot background fill.
    pub background: Option<Color>,
    /// Tick-label / legend font size in px.
    pub label_size_px: u32,
    /// Series polyline stroke width in px.
    pub series_width: u32,
    /// Alpha (0-255) of the translucent area fill under a filled series.
    pub area_alpha: u8,
    /// Plot insets.
    pub margin: Margin,
    /// Target x-axis tick count (nice-number snapped).
    pub x_ticks: usize,
    /// Target y-axis tick count (nice-number snapped).
    pub y_ticks: usize,
    /// Whether to render the legend row.
    pub legend: bool,
    /// Inspect crosshair line colour (the vertical scrub guide).
    pub crosshair: Color,
    /// Radius (px) of the per-series inspect marker dots.
    pub marker_radius: u32,
    /// Inspect tooltip background fill.
    pub tooltip_bg: Color,
    /// Inspect tooltip header / text colour (series values use the
    /// series colour; this is the `x = …` header).
    pub tooltip_fg: Color,
}

impl ChartStyle {
    /// The default chart style (neutral greys that read on a mid
    /// surface). Alias for [`Default::default`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for ChartStyle {
    fn default() -> Self {
        let neutral = Color::rgb(0x8A, 0x92, 0x9E);
        Self {
            axis: neutral,
            grid: neutral.with_alpha(0x30),
            label: neutral,
            background: None,
            label_size_px: 11,
            series_width: 2,
            area_alpha: 40,
            margin: Margin::default(),
            x_ticks: 6,
            y_ticks: 5,
            legend: true,
            crosshair: neutral.with_alpha(0xB0),
            marker_radius: 4,
            tooltip_bg: Color::rgb(0x25, 0x2A, 0x33),
            tooltip_fg: Color::rgb(0xE8, 0xEB, 0xEF),
        }
    }
}

/// A line chart: one or more [`Series`] drawn as polylines with nice
/// axes, gridlines, labels, and a legend. Set [`filled`](Self::filled) to
/// also paint a translucent area under each series (an area chart).
#[derive(Debug, Clone)]
pub struct LineChart {
    series: Vec<Series>,
    palette: CategoricalPalette,
    x_domain: Option<(f64, f64)>,
    y_domain: Option<(f64, f64)>,
    fill_area: bool,
    inspect: Option<f32>,
    tag_prefix: String,
}

impl LineChart {
    /// A line chart over `series`, using the default palette, auto
    /// domains, no area fill, and the `"chart"` tag prefix.
    #[must_use]
    pub fn new(series: Vec<Series>) -> Self {
        Self {
            series,
            palette: CategoricalPalette::default(),
            x_domain: None,
            y_domain: None,
            fill_area: false,
            inspect: None,
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

    /// Fill a translucent area under each series (turns the line chart
    /// into an area chart).
    #[must_use]
    pub fn filled(mut self, fill: bool) -> Self {
        self.fill_area = fill;
        self
    }

    /// Show the inspect overlay (a vertical crosshair, per-series marker
    /// dots, and a value tooltip) at `fraction` — the cursor's position
    /// as a fraction `0.0..=1.0` across the chart `rect` width. `None`
    /// (the default) draws no overlay. The fraction is the natural output
    /// of a pointer-capture scrub (`SliderExternal` value / `pointer_move`
    /// `x_rel`); the chart maps it through its own margins + domain to the
    /// nearest data point, so the caller needs no domain knowledge.
    #[must_use]
    pub fn inspect(mut self, fraction: Option<f32>) -> Self {
        self.inspect = fraction;
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

    /// Build the chart into a [`Scene`] occupying `rect`. The rect must be
    /// the chart's final geometry — it is resolved here, before layout runs.
    /// See the module-level coordinate contract.
    #[must_use]
    pub fn build(&self, rect: Rect, style: &ChartStyle) -> Scene {
        let plot = Plot::resolve(rect, &self.series, self.x_domain, self.y_domain, style);
        let (y_lo, y_hi) = plot.y.domain();
        let (x_lo, x_hi) = plot.x.domain();
        // One tick-set resolver, shared with `inspect_readout` so the
        // painted axis and the AT readout format at the same precision.
        let x_ticks = Self::axis_ticks((x_lo, x_hi), style.x_ticks);
        let y_ticks = Self::axis_ticks((y_lo, y_hi), style.y_ticks);
        let baseline = clamp(0.0, y_lo, y_hi);
        // The label precision each axis formats at (R1359: `format_si` alone
        // collapsed every sub-0.1 step onto one rounded digit).
        let steps = Steps {
            x: step_of(&x_ticks),
            y: step_of(&y_ticks),
        };

        // Inspect overlay, split so the crosshair paints behind the
        // series, the markers on top of the lines, and the tooltip above
        // everything.
        let (crosshair, markers, tooltip) = match self.resolve_inspect(&plot, rect, style, steps) {
            Some(i) => (Some(i.crosshair), i.markers, i.tooltip),
            None => (None, Vec::new(), Vec::new()),
        };

        let mut children: Vec<Scene> = Vec::new();
        if let Some(bg) = style.background {
            children.push(box_node(rect, bg, format!("{}.bg", self.tag_prefix)));
        }
        children.extend(self.gridlines(&plot, &x_ticks, &y_ticks, style));
        if let Some(crosshair) = crosshair {
            children.push(crosshair);
        }
        children.extend(self.axes(&plot, style));
        children.extend(self.series_layer(&plot, baseline, style));
        children.extend(markers);
        children.extend(self.tick_labels(&plot, rect, &x_ticks, &y_ticks, style, steps));
        if style.legend {
            children.extend(self.legend(rect, style));
        }
        children.extend(tooltip);

        Scene::Container(ContainerNode::new(children).with_tag(self.tag_prefix.clone()))
    }

    /// Horizontal (per y-tick) and vertical (per x-tick) gridlines.
    fn gridlines(
        &self,
        plot: &Plot,
        x_ticks: &[f64],
        y_ticks: &[f64],
        style: &ChartStyle,
    ) -> Vec<Scene> {
        let stroke = Stroke::new(style.grid, 1);
        let mut out = Vec::new();
        for (k, &t) in y_ticks.iter().enumerate() {
            let y = plot.y.map(t);
            out.push(stroke_path(
                &[(plot.left, y), (plot.right, y)],
                stroke,
                format!("{}.grid.y.{k}", self.tag_prefix),
            ));
        }
        for (k, &t) in x_ticks.iter().enumerate() {
            let x = plot.x.map(t);
            out.push(stroke_path(
                &[(x, plot.top), (x, plot.bottom)],
                stroke,
                format!("{}.grid.x.{k}", self.tag_prefix),
            ));
        }
        out
    }

    /// The left (y) and bottom (x) axis lines.
    fn axes(&self, plot: &Plot, style: &ChartStyle) -> Vec<Scene> {
        let stroke = Stroke::new(style.axis, 1);
        vec![
            stroke_path(
                &[(plot.left, plot.top), (plot.left, plot.bottom)],
                stroke,
                format!("{}.axis.y", self.tag_prefix),
            ),
            stroke_path(
                &[(plot.left, plot.bottom), (plot.right, plot.bottom)],
                stroke,
                format!("{}.axis.x", self.tag_prefix),
            ),
        ]
    }

    /// The per-series area fills (when [`filled`](Self::filled)) and
    /// polylines. Areas paint before lines so the stroke sits on top.
    fn series_layer(&self, plot: &Plot, baseline: f64, style: &ChartStyle) -> Vec<Scene> {
        let baseline_y = plot.y.map(baseline);
        let (x_lo, x_hi) = plot.x.domain();
        let mut out = Vec::new();
        for (i, s) in self.series.iter().enumerate() {
            let color = s.color.unwrap_or_else(|| self.palette.color(i));
            // Clip to the x-domain BEFORE mapping: path commands are
            // absolute device pixels the paint adapter does not clip to
            // the node rect, so an out-of-domain point would paint
            // straight through the axes (a pinned / zoomed domain).
            let finite: Vec<DataPoint> = s
                .points
                .iter()
                .filter(|p| p.x.is_finite() && p.y.is_finite())
                .copied()
                .collect();
            let pts: Vec<(f32, f32)> = clip_to_x_domain(&finite, x_lo, x_hi)
                .iter()
                .map(|p| (plot.x.map(p.x), plot.y.map(p.y)))
                .collect();
            if pts.is_empty() {
                continue;
            }
            if self.fill_area {
                out.push(area_path(
                    &pts,
                    baseline_y,
                    color.with_alpha(style.area_alpha),
                    format!("{}.area.{i}", self.tag_prefix),
                ));
            }
            let stroke = Stroke::new(color, style.series_width.max(1)).with_cap(StrokeCap::Round);
            out.push(stroke_path(
                &pts,
                stroke,
                format!("{}.series.{i}", self.tag_prefix),
            ));
        }
        out
    }

    /// Right-aligned y-axis labels and centred x-axis labels.
    fn tick_labels(
        &self,
        plot: &Plot,
        rect: Rect,
        x_ticks: &[f64],
        y_ticks: &[f64],
        style: &ChartStyle,
        steps: Steps,
    ) -> Vec<Scene> {
        let size = style.label_size_px.max(1);
        let mut out = Vec::new();
        let gutter = style.margin.left.saturating_sub(6).max(1);
        for (k, &t) in y_ticks.iter().enumerate() {
            let y = to_u32(plot.y.map(t)).saturating_sub(size / 2 + 1);
            out.push(label_node(
                format_axis_tick(t, steps.y),
                rect.x + 2,
                y,
                gutter,
                TextAlign::End,
                style.label,
                size,
                format!("{}.label.y.{k}", self.tag_prefix),
            ));
        }
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

    /// The legend row (fixed-width slots) in the top margin band.
    fn legend(&self, rect: Rect, style: &ChartStyle) -> Vec<Scene> {
        let size = style.label_size_px.max(1);
        let swatch = size;
        let slot = 104;
        let row_y = rect.y + 6;
        let start_x = rect.x + style.margin.left;
        let mut out = Vec::new();
        for (i, s) in self.series.iter().enumerate() {
            let color = s.color.unwrap_or_else(|| self.palette.color(i));
            #[allow(
                clippy::cast_possible_truncation,
                reason = "legend index is small; slot offset stays within u32"
            )]
            let entry_x = start_x + (i as u32) * slot;
            out.push(box_node(
                Rect::new(entry_x, row_y, swatch, swatch),
                color,
                format!("{}.legend.{i}.swatch", self.tag_prefix),
            ));
            out.push(label_node(
                s.name.clone(),
                entry_x + swatch + 4,
                row_y.saturating_sub(1),
                slot.saturating_sub(swatch + 4),
                TextAlign::Start,
                style.label,
                size,
                format!("{}.legend.{i}.label", self.tag_prefix),
            ));
        }
        out
    }

    /// Resolve which data point the inspect cursor is focused on: the
    /// focus x, plus the nearest visible point of each series.
    ///
    /// The single source for "what is the inspector pointing at" — the
    /// painted overlay ([`Self::resolve_inspect`]) and the AT-facing
    /// readout ([`Self::inspect_readout`]) both derive from it, so the
    /// tooltip a sighted user sees and the description a screen reader
    /// hears can never disagree.
    fn resolve_focus(&self, plot: &Plot, rect: Rect) -> Option<(f64, Vec<(usize, DataPoint)>)> {
        let fraction = self.inspect?;
        let span = plot.right - plot.left;
        if span <= 0.0 {
            return None;
        }
        // chart-rect fraction -> plot fraction -> data x.
        let cursor_px = to_f32(rect.x) + fraction.clamp(0.0, 1.0) * to_f32(rect.w);
        let plot_frac = ((cursor_px - plot.left) / span).clamp(0.0, 1.0);
        let (x_lo, x_hi) = plot.x.domain();
        let data_x = x_lo + f64::from(plot_frac) * (x_hi - x_lo);

        // Nearest point per series + the overall focus x (the series
        // point nearest the cursor across every series).
        let mut focus_x: Option<f64> = None;
        let mut hits: Vec<(usize, DataPoint)> = Vec::new();
        for (i, s) in self.series.iter().enumerate() {
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

    /// The inspect readout as one line of text — the same focus and values
    /// the tooltip paints (`x = 10, ingress 2.6k, egress 1.6k`), or `None`
    /// when nothing is being inspected.
    ///
    /// The painted tooltip is unreachable to assistive tech: it is a `Box`
    /// plus `Text` leaves, not a described region. A consumer wires this
    /// string into its `WidgetA11y` node (e.g. via
    /// `pinion_a11y::described::describedby_region`) so the readout the
    /// chart exists to deliver actually reaches a screen reader.
    #[must_use]
    pub fn inspect_readout(&self, rect: Rect, style: &ChartStyle) -> Option<String> {
        let plot = Plot::resolve(rect, &self.series, self.x_domain, self.y_domain, style);
        let x_ticks: Vec<f64> = Self::axis_ticks(plot.x.domain(), style.x_ticks);
        let y_ticks: Vec<f64> = Self::axis_ticks(plot.y.domain(), style.y_ticks);
        let steps = Steps {
            x: step_of(&x_ticks),
            y: step_of(&y_ticks),
        };
        let (focus_x, hits) = self.resolve_focus(&plot, rect)?;
        let mut out = format!("x = {}", format_axis_tick(focus_x, steps.x));
        for (i, p) in &hits {
            // `write!` to a String is infallible; the Result is discarded
            // exactly as `push_str` would have.
            let _ = write!(
                out,
                ", {} {}",
                self.series[*i].name,
                format_axis_tick(p.y, steps.y)
            );
        }
        Some(out)
    }

    /// Nice ticks for `domain`, clipped to it — the axis's own tick set.
    fn axis_ticks(domain: (f64, f64), target: usize) -> Vec<f64> {
        let (lo, hi) = domain;
        nice_ticks(lo, hi, target)
            .into_iter()
            .filter(|t| in_domain(*t, lo, hi))
            .collect()
    }

    /// Resolve the inspect overlay for the current `inspect` fraction, or
    /// `None` when inspection is off, the plot is degenerate, or there is
    /// no data under the cursor.
    fn resolve_inspect(
        &self,
        plot: &Plot,
        rect: Rect,
        style: &ChartStyle,
        steps: Steps,
    ) -> Option<Inspect> {
        let (focus_x, hits) = self.resolve_focus(plot, rect)?;
        let focus_pixel = plot.x.map(focus_x);

        let crosshair = stroke_path(
            &[(focus_pixel, plot.top), (focus_pixel, plot.bottom)],
            Stroke::new(style.crosshair, 1),
            format!("{}.inspect.crosshair", self.tag_prefix),
        );

        let radius = style.marker_radius.max(1);
        let markers: Vec<Scene> = hits
            .iter()
            .map(|(i, p)| {
                let color = self.series[*i]
                    .color
                    .unwrap_or_else(|| self.palette.color(*i));
                marker_node(
                    plot.x.map(p.x),
                    plot.y.map(p.y),
                    radius,
                    color,
                    format!("{}.inspect.marker.{i}", self.tag_prefix),
                )
            })
            .collect();

        let tooltip = self.inspect_tooltip(plot, focus_pixel, focus_x, &hits, style, steps);
        Some(Inspect {
            crosshair,
            markers,
            tooltip,
        })
    }

    /// The inspect tooltip: a rounded box, an `x = …` header, and one
    /// series-coloured value line per hit series.
    fn inspect_tooltip(
        &self,
        plot: &Plot,
        focus_pixel: f32,
        focus_x: f64,
        hits: &[(usize, DataPoint)],
        style: &ChartStyle,
        steps: Steps,
    ) -> Vec<Scene> {
        let size = style.label_size_px.max(1);
        let line_h = size + 6;
        let pad = 8;
        let width = 132;
        let rows = u32::try_from(hits.len()).unwrap_or(0) + 1; // header + values
        let height = rows * line_h + pad;
        // Place right of the crosshair; flip left if it would overflow.
        let mut box_x = to_u32(focus_pixel) + 12;
        if box_x + width > to_u32(plot.right) {
            box_x = to_u32(focus_pixel).saturating_sub(width + 12);
        }
        let box_y = to_u32(plot.top) + 8;
        let text_x = box_x + pad;

        let mut out = vec![rounded_box_node(
            Rect::new(box_x, box_y, width, height),
            style.tooltip_bg,
            6,
            format!("{}.inspect.tooltip", self.tag_prefix),
        )];
        let mut ty = box_y + pad / 2;
        out.push(label_node(
            format!("x = {}", format_axis_tick(focus_x, steps.x)),
            text_x,
            ty,
            width - pad * 2,
            TextAlign::Start,
            style.tooltip_fg,
            size,
            format!("{}.inspect.header", self.tag_prefix),
        ));
        ty += line_h;
        for (i, p) in hits {
            let color = self.series[*i]
                .color
                .unwrap_or_else(|| self.palette.color(*i));
            out.push(label_node(
                format!(
                    "{}  {}",
                    self.series[*i].name,
                    format_axis_tick(p.y, steps.y)
                ),
                text_x,
                ty,
                width - pad * 2,
                TextAlign::Start,
                color,
                size,
                format!("{}.inspect.value.{i}", self.tag_prefix),
            ));
            ty += line_h;
        }
        out
    }
}

/// Per-axis tick step — the precision axis and tooltip labels format at.
#[derive(Debug, Clone, Copy)]
struct Steps {
    x: f64,
    y: f64,
}

/// The gap between consecutive ticks, or 0 for a degenerate axis (one or
/// no tick) — `tick_decimals(0.0)` is 0, i.e. whole-number labels.
fn step_of(ticks: &[f64]) -> f64 {
    match ticks {
        [a, b, ..] => (b - a).abs(),
        _ => 0.0,
    }
}

/// The three inspect layers, kept separate so `build` can interleave
/// them at the right paint depths (crosshair behind series, markers on
/// top of the lines, tooltip above everything).
struct Inspect {
    crosshair: Scene,
    markers: Vec<Scene>,
    tooltip: Vec<Scene>,
}

/// Append `p` unless it duplicates the last pushed point (a vertex that
/// sits exactly on a boundary is both an in-range point and a crossing).
fn push_unique(out: &mut Vec<DataPoint>, p: DataPoint) {
    let dup = out
        .last()
        .is_some_and(|l| (l.x - p.x).abs() < f64::EPSILON && (l.y - p.y).abs() < f64::EPSILON);
    if !dup {
        out.push(p);
    }
}

/// Clip a point sequence to the x range `[lo, hi]`, interpolating y at
/// each boundary crossing so the polyline meets the plot edge exactly
/// instead of extrapolating outside it. Points must already be finite.
///
/// Without this a pinned / zoomed x-domain paints its lines straight
/// through the axes: a path's `rect` is a bounding box for layout and
/// hit-test, never a clip, so the paint adapter happily rasterizes a
/// vertex that falls outside it (R1358 rebased the commands onto the
/// rect's origin; it did not make the rect clip them).
fn clip_to_x_domain(points: &[DataPoint], lo: f64, hi: f64) -> Vec<DataPoint> {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let inside = |x: f64| x >= lo && x <= hi;
    let mut out: Vec<DataPoint> = Vec::new();
    match points.len() {
        0 => return out,
        1 => {
            if inside(points[0].x) {
                out.push(points[0]);
            }
            return out;
        }
        _ => {}
    }
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        if inside(a.x) {
            push_unique(&mut out, a);
        }
        // Boundary crossings on this segment, in travel order.
        let mut crossings: Vec<DataPoint> = Vec::new();
        for bound in [lo, hi] {
            let crosses = (a.x < bound && b.x > bound) || (a.x > bound && b.x < bound);
            let span = b.x - a.x;
            if crosses && span.abs() > f64::EPSILON {
                let t = (bound - a.x) / span;
                crossings.push(DataPoint::new(bound, a.y + t * (b.y - a.y)));
            }
        }
        crossings.sort_by(|p, q| {
            (p.x - a.x)
                .abs()
                .partial_cmp(&(q.x - a.x).abs())
                .unwrap_or(core::cmp::Ordering::Equal)
        });
        for c in crossings {
            push_unique(&mut out, c);
        }
    }
    if let Some(&last) = points.last()
        && inside(last.x)
    {
        push_unique(&mut out, last);
    }
    out
}

/// The series point whose x is nearest `data_x`, restricted to the
/// visible x range `[lo, hi]` so a zoomed chart never inspects a point
/// outside the plot. Non-finite points are skipped.
fn nearest_point_in(series: &Series, data_x: f64, lo: f64, hi: f64) -> Option<DataPoint> {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    series
        .points
        .iter()
        .filter(|p| p.x.is_finite() && p.y.is_finite() && p.x >= lo && p.x <= hi)
        .copied()
        .min_by(|a, b| {
            (a.x - data_x)
                .abs()
                .partial_cmp(&(b.x - data_x).abs())
                .unwrap_or(core::cmp::Ordering::Equal)
        })
}

/// A filled circle marker at `(cx, cy)`, approximated by four cubic
/// Béziers (no arc `PathCommand`; kappa = 0.5522847498).
fn marker_node(cx: f32, cy: f32, r: u32, fill: Color, tag: String) -> Scene {
    let bbox = Rect::new(
        to_u32(cx - to_f32(r)),
        to_u32(cy - to_f32(r)),
        r * 2 + 1,
        r * 2 + 1,
    );
    // R1358 — rect-relative commands; the circle centres on its own bbox.
    let commands = circle_commands(cx - to_f32(bbox.x), cy - to_f32(bbox.y), to_f32(r));
    Scene::Path(
        PathNode::new(bbox, commands, PathStyle::filled(fill))
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    )
}

fn circle_commands(cx: f32, cy: f32, r: f32) -> Vec<PathCommand> {
    let k = 0.552_285 * r;
    let p = |x: f32, y: f32| PathPoint::new(x, y);
    vec![
        PathCommand::MoveTo(p(cx + r, cy)),
        PathCommand::CurveTo {
            c1: p(cx + r, cy + k),
            c2: p(cx + k, cy + r),
            end: p(cx, cy + r),
        },
        PathCommand::CurveTo {
            c1: p(cx - k, cy + r),
            c2: p(cx - r, cy + k),
            end: p(cx - r, cy),
        },
        PathCommand::CurveTo {
            c1: p(cx - r, cy - k),
            c2: p(cx - k, cy - r),
            end: p(cx, cy - r),
        },
        PathCommand::CurveTo {
            c1: p(cx + k, cy - r),
            c2: p(cx + r, cy - k),
            end: p(cx + r, cy),
        },
        PathCommand::Close,
    ]
}

fn rounded_box_node(rect: Rect, fill: Color, radius: u32, tag: String) -> Scene {
    Scene::Box(
        BoxNode::new(rect, BoxStyle::filled(fill).with_corner_radius(radius))
            .with_tag(tag)
            .with_layout(absolute(rect)),
    )
}

/// The resolved plot rectangle (in float pixels) plus the two scales.
#[derive(Debug, Clone, Copy)]
struct Plot {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    x: crate::scale::LinearScale,
    y: crate::scale::LinearScale,
}

impl Plot {
    fn resolve(
        rect: Rect,
        series: &[Series],
        x_domain: Option<(f64, f64)>,
        y_domain: Option<(f64, f64)>,
        style: &ChartStyle,
    ) -> Self {
        let m = style.margin;
        let x0 = rect.x + m.left;
        let y0 = rect.y + m.top;
        let x1 = (rect.x + rect.w).saturating_sub(m.right).max(x0 + 1);
        let y1 = (rect.y + rect.h).saturating_sub(m.bottom).max(y0 + 1);
        let (left, right, top, bottom) = (to_f32(x0), to_f32(x1), to_f32(y0), to_f32(y1));

        let bounds = data_bounds(series);
        let raw_x = x_domain.or(bounds.map(|b| b.x)).unwrap_or((0.0, 1.0));
        let raw_y = y_domain.or(bounds.map(|b| b.y)).unwrap_or((0.0, 1.0));
        // Auto domains snap to the nice-tick extent so the outer gridlines
        // land on the plot edges (Qt `applyNiceNumbers`); a pinned domain
        // is honoured verbatim.
        let dom_x = domain_from_ticks(x_domain, raw_x, style.x_ticks);
        let dom_y = domain_from_ticks(y_domain, raw_y, style.y_ticks);

        Self {
            left,
            right,
            top,
            bottom,
            x: crate::scale::LinearScale::new(dom_x, (left, right)),
            // y is inverted: domain-hi maps to the top (small pixel).
            y: crate::scale::LinearScale::new(dom_y, (bottom, top)),
        }
    }
}

/// Resolve a final domain: a pinned domain verbatim, else the nice-tick
/// extent of the raw data domain (falling back to the raw domain when
/// ticks are unavailable, e.g. a collapsed range).
fn domain_from_ticks(pinned: Option<(f64, f64)>, raw: (f64, f64), target: usize) -> (f64, f64) {
    if let Some(d) = pinned {
        return d;
    }
    let ticks = nice_ticks(raw.0, raw.1, target);
    match (ticks.first(), ticks.last()) {
        (Some(&lo), Some(&hi)) if (hi - lo).abs() > f64::EPSILON => (lo, hi),
        _ => raw,
    }
}

fn in_domain(value: f64, lo: f64, hi: f64) -> bool {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    value >= lo - f64::EPSILON && value <= hi + f64::EPSILON
}

fn clamp(value: f64, lo: f64, hi: f64) -> f64 {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    value.max(lo).min(hi)
}

/// A stroked polyline path from plot-space points.
fn stroke_path(points: &[(f32, f32)], stroke: Stroke, tag: String) -> Scene {
    let bbox = bbox_of(points, stroke.width);
    let commands = polyline_commands(&rebased(points, bbox), false);
    Scene::Path(
        PathNode::new(bbox, commands, PathStyle::stroked(stroke))
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    )
}

/// A filled area path: the polyline dropped to `baseline_y` and closed.
fn area_path(points: &[(f32, f32)], baseline_y: f32, fill: Color, tag: String) -> Scene {
    // The bbox must be resolved BEFORE the commands: the baseline union can
    // move the box's origin (a baseline above every point lifts `bbox.y`), and
    // R1358 rebases the commands onto that final origin.
    let mut bbox = bbox_of(points, 0);
    bbox = bbox.union(Rect::new(bbox.x, to_u32(baseline_y), 1, 1));
    let (ox, oy) = (to_f32(bbox.x), to_f32(bbox.y));
    let mut commands = polyline_commands(&rebased(points, bbox), false);
    if let (Some(&(last_x, _)), Some(&(first_x, _))) = (points.last(), points.first()) {
        commands.push(PathCommand::LineTo(PathPoint::new(
            last_x - ox,
            baseline_y - oy,
        )));
        commands.push(PathCommand::LineTo(PathPoint::new(
            first_x - ox,
            baseline_y - oy,
        )));
        commands.push(PathCommand::Close);
    }
    Scene::Path(
        PathNode::new(bbox, commands, PathStyle::filled(fill))
            .with_tag(tag)
            .with_layout(absolute(bbox)),
    )
}

/// R1358 — rebase plot-space points onto `bbox`'s origin so the emitted
/// [`PathCommand`]s are relative to the path node's own rect, which is what
/// positions it. Subtracting exactly the origin the node carries makes the
/// rebase pixel-exact: the paint adapter translates by the same value, and a
/// `bbox_of` origin clamped at 0 stays consistent with the commands built
/// from it.
fn rebased(points: &[(f32, f32)], bbox: Rect) -> Vec<(f32, f32)> {
    let (ox, oy) = (to_f32(bbox.x), to_f32(bbox.y));
    points.iter().map(|&(x, y)| (x - ox, y - oy)).collect()
}

fn polyline_commands(points: &[(f32, f32)], close: bool) -> Vec<PathCommand> {
    let mut commands = Vec::with_capacity(points.len() + usize::from(close));
    for (i, &(x, y)) in points.iter().enumerate() {
        let p = PathPoint::new(x, y);
        if i == 0 {
            commands.push(PathCommand::MoveTo(p));
        } else {
            commands.push(PathCommand::LineTo(p));
        }
    }
    if close && !points.is_empty() {
        commands.push(PathCommand::Close);
    }
    commands
}

fn box_node(rect: Rect, fill: Color, tag: String) -> Scene {
    Scene::Box(
        BoxNode::new(rect, BoxStyle::filled(fill))
            .with_tag(tag)
            .with_layout(absolute(rect)),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "a label is intrinsically a box + text + alignment tuple; grouping them into a struct would not reduce the real parameter count"
)]
fn label_node(
    text: impl Into<String>,
    x: u32,
    y: u32,
    width: u32,
    align: TextAlign,
    color: Color,
    size: u32,
    tag: String,
) -> Scene {
    Scene::Text(
        TextNode::styled(
            text,
            Rect::default(),
            TextStyle::new()
                .with_size_px(size)
                .with_fg(color)
                .with_align(align),
        )
        .with_tag(tag)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(x, y)
                .with_size(Size::px(width.max(1), size + 4)),
        ),
    )
}

fn absolute(rect: Rect) -> LayoutStyle {
    LayoutStyle::new()
        .with_absolute_position(rect.x, rect.y)
        .with_size(Size::px(rect.w.max(1), rect.h.max(1)))
}

fn bbox_of(points: &[(f32, f32)], pad: u32) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for &(x, y) in points {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    if !min_x.is_finite() {
        return Rect::default();
    }
    let pad_f = to_f32(pad);
    let x = to_u32(min_x - pad_f);
    let y = to_u32(min_y - pad_f);
    let w = to_u32(max_x - min_x) + pad * 2 + 1;
    let h = to_u32(max_y - min_y) + pad * 2 + 1;
    Rect::new(x, y, w, h)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "pixel coordinate u32 -> f32; display-bounded magnitudes"
)]
fn to_f32(v: u32) -> f32 {
    v as f32
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "pixel f32 -> u32; rounded and clamped non-negative"
)]
fn to_u32(v: f32) -> u32 {
    v.round().max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::series::DataPoint;

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(scene);
                }
                c.children.iter().find_map(|ch| find(ch, tag))
            }
            other => {
                if other.tag() == Some(tag) {
                    Some(scene)
                } else {
                    None
                }
            }
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
                vec![DataPoint::new(0.0, 0.0), DataPoint::new(3600.0, 3600.0)],
            ),
            Series::new(
                "b",
                vec![DataPoint::new(0.0, 1000.0), DataPoint::new(3600.0, 500.0)],
            ),
        ]
    }

    fn no_legend_zero_margin() -> ChartStyle {
        ChartStyle {
            margin: Margin::uniform(0),
            legend: false,
            background: None,
            ..ChartStyle::default()
        }
    }

    #[test]
    fn root_carries_the_prefix_tag() {
        let scene =
            LineChart::new(two_series()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "chart").is_some());
    }

    #[test]
    fn one_series_path_per_series_with_point_count_commands() {
        let scene =
            LineChart::new(two_series()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        for i in 0..2 {
            let s = find(&scene, &format!("chart.series.{i}")).expect("series path");
            let Scene::Path(p) = s else {
                panic!("series is a path")
            };
            assert_eq!(p.commands.len(), 2, "MoveTo + one LineTo");
            assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
            assert!(matches!(p.commands[1], PathCommand::LineTo(_)));
        }
    }

    #[test]
    fn area_paths_only_when_filled_and_are_closed() {
        let plain =
            LineChart::new(two_series()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&plain, "chart.area.0").is_none());

        let filled = LineChart::new(two_series())
            .filled(true)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let a = find(&filled, "chart.area.0").expect("area path when filled");
        let Scene::Path(p) = a else {
            panic!("area is a path")
        };
        assert!(matches!(p.commands.last(), Some(PathCommand::Close)));
        assert!(p.style.fill.is_some());
        assert!(p.style.stroke.is_none());
    }

    #[test]
    fn gridline_and_label_counts_match_in_domain_ticks() {
        // y in [0,3600] target 5 -> ticks 0,1000,2000,3000,4000; auto
        // domain snaps to [0,4000] so all five are in-domain.
        let scene = LineChart::new(two_series()).build(
            Rect::new(0, 0, 400, 300),
            &ChartStyle {
                y_ticks: 5,
                ..ChartStyle::default()
            },
        );
        assert_eq!(count_prefix(&scene, "chart.grid.y."), 5);
        assert_eq!(count_prefix(&scene, "chart.label.y."), 5);
    }

    #[test]
    fn legend_entries_track_series_and_toggle_off() {
        let on =
            LineChart::new(two_series()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(count_prefix(&on, "chart.legend.0"), 2); // swatch + label
        assert_eq!(count_prefix(&on, "chart.legend.1"), 2);

        let off = LineChart::new(two_series()).build(
            Rect::new(0, 0, 400, 300),
            &ChartStyle {
                legend: false,
                ..ChartStyle::default()
            },
        );
        assert_eq!(count_prefix(&off, "chart.legend."), 0);
    }

    #[test]
    fn explicit_series_colour_overrides_palette() {
        let series = vec![
            Series::new(
                "x",
                vec![DataPoint::new(0.0, 0.0), DataPoint::new(1.0, 1.0)],
            )
            .with_color(Color::rgb(0x11, 0x22, 0x33)),
        ];
        let scene =
            LineChart::new(series).build(Rect::new(0, 0, 200, 200), &no_legend_zero_margin());
        let Scene::Path(p) = find(&scene, "chart.series.0").expect("series") else {
            panic!("path")
        };
        assert_eq!(p.style.stroke.unwrap().color, Color::rgb(0x11, 0x22, 0x33));
    }

    #[test]
    fn coordinate_pipeline_maps_points_to_plot_edges() {
        // rect 200x100, zero margins, pinned unit domains: the point
        // (0,0) sits at (left=0, bottom=100), (1,1) at (right=200, top=0).
        let series = vec![Series::new(
            "u",
            vec![DataPoint::new(0.0, 0.0), DataPoint::new(1.0, 1.0)],
        )];
        let scene = LineChart::new(series)
            .with_x_domain(0.0, 1.0)
            .with_y_domain(0.0, 1.0)
            .build(Rect::new(0, 0, 200, 100), &no_legend_zero_margin());
        let Scene::Path(p) = find(&scene, "chart.series.0").expect("series") else {
            panic!("path")
        };
        let PathCommand::MoveTo(m) = p.commands[0] else {
            panic!("first is MoveTo")
        };
        let PathCommand::LineTo(l) = p.commands[1] else {
            panic!("second is LineTo")
        };
        assert!(
            (m.x - 0.0).abs() < 0.01 && (m.y - 100.0).abs() < 0.01,
            "start at bottom-left, got {m:?}"
        );
        assert!(
            (l.x - 200.0).abs() < 0.01 && (l.y - 0.0).abs() < 0.01,
            "end at top-right, got {l:?}"
        );
    }

    #[test]
    fn axes_are_present() {
        let scene =
            LineChart::new(two_series()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "chart.axis.x").is_some());
        assert!(find(&scene, "chart.axis.y").is_some());
    }

    #[test]
    fn empty_series_still_builds_a_container() {
        let scene =
            LineChart::new(Vec::new()).build(Rect::new(0, 0, 300, 200), &ChartStyle::default());
        assert!(find(&scene, "chart").is_some());
        // No panic, no series paths.
        assert_eq!(count_prefix(&scene, "chart.series."), 0);
    }

    #[test]
    fn degenerate_rect_does_not_panic() {
        let scene =
            LineChart::new(two_series()).build(Rect::new(10, 10, 4, 4), &ChartStyle::default());
        assert!(find(&scene, "chart").is_some());
    }

    #[test]
    fn pinned_domain_clips_series_to_the_plot() {
        // Data spans x 0..10 but the domain is pinned to 4..6 (a zoom).
        // Every emitted vertex must sit inside the plot's x range —
        // without clipping the polyline extrapolates far outside it.
        let series = vec![Series::new(
            "s",
            (0..=10)
                .map(|i| DataPoint::new(f64::from(i), 5.0))
                .collect(),
        )];
        let scene = LineChart::new(series)
            .with_x_domain(4.0, 6.0)
            .with_y_domain(0.0, 10.0)
            .build(Rect::new(0, 0, 200, 100), &no_legend_zero_margin());
        let Scene::Path(p) = find(&scene, "chart.series.0").expect("series") else {
            panic!("path")
        };
        // R1358 — commands are relative to the path's OWN rect, so the vertex
        // must be read back as `rect.x + command.x` to be a plot-space claim.
        // Testing the bare command here would be vacuous: a rect-local x is
        // ~0-based by construction and would sit inside 0..200 even for a
        // series that escaped the plot entirely.
        let origin = to_f32(p.rect.x);
        for cmd in &p.commands {
            let (PathCommand::MoveTo(pt) | PathCommand::LineTo(pt)) = *cmd else {
                continue;
            };
            let x = origin + pt.x;
            assert!(
                (-0.01..=200.01).contains(&x),
                "vertex x={x} escaped the 0..200 plot"
            );
        }
    }

    #[test]
    fn r1358_series_commands_are_relative_to_the_paths_own_rect() {
        // The R1358 contract, pinned at the chart's most-scrutinised producer:
        // a series' geometry is authored in its own box and PLACED by its
        // rect, so `rect.origin + command` is the plot pixel the scale maps to
        // — and the bare commands are NOT those pixels.
        //
        // The chart is built away from the window origin so the two bases are
        // distinguishable; before R1358 the commands WERE the window pixels
        // and this test's second half would fail.
        let series = vec![Series::new(
            "s",
            (0..=4).map(|i| DataPoint::new(f64::from(i), 5.0)).collect(),
        )];
        let scene = LineChart::new(series)
            .with_x_domain(0.0, 4.0)
            .with_y_domain(0.0, 10.0)
            .build(Rect::new(300, 200, 200, 100), &no_legend_zero_margin());
        let Scene::Path(p) = find(&scene, "chart.series.0").expect("series") else {
            panic!("path")
        };
        // The rect sits at the plot's left edge less the stroke's bbox pad —
        // near 300, and unambiguously far from the window origin, which is
        // what makes "local vs window" distinguishable below.
        assert!(
            (250..=300).contains(&p.rect.x),
            "series rect.x={} tracks the chart placed at x=300 (less stroke pad)",
            p.rect.x
        );
        let verts: Vec<(f32, f32)> = p
            .commands
            .iter()
            .filter_map(|cmd| match *cmd {
                PathCommand::MoveTo(pt) | PathCommand::LineTo(pt) => Some((pt.x, pt.y)),
                _ => None,
            })
            .collect();
        assert_eq!(verts.len(), 5, "one vertex per data point");
        // Every vertex is rect-local: inside its own box, and NOT the window px.
        for (i, (vx, vy)) in verts.iter().enumerate() {
            assert!(
                *vx >= -0.01 && *vx <= to_f32(p.rect.w) + 0.01,
                "vertex {i} x={vx} is local to the rect (w={})",
                p.rect.w
            );
            assert!(
                *vx < 300.0,
                "vertex {i} x={vx} must NOT be the window px (the rect carries that)"
            );
            let _ = vy;
        }
        // …and the sum is the plot pixel: a flat y=5 series in domain 0..10
        // over a 100px-tall plot placed at y=200 sits at the vertical middle.
        let first_window_x = to_f32(p.rect.x) + verts[0].0;
        let last_window_x = to_f32(p.rect.x) + verts[4].0;
        assert!(
            (first_window_x - 300.0).abs() < 1.5,
            "x=0 maps to the plot's left edge (window 300), got {first_window_x}"
        );
        assert!(
            (last_window_x - 500.0).abs() < 1.5,
            "x=4 maps to the plot's right edge (window 500), got {last_window_x}"
        );
        let first_window_y = to_f32(p.rect.y) + verts[0].1;
        assert!(
            (first_window_y - 250.0).abs() < 1.5,
            "y=5 of 0..10 maps to the plot's vertical middle (window 250), \
             got {first_window_y}"
        );
    }

    #[test]
    fn clipping_interpolates_the_boundary_crossing() {
        // A single segment 0..10 rising 0..10, domain pinned to 2..8:
        // the clipped polyline must start at (2, 2) and end at (8, 8) —
        // interpolated crossings, not the original endpoints.
        let clipped = clip_to_x_domain(
            &[DataPoint::new(0.0, 0.0), DataPoint::new(10.0, 10.0)],
            2.0,
            8.0,
        );
        assert_eq!(clipped.len(), 2, "two boundary crossings");
        assert!((clipped[0].x - 2.0).abs() < 1e-9 && (clipped[0].y - 2.0).abs() < 1e-9);
        assert!((clipped[1].x - 8.0).abs() < 1e-9 && (clipped[1].y - 8.0).abs() < 1e-9);
    }

    #[test]
    fn no_inspect_overlay_by_default() {
        let scene =
            LineChart::new(two_series()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "chart.inspect.crosshair").is_none());
        assert!(find(&scene, "chart.inspect.tooltip").is_none());
        assert_eq!(count_prefix(&scene, "chart.inspect.marker."), 0);
    }

    #[test]
    fn inspect_emits_crosshair_markers_and_tooltip() {
        let scene = LineChart::new(two_series())
            .inspect(Some(0.5))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "chart.inspect.crosshair").is_some());
        assert!(find(&scene, "chart.inspect.tooltip").is_some());
        assert!(find(&scene, "chart.inspect.header").is_some());
        // One marker + one value line per series.
        assert_eq!(count_prefix(&scene, "chart.inspect.marker."), 2);
        assert_eq!(count_prefix(&scene, "chart.inspect.value."), 2);
    }

    #[test]
    fn inspect_marker_is_a_closed_filled_circle_path() {
        let scene = LineChart::new(two_series())
            .inspect(Some(0.5))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let Scene::Path(p) = find(&scene, "chart.inspect.marker.0").expect("marker") else {
            panic!("marker is a path")
        };
        // MoveTo + 4 CurveTo + Close (a Bézier circle).
        assert_eq!(p.commands.len(), 6);
        assert!(matches!(p.commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(p.commands[1], PathCommand::CurveTo { .. }));
        assert!(matches!(p.commands.last(), Some(PathCommand::Close)));
        assert!(p.style.fill.is_some());
    }

    #[test]
    fn inspect_snaps_to_the_nearest_data_point() {
        // Single series at integer x 0..3; a fraction near the right edge
        // must snap the crosshair to the last point, not interpolate.
        let series = vec![Series::new(
            "s",
            vec![
                DataPoint::new(0.0, 10.0),
                DataPoint::new(1.0, 20.0),
                DataPoint::new(2.0, 30.0),
                DataPoint::new(3.0, 40.0),
            ],
        )];
        let scene = LineChart::new(series)
            .with_x_domain(0.0, 3.0)
            .with_y_domain(0.0, 40.0)
            .inspect(Some(1.0)) // far right -> nearest is x=3, value 40
            .build(Rect::new(0, 0, 200, 100), &no_legend_zero_margin());
        let Scene::Text(header) = find(&scene, "chart.inspect.header").expect("header") else {
            panic!("header is text")
        };
        // The header formats at the x-axis's own precision: this domain
        // (0..3, 6 target ticks) resolves to a 0.5 step, so the axis reads
        // `0.0 / 0.5 / … / 3.0` and the header must agree — `x = 3` would
        // contradict the gridline it points at.
        assert_eq!(header.content, "x = 3.0");
        let Scene::Text(value) = find(&scene, "chart.inspect.value.0").expect("value") else {
            panic!("value is text")
        };
        assert_eq!(value.content, "s  40");
    }
}
