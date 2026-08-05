//! The timeline builder — projects labelled time [`Span`]s, grouped into
//! horizontal [`Lane`]s, onto a shared numeric time axis, with a draggable
//! **playhead** scrubber. The crate's first NON-value form: a line / bar /
//! scatter chart answers "how big", a timeline answers "when, and for how
//! long" — the track view (Chrome tracing / Unreal Insights / a DAW's
//! transport) an editor sequencer and a capture-replay tool reach for.
//!
//! # Why a distinct builder rather than a chart mode
//!
//! A cartesian chart maps VALUE to the y-axis; a timeline maps LANE (a
//! categorical row) to y and a time INTERVAL (`start..end`) to a horizontal
//! extent on x. Its mark is a placed box spanning `[map(start), map(end)]`, not
//! a point or a baseline-grown bar — the layout genuinely diverges. It shares
//! the crate's scale / ticks / palette / draw core with the charts: the time
//! axis is the same numeric [`LinearScale`] a line chart interpolates along, the
//! ruler ticks are the same Heckbert [`nice_ticks`], the lane colours the same
//! [`CategoricalPalette`], and every leaf a [`crate::draw`] primitive (a span IS
//! a `box_node`, the ruler a `stroke_path`, a label a `label_node`, the playhead
//! readout the shared `callout`).
//!
//! # Introspection
//!
//! Every node carries a tag under `tag_prefix` (default `"timeline"`):
//! `timeline.bg`, `timeline.axis.x` (the top ruler line) / `timeline.axis.y`
//! (the left lane-gutter edge), `timeline.grid.x.{k}` (a vertical time gridline
//! per ruler tick), `timeline.tick.{k}` (the ruler's time label per tick),
//! `timeline.rule.{i}` (the separator above lane `i`, for `i` in `1..lanes`),
//! `timeline.lane.{i}.label` (the lane name in the left gutter), and
//! `timeline.lane.{i}.span.{j}` (span `j` of lane `i` — a filled box; a
//! non-finite or fully-out-of-domain span emits none, so the count is
//! present-if-visible).
//!
//! When [`playhead`](Timeline::playhead) is set the overlay adds
//! `timeline.playhead` (a vertical line at the scrubbed time across all lanes),
//! `timeline.playhead.tooltip` (the readout callout box),
//! `timeline.playhead.header` (the scrubbed time), and `timeline.playhead.value.{i}`
//! (one row per lane that HAS a span containing the scrubbed time — its lane
//! name + that span's label; lanes idle at that instant emit no row).
//!
//! # Coordinate contract
//!
//! Identical to the charts: [`Timeline::build_fill`] is the **layout-native**
//! entry point (fill-parent root, children in the timeline's own `(0,0)..(w,h)`
//! frame), [`Timeline::build`] pins to a caller rect. Read the crate-level
//! "Known limitations": the `build_fill` measured-rect seam is Vello-only, and
//! `Scene::Path` (the ruler / gridlines / playhead) does not render on TUI (the
//! span boxes and text labels do).

use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{Color, Stroke, TextAlign};

use crate::draw::{
    CalloutRow, absolute, box_node, callout, fill_parent, gridlines, label_node, plot_rect,
    stroke_path, to_f32, to_u32,
};
use crate::palette::CategoricalPalette;
use crate::scale::LinearScale;
use crate::style::ChartStyle;
use crate::ticks::{TickFormat, nice_ticks, tick_step, time_ticks};

/// The vertical inset (px) between a lane's band edges and its span boxes, so
/// adjacent lanes' spans read as distinct rows rather than one solid block.
const SPAN_VPAD: f32 = 3.0;

/// Fixed width (px) of a ruler time label's box, centred on its tick. Wide
/// enough for an SI-suffixed time (`1.2k`) or a small ms value; the ruler
/// aims for `style.x_ticks` labels, so the boxes do not crowd.
const TICK_LABEL_W: u32 = 56;

/// One time span: a half-open-ish interval `[start, end]` on the time axis, a
/// `label` (shown in the playhead readout when scrubbed onto it), and an
/// optional per-span `color` override (else the lane's palette colour). `start`
/// and `end` may be given in either order — the builder normalises them.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// The span's start time on the axis.
    pub start: f64,
    /// The span's end time on the axis.
    pub end: f64,
    /// The span's label — surfaced in the playhead readout.
    pub label: String,
    /// An optional colour override for THIS span (else the lane colour).
    pub color: Option<Color>,
}

impl Span {
    /// A span over `[start, end]` with the lane's palette colour.
    #[must_use]
    pub fn new(start: f64, end: f64, label: impl Into<String>) -> Self {
        Self {
            start,
            end,
            label: label.into(),
            color: None,
        }
    }

    /// Override this span's colour.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// The normalised `(lo, hi)` extent (`start`/`end` in ascending order).
    fn extent(&self) -> (f64, f64) {
        if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    /// Whether this span's interval contains `time` (endpoints inclusive).
    fn contains(&self, time: f64) -> bool {
        let (lo, hi) = self.extent();
        lo <= time && time <= hi
    }
}

/// One horizontal track: a `name` (shown in the left gutter) and its time
/// [`Span`]s. Spans within a lane are independent — they may abut, gap, or (the
/// caller's choice) overlap.
#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    /// The lane's name, shown left-aligned-end in the gutter.
    pub name: String,
    /// The lane's spans, in the caller's order (`.span.{j}` = the j-th).
    pub spans: Vec<Span>,
}

impl Lane {
    /// A lane named `name` holding `spans`.
    #[must_use]
    pub fn new(name: impl Into<String>, spans: Vec<Span>) -> Self {
        Self {
            name: name.into(),
            spans,
        }
    }
}

/// A lane timeline over labelled time [`Span`]s with a draggable playhead,
/// sharing the crate's scale / ticks / palette / draw core with the charts.
pub struct Timeline {
    lanes: Vec<Lane>,
    palette: CategoricalPalette,
    x_domain: Option<(f64, f64)>,
    playhead: Option<f32>,
    time_axis: bool,
    tag_prefix: String,
}

impl Timeline {
    /// A timeline over `lanes`, using the default palette (one colour per lane
    /// by index), an auto time-domain (the min span start to the max span end,
    /// nice-tick-snapped for the ruler), no playhead, and the `"timeline"` tag
    /// prefix.
    #[must_use]
    pub fn new(lanes: Vec<Lane>) -> Self {
        Self {
            lanes,
            palette: CategoricalPalette::default(),
            x_domain: None,
            playhead: None,
            time_axis: false,
            tag_prefix: "timeline".to_string(),
        }
    }

    /// Override the default per-lane colour palette (a span's own
    /// [`with_color`](Span::with_color) still wins).
    #[must_use]
    pub fn with_palette(mut self, palette: CategoricalPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Pin the time-axis domain instead of deriving it from the spans — the
    /// scrubbed-window / brush-zoom entry point (a caller re-domains to show a
    /// slice of a longer capture). Spans outside the domain are clipped.
    #[must_use]
    pub fn with_x_domain(mut self, lo: f64, hi: f64) -> Self {
        self.x_domain = Some((lo, hi));
        self
    }

    /// Read the ruler as UTC wall-clock time (R1529) — span `start` / `end`
    /// values become epoch **milliseconds**, as on
    /// [`LineChart::x_time`](crate::LineChart::x_time).
    ///
    /// A timeline is the crate's most literally time-shaped view, and until
    /// R1529 its ruler could only be a plain number line: a capture that
    /// began at a real instant printed `1772.4G` on every tick, because the
    /// nice-number step is decimal and the SI label compacts by magnitude
    /// (see the `ticks` module docs). Opt in when the spans are wall-clock
    /// instants; leave it off — the default — when they are offsets from a
    /// capture start, where a bare number IS the right reading.
    ///
    /// The ruler ticks then land on clock boundaries and carry
    /// multi-resolution labels, while the playhead readout takes the
    /// unambiguous full stamp: a scrub has no neighbouring labels to read
    /// the date from.
    #[must_use]
    pub fn time_axis(mut self) -> Self {
        self.time_axis = true;
        self
    }

    /// Override the intent/introspection tag prefix (default `"timeline"`).
    #[must_use]
    pub fn with_tag_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.tag_prefix = prefix.into();
        self
    }

    /// Show the playhead overlay (a vertical line across every lane at the
    /// scrubbed time, plus a readout of the span under it per lane) at
    /// `fraction` — the cursor's position as a fraction `0.0..=1.0` across the
    /// timeline `rect` width. `None` (the default) draws no playhead. Mirrors
    /// [`LineChart::inspect`](crate::LineChart::inspect): the fraction is the
    /// natural output of a pointer-capture scrub (`SliderExternal` value /
    /// `pointer_move` `x_rel`), which the builder maps through its own margins to
    /// a time on the axis, so the caller needs no layout knowledge.
    #[must_use]
    pub fn playhead(mut self, fraction: Option<f32>) -> Self {
        self.playhead = fraction;
        self
    }

    /// The lanes this timeline was built with.
    #[must_use]
    pub fn lanes(&self) -> &[Lane] {
        &self.lanes
    }

    /// Build the timeline PINNED to `rect` (the caller states the geometry).
    /// See [`crate::LineChart::build`] — same contract; prefer
    /// [`Self::build_fill`] for anything layout-placed.
    #[must_use]
    pub fn build(&self, rect: Rect, style: &ChartStyle) -> Scene {
        Scene::Container(
            self.build_body(Rect::new(0, 0, rect.w, rect.h), style)
                .with_layout(absolute(rect)),
        )
    }

    /// Build the timeline as a **layout-native** subtree (R1360 contract): the
    /// root fills its slot; children are authored in the timeline's own
    /// `(0,0)..(w,h)` frame. `(0,0)` returns an empty tagged root that still
    /// measures, so the size feeds back on the next paint (the `build_fill`
    /// bootstrap). See [`crate::LineChart::build_fill`].
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

    /// The timeline body, authored in `rect`'s frame — the ONE builder both
    /// entry points wrap (the `build`/`build_fill` split of R1360.4).
    #[allow(
        clippy::cast_precision_loss,
        reason = "the lane index / count is a small display count, exact in f32"
    )]
    fn build_body(&self, rect: Rect, style: &ChartStyle) -> ContainerNode {
        let g = self.geom(rect, style);
        let size = style.label_size_px.max(1);

        let mut children: Vec<Scene> = Vec::new();
        if let Some(bg) = style.background {
            children.push(box_node(rect, bg, format!("{}.bg", self.tag_prefix)));
        }

        // Vertical time gridlines (one per ruler tick) — the shared cartesian
        // primitive, spanning the full lane stack; no horizontal gridlines (a
        // lane is a categorical row, ruled by the separators below, not ticked).
        let x_pos: Vec<f32> = g.x_ticks.iter().map(|&t| g.x.map(t)).collect();
        children.extend(gridlines(
            (g.left, g.right, g.top, g.bottom),
            &x_pos,
            &[],
            style,
            &self.tag_prefix,
        ));

        // The top ruler line and the left lane-gutter edge (the L-frame, but
        // TOP-x rather than a chart's bottom-x, because a timeline reads its
        // time axis at the top over the lanes).
        let axis_stroke = Stroke::new(style.axis, 1);
        children.push(stroke_path(
            &[(g.left, g.top), (g.right, g.top)],
            axis_stroke,
            format!("{}.axis.x", self.tag_prefix),
        ));
        children.push(stroke_path(
            &[(g.left, g.top), (g.left, g.bottom)],
            axis_stroke,
            format!("{}.axis.y", self.tag_prefix),
        ));

        // Ruler time labels, centred over each tick in the top margin.
        let grid_stroke = Stroke::new(style.grid, 1);
        for (k, (&t, &px)) in g.x_ticks.iter().zip(&x_pos).enumerate() {
            children.push(label_node(
                g.x_format.label(t),
                // (R1396) The third x-tick consumer of the shared clamp: the
                // ruler's last time label no longer overhangs the timeline's
                // right edge into a docked neighbour.
                crate::draw::centered_label_x(px, TICK_LABEL_W, rect),
                rect.y + 4,
                TICK_LABEL_W,
                TextAlign::Center,
                style.label,
                size,
                format!("{}.tick.{k}", self.tag_prefix),
            ));
        }

        // Lanes: a separator above every lane but the first, the lane name in
        // the gutter, and one placed box per visible span.
        for (i, lane) in self.lanes.iter().enumerate() {
            let band_top = g.top + (i as f32) * g.lane_h;
            if i > 0 {
                children.push(stroke_path(
                    &[(g.left, band_top), (g.right, band_top)],
                    grid_stroke,
                    format!("{}.rule.{i}", self.tag_prefix),
                ));
            }
            // Lane name, vertically centred in its band, right-aligned into the
            // gutter (the same End alignment a y-tick label uses).
            let name_y = to_u32(band_top + g.lane_h / 2.0).saturating_sub(size / 2 + 1);
            children.push(label_node(
                lane.name.clone(),
                rect.x + 2,
                name_y,
                style.margin.left.saturating_sub(6).max(1),
                TextAlign::End,
                style.label,
                size,
                format!("{}.lane.{i}.label", self.tag_prefix),
            ));

            let span_top = to_u32(band_top + SPAN_VPAD);
            let span_h = to_u32((g.lane_h - 2.0 * SPAN_VPAD).max(1.0));
            let color = self.palette.color(i);
            for (j, span) in lane.spans.iter().enumerate() {
                if let Some(rect) = Self::span_rect(&g, span, span_top, span_h) {
                    children.push(box_node(
                        rect,
                        span.color.unwrap_or(color),
                        format!("{}.lane.{i}.span.{j}", self.tag_prefix),
                    ));
                }
            }
        }

        // The playhead over the lanes + its readout callout above everything.
        if let Some(px) = self.playhead_px(&g, rect) {
            children.push(stroke_path(
                &[(px, g.top), (px, g.bottom)],
                Stroke::new(style.crosshair, 2),
                format!("{}.playhead", self.tag_prefix),
            ));
            children.extend(self.playhead_callout(&g, px, style));
        }

        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
    }

    /// Span `s`'s placed box within a lane band (`span_top`, `span_h`), or
    /// `None` when it is non-finite or falls entirely outside the time domain.
    /// The x-extent is clamped into the plot so a span reaching past a pinned
    /// domain edge does not paint over the gutter / neighbours (a `Scene::Box`
    /// is not clipped by its container — the same guard the bar chart applies to
    /// an over-domain bar).
    fn span_rect(g: &TimelineGeom, s: &Span, span_top: u32, span_h: u32) -> Option<Rect> {
        let (lo, hi) = s.extent();
        if !lo.is_finite() || !hi.is_finite() {
            return None;
        }
        let (dom_lo, dom_hi) = g.x.domain();
        // Fully outside the domain -> nothing to draw.
        if hi < dom_lo || lo > dom_hi {
            return None;
        }
        let x0 = g.x.map(lo).clamp(g.left, g.right);
        let x1 = g.x.map(hi).clamp(g.left, g.right);
        let w = (x1 - x0).max(1.0);
        Some(Rect::new(to_u32(x0), span_top, to_u32(w), span_h))
    }

    /// The playhead's pixel x, or `None` when there is no playhead / the plot is
    /// degenerate. The fraction across the timeline rect maps to a pixel clamped
    /// into the plot — the same fraction-to-cursor resolve the charts' inspect
    /// uses.
    fn playhead_px(&self, g: &TimelineGeom, rect: Rect) -> Option<f32> {
        let fraction = self.playhead?;
        let span = g.right - g.left;
        if span <= 0.0 {
            return None;
        }
        let cursor = to_f32(rect.x) + fraction.clamp(0.0, 1.0) * to_f32(rect.w);
        Some(cursor.clamp(g.left, g.right))
    }

    /// The `(lane index, span index)` of the FIRST span in each lane whose
    /// interval contains `time` — at most one per lane, in lane order. The
    /// shared resolve the playhead callout and the a11y readout both read, so
    /// the painted tooltip and the screen-reader text can never disagree.
    fn active_at(&self, time: f64) -> Vec<(usize, usize)> {
        self.lanes
            .iter()
            .enumerate()
            .filter_map(|(i, lane)| {
                lane.spans
                    .iter()
                    .position(|s| s.contains(time))
                    .map(|j| (i, j))
            })
            .collect()
    }

    /// The playhead readout callout: the scrubbed time as a header and one row
    /// per lane that has a span at that time (`{lane}  {span label}`). Anchored
    /// at the playhead pixel, flipping left at the right edge like every chart
    /// tooltip.
    fn playhead_callout(&self, g: &TimelineGeom, px: f32, style: &ChartStyle) -> Vec<Scene> {
        let time = g.x.invert(px);
        let rows: Vec<CalloutRow> = self
            .active_at(time)
            .into_iter()
            .map(|(i, j)| CalloutRow {
                text: format!("{}  {}", self.lanes[i].name, self.lanes[i].spans[j].label),
                color: style.tooltip_fg,
                tag: format!("{}.playhead.value.{i}", self.tag_prefix),
            })
            .collect();
        callout(
            px,
            g.right,
            g.top,
            &g.x_format.readout(time),
            format!("{}.playhead.header", self.tag_prefix),
            &rows,
            style,
            format!("{}.playhead.tooltip", self.tag_prefix),
        )
    }

    /// The playhead readout as one line — the scrubbed time plus each active
    /// lane's span (`t = 12 | build: f3 | render: f3`), or `None` when there is
    /// no playhead. A consumer wires this into the scrub control's `WidgetA11y`
    /// node (via `pinion_a11y::described::describedby_region`) so the reading a
    /// sighted user sees in the tooltip reaches a screen reader too — the R1355
    /// parity, now for the timeline. Built from the SAME `active_at` resolve the
    /// painted callout uses.
    #[must_use]
    pub fn playhead_readout(&self, rect: Rect, style: &ChartStyle) -> Option<String> {
        use std::fmt::Write;
        let g = self.geom(rect, style);
        let px = self.playhead_px(&g, rect)?;
        let time = g.x.invert(px);
        let mut out = format!("t = {}", g.x_format.readout(time));
        for (i, j) in self.active_at(time) {
            let _ = write!(
                out,
                " | {}: {}",
                self.lanes[i].name, self.lanes[i].spans[j].label
            );
        }
        Some(out)
    }

    /// The plot geometry, time scale, ruler-tick set, and lane-band height every
    /// span, gridline, and the playhead derive from — the ONE definition of
    /// "where does time `t` / lane `i` sit", so the painted spans and the
    /// playhead readout can never disagree.
    #[allow(
        clippy::cast_precision_loss,
        reason = "the lane count is a small display count, exact in f32"
    )]
    fn geom(&self, rect: Rect, style: &ChartStyle) -> TimelineGeom {
        let (left, right, top, bottom) = plot_rect(rect, style.margin);
        let (x_lo, x_hi) = self.x_domain_resolved();
        let x = LinearScale::new((x_lo, x_hi), (left, right));
        // R1529 — which generator the ruler ticks come from is the axis's
        // kind, exactly as it is for a chart (`crate::plot::axis_ticks`).
        let x_ticks = if self.time_axis {
            time_ticks(x_lo, x_hi, style.x_ticks)
        } else {
            nice_ticks(x_lo, x_hi, style.x_ticks)
        };
        let x_format = if self.time_axis {
            TickFormat::Time
        } else {
            TickFormat::Step(tick_step(&x_ticks))
        };
        let n = self.lanes.len();
        let lane_h = if n > 0 {
            (bottom - top).max(1.0) / n as f32
        } else {
            0.0
        };
        TimelineGeom {
            left,
            right,
            top,
            bottom,
            x,
            x_ticks,
            x_format,
            lane_h,
        }
    }

    /// The final time domain: a pinned domain verbatim, else `[min start, max
    /// end]` across every finite span, falling back to `[0, 1]` when there are
    /// no finite spans (an empty timeline still draws its ruler + lanes).
    fn x_domain_resolved(&self) -> (f64, f64) {
        if let Some(d) = self.x_domain {
            return d;
        }
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for lane in &self.lanes {
            for s in &lane.spans {
                let (s_lo, s_hi) = s.extent();
                if s_lo.is_finite() && s_hi.is_finite() {
                    lo = lo.min(s_lo);
                    hi = hi.max(s_hi);
                }
            }
        }
        if !lo.is_finite() || !hi.is_finite() {
            return (0.0, 1.0);
        }
        if (hi - lo).abs() <= f64::EPSILON {
            hi = lo + 1.0;
        }
        (lo, hi)
    }
}

/// The plot geometry, time scale, ruler-tick set, and lane metrics a timeline
/// body derives everything from. One resolve, shared by the spans, gridlines,
/// and the playhead — so they can never drift apart.
struct TimelineGeom {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    x: LinearScale,
    x_ticks: Vec<f64>,
    x_format: TickFormat,
    lane_h: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_probe::{find, tags, text_of};
    use pinion_core::scene::Rect;

    /// Two lanes, three spans each — abutting so a scrub always lands in one.
    fn two_lanes() -> Vec<Lane> {
        vec![
            Lane::new(
                "build",
                vec![
                    Span::new(0.0, 10.0, "f0"),
                    Span::new(10.0, 20.0, "f1"),
                    Span::new(20.0, 30.0, "f2"),
                ],
            ),
            Lane::new(
                "render",
                vec![
                    Span::new(0.0, 12.0, "f0"),
                    Span::new(12.0, 22.0, "f1"),
                    Span::new(22.0, 30.0, "f2"),
                ],
            ),
        ]
    }

    #[test]
    fn one_span_box_per_span_plus_lanes_ruler_and_gridlines() {
        let scene =
            Timeline::new(two_lanes()).build(Rect::new(0, 0, 500, 300), &ChartStyle::default());
        let t = tags(&scene);
        for i in 0..2 {
            assert!(
                t.contains(&format!("timeline.lane.{i}.label")),
                "lane {i} name"
            );
            for j in 0..3 {
                assert!(
                    t.contains(&format!("timeline.lane.{i}.span.{j}")),
                    "lane {i} span {j} emitted"
                );
            }
            assert!(
                !t.contains(&format!("timeline.lane.{i}.span.3")),
                "no phantom 4th span"
            );
        }
        assert!(t.contains(&"timeline.axis.x".to_string()), "top ruler line");
        assert!(
            t.contains(&"timeline.axis.y".to_string()),
            "left gutter edge"
        );
        assert!(
            t.contains(&"timeline.grid.x.0".to_string()),
            "time gridline"
        );
        assert!(
            t.contains(&"timeline.tick.0".to_string()),
            "ruler time label"
        );
        // A separator above lane 1 (not lane 0).
        assert!(
            t.contains(&"timeline.rule.1".to_string()),
            "separator above lane 1"
        );
        assert!(
            !t.contains(&"timeline.rule.0".to_string()),
            "no rule above the first lane"
        );
    }

    #[test]
    fn a_longer_span_makes_a_wider_box() {
        let scene =
            Timeline::new(two_lanes()).build(Rect::new(0, 0, 500, 300), &ChartStyle::default());
        // Lane 1 (render): f0 spans 0..12, f2 spans 22..30 -> 12 wide vs 8 wide.
        let w = |i: usize, j: usize| {
            let Scene::Box(b) = find(&scene, &format!("timeline.lane.{i}.span.{j}")).unwrap()
            else {
                panic!("span is a box")
            };
            b.rect.w
        };
        assert!(
            w(1, 0) > w(1, 2),
            "the 12-wide span is wider than the 8-wide one"
        );
    }

    #[test]
    fn lanes_stack_vertically_without_overlap() {
        let scene =
            Timeline::new(two_lanes()).build(Rect::new(0, 0, 500, 300), &ChartStyle::default());
        let rect_of = |i: usize| {
            let Scene::Box(b) = find(&scene, &format!("timeline.lane.{i}.span.0")).unwrap() else {
                panic!("span is a box")
            };
            b.rect
        };
        let (r0, r1) = (rect_of(0), rect_of(1));
        assert!(
            r0.y + r0.h <= r1.y,
            "lane 0's band ({}+{}) sits above lane 1's ({})",
            r0.y,
            r0.h,
            r1.y
        );
    }

    #[test]
    fn a_span_colour_override_wins_over_the_lane_palette() {
        let red = Color::rgb(0xE0, 0x40, 0x40);
        let lanes = vec![Lane::new(
            "l",
            vec![
                Span::new(0.0, 5.0, "a"),
                Span::new(5.0, 10.0, "b").with_color(red),
            ],
        )];
        let scene = Timeline::new(lanes).build(Rect::new(0, 0, 400, 200), &ChartStyle::default());
        let fill = |j: usize| {
            let Scene::Box(b) = find(&scene, &format!("timeline.lane.0.span.{j}")).unwrap() else {
                panic!("span is a box")
            };
            b.style.fill
        };
        assert_eq!(fill(1), red, "the overridden span carries its own colour");
        assert_ne!(fill(0), red, "the default span does not");
    }

    #[test]
    fn a_non_finite_span_emits_no_box() {
        let lanes = vec![Lane::new(
            "l",
            vec![Span::new(0.0, 5.0, "ok"), Span::new(f64::NAN, 5.0, "bad")],
        )];
        let scene = Timeline::new(lanes).build(Rect::new(0, 0, 400, 200), &ChartStyle::default());
        let t = tags(&scene);
        assert!(t.contains(&"timeline.lane.0.span.0".to_string()));
        assert!(
            !t.contains(&"timeline.lane.0.span.1".to_string()),
            "no box for a non-finite span"
        );
    }

    #[test]
    fn a_pinned_domain_clips_out_of_window_spans() {
        // Pin [10, 20]; a span entirely at [0, 5] is out of the window.
        let lanes = vec![Lane::new(
            "l",
            vec![
                Span::new(0.0, 5.0, "before"),
                Span::new(12.0, 18.0, "inside"),
            ],
        )];
        let scene = Timeline::new(lanes)
            .with_x_domain(10.0, 20.0)
            .build(Rect::new(0, 0, 400, 200), &ChartStyle::default());
        let t = tags(&scene);
        assert!(
            !t.contains(&"timeline.lane.0.span.0".to_string()),
            "the out-of-window span is clipped away"
        );
        assert!(
            t.contains(&"timeline.lane.0.span.1".to_string()),
            "the in-window span survives"
        );
    }

    #[test]
    fn build_fill_zero_size_is_an_empty_but_tagged_root() {
        let scene = Timeline::new(two_lanes()).build_fill((0, 0), &ChartStyle::default());
        let Scene::Container(root) = &scene else {
            panic!("the fill-parent root is a container")
        };
        assert_eq!(
            root.tag.as_deref(),
            Some("timeline"),
            "the root still measures"
        );
        assert!(root.children.is_empty(), "no body until a size feeds back");
    }

    #[test]
    fn a_pinned_tag_prefix_renames_every_node() {
        let scene = Timeline::new(two_lanes())
            .with_tag_prefix("track")
            .build(Rect::new(0, 0, 500, 300), &ChartStyle::default());
        let t = tags(&scene);
        assert!(t.contains(&"track.lane.0.span.0".to_string()));
        assert!(
            t.iter().all(|s| !s.starts_with("timeline.")),
            "no default prefix leaks"
        );
    }

    #[test]
    fn no_playhead_by_default() {
        let scene =
            Timeline::new(two_lanes()).build(Rect::new(0, 0, 500, 300), &ChartStyle::default());
        assert!(find(&scene, "timeline.playhead").is_none());
        assert!(find(&scene, "timeline.playhead.tooltip").is_none());
    }

    #[test]
    fn playhead_emits_a_line_and_a_per_lane_readout() {
        let scene = Timeline::new(two_lanes())
            .playhead(Some(0.5))
            .build(Rect::new(0, 0, 500, 300), &ChartStyle::default());
        assert!(
            find(&scene, "timeline.playhead").is_some(),
            "the playhead line"
        );
        assert!(
            find(&scene, "timeline.playhead.tooltip").is_some(),
            "the readout box"
        );
        assert!(
            find(&scene, "timeline.playhead.header").is_some(),
            "the time header"
        );
        // Both lanes are busy across the whole window, so both emit a row.
        assert!(
            find(&scene, "timeline.playhead.value.0").is_some(),
            "lane 0 row"
        );
        assert!(
            find(&scene, "timeline.playhead.value.1").is_some(),
            "lane 1 row"
        );
    }

    #[test]
    fn playhead_reads_the_span_under_it_per_lane() {
        // Scrub near the right edge (time ~= 30) -> lane 0 is in f2 (20..30),
        // lane 1 in f2 (22..30). The rows name those spans.
        let scene = Timeline::new(two_lanes())
            .playhead(Some(0.99))
            .build(Rect::new(0, 0, 500, 300), &ChartStyle::default());
        let v0 = text_of(&scene, "timeline.playhead.value.0").expect("lane 0 row");
        assert!(
            v0.contains("build") && v0.contains("f2"),
            "lane 0 reads build/f2: {v0:?}"
        );
    }

    #[test]
    fn playhead_moves_with_the_fraction() {
        // The playhead is a stroke_path whose commands are rebased onto its
        // bbox origin (R1358), so its absolute position lives in the node rect.
        let head_x = |frac: f32| {
            let scene = Timeline::new(two_lanes())
                .playhead(Some(frac))
                .build(Rect::new(0, 0, 500, 300), &ChartStyle::default());
            let Scene::Path(p) = find(&scene, "timeline.playhead").expect("playhead") else {
                panic!("playhead is a path")
            };
            p.rect.x
        };
        assert!(
            head_x(0.1) < head_x(0.9),
            "the playhead moves right as the scrub advances"
        );
    }

    #[test]
    fn playhead_readout_names_the_time_and_active_spans() {
        let timeline = Timeline::new(two_lanes()).playhead(Some(0.99));
        let readout = timeline
            .playhead_readout(Rect::new(0, 0, 500, 300), &ChartStyle::default())
            .expect("a readout when the playhead is set");
        assert!(
            readout.starts_with("t = "),
            "leads with the scrubbed time: {readout:?}"
        );
        assert!(
            readout.contains("build:") && readout.contains("render:"),
            "names both lanes: {readout:?}"
        );
        assert!(readout.contains("f2"), "names the active span: {readout:?}");
        // The painted header agrees with the readout's time (R1355 parity).
        let scene = timeline.build(Rect::new(0, 0, 500, 300), &ChartStyle::default());
        let header = text_of(&scene, "timeline.playhead.header").expect("header");
        assert!(
            readout.contains(header),
            "the readout time equals the painted header: {readout:?} vs {header:?}"
        );
        // No readout when the playhead is off.
        assert!(
            Timeline::new(two_lanes())
                .playhead(None)
                .playhead_readout(Rect::new(0, 0, 500, 300), &ChartStyle::default())
                .is_none(),
            "no readout without a playhead"
        );
    }

    #[test]
    fn playhead_tooltip_flips_left_at_the_right_edge() {
        const W: u32 = 500;
        let scene = Timeline::new(two_lanes())
            .playhead(Some(1.0))
            .build(Rect::new(0, 0, W, 300), &ChartStyle::default());
        let Scene::Box(tip) = find(&scene, "timeline.playhead.tooltip").expect("tooltip") else {
            panic!("tooltip is a box")
        };
        assert!(
            tip.rect.x + tip.rect.w <= W,
            "the tooltip stays within the timeline ({}+{} <= {W})",
            tip.rect.x,
            tip.rect.w
        );
    }

    // ---- R1529: the ruler can read wall-clock time ---------------------

    /// One UTC instant as epoch milliseconds.
    fn at(y: i64, mo: u32, d: u32, h: u32, mi: u32) -> f64 {
        crate::civil::Civil {
            year: y,
            month: mo,
            day: d,
            hour: h,
            minute: mi,
            second: 0,
            milli: 0,
        }
        .to_millis()
    }

    /// A two-hour capture that began at a real instant.
    fn wall_clock_lanes() -> Vec<Lane> {
        let t0 = at(2026, 3, 2, 9, 0);
        vec![Lane::new(
            "build",
            vec![
                Span::new(t0, t0 + 1_800_000.0, "compile"),
                Span::new(t0 + 3_600_000.0, t0 + 7_200_000.0, "link"),
            ],
        )]
    }

    /// ★ The timeline is the crate's most literally time-shaped view, and
    /// its ruler was a plain number line: every tick of a wall-clock capture
    /// printed the same SI-compacted magnitude. Opting in gives clock
    /// labels; leaving it off is unchanged, which is what makes this an
    /// opt-in rather than a reinterpretation of existing spans.
    #[test]
    fn r1529_the_ruler_reads_wall_clock_time_when_asked() {
        let rect = Rect::new(0, 0, 600, 240);
        let style = ChartStyle::default();

        let numeric = Timeline::new(wall_clock_lanes()).build(rect, &style);
        let numeric_labels: Vec<String> = (0..4)
            .filter_map(|k| text_of(&numeric, &format!("timeline.tick.{k}")))
            .map(str::to_string)
            .collect();
        let distinct: std::collections::BTreeSet<&String> = numeric_labels.iter().collect();
        assert_eq!(
            distinct.len(),
            1,
            "the numeric ruler collapses: {numeric_labels:?}"
        );

        let timed = Timeline::new(wall_clock_lanes())
            .time_axis()
            .build(rect, &style);
        let timed_labels: Vec<String> = (0..4)
            .filter_map(|k| text_of(&timed, &format!("timeline.tick.{k}")))
            .map(str::to_string)
            .collect();
        assert_eq!(timed_labels, ["09:00", "09:30", "10:00", "10:30"]);
    }

    /// ★ A readout is not a tick label. The ruler says `09:30` because its
    /// neighbours carry the date; the playhead has no neighbours, so it
    /// takes the full stamp.
    #[test]
    fn r1529_the_playhead_reads_the_full_stamp_not_the_tick_label() {
        let rect = Rect::new(0, 0, 600, 240);
        let style = ChartStyle::default();
        let timed = Timeline::new(wall_clock_lanes())
            .time_axis()
            .playhead(Some(0.0));

        let scene = timed.build(rect, &style);
        let header = text_of(&scene, "timeline.playhead.header").expect("header");
        assert_eq!(header, "2026-03-02 09:00:00");
        assert!(
            header.contains("2026-03-02"),
            "a scrub says which day it is on"
        );

        let readout = timed.playhead_readout(rect, &style).expect("readout");
        assert!(
            readout.starts_with("t = 2026-03-02 09:00:00"),
            "the a11y readout matches the painted one: {readout}"
        );
        assert!(readout.contains("build: compile"), "and names the span");
    }

    /// An offset-from-capture-start timeline is the case the numeric ruler
    /// is RIGHT for, so the default must not move.
    #[test]
    fn r1529_the_numeric_ruler_is_unchanged_by_default() {
        let rect = Rect::new(0, 0, 600, 240);
        let style = ChartStyle::default();
        let scene = Timeline::new(vec![Lane::new(
            "frame",
            vec![Span::new(0.0, 8.0, "sim"), Span::new(8.0, 16.0, "render")],
        )])
        .build(rect, &style);
        let labels: Vec<String> = (0..3)
            .filter_map(|k| text_of(&scene, &format!("timeline.tick.{k}")))
            .map(str::to_string)
            .collect();
        assert_eq!(labels, ["0", "5", "10"], "plain milliseconds stay plain");
    }

    #[test]
    fn an_empty_timeline_still_draws_its_ruler() {
        let scene =
            Timeline::new(Vec::new()).build(Rect::new(0, 0, 400, 200), &ChartStyle::default());
        assert!(
            find(&scene, "timeline.axis.x").is_some(),
            "the ruler survives an empty timeline"
        );
        assert!(
            find(&scene, "timeline.tick.0").is_some(),
            "with time labels"
        );
    }
}
