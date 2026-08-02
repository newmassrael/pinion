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
//! `stroke_path`, a tick a `label_node`). R1377's scatter chart was the third
//! axis-based chart the mid-level ASSEMBLY was waiting for, so the horizontal
//! gridlines, the two axes, and the y-tick-label loop are now the SHARED
//! [`crate::draw`] furniture (`gridlines` / `axes` / `y_tick_labels`) this
//! builder CALLS rather than re-derives. Both builders emit the same tagged,
//! layout-native `Scene`, so a bar chart docks / flexes / resizes and reads
//! back over §2 #7 introspection exactly as a line chart does.
//!
//! # The x is an axis (R1545)
//!
//! It did not used to be. The slot metric was `left + i * slot`, written out
//! three times in this file — for the bar box, for its label, for its click
//! surface — and reachable from nowhere else, which is why the crate's
//! charting axis could offer linear, logarithmic and time but not
//! categorical. It is now a [`CategoryScale`]: the bar
//! box, the label box and the hit region are all one
//! [`band`](crate::CategoryScale::band) call, the label text comes from the
//! axis's own [`TickFormat`], the inspect hit-test is the axis's
//! [`nearest`](crate::CategoryScale::nearest), and the same axis can be
//! handed to a line or scatter chart
//! ([`LineChart::x_category`](crate::LineChart::x_category)).
//!
//! [`CategoryScale`]: crate::CategoryScale
//!
//! What that bought beyond tidiness is [`BarChart::x_window`] — Qt's
//! `QBarCategoryAxis::setRange`. A window narrows the axis domain, and
//! because everything above derives from the axis, the bars, their labels,
//! their click surfaces and the inspect hit-test all follow with no further
//! code.
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
//! When [`inspect`](BarChart::inspect) is set the overlay adds
//! `chart.inspect.highlight` (a ring framing the focused bar — absent when that
//! bar draws no box), `chart.inspect.tooltip` (the callout box),
//! `chart.inspect.header` (the focused category label) and `chart.inspect.value`
//! (its value — one, singular, unlike line's per-series `chart.inspect.value.{i}`,
//! because a categorical slot holds one value).
//!
//! [`select`](BarChart::select) + [`selectable`](BarChart::selectable) add
//! cross-filtering (R1384): `select` mutes the bars outside the active category
//! set (their `chart.bar.{i}` box keeps its tag but drops to a low alpha),
//! and `selectable` overlays one transparent, focusable, hit-testable region per
//! bar — tagged in the CALLER's namespace (not `chart.*`), spanning the whole
//! slot column — so a click in a category's column routes to that bar's tag.
//! The chart only renders the selection; the caller owns the state and the
//! cross-widget link (see `examples/hello-cross-filter`).
//!
//! [`select_x_range`](BarChart::select_x_range) adds the NUMERIC cross-filter
//! (R1395): when a bar declares a numeric [`bin`](Bar::bin) (`[lo, hi)`), a
//! brush window mutes the bars whose bin falls outside it — so the same numeric
//! x-window brush that dims scatter points ([`ScatterChart::select_x_range`](crate::ScatterChart::select_x_range))
//! or a line's out-of-window portion ([`LineChart::select_x_range`](crate::LineChart::select_x_range))
//! also dims the matching HISTOGRAM bins here. It is the DIFFERENT-geometry leg
//! of the cross-filter matrix (a numeric brush over a categorical bar layout);
//! see `examples/hello-histogram-brush`.
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
use pinion_core::style::{Color, LayoutStyle, Size, TextAlign};

use crate::draw::{
    CalloutRow, MUTED_ALPHA, absolute, box_node, callout, fill_parent, label_node, outline_box,
    plot_rect, to_f32, to_u32,
};
use crate::palette::CategoricalPalette;
use crate::plot::axis_format;
use crate::scale::{
    Categories, CategoryScale, CategoryWindow, LinearScale, ValueScale, index_value,
};
use crate::style::ChartStyle;
use crate::ticks::{TickFormat, format_axis_tick, nice_ticks, tick_step};

/// The fraction of each bar's slot left empty as the inter-bar gap (so
/// adjacent bars read as distinct). `0.2` = a bar fills 80% of its slot.
const BAR_GAP_FRAC: f32 = 0.2;

/// One bar: a category `label`, its `value`, an optional per-bar `color`
/// override (else the chart's palette colour), and an optional numeric `bin`
/// extent. The colour override is what lets a histogram paint its over-budget
/// bins in a distinct colour; the `bin` is what lets a numeric brush
/// cross-filter it ([`BarChart::select_x_range`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    /// The category label, centred under the bar on the x-axis.
    pub label: String,
    /// The bar's value on the y-axis.
    pub value: f64,
    /// An optional colour override for THIS bar.
    pub color: Option<Color>,
    /// The numeric half-open interval `[lo, hi)` this bar spans on an implicit
    /// x-axis — a histogram bin. `None` (the default) = a purely categorical
    /// bar with no numeric position, which [`BarChart::select_x_range`] never
    /// mutes (there is no window to compare it to). Set it (via
    /// [`Bar::with_bin`]) to make the bar a histogram bin a numeric brush can
    /// cross-filter — the categorical-layout analogue of a scatter point's own
    /// numeric x.
    pub bin: Option<(f64, f64)>,
}

impl Bar {
    /// A bar with the default (palette) colour and no numeric bin extent.
    #[must_use]
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        Self {
            label: label.into(),
            value,
            color: None,
            bin: None,
        }
    }

    /// Override this bar's colour (e.g. an over-budget histogram bin).
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Declare this bar a histogram bin spanning the numeric half-open interval
    /// `[lo, hi)` — the numeric x-extent a [`BarChart::select_x_range`] brush
    /// window is tested against. A bar without a bin is never numerically muted.
    #[must_use]
    pub fn with_bin(mut self, lo: f64, hi: f64) -> Self {
        self.bin = Some((lo, hi));
        self
    }
}

/// A vertical bar chart over labelled [`Bar`]s, sharing the crate's
/// scale / ticks / palette / draw core with [`crate::line::LineChart`].
pub struct BarChart {
    bars: Vec<Bar>,
    palette: CategoricalPalette,
    y_domain: Option<(f64, f64)>,
    inspect: Option<f32>,
    /// The active cross-filter mask (R1384): `selected[i]` = category `i` is in
    /// the filter set. Empty / all-`false` = no filter (every bar full).
    selected: Vec<bool>,
    /// Per-bar caller tags making the bars clickable (R1384); `None` = the bars
    /// are display-only.
    select_tags: Option<Vec<String>>,
    /// The active numeric cross-filter window (R1395): a bar whose
    /// [`bin`](Bar::bin) does not overlap `[lo, hi]` renders muted. `None` = no
    /// numeric filter. Orthogonal to the categorical [`selected`](Self::selected)
    /// mask; either dims a bar.
    select_x_range: Option<(f64, f64)>,
    /// R1545 — the x-axis's category list, derived from the bar labels once at
    /// construction. Held rather than rebuilt per frame because it is what the
    /// [`AxisKind::Category`](crate::AxisKind) the axis resolves from carries,
    /// and rebuilding it would clone `n` heap strings on every paint.
    categories: Categories,
    /// R1545 — the visible slice of the category axis (Qt
    /// `QBarCategoryAxis::setRange`). `None` shows every category.
    x_window: Option<CategoryWindow>,
    tag_prefix: String,
}

impl BarChart {
    /// A bar chart over `bars`, using the default palette, an auto y-domain
    /// (baseline `0` to the data max, nice-tick-snapped), no inspect overlay,
    /// every category in view, and the `"chart"` tag prefix.
    #[must_use]
    pub fn new(bars: Vec<Bar>) -> Self {
        let categories = Categories::new(bars.iter().map(|b| b.label.clone()));
        Self {
            bars,
            palette: CategoricalPalette::default(),
            y_domain: None,
            inspect: None,
            selected: Vec::new(),
            select_tags: None,
            select_x_range: None,
            categories,
            x_window: None,
            tag_prefix: "chart".to_string(),
        }
    }

    /// This chart's x-axis category list (R1545) — Qt's
    /// `QBarCategoryAxis::categories`, derived from the bar labels.
    ///
    /// The entry point for resolving a [`x_window`](Self::x_window) by NAME:
    /// `chart.categories().window("Mar", "Aug")` answers a
    /// [`CategoryWindow`] or names why the request could not be honoured
    /// ([`CategoryLookup`](crate::CategoryLookup)).
    #[must_use]
    pub const fn categories(&self) -> &Categories {
        &self.categories
    }

    /// Show only `window`'s slice of the category axis — Qt's
    /// `QBarCategoryAxis::setRange(min, max)`, with the resolution already
    /// done (see [`categories`](Self::categories)).
    ///
    /// The bars, their labels, their click surfaces and the inspect hit-test
    /// all narrow together, because all four derive from the axis. A 60-bar
    /// chart in 600px gives every bar 10px; windowed to twelve it gives each
    /// one 50px, which is the difference between a readable axis and a comb.
    ///
    /// Qt takes the two endpoints as `QString`s and returns `void`, so a name
    /// that is not a category leaves the axis silently unwindowed. Here the
    /// name is resolved before it can reach the chart, so a typo is a value
    /// the caller must handle rather than a chart that quietly ignored it.
    #[must_use]
    pub const fn x_window(mut self, window: CategoryWindow) -> Self {
        self.x_window = Some(window);
        self
    }

    /// The categories currently **in view** in a chart of `rect` under
    /// `style` — the window a wheel zoom or [`x_window`](Self::x_window) has
    /// left, resolved through the same axis the bars are drawn from.
    ///
    /// `None` when the chart carries no category, or when the window has
    /// closed past every one of them. Qt has no equivalent: a
    /// `QBarCategoryAxis` reports `count()` (all of them) and the min/max
    /// names it was *set* to, never which slots a live window is showing.
    #[must_use]
    pub fn visible_categories(&self, rect: Rect, style: &ChartStyle) -> Option<CategoryWindow> {
        self.geom(rect, style).x.visible()
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

    /// Show the inspect overlay (a highlight ring around the focused bar and a
    /// value tooltip) at `fraction` — the cursor's position as a fraction
    /// `0.0..=1.0` across the chart `rect` width. `None` (the default) draws no
    /// overlay. Mirrors [`LineChart::inspect`](crate::line::LineChart::inspect):
    /// the fraction is the natural output of a pointer-capture scrub
    /// (`SliderExternal` value / `pointer_move` `x_rel`), and the chart maps it
    /// through its own margins to the CATEGORICAL slot under the cursor (the
    /// bar-chart analogue of the line chart's nearest-numeric-point), so the
    /// caller needs no layout knowledge.
    #[must_use]
    pub fn inspect(mut self, fraction: Option<f32>) -> Self {
        self.inspect = fraction;
        self
    }

    /// Mark the active cross-filter selection (R1384): given a per-bar mask
    /// (`active[i]` = category `i` is in the filter set), every bar OUTSIDE the
    /// set renders muted (dimmed to a low alpha) while the selected bars
    /// keep full strength — so the chart reads as "these categories are the
    /// active filter". An empty mask, or an all-`false` one, is "no filter" and
    /// leaves every bar at full strength (the crossfilter convention: no
    /// selection = all data).
    ///
    /// This is only the RENDER of the selection state.
    /// [`selectable`](Self::selectable) is what makes the bars *drive* it from a
    /// pointer, and a companion widget reads the same mask to filter its own
    /// data — the cross-widget link that makes this cross-filtering rather than
    /// a self-contained toggle. Mirrors the line chart's
    /// [`Series::visible`](crate::Series) mute + interactive-legend split
    /// ("the chart emits, the caller owns the state").
    #[must_use]
    pub fn select(mut self, active: Vec<bool>) -> Self {
        self.selected = active;
        self
    }

    /// Make each bar a focusable, hit-testable region tagged with the caller's
    /// `tags[i]` (one per bar, in bar order): a transparent overlay spanning the
    /// bar's whole slot COLUMN, so the router's deepest-tagged-ancestor hit-test
    /// resolves a click anywhere in that category's column to `tags[i]`. This is
    /// the same chip mechanism
    /// [`LineChart::interactive_legend`](crate::LineChart::interactive_legend)
    /// gives its legend entries, now applied to the bars themselves — the caller
    /// owns the tag namespace and wires each tag to set [`select`](Self::select),
    /// so the chart stays a pure scene producer. Only the first `tags.len()`
    /// bars become clickable; any extra tags past the last bar are ignored, and
    /// bars past `tags.len()` stay display-only.
    #[must_use]
    pub fn selectable(mut self, tags: Vec<String>) -> Self {
        self.select_tags = Some(tags);
        self
    }

    /// Cross-filter the bars to a numeric x-range: with `Some((lo, hi))`, every
    /// bar whose [`bin`](Bar::bin) does NOT overlap `[lo, hi]` renders muted
    /// (its box dimmed to a low alpha) while the in-window bars stay full — the
    /// **different-geometry** peer of the numeric-range
    /// [`ScatterChart::select_x_range`](crate::ScatterChart::select_x_range)
    /// (R1391) / [`LineChart::select_x_range`](crate::LineChart::select_x_range)
    /// (R1394): the same brush window that mutes scatter points or a line's
    /// out-of-window portion here mutes the matching HISTOGRAM bins, so one
    /// numeric brush cross-filters a bar chart's categorical bar layout too.
    ///
    /// A bar with no `bin` is never muted by this (it has no numeric position);
    /// a bar's bin `[blo, bhi)` overlaps the window iff `blo < hi && bhi > lo`,
    /// so a bin touching a window edge follows the half-open convention. `None`
    /// (the default) is "no filter" and every bar renders full — the crossfilter
    /// convention that no selection = all data, which makes a full-span brush a
    /// natural no-op (no bin is ever outside the whole extent). Muted bars stay
    /// DRAWN (dimmed as context), like the scatter's muted points. Render-only;
    /// the caller owns the range (e.g. wired from a `RangeSlider` brush through
    /// the [`Brush`](crate::Brush) substrate). Orthogonal to
    /// [`select`](Self::select): either dims a bar.
    #[must_use]
    pub fn select_x_range(mut self, range: Option<(f64, f64)>) -> Self {
        self.select_x_range = range;
        self
    }

    /// Whether bar `i` is in the active cross-filter set. An out-of-range index
    /// is inactive, so a short mask simply leaves the trailing bars unselected.
    fn is_active(&self, i: usize) -> bool {
        self.selected.get(i).copied().unwrap_or(false)
    }

    /// Whether `bar` falls OUTSIDE the active numeric brush window (R1395), so
    /// its box mutes. A bar with no [`bin`](Bar::bin), or no active range, is
    /// never x-muted; otherwise the bin `[blo, bhi)` is muted when it does not
    /// overlap the window `[lo, hi]` (`blo >= hi || bhi <= lo`).
    fn is_x_muted(&self, bar: &Bar) -> bool {
        match (self.select_x_range, bar.bin) {
            (Some((lo, hi)), Some((blo, bhi))) => blo >= hi || bhi <= lo,
            _ => false,
        }
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
        let g = self.geom(rect, style);
        // Resolved once so the painted bars, the highlight ring, and the
        // tooltip all read the ONE geometry (R1375); split so the ring paints
        // over the bars and the tooltip above everything (`Scene` is not
        // `Clone`, so the two layers move out by value here).
        let (highlight, tooltip) = match self.resolve_inspect(&g, rect, style) {
            Some(i) => (i.highlight, i.tooltip),
            None => (None, Vec::new()),
        };

        let mut children: Vec<Scene> = Vec::new();
        if let Some(bg) = style.background {
            children.push(box_node(rect, bg, format!("{}.bg", self.tag_prefix)));
        }
        // Horizontal gridlines (one per y-tick) + the L-frame axes — the shared
        // cartesian furniture (R1377). A bar chart's x-axis is CATEGORICAL, so
        // it passes NO x-gridline positions (its slots are labelled, not ticked).
        let y_pos: Vec<f32> = g.y_ticks.iter().map(|&t| g.y.map(t)).collect();
        children.extend(crate::draw::gridlines(
            (g.left, g.right, g.top, g.bottom),
            &[],
            &y_pos,
            style,
            &self.tag_prefix,
        ));
        children.extend(crate::draw::axes(
            (g.left, g.right, g.top, g.bottom),
            style,
            &self.tag_prefix,
        ));

        // The cross-filter selection (R1384): when ANY category is selected,
        // bars OUTSIDE the active set render muted; an empty / all-`false` mask
        // is "no filter" and leaves every bar full.
        let any_selected = self.selected.iter().any(|&a| a);

        // Bars — one per VISIBLE category (R1545: the window narrows what is
        // drawn, not only what is domained), each centred in its band and
        // grown from the baseline to its value. The per-slot geometry lives in
        // `bar_slot_center_and_rect`, the shared source the inspect highlight
        // also reads.
        let size = style.label_size_px.max(1);
        // The label text comes from the AXIS, not from `bar.label` directly, so
        // the string under a slot is the one the axis carries for it — the same
        // derivation the tick labels of every other axis kind take (R1525).
        let x_format = axis_format(&g.x_axis(), &[]);
        for i in visible_indices(&g) {
            let bar = &self.bars[i];
            let (_, bar_rect) = self.bar_slot_center_and_rect(&g, i);
            if let Some(r) = bar_rect {
                // A bar defaults to its palette colour BY INDEX (a categorical
                // chart's distinct-per-bar colouring); a per-bar override wins
                // (a histogram paints every bin uniform + its over-budget bins
                // in the jank colour).
                let base = bar.color.unwrap_or_else(|| self.palette.color(i));
                // A bar is dimmed when EITHER cross-filter excludes it: the
                // categorical set (R1384 — not in a non-empty active set) or the
                // numeric brush window (R1395 — its bin outside `[lo, hi]`).
                let muted = (any_selected && !self.is_active(i)) || self.is_x_muted(bar);
                let color = if muted {
                    base.with_alpha(MUTED_ALPHA)
                } else {
                    base
                };
                children.push(box_node(r, color, format!("{}.bar.{i}", self.tag_prefix)));
            }
            // Category label centred under the slot (always, even for a
            // non-finite bar, so the axis stays legible).
            let (slot_lo, slot_hi) = g.x.band(i).unwrap_or((g.left, g.left));
            children.push(label_node(
                x_format.label(index_value(i)),
                to_u32(slot_lo),
                to_u32(g.bottom) + 4,
                to_u32(slot_hi - slot_lo).max(1),
                TextAlign::Center,
                style.label,
                size,
                format!("{}.xlabel.{i}", self.tag_prefix),
            ));
        }

        // The highlight ring sits over the bars (so it frames the focused one)
        // but under the y-labels and tooltip.
        if let Some(highlight) = highlight {
            children.push(highlight);
        }

        // Right-aligned y-axis tick labels in the left gutter — the shared
        // cartesian furniture (R1377), reusing the `y_pos` mapped above.
        children.extend(crate::draw::y_tick_labels(
            rect.x,
            &g.y_ticks,
            &y_pos,
            // A bar encodes magnitude by length from a ZERO baseline, and a
            // log axis has no zero — so this axis is linear by construction
            // and takes the constant-step format (R1528). Qt permits a log
            // bar chart; the length encoding is what makes it a lie.
            &TickFormat::Step(g.y_step),
            style,
            &self.tag_prefix,
        ));

        // The tooltip paints above everything.
        children.extend(tooltip);

        // Cross-filter click surfaces (R1384): a transparent, focusable, tagged
        // overlay per bar spanning its FULL slot column, emitted LAST so the
        // geometric hit-test resolves a click anywhere in a category's column to
        // that bar's caller tag (the bar analogue of the interactive legend's
        // per-entry chip). Transparent (no `BoxStyle`), so it never obscures the
        // bar / mute beneath it — it exists only to be hit.
        if let Some(tags) = &self.select_tags {
            let col_h = to_u32(g.bottom - g.top);
            for i in visible_indices(&g) {
                let Some(tag) = tags.get(i) else { continue };
                let (slot_lo, slot_hi) = g.x.band(i).unwrap_or((g.left, g.left));
                children.push(Scene::Container(
                    ContainerNode::new(Vec::new())
                        .with_tag(tag.clone())
                        .with_layout(
                            LayoutStyle::new()
                                .with_absolute_position(to_u32(slot_lo), to_u32(g.top))
                                .with_size(Size::px(to_u32(slot_hi - slot_lo).max(1), col_h))
                                .with_focusable(true),
                        ),
                ));
            }
        }

        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
    }

    /// The plot geometry, value scale, y-tick set, and slot metrics every bar,
    /// gridline, and the inspect hit-test derive from — the ONE definition of
    /// "where does bar `i` sit" (R1375), so the painted bar and the inspect
    /// highlight can never disagree.
    #[allow(
        clippy::cast_precision_loss,
        reason = "the bar count is a small display count, exact in f32"
    )]
    fn geom(&self, rect: Rect, style: &ChartStyle) -> BarGeom {
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
        // R1545 — the x-axis. Its domain is the category window when one is
        // set (Qt `QBarCategoryAxis::setRange`), else the whole band run; the
        // slot width the bars are sized from is the axis's own band width, so
        // "how wide is a bar" and "where is category i" answer from one place.
        let domain = self
            .x_window
            .map_or_else(|| self.categories.extent(), CategoryWindow::domain);
        let x = CategoryScale::new(self.categories.clone(), domain, (left, right));
        let bar_w = (x.band_width() * (1.0 - BAR_GAP_FRAC)).max(1.0);
        BarGeom {
            left,
            right,
            top,
            bottom,
            y,
            y_ticks,
            y_step,
            baseline_y,
            x,
            bar_w,
        }
    }

    /// Bar `i`'s slot-centre x (the inspect anchor) and its filled box rect —
    /// `None` for a non-finite value (present as a slot, but no drawn box), or
    /// for an index the axis carries no category for. The single arithmetic
    /// both [`Self::build_body`] and the inspect highlight read, so the ring
    /// lands exactly on the bar.
    fn bar_slot_center_and_rect(&self, g: &BarGeom, i: usize) -> (f32, Option<Rect>) {
        // R1545 — the band IS the slot: `(lo, hi)` from the axis, where this
        // file used to compute `left + i * slot` itself.
        let (slot_lo, slot_hi) = g.x.band(i).unwrap_or((g.left, g.left));
        let bx = slot_lo + ((slot_hi - slot_lo) - g.bar_w) / 2.0;
        let center_x = bx + g.bar_w / 2.0;
        let Some(bar) = self.bars.get(i) else {
            return (center_x, None);
        };
        let rect = bar.value.is_finite().then(|| {
            // Clamp the bar top into the plot: a value outside a PINNED
            // `y_domain` would otherwise map past the plot edge and — a
            // `Scene::Box` is not rect-clipped by its container — paint over the
            // axis / gridlines / neighbours. The auto domain always brackets the
            // data, so this only guards a pinned domain.
            let top_y = g.y.map(bar.value).clamp(g.top, g.bottom);
            // A bar above the baseline grows up (top = value, height = baseline
            // - value); a negative bar grows down.
            let (rect_top, rect_h) = if top_y <= g.baseline_y {
                (top_y, g.baseline_y - top_y)
            } else {
                (g.baseline_y, top_y - g.baseline_y)
            };
            // `max(1.0)`: a zero-value bar (an empty histogram bin) still emits a
            // 1px baseline stub, so its slot stays addressable and slot-aligned —
            // distinct from a non-finite bar, which is omitted entirely.
            Rect::new(
                to_u32(bx),
                to_u32(rect_top),
                to_u32(g.bar_w),
                to_u32(rect_h.max(1.0)),
            )
        });
        (center_x, rect)
    }

    /// The slot index the inspect cursor is over, or `None` when inspection is
    /// off / no category is in view / the plot is degenerate. A CATEGORICAL
    /// hit-test — the bar-chart analogue of the line chart's
    /// nearest-numeric-point resolve.
    ///
    /// R1545 asks the axis ([`CategoryScale::nearest`]) rather than dividing
    /// the plot width by the bar count, which is what makes the hit-test
    /// follow a window: with categories `20..=31` in view, the pixel under the
    /// cursor inverts to an index in that range instead of to a fraction of
    /// all sixty.
    fn resolve_focus(&self, g: &BarGeom, rect: Rect) -> Option<usize> {
        let fraction = self.inspect?;
        if g.right - g.left <= 0.0 {
            return None;
        }
        let cursor_px = to_f32(rect.x) + fraction.clamp(0.0, 1.0) * to_f32(rect.w);
        g.x.nearest(cursor_px.clamp(g.left, g.right))
    }

    /// Resolve the inspect overlay: a highlight ring around the focused bar
    /// (absent when that bar draws no box, e.g. a non-finite value) and a value
    /// tooltip (`{label}` header + one `{value}` row).
    fn resolve_inspect(&self, g: &BarGeom, rect: Rect, style: &ChartStyle) -> Option<BarInspect> {
        let idx = self.resolve_focus(g, rect)?;
        let bar = &self.bars[idx];
        let (center_x, bar_rect) = self.bar_slot_center_and_rect(g, idx);
        let highlight = bar_rect.map(|r| {
            outline_box(
                r,
                style.crosshair,
                format!("{}.inspect.highlight", self.tag_prefix),
            )
        });
        let rows = vec![CalloutRow {
            text: value_text(bar, g.y_step),
            color: style.tooltip_fg,
            tag: format!("{}.inspect.value", self.tag_prefix),
        }];
        let tooltip = callout(
            center_x,
            g.right,
            g.top,
            &bar.label,
            format!("{}.inspect.header", self.tag_prefix),
            &rows,
            style,
            format!("{}.inspect.tooltip", self.tag_prefix),
        );
        Some(BarInspect { highlight, tooltip })
    }

    /// The inspect readout as one line — the same focus label + value the
    /// tooltip paints (`8 = 3`), or `None` when nothing is inspected. A
    /// consumer wires this into the scrub control's `WidgetA11y` node (via
    /// `pinion_a11y::described::describedby_region`) so the reading a sighted
    /// user sees in the tooltip reaches a screen reader too — the R1355 parity
    /// the line chart established, now for the bar chart.
    #[must_use]
    pub fn inspect_readout(&self, rect: Rect, style: &ChartStyle) -> Option<String> {
        let g = self.geom(rect, style);
        let idx = self.resolve_focus(&g, rect)?;
        let bar = &self.bars[idx];
        Some(format!("{} = {}", bar.label, value_text(bar, g.y_step)))
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

/// The plot geometry, value scale, y-tick set, and slot metrics a bar chart
/// body derives everything from (R1375). One resolve, shared by the painted
/// bars, the gridlines, and the inspect hit-test.
struct BarGeom {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    y: LinearScale,
    y_ticks: Vec<f64>,
    y_step: f64,
    baseline_y: f32,
    /// R1545 — the categorical x-axis. Every slot position in this file used
    /// to be `left + i * slot`, written out three times; it is now one
    /// [`CategoryScale::band`] call, and the axis is a [`ValueScale`] arm the
    /// line and scatter charts can take too.
    x: CategoryScale,
    bar_w: f32,
}

impl BarGeom {
    /// The categorical x-axis as a [`ValueScale`], for the shared axis
    /// furniture (tick set, label format) that takes one.
    fn x_axis(&self) -> ValueScale {
        ValueScale::Category(self.x.clone())
    }
}

/// The resolved inspect overlay: a highlight ring around the focused bar
/// (absent when that bar draws no box, e.g. a non-finite value) and a value
/// tooltip. Split so `build_body` paints the ring over the bars and the tooltip
/// above everything.
struct BarInspect {
    highlight: Option<Scene>,
    tooltip: Vec<Scene>,
}

/// The category indices this geometry draws — the axis's visible window, or
/// nothing when no category is in view (R1545).
fn visible_indices(g: &BarGeom) -> impl Iterator<Item = usize> {
    g.x.visible().into_iter().flat_map(|w| w.lo()..=w.hi())
}

/// A bar's value formatted at the y-axis precision, or `"—"` for a non-finite
/// value (a slot with no drawn bar). Shared by the tooltip and the a11y readout
/// so the two never disagree.
fn value_text(bar: &Bar, y_step: f64) -> String {
    if bar.value.is_finite() {
        format_axis_tick(bar.value, y_step)
    } else {
        "\u{2014}".to_string()
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

    // ---- R1545: the bar chart plots through the category axis --------------

    fn twelve() -> Vec<Bar> {
        [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ]
        .iter()
        .enumerate()
        .map(|(i, m)| Bar::new(*m, 10.0 + index_value(i)))
        .collect()
    }

    /// ★ The window narrows what is DRAWN, not only what is domained — the
    /// bars, their labels and their click surfaces all follow, because all
    /// three come from the one axis. The counterfactual is the un-windowed
    /// chart in the same test: it emits all twelve.
    #[test]
    fn r1545_a_category_window_narrows_the_bars_their_labels_and_their_hits() {
        let rect = Rect::new(0, 0, 600, 300);
        let style = ChartStyle::default();
        let hits: Vec<String> = (0..12).map(|i| format!("pick.{i}")).collect();
        let full = BarChart::new(twelve()).selectable(hits.clone());
        let all = tags(&full.build(rect, &style));
        for i in 0..12 {
            assert!(
                all.contains(&format!("chart.bar.{i}")),
                "bar {i} unwindowed"
            );
            assert!(all.contains(&format!("pick.{i}")), "hit {i} unwindowed");
        }

        let chart = BarChart::new(twelve()).selectable(hits);
        let window = chart.categories().window("Mar", "Jun").expect("named");
        let scene = chart.x_window(window).build(rect, &style);
        let t = tags(&scene);
        for i in 2..=5 {
            assert!(t.contains(&format!("chart.bar.{i}")), "bar {i} in window");
            assert!(t.contains(&format!("chart.xlabel.{i}")), "label {i}");
            assert!(t.contains(&format!("pick.{i}")), "hit region {i}");
        }
        for i in [0, 1, 6, 11] {
            assert!(
                !t.contains(&format!("chart.bar.{i}")),
                "bar {i} is windowed out"
            );
            assert!(!t.contains(&format!("chart.xlabel.{i}")));
            assert!(!t.contains(&format!("pick.{i}")));
        }
    }

    /// ★ A windowed bar is WIDER, because the axis band widened. This is the
    /// point of the feature — twelve bars in 600px give each 50px of gutter-
    /// less width; four give each 150.
    #[test]
    fn r1545_a_window_widens_the_bars_it_keeps() {
        let rect = Rect::new(0, 0, 600, 300);
        let style = ChartStyle::default();
        let width = |chart: &BarChart, i: usize| {
            let scene = chart.build(rect, &style);
            let Scene::Box(b) = find(&scene, &format!("chart.bar.{i}")).unwrap() else {
                panic!("bar is a box")
            };
            b.rect.w
        };
        let full = BarChart::new(twelve());
        let windowed = BarChart::new(twelve()).x_window(CategoryWindow::new(2, 5));
        let (wide, narrow) = (width(&windowed, 3), width(&full, 3));
        assert!(
            wide > narrow * 2,
            "3 of 12 -> 4 of 12 visible should widen a lot: {narrow} -> {wide}"
        );
        // And the visible window is reportable — what Qt's axis cannot answer.
        assert_eq!(
            windowed.visible_categories(rect, &style),
            Some(CategoryWindow::new(2, 5))
        );
        assert_eq!(
            full.visible_categories(rect, &style),
            Some(CategoryWindow::new(0, 11))
        );
        assert_eq!(
            BarChart::new(Vec::new()).visible_categories(rect, &style),
            None
        );
    }

    /// ★ The label under a slot is the string the AXIS carries for it, not a
    /// second read of `bar.label` — so a window cannot shift the labels off
    /// their bars. Checked by index, which is where an off-by-one would show.
    #[test]
    fn r1545_the_slot_label_is_the_one_the_axis_carries() {
        let rect = Rect::new(0, 0, 600, 300);
        let style = ChartStyle::default();
        let chart = BarChart::new(twelve()).x_window(CategoryWindow::new(2, 5));
        let scene = chart.build(rect, &style);
        for (i, name) in [(2, "Mar"), (3, "Apr"), (4, "May"), (5, "Jun")] {
            assert_eq!(
                text_of(&scene, &format!("chart.xlabel.{i}")),
                Some(name),
                "slot {i} is labelled by its own category"
            );
        }
        assert_eq!(chart.categories().at(5), Some("Jun"));
    }

    /// ★ The inspect hit-test follows the window: the same cursor fraction
    /// resolves to a different category once the axis is windowed, which a
    /// `floor(fraction * bar_count)` hit-test could not do.
    #[test]
    fn r1545_the_inspect_hit_test_follows_the_window() {
        let rect = Rect::new(0, 0, 600, 300);
        let style = ChartStyle::default();
        let header = |chart: BarChart| {
            let scene = chart.inspect(Some(0.5)).build(rect, &style);
            text_of(&scene, "chart.inspect.header").map(str::to_string)
        };
        let full = header(BarChart::new(twelve()));
        let windowed = header(BarChart::new(twelve()).x_window(CategoryWindow::new(2, 5)));
        assert_eq!(full.as_deref(), Some("Jun"), "mid-axis of all twelve");
        assert_eq!(windowed.as_deref(), Some("Apr"), "mid-axis of Mar..Jun");
        assert_ne!(full, windowed);
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

    fn text_of<'a>(scene: &'a Scene, tag: &str) -> Option<&'a str> {
        match find(scene, tag)? {
            Scene::Text(t) => Some(t.content.as_str()),
            _ => None,
        }
    }

    #[test]
    fn no_inspect_overlay_by_default() {
        let scene = BarChart::new(three()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "chart.inspect.highlight").is_none());
        assert!(find(&scene, "chart.inspect.tooltip").is_none());
        assert!(find(&scene, "chart.inspect.header").is_none());
        assert!(find(&scene, "chart.inspect.value").is_none());
    }

    #[test]
    fn inspect_emits_a_highlight_ring_and_a_value_tooltip() {
        let scene = BarChart::new(three())
            .inspect(Some(0.5))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "chart.inspect.highlight").is_some());
        assert!(find(&scene, "chart.inspect.tooltip").is_some());
        assert!(find(&scene, "chart.inspect.header").is_some());
        assert!(find(&scene, "chart.inspect.value").is_some());
    }

    #[test]
    fn inspect_hit_test_selects_the_slot_under_the_cursor() {
        // Three evenly-spaced slots: the fraction across the chart maps to the
        // categorical slot, so the tooltip header names the bar under the
        // cursor (left -> a, middle -> b, right -> c).
        for (fraction, label) in [(0.0_f32, "a"), (0.5, "b"), (1.0, "c")] {
            let scene = BarChart::new(three())
                .inspect(Some(fraction))
                .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
            assert_eq!(
                text_of(&scene, "chart.inspect.header"),
                Some(label),
                "fraction {fraction} focuses bar {label}"
            );
        }
    }

    #[test]
    fn inspect_highlight_frames_exactly_the_focused_bar() {
        // The geom SSOT: the highlight ring's rect must equal the focused bar's
        // own box rect, so the ring can never drift off its bar.
        let scene = BarChart::new(three())
            .inspect(Some(0.5)) // -> bar b (index 1)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let Scene::Box(ring) = find(&scene, "chart.inspect.highlight").expect("highlight") else {
            panic!("highlight is a box")
        };
        let Scene::Box(bar) = find(&scene, "chart.bar.1").expect("bar b") else {
            panic!("bar is a box")
        };
        assert_eq!(
            ring.rect, bar.rect,
            "the highlight ring frames exactly the focused bar's rect"
        );
    }

    #[test]
    fn inspect_readout_names_the_focused_bar_and_its_value() {
        let chart = BarChart::new(three()).inspect(Some(0.5)); // -> bar b, value 40
        let readout = chart
            .inspect_readout(Rect::new(0, 0, 400, 300), &ChartStyle::default())
            .expect("a readout when inspecting");
        assert!(
            readout.starts_with("b = "),
            "readout names the focused label: {readout:?}"
        );
        assert!(
            readout.contains("40"),
            "readout carries the value: {readout:?}"
        );
        // And the painted tooltip must agree with the readout (R1355 parity).
        assert_eq!(
            text_of(
                &chart.build(Rect::new(0, 0, 400, 300), &ChartStyle::default()),
                "chart.inspect.header"
            ),
            Some("b")
        );
        assert!(
            BarChart::new(three())
                .inspect(None)
                .inspect_readout(Rect::new(0, 0, 400, 300), &ChartStyle::default())
                .is_none(),
            "no readout when inspection is off"
        );
    }

    #[test]
    fn inspect_a_non_finite_bar_shows_no_ring_but_a_dashed_value() {
        // A NaN bar draws no box, so hovering it must emit NO highlight ring —
        // but the tooltip still names the category with an em-dash value, so
        // the reader learns the slot is empty rather than getting nothing.
        let bars = vec![Bar::new("ok", 10.0), Bar::new("gap", f64::NAN)];
        let scene = BarChart::new(bars)
            .inspect(Some(1.0)) // rightmost slot -> the NaN bar (index 1)
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(text_of(&scene, "chart.inspect.header"), Some("gap"));
        assert_eq!(
            text_of(&scene, "chart.inspect.value"),
            Some("\u{2014}"),
            "a non-finite bar's value reads as an em-dash"
        );
        assert!(
            find(&scene, "chart.inspect.highlight").is_none(),
            "no ring for a bar that draws no box"
        );
    }

    #[test]
    fn inspect_tooltip_flips_left_at_the_right_edge_and_stays_in_the_chart() {
        // Hovering the LAST bar would push a right-placed tooltip off the plot;
        // it must flip left and stay inside the chart width.
        const W: u32 = 400;
        let scene = BarChart::new(three())
            .inspect(Some(1.0))
            .build(Rect::new(0, 0, W, 300), &ChartStyle::default());
        let Scene::Box(tip) = find(&scene, "chart.inspect.tooltip").expect("tooltip") else {
            panic!("tooltip is a box")
        };
        assert!(
            tip.rect.x + tip.rect.w <= W,
            "the tooltip stays within the chart ({}+{} <= {W})",
            tip.rect.x,
            tip.rect.w
        );
    }

    // ── R1384 cross-filter: select() mutes, selectable() adds click surfaces ──

    fn fill_of(scene: &Scene, i: usize) -> pinion_core::style::Color {
        let Scene::Box(b) = find(scene, &format!("chart.bar.{i}")).expect("bar box") else {
            panic!("bar is a box")
        };
        b.style.fill
    }

    #[test]
    fn no_selection_leaves_every_bar_full() {
        let rect = Rect::new(0, 0, 400, 300);
        let full = BarChart::new(three()).build(rect, &ChartStyle::default());
        // Both an all-false mask AND an empty mask are "no filter": no mute
        // (the crossfilter convention that an empty selection = all data).
        let all_false = BarChart::new(three())
            .select(vec![false, false, false])
            .build(rect, &ChartStyle::default());
        let empty = BarChart::new(three())
            .select(vec![])
            .build(rect, &ChartStyle::default());
        for i in 0..3 {
            assert_eq!(
                fill_of(&all_false, i),
                fill_of(&full, i),
                "an all-false mask mutes nothing"
            );
            assert_eq!(
                fill_of(&empty, i),
                fill_of(&full, i),
                "an empty mask mutes nothing"
            );
        }
    }

    #[test]
    fn select_mutes_the_bars_outside_the_active_set() {
        let rect = Rect::new(0, 0, 400, 300);
        let full = BarChart::new(three()).build(rect, &ChartStyle::default());
        // Select ONLY category b (index 1): a and c mute, b stays full.
        let sel = BarChart::new(three())
            .select(vec![false, true, false])
            .build(rect, &ChartStyle::default());
        assert_eq!(
            fill_of(&sel, 1),
            fill_of(&full, 1),
            "the selected bar keeps its full colour"
        );
        assert_ne!(
            fill_of(&sel, 0),
            fill_of(&full, 0),
            "an unselected bar is muted"
        );
        assert_ne!(
            fill_of(&sel, 2),
            fill_of(&full, 2),
            "an unselected bar is muted"
        );
        // The mute is EXACTLY the full colour at MUTED_ALPHA (a dimmed alpha,
        // not some other tint) — so the category stays recognisable.
        assert_eq!(
            fill_of(&sel, 0),
            fill_of(&full, 0).with_alpha(MUTED_ALPHA),
            "a muted bar is its own colour dimmed to MUTED_ALPHA"
        );
    }

    #[test]
    fn selectable_makes_each_bar_a_focusable_tagged_hit_region() {
        let tags = vec![
            "cat_0".to_string(),
            "cat_1".to_string(),
            "cat_2".to_string(),
        ];
        let scene = BarChart::new(three())
            .selectable(tags.clone())
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        for tag in &tags {
            let Some(Scene::Container(hit)) = find(&scene, tag) else {
                panic!("hit region {tag} is a focusable container")
            };
            assert!(hit.layout.focusable, "region {tag} is a Tab / click target");
        }
        // And NONE of those tags exists when the chart is not made selectable.
        let plain = BarChart::new(three()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        for tag in &tags {
            assert!(
                find(&plain, tag).is_none(),
                "no hit region without selectable()"
            );
        }
    }

    #[test]
    fn selectable_tags_only_as_many_bars_as_tags_given() {
        // Two tags for three bars -> only the first two columns are clickable.
        let scene = BarChart::new(three())
            .selectable(vec!["c0".to_string(), "c1".to_string()])
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "c0").is_some());
        assert!(find(&scene, "c1").is_some());
        assert!(
            find(&scene, "c2").is_none(),
            "the untagged third bar gets no hit region"
        );
    }

    #[test]
    fn selectable_ignores_tags_past_the_last_bar() {
        // Four tags for three bars -> the phantom fourth tag emits no region.
        let scene = BarChart::new(three())
            .selectable(vec![
                "c0".to_string(),
                "c1".to_string(),
                "c2".to_string(),
                "c3".to_string(),
            ])
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "c2").is_some());
        assert!(
            find(&scene, "c3").is_none(),
            "no phantom hit region past the last bar"
        );
    }

    // ── R1395 numeric cross-filter: select_x_range mutes out-of-window bins ──

    /// Three histogram bins tiling `[0, 3)` uniformly (`[0,1)`, `[1,2)`, `[2,3)`),
    /// so a brush window over the numeric x-axis can select a sub-range of them.
    fn histogram() -> Vec<Bar> {
        vec![
            Bar::new("0", 10.0).with_bin(0.0, 1.0),
            Bar::new("1", 40.0).with_bin(1.0, 2.0),
            Bar::new("2", 20.0).with_bin(2.0, 3.0),
        ]
    }

    #[test]
    fn select_x_range_mutes_bins_outside_the_window_and_keeps_them_drawn() {
        let rect = Rect::new(0, 0, 400, 300);
        let full = BarChart::new(histogram()).build(rect, &ChartStyle::default());
        // Brush x in [1, 2] selects the middle bin ([1,2)); the outer bins
        // ([0,1) and [2,3)) do not overlap it, so they mute.
        let sel = BarChart::new(histogram())
            .select_x_range(Some((1.0, 2.0)))
            .build(rect, &ChartStyle::default());
        // Muting DIMS, it does not DROP: every bin still emits a box.
        for i in 0..3 {
            assert!(
                find(&sel, &format!("chart.bar.{i}")).is_some(),
                "bin {i} stays drawn (muted, not dropped)"
            );
        }
        assert_eq!(
            fill_of(&sel, 1),
            fill_of(&full, 1),
            "the in-window bin keeps its full colour"
        );
        assert_eq!(
            fill_of(&sel, 0),
            fill_of(&full, 0).with_alpha(MUTED_ALPHA),
            "a bin below the window is its own colour dimmed to MUTED_ALPHA"
        );
        assert_eq!(
            fill_of(&sel, 2),
            fill_of(&full, 2).with_alpha(MUTED_ALPHA),
            "a bin above the window mutes too"
        );
    }

    #[test]
    fn select_x_range_none_leaves_every_bar_full() {
        let rect = Rect::new(0, 0, 400, 300);
        let full = BarChart::new(histogram()).build(rect, &ChartStyle::default());
        let none = BarChart::new(histogram())
            .select_x_range(None)
            .build(rect, &ChartStyle::default());
        for i in 0..3 {
            assert_eq!(
                fill_of(&none, i),
                fill_of(&full, i),
                "no range = bin {i} full"
            );
        }
    }

    #[test]
    fn a_full_span_window_mutes_nothing() {
        // A brush covering the whole extent [0, 3] leaves every bin in the
        // window — the natural no-op a full brush must be (the scatter symmetry).
        let rect = Rect::new(0, 0, 400, 300);
        let full = BarChart::new(histogram()).build(rect, &ChartStyle::default());
        let spanned = BarChart::new(histogram())
            .select_x_range(Some((0.0, 3.0)))
            .build(rect, &ChartStyle::default());
        for i in 0..3 {
            assert_eq!(
                fill_of(&spanned, i),
                fill_of(&full, i),
                "bin {i} inside the full span stays full"
            );
        }
    }

    #[test]
    fn a_bar_without_a_bin_is_never_x_muted() {
        // A categorical bar (no `with_bin`) has no numeric position, so a brush
        // window cannot exclude it — it stays full regardless of the range.
        let rect = Rect::new(0, 0, 400, 300);
        let full = BarChart::new(three()).build(rect, &ChartStyle::default());
        let sel = BarChart::new(three())
            .select_x_range(Some((100.0, 200.0))) // a window disjoint from any data
            .build(rect, &ChartStyle::default());
        for i in 0..3 {
            assert_eq!(
                fill_of(&sel, i),
                fill_of(&full, i),
                "binless bar {i} is untouched by a numeric brush"
            );
        }
    }

    #[test]
    fn select_x_range_overlap_is_half_open_at_the_window_edges() {
        // Window [1, 2]: the middle bin [1,2) overlaps (in); [0,1)'s right edge
        // touches lo (bhi==lo -> out); [2,3)'s left edge touches hi (blo==hi ->
        // out). A bin only survives if it genuinely overlaps the window.
        let rect = Rect::new(0, 0, 400, 300);
        let full = BarChart::new(histogram()).build(rect, &ChartStyle::default());
        let sel = BarChart::new(histogram())
            .select_x_range(Some((1.0, 2.0)))
            .build(rect, &ChartStyle::default());
        assert_ne!(
            fill_of(&sel, 0),
            fill_of(&full, 0),
            "[0,1) ends at lo -> out"
        );
        assert_eq!(fill_of(&sel, 1), fill_of(&full, 1), "[1,2) overlaps -> in");
        assert_ne!(
            fill_of(&sel, 2),
            fill_of(&full, 2),
            "[2,3) starts at hi -> out"
        );
    }

    #[test]
    fn a_partial_overlap_bin_stays_in_the_window() {
        // A window [1.5, 2.5] straddles two bins: [1,2) (1<2.5 && 2>1.5) and
        // [2,3) (2<2.5 && 3>1.5) both overlap, so both stay full; [0,1) does not.
        let rect = Rect::new(0, 0, 400, 300);
        let full = BarChart::new(histogram()).build(rect, &ChartStyle::default());
        let sel = BarChart::new(histogram())
            .select_x_range(Some((1.5, 2.5)))
            .build(rect, &ChartStyle::default());
        assert_ne!(
            fill_of(&sel, 0),
            fill_of(&full, 0),
            "[0,1) disjoint -> muted"
        );
        assert_eq!(
            fill_of(&sel, 1),
            fill_of(&full, 1),
            "[1,2) partial overlap -> in"
        );
        assert_eq!(
            fill_of(&sel, 2),
            fill_of(&full, 2),
            "[2,3) partial overlap -> in"
        );
    }

    #[test]
    fn categorical_and_numeric_cross_filters_each_dim_a_bar() {
        // The two filters are orthogonal: a bar is dimmed if EITHER excludes it.
        // Categorical select picks bin 0; the brush window [1,2] picks bin 1.
        // Bin 2 is excluded by both, bin 0 kept by select but dropped by the
        // window, bin 1 dropped by select but kept by the window -> only a bar
        // in BOTH active sets stays full, and here that is none, so every bar
        // that is out of EITHER dims. Concretely: bin 0 (select=in, window=out)
        // and bin 1 (select=out, window=in) both mute.
        let rect = Rect::new(0, 0, 400, 300);
        let full = BarChart::new(histogram()).build(rect, &ChartStyle::default());
        let both = BarChart::new(histogram())
            .select(vec![true, false, false]) // categorical: only bin 0 active
            .select_x_range(Some((1.0, 2.0))) // numeric: only bin 1 in window
            .build(rect, &ChartStyle::default());
        assert_ne!(
            fill_of(&both, 0),
            fill_of(&full, 0),
            "bin 0 is in the categorical set but OUT of the window -> muted"
        );
        assert_ne!(
            fill_of(&both, 1),
            fill_of(&full, 1),
            "bin 1 is in the window but OUT of the categorical set -> muted"
        );
        assert_ne!(
            fill_of(&both, 2),
            fill_of(&full, 2),
            "bin 2 out of both -> muted"
        );
    }
}
