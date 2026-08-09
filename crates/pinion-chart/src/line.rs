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
//! # Colouring the trace by a measure (R1440)
//!
//! [`LineChart::color_by`] encodes each sample's [`DataPoint::value`] third
//! channel as colour, the encoding the scatter (R1438) and treemap (R1439)
//! already carry. This chart is where the GEOMETRY stops fitting: a point is one
//! mark and a tile is one box, but a trace is a polyline whose colour has to
//! change *along* it. The two marks therefore take different mechanisms, because
//! the scene primitives do:
//!
//! * the LINE becomes one stroked path per segment (`chart.series.{i}.seg.{k}`),
//!   each at the mean of its endpoints' measures — a stroke takes a flat colour,
//!   since `PathStyle`'s gradient replaces the FILL.
//! * the AREA keeps one path and takes a horizontal gradient whose stops sit at
//!   the samples' own x positions — genuinely continuous, and exact rather than
//!   approximate.
//!
//! Both ride in the scene as data, so §2 #7 introspection reads the encoding
//! without a pixel. The legend becomes a colour bar, as on the other two.
//!
//! # Introspection
//!
//! Every emitted node carries a tag under the chart's `tag_prefix`
//! (default `"chart"`): `chart.bg`, `chart.series.{i}`, `chart.area.{i}`,
//! and — when [`select_x_range`](LineChart::select_x_range) is set — the
//! full-colour in-window overdraw `chart.focus.series.{i}` /
//! `chart.focus.area.{i}` (the muted `chart.series.{i}` staying as context),
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
use pinion_core::style::{Color, Stroke, StrokeCap};

use crate::color_scale::{ColorScale, ValueEncoding};
use crate::draw::{
    CalloutRow, MUTED_ALPHA, absolute, area_path, area_path_along_x, box_node, callout,
    fill_parent, legend_band_color_bar, marker_node, stroke_path,
};
use crate::palette::CategoricalPalette;
use crate::plot::{
    AxisKinds, CartesianPlot, OffScale, Rescale, axis_format, axis_minor_ticks, axis_ticks,
    off_scale_points, tick_pixels,
};
use crate::scale::{AxisKind, Categories};
use crate::series::{DataPoint, Series, StackedBand, stack, stacked_value_bounds, value_bounds};
use crate::style::ChartStyle;
use crate::ticks::TickFormat;

/// A line chart: one or more [`Series`] drawn as polylines with nice
/// axes, gridlines, labels, and a legend. Set [`filled`](Self::filled) to
/// also paint a translucent area under each series (an area chart).
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "R1622 — four INDEPENDENT display modes (area fill, stacking, and               the two rescale axes), each set by its own builder call and each               meaningful alone. Grouping them behind a flags struct would               change no call site and hide which are related; the lint's real               target is a struct whose bools encode one state machine, and               these do not"
)]
pub struct LineChart {
    series: Vec<Series>,
    palette: CategoricalPalette,
    x_domain: Option<(f64, f64)>,
    y_domain: Option<(f64, f64)>,
    fill_area: bool,
    /// R1622 §5.28 — draw the series **stacked**: each band sits on the
    /// cumulative total of the visible ones before it.
    stacked: bool,
    inspect: Option<f32>,
    legend_tags: Option<Vec<String>>,
    rescale_to_visible: bool,
    rescale_y_to_x_window: bool,
    select_x_range: Option<(f64, f64)>,
    color: ValueEncoding,
    kinds: AxisKinds,
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
            stacked: false,
            inspect: None,
            legend_tags: None,
            rescale_to_visible: false,
            rescale_y_to_x_window: false,
            select_x_range: None,
            color: ValueEncoding::default(),
            kinds: AxisKinds::default(),
            tag_prefix: "chart".to_string(),
        }
    }

    /// R1440 — colour the trace by each sample's [`DataPoint::value`] third
    /// channel on a SEQUENTIAL ramp, instead of by which series it belongs to.
    ///
    /// The line chart's version of the encoding the scatter (R1438) and the
    /// treemap (R1439) already carry, and the one whose GEOMETRY does not fit.
    /// A point is one mark with one colour and a tile is one box; a trace is a
    /// polyline whose colour would have to change *along* it. Two shapes follow,
    /// and they are different because the scene primitives are:
    ///
    /// * the **line** becomes one stroked path PER SEGMENT
    ///   (`{prefix}.series.{i}.seg.{k}`), each coloured by the mean of its two
    ///   endpoints' values — a stroke takes a flat colour
    ///   ([`PathStyle`](pinion_core::style::PathStyle)'s gradient replaces the
    ///   FILL), so segment-wise is as continuous as a stroke can be. The mean,
    ///   not the start value, because a segment spans two samples and one colour
    ///   is necessarily a compromise; taking the start would bias every segment
    ///   toward the left and make the trace read as a step function.
    /// * the **area** (with [`filled`](Self::filled)) keeps its single path and
    ///   takes a horizontal GRADIENT whose stops sit at the samples' own x
    ///   positions — genuinely continuous, and exact rather than approximate,
    ///   since colour is piecewise-linear between stops just as the encoding is
    ///   between samples.
    ///
    /// Turning it on swaps the legend for a colour bar, as on the other two: a
    /// swatch row would claim the colours name the series, which stops being true
    /// the moment they mean magnitude.
    ///
    /// A sample carrying no value keeps the series colour, and a series with no
    /// values at all is drawn exactly as before — one path, one colour.
    #[must_use]
    pub fn color_by(mut self, scale: ColorScale) -> Self {
        self.color.sequential(scale);
        self
    }

    /// R1440 — colour the trace by its [`DataPoint::value`] on a DIVERGING ramp
    /// anchored at `neutral`.
    ///
    /// The sibling of [`color_by`](Self::color_by) for a signed measure, which is
    /// the common case for a trace: a slope, a drift from nominal, a delta
    /// against target. Each wing normalises on its own width (R1436), so the
    /// neutral lands on the ramp's centre colour even on the asymmetric domain
    /// real data almost always has.
    #[must_use]
    pub fn color_by_diverging(mut self, scale: ColorScale, neutral: f64) -> Self {
        self.color.diverging(scale, neutral);
        self
    }

    /// R1440 — pin the colour domain instead of deriving it from the data.
    ///
    /// Worth pinning whenever two charts must be comparable (the same colour has
    /// to mean the same measure in both) or when the ramp should span a known
    /// operating range rather than this sample's own extremes.
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

    /// Fill a translucent area under each series (turns the line chart
    /// into an area chart).
    #[must_use]
    pub fn filled(mut self, fill: bool) -> Self {
        self.fill_area = fill;
        self
    }

    /// R1622 §5.28 — draw the series **stacked**: each band sits on the
    /// cumulative total of the visible ones before it, and the value axis is
    /// scaled to the total rather than to any one series.
    ///
    /// This is the composition an application previously had to perform for
    /// itself, by handing in a series whose `y` was already a running sum —
    /// which threw the original measurement away before anything could ask for
    /// it. Here [`Series::points`] keeps what was measured and the stacking is
    /// the chart's ([`crate::stack`], usable on its own).
    ///
    /// Implies [`filled`](Self::filled): a stack of bare polylines shows the
    /// cumulative totals and hides the bands, which is not what stacking is
    /// for. The lines are still drawn, along each band's top edge.
    #[must_use]
    pub fn stacked(mut self, stacked: bool) -> Self {
        self.stacked = stacked;
        if stacked {
            self.fill_area = true;
        }
        self
    }

    /// R1622 §5.28 — what the geometry loop iterates: `(palette index, the
    /// series whose curve is drawn, the band's lower edge)`.
    ///
    /// One source for both modes. Stacking changes WHERE a series is drawn,
    /// not what is done with it, so the polyline, the colour encoding, the
    /// markers and the inspect overlay stay one code path instead of forking
    /// into a stacked copy that could drift from the flat one.
    fn drawn_series(&self) -> Vec<(usize, Series, Option<Vec<DataPoint>>)> {
        // `bands()` is the one derivation this and the y-domain both read;
        // recomputing it beats threading it through a signature several
        // callers share.
        let bands = self.bands();
        if bands.is_empty() {
            return self
                .series
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, s)| (i, s, None))
                .collect();
        }
        bands
            .iter()
            .map(|band| {
                let mut top = band.series.clone();
                top.points.clone_from(&band.upper);
                (band.index, top, Some(band.lower.clone()))
            })
            .collect()
    }

    /// R1622 §5.28 — the value domain this chart will actually resolve with:
    /// an explicit [`with_y_domain`](Self::with_y_domain) if one was set, else
    /// the STACK's cumulative extent when stacked, else `None` (auto-fit per
    /// series).
    ///
    /// Its own method so the choice is assertable. A counterfactual pinning
    /// the domain to something other than the stack's extent — the defect that
    /// runs the top band off the plot — was caught by nothing while this lived
    /// inline in `build_body`: a test can compare two pictures, but it cannot
    /// say which domain produced either.
    #[must_use]
    pub fn resolved_y_domain(&self, bands: &[StackedBand]) -> Option<(f64, f64)> {
        self.y_domain.or_else(|| {
            (!bands.is_empty())
                .then(|| stacked_value_bounds(bands))
                .flatten()
        })
    }

    /// R1622 — the bands this chart would draw, or empty when it is not
    /// stacked. Public so a binding can label, hit-test or tooltip a band
    /// against the same composition the picture used, rather than repeating it.
    #[must_use]
    pub fn bands(&self) -> Vec<StackedBand> {
        if self.stacked {
            stack(&self.series)
        } else {
            Vec::new()
        }
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

    /// Fit the auto y-axis to the **brushed x-window** (R1397): when `true` and
    /// the y-domain is auto, the y-axis measures only the points whose x falls
    /// inside the resolved x-domain — so pairing it with a brush that
    /// [`with_x_domain`](Self::with_x_domain)-zooms the chart lets the y-axis
    /// zoom in on just that window's values. A large transient outside the
    /// window then stops flattening the detail inside it (the canonical
    /// "auto-scale Y to the visible X range" of a monitoring chart).
    ///
    /// Distinct from [`rescale_to_visible`](Self::rescale_to_visible): that one
    /// picks which *series* the axes measure (the visible-toggle axis); this one
    /// windows the *y-fit* to the visible *x-range* (the brush-zoom axis). They
    /// are orthogonal opt-ins. Default `false`. Only bites when the x-domain is
    /// narrower than the data — a full-width x-domain includes every point, so
    /// the fit is a no-op — and a pinned [`with_y_domain`](Self::with_y_domain)
    /// still wins.
    #[must_use]
    pub fn rescale_y_to_x_window(mut self, rescale: bool) -> Self {
        self.rescale_y_to_x_window = rescale;
        self
    }

    /// Numeric brush-range **cross-filter** (R1394): the polyline portion whose
    /// x falls outside `[lo, hi]` mutes (dimmed to a context ghost, still drawn)
    /// while the in-window portion keeps full colour — the "focus + context"
    /// twin of
    /// [`ScatterChart::select_x_range`](crate::ScatterChart::select_x_range) for
    /// a continuous line, so a brush over one chart can emphasise the matching
    /// window of another. `None` (the default) — and any window that already
    /// covers the whole visible domain (a full brush) — draws every series full.
    ///
    /// Boundaries are inclusive: the focus segment is cut at exactly `lo` / `hi`
    /// with y interpolated at each crossing (the same domain clip R1356 uses),
    /// so it meets the window edge precisely. Independent of
    /// [`with_x_domain`](Self::with_x_domain) — the domain sets what is
    /// *visible*, this sets what is *emphasised* within it.
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

    /// Bundle this chart's two rescale opt-ins into the [`Rescale`] the plot
    /// resolver takes — one place, so `build_body` and `inspect_readout` resolve
    /// the identical domains (the tooltip a sighted user sees and the a11y
    /// readout can never disagree).
    fn rescale(&self) -> Rescale {
        Rescale {
            to_visible: self.rescale_to_visible,
            y_to_x_window: self.rescale_y_to_x_window,
        }
    }

    /// Plot the **y**-axis logarithmically in base 10 (R1528) — the toolkit's
    /// log value axis on the value axis.
    ///
    /// Use it when the interesting structure spans orders of magnitude: a
    /// latency series over `0.1 ms .. 1000 ms` puts every sub-millisecond
    /// sample on the baseline of a linear axis, and equal ratios on equal
    /// pixel spans here. The auto-domain then measures only the strictly
    /// positive samples and snaps to whole decades.
    ///
    /// Samples at or below zero have no pixel on a log axis and so draw no
    /// mark; [`off_scale`](Self::off_scale) reports them, because silently
    /// placing them on the baseline would be indistinguishable from real
    /// samples at the domain floor.
    #[must_use]
    pub fn y_log(self) -> Self {
        self.y_log_base(crate::scale::DEFAULT_LOG_BASE)
    }

    /// [`y_log`](Self::y_log) in an explicit `base` (the toolkit's
    /// `base`) — base 2 for a size or capacity axis, where a
    /// tick per doubling is what a reader counts in.
    #[must_use]
    pub fn y_log_base(mut self, base: f64) -> Self {
        self.kinds.y = AxisKind::Log(base);
        self
    }

    /// Plot the **x**-axis logarithmically in base 10 (R1528). The x-axis
    /// twin of [`y_log`](Self::y_log); see it for the whole contract.
    #[must_use]
    pub fn x_log(self) -> Self {
        self.x_log_base(crate::scale::DEFAULT_LOG_BASE)
    }

    /// [`x_log`](Self::x_log) in an explicit `base`.
    #[must_use]
    pub fn x_log_base(mut self, base: f64) -> Self {
        self.kinds.x = AxisKind::Log(base);
        self
    }

    /// Plot the **x**-axis as UTC time (R1529) — the toolkit's date time axis,
    /// d3's `scaleUtc`. Sample `x` values are read as epoch **milliseconds**.
    ///
    /// This is the axis a monitoring chart has. Without it a timestamp is a
    /// plain number, so the gridlines land on multiples of a decimal step
    /// (`00:06:40`, `00:23:20`) and the labels compact by magnitude — which
    /// on a sub-day domain renders every one of them as the same string
    /// (`1772.4G`), because a one-decimal SI suffix at that scale has
    /// 27-hour resolution.
    ///
    /// Ticks instead land on clock and calendar boundaries, and each label
    /// carries the finest field that distinguishes it
    /// ([`format_time_tick`](crate::format_time_tick)), so the date appears
    /// exactly where the axis crosses into a new day rather than on every
    /// label. The scrub readout takes the unambiguous full stamp
    /// ([`format_time_stamp`](crate::format_time_stamp)), which an axis
    /// label cannot be.
    #[must_use]
    pub fn x_time(mut self) -> Self {
        self.kinds.x = AxisKind::Time;
        self
    }

    /// Plot the **y**-axis as UTC time (R1529) — the y-axis twin of
    /// [`x_time`](Self::x_time); see it for the whole contract. Uncommon but legal,
    /// exactly as the toolkit allows a date time axis on either axis: a chart
    /// of "when did this run finish" against a run index has time on y.
    #[must_use]
    pub fn y_time(mut self) -> Self {
        self.kinds.y = AxisKind::Time;
        self
    }

    /// Plot the **x**-axis over named categories (R1545) — the toolkit's
    /// bar category axis attached to a line series, d3's `scalePoint`.
    /// Sample `x` values are read as category **indices**: category `i` sits
    /// at `x = i`.
    ///
    /// This is the trend-over-categories chart — a line across the same
    /// buckets a [`BarChart`](crate::BarChart) would draw as bars, so the two
    /// can share an axis and be read against each other. Until R1545 the
    /// crate's categorical layout lived only in the bar builder's private
    /// slot metric, so a line chart could name its buckets only by pinning a
    /// numeric domain and letting the axis label them `0, 1, 2`.
    ///
    /// A sample whose x names no slot — index `9` of six categories, or a
    /// negative one — has **no pixel** and is reported by
    /// [`off_scale`](Self::off_scale), the R1528 stance for a log axis's zero
    /// applied to the arm where "no such slot" is the failure. Fractional
    /// positions between slots do map: that is where a segment crossing from
    /// one category to the next lives.
    #[must_use]
    pub fn x_category<I, S>(mut self, categories: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.kinds.x = AxisKind::Category(Categories::new(categories));
        self
    }

    /// Plot the **y**-axis over named categories (R1545) — the y-axis twin of
    /// [`x_category`](Self::x_category); see it for the whole contract. This
    /// is the axis a horizontal category chart puts its buckets on, exactly
    /// as the toolkit attaches a bar category axis to the y of a
    /// horizontal bar series.
    #[must_use]
    pub fn y_category<I, S>(mut self, categories: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.kinds.y = AxisKind::Category(Categories::new(categories));
        self
    }

    /// Every point this chart's axes cannot place, in series then point
    /// order (R1528) — empty for an all-linear chart over finite data.
    ///
    /// The counterpart of [`Mapped::unreadable`](crate::Mapped) on the other
    /// input path: the chart stays a pure scene producer, and what it could
    /// not draw is reported as data (§2 #7) rather than faked or dropped in
    /// silence. `examples/hello-log-chart` renders it as a caption.
    #[must_use]
    pub fn off_scale(&self) -> Vec<OffScale> {
        off_scale_points(&self.series, &self.kinds)
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
        // R1622 — a stack reaches the SUM of its series, so the value axis is
        // scaled to the cumulative total. Scaling to the per-series maximum
        // (which is what `resolve` measures) would run three 40-peak bands off
        // the top of a plot that stops at 40. An explicit `with_y_domain`
        // still wins, as it does for every other rescale.
        let bands = self.bands();
        let y_domain = self.resolved_y_domain(&bands);
        let plot = CartesianPlot::resolve(
            rect,
            &self.series,
            self.x_domain,
            y_domain,
            style,
            self.rescale(),
            &self.kinds,
        );
        let (y_lo, y_hi) = plot.y.domain();
        // One tick-set resolver, shared with `inspect_readout` so the
        // painted axis and the AT readout format at the same precision.
        let x_ticks = axis_ticks(&plot.x, style.x_ticks);
        let y_ticks = axis_ticks(&plot.y, style.y_ticks);
        let baseline = clamp(0.0, y_lo, y_hi);
        // The label precision each axis formats at (R1359: `format_si` alone
        // collapsed every sub-0.1 step onto one rounded digit; R1528: a log
        // axis has no ONE step to derive that precision from).
        let steps = Steps {
            x: axis_format(&plot.x, &x_ticks),
            y: axis_format(&plot.y, &y_ticks),
        };

        // Inspect overlay, split so the crosshair paints behind the
        // series, the markers on top of the lines, and the tooltip above
        // everything.
        let (crosshair, markers, tooltip) = match self.resolve_inspect(&plot, rect, style, &steps) {
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
        children.extend(self.tick_labels(&plot, rect, &x_ticks, &y_ticks, style, &steps));
        if style.legend {
            // R1440 — one colour legend, never two: a value encoding replaces
            // the series swatch row with the colour bar, because the swatches
            // would name a series-to-colour mapping that no longer holds.
            if self.color.is_set() {
                children.extend(legend_band_color_bar(
                    &self.color,
                    self.resolved_color_domain(),
                    rect,
                    style,
                    &self.tag_prefix,
                ));
            } else {
                children.extend(self.legend(rect, style));
            }
        }
        children.extend(tooltip);

        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
    }

    /// Horizontal (per y-tick) and vertical (per x-tick) gridlines. Maps the
    /// ticks to pixels and hands them to the shared [`crate::draw::gridlines`]
    /// (R1377) — the pure "draw lines at these positions" primitive the bar and
    /// scatter charts also emit.
    ///
    /// A logarithmic axis contributes a second, fainter set below them
    /// ([`crate::draw::minor_gridlines`], R1528); a linear axis has no minor
    /// ticks, so for a linear chart this emits exactly what it always did.
    fn gridlines(
        &self,
        plot: &CartesianPlot,
        x_ticks: &[f64],
        y_ticks: &[f64],
        style: &ChartStyle,
    ) -> Vec<Scene> {
        let frame = (plot.left, plot.right, plot.top, plot.bottom);
        let mut out = crate::draw::minor_gridlines(
            frame,
            &tick_pixels(&plot.x, &axis_minor_ticks(&plot.x)),
            &tick_pixels(&plot.y, &axis_minor_ticks(&plot.y)),
            style,
            &self.tag_prefix,
        );
        out.extend(crate::draw::gridlines(
            frame,
            &tick_pixels(&plot.x, x_ticks),
            &tick_pixels(&plot.y, y_ticks),
            style,
            &self.tag_prefix,
        ));
        out
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
        // A log y-axis has no zero, so `baseline` was already clamped into
        // the (positive) domain and maps; `plot.bottom` is the fallback an
        // area drops to if an axis ever cannot place it.
        let baseline_y = plot.y.map(baseline).unwrap_or(plot.bottom);
        let (x_lo, x_hi) = plot.x.domain();
        // R1394 — a window that already covers the whole visible domain clips
        // nothing, so it filters nothing (a full brush = no cross-filter): drop
        // it to `None` here so the line stays full, matching the scatter's
        // per-point `is_muted` (which never mutes an in-domain point at full span).
        let window = self
            .select_x_range
            .filter(|&(lo, hi)| lo > x_lo || hi < x_hi);
        let mut out = Vec::new();
        // R1622 — one iteration source for both modes: `(palette index, the
        // series whose curve is drawn, the band's lower edge)`. Stacking
        // changes WHERE a series is drawn, not what the loop below does, so
        // the geometry, the colour encoding and the inspect overlay stay one
        // code path rather than forking into a stacked copy that could drift.
        let drawn = self.drawn_series();
        for (i, s, lower) in &drawn {
            let (i, s) = (*i, s);
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
            // Kept as `DataPoint`s, not just pixels: R1440's colour encoding
            // reads each clipped sample's third channel, and a boundary crossing
            // carries an interpolated one (see `clip_to_x_domain`).
            // R1528 — the clip, the off-scale drop, and the mapping are ONE
            // step, so a sample and its encoded colour cannot desynchronise:
            // two passes over the same points through two predicates is the
            // shape that silently shifts a ramp by one sample.
            let placed: Vec<(DataPoint, (f32, f32))> = clip_to_x_domain(&finite, x_lo, x_hi)
                .into_iter()
                .filter_map(|p| plot.map_point(&p).map(|px| (p, px)))
                .collect();
            let clipped: Vec<DataPoint> = placed.iter().map(|(p, _)| *p).collect();
            let pts: Vec<(f32, f32)> = placed.iter().map(|(_, px)| *px).collect();
            if pts.is_empty() {
                continue;
            }
            // R1394 cross-filter: when a brush window is set, the whole line
            // draws muted (a context ghost) and the in-window portion is
            // overdrawn at full colour below, so the emphasis reads as focus.
            let muted = window.is_some();
            let width = style.series_width.max(1);
            // R1440 — the encoded colour per clipped sample, `None` where this
            // chart is not encoding (or the sample carries no measure), in which
            // case both geometries fall back to the flat series colour.
            let encoded: Vec<Option<Color>> =
                clipped.iter().map(|p| self.value_color(*p)).collect();
            let any_encoded = encoded.iter().any(Option::is_some);
            if self.fill_area {
                let alpha = if muted {
                    crate::draw::mul_alpha(style.area_alpha, MUTED_ALPHA)
                } else {
                    style.area_alpha
                };
                let tag = format!("{}.area.{i}", self.tag_prefix);
                if any_encoded {
                    // The area spans a range of x, so its measure is encoded
                    // CONTINUOUSLY: a gradient whose stops sit at the samples'
                    // own x positions rather than one colour standing in for
                    // every value under the curve.
                    let ramp: Vec<(f32, Color)> = pts
                        .iter()
                        .zip(&encoded)
                        .map(|(&(px, _), enc)| (px, enc.unwrap_or(color).with_alpha(alpha)))
                        .collect();
                    out.push(area_path_along_x(&pts, baseline_y, &ramp, tag));
                } else if let Some(lower) = lower {
                    // R1622 — a band is filled between its own curve and the
                    // cumulative total below it, which a scalar baseline
                    // cannot express.
                    let base: Vec<(f32, f32)> =
                        lower.iter().filter_map(|p| plot.map_point(p)).collect();
                    out.extend(crate::draw::area_between(
                        &pts,
                        &base,
                        color.with_alpha(alpha),
                        tag,
                    ));
                } else {
                    out.push(area_path(&pts, baseline_y, color.with_alpha(alpha), tag));
                }
            }
            let line_color = if muted {
                color.with_alpha(MUTED_ALPHA)
            } else {
                color
            };
            if any_encoded {
                out.extend(self.encoded_segments(&pts, &encoded, line_color, width, i));
            } else {
                out.push(stroke_path(
                    &pts,
                    Stroke::new(line_color, width).with_cap(StrokeCap::Round),
                    format!("{}.series.{i}", self.tag_prefix),
                ));
            }
            // The focus overdraw: the in-window sub-polyline at full colour.
            // `clip_to_x_domain` interpolates y at the `[lo, hi]` crossings, so
            // the segment meets the brush edge exactly (R1356's clip reused).
            if let Some((lo, hi)) = window {
                let focus: Vec<(f32, f32)> = clip_to_x_domain(&finite, lo.max(x_lo), hi.min(x_hi))
                    .iter()
                    .filter_map(|p| plot.map_point(p))
                    .collect();
                if focus.len() >= 2 {
                    if self.fill_area {
                        out.push(area_path(
                            &focus,
                            baseline_y,
                            color.with_alpha(style.area_alpha),
                            format!("{}.focus.area.{i}", self.tag_prefix),
                        ));
                    }
                    out.push(stroke_path(
                        &focus,
                        Stroke::new(color, width).with_cap(StrokeCap::Round),
                        format!("{}.focus.series.{i}", self.tag_prefix),
                    ));
                }
            }
        }
        out
    }

    /// R1440 — the value-encoded trace as one stroked path PER SEGMENT, tagged
    /// `{prefix}.series.{i}.seg.{k}`.
    ///
    /// A stroke takes a flat colour — [`PathStyle`](pinion_core::style::PathStyle)'s
    /// gradient replaces the FILL, not the stroke — so this is as continuous as a
    /// polyline can be made without inventing a primitive. Each segment takes the
    /// encoding at the MEAN of its two endpoints' measures: a segment spans two
    /// samples, so one colour is a compromise either way, and the mean is the
    /// symmetric choice (taking the start value would bias every segment leftward
    /// and make a smooth trace read as a step function).
    ///
    /// A segment whose endpoints carry no measure falls back to `flat`, so a
    /// partially-measured series degrades per segment rather than all-or-nothing.
    fn encoded_segments(
        &self,
        pts: &[(f32, f32)],
        encoded: &[Option<Color>],
        flat: Color,
        width: u32,
        i: usize,
    ) -> Vec<Scene> {
        let mut out = Vec::with_capacity(pts.len().saturating_sub(1));
        for (k, pair) in pts.windows(2).enumerate() {
            let color = segment_color(
                encoded.get(k).copied().flatten(),
                encoded.get(k + 1).copied().flatten(),
            )
            .unwrap_or(flat);
            out.push(stroke_path(
                pair,
                Stroke::new(color, width).with_cap(StrokeCap::Round),
                format!("{}.series.{i}.seg.{k}", self.tag_prefix),
            ));
        }
        out
    }

    /// R1440 — the active colour domain: pinned, else measured off the third
    /// channel of every series, else `None` (the chart stays categorical).
    fn resolved_color_domain(&self) -> Option<(f64, f64)> {
        self.color.domain(|| value_bounds(&self.series))
    }

    /// R1440 — the encoded colour for one sample, or `None` when this chart is
    /// not encoding / the sample carries no finite measure.
    fn value_color(&self, point: DataPoint) -> Option<Color> {
        self.color
            .color_for(point.value, || value_bounds(&self.series))
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
        steps: &Steps,
    ) -> Vec<Scene> {
        let y_pos = tick_pixels(&plot.y, y_ticks);
        let mut out =
            crate::draw::y_tick_labels(rect.x, y_ticks, &y_pos, &steps.y, style, &self.tag_prefix);
        // R1567 lifted the numeric x-label loop into `crate::draw` at its third
        // consumer, which is the threshold the comment that used to sit here
        // named in advance (line + scatter were two; the candlestick chart's
        // elapsed reading is the third).
        out.extend(crate::draw::x_tick_labels(
            &plot.x,
            x_ticks,
            plot.bottom,
            rect,
            &steps.x,
            style,
            &self.tag_prefix,
        ));
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
        // (R1396) The row's width from `start_x` to the chart's right edge; the
        // legend shrinks / drops entries to fit it rather than running past.
        let avail = rect.w.saturating_sub(style.margin.left);
        if let Some(tags) = &self.legend_tags {
            return self.interactive_legend_entries(tags, start_x, row_y, avail, style);
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
        avail: u32,
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
        crate::draw::interactive_legend_row(
            &entries,
            tags,
            start_x,
            row_y,
            avail,
            style,
            &self.tag_prefix,
        )
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
            self.rescale(),
            &self.kinds,
        );
        let x_ticks: Vec<f64> = axis_ticks(&plot.x, style.x_ticks);
        let y_ticks: Vec<f64> = axis_ticks(&plot.y, style.y_ticks);
        let steps = Steps {
            x: axis_format(&plot.x, &x_ticks),
            y: axis_format(&plot.y, &y_ticks),
        };
        let (focus_x, hits) = self.resolve_focus(&plot, rect)?;
        let mut out = format!("x = {}", steps.x.readout(focus_x));
        for (i, p) in &hits {
            // `write!` to a String is infallible; the Result is discarded
            // exactly as `push_str` would have.
            let _ = write!(out, ", {} {}", self.series[*i].name, steps.y.label(p.y));
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
        steps: &Steps,
    ) -> Option<Inspect> {
        let (focus_x, hits) = self.resolve_focus(plot, rect)?;
        let focus_pixel = plot.x.map(focus_x)?;

        let crosshair = stroke_path(
            &[(focus_pixel, plot.top), (focus_pixel, plot.bottom)],
            Stroke::new(style.crosshair, 1),
            format!("{}.inspect.crosshair", self.tag_prefix),
        );

        let radius = style.marker_radius.max(1);
        let markers: Vec<Scene> = hits
            .iter()
            .filter_map(|(i, p)| {
                let color = self.series[*i]
                    .color
                    .unwrap_or_else(|| self.palette.color(*i));
                let (px, py) = plot.map_point(p)?;
                Some(marker_node(
                    px,
                    py,
                    radius,
                    color,
                    format!("{}.inspect.marker.{i}", self.tag_prefix),
                ))
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
        steps: &Steps,
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

/// Per-axis label format — the precision axis and tooltip labels format at.
#[derive(Debug, Clone)]
struct Steps {
    x: TickFormat,
    y: TickFormat,
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
/// The colour of a segment whose endpoints resolved to `a` / `b`.
///
/// Both present: the midpoint in LINEAR light, which is what
/// [`Color::lerp`](pinion_core::style::Color::lerp) does and therefore agrees
/// with how the ramp itself interpolates — averaging the two sRGB triples
/// instead would darken every segment of a bright ramp. One present: that one,
/// so a segment adjoining an unmeasured sample still carries the measure it has.
/// Neither: `None`, and the caller falls back to the series colour.
fn segment_color(a: Option<Color>, b: Option<Color>) -> Option<Color> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.lerp(b, 0.5)),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

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
                // Every channel that varies along the segment interpolates on
                // the same `t`. Dropping `value` here (as this did before R1440)
                // left a value-encoded trace with an unmeasured sample at each
                // boundary, so a pinned or brushed domain showed a colour
                // discontinuity at exactly the edge the clip created.
                let mut crossing = DataPoint::new(bound, a.y + t * (b.y - a.y));
                if let (Some(av), Some(bv)) = (a.value, b.value) {
                    crossing = crossing.with_value(av + t * (bv - av));
                }
                crossings.push(crossing);
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
    use crate::scene_probe::find;
    use crate::series::DataPoint;
    use crate::style::Margin;
    use pinion_core::scene::PathCommand;
    use pinion_core::style::{Size, SizeValue};

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

    /// R1622 §5.28 — the PICTURE stacks: the bands are drawn between curves,
    /// the value axis is scaled to the total, and none of it happens unless
    /// the caller asked.
    #[test]
    fn r1622_a_stacked_chart_draws_bands_and_scales_to_the_total() {
        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        let flat = LineChart::new(two_series()).filled(true);
        let stacked = LineChart::new(two_series()).stacked(true);

        // Both draw one area per series, so the count alone proves nothing —
        // which is why the geometry is compared below.
        let a_flat = count_prefix(&flat.build(rect, &style), "chart.area.");
        let a_stack = count_prefix(&stacked.build(rect, &style), "chart.area.");
        assert_eq!(a_flat, a_stack, "one area per series either way");

        // The top band's fill must differ from the unstacked one: stacked, it
        // is lifted onto the series below. Identical paths would mean the
        // switch did nothing.
        let path_of = |scene: &Scene, tag: &str| match find(scene, tag) {
            Some(Scene::Path(p)) => p.commands.clone(),
            other => panic!("{tag} is not a path: {other:?}"),
        };
        let flat_top = path_of(&flat.build(rect, &style), "chart.area.1");
        let stacked_top = path_of(&stacked.build(rect, &style), "chart.area.1");
        assert_ne!(
            flat_top, stacked_top,
            "the second band is lifted onto the first — a stack that drew the \
             same path as an overlap would be a stack in name only",
        );
        // The discriminating shape: an area closed onto a scalar baseline ends
        // with two vertices at the SAME y (the baseline). A band ends on the
        // curve below, so those two differ wherever the lower series does — a
        // band that came out flat-bottomed would be an overlap wearing a new
        // name.
        let last_two_ys = |cmds: &[PathCommand]| -> Vec<f32> {
            cmds.iter()
                .filter_map(|c| match c {
                    PathCommand::LineTo(p) | PathCommand::MoveTo(p) => Some(p.y),
                    _ => None,
                })
                .rev()
                .take(2)
                .collect()
        };
        let flat_tail = last_two_ys(&flat_top);
        let band_tail = last_two_ys(&stacked_top);
        // Exact: both are the same baseline constant written twice by the
        // same code path, so a tolerance would weaken the claim rather than
        // make it robust.
        assert!(
            (flat_tail[0] - flat_tail[1]).abs() < f32::EPSILON,
            "an unstacked area closes flat on the baseline: {flat_tail:?}",
        );
        assert!(
            (band_tail[0] - band_tail[1]).abs() > 1.0,
            "a band closes on the CURVE below it: {band_tail:?}",
        );

        // And the axis reaches the total. `two_series` peaks below the sum, so
        // an unstacked domain would clip the top band off the plot.
        let stacked_bands = stacked.bands();
        let (_, top) = stacked_value_bounds(&stacked_bands).expect("bands exist");
        let per_series = crate::series::data_bounds(&two_series())
            .expect("points")
            .y
            .1;
        assert!(
            top > per_series,
            "the stack reaches {top}, above any one series' {per_series}",
        );

        // ...and the BUILD must use those bounds, not merely be able to
        // compute them. Pinning the domain to the per-series maximum is
        // exactly the defect: if `build` ignored the stacked bounds the two
        // pictures would be identical. A counterfactual found nothing
        // catching this until here.
        assert_eq!(
            stacked.resolved_y_domain(&stacked_bands),
            stacked_value_bounds(&stacked_bands),
            "the chart RESOLVES with the stack's extent, not merely knows it",
        );
        assert_eq!(
            LineChart::new(two_series())
                .filled(true)
                .resolved_y_domain(&[]),
            None,
            "an unstacked chart leaves the domain to the per-series auto-fit",
        );
        assert_eq!(
            LineChart::new(two_series())
                .stacked(true)
                .with_y_domain(0.0, per_series)
                .resolved_y_domain(&stacked_bands),
            Some((0.0, per_series)),
            "and an explicit domain still wins, as for every other rescale",
        );

        // NEGATIVE CONTROL — an unstacked chart reports no bands at all, so
        // `bands()` is not answering unconditionally.
        assert!(flat.bands().is_empty(), "nothing stacked, nothing to band");
        assert!(!stacked.bands().is_empty());
        // And `stacked(true)` implies a fill: a stack of bare polylines shows
        // the cumulative totals and hides the bands.
        assert_eq!(
            count_prefix(
                &LineChart::new(two_series())
                    .stacked(true)
                    .build(rect, &style),
                "chart.area."
            ),
            2,
        );
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

    // ── R1394 numeric brush-range cross-filter (select_x_range) ───────────
    /// A diagonal five-point ramp over x `0..=4`, pinned so the brush window
    /// maps predictably onto the plot.
    fn ramp_chart(range: Option<(f64, f64)>) -> Scene {
        let series = vec![Series::new(
            "s",
            (0..=4)
                .map(|i| DataPoint::new(f64::from(i), f64::from(i)))
                .collect(),
        )];
        LineChart::new(series)
            .with_x_domain(0.0, 4.0)
            .with_y_domain(0.0, 4.0)
            .select_x_range(range)
            .build(Rect::new(0, 0, 200, 200), &no_legend_zero_margin())
    }

    #[test]
    fn select_x_range_none_leaves_the_line_full_and_emits_no_focus() {
        let scene = ramp_chart(None);
        assert_eq!(
            count_prefix(&scene, "chart.focus."),
            0,
            "no focus overdraw without a window"
        );
        let Scene::Path(p) = find(&scene, "chart.series.0").expect("series") else {
            panic!("path")
        };
        assert_eq!(
            p.style.stroke.unwrap().color,
            CategoricalPalette::default().color(0),
            "the line stays full colour"
        );
    }

    #[test]
    fn select_x_range_mutes_the_context_line_and_overdraws_a_focus_segment() {
        let scene = ramp_chart(Some((1.0, 3.0)));
        let full = CategoricalPalette::default().color(0);
        let Scene::Path(context) = find(&scene, "chart.series.0").expect("context line") else {
            panic!("path")
        };
        assert_eq!(
            context.style.stroke.unwrap().color,
            full.with_alpha(MUTED_ALPHA),
            "the whole line dims to a context ghost"
        );
        let Scene::Path(focus) = find(&scene, "chart.focus.series.0").expect("focus segment")
        else {
            panic!("path")
        };
        assert_eq!(
            focus.style.stroke.unwrap().color,
            full,
            "the in-window segment keeps full colour"
        );
    }

    #[test]
    fn select_x_range_outside_the_series_draws_context_only() {
        // A window entirely past the data: the whole line is context, and the
        // focus overdraw would be a single point, so it is dropped.
        let scene = ramp_chart(Some((10.0, 20.0)));
        assert_eq!(
            count_prefix(&scene, "chart.focus.series.0"),
            0,
            "a window past the data emits no focus segment"
        );
        assert!(
            find(&scene, "chart.series.0").is_some(),
            "the context line is still drawn"
        );
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

    // --- R1440: the trace's colour encodes a measure ------------------------

    /// A trace whose y and third channel are INDEPENDENT: y climbs steadily
    /// while the measure swings from negative to positive, so a colour that
    /// tracked y would be indistinguishable from one that tracks the measure.
    fn measured_series() -> Vec<Series> {
        vec![Series::new(
            "trace",
            vec![
                DataPoint::new(0.0, 10.0).with_value(-4.0),
                DataPoint::new(1.0, 20.0).with_value(0.0),
                DataPoint::new(2.0, 30.0).with_value(4.0),
                DataPoint::new(3.0, 40.0).with_value(12.0),
            ],
        )]
    }

    fn ramp() -> ColorScale {
        ColorScale::viridis()
    }

    fn stroke_of(scene: &Scene, tag: &str) -> Color {
        match find(scene, tag) {
            Some(Scene::Path(p)) => p.style.stroke.expect("a stroked segment").color,
            _ => panic!("segment {tag} missing"),
        }
    }

    /// An UNENCODED line is byte-unchanged: one polyline, no segment nodes, and
    /// the categorical swatch row still stands.
    #[test]
    fn r1440_no_encoding_leaves_the_single_polyline_alone() {
        let scene = LineChart::new(measured_series())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "chart.series.0").is_some(), "one polyline");
        assert_eq!(
            count_prefix(&scene, "chart.series.0.seg."),
            0,
            "no segments"
        );
        assert!(find(&scene, "chart.colorbar.strip").is_none(), "no bar");
        assert!(find(&scene, "chart.legend.0.swatch").is_some(), "swatches");
    }

    /// The encoded trace becomes one stroked path per SEGMENT, and the single
    /// polyline goes away (two traces of the same line would double-strike it).
    #[test]
    fn r1440_an_encoded_trace_is_one_path_per_segment() {
        let scene = LineChart::new(measured_series())
            .color_by(ramp())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(
            count_prefix(&scene, "chart.series.0.seg."),
            3,
            "four samples make three segments"
        );
        assert!(
            find(&scene, "chart.series.0").is_none(),
            "and the flat polyline is not ALSO drawn"
        );
        // The legend changed kind.
        assert!(
            find(&scene, "chart.colorbar.strip").is_some(),
            "a colour bar"
        );
        assert!(
            find(&scene, "chart.legend.0.swatch").is_none(),
            "no swatches while colour means magnitude"
        );
    }

    /// ★ A segment takes the encoding at the MEAN of its endpoints, not at its
    /// start. The two differ for every segment here, so this pins the choice.
    #[test]
    fn r1440_a_segment_is_coloured_by_the_mean_of_its_endpoints() {
        let scale = ramp();
        let (lo, hi) = (-4.0, 12.0);
        let scene = LineChart::new(measured_series())
            .color_by(scale.clone())
            .with_color_domain(lo, hi)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        // Segment 0 spans -4 -> 0; its colour is the linear-light midpoint of
        // the two endpoint colours.
        let expected = scale.map(-4.0, lo, hi).lerp(scale.map(0.0, lo, hi), 0.5);
        assert_eq!(stroke_of(&scene, "chart.series.0.seg.0"), expected);
        assert_ne!(
            stroke_of(&scene, "chart.series.0.seg.0"),
            scale.map(-4.0, lo, hi),
            "★ not the START value's colour — that would read as a step function"
        );
        // Consecutive segments differ, so the trace really varies along itself.
        assert_ne!(
            stroke_of(&scene, "chart.series.0.seg.0"),
            stroke_of(&scene, "chart.series.0.seg.1"),
        );
    }

    /// The AREA takes a real gradient whose stops sit at the samples' own x
    /// positions within the AREA PATH's box — not at even spacing, and not
    /// against the plot rect.
    #[test]
    fn r1440_the_area_encodes_continuously_as_a_gradient_along_x() {
        let scene = LineChart::new(measured_series())
            .filled(true)
            .color_by(ramp())
            .with_color_domain(-4.0, 12.0)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let Some(Scene::Path(area)) = find(&scene, "chart.area.0") else {
            panic!("the area is a path")
        };
        let gradient = area.style.gradient.as_ref().expect("a real gradient");
        assert_eq!(gradient.stops.len(), 4, "one stop per sample");
        let offsets: Vec<f32> = gradient.stops.iter().map(|s| s.offset).collect();
        assert!(
            (offsets[0] - 0.0).abs() < 1e-3,
            "starts at the mark's left edge"
        );
        // NOT exactly 1.0, and correctly so: `bbox_of` gives the path a box one
        // px wider than the vertex span (a zero-width span still has to be
        // paintable), so the last SAMPLE sits one px short of the box's right
        // edge. Pin that px rather than papering over it with a loose epsilon.
        let short_by = (1.0 - offsets[3]) * to_f32(area.rect.w);
        assert!(
            short_by > 0.0 && short_by <= 1.5,
            "the last stop is within a px of the right edge, {short_by}px short \
             (offsets {offsets:?}, box {}px)",
            area.rect.w
        );
        assert!(
            offsets.windows(2).all(|w| w[0] < w[1]),
            "stops ascend with x: {offsets:?}"
        );
        // The samples are evenly spaced in x HERE, so the interior stops land on
        // the thirds — a naive "even spacing" would agree, which is why the
        // uneven-x case below is the one that discriminates.
        assert!((offsets[1] - 1.0 / 3.0).abs() < 0.02, "{offsets:?}");
    }

    /// ★ Uneven x spacing separates "stops at the samples' x" from "stops evenly
    /// spaced": the middle sample sits a TENTH along in x, so its stop must too.
    #[test]
    fn r1440_area_gradient_stops_follow_x_not_index() {
        let series = vec![Series::new(
            "uneven",
            vec![
                DataPoint::new(0.0, 10.0).with_value(0.0),
                DataPoint::new(1.0, 12.0).with_value(5.0),
                DataPoint::new(10.0, 14.0).with_value(10.0),
            ],
        )];
        let scene = LineChart::new(series)
            .filled(true)
            .color_by(ramp())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let Some(Scene::Path(area)) = find(&scene, "chart.area.0") else {
            panic!("the area is a path")
        };
        let gradient = area.style.gradient.as_ref().expect("a real gradient");
        let mid = gradient.stops[1].offset;
        assert!(
            (mid - 0.1).abs() < 0.02,
            "★ the middle sample is a tenth along in X, so is its stop (got {mid}) \
             — even spacing would put it at 0.5"
        );
    }

    /// ★ A clipped boundary sample carries an INTERPOLATED measure, so a pinned
    /// domain does not leave the trace unmeasured at its own edge.
    #[test]
    fn r1440_a_clipped_boundary_sample_keeps_an_interpolated_measure() {
        // Cut the domain at x = 0.5, halfway along segment 0 (-4 -> 0), so the
        // crossing's measure must be -2.
        let clipped = clip_to_x_domain(
            &[
                DataPoint::new(0.0, 10.0).with_value(-4.0),
                DataPoint::new(1.0, 20.0).with_value(0.0),
            ],
            0.5,
            1.0,
        );
        assert_eq!(clipped.len(), 2, "the crossing plus the in-domain sample");
        let crossing = clipped[0];
        assert!((crossing.x - 0.5).abs() < 1e-9);
        assert_eq!(
            crossing.value,
            Some(-2.0),
            "★ the third channel interpolates on the same t as y"
        );
        assert!(
            (crossing.y - 15.0).abs() < 1e-9,
            "and y is unchanged by the fix"
        );

        // A segment with no measures at either end stays unmeasured — the fix
        // interpolates, it does not invent.
        let plain = clip_to_x_domain(
            &[DataPoint::new(0.0, 10.0), DataPoint::new(1.0, 20.0)],
            0.5,
            1.0,
        );
        assert_eq!(
            plain[0].value, None,
            "no measure to interpolate, none made up"
        );
    }

    // ---- R1528: the logarithmic value axis -----------------------------

    /// A latency series whose structure lives across four decades, with one
    /// zero sample — the shape a log axis exists for, and the shape a log
    /// axis cannot fully carry.
    fn decades() -> Vec<Series> {
        vec![Series::new(
            "latency",
            vec![
                DataPoint::new(0.0, 0.4),
                DataPoint::new(1.0, 12.0),
                DataPoint::new(2.0, 0.0),
                DataPoint::new(3.0, 730.0),
            ],
        )]
    }

    fn label_text(scene: &Scene, tag: &str) -> String {
        match find(scene, tag) {
            Some(Scene::Text(t)) => t.content.clone(),
            _ => panic!("no text node tagged {tag}"),
        }
    }

    fn rect() -> Rect {
        Rect::new(0, 0, 480, 320)
    }

    // ---- R1545: the axis is swappable to categorical ----------------------

    /// ★ The gap this round closes, stated as a test: a NUMERIC-x chart can
    /// now take the categorical axis, exactly as it can take the log and time
    /// ones. Its x labels become the category names — over the identical data
    /// the default linear axis labels the same positions `0, 1, 2, …`, which
    /// is the counterfactual.
    #[test]
    fn r1545_a_line_chart_can_swap_in_the_category_axis() {
        let series = vec![Series::new(
            "revenue",
            (0..4)
                .map(|i| DataPoint::new(f64::from(i), 10.0 + f64::from(i)))
                .collect(),
        )];
        let names = ["North", "South", "East", "West"];

        let categorical = LineChart::new(series.clone())
            .x_category(names)
            .build(rect(), &ChartStyle::default());
        let labels: Vec<String> = (0..4)
            .map(|k| label_text(&categorical, &format!("chart.label.x.{k}")))
            .collect();
        assert_eq!(labels, names, "the axis labels its slots by name");

        let linear = LineChart::new(series).build(rect(), &ChartStyle::default());
        assert_ne!(
            label_text(&linear, "chart.label.x.0"),
            "North",
            "a linear x labels the same positions numerically"
        );
    }

    /// ★ A sample that names no slot draws no vertex and is reported — the
    /// same contract the log axis's zero has, on the categorical arm.
    ///
    /// Read on the **y**-axis, because the x-axis clip is a different rule
    /// that would mask this one: a sample past the x-domain is clipped to the
    /// domain edge with an interpolated crossing (R1356, so a windowed line
    /// meets the plot edge), and that crossing IS a position the axis
    /// defines. So on x the off-list sample is reported and its own position
    /// is never placed, while on y — where no clip runs — the vertex simply
    /// is not there.
    #[test]
    fn r1545_a_sample_off_the_category_list_is_reported_not_drawn() {
        let samples = || {
            vec![Series::new(
                "s",
                vec![
                    DataPoint::new(0.0, 0.0),
                    DataPoint::new(1.0, 1.0),
                    DataPoint::new(2.0, 7.0),
                ],
            )]
        };
        let verts = |scene: &Scene| match find(scene, "chart.series.0") {
            Some(Scene::Path(p)) => p.commands.len(),
            _ => panic!("the series is a path"),
        };

        let chart = LineChart::new(samples()).y_category(["low", "mid", "high"]);
        let off = chart.off_scale();
        assert_eq!(off.len(), 1, "only the sample naming no slot: {off:?}");
        assert!((off[0].point.y - 7.0).abs() < f64::EPSILON);
        assert_eq!(
            verts(&chart.build(rect(), &ChartStyle::default())),
            2,
            "the off-list sample contributes no vertex"
        );
        // The counterfactual: over the identical data a linear y carries all
        // three, so the missing vertex is the AXIS's decision, not the data's.
        assert_eq!(
            verts(&LineChart::new(samples()).build(rect(), &ChartStyle::default())),
            3
        );

        // And on x the report is the same, from the same `defines` rule.
        let on_x = LineChart::new(vec![Series::new(
            "s",
            vec![DataPoint::new(0.0, 5.0), DataPoint::new(7.0, 9.0)],
        )])
        .x_category(["a", "b", "c"]);
        let off = on_x.off_scale();
        assert_eq!(off.len(), 1);
        assert!((off[0].point.x - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn r1528_log_y_axis_snaps_to_decades_and_labels_them() {
        let scene = LineChart::new(decades())
            .y_log()
            .build(rect(), &ChartStyle::default());
        // 0.1 .. 1000 — the whole-decade bracket of the POSITIVE data
        // (0.4 .. 730); the zero sample does not drag the floor anywhere,
        // because a log axis has no floor to drag it to.
        let labels: Vec<String> = (0..5)
            .map(|k| label_text(&scene, &format!("chart.label.y.{k}")))
            .collect();
        assert_eq!(labels, ["0.1", "1", "10", "100", "1k"]);
    }

    /// ★ The defining property, read off the painted scene: equal ratios
    /// occupy equal pixel spans. A linear axis over the same data cannot,
    /// which is the counterfactual that makes the assertion mean something.
    #[test]
    fn r1528_decade_gridlines_are_evenly_spaced_and_linear_ones_are_not() {
        let gaps = |scene: &Scene| -> Vec<f32> {
            let ys: Vec<f32> = (0..5)
                .filter_map(|k| find(scene, &format!("chart.grid.y.{k}")))
                .map(|g| match g {
                    Scene::Path(p) => to_f32(p.rect.y),
                    _ => panic!("gridline is a path"),
                })
                .collect();
            ys.windows(2).map(|w| (w[1] - w[0]).abs()).collect()
        };

        let log = LineChart::new(decades())
            .y_log()
            .build(rect(), &ChartStyle::default());
        let log_gaps = gaps(&log);
        assert_eq!(log_gaps.len(), 4, "four gaps between five decades");
        let first = log_gaps[0];
        for g in &log_gaps {
            assert!(
                (g - first).abs() <= 1.0,
                "every decade spans the same pixels: {log_gaps:?}"
            );
        }

        // The linear chart over the SAME data: 0..800 in nice steps, so the
        // three sub-millisecond-to-12ms samples all land within a few pixels
        // of the baseline — the structure the log axis exists to reveal.
        let lin = LineChart::new(decades()).build(rect(), &ChartStyle::default());
        let plot = CartesianPlot::resolve(
            rect(),
            &decades(),
            None,
            None,
            &ChartStyle::default(),
            Rescale::default(),
            &AxisKinds::default(),
        );
        let spread = plot.y.map(0.4).unwrap() - plot.y.map(12.0).unwrap();
        assert!(
            spread.abs() < 5.0,
            "linear: 0.4ms and 12ms are {spread}px apart — indistinguishable"
        );
        assert!(!gaps(&lin).is_empty(), "the linear chart still has a grid");
    }

    /// ★ A sample a log axis cannot carry is REPORTED, not placed. Placing it
    /// on the baseline would be indistinguishable from a real sample at the
    /// domain floor, which is the failure `Mapped::unreadable` exists to
    /// avoid on the other input path.
    #[test]
    fn r1528_off_scale_samples_are_reported_and_draw_no_mark() {
        let chart = LineChart::new(decades()).y_log();
        let off = chart.off_scale();
        assert_eq!(off.len(), 1, "exactly the zero sample");
        assert_eq!(off[0].series, 0);
        assert!(
            (off[0].point.x - 2.0).abs() < f64::EPSILON,
            "the x=2 sample"
        );
        assert!(
            off[0].point.y.abs() < f64::EPSILON,
            "reported with its own value"
        );

        // The same chart on a linear axis reports nothing: zero is an
        // ordinary value there, so this is a property of the AXIS, not of
        // the data.
        assert!(LineChart::new(decades()).off_scale().is_empty());

        // And the polyline carries three points, not four — the dropped one
        // leaves no vertex at the baseline.
        let scene = chart.build(rect(), &ChartStyle::default());
        let Some(Scene::Path(path)) = find(&scene, "chart.series.0") else {
            panic!("series path");
        };
        let vertices = path
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::MoveTo(_) | PathCommand::LineTo(_)))
            .count();
        assert_eq!(vertices, 3, "the zero sample contributes no vertex");
    }

    #[test]
    fn r1528_minor_gridlines_appear_only_on_a_log_axis() {
        let log = LineChart::new(decades())
            .y_log()
            .build(rect(), &ChartStyle::default());
        // Four decades x eight subdivisions, clipped to the domain.
        assert_eq!(count_prefix(&log, "chart.grid.minor.y."), 32);
        assert_eq!(
            count_prefix(&log, "chart.grid.minor.x."),
            0,
            "the x-axis is still linear, so it has no minor ticks"
        );
        // The labelled grid keeps counting only the labelled lines, so every
        // pre-R1528 gridline assertion still means what it meant.
        assert_eq!(count_prefix(&log, "chart.grid.y."), 5);

        let lin = LineChart::new(decades()).build(rect(), &ChartStyle::default());
        assert_eq!(count_prefix(&lin, "chart.grid.minor."), 0);
    }

    #[test]
    fn r1528_a_log_chart_with_no_positive_data_stays_legible() {
        // Every sample off-scale: the axis draws its unit decade and reports
        // all of them, rather than collapsing or dividing by zero.
        let flat = vec![Series::new(
            "dead",
            vec![DataPoint::new(0.0, 0.0), DataPoint::new(1.0, -3.0)],
        )];
        let chart = LineChart::new(flat).y_log();
        assert_eq!(chart.off_scale().len(), 2);
        let scene = chart.build(rect(), &ChartStyle::default());
        assert_eq!(label_text(&scene, "chart.label.y.0"), "1");
        assert!(
            find(&scene, "chart.series.0").is_none(),
            "no polyline: not one point can be placed"
        );
    }

    #[test]
    fn r1528_log_scrub_inverts_through_the_axis_and_skips_off_scale_points() {
        let style = ChartStyle::default();
        // x is log too, so a scrub at the middle of the plot must land near
        // the GEOMETRIC middle of the x-domain, not the arithmetic one.
        let series = vec![Series::new(
            "s",
            vec![
                DataPoint::new(1.0, 5.0),
                DataPoint::new(10.0, 6.0),
                DataPoint::new(100.0, 7.0),
            ],
        )];
        let readout = LineChart::new(series)
            .x_log()
            .inspect(Some(0.5))
            .inspect_readout(rect(), &style)
            .expect("a scrub in the middle finds a point");
        assert!(
            readout.starts_with("x = 10"),
            "the geometric middle of 1..100 is 10, not 50.5 — got {readout}"
        );

        // A series whose nearest sample to the cursor is off-scale still
        // contributes its nearest DRAWN sample, rather than dropping out.
        let holed = vec![Series::new(
            "h",
            vec![
                DataPoint::new(0.0, 4.0),
                DataPoint::new(5.0, 0.0),
                DataPoint::new(10.0, 9.0),
            ],
        )];
        let readout = LineChart::new(holed)
            .y_log()
            .inspect(Some(0.5))
            .inspect_readout(rect(), &style)
            .expect("the scrub still finds a drawable point");
        assert!(
            !readout.contains("h 0"),
            "the off-scale sample is never named: {readout}"
        );
    }
}
