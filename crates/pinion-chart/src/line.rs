//! The line / area chart builder — projects [`Series`] data into a
//! retained [`Scene`] of axes, gridlines, tick labels, series polylines,
//! optional area fills, and a legend.
//!
//! # Coordinate contract — one body, two placements
//!
//! Every child is authored by resolving [`CartesianPlot`] against the `rect` this
//! module is handed, and `rect` enters only as an **additive origin**. So
//! the same builder yields either placement:
//!
//! * [`LineChart::build_fill(size)`](LineChart::build_fill) (R1360) passes
//!   `Rect::new(0, 0, w, h)`, which makes every child local to `(0, 0)`,
//!   and hands back a **fill-parent** root. taffy sizes and places that
//!   root; each child's `absolute_position` is parent-relative (R55.D.6),
//!   so it resolves against wherever the root lands. **This is the
//!   layout-native entry point** — dock it, flex it, resize it.
//! * [`LineChart::build(rect)`](LineChart::build) passes a window-absolute
//!   rect, which makes every child window-absolute, under a root that
//!   declares no layout. It is therefore correct **only under a root at the
//!   window origin** — a real precondition, and the reason `build_fill` is
//!   preferred for anything new.
//!
//! R1358 is what made the first bullet possible: before it, `Scene::Path`
//! painted `commands` at literal device coordinates and ignored the rect
//! layout assigned, so a path could not participate in layout at all and no
//! chart-side change could have fixed it. R1358 made commands relative to
//! the node's own rect (the basis the R722 gradient UV already used); every
//! path here declares `absolute_position` + size, which is why that
//! migration was pixel-identical for this crate.
//!
//! ## What "authored in the chart's frame" does and does not promise
//!
//! It means the origin: a child's position is `chart_origin + local`. It
//! does **not** promise containment — nothing clips a `Container`'s
//! children, and two children reach outside a tight slot:
//!
//! * an x tick label is a fixed 60px slot centred on its tick. The
//!   rightmost tick sits at `plot.right` = `w - margin.right`, so the slot
//!   ends at `w + slot/2 - margin.right` — with the defaults, **14px past
//!   the chart's own right edge, at every size**.
//! * the legend lays out at a fixed 104px per series starting at
//!   `margin.left`, so it needs `margin.left + 104 * series` px of width
//!   (260px for two series with the defaults) and silently overruns below
//!   that.
//!
//! Neither is new (both predate R1360 and are what the margins exist to
//! absorb), but a docked chart narrower than its legend will paint over its
//! neighbour. Clamping them is a follow-up.
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
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{Color, Stroke, StrokeCap, TextAlign};

use crate::draw::{
    CalloutRow, absolute, area_path, box_node, callout, fill_parent, label_node, marker_node,
    stroke_path, to_u32,
};
use crate::palette::CategoricalPalette;
use crate::plot::{CartesianPlot, axis_ticks};
use crate::series::{DataPoint, Series};
use crate::style::ChartStyle;
use crate::ticks::{format_axis_tick, tick_step};

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
    legend_tags: Option<Vec<String>>,
    rescale_to_visible: bool,
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
            legend_tags: None,
            rescale_to_visible: false,
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

    /// Make the legend **interactive** (R1380): each entry becomes a focusable,
    /// hit-testable region tagged with the caller's `tags[i]` (one tag per
    /// series, in series order), and a hidden series' entry renders muted (a
    /// grey swatch + a dimmed label — the "this line is hidden" affordance).
    ///
    /// This gives the chart-owned legend the hit geometry it lacked (the R1379
    /// follow-up): a click / press anywhere on entry `i` resolves — through the
    /// router's deepest-tagged-ancestor hit-test, exactly as a
    /// [`toggle_group`](pinion_core::widgets::toggle_group) chip does — to the
    /// [`External`](pinion_core::external::External) the caller binds to
    /// `tags[i]`, and the entry is its own `Tab` stop. The chart stays a pure
    /// scene producer: it emits the tagged, focusable entries; the caller owns
    /// the tag namespace and wires the toggles (so the legend can drive
    /// [`Series::visible`](crate::Series::visible) without the chart importing
    /// any widget). Requires [`ChartStyle::legend`](crate::ChartStyle::legend)
    /// `= true`; entries beyond `tags.len()` fall back to no legend entry.
    #[must_use]
    pub fn interactive_legend(mut self, tags: Vec<String>) -> Self {
        self.legend_tags = Some(tags);
        self
    }

    /// Rescale the auto-domain to the **visible** series (R1381): when `true`,
    /// hiding the dominant series lets the axes snap to the remaining visible
    /// ones so a small series becomes readable, instead of staying pinned to
    /// the (now-hidden) big one. Default `false` — every series is measured, so
    /// the grid is stable across toggles (the R1379 dashboard default). When
    /// every series is hidden the domain falls back to the all-series bounds
    /// (the grid stays put rather than collapsing), and a pinned
    /// [`with_x_domain`](Self::with_x_domain) /
    /// [`with_y_domain`](Self::with_y_domain) still wins over either.
    #[must_use]
    pub fn rescale_to_visible(mut self, rescale: bool) -> Self {
        self.rescale_to_visible = rescale;
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

    /// Build the chart PINNED to `rect` — the caller states the geometry and
    /// the chart occupies exactly it.
    ///
    /// The body is authored in the chart's own `(0, 0)..(w, h)` frame and the
    /// root is placed with `absolute_position(rect.xy)`, so — unlike the
    /// pre-R1360.4 shape, which baked `rect`'s origin into every child and
    /// was therefore correct **only** under a root at the window origin —
    /// this is correct under any parent. Pixel-identical at the window
    /// origin, which is where its one consumer sits.
    ///
    /// `rect` must still be known before the layout pass runs. When the
    /// geometry should come *from* layout instead, use
    /// [`Self::build_fill`].
    #[must_use]
    pub fn build(&self, rect: Rect, style: &ChartStyle) -> Scene {
        Scene::Container(
            self.build_body(Rect::new(0, 0, rect.w, rect.h), style)
                .with_layout(absolute(rect)),
        )
    }

    /// Build the chart as a **layout-native** subtree (R1360): the root fills
    /// its layout slot, so the chart is *placed and sized by layout* — dock
    /// it, flex it, resize its container.
    ///
    /// R1358 is the prerequisite: it made a [`Scene::Path`]'s commands
    /// relative to its own rect, so the body's local paths paint correctly
    /// wherever the root lands. Before it they were welded to a window
    /// coordinate and no chart-side change could have helped.
    ///
    /// `size` is the slot to fill. The consumer gets it from
    /// [`use_pane_viewport_size`](pinion_core::use_pane_viewport_size) keyed
    /// on `tag_prefix`: the shell publishes the root's measured rect after
    /// layout and its same-frame re-pass rebuilds at that size
    /// (`examples/hello-chart-fill` is the worked example). The `(0, 0)` =
    /// unmeasured sentinel returns an empty tagged root that still *measures*
    /// — its size is its slot's, not its content's — so the size feeds back
    /// on the very next paint and the loop can bootstrap.
    ///
    /// Read the module's "Known limitations" first: this seam is published
    /// only by the live Vello paint, so on TUI the chart is empty, and a
    /// `scene/layout {viewport}` query is incoherent for it.
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

    /// The chart body, authored in the frame `rect` describes — the ONE
    /// builder both entry points wrap.
    ///
    /// Split out at R1360.4 so placement is a policy applied by the caller
    /// rather than a mutation of an already-returned tree: `build_fill` used
    /// to call `build` and then overwrite the root's `layout` through an
    /// `if let`, which silently no-ops the day `build` returns another
    /// variant (the R832 class). Returning a `ContainerNode` makes the
    /// wrapper's `with_layout` total.
    ///
    /// `rect` enters only as an additive origin, so callers pass a
    /// zero-origin rect to get a local-frame body.
    fn build_body(&self, rect: Rect, style: &ChartStyle) -> ContainerNode {
        let plot = CartesianPlot::resolve(
            rect,
            &self.series,
            self.x_domain,
            self.y_domain,
            style,
            self.rescale_to_visible,
        );
        let (y_lo, y_hi) = plot.y.domain();
        let (x_lo, x_hi) = plot.x.domain();
        // One tick-set resolver, shared with `inspect_readout` so the
        // painted axis and the AT readout format at the same precision.
        let x_ticks = axis_ticks((x_lo, x_hi), style.x_ticks);
        let y_ticks = axis_ticks((y_lo, y_hi), style.y_ticks);
        let baseline = clamp(0.0, y_lo, y_hi);
        // The label precision each axis formats at (R1359: `format_si` alone
        // collapsed every sub-0.1 step onto one rounded digit).
        let steps = Steps {
            x: tick_step(&x_ticks),
            y: tick_step(&y_ticks),
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

        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
    }

    /// Horizontal (per y-tick) and vertical (per x-tick) gridlines. Maps the
    /// ticks to pixels and hands them to the shared [`crate::draw::gridlines`]
    /// (R1377) — the pure "draw lines at these positions" primitive the bar and
    /// scatter charts also emit.
    fn gridlines(
        &self,
        plot: &CartesianPlot,
        x_ticks: &[f64],
        y_ticks: &[f64],
        style: &ChartStyle,
    ) -> Vec<Scene> {
        let x_pos: Vec<f32> = x_ticks.iter().map(|&t| plot.x.map(t)).collect();
        let y_pos: Vec<f32> = y_ticks.iter().map(|&t| plot.y.map(t)).collect();
        crate::draw::gridlines(
            (plot.left, plot.right, plot.top, plot.bottom),
            &x_pos,
            &y_pos,
            style,
            &self.tag_prefix,
        )
    }

    /// The left (y) and bottom (x) axis lines — the shared L-frame
    /// ([`crate::draw::axes`], R1377).
    fn axes(&self, plot: &CartesianPlot, style: &ChartStyle) -> Vec<Scene> {
        crate::draw::axes(
            (plot.left, plot.right, plot.top, plot.bottom),
            style,
            &self.tag_prefix,
        )
    }

    /// The per-series area fills (when [`filled`](Self::filled)) and
    /// polylines. Areas paint before lines so the stroke sits on top.
    fn series_layer(&self, plot: &CartesianPlot, baseline: f64, style: &ChartStyle) -> Vec<Scene> {
        let baseline_y = plot.y.map(baseline);
        let (x_lo, x_hi) = plot.x.domain();
        let mut out = Vec::new();
        for (i, s) in self.series.iter().enumerate() {
            // A hidden series draws no area / polyline (R1379); it keeps its
            // palette index `i` so the visible series never re-colour.
            if !s.visible {
                continue;
            }
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

    /// Right-aligned y-axis labels (the shared [`crate::draw::y_tick_labels`],
    /// R1377) and centred numeric x-axis labels.
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
        // The numeric x-axis labels stay here: shared only with the scatter chart
        // (two consumers) — the categorical bar chart labels its slots with
        // category names instead — so the numeric x-label loop is deferred from
        // the R1377 lift until a third numeric-x consumer arrives.
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

    /// The legend row (fixed-width slots) in the top margin band. When the
    /// legend is [`interactive`](Self::interactive_legend) each entry is a
    /// focusable, tagged hit region ([`Self::interactive_legend_entries`]);
    /// otherwise it is the shared static [`crate::draw::legend_row`] (R1377) —
    /// a swatch + label per series, positioned at the top-left of the plot.
    fn legend(&self, rect: Rect, style: &ChartStyle) -> Vec<Scene> {
        let start_x = rect.x + style.margin.left;
        let row_y = rect.y + 6;
        if let Some(tags) = &self.legend_tags {
            return self.interactive_legend_entries(tags, start_x, row_y, style);
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
        crate::draw::legend_row(&entries, start_x, row_y, style, &self.tag_prefix)
    }

    /// The interactive legend (R1380): one focusable, hit-testable entry per
    /// series, on the same `LEGEND_SLOT` grid as the static row, each tagged
    /// with the caller's `tags[i]`. The entry is a `Container` whose only
    /// children are an untagged swatch + label, so the router's
    /// deepest-tagged-ancestor hit-test resolves a click anywhere on it to
    /// `tags[i]` (the chip structure, chart-authored). A hidden series' entry
    /// swaps its series-colour swatch for a grey one and dims its label — the
    /// legend then reads as "this line is off" without the geometry going away
    /// (the slot, and the toggle back on, stay put).
    fn interactive_legend_entries(
        &self,
        tags: &[String],
        start_x: u32,
        row_y: u32,
        style: &ChartStyle,
    ) -> Vec<Scene> {
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
        crate::draw::interactive_legend_row(&entries, tags, start_x, row_y, style)
    }

    /// Resolve which data point the inspect cursor is focused on — the shared
    /// [`crate::plot::resolve_focus`] (R1377), gated on this chart's `inspect`
    /// fraction. The single source for "what is the inspector pointing at": the
    /// painted overlay ([`Self::resolve_inspect`]) and the AT-facing readout
    /// ([`Self::inspect_readout`]) both derive from it, so the tooltip a sighted
    /// user sees and the description a screen reader hears can never disagree.
    fn resolve_focus(
        &self,
        plot: &CartesianPlot,
        rect: Rect,
    ) -> Option<(f64, Vec<(usize, DataPoint)>)> {
        crate::plot::resolve_focus(&self.series, self.inspect?, plot, rect)
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
        let plot = CartesianPlot::resolve(
            rect,
            &self.series,
            self.x_domain,
            self.y_domain,
            style,
            self.rescale_to_visible,
        );
        let x_ticks: Vec<f64> = axis_ticks(plot.x.domain(), style.x_ticks);
        let y_ticks: Vec<f64> = axis_ticks(plot.y.domain(), style.y_ticks);
        let steps = Steps {
            x: tick_step(&x_ticks),
            y: tick_step(&y_ticks),
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

    /// Resolve the inspect overlay for the current `inspect` fraction, or
    /// `None` when inspection is off, the plot is degenerate, or there is
    /// no data under the cursor.
    fn resolve_inspect(
        &self,
        plot: &CartesianPlot,
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
    /// series-coloured value line per hit series. Assembles the header + rows
    /// and hands them to the shared [`callout`] placement (R1375) — the box
    /// geometry / right-flip it shares with the bar chart lives there; the
    /// per-series row content is what stays here.
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

/// Per-axis tick step — the precision axis and tooltip labels format at.
#[derive(Debug, Clone, Copy)]
struct Steps {
    x: f64,
    y: f64,
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

fn clamp(value: f64, lo: f64, hi: f64) -> f64 {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    value.max(lo).min(hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::to_f32;
    use crate::series::DataPoint;
    use crate::style::Margin;
    use pinion_core::scene::PathCommand;
    use pinion_core::style::{Size, SizeValue};

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
    fn r1379_hidden_series_drops_area_and_polyline_but_keeps_legend() {
        let mut series = two_series();
        series[1].visible = false;
        let scene = LineChart::new(series)
            .filled(true)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(
            count_prefix(&scene, "chart.series.1"),
            0,
            "hidden: no polyline"
        );
        assert_eq!(
            count_prefix(&scene, "chart.area.1"),
            0,
            "hidden: no fill area"
        );
        assert_eq!(
            count_prefix(&scene, "chart.series.0"),
            1,
            "visible series unaffected"
        );
        assert_eq!(count_prefix(&scene, "chart.area.0"), 1);
        // Geometry drops, but the palette index + legend slot are retained
        // (hiding series 1 never re-colours or re-indexes series 0).
        assert_eq!(count_prefix(&scene, "chart.legend.0"), 2);
        assert_eq!(
            count_prefix(&scene, "chart.legend.1"),
            2,
            "hidden series keeps its legend slot"
        );
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
        // R1358 — this is a PLOT-space claim, so read back the plot position:
        // `rect.origin + command`. Asserting the bare command would pass here
        // only by coincidence (this chart sits at the window origin and the
        // stroke-padded bbox clamps to 0, so the rebase happens to subtract
        // zero) and would then FAIL for a correct chart built anywhere else.
        let (ox, oy) = (to_f32(p.rect.x), to_f32(p.rect.y));
        let (m, l) = ((ox + m.x, oy + m.y), (ox + l.x, oy + l.y));
        assert!(
            (m.0 - 0.0).abs() < 0.01 && (m.1 - 100.0).abs() < 0.01,
            "start at bottom-left, got {m:?}"
        );
        assert!(
            (l.0 - 200.0).abs() < 0.01 && (l.1 - 0.0).abs() < 0.01,
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
        // Two contracts at once, at the chart's most-scrutinised producer:
        //
        // * R1358 — a series' geometry is authored in its OWN box and placed
        //   by its rect, so `rect.origin + command` is the pixel the scale
        //   maps to, and the bare commands are NOT that pixel.
        // * R1360.4 — `build(rect)` now emits a LOCAL body under a root
        //   placed by `absolute(rect)`. Before it, every child carried
        //   `rect`'s origin, which is why the chart only landed correctly
        //   under a root at the window origin.
        //
        // The chart is built away from the origin so the two bases are
        // distinguishable in both directions.
        const AT: Rect = Rect::new(300, 200, 200, 100);
        let series = vec![Series::new(
            "s",
            (0..=4).map(|i| DataPoint::new(f64::from(i), 5.0)).collect(),
        )];
        let scene = LineChart::new(series)
            .with_x_domain(0.0, 4.0)
            .with_y_domain(0.0, 10.0)
            .build(AT, &no_legend_zero_margin());

        // R1360.4: the ROOT carries the placement — the whole chart moves by
        // changing one node, which is what makes it parent-agnostic.
        let Scene::Container(root) = &scene else {
            panic!("root Container")
        };
        assert_eq!(
            root.layout.absolute_position,
            Some((AT.x, AT.y)),
            "build(rect) places the ROOT at rect; a child carrying rect's \
             origin instead is the pre-R1360.4 landmine"
        );

        let Scene::Path(p) = find(&scene, "chart.series.0").expect("series") else {
            panic!("path")
        };
        // The series is local to the CHART, not the window: its rect sits near
        // the chart's own left edge (0, less the stroke's bbox pad), nowhere
        // near 300.
        assert!(
            p.rect.x < 50,
            "series rect.x={} is local to the chart frame, not the window \
             (the root carries the 300)",
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
        // R1358: every vertex is local to its own path rect.
        for (i, (vx, _vy)) in verts.iter().enumerate() {
            assert!(
                *vx >= -0.01 && *vx <= to_f32(p.rect.w) + 0.01,
                "vertex {i} x={vx} is local to the rect (w={})",
                p.rect.w
            );
        }
        // The sum is the plot pixel IN THE CHART'S FRAME: x=0..4 spans a
        // zero-margin 200px plot, and a flat y=5 of domain 0..10 sits at the
        // vertical middle of its 100px height.
        assert!(
            (to_f32(p.rect.x) + verts[0].0).abs() < 1.5,
            "x=0 maps to the chart's left edge (0), got {}",
            to_f32(p.rect.x) + verts[0].0
        );
        assert!(
            ((to_f32(p.rect.x) + verts[4].0) - 200.0).abs() < 1.5,
            "x=4 maps to the chart's right edge (200), got {}",
            to_f32(p.rect.x) + verts[4].0
        );
        assert!(
            ((to_f32(p.rect.y) + verts[0].1) - 50.0).abs() < 1.5,
            "y=5 of 0..10 maps to the chart's vertical middle (50), got {}",
            to_f32(p.rect.y) + verts[0].1
        );
    }

    #[test]
    fn build_fill_root_fills_its_slot_and_children_are_local() {
        // R1360 — the layout-native entry: the root declares a fill-parent
        // size (so taffy sizes it from its slot, not from a window rect) and
        // carries the prefix tag (so `use_pane_viewport_size` can measure it).
        let chart = LineChart::new(two_series()).filled(true);
        let scene = chart.build_fill((300, 200), &no_legend_zero_margin());
        let Scene::Container(root) = &scene else {
            panic!("build_fill returns a Container root")
        };
        assert_eq!(root.tag.as_deref(), Some("chart"), "root carries the tag");
        assert_eq!(
            root.layout.size.width,
            SizeValue::Percent(100),
            "root fills its slot width (not a fixed px / window rect)"
        );
        assert_eq!(root.layout.size.height, SizeValue::Percent(100));
        assert!(
            root.layout.absolute_position.is_none(),
            "the root is PLACED by layout, so it declares no absolute position \
             of its own — that is the whole point vs build()"
        );

        // Every series vertex is authored in the chart's own (0,0)..(300,200)
        // frame: rect-local commands, and the placing `absolute_position` is
        // parent-relative and inside the slot — never a window coordinate.
        for i in 0..2 {
            let Some(Scene::Path(p)) = find(&scene, &format!("chart.series.{i}")) else {
                panic!("series {i} path")
            };
            assert!(
                p.rect.x < 300 && p.rect.y < 200,
                "series {i} rect origin ({}, {}) is inside the slot, not a \
                 window coordinate",
                p.rect.x,
                p.rect.y
            );
            let (ax, ay) = p.layout.absolute_position.expect("series is placed");
            assert!(
                ax < 300 && ay < 200,
                "series {i} is pinned parent-relative within the slot, got ({ax}, {ay})"
            );
            for cmd in &p.commands {
                let (PathCommand::MoveTo(pt) | PathCommand::LineTo(pt)) = *cmd else {
                    continue;
                };
                assert!(
                    pt.x >= -1.0 && pt.x <= to_f32(p.rect.w) + 1.0,
                    "series {i} command x={} is local to its own rect (w={})",
                    pt.x,
                    p.rect.w
                );
            }
        }
    }

    /// R1360 — the linchpin, under a REAL layout pass. A `build_fill` chart is
    /// dropped into a parent slot placed away from the window origin; after
    /// `compute_layout` the chart root must be MEASURED to the slot (proving it
    /// is placed by layout, not a fixed rect), and a series vertex must resolve
    /// to `slot_origin + local` in window px (proving the parent-relative
    /// children + R1358 rect-local commands compose correctly). This is what
    /// `build()` structurally cannot do — its children carry window-absolute
    /// `absolute_position`s.
    #[test]
    fn build_fill_lays_out_relative_to_its_placed_slot() {
        use pinion_core::scene::ContainerNode;
        use pinion_core::style::LayoutStyle;

        // Slot: 200x100, placed at (120, 60) inside a 400x300 window via
        // absolute_position (the "dock panel at an offset" case).
        const SLOT_W: u32 = 200;
        const SLOT_H: u32 = 100;
        const SLOT_X: u32 = 120;
        const SLOT_Y: u32 = 60;

        // A flat y=5 series over domain 0..10: with zero margins it maps to the
        // plot's vertical middle, an easy oracle after placement.
        let series = vec![Series::new(
            "s",
            (0..=4).map(|i| DataPoint::new(f64::from(i), 5.0)).collect(),
        )];
        let chart = LineChart::new(series)
            .with_x_domain(0.0, 4.0)
            .with_y_domain(0.0, 10.0);
        let chart_scene = chart.build_fill((SLOT_W, SLOT_H), &no_legend_zero_margin());
        let slot = Scene::Container(
            ContainerNode::new(vec![chart_scene]).with_layout(
                LayoutStyle::new()
                    .with_absolute_position(SLOT_X, SLOT_Y)
                    .with_size(Size::px(SLOT_W, SLOT_H)),
            ),
        );
        let mut scene = Scene::Container(ContainerNode::new(vec![slot]));

        let mut text_cache = pinion_text::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut text_cache, 400, 300);

        // The chart root was placed + sized by layout to fill its slot.
        let Some(Scene::Container(root)) = find(&scene, "chart") else {
            panic!("chart root present after layout")
        };
        assert_eq!(
            (root.rect.x, root.rect.y),
            (SLOT_X, SLOT_Y),
            "the fill-parent chart root is PLACED by layout at the slot origin"
        );
        assert_eq!(
            (root.rect.w, root.rect.h),
            (SLOT_W, SLOT_H),
            "the fill-parent chart root is SIZED by layout to the slot"
        );

        // A series vertex now sits at slot_origin + local. Flat y=5 of 0..10
        // over a 100px slot placed at y=60 lands at the vertical middle: 110.
        let Some(Scene::Path(p)) = find(&scene, "chart.series.0") else {
            panic!("series after layout")
        };
        let PathCommand::MoveTo(first) = p.commands[0] else {
            panic!("series starts with MoveTo")
        };
        let win_x = to_f32(p.rect.x) + first.x;
        let win_y = to_f32(p.rect.y) + first.y;
        assert!(
            (win_x - to_f32(SLOT_X)).abs() < 1.5,
            "x=0 lands at the slot's left edge (window {SLOT_X}), got {win_x}"
        );
        assert!(
            (win_y - 110.0).abs() < 1.5,
            "y=5 of 0..10 lands at the slot's vertical middle (window 110), got {win_y}"
        );
    }

    #[test]
    fn build_fill_unmeasured_is_an_empty_tagged_fill_root() {
        // The (0,0) sentinel: still measurable (fill-parent), paints nothing,
        // so the shell's post-layout publish feeds the real size back on the
        // next paint. A degenerate build must not panic or emit series.
        let chart = LineChart::new(two_series());
        let scene = chart.build_fill((0, 0), &ChartStyle::default());
        let Scene::Container(root) = &scene else {
            panic!("root Container")
        };
        assert_eq!(root.tag.as_deref(), Some("chart"));
        assert_eq!(root.layout.size.width, SizeValue::Percent(100));
        assert!(root.children.is_empty(), "unmeasured chart paints nothing");
        assert!(
            find(&scene, "chart.series.0").is_none(),
            "no series until the slot is measured"
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

    // ── R1380: interactive legend (click-to-toggle hit geometry) ──────────

    fn coloured_series() -> Vec<Series> {
        vec![
            Series::new(
                "a",
                vec![DataPoint::new(0.0, 0.0), DataPoint::new(10.0, 10.0)],
            )
            .with_color(Color::rgb(0x11, 0x22, 0x33)),
            Series::new(
                "b",
                vec![DataPoint::new(0.0, 5.0), DataPoint::new(10.0, 5.0)],
            )
            .with_color(Color::rgb(0x44, 0x55, 0x66)),
        ]
    }

    fn legend_tags() -> Vec<String> {
        vec!["leg.0".to_string(), "leg.1".to_string()]
    }

    /// The swatch fill of interactive legend entry `tag` (its first child Box).
    fn entry_swatch_fill(scene: &Scene, tag: &str) -> Color {
        let Some(Scene::Container(c)) = find(scene, tag) else {
            panic!("entry {tag} is a container")
        };
        let Some(Scene::Box(swatch)) = c.children.first() else {
            panic!("entry {tag}'s first child is the swatch box")
        };
        swatch.style.fill
    }

    #[test]
    fn interactive_legend_entries_are_focusable_and_tagged() {
        // Each entry carries the CALLER's tag (not the chart prefix), is a Tab
        // stop, and wraps exactly a swatch + a label — the chip structure, so a
        // click anywhere on it hit-tests to the entry tag.
        let scene = LineChart::new(coloured_series())
            .interactive_legend(legend_tags())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        for tag in ["leg.0", "leg.1"] {
            let Some(Scene::Container(c)) = find(&scene, tag) else {
                panic!("interactive entry {tag} present as a container")
            };
            assert!(c.layout.focusable, "entry {tag} is a Tab stop");
            assert_eq!(c.children.len(), 2, "entry {tag} = swatch + label only");
            assert!(
                matches!(c.children[0], Scene::Box(_)),
                "child 0 is the swatch"
            );
            assert!(
                matches!(c.children[1], Scene::Text(_)),
                "child 1 is the label"
            );
        }
        // The interactive path replaces the static per-part legend tags.
        assert_eq!(
            count_prefix(&scene, "chart.legend."),
            0,
            "no static .legend.{{i}}.swatch/.label nodes in interactive mode"
        );
    }

    #[test]
    fn interactive_legend_greys_a_hidden_series_swatch() {
        // Visible entry keeps its series colour; hidden entry greys to the
        // muted label colour (the "line is off" affordance) — the palette index
        // and slot are untouched, so this never re-colours the sibling.
        let mut series = coloured_series();
        series[1] = series[1].clone().with_visible(false);
        let style = ChartStyle::default();
        let scene = LineChart::new(series)
            .interactive_legend(legend_tags())
            .build(Rect::new(0, 0, 400, 300), &style);
        assert_eq!(
            entry_swatch_fill(&scene, "leg.0"),
            Color::rgb(0x11, 0x22, 0x33),
            "visible entry keeps its series colour"
        );
        assert_eq!(
            entry_swatch_fill(&scene, "leg.1"),
            style.label,
            "hidden entry greys to the muted label colour"
        );
    }

    #[test]
    fn rescale_to_visible_grows_a_small_series_when_the_big_one_is_hidden() {
        // series 0 dominates (y to ~4000), series 1 is small (y to 8). With the
        // big one hidden, rescale_to_visible snaps the y-domain to the small
        // series, so its polyline spans most of the plot instead of a sliver.
        let build = |rescale: bool| {
            let series = vec![
                Series::new(
                    "big",
                    (0..=3)
                        .map(|i| DataPoint::new(f64::from(i), f64::from(i) * 1333.0))
                        .collect(),
                )
                .with_visible(false),
                Series::new(
                    "small",
                    (0..=3)
                        .map(|i| DataPoint::new(f64::from(i), 2.0 + f64::from(i) * 2.0))
                        .collect(),
                ),
            ];
            let scene = LineChart::new(series)
                .rescale_to_visible(rescale)
                .build(Rect::new(0, 0, 400, 300), &no_legend_zero_margin());
            let Scene::Path(p) = find(&scene, "chart.series.1").expect("small series") else {
                panic!("series is a path")
            };
            p.rect.h
        };
        let h_off = build(false);
        let h_on = build(true);
        assert!(
            h_on > h_off * 3,
            "rescale grows the small series' vertical extent (off={h_off}, on={h_on})"
        );
    }

    #[test]
    fn legend_stays_static_and_unfocusable_without_interactive() {
        // The default legend is byte-identical to the pre-R1380 static row:
        // per-part tags, and no focusable legend container.
        let scene = LineChart::new(coloured_series())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(count_prefix(&scene, "chart.legend.0"), 2, "swatch + label");
        assert!(
            find(&scene, "leg.0").is_none(),
            "no caller-tagged entries without interactive_legend"
        );
        // The static swatch/label are leaves, never a focusable Tab stop.
        let Some(Scene::Box(_)) = find(&scene, "chart.legend.0.swatch") else {
            panic!("static swatch is a plain box")
        };
    }
}
