//! R1567 — the candlestick chart, and the round's second claim: **one datum,
//! two readings of the x-axis**.
//!
//! [`Candle`] carries the session's own instant, so what the x-axis *is* — a
//! row of equal slots or a stretch of real UTC time — is a property of the
//! chart, not of the data. [`SessionAxis`] switches between them and nothing
//! else changes.
//!
//! # Why that is the interesting part
//!
//! Financial charts have used both readings for a century, for a reason a
//! reader meets immediately: markets are closed most of the time. Drawn on
//! real time, a week of daily sessions is five candles and two days of blank
//! paper, and an intraday chart is 6.5 hours of marks in 24 hours of frame.
//! Drawn on ordinal slots, the sessions abut and the weekend is invisible —
//! which is right for reading price action and wrong for anything that cares
//! how long a move took.
//!
//! The toolkit exposes both — candlestick series attaches to a date time axis
//! or a bar category axis — and the choice costs something there that it does
//! not cost here. A bar category axis is constructed from a string list the
//! caller supplies **separately** from the sets' `timestamp` values, so a toolkit
//! candlestick chart holds two descriptions of when its sessions were, in two
//! objects, with nothing checking that they agree or even that they are the
//! same length. Here the slot labels are *derived* from the instants by
//! R1529's own UTC formatter, and the sessions are sorted by instant once at
//! construction, so the two readings cannot disagree about their order or
//! their names.
//!
//! # What else is past the toolkit 6.11
//!
//! * **The direction survives the loss of hue.** the toolkit encodes it in
//!   `increasingColor` / `decreasingColor` alone. Here it is *also* the body's
//!   [`BodyFill`](crate::BodyFill) — hollow for a rise, solid for a fall, the
//!   traditional Japanese form, which predates colour — and
//!   [`direction_contrast`](CandlestickChart::direction_contrast) publishes
//!   the WCAG ratio between the two hues so "these are distinguishable" is an
//!   assertion rather than a hope.
//! * **A value the axis cannot place is REPORTED.** A logarithmic price axis
//!   is the one a long history actually wants, and a slot at or below zero has
//!   no pixel on it; [`off_scale`](CandlestickChart::off_scale) names each,
//!   the way R1528 named an off-scale point and R1553 an off-scale landmark.
//! * **The window is one declaration read two ways.** A [`CategoryWindow`]
//!   narrows the ordinal slots *and* the elapsed time domain, from the same
//!   index pair.
//!
//! # Introspection
//!
//! Under `tag_prefix` (default `"chart"`), per candle `i`: `chart.candle.{i}` (the body), `chart.wick.{i}.hi` / `.lo`, and —
//! when [`with_caps`](CandlestickChart::with_caps) is on, the toolkit's `capsVisible` — `chart.cap.{i}.hi` /
//! `.lo`. The shared cartesian furniture keeps its usual tags. The x labels
//! differ **by reading**, and deliberately: the ordinal reading emits one per
//! slot (`chart.xlabel.{i}`, the bar chart's cardinality) and the elapsed reading one per
//! tick (`chart.label.x.{k}`, the line chart's), because those are two different questions
//! and collapsing them would make the tag lie about what it counts.
//!
//! # Coordinate contract
//!
//! Identical to every other builder here: [`build_fill`](CandlestickChart::build_fill)
//! is layout-native, [`build`](CandlestickChart::build) pins to a caller rect.
//! Read the crate-level "Known limitations" — `Scene::Path` does not render on
//! TUI, and a candlestick chart is almost entirely paths.

use pinion_a11y::chart::{ChartCell, ChartColumn, ChartRow, ChartTable};
use pinion_core::Scene;
use pinion_core::contrast::contrast_ratio;
use pinion_core::derivation::{Derivation, DerivationKind, DerivationSet, Evidence};
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{Color, PathStyle, Stroke};

use crate::candle::{Candle, CandlePosition, Direction, candle_bounds, positive_candle_bounds};
use crate::derivations;
use crate::draw::{
    CalloutRow, absolute, box_node, callout, category_label_node, fill_parent, outline_box,
    plot_rect, polygon_node, stroke_path, to_f32, to_u32, x_tick_labels,
};
use crate::fit::Fitted;
use crate::legend::{ChartLegend, Legend};
use crate::plot::{
    axis_domain, axis_format, axis_minor_ticks, axis_scale, axis_ticks, kind_extent, tick_pixels,
};
use crate::scale::{
    AxisKind, Categories, CategoryScale, CategoryWindow, DEFAULT_LOG_BASE, ValueScale,
};
use crate::style::ChartStyle;
use crate::ticks::format_si;
use crate::ticks::{TickFormat, format_time_tick, tick_step};

/// The fraction of a session's pitch the body occupies. The toolkit's
/// `bodyWidth` defaults to `0.5`; a little wider reads
/// better against this crate's gap-free bands, and matches
/// [`BoxPlotChart`](crate::BoxPlotChart)'s box so the two interval forms sit
/// at the same weight in one dashboard.
const BODY_WIDTH_FRAC: f32 = 0.6;

/// The cap's width as a fraction of the body's — the toolkit's `capsWidth` default
/// relative to `bodyWidth`, and the same subordinate proportion the box plot's whisker
/// caps use.
const CAP_WIDTH_FRAC: f32 = 0.5;

/// How many pitches wide the plot is assumed to be when a single session
/// leaves no gap to measure one from. Twenty is a legible lone candle, and
/// the case is degenerate either way — the alternative is a body spanning the
/// whole plot.
const LONE_SESSION_PITCHES: f32 = 20.0;

/// Which reading of the x-axis a candlestick chart is drawn on.
///
/// The two are the toolkit's bar category axis and date time axis, reached
/// there by attaching different axis objects and supplying the category names
/// as a second data source. Here they are one declaration over one datum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionAxis {
    /// Each session is one equal slot, whatever real time separates it from
    /// its neighbour (d3's `scaleBand`, the toolkit's bar category axis).
    ///
    /// The default, because it is what a price chart is read on: the gap a
    /// weekend or a halt leaves is not information about the price, and
    /// drawing it spends pixels on paper the market was shut.
    #[default]
    Ordinal,
    /// Real UTC time (R1529's [`AxisKind::Time`], the toolkit's date time axis).
    ///
    /// Every non-trading interval is visible as a gap, which is what a
    /// reader asking *how long* a move took needs and what a reader of price
    /// action does not.
    Elapsed,
}

impl SessionAxis {
    /// This reading's name, for a readout or a wire field.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ordinal => "ordinal",
            Self::Elapsed => "elapsed",
        }
    }
}

/// R1624 — which **mark** a session is drawn as.
///
/// A candlestick and an open-high-low-close bar are two renderings of one
/// datum, not two datasets, so this is a property of the chart and everything
/// else — the sort, both [`SessionAxis`] readings, the log value axis and its
/// [`off_scale`](CandlestickChart::off_scale) report, the
/// [`window`](CandlestickChart::window), the inspect readout, the direction
/// colours and their published contrast — is shared unchanged. The reference
/// toolkit has a candlestick series and no bar one at all, so there is nothing
/// there to be a second series *of*.
///
/// # Why the choice is not cosmetic
///
/// The two marks disagree about what the session's ANCHOR is, and that is
/// visible whenever the value axis cannot place part of a session. A candle
/// hangs its wicks off its body, so a body the axis cannot place takes the
/// whole session with it. A bar has **no single anchor** — spine, open tick
/// and close tick are each placed from the prices they need — so it draws
/// whatever the axis can carry. Neither is a repair of the other; they are
/// the marks' own definitions, and
/// [`off_scale`](CandlestickChart::off_scale) names what went missing either
/// way.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum SessionMark {
    /// The Japanese candlestick: a body between open and close with a wick to
    /// each extreme. The default, and what R1567 built.
    #[default]
    Candle,
    /// The Western open-high-low-close bar: a vertical spine over the whole
    /// range, an **open** tick to its left and a **close** tick to its right.
    ///
    /// Direction is legible here without any colour at all — the close tick
    /// sits above the open tick on a rise and below it on a fall — which is
    /// the same redundancy [`BodyFill`](crate::BodyFill) gives the candle, in
    /// the form this mark has available.
    Ohlc,
}

impl SessionMark {
    /// Every mark, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 2] = [Self::Candle, Self::Ohlc];

    /// The tag stem this mark's primary node carries.
    #[must_use]
    pub const fn tag_stem(self) -> &'static str {
        match self {
            Self::Candle => "candle",
            Self::Ohlc => "ohlc",
        }
    }
}

/// One value a candlestick chart's value axis cannot place (R1567) — the
/// [`OffScale`](crate::OffScale) of a session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffScaleCandle {
    /// Index of the session, in the chart's own (instant-ascending) order.
    pub candle: usize,
    /// Which of its four slots this is.
    pub at: CandlePosition,
    /// The value the axis could not place.
    pub value: f64,
}

/// A candlestick chart over instant-stamped [`Candle`]s.
pub struct CandlestickChart {
    /// Sorted by instant at construction — see [`new`](Self::new).
    candles: Vec<Candle>,
    /// The ordinal reading's slot names, derived from the instants once.
    /// Held rather than rebuilt per frame for the reason [`crate::bar`] holds
    /// its own: rebuilding clones `n` heap strings on every paint.
    sessions: Categories,
    reading: SessionAxis,
    mark: SessionMark,
    y_domain: Option<(f64, f64)>,
    y_kind: AxisKind,
    window: Option<CategoryWindow>,
    inspect: Option<f32>,
    rising: Color,
    falling: Color,
    doji: Color,
    caps: bool,
    tag_prefix: String,
}

impl CandlestickChart {
    /// A candlestick chart over `candles`, on the ordinal reading, with an
    /// auto linear value axis, no caps, no inspect overlay, every session in
    /// view, and the `"chart"` tag prefix.
    ///
    /// **The sessions are sorted by instant**, stably. That is not tidying:
    /// both readings are derived from this one order, so sorting here is what
    /// makes them agree by construction. `append` does not
    /// sort, and its two axes disagree when the caller's insertion order does
    /// not match its timestamps — the category axis draws insertion order
    /// while the datetime axis draws time order, from the same sets.
    #[must_use]
    pub fn new(candles: Vec<Candle>) -> Self {
        let mut candles = candles;
        // `sort_by` rather than `sort_unstable_by`: two sessions may share an
        // instant (a bar and its revision), and a caller's order between them
        // is the only thing left to break the tie.
        candles.sort_by(|a, b| a.instant().total_cmp(&b.instant()));
        let sessions = Categories::new(candles.iter().map(|c| format_time_tick(c.instant())));
        Self {
            candles,
            sessions,
            reading: SessionAxis::default(),
            mark: SessionMark::default(),
            y_domain: None,
            y_kind: AxisKind::Linear,
            window: None,
            inspect: None,
            rising: DEFAULT_RISING,
            falling: DEFAULT_FALLING,
            doji: DEFAULT_DOJI,
            caps: false,
            tag_prefix: "chart".to_string(),
        }
    }

    /// The sessions this chart holds, in instant order.
    #[must_use]
    pub fn candles(&self) -> &[Candle] {
        &self.candles
    }

    /// The ordinal reading's slot names — derived from the instants, and the
    /// entry point for resolving a [`window`](Self::window) by name.
    #[must_use]
    pub const fn sessions(&self) -> &Categories {
        &self.sessions
    }

    /// Which reading of the x-axis this chart is on.
    #[must_use]
    pub const fn reading(&self) -> SessionAxis {
        self.reading
    }

    /// Which mark the sessions are drawn as. See [`SessionMark`].
    #[must_use]
    pub const fn mark(&self) -> SessionMark {
        self.mark
    }

    /// Draw the sessions as `mark` instead of the default candlestick.
    ///
    /// Everything else is untouched — this is a rendering of the same data,
    /// so the sort, both axis readings, the value axis, the window and the
    /// inspect readout are shared rather than reimplemented.
    #[must_use]
    pub const fn with_mark(mut self, mark: SessionMark) -> Self {
        self.mark = mark;
        self
    }

    /// Draw the x-axis as real UTC time (the toolkit's date time axis) instead
    /// of equal slots. See [`SessionAxis`].
    #[must_use]
    pub const fn elapsed(mut self) -> Self {
        self.reading = SessionAxis::Elapsed;
        self
    }

    /// Draw the x-axis as equal ordinal slots (the toolkit's bar category
    /// axis) — the default, stated explicitly.
    #[must_use]
    pub const fn ordinal(mut self) -> Self {
        self.reading = SessionAxis::Ordinal;
        self
    }

    /// Show only `window`'s stretch of sessions.
    ///
    /// **One declaration, both readings.** On the ordinal reading it narrows
    /// the category domain; on the elapsed reading it narrows the *time*
    /// domain to the instants those indices carry. The toolkit needs two calls
    /// against two axis objects, in two vocabularies (`setRange` takes category
    /// *strings*, `setRange` takes date times), and nothing relates them.
    #[must_use]
    pub const fn window(mut self, window: CategoryWindow) -> Self {
        self.window = Some(window);
        self
    }

    /// R1634 §5.40 — this chart's data as an accessible **table**: one row per
    /// session, one column per OHLC slot.
    ///
    /// The same shape the box plot takes and for the same reason: a column is
    /// what the chart carries **per point**, which here is the four prices a
    /// session IS. A reader crossing a row hears open, high, low and close for
    /// one session — the comparison a candle is drawn to make, and one a single
    /// summary string gives no way to step through.
    ///
    /// The session names come from the chart's own slot names, so a windowed
    /// chart's rows are the sessions it drew and
    /// [`ChartTable::set_size`] states how many there are altogether.
    #[must_use]
    pub fn access_table(&self, name: &str, axis_name: &str) -> ChartTable {
        let prefix = &self.tag_prefix;
        let visible: Vec<usize> = self.window.as_ref().map_or_else(
            || (0..self.candles.len()).collect(),
            |w| {
                (w.lo()..=w.hi())
                    .filter(|i| *i < self.candles.len())
                    .collect()
            },
        );
        ChartTable {
            tag: prefix.clone(),
            name: name.to_owned(),
            axis_name: axis_name.to_owned(),
            columns: Candle::positions()
                .into_iter()
                .map(|at| ChartColumn {
                    tag: format!("{prefix}.a11y.{}", at.name()),
                    name: at.name().to_owned(),
                })
                .collect(),
            rows: visible
                .iter()
                .map(|&i| ChartRow {
                    tag: format!("{prefix}.a11y.r{i}"),
                    name: self.sessions.at(i).unwrap_or_default().to_owned(),
                    cells: Candle::positions()
                        .into_iter()
                        .map(|at| ChartCell {
                            // Open and close are the body's two edges; the
                            // extremes are the wicks' ends, each drawn as its
                            // own stroke, so all four have a mark to point at.
                            tag: Some(match at {
                                CandlePosition::High => format!("{prefix}.wick.{i}.hi"),
                                CandlePosition::Low => format!("{prefix}.wick.{i}.lo"),
                                CandlePosition::Open | CandlePosition::Close => {
                                    format!("{prefix}.candle.{i}")
                                }
                            }),
                            value: format_si(self.candles[i].at(at)),
                        })
                        .collect(),
                })
                .collect(),
            set_size: (visible.len() < self.candles.len()).then_some(self.candles.len()),
        }
    }

    /// Pin the value-axis domain instead of deriving it from the data.
    #[must_use]
    pub fn with_y_domain(mut self, lo: f64, hi: f64) -> Self {
        self.y_domain = Some((lo, hi));
        self
    }

    /// Make the value axis logarithmic at the default base 10 (R1528's
    /// [`LogScale`](crate::LogScale)).
    ///
    /// The axis a long price history wants: equal *ratios* over equal pixel
    /// spans, so a 10% move reads the same at 20 and at 200. The auto-domain
    /// then measures only the strictly positive slots; one at or below zero
    /// has no pixel and is reported by [`off_scale`](Self::off_scale).
    #[must_use]
    pub fn y_log(self) -> Self {
        self.y_log_base(DEFAULT_LOG_BASE)
    }

    /// [`y_log`](Self::y_log) at an explicit base.
    #[must_use]
    pub fn y_log_base(mut self, base: f64) -> Self {
        self.y_kind = AxisKind::Log(base);
        self
    }

    /// Draw the short horizontal caps at each session's high and low (the
    /// toolkit's `capsVisible`, which also defaults to off).
    #[must_use]
    pub const fn with_caps(mut self, caps: bool) -> Self {
        self.caps = caps;
        self
    }

    /// Override the three direction colours (the toolkit has the first two as
    /// `increasingColor` / `decreasingColor`; the third has a toolkit equivalent because a doji has
    /// a toolkit name).
    #[must_use]
    pub const fn with_direction_colors(
        mut self,
        rising: Color,
        falling: Color,
        doji: Color,
    ) -> Self {
        self.rising = rising;
        self.falling = falling;
        self.doji = doji;
        self
    }

    /// The colour a session of `direction` is drawn in.
    #[must_use]
    pub const fn direction_color(&self, direction: Direction) -> Color {
        match direction {
            Direction::Rising => self.rising,
            Direction::Falling => self.falling,
            Direction::Doji => self.doji,
        }
    }

    /// The WCAG contrast ratio between the rising and falling hues, `1.0` to
    /// `21.0`.
    ///
    /// WCAG 2.2 §1.4.11 asks for at least `3.0` between a meaningful graphical
    /// object and the colours adjacent to it, and the two candle bodies are
    /// exactly that pair. Publishing it makes "a reader can tell these apart"
    /// a thing a test can assert; the toolkit will paint any two brushes and
    /// say nothing. Note this is the *hue* half only — the fill half ([`Direction::body_fill`])
    /// is what carries the distinction when hue is gone altogether.
    #[must_use]
    pub fn direction_contrast(&self) -> f32 {
        contrast_ratio(self.rising, self.falling)
    }

    /// Show the inspect overlay at `fraction` — the cursor's position as a
    /// fraction `0.0..=1.0` across the chart `rect` width, exactly as
    /// [`BarChart::inspect`](crate::BarChart::inspect) takes it.
    #[must_use]
    pub const fn inspect(mut self, fraction: Option<f32>) -> Self {
        self.inspect = fraction;
        self
    }

    /// Override the intent/introspection tag prefix (default `"chart"`).
    #[must_use]
    pub fn with_tag_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.tag_prefix = prefix.into();
        self
    }

    /// Every value this chart's value axis cannot place, in session order
    /// then slot order (R1567).
    ///
    /// Depends only on the axis KIND, not on the geometry or the window, so a
    /// consumer can caption the omission without laying the chart out. Empty
    /// on a linear axis — which is the counterfactual worth keeping in mind
    /// when reading a non-empty one.
    #[must_use]
    pub fn off_scale(&self) -> Vec<OffScaleCandle> {
        let mut out = Vec::new();
        for (i, c) in self.candles.iter().enumerate() {
            for at in Candle::positions() {
                let value = c.at(at);
                if !self.y_kind.defines(value) {
                    out.push(OffScaleCandle {
                        candle: i,
                        at,
                        value,
                    });
                }
            }
        }
        out
    }

    /// R1629 §2 #7 — what this drawing did that the drawing cannot give back.
    ///
    /// * [`Chosen`](DerivationKind::Chosen) — the
    ///   [`mark`](Self::with_mark), always. Candle and bar encode the same
    ///   four numbers with different geometry, and which one a reader is
    ///   looking at decides how they read it.
    /// * [`Omitted`](DerivationKind::Omitted) — how many of each session's
    ///   four values the value axis could not place
    ///   ([`off_scale`](Self::off_scale)).
    /// * [`Discarded`](DerivationKind::Discarded) — **caps asked for under a
    ///   mark that has none.** [`with_caps`](Self::with_caps) is read only
    ///   where candle wicks are drawn, so `with_caps(true).with_mark(Ohlc)`
    ///   used to add nothing and say nothing; the bar's open and close ticks
    ///   already play the caps' role, so there is no counterpart to draw and
    ///   the honest answer is to report the combination. The evidence names
    ///   the mark that made the setting meaningless.
    ///
    /// The reference toolkit has no bar mark at all, so it never reaches this
    /// combination — and its meta-object protocol would in any case only read
    /// `capsVisible` back, which says what was asked and not what the drawing
    /// did with it.
    #[must_use]
    pub fn derivations(&self) -> DerivationSet {
        let mut set = DerivationSet::over(derivations::domain::SLOT).stating(
            derivations::chosen_name(derivations::name::MARK, self.mark.tag_stem()),
        );
        if self.caps && self.mark == SessionMark::Ohlc {
            set = set.stating(
                Derivation::new(
                    DerivationKind::Discarded,
                    derivations::name::CAPS,
                    Evidence::Name(self.mark.tag_stem().into()),
                )
                .about(derivations::name::MARK),
            );
        }
        set.stating_all(derivations::omitted_counts(
            derivations::name::OFF_SCALE,
            self.off_scale()
                .into_iter()
                .map(|off| derivations::slot_subject(off.candle)),
        ))
    }

    /// The sessions currently in view in a chart of `rect` under `style`.
    #[must_use]
    pub fn visible_sessions(&self, rect: Rect, style: &ChartStyle) -> Option<CategoryWindow> {
        let g = self.geom(rect, style);
        let (lo, hi) = self.drawn_range(&g)?;
        Some(CategoryWindow::new(lo, hi))
    }

    /// Build the chart PINNED to `rect`. See
    /// [`LineChart::build`](crate::LineChart::build) — same contract; prefer
    /// [`build_fill`](Self::build_fill) for anything layout-placed.
    #[must_use]
    pub fn build(&self, rect: Rect, style: &ChartStyle) -> Scene {
        Scene::Container(
            self.build_body(Rect::new(0, 0, rect.w, rect.h), style)
                .with_layout(absolute(rect)),
        )
    }

    /// Build the chart as a **layout-native** subtree (R1360 contract).
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
    /// entry points wrap.
    fn build_body(&self, rect: Rect, style: &ChartStyle) -> ContainerNode {
        let g = self.geom(rect, style);
        let (highlight, tooltip) = match self.resolve_inspect(&g, rect, style) {
            Some(i) => (i.highlight, i.tooltip),
            None => (None, Vec::new()),
        };

        let mut children: Vec<Scene> = Vec::new();
        if let Some(bg) = style.background {
            children.push(box_node(rect, bg, format!("{}.bg", self.tag_prefix)));
        }

        let frame = (g.left, g.right, g.top, g.bottom);
        let y_pos = tick_pixels(&g.y, g.y_ticks.labelled());
        // A log value axis needs its per-decade subdivisions (R1528); a linear
        // one produces none, so this is one call either way.
        let minor_pos = tick_pixels(&g.y, &axis_minor_ticks(&g.y));
        children.extend(crate::draw::minor_gridlines(
            frame,
            &[],
            &minor_pos,
            style,
            &self.tag_prefix,
        ));
        // The elapsed reading has numeric x-gridlines; the ordinal reading has
        // none, for the bar chart's reason — a slot boundary is not a value.
        let x_pos = tick_pixels(&g.x, g.x_ticks.labelled());
        children.extend(crate::draw::gridlines(
            frame,
            &x_pos,
            &y_pos,
            style,
            &self.tag_prefix,
        ));
        children.extend(crate::draw::axes(frame, style, &self.tag_prefix));

        for i in self.drawn_indices(&g) {
            children.extend(self.marks_for(&g, i, style));
        }
        children.extend(self.x_labels(&g, rect, style));

        if let Some(highlight) = highlight {
            children.push(highlight);
        }
        children.extend(crate::draw::y_tick_labels(
            rect.x,
            g.y_ticks.labelled(),
            &y_pos,
            &g.y_format(),
            style,
            &self.tag_prefix,
        ));
        children.extend(tooltip);

        // R1633 — the label fit is GEOMETRY, and `derivations()` is documented
        // as answering without any. So the fit's reports join the set here,
        // where the pixels are known, rather than making that method depend on
        // a layout pass.
        let fitted = self.derivations().stating_all(
            [("x", &g.x_ticks), ("y", &g.y_ticks)]
                .into_iter()
                .flat_map(|(axis, f)| derivations::fit_reports(axis, f)),
        );
        derivations::chart_root(children, self.tag_prefix.clone(), fitted)
    }

    /// The x-axis labels — one per drawn slot on the ordinal reading, one per
    /// tick on the elapsed one. Two cardinalities because they answer two
    /// questions; see the module doc.
    fn x_labels(&self, g: &CandleGeom, rect: Rect, style: &ChartStyle) -> Vec<Scene> {
        let size = style.label_size_px.max(1);
        let format = axis_format(&g.x, g.x_ticks.ticks());
        match g.x.category() {
            // R1633 — an ordinal session axis is a category axis, so its names
            // thin the same way. Its ticks are not `g.x_ticks` (that field is
            // empty on this arm by construction), so the fit is taken here over
            // the axis itself.
            Some(cat) => {
                crate::fit::labelled_indices(&axis_ticks(&g.x, style.x_ticks, &style.room_x()))
                    .into_iter()
                    .filter(|i| self.drawn_indices(g).contains(i))
                    .map(|i| {
                        category_label_node(
                            cat,
                            i,
                            g.left,
                            g.bottom,
                            &format,
                            style.label,
                            size,
                            &self.tag_prefix,
                        )
                    })
                    .collect()
            }
            None => x_tick_labels(
                &g.x,
                g.x_ticks.labelled(),
                g.bottom,
                rect,
                &format,
                style,
                &self.tag_prefix,
            ),
        }
    }

    /// Every mark of session `i`: the body, the two wicks, and the two caps
    /// when they are shown.
    ///
    /// A session whose body the axis cannot place draws nothing — the body is
    /// the landmark every other mark hangs off — and
    /// [`off_scale`](Self::off_scale) names what is missing. That is the box
    /// plot's rule, applied to the datum whose anchor is its body rather than
    /// its quartiles.
    fn marks_for(&self, g: &CandleGeom, i: usize, style: &ChartStyle) -> Vec<Scene> {
        match self.mark {
            SessionMark::Ohlc => self.ohlc_marks_for(g, i, style),
            _ => self.candle_marks_for(g, i, style),
        }
    }

    /// R1624 — session `i` as an open-high-low-close bar: the spine over the
    /// whole range, an open tick left of it and a close tick right of it.
    ///
    /// **A bar has no single anchor, and that is the difference from the
    /// candle.** Each of its three marks is placed from the prices IT needs —
    /// the spine from both extremes, each tick from its own price — so a bar
    /// draws whatever the value axis can carry and drops only the rest. The
    /// candle cannot work that way: its wicks hang off body edges, so a body
    /// the axis refuses takes the whole session with it.
    ///
    /// That asymmetry is reachable, and the first draft of this function got
    /// it backwards. On a logarithmic axis an unplaceable *open* implies an
    /// unplaceable *low* (the low is at or below the open), so "the spine is
    /// the anchor and the ticks are optional" describes a case that cannot
    /// happen; the case that does happen is the reverse, and a bar that bailed
    /// on its spine would have drawn nothing exactly where it has the most to
    /// say. Nothing is ever clamped onto the plot floor —
    /// [`off_scale`](Self::off_scale) names the price instead, because a mark
    /// at an invented pixel is a price where no price is.
    fn ohlc_marks_for(&self, g: &CandleGeom, i: usize, style: &ChartStyle) -> Vec<Scene> {
        let Some(candle) = self.candles.get(i) else {
            return Vec::new();
        };
        let Some(body) = self.slot_geometry(g, i) else {
            return Vec::new();
        };
        let color = self.direction_color(candle.direction());
        let stroke = Stroke::new(color, style.series_width.max(1));
        let mut out = Vec::new();
        if let (Some(high), Some(low)) = (g.y.map(candle.high()), g.y.map(candle.low())) {
            out.push(stroke_path(
                &[(body.center, high), (body.center, low)],
                stroke,
                format!("{}.ohlc.{i}.range", self.tag_prefix),
            ));
        }
        // Open to the LEFT, close to the RIGHT. That order is the whole of
        // how this mark says which way the session went without using
        // colour: on a rise the right tick is the higher one.
        for (price, from, to, tag) in [
            (candle.open(), body.left, body.center, "open"),
            (candle.close(), body.center, body.right, "close"),
        ] {
            let Some(y) = g.y.map(price) else { continue };
            out.push(stroke_path(
                &[(from, y), (to, y)],
                stroke,
                format!("{}.ohlc.{i}.{tag}", self.tag_prefix),
            ));
        }
        out
    }

    fn candle_marks_for(&self, g: &CandleGeom, i: usize, style: &ChartStyle) -> Vec<Scene> {
        let Some(candle) = self.candles.get(i) else {
            return Vec::new();
        };
        let Some(body) = self.body_geometry(g, i) else {
            return Vec::new();
        };
        let direction = candle.direction();
        let color = self.direction_color(direction);
        let stroke = Stroke::new(color, style.series_width.max(1));
        let fill = color.with_alpha(direction.body_fill().alpha());
        let mut out = vec![polygon_node(
            &[
                (body.left, body.top),
                (body.right, body.top),
                (body.right, body.bottom),
                (body.left, body.bottom),
            ],
            PathStyle::filled(fill).with_stroke(stroke),
            format!("{}.candle.{i}", self.tag_prefix),
        )];

        // The wicks hang off the body edges the axis DID place, so an upper
        // shadow survives a low the axis cannot carry.
        for (end, from, tag) in [
            (candle.high(), body.top, "hi"),
            (candle.low(), body.bottom, "lo"),
        ] {
            let Some(y) = g.y.map(end) else { continue };
            out.push(stroke_path(
                &[(body.center, from), (body.center, y)],
                stroke,
                format!("{}.wick.{i}.{tag}", self.tag_prefix),
            ));
            if self.caps {
                out.push(stroke_path(
                    &[(body.cap_left, y), (body.cap_right, y)],
                    stroke,
                    format!("{}.cap.{i}.{tag}", self.tag_prefix),
                ));
            }
        }
        out
    }

    /// The pixel geometry of session `i`'s body — `None` when the x-axis
    /// carries no place for it, or the value axis cannot place a body edge.
    ///
    /// A doji's body has `top == bottom`, which is the correct glyph rather
    /// than a degenerate case to pad away: the form's whole point is that
    /// there is no body.
    fn body_geometry(&self, g: &CandleGeom, i: usize) -> Option<BodyRect> {
        let c = self.candles.get(i)?;
        let slot = self.slot_geometry(g, i)?;
        let (body_lo, body_hi) = c.body();
        let top = g.y.map(body_hi)?;
        let bottom = g.y.map(body_lo)?;
        Some(BodyRect {
            center: slot.center,
            left: slot.left,
            right: slot.right,
            top,
            bottom,
            cap_left: slot.cap_left,
            cap_right: slot.cap_right,
        })
    }

    /// R1624 — the pixel box session `i`'s mark occupies, or `None` when the
    /// value axis can place neither of the prices that box needs.
    ///
    /// A candle's box is its body; a bar's is its full range. One question,
    /// two answers, asked once so the inspect ring and anything else that
    /// wants "where is this session drawn" cannot drift apart.
    fn mark_bounds(&self, g: &CandleGeom, i: usize) -> Option<BodyRect> {
        match self.mark {
            SessionMark::Ohlc => {
                let c = self.candles.get(i)?;
                let slot = self.slot_geometry(g, i)?;
                Some(BodyRect {
                    center: slot.center,
                    left: slot.left,
                    right: slot.right,
                    top: g.y.map(c.high())?,
                    bottom: g.y.map(c.low())?,
                    cap_left: slot.cap_left,
                    cap_right: slot.cap_right,
                })
            }
            _ => self.body_geometry(g, i),
        }
    }

    /// R1624 — session `i`'s HORIZONTAL extent, which no price enters.
    ///
    /// Split out because the two marks need it at different moments: a candle
    /// needs it only once the value axis has placed its body, and a bar needs
    /// it before the axis has placed anything, since a bar can lose a tick and
    /// still be a bar.
    fn slot_geometry(&self, g: &CandleGeom, i: usize) -> Option<SlotRect> {
        let center = self.center_px(g, i)?;
        let half = g.body_w / 2.0;
        let cap_half = half * CAP_WIDTH_FRAC;
        Some(SlotRect {
            center,
            left: center - half,
            right: center + half,
            cap_left: center - cap_half,
            cap_right: center + cap_half,
        })
    }

    /// Where session `i` sits horizontally — its band's centre on the ordinal
    /// reading, its instant's pixel on the elapsed one. The ONE definition
    /// both the marks and the inspect hit-test read.
    fn center_px(&self, g: &CandleGeom, i: usize) -> Option<f32> {
        match g.x.category() {
            Some(cat) => cat.band(i).map(|(lo, hi)| f32::midpoint(lo, hi)),
            None => g.x.map(self.candles.get(i)?.instant()),
        }
    }

    /// The inclusive index range this geometry draws, or `None` when nothing
    /// is in view.
    ///
    /// The ordinal reading asks its axis ([`CategoryScale::visible`], which
    /// moves with the domain); the elapsed reading takes the declared window,
    /// which is exactly what its time domain was built from, so the two
    /// answers agree by construction rather than by two derivations happening
    /// to match.
    fn drawn_range(&self, g: &CandleGeom) -> Option<(usize, usize)> {
        if self.candles.is_empty() {
            return None;
        }
        match g.x.category() {
            Some(cat) => cat.visible().map(|w| (w.lo(), w.hi())),
            None => Some(self.window_range()),
        }
    }

    /// [`drawn_range`](Self::drawn_range) as indices.
    fn drawn_indices(&self, g: &CandleGeom) -> Vec<usize> {
        self.drawn_range(g)
            .map_or_else(Vec::new, |(lo, hi)| (lo..=hi).collect())
    }

    /// The declared window clamped onto the sessions this chart holds, or the
    /// whole run. Callers guard emptiness first.
    fn window_range(&self) -> (usize, usize) {
        let last = self.candles.len().saturating_sub(1);
        self.window
            .map_or((0, last), |w| (w.lo().min(last), w.hi().min(last)))
    }

    /// The sessions the window selects — what both axes are domained from.
    fn windowed(&self) -> &[Candle] {
        if self.candles.is_empty() {
            return &self.candles;
        }
        let (lo, hi) = self.window_range();
        &self.candles[lo..=hi]
    }

    /// The narrowest positive gap between consecutive windowed instants —
    /// the pitch a body must fit inside for two sessions never to overlap.
    ///
    /// The *minimum* rather than a mean: the toolkit clamps a mean-derived
    /// column between `minimumColumnWidth` and `maximumColumnWidth`, which overlaps wherever the data is
    /// irregular — and real session data is irregular by construction, because
    /// a half-day before a holiday is a real bar. `None` when no two sessions
    /// differ, which is the degenerate case [`LONE_SESSION_PITCHES`] covers.
    fn min_pitch_ms(&self) -> Option<f64> {
        let mut best: Option<f64> = None;
        for pair in self.windowed().windows(2) {
            let [a, b] = pair else { continue };
            let gap = b.instant() - a.instant();
            if gap > 0.0 {
                best = Some(best.map_or(gap, |m: f64| m.min(gap)));
            }
        }
        best
    }

    /// The plot geometry, both scales, the tick sets and the body pitch every
    /// mark and the inspect hit-test derive from — the ONE definition of
    /// "where does session `i` sit", so the painted body and the inspect ring
    /// can never disagree.
    fn geom(&self, rect: Rect, style: &ChartStyle) -> CandleGeom {
        let (left, right, top, bottom) = plot_rect(rect, style.margin);

        // The value axis measures the WINDOWED sessions, unlike the box
        // plot's, which measures every distribution it holds. The two windows
        // mean different things: a box plot's selects among independent
        // groups, where a stable axis is what makes them comparable, while
        // this one selects a contiguous stretch of ONE series, where an axis
        // covering the sessions off screen spends its range on prices nothing
        // in view shows. The toolkit rescales neither.
        let measured = match self.y_kind {
            AxisKind::Log(_) => positive_candle_bounds(self.windowed()),
            AxisKind::Linear | AxisKind::Time | AxisKind::Category(_) => {
                candle_bounds(self.windowed())
            }
        };
        let raw = self
            .y_domain
            .or(measured)
            .unwrap_or_else(|| kind_extent(&self.y_kind));
        let dom = axis_domain(self.y_domain, raw, style.y_ticks, &self.y_kind);
        let y = axis_scale(dom, (bottom, top), &self.y_kind);
        let y_ticks = axis_ticks(&y, style.y_ticks, &style.room_y());

        let (x, x_ticks, body_w) = match self.reading {
            SessionAxis::Ordinal => {
                let domain = self
                    .window
                    .map_or_else(|| self.sessions.extent(), CategoryWindow::domain);
                let cat = CategoryScale::new(self.sessions.clone(), domain, (left, right));
                let w = (cat.band_width() * BODY_WIDTH_FRAC).max(1.0);
                (ValueScale::Category(cat), Fitted::empty(), w)
            }
            SessionAxis::Elapsed => {
                let scale = self.elapsed_scale((left, right), style);
                let ticks = axis_ticks(&scale, style.x_ticks, &style.room_x());
                let w = self.elapsed_body_width(&scale, (left, right));
                (scale, ticks, w)
            }
        };

        CandleGeom {
            left,
            right,
            top,
            bottom,
            y,
            y_ticks,
            x,
            x_ticks,
            body_w,
        }
    }

    /// The elapsed reading's time scale over the windowed instants, padded by
    /// half a pitch each side so the first and last bodies are not half
    /// outside the plot, then snapped to calendar boundaries by R1529's own
    /// resolver.
    fn elapsed_scale(&self, range: (f32, f32), style: &ChartStyle) -> ValueScale {
        let kind = AxisKind::Time;
        let raw = match (self.windowed().first(), self.windowed().last()) {
            (Some(a), Some(b)) => {
                let pad = self.min_pitch_ms().unwrap_or(0.0) / 2.0;
                (a.instant() - pad, b.instant() + pad)
            }
            _ => kind_extent(&kind),
        };
        axis_scale(axis_domain(None, raw, style.x_ticks, &kind), range, &kind)
    }

    /// The body width on the elapsed reading: the pitch measured in pixels
    /// through the scale itself, so a domain snap that widened the axis
    /// narrows the bodies with it.
    fn elapsed_body_width(&self, scale: &ValueScale, range: (f32, f32)) -> f32 {
        let (lo, _) = scale.domain();
        let span = range.1 - range.0;
        let pitch_px = match self.min_pitch_ms() {
            Some(pitch) => match (scale.map(lo), scale.map(lo + pitch)) {
                (Some(a), Some(b)) => b - a,
                _ => span / LONE_SESSION_PITCHES,
            },
            None => span / LONE_SESSION_PITCHES,
        };
        (pitch_px * BODY_WIDTH_FRAC).max(1.0)
    }

    /// The session index the inspect cursor is over.
    ///
    /// The ordinal reading defers to [`CategoryScale::nearest`], so the
    /// hit-test follows a window. The elapsed reading has no such helper —
    /// its slots are not evenly spaced, which is the point of it — so it takes
    /// the drawn session whose centre is nearest.
    fn resolve_focus(&self, g: &CandleGeom, rect: Rect) -> Option<usize> {
        let fraction = self.inspect?;
        if g.right - g.left <= 0.0 {
            return None;
        }
        let cursor =
            (to_f32(rect.x) + fraction.clamp(0.0, 1.0) * to_f32(rect.w)).clamp(g.left, g.right);
        match g.x.category() {
            Some(cat) => cat.nearest(cursor),
            None => self
                .drawn_indices(g)
                .into_iter()
                .filter_map(|i| self.center_px(g, i).map(|px| (i, (px - cursor).abs())))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i),
        }
    }

    /// Resolve the inspect overlay: a ring framing the focused body, plus a
    /// tooltip stating the four prices AND the direction.
    fn resolve_inspect(
        &self,
        g: &CandleGeom,
        rect: Rect,
        style: &ChartStyle,
    ) -> Option<CandleInspect> {
        let idx = self.resolve_focus(g, rect)?;
        let c = self.candles.get(idx)?;
        // R1624 — the ring outlines what the MARK occupies, which is the
        // body for a candle and the whole range for a bar. Reading it off
        // `body_geometry` for both would have ringed a bar's open-to-close
        // span and left its spine sticking out of the highlight.
        let b = self.mark_bounds(g, idx);
        let highlight = b.as_ref().map(|b| {
            outline_box(
                Rect::new(
                    to_u32(b.left),
                    to_u32(b.top),
                    to_u32(b.right - b.left).max(1),
                    to_u32(b.bottom - b.top).max(1),
                ),
                style.crosshair,
                format!("{}.inspect.highlight", self.tag_prefix),
            )
        });
        // Every number goes through the AXIS's own readout format, so a log
        // axis reads `1.2k` in the same vocabulary its tick labels use.
        let fmt = |v: f64| g.y_format().readout(v);
        let direction = c.direction();
        let mut rows = vec![
            self.row("open", &fmt(c.open()), style.tooltip_fg, "open"),
            self.row("high", &fmt(c.high()), style.tooltip_fg, "high"),
            self.row("low", &fmt(c.low()), style.tooltip_fg, "low"),
            self.row("close", &fmt(c.close()), style.tooltip_fg, "close"),
        ];
        let change = match c.change_ratio() {
            Some(r) => format!("{} ({:+.2}%)", fmt(c.change()), r * 100.0),
            None => fmt(c.change()),
        };
        rows.push(self.row(
            direction.name(),
            &change,
            self.direction_color(direction),
            "change",
        ));
        let anchor = b.as_ref().map_or(g.left, |b| b.center);
        let header = TickFormat::Time.readout(c.instant());
        let tooltip = callout(
            anchor,
            g.right,
            g.top,
            &header,
            format!("{}.inspect.header", self.tag_prefix),
            &rows,
            style,
            format!("{}.inspect.tooltip", self.tag_prefix),
        );
        Some(CandleInspect { highlight, tooltip })
    }

    /// One `label  value` tooltip row under this chart's tag prefix.
    ///
    /// Byte-identical to [`BoxPlotChart`](crate::BoxPlotChart)'s, and
    /// deliberately not lifted: the obligation-3b threshold is three
    /// mechanical copies, and these are the only **two** — the other six
    /// charts build a `CalloutRow` inline because their tooltips are one row
    /// or a per-series map, not a fixed labelled ladder. The third multi-row
    /// chart is what should move it into [`crate::draw`].
    fn row(&self, name: &str, value: &str, color: Color, tag: &str) -> CalloutRow {
        CalloutRow {
            text: format!("{name} {value}"),
            color,
            tag: format!("{}.inspect.{tag}", self.tag_prefix),
        }
    }

    /// The inspect readout as one line — the same session the tooltip paints,
    /// stated for a screen reader. `None` when nothing is inspected.
    ///
    /// It names the direction, which is the fact the picture encodes twice
    /// (hue and fill) and a screen reader receives neither of. The toolkit's
    /// charts implement no accessibility interface at all, so a toolkit
    /// candlestick chart announces nothing.
    #[must_use]
    pub fn inspect_readout(&self, rect: Rect, style: &ChartStyle) -> Option<String> {
        let g = self.geom(rect, style);
        let idx = self.resolve_focus(&g, rect)?;
        let c = self.candles.get(idx)?;
        Some(c.readout(tick_step(g.y_ticks.ticks())))
    }
}

/// The rising hue — a bright green, and the brightness is the point.
///
/// **Chosen by measurement, not by convention.** The obvious pair, a mid
/// green and a mid red of the weight this crate's palette uses
/// (`#1F8A5F` / `#D13A3A`), measures **1.11:1** — the two are all but
/// *isoluminant*, which is exactly why the traditional finance palette fails
/// a deuteranope so completely: strip the hue and there is nothing left. Two
/// colours can only be told apart without hue if they differ in *luminance*,
/// so the default pair is separated there — a light green against a deep red,
/// clearing WCAG 2.2 §1.4.11's 3:1 with room. See
/// [`CandlestickChart::direction_contrast`], and the test that measures both
/// pairs.
const DEFAULT_RISING: Color = Color::rgb(0x00, 0xC8, 0x53);

/// The falling hue — a deep red. Green-up / red-down is what a reader of this
/// form expects, and keeping the convention costs nothing once the pair is
/// separated in luminance (see [`DEFAULT_RISING`]) and the direction is
/// carried a second time by [`Direction::body_fill`].
const DEFAULT_FALLING: Color = Color::rgb(0x8B, 0x1A, 0x1A);

/// The doji hue — neutral, because a doji is neither, and the toolkit has no
/// slot for it at all.
const DEFAULT_DOJI: Color = Color::rgb(0x8A, 0x92, 0x9E);

/// The plot geometry, both scales, tick sets and body pitch a candlestick
/// body derives everything from. One resolve, shared by the marks, the
/// gridlines and the inspect hit-test.
struct CandleGeom {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    y: ValueScale,
    y_ticks: Fitted,
    /// The x-axis: `Category` on the ordinal reading, `Time` on the elapsed
    /// one. Which arm it is *is* the reading, so nothing downstream carries a
    /// second copy of that choice.
    x: ValueScale,
    /// Empty on the ordinal reading, where labels are per-slot rather than
    /// per-tick.
    x_ticks: Fitted,
    body_w: f32,
}

impl CandleGeom {
    /// The value axis's label format — per-magnitude on a log axis, the
    /// constant tick step on a linear one (R1528).
    fn y_format(&self) -> TickFormat {
        axis_format(&self.y, self.y_ticks.ticks())
    }
}

/// One session's resolved pixel geometry.
/// R1624 — a session's horizontal extent, before any price is placed.
struct SlotRect {
    center: f32,
    left: f32,
    right: f32,
    cap_left: f32,
    cap_right: f32,
}

struct BodyRect {
    center: f32,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    cap_left: f32,
    cap_right: f32,
}

/// The resolved inspect overlay, split so the ring paints over the marks and
/// the tooltip above everything.
struct CandleInspect {
    highlight: Option<Scene>,
    tooltip: Vec<Scene>,
}

impl ChartLegend for CandlestickChart {
    /// **An empty roster**: this chart draws one session series, so there is no
    /// second named thing for a legend to distinguish. What its two colours mean
    /// is a direction (up or down) rather than a part that could be hidden, and
    /// [`CandlestickChart::direction_contrast`] is where that is stated.
    fn legend(&self) -> Legend {
        Legend::new(&self.tag_prefix, "Sessions", Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_probe::{count_prefix, find, has, tags, text_of};

    /// ★ R1634 — a candlestick's columns are its four PRICES, and a windowed
    /// chart presents the sessions it drew while declaring how many there are.
    ///
    /// The third consumer of the projection and the second whose columns are
    /// not series, which is what makes "a column is what the chart carries per
    /// point" a rule rather than one chart's convenience.
    #[test]
    fn r1634_a_candlestick_projects_its_four_prices_as_columns() {
        let candles: Vec<Candle> = (0..8)
            .map(|i| {
                let base = f64::from(i) + 10.0;
                Candle::new(
                    f64::from(i) * 86_400_000.0,
                    base,
                    base + 3.0,
                    base - 2.0,
                    base + 1.0,
                )
                .expect("a well-ordered session")
            })
            .collect();
        let chart = CandlestickChart::new(candles).with_tag_prefix("cs");

        let table = chart.access_table("Prices", "Session");
        assert_eq!(
            table
                .columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["open", "high", "low", "close"],
        );
        assert_eq!(table.rows.len(), 8);
        assert_eq!(table.rows[0].cells.len(), 4);
        assert_eq!(table.rows[0].cells[0].value, "10", "the first open");
        assert_eq!(table.rows[0].cells[1].value, "13", "and its high");
        assert_eq!(
            table.rows[0].cells[1].tag.as_deref(),
            Some("cs.wick.0.hi"),
            "the high points at the wick it is the end of"
        );
        assert_eq!(
            table.rows[0].cells[0].tag.as_deref(),
            Some("cs.candle.0"),
            "and the open at the body it is an edge of"
        );
        assert_eq!(table.set_size, None, "nothing is windowed");

        let windowed = chart
            .window(CategoryWindow::new(2, 4))
            .access_table("Prices", "Session");
        assert_eq!(windowed.rows.len(), 3, "the window is the bound");
        assert_eq!(
            windowed.set_size,
            Some(8),
            "★ and the whole extent is declared, so an AT does not describe the \
             window as the data"
        );
    }

    const RECT: Rect = Rect::new(0, 0, 720, 400);

    /// 2026-03-02T00:00:00Z — a Monday, so the fixture's weekend gap is real.
    const MON: f64 = 1_772_409_600_000.0;
    const DAY_MS: f64 = 86_400_000.0;

    /// Five daily sessions Monday..Friday, then the NEXT Monday — so the last
    /// gap is three days where every other is one. That irregularity is what
    /// the two readings disagree about.
    fn week() -> Vec<Candle> {
        let spec: [(f64, f64, f64, f64, f64); 6] = [
            (0.0, 100.0, 104.0, 99.0, 103.0),  // rising
            (1.0, 103.0, 105.0, 101.0, 102.0), // falling
            (2.0, 102.0, 106.0, 102.0, 106.0), // rising
            (3.0, 106.0, 107.0, 103.0, 106.0), // DOJI
            (4.0, 106.0, 108.0, 104.0, 105.0), // falling
            (7.0, 105.0, 111.0, 105.0, 110.0), // rising, after the weekend
        ];
        spec.into_iter()
            .map(|(d, o, h, l, c)| {
                Candle::new(d.mul_add(DAY_MS, MON), o, h, l, c).expect("ordered fixture")
            })
            .collect()
    }

    fn week_chart() -> CandlestickChart {
        CandlestickChart::new(week())
    }

    /// R1624 — the bar's spine spans the whole range and its two ticks sit
    /// on the open and the close, left and right of it.
    ///
    /// Checked against the geometry rather than against a picture: the tick
    /// prices are read back out of the painted y coordinates through the same
    /// scale, so a mark that drew the close where the open belongs fails here.
    #[test]
    fn r1624_an_ohlc_bar_is_a_spine_with_an_open_tick_and_a_close_tick() {
        let style = ChartStyle::default();
        let chart = week_chart().with_mark(SessionMark::Ohlc);
        let scene = chart.build(RECT, &style);
        let g = chart.geom(RECT, &style);

        assert_eq!(chart.mark(), SessionMark::Ohlc);
        assert_eq!(
            count_prefix(&scene, "chart.ohlc."),
            18,
            "3 nodes x 6 sessions"
        );
        assert_eq!(
            count_prefix(&scene, "chart.candle."),
            0,
            "a bar is not a candle wearing a different tag",
        );

        for (i, c) in chart.candles().iter().enumerate() {
            let range = find(&scene, &format!("chart.ohlc.{i}.range")).expect("spine");
            let (top, bottom) = y_span(range);
            assert!(
                (top - g.y.map(c.high()).expect("high placed")).abs() < 0.5,
                "session {i} spine starts at the high",
            );
            assert!(
                (bottom - g.y.map(c.low()).expect("low placed")).abs() < 0.5,
                "session {i} spine ends at the low",
            );

            let slot = chart.slot_geometry(&g, i).expect("placed");
            for (tag, price, x_lo, x_hi) in [
                ("open", c.open(), slot.left, slot.center),
                ("close", c.close(), slot.center, slot.right),
            ] {
                let node = find(&scene, &format!("chart.ohlc.{i}.{tag}")).expect(tag);
                let (y, _) = y_span(node);
                assert!(
                    (y - g.y.map(price).expect("price placed")).abs() < 0.5,
                    "session {i} {tag} tick sits on its price",
                );
                let (left, right) = x_span(node);
                assert!(
                    (left - x_lo).abs() < 0.5 && (right - x_hi).abs() < 0.5,
                    "session {i} {tag} tick is on its own side: {left}..{right}",
                );
            }
        }
    }

    /// R1624 — direction survives the loss of colour, by SHAPE.
    ///
    /// The candle answers this with a hollow body; a bar has no body, so it
    /// answers with the ticks' relative height. Two sessions with the same
    /// prices in the opposite order must therefore differ geometrically, not
    /// only in hue — and that is asserted here with the colours forced equal,
    /// so a test that passed on colour alone cannot.
    #[test]
    fn r1624_a_bars_direction_is_legible_without_colour() {
        let style = ChartStyle::default();
        let one = Color::rgb(0x40, 0x40, 0x40);
        let flat = |candles: Vec<Candle>| {
            CandlestickChart::new(candles)
                .with_mark(SessionMark::Ohlc)
                .with_direction_colors(one, one, one)
                .build(RECT, &style)
        };
        let up = flat(vec![
            Candle::new(MON, 100.0, 110.0, 90.0, 105.0).expect("ok"),
        ]);
        let down = flat(vec![
            Candle::new(MON, 105.0, 110.0, 90.0, 100.0).expect("ok"),
        ]);

        let tick_y = |scene: &Scene, tag: &str| {
            let node = find(scene, &format!("chart.ohlc.0.{tag}")).expect(tag);
            y_span(node).0
        };
        // On a rise the close tick is ABOVE the open tick (smaller y).
        assert!(
            tick_y(&up, "close") < tick_y(&up, "open"),
            "a rising session's close tick is the higher one",
        );
        assert!(
            tick_y(&down, "close") > tick_y(&down, "open"),
            "a falling session's close tick is the lower one",
        );
        // ...and the spines are identical, so nothing but the ticks carries it.
        let spine = |scene: &Scene| y_span(find(scene, "chart.ohlc.0.range").expect("spine"));
        assert_eq!(spine(&up), spine(&down), "the range is the same range");
    }

    /// R1624 — the two marks disagree about the session's ANCHOR, and that
    /// is the behaviour, not a bug in one of them.
    ///
    /// A candle hangs its wicks off its body, so a body the log axis cannot
    /// place takes the whole session. A bar has no single anchor, so it keeps
    /// the marks whose prices the axis CAN place.
    ///
    /// The fixture is the reachable case, and finding that out is what made
    /// the rule honest: on a log axis an unplaceable open implies an
    /// unplaceable low, so the asymmetry always arrives from the low side.
    #[test]
    fn r1624_a_bar_keeps_the_marks_the_axis_can_place() {
        let style = ChartStyle::default();
        // Open at zero: a logarithmic axis has no pixel for it.
        let candles = vec![
            Candle::new(MON, 0.0, 110.0, 0.0, 105.0).expect("ok"),
            Candle::new(MON + DAY_MS, 100.0, 112.0, 99.0, 108.0).expect("ok"),
        ];
        let of = |mark| {
            CandlestickChart::new(candles.clone())
                .with_mark(mark)
                .y_log()
                .build(RECT, &style)
        };
        let candle = of(SessionMark::Candle);
        assert!(
            !has(&candle, "chart.candle.0"),
            "the candle's anchor is its body, and the body has no pixel",
        );

        let bar = of(SessionMark::Ohlc);
        assert!(
            has(&bar, "chart.ohlc.0.close"),
            "the bar keeps the close, which the axis CAN place",
        );
        assert!(
            !has(&bar, "chart.ohlc.0.open"),
            "and drops the open, whose price has no pixel",
        );
        assert!(
            !has(&bar, "chart.ohlc.0.range"),
            "and the spine too, because its low has none either",
        );
        // The unaffected session is whole under either mark.
        assert!(has(&bar, "chart.ohlc.1.range"));
        assert!(has(&bar, "chart.ohlc.1.open"));
        assert!(has(&bar, "chart.ohlc.1.close"));
        // And the omission is REPORTED rather than inferred from the picture.
        let reported = CandlestickChart::new(candles).y_log().off_scale();
        assert!(
            reported.iter().any(|o| o.candle == 0),
            "off_scale names the session: {reported:?}",
        );
    }

    /// R1624 — the mark is a rendering, so everything else is shared.
    ///
    /// Both readings of the x-axis, the window, and the value axis are the
    /// chart's, not the mark's: switching marks must move the session's
    /// centre by exactly nothing.
    ///
    /// FOUND BY A COUNTERFACTUAL: the first draft asked `center_px` for both
    /// marks, which is upstream of everything the mark touches — nudging the
    /// slot geometry by two pixels for bars only was caught by nothing,
    /// because no assertion here had looked at a painted node. It reads the
    /// SCENE now, which is the thing the claim is about.
    #[test]
    fn r1624_the_mark_changes_the_glyph_and_nothing_else() {
        let style = ChartStyle::default();
        for reading in [SessionAxis::Ordinal, SessionAxis::Elapsed] {
            let painted = |mark: SessionMark| {
                let c = match reading {
                    SessionAxis::Elapsed => week_chart().elapsed(),
                    SessionAxis::Ordinal => week_chart().ordinal(),
                }
                .with_mark(mark);
                let scene = c.build(RECT, &style);
                (0..6)
                    .map(|i| {
                        let tag = match mark {
                            SessionMark::Ohlc => format!("chart.ohlc.{i}.range"),
                            _ => format!("chart.candle.{i}"),
                        };
                        let node = find(&scene, &tag).expect("painted");
                        let (left, right) = x_span(node);
                        f32::midpoint(left, right)
                    })
                    .collect::<Vec<f32>>()
            };
            let candle = painted(SessionMark::Candle);
            let ohlc = painted(SessionMark::Ohlc);
            for (i, (a, b)) in candle.iter().zip(ohlc.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 0.01,
                    "{reading:?} session {i}: the mark moved it, {a} vs {b}",
                );
            }
        }
        // The window narrows both marks identically.
        let windowed = |mark| {
            week_chart()
                .with_mark(mark)
                .window(CategoryWindow::new(1, 3))
                .build(RECT, &style)
        };
        assert_eq!(
            count_prefix(&windowed(SessionMark::Candle), "chart.candle."),
            3
        );
        assert_eq!(
            count_prefix(&windowed(SessionMark::Ohlc), "chart.ohlc."),
            9,
            "3 sessions x 3 nodes",
        );
    }

    /// R1624 — the inspect ring outlines what the MARK occupies.
    ///
    /// A bar's extent is its range; ringing its open-to-close span would
    /// leave the spine sticking out of the highlight, which is what reading
    /// the ring off `body_geometry` for both marks would do.
    ///
    /// **The fixture is the test.** The first draft focused the week
    /// fixture's middle session, which happens to open at its low and close
    /// at its high — so its body IS its range and both rings measured 69px
    /// whatever the code did. A session whose body is strictly inside its
    /// range is the only one that can tell the two apart.
    #[test]
    fn r1624_the_inspect_ring_outlines_the_mark_not_the_body() {
        let style = ChartStyle::default();
        let one = vec![Candle::new(MON, 100.0, 120.0, 80.0, 110.0).expect("ok")];
        let ring = |mark: SessionMark| {
            let scene = CandlestickChart::new(one.clone())
                .with_mark(mark)
                .inspect(Some(0.5))
                .build(RECT, &style);
            let node = find(&scene, "chart.inspect.highlight").expect("ring");
            let Scene::Box(b) = node else {
                panic!("expected a box, got {node:?}");
            };
            (b.rect.y, b.rect.h)
        };
        let (candle_y, candle_h) = ring(SessionMark::Candle);
        let (bar_y, bar_h) = ring(SessionMark::Ohlc);
        assert!(
            bar_h > candle_h,
            "a bar's ring covers the whole range, taller than the body: \
             {candle_h} -> {bar_h}",
        );
        assert!(
            bar_y < candle_y,
            "and starts above the body's top: {bar_y} vs {candle_y}",
        );
        // The ring really does contain the spine it rings.
        let scene = CandlestickChart::new(one)
            .with_mark(SessionMark::Ohlc)
            .inspect(Some(0.5))
            .build(RECT, &style);
        let (top, bottom) = y_span(find(&scene, "chart.ohlc.0.range").expect("spine"));
        let (ry, rh) = (to_f32(bar_y), to_f32(bar_h));
        assert!(
            top >= ry - 1.0 && bottom <= ry + rh + 1.0,
            "the spine {top}..{bottom} is inside the ring {ry}..{}",
            ry + rh,
        );
    }

    /// R1624 — every mark in the census draws something, so a new one cannot
    /// be added and left unpainted.
    #[test]
    fn r1624_every_mark_in_the_census_paints() {
        let style = ChartStyle::default();
        for mark in SessionMark::ALL {
            let scene = week_chart().with_mark(mark).build(RECT, &style);
            let stem = mark.tag_stem();
            assert!(
                count_prefix(&scene, &format!("chart.{stem}.")) > 0,
                "{mark:?} paints nodes under its own stem",
            );
        }
    }

    /// The y extent of a path node, as `(min_y, max_y)`.
    fn y_span(scene: &Scene) -> (f32, f32) {
        let Scene::Path(p) = scene else {
            panic!("expected a path, got {scene:?}");
        };
        let b = pinion_core::path_data::bounds(&p.commands).expect("has bounds");
        (b.min_y + to_f32(p.rect.y), b.max_y + to_f32(p.rect.y))
    }

    /// The x extent of a path node, as `(min_x, max_x)`.
    fn x_span(scene: &Scene) -> (f32, f32) {
        let Scene::Path(p) = scene else {
            panic!("expected a path, got {scene:?}");
        };
        let b = pinion_core::path_data::bounds(&p.commands).expect("has bounds");
        (b.min_x + to_f32(p.rect.x), b.max_x + to_f32(p.rect.x))
    }

    /// ★ The round's second claim: ONE datum, two readings, and the
    /// difference is measurable. On the ordinal reading the six sessions are
    /// evenly pitched — the weekend is invisible. On the elapsed reading the
    /// last gap is three times the others, because three days passed.
    ///
    /// The toolkit reaches the same two pictures by attaching two different
    /// axis objects and supplying the category names as a SECOND data source.
    #[test]
    fn r1567_one_datum_two_readings_of_the_x_axis() {
        let style = ChartStyle::default();
        let gaps = |chart: &CandlestickChart| {
            let g = chart.geom(RECT, &style);
            let px: Vec<f32> = (0..6)
                .map(|i| chart.center_px(&g, i).expect("placed"))
                .collect();
            px.windows(2).map(|w| w[1] - w[0]).collect::<Vec<f32>>()
        };

        let ordinal = gaps(&week_chart());
        let first = ordinal[0];
        for (k, gap) in ordinal.iter().enumerate() {
            assert!(
                (gap - first).abs() < 0.5,
                "ordinal gap {k} is {gap}, not the uniform {first}"
            );
        }

        let elapsed = gaps(&week_chart().elapsed());
        let one_day = elapsed[0];
        for (k, gap) in elapsed[..4].iter().enumerate() {
            assert!(
                (gap - one_day).abs() < 0.5,
                "elapsed gap {k} is {gap}, not the one-day {one_day}"
            );
        }
        assert!(
            (elapsed[4] - one_day * 3.0).abs() < 1.0,
            "the weekend is three days wide: {} vs {one_day}",
            elapsed[4]
        );
    }

    /// ★ The slot names are DERIVED from the instants, so the two readings
    /// cannot disagree about which session is which. The toolkit's category
    /// axis takes a string list unrelated to the sets' timestamps.
    #[test]
    fn r1567_the_slot_names_come_from_the_instants() {
        let chart = week_chart();
        let names: Vec<&str> = chart
            .sessions()
            .as_slice()
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(
            names,
            vec!["Mar 02", "Mar 03", "Mar 04", "Mar 05", "Mar 06", "Mar 09"],
            "the weekend is absent from the ORDINAL axis, and the names say so"
        );

        let scene = chart.build(RECT, &ChartStyle::default());
        assert_eq!(text_of(&scene, "chart.xlabel.5"), Some("Mar 09"));
    }

    /// ★ The sessions are sorted by instant at construction, so a caller's
    /// insertion order cannot make the two readings disagree.
    /// `append` does not sort, and its category axis then
    /// draws insertion order while its datetime axis draws time order.
    #[test]
    fn r1567_the_order_is_the_instants_not_the_insertion() {
        let mut shuffled = week();
        shuffled.reverse();
        let chart = CandlestickChart::new(shuffled);
        let instants: Vec<f64> = chart.candles().iter().map(Candle::instant).collect();
        assert!(
            instants.windows(2).all(|w| w[0] <= w[1]),
            "not ascending: {instants:?}"
        );
        assert_eq!(chart.sessions().at(0), Some("Mar 02"));
        assert_eq!(chart.sessions().at(5), Some("Mar 09"));
    }

    /// ★ The direction reaches the PAINT twice — once as hue, once as fill —
    /// and the fill half is the one that survives a grayscale pipeline. The
    /// toolkit encodes it in hue alone.
    ///
    /// The counterfactual is inside the test: if only hue carried it, the
    /// fill alpha would be constant across the six bodies.
    #[test]
    fn r1567_the_body_encodes_its_direction_twice() {
        let chart = week_chart();
        let scene = chart.build(RECT, &ChartStyle::default());
        let expected = [
            Direction::Rising,
            Direction::Falling,
            Direction::Rising,
            Direction::Doji,
            Direction::Falling,
            Direction::Rising,
        ];
        let mut alphas = Vec::new();
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(chart.candles()[i].direction(), *want, "session {i}");
            let Some(Scene::Path(p)) = find(&scene, &format!("chart.candle.{i}")) else {
                panic!("body {i} is a path")
            };
            let fill = p.style.fill.expect("a body declares a fill");
            let stroke = p.style.stroke.expect("a body declares a stroke");
            assert_eq!(
                fill.a,
                want.body_fill().alpha(),
                "session {i} ({want}) fill alpha"
            );
            assert_eq!(
                (stroke.color.r, stroke.color.g, stroke.color.b),
                {
                    let c = chart.direction_color(*want);
                    (c.r, c.g, c.b)
                },
                "session {i} ({want}) hue"
            );
            alphas.push(fill.a);
        }
        assert!(
            alphas.iter().any(|a| *a != alphas[0]),
            "the fill would be constant if only hue carried the direction: {alphas:?}"
        );

        // ...and the three inks are pairwise distinct, so the DOJI is not
        // quietly wearing the falling colour — which is precisely what the
        // toolkit does, having only two brushes to give it.
        let chart = week_chart();
        let inks = [Direction::Rising, Direction::Falling, Direction::Doji]
            .map(|d| chart.direction_color(d));
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            assert_ne!(inks[a], inks[b], "inks {a} and {b} are the same colour");
        }
    }

    /// ★ A doji's body has no height — that is the glyph, not a rounding
    /// error — while every other session's body does.
    #[test]
    fn r1567_a_doji_draws_a_bodyless_cross() {
        let style = ChartStyle::default();
        let chart = week_chart();
        let g = chart.geom(RECT, &style);
        let height = |i: usize| {
            let b = chart.body_geometry(&g, i).expect("placed");
            (b.bottom - b.top).abs()
        };
        assert!(height(3) < 0.001, "the doji body is {}px tall", height(3));
        for i in [0, 1, 2, 4, 5] {
            assert!(height(i) > 1.0, "session {i} body is {}px tall", height(i));
        }
        // ...and it still draws its wicks, so the session is not invisible.
        let scene = chart.build(RECT, &style);
        assert!(has(&scene, "chart.wick.3.hi"));
        assert!(has(&scene, "chart.wick.3.lo"));
    }

    /// ★ The two hues clear WCAG 2.2 §1.4.11's 3:1 for a meaningful graphical
    /// object — and the measurement that forced the defaults is kept here,
    /// because it is the round's own finding: the *conventional* finance pair
    /// is nearly ISOLUMINANT, so a reader who loses hue loses everything.
    ///
    /// The toolkit will paint any two brushes and say nothing, and its own
    /// documentation offers no guidance beyond "the colour used when the
    /// close value is higher than the open value".
    #[test]
    fn r1567_the_two_hues_are_distinguishable_and_it_is_stated() {
        let ratio = week_chart().direction_contrast();
        assert!(ratio >= 3.0, "rising vs falling contrast is {ratio}");

        // The pair a mid-weight palette would have picked, and the number that
        // sent the defaults to a light green against a deep red instead.
        let conventional = week_chart().with_direction_colors(
            Color::rgb(0x1F, 0x8A, 0x5F),
            Color::rgb(0xD1, 0x3A, 0x3A),
            DEFAULT_DOJI,
        );
        assert!(
            conventional.direction_contrast() < 1.2,
            "the conventional green/red pair measures {}, all but isoluminant",
            conventional.direction_contrast()
        );

        // Counterfactual: a caller that picks two near-identical hues is told
        // so rather than shipping an unreadable chart in silence.
        let bad = week_chart().with_direction_colors(
            Color::rgb(0x20, 0x80, 0x60),
            Color::rgb(0x22, 0x82, 0x62),
            Color::rgb(0, 0, 0),
        );
        assert!(
            bad.direction_contrast() < 1.2,
            "{}",
            bad.direction_contrast()
        );
    }

    /// ★ One window declaration narrows BOTH readings. The toolkit needs two
    /// calls against two axis objects, in two vocabularies, with nothing
    /// relating them.
    ///
    /// The second half is the part a counterfactual found missing: asserting
    /// only *which* sessions are drawn cannot see a window that failed to
    /// reach an axis's DOMAIN, because the survivors are still drawn — just
    /// in the pixels the unwindowed domain gives them. So the drawn ones must
    /// also SPREAD, which is the observable both readings share.
    #[test]
    fn r1567_one_window_narrows_both_readings() {
        let style = ChartStyle::default();
        let w = week_chart()
            .sessions()
            .window("Mar 04", "Mar 06")
            .expect("both names exist");

        // The middle pair's pixel gap before any window — the baseline the
        // narrowed chart has to beat on each reading.
        let gap = |chart: &CandlestickChart| {
            let g = chart.geom(RECT, &style);
            chart.center_px(&g, 3).expect("placed") - chart.center_px(&g, 2).expect("placed")
        };
        let full = [gap(&week_chart()), gap(&week_chart().elapsed())];

        for (k, chart) in [week_chart().window(w), week_chart().elapsed().window(w)]
            .into_iter()
            .enumerate()
        {
            let reading = chart.reading();
            let scene = chart.build(RECT, &style);
            let t = tags(&scene);
            assert!(
                !t.contains(&"chart.candle.0".to_string()),
                "{reading:?}: Monday is out"
            );
            assert!(
                !t.contains(&"chart.candle.5".to_string()),
                "{reading:?}: next Monday is out"
            );
            assert_eq!(
                count_prefix(&scene, "chart.candle."),
                3,
                "{reading:?}: three sessions in the window"
            );
            assert_eq!(
                chart
                    .visible_sessions(RECT, &style)
                    .map(|w| (w.lo(), w.hi())),
                Some((2, 4)),
                "{reading:?}"
            );
            assert!(
                gap(&chart) > full[k] * 1.5,
                "{reading:?}: the window reached the axis DOMAIN, so the three \
                 survivors spread across the plot ({} vs {})",
                gap(&chart),
                full[k]
            );
        }
    }

    /// ★ The value axis follows the window, so zooming in does not leave the
    /// price range spent on sessions off screen. The counterfactual is the
    /// unwindowed chart, whose domain reaches the whole week's high.
    #[test]
    fn r1567_the_value_axis_follows_the_window() {
        let style = ChartStyle::default();
        let full = week_chart().geom(RECT, &style).y.domain();
        let w = week_chart()
            .sessions()
            .window("Mar 02", "Mar 03")
            .expect("both names exist");
        let narrow = week_chart().window(w).geom(RECT, &style).y.domain();
        assert!(full.1 >= 111.0, "the full week reaches 111: {full:?}");
        assert!(narrow.1 < 111.0, "the two-day window does not: {narrow:?}");
    }

    /// ★ A slot a logarithmic axis cannot place draws nothing and is
    /// REPORTED — R1528's stance, on a datum with four slots. The
    /// counterfactual is the same data on a linear axis, where nothing is
    /// off-scale.
    #[test]
    fn r1567_a_slot_off_a_log_axis_is_reported_not_placed() {
        // A session that traded down to zero — a delisting, or a spread that
        // collapsed. Its low has no pixel on a log axis; its body does.
        let zeroed = vec![
            Candle::new(MON, 4.0, 6.0, 0.0, 5.0).expect("ordered"),
            Candle::new(MON + DAY_MS, 5.0, 9.0, 4.0, 8.0).expect("ordered"),
        ];
        let logged = CandlestickChart::new(zeroed.clone()).y_log();
        let off = logged.off_scale();
        assert_eq!(off.len(), 1, "only the zero low: {off:?}");
        assert_eq!(off[0].at, CandlePosition::Low);
        assert_eq!(off[0].candle, 0);

        let scene = logged.build(RECT, &ChartStyle::default());
        assert!(
            !has(&scene, "chart.wick.0.lo"),
            "the unplaceable low draws nothing"
        );
        assert!(has(&scene, "chart.wick.0.hi"), "the high is unaffected");
        assert!(has(&scene, "chart.candle.0"), "the body still draws");

        // Counterfactual: on a linear axis the same session is wholly
        // placeable, so this is a property of the AXIS.
        let linear = CandlestickChart::new(zeroed);
        assert!(linear.off_scale().is_empty());
        assert!(has(
            &linear.build(RECT, &ChartStyle::default()),
            "chart.wick.0.lo"
        ));
    }

    /// ★ Bodies never overlap on the elapsed reading, because the pitch is
    /// the MINIMUM gap rather than a mean. A mean over this fixture (five
    /// one-day gaps and one three-day) would be 1.4 days and the weekday
    /// bodies would run into each other.
    #[test]
    fn r1567_the_elapsed_pitch_is_the_narrowest_gap() {
        let style = ChartStyle::default();
        let chart = week_chart().elapsed();
        let g = chart.geom(RECT, &style);
        for i in 0..5 {
            let a = chart.body_geometry(&g, i).expect("placed");
            let b = chart.body_geometry(&g, i + 1).expect("placed");
            assert!(
                a.right <= b.left + 0.001,
                "bodies {i} and {} overlap: {} > {}",
                i + 1,
                a.right,
                b.left
            );
        }
        // And the pitch really is the one-day gap, not the 1.4-day mean.
        assert!(
            (chart.min_pitch_ms().expect("six sessions") - DAY_MS).abs() < 1.0,
            "{:?}",
            chart.min_pitch_ms()
        );
    }

    /// ★ The x-label cardinality follows the reading, and the tags say which
    /// question was answered: one per SLOT on the ordinal axis, one per TICK
    /// on the elapsed one.
    #[test]
    fn r1567_each_reading_labels_what_it_is_made_of() {
        let style = ChartStyle::default();
        let ordinal = week_chart().build(RECT, &style);
        assert_eq!(count_prefix(&ordinal, "chart.xlabel."), 6);
        assert_eq!(count_prefix(&ordinal, "chart.label.x."), 0);
        assert_eq!(count_prefix(&ordinal, "chart.grid.x."), 0);

        let elapsed = week_chart().elapsed().build(RECT, &style);
        assert_eq!(count_prefix(&elapsed, "chart.xlabel."), 0);
        assert!(count_prefix(&elapsed, "chart.label.x.") > 0);
        assert!(count_prefix(&elapsed, "chart.grid.x.") > 0);
    }

    /// ★ The caps are the toolkit's `capsVisible`, off by default, and they are their
    /// own addressable marks when on.
    #[test]
    fn r1567_the_caps_are_opt_in() {
        let style = ChartStyle::default();
        assert_eq!(
            count_prefix(&week_chart().build(RECT, &style), "chart.cap."),
            0
        );
        assert_eq!(
            count_prefix(
                &week_chart().with_caps(true).build(RECT, &style),
                "chart.cap."
            ),
            12,
            "two per session"
        );
    }

    /// ★ The scrub names the four prices AND the direction, and the tooltip
    /// and the a11y readout come from one derivation, so they cannot state
    /// different numbers.
    #[test]
    fn r1567_the_scrub_states_the_session_and_its_direction() {
        let style = ChartStyle::default();
        let chart = week_chart().inspect(Some(0.5));
        let readout = chart.inspect_readout(RECT, &style).expect("focused");
        assert!(readout.contains("2026-03-0"), "{readout}");
        assert!(readout.contains("open "), "{readout}");

        let scene = chart.build(RECT, &style);
        for expected in [
            "chart.inspect.highlight",
            "chart.inspect.tooltip",
            "chart.inspect.header",
            "chart.inspect.open",
            "chart.inspect.high",
            "chart.inspect.low",
            "chart.inspect.close",
            "chart.inspect.change",
        ] {
            assert!(has(&scene, expected), "missing {expected}");
        }

        // Counterfactual: with no scrub there is no overlay at all.
        let quiet = week_chart().build(RECT, &style);
        assert_eq!(count_prefix(&quiet, "chart.inspect."), 0);
    }

    /// ★ An empty chart builds a legible empty plot on either reading rather
    /// than panicking or collapsing its axis.
    #[test]
    fn r1567_an_empty_chart_still_builds() {
        let style = ChartStyle::default();
        for chart in [
            CandlestickChart::new(Vec::new()),
            CandlestickChart::new(Vec::new()).elapsed(),
        ] {
            let scene = chart.build(RECT, &style);
            assert!(has(&scene, "chart.axis.x"));
            assert_eq!(count_prefix(&scene, "chart.candle."), 0);
            assert!(chart.off_scale().is_empty());
            assert_eq!(chart.visible_sessions(RECT, &style), None);
        }
        // `build_fill` at zero size is the bootstrap sentinel, not a panic.
        let empty = week_chart().build_fill((0, 0), &ChartStyle::default());
        assert_eq!(tags(&empty), vec!["chart".to_string()]);
    }

    /// ★ A single session is a total case on both readings — no gap to
    /// measure a pitch from, and no division by zero.
    #[test]
    fn r1567_a_lone_session_is_drawn_on_both_readings() {
        let style = ChartStyle::default();
        let lone = vec![Candle::new(MON, 100.0, 104.0, 99.0, 103.0).expect("ordered")];
        for chart in [
            CandlestickChart::new(lone.clone()),
            CandlestickChart::new(lone.clone()).elapsed(),
        ] {
            let reading = chart.reading();
            assert_eq!(chart.min_pitch_ms(), None, "{reading:?}");
            let g = chart.geom(RECT, &style);
            assert!(g.body_w >= 1.0, "{reading:?}: body {}", g.body_w);
            let b = chart.body_geometry(&g, 0).expect("placed");
            assert!(b.right > b.left, "{reading:?}");
            assert!(
                has(&chart.build(RECT, &style), "chart.candle.0"),
                "{reading:?}"
            );
        }
    }
}
