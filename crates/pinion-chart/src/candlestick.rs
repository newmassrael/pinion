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

use pinion_core::Scene;
use pinion_core::contrast::contrast_ratio;
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{Color, PathStyle, Stroke};

use crate::candle::{Candle, CandlePosition, Direction, candle_bounds, positive_candle_bounds};
use crate::draw::{
    CalloutRow, absolute, box_node, callout, category_label_node, fill_parent, outline_box,
    plot_rect, polygon_node, stroke_path, to_f32, to_u32, x_tick_labels,
};
use crate::plot::{
    axis_domain, axis_format, axis_minor_ticks, axis_scale, axis_ticks, kind_extent, tick_pixels,
};
use crate::scale::{
    AxisKind, Categories, CategoryScale, CategoryWindow, DEFAULT_LOG_BASE, ValueScale,
};
use crate::style::ChartStyle;
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
    /// `increasingColor` / `decreasingColor`; the third has no the toolkit equivalent because a doji has
    /// no the toolkit name).
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
        let y_pos = tick_pixels(&g.y, &g.y_ticks);
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
        let x_pos = tick_pixels(&g.x, &g.x_ticks);
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
            &g.y_ticks,
            &y_pos,
            &g.y_format(),
            style,
            &self.tag_prefix,
        ));
        children.extend(tooltip);

        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
    }

    /// The x-axis labels — one per drawn slot on the ordinal reading, one per
    /// tick on the elapsed one. Two cardinalities because they answer two
    /// questions; see the module doc.
    fn x_labels(&self, g: &CandleGeom, rect: Rect, style: &ChartStyle) -> Vec<Scene> {
        let size = style.label_size_px.max(1);
        let format = axis_format(&g.x, &g.x_ticks);
        match g.x.category() {
            Some(cat) => self
                .drawn_indices(g)
                .into_iter()
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
                .collect(),
            None => x_tick_labels(
                &g.x,
                &g.x_ticks,
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
        let center = self.center_px(g, i)?;
        let half = g.body_w / 2.0;
        let (body_lo, body_hi) = c.body();
        let top = g.y.map(body_hi)?;
        let bottom = g.y.map(body_lo)?;
        let cap_half = half * CAP_WIDTH_FRAC;
        Some(BodyRect {
            center,
            left: center - half,
            right: center + half,
            top,
            bottom,
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
        let y_ticks = axis_ticks(&y, style.y_ticks);

        let (x, x_ticks, body_w) = match self.reading {
            SessionAxis::Ordinal => {
                let domain = self
                    .window
                    .map_or_else(|| self.sessions.extent(), CategoryWindow::domain);
                let cat = CategoryScale::new(self.sessions.clone(), domain, (left, right));
                let w = (cat.band_width() * BODY_WIDTH_FRAC).max(1.0);
                (ValueScale::Category(cat), Vec::new(), w)
            }
            SessionAxis::Elapsed => {
                let scale = self.elapsed_scale((left, right), style);
                let ticks = axis_ticks(&scale, style.x_ticks);
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
        let b = self.body_geometry(g, idx);
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
        Some(c.readout(tick_step(&g.y_ticks)))
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
    y_ticks: Vec<f64>,
    /// The x-axis: `Category` on the ordinal reading, `Time` on the elapsed
    /// one. Which arm it is *is* the reading, so nothing downstream carries a
    /// second copy of that choice.
    x: ValueScale,
    /// Empty on the ordinal reading, where labels are per-slot rather than
    /// per-tick.
    x_ticks: Vec<f64>,
    body_w: f32,
}

impl CandleGeom {
    /// The value axis's label format — per-magnitude on a log axis, the
    /// constant tick step on a linear one (R1528).
    fn y_format(&self) -> TickFormat {
        axis_format(&self.y, &self.y_ticks)
    }
}

/// One session's resolved pixel geometry.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_probe::{count_prefix, find, has, tags, text_of};

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
