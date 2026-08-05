//! R1568 — the polar chart (Qt's `QPolarChart`).
//!
//! The consumer of [`crate::polar`]'s coordinate system, and the first chart
//! here whose grid is not a pair of orthogonal rulers: rings at the radial
//! ticks, spokes at the angular ones, and a rim that is the plot's edge.
//!
//! # It takes the SAME series as a cartesian chart
//!
//! A [`Series`] of `(x, y)` is re-read as `(angular value, radial value)`,
//! which is what Qt does too — `QPolarChart` accepts the ordinary
//! `QLineSeries` / `QScatterSeries` and re-plots them. Keeping the datum
//! means a dashboard can hand the same data to a line chart and a polar one
//! and compare, and it means every consumer of [`crate::plot`]'s off-scale
//! reporting keeps working.
//!
//! # The angular axis is REQUIRED, and that is the design
//!
//! Every cartesian builder here auto-scales an axis it was not given.
//! [`PolarChart::new`] will not, because a period is a fact about the
//! *quantity* rather than about the samples — see [`AngularScale`]. Qt
//! auto-scales the angular axis like any other, so a wind rose whose samples
//! span `10 .. 350` silently gets a period of 340 and every bearing means
//! something different.
//!
//! # What follows from periodicity
//!
//! * The series **closes on itself** when the axis closes, derived rather
//!   than asked for. A Qt radar needs the caller to append the first point
//!   again, which puts a duplicate in the data the model does not contain.
//! * The tick at the end of the period is the tick at its start, so a closed
//!   axis emits **one** of them. Qt would draw both, stacking two labels at
//!   12 o'clock.
//! * A sample outside the period is placed and **reported as wrapped**
//!   ([`PolarChart::wrapped`]), where Qt drops it.
//!
//! # Introspection
//!
//! Under `tag_prefix` (default `"chart"`): `chart.bg`, `chart.ring.{k}` (a
//! radial gridline), `chart.spoke.{k}` (an angular one), `chart.rim` (the
//! outer circle, this chart's `axis`), `chart.series.{i}` (the polyline,
//! closed or not), `chart.area.{i}` (the filled radar polygon when
//! [`filled`](PolarChart::filled)), `chart.point.{i}.{j}`,
//! `chart.label.r.{k}` (radial tick labels along the origin spoke) and
//! `chart.label.a.{k}` (angular tick labels around the rim), plus the shared
//! legend row and — when [`inspect`](PolarChart::inspect) is set —
//! `chart.inspect.spoke` / `.ring.{i}` / `.tooltip` / `.header` /
//! `.value.{i}`.
//!
//! # Coordinate contract
//!
//! Identical to every other builder here: [`build_fill`](PolarChart::build_fill)
//! is layout-native, [`build`](PolarChart::build) pins to a caller rect.
//! Read the crate-level "Known limitations" — `Scene::Path` does not render
//! on TUI, and a polar chart is entirely paths.

use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, PathNode, Rect};
use pinion_core::style::{Color, PathStyle, Stroke, TextAlign};

use crate::draw::{
    CalloutRow, absolute, box_node, callout, circle_commands, fill_parent, label_node, legend_row,
    marker_node, plot_rect, polygon_node, stroke_path, to_f32, to_u32,
};
use crate::palette::CategoricalPalette;
use crate::plot::{OffScale, axis_domain, axis_format, axis_scale, axis_ticks, kind_extent};
use crate::polar::{AngularScale, PolarPlot, Winding};
use crate::scale::{AxisKind, Categories, DEFAULT_LOG_BASE, index_value};
use crate::series::{DataPoint, Series};
use crate::style::ChartStyle;
use crate::ticks::{TickFormat, nice_ticks, tick_step};

/// How much of the smaller half-dimension the plot circle occupies, leaving
/// the rest for the angular tick labels that sit outside the rim.
const RADIUS_FRAC: f32 = 0.82;

/// Width (px) of an angular tick label's box. The labels sit around a
/// circle, so unlike a cartesian axis's row they cannot be packed — each gets
/// a fixed slot centred on its spoke.
const ANGLE_LABEL_SLOT: u32 = 56;

/// A polar chart: [`Series`] re-read as `(angular value, radial value)` over
/// a required [`AngularScale`] and a derived radial axis.
#[derive(Debug, Clone)]
pub struct PolarChart {
    series: Vec<Series>,
    angular: AngularScale,
    palette: CategoricalPalette,
    radial_domain: Option<(f64, f64)>,
    radial_kind: AxisKind,
    angular_labels: Option<Categories>,
    filled: bool,
    markers: bool,
    inspect: Option<f64>,
    tag_prefix: String,
}

impl PolarChart {
    /// A polar chart over `series` on `angular`, with an auto linear radial
    /// axis, no area fill, point markers on, no inspect overlay, and the
    /// `"chart"` tag prefix.
    ///
    /// **The angular axis is required.** See the module doc: a period cannot
    /// be inferred from samples, and Qt inferring one is how a wind rose
    /// silently gets a 340-degree compass.
    #[must_use]
    pub fn new(series: Vec<Series>, angular: AngularScale) -> Self {
        Self {
            series,
            angular,
            palette: CategoricalPalette::default(),
            radial_domain: None,
            radial_kind: AxisKind::Linear,
            angular_labels: None,
            filled: false,
            markers: true,
            inspect: None,
            tag_prefix: "chart".to_string(),
        }
    }

    /// A radar chart over `categories`: one spoke per category, in order, on
    /// a full turn.
    ///
    /// The commonest polar form, and the one whose angular axis is nominal
    /// rather than numeric — sample `x` values are category **indices**, the
    /// convention [`CategoryScale`](crate::CategoryScale) already sets for a
    /// cartesian slot axis. The period is `0 .. n`, so category `i` sits at
    /// `i / n` of the turn and the loop closes between the last and the
    /// first, which is exactly the segment a Qt radar needs a duplicated
    /// datum to draw.
    #[must_use]
    pub fn radar(series: Vec<Series>, categories: Categories) -> Self {
        let period = (0.0, index_value(categories.len().max(1)));
        let mut chart = Self::new(series, AngularScale::new(period));
        chart.angular_labels = Some(categories);
        chart.filled = true;
        chart
    }

    /// The series this chart was built with.
    #[must_use]
    pub fn series(&self) -> &[Series] {
        &self.series
    }

    /// This chart's angular axis.
    #[must_use]
    pub const fn angular(&self) -> &AngularScale {
        &self.angular
    }

    /// Label the angular ticks with these names instead of numbers — one per
    /// slot, the [`radar`](Self::radar) shape applied to an axis built by
    /// hand.
    #[must_use]
    pub fn with_angular_labels(mut self, categories: Categories) -> Self {
        self.angular_labels = Some(categories);
        self
    }

    /// Occupy `sweep` radians rather than a full turn — a gauge, a fan, a
    /// quarter plot. `QPolarChart` is always a full circle.
    ///
    /// A chart-level builder rather than something a caller reaches by
    /// handing in a whole replacement axis, and the split is the round's own
    /// argument turned on itself: the **period** cannot be inferred and so
    /// belongs to the axis at construction, while the sweep and the winding
    /// are presentation declarations that must not be able to corrupt it. An
    /// `with_angular(AngularScale)` setter would let a caller re-seat a radar
    /// on a period its five labels do not describe — two descriptions of the
    /// same thing, which is exactly what this chart exists not to have.
    #[must_use]
    pub fn with_sweep(mut self, sweep: f32) -> Self {
        self.angular = self.angular.with_sweep(sweep);
        self
    }

    /// Set which way values increase. `QPolarChart` hard-codes clockwise.
    #[must_use]
    pub fn with_winding(mut self, winding: Winding) -> Self {
        self.angular = self.angular.with_winding(winding);
        self
    }

    /// Override the default series palette.
    #[must_use]
    pub fn with_palette(mut self, palette: CategoricalPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Pin the radial domain instead of deriving it from the data.
    #[must_use]
    pub fn with_radial_domain(mut self, lo: f64, hi: f64) -> Self {
        self.radial_domain = Some((lo, hi));
        self
    }

    /// Make the radial axis logarithmic at the default base 10 — R1528's
    /// [`LogScale`](crate::LogScale), reached through the shared resolver
    /// rather than reimplemented, because a radial axis IS a value axis on a
    /// pixel range.
    ///
    /// `QPolarChart` supports a `QLogValueAxis` here too, so this is parity
    /// rather than divergence — but a radial value the axis cannot place is
    /// reported by [`off_scale`](Self::off_scale) rather than dropped
    /// silently.
    #[must_use]
    pub fn radial_log(self) -> Self {
        self.radial_log_base(DEFAULT_LOG_BASE)
    }

    /// [`radial_log`](Self::radial_log) at an explicit base.
    #[must_use]
    pub fn radial_log_base(mut self, base: f64) -> Self {
        self.radial_kind = AxisKind::Log(base);
        self
    }

    /// Fill each series' polygon (the radar form) as well as stroking it.
    #[must_use]
    pub const fn filled(mut self, filled: bool) -> Self {
        self.filled = filled;
        self
    }

    /// Draw a mark at every sample (on by default).
    #[must_use]
    pub const fn markers(mut self, markers: bool) -> Self {
        self.markers = markers;
        self
    }

    /// Show the inspect overlay at an **angular value**.
    ///
    /// Deliberately not the `0.0 ..= 1.0` cursor fraction every cartesian
    /// chart here takes: a fraction across the width addresses a vertical
    /// slice, and a polar plot has no vertical slices. The natural address on
    /// a circle is a bearing, so that is the argument — and it composes with
    /// the axis, which means a value outside the period scrubs to its wrapped
    /// twin instead of missing.
    #[must_use]
    pub const fn inspect(mut self, angular: Option<f64>) -> Self {
        self.inspect = angular;
        self
    }

    /// Override the intent/introspection tag prefix (default `"chart"`).
    #[must_use]
    pub fn with_tag_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.tag_prefix = prefix.into();
        self
    }

    /// Every sample this chart's axes cannot place, in series order then
    /// point order.
    ///
    /// On a **closed** angular axis no sample is ever off-scale for its
    /// bearing — that is what periodicity means — so a non-empty report here
    /// names either a radial value the axis cannot carry (a non-positive one
    /// on a log radius) or a bearing outside a **sector**.
    #[must_use]
    pub fn off_scale(&self) -> Vec<OffScale> {
        let mut out = Vec::new();
        for (i, s) in self.series.iter().enumerate() {
            if !s.visible {
                continue;
            }
            for p in &s.points {
                if !self.angular.defines(p.x) || !self.radial_kind.defines(p.y) {
                    out.push(OffScale {
                        series: i,
                        point: *p,
                    });
                }
            }
        }
        out
    }

    /// Every sample the angular axis carried by **wrapping** it — placed, and
    /// outside the period as given.
    ///
    /// The report a periodic axis owes its caller. Qt cannot answer it: such
    /// a value is simply not drawn there, and a consumer that pre-wrapped its
    /// own data has already destroyed the evidence.
    #[must_use]
    pub fn wrapped(&self) -> Vec<OffScale> {
        let mut out = Vec::new();
        for (i, s) in self.series.iter().enumerate() {
            if !s.visible {
                continue;
            }
            for p in &s.points {
                if self.angular.wrapped(p.x) {
                    out.push(OffScale {
                        series: i,
                        point: *p,
                    });
                }
            }
        }
        out
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
        let plot = self.plot(rect, style);
        let radial_ticks = axis_ticks(&plot.radial, style.y_ticks);
        let angular_ticks = self.angular_ticks(style);

        let mut children: Vec<Scene> = Vec::new();
        if let Some(bg) = style.background {
            children.push(box_node(rect, bg, format!("{}.bg", self.tag_prefix)));
        }
        children.extend(self.grid(&plot, &radial_ticks, &angular_ticks, style));
        children.extend(self.marks(&plot, style));
        children.extend(self.tick_labels(&plot, &radial_ticks, &angular_ticks, rect, style));
        if style.legend {
            children.extend(self.legend(rect, style));
        }
        children.extend(self.overlay(&plot, &radial_ticks, style));

        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
    }

    /// The rings, the spokes and the rim.
    fn grid(
        &self,
        plot: &PolarPlot,
        radial_ticks: &[f64],
        angular_ticks: &[f64],
        style: &ChartStyle,
    ) -> Vec<Scene> {
        let mut out = Vec::new();
        let grid = Stroke::new(style.grid, 1);
        for (k, &t) in radial_ticks.iter().enumerate() {
            let Some(r) = plot.radial.map(t) else {
                continue;
            };
            if r <= 0.0 {
                continue;
            }
            out.push(ring_node(
                plot.cx,
                plot.cy,
                r,
                grid,
                format!("{}.ring.{k}", self.tag_prefix),
            ));
        }
        for (k, &t) in angular_ticks.iter().enumerate() {
            let Some(angle) = self.angular.angle(t) else {
                continue;
            };
            let end = plot.at(angle, plot.radius);
            out.push(stroke_path(
                &[(plot.cx, plot.cy), end],
                grid,
                format!("{}.spoke.{k}", self.tag_prefix),
            ));
        }
        // The rim is this chart's axis line: on a closed axis it is the whole
        // circle, on a sector the two bounding spokes are already drawn by the
        // tick loop above and the rim is the arc between them — which the ring
        // primitive cannot express, so a sector draws no rim rather than a
        // full circle that would claim angles the axis does not carry.
        if self.angular.closes() {
            out.push(ring_node(
                plot.cx,
                plot.cy,
                plot.radius,
                Stroke::new(style.axis, 1),
                format!("{}.rim", self.tag_prefix),
            ));
        }
        out
    }

    /// Each visible series' polygon, its optional fill, and its point marks.
    fn marks(&self, plot: &PolarPlot, style: &ChartStyle) -> Vec<Scene> {
        let mut out = Vec::new();
        for (i, s) in self.series.iter().enumerate() {
            if !s.visible {
                continue;
            }
            let color = s.color.unwrap_or_else(|| self.palette.color(i));
            let pixels: Vec<(f32, f32)> =
                s.points.iter().filter_map(|p| plot.map(p.x, p.y)).collect();
            if pixels.len() >= 2 {
                // THE ROUND, in one boolean: the segment from the last sample
                // back to the first is a property of the AXIS, not of the
                // data. A Qt radar gets it by appending the first point again.
                let closed = self.angular.closes();
                if self.filled {
                    out.push(polygon_node(
                        &pixels,
                        PathStyle::filled(color.with_alpha(style.area_alpha)),
                        format!("{}.area.{i}", self.tag_prefix),
                    ));
                }
                out.push(series_path(
                    &pixels,
                    closed,
                    Stroke::new(color, style.series_width.max(1)),
                    format!("{}.series.{i}", self.tag_prefix),
                ));
            }
            if self.markers {
                for (j, p) in s.points.iter().enumerate() {
                    let Some((x, y)) = plot.map(p.x, p.y) else {
                        continue;
                    };
                    out.push(marker_node(
                        x,
                        y,
                        style.marker_radius,
                        color,
                        format!("{}.point.{i}.{j}", self.tag_prefix),
                    ));
                }
            }
        }
        out
    }

    /// Radial tick labels along the origin spoke, angular ones around the rim.
    fn tick_labels(
        &self,
        plot: &PolarPlot,
        radial_ticks: &[f64],
        angular_ticks: &[f64],
        rect: Rect,
        style: &ChartStyle,
    ) -> Vec<Scene> {
        let size = style.label_size_px.max(1);
        let mut out = Vec::new();
        let radial_format = axis_format(&plot.radial, radial_ticks);
        // Along the origin spoke, just clear of it, so the numbers read
        // against the rings they name.
        for (k, &t) in radial_ticks.iter().enumerate() {
            let Some(r) = plot.radial.map(t) else {
                continue;
            };
            let (x, y) = plot.at(self.angular.origin(), r);
            out.push(label_node(
                radial_format.label(t),
                to_u32(x + 4.0),
                to_u32(y - to_f32(size)),
                ANGLE_LABEL_SLOT,
                TextAlign::Start,
                style.label,
                size,
                format!("{}.label.r.{k}", self.tag_prefix),
            ));
        }
        let angular_format = self.angular_format(angular_ticks);
        // Outside the rim, centred on each spoke. The box is clamped inside
        // the chart rect for `centered_label_x`'s reason — a label at 3
        // o'clock would otherwise overhang into a docked neighbour.
        let label_r = plot.radius + to_f32(size);
        for (k, &t) in angular_ticks.iter().enumerate() {
            let Some(angle) = self.angular.angle(t) else {
                continue;
            };
            let (x, y) = plot.at(angle, label_r);
            let left = crate::draw::centered_label_x(x, ANGLE_LABEL_SLOT, rect);
            out.push(label_node(
                angular_format.label(t),
                left,
                to_u32(y - to_f32(size) / 2.0),
                ANGLE_LABEL_SLOT,
                TextAlign::Center,
                style.label,
                size,
                format!("{}.label.a.{k}", self.tag_prefix),
            ));
        }
        out
    }

    /// The shared legend row in the top margin band.
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
        legend_row(
            &entries,
            rect.x + style.margin.left,
            rect.y + 6,
            rect.w.saturating_sub(style.margin.left),
            style,
            &self.tag_prefix,
        )
    }

    /// The inspect overlay: the scrubbed spoke, a ring on each series' sample
    /// at that bearing, and a tooltip.
    fn overlay(&self, plot: &PolarPlot, radial_ticks: &[f64], style: &ChartStyle) -> Vec<Scene> {
        let Some(angular) = self.inspect else {
            return Vec::new();
        };
        let Some(angle) = self.angular.angle(angular) else {
            return Vec::new();
        };
        let mut out = vec![stroke_path(
            &[(plot.cx, plot.cy), plot.at(angle, plot.radius)],
            Stroke::new(style.crosshair, 1),
            format!("{}.inspect.spoke", self.tag_prefix),
        )];
        let mut rows = Vec::new();
        let radial_format = axis_format(&plot.radial, radial_ticks);
        for (i, s) in self.series.iter().enumerate() {
            if !s.visible {
                continue;
            }
            let Some(p) = self.nearest_in(s, angular) else {
                continue;
            };
            let color = s.color.unwrap_or_else(|| self.palette.color(i));
            if let Some((x, y)) = plot.map(p.x, p.y) {
                out.push(ring_node(
                    x,
                    y,
                    to_f32(style.marker_radius) + 2.0,
                    Stroke::new(color, 2),
                    format!("{}.inspect.ring.{i}", self.tag_prefix),
                ));
            }
            rows.push(CalloutRow {
                text: format!("{}  {}", s.name, radial_format.readout(p.y)),
                color,
                tag: format!("{}.inspect.value.{i}", self.tag_prefix),
            });
        }
        let header = self.angular_format(&self.angular_ticks(style)).readout(
            self.angular
                .value_at(self.angular.fraction(angular).unwrap_or(0.0)),
        );
        out.extend(callout(
            plot.cx,
            plot.cx + plot.radius,
            plot.cy - plot.radius,
            &header,
            format!("{}.inspect.header", self.tag_prefix),
            &rows,
            style,
            format!("{}.inspect.tooltip", self.tag_prefix),
        ));
        out
    }

    /// The sample of `series` whose bearing is closest to `angular`, measured
    /// **around the circle** rather than along a line.
    ///
    /// The distinction is the axis's: on a closed period the samples at 350
    /// and at 10 are twenty degrees apart, and a `|a - b|` nearest — the
    /// cartesian one every other chart here uses — would call them 340.
    fn nearest_in(&self, series: &Series, angular: f64) -> Option<DataPoint> {
        let target = self.angular.fraction(angular)?;
        let closed = self.angular.closes();
        series
            .points
            .iter()
            .filter_map(|p| {
                let t = self.angular.fraction(p.x)?;
                let raw = (t - target).abs();
                // Round the short way when the axis closes.
                let d = if closed { raw.min(1.0 - raw) } else { raw };
                Some((*p, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(p, _)| p)
    }

    /// The inspect readout as one line — the same facts the tooltip paints,
    /// stated for a screen reader. `None` when nothing is scrubbed.
    ///
    /// Names the bearing it scrubbed and every series' value there. Qt's
    /// charts implement no accessibility interface at all, so a Qt polar
    /// chart announces nothing.
    #[must_use]
    pub fn inspect_readout(&self, rect: Rect, style: &ChartStyle) -> Option<String> {
        let angular = self.inspect?;
        let plot = self.plot(rect, style);
        let ticks = axis_ticks(&plot.radial, style.y_ticks);
        let radial_format = axis_format(&plot.radial, &ticks);
        let fraction = self.angular.fraction(angular)?;
        let head = self
            .angular_format(&self.angular_ticks(style))
            .readout(self.angular.value_at(fraction));
        let body: Vec<String> = self
            .series
            .iter()
            .filter(|s| s.visible)
            .filter_map(|s| {
                let p = self.nearest_in(s, angular)?;
                Some(format!("{} {}", s.name, radial_format.readout(p.y)))
            })
            .collect();
        Some(format!("{head}: {}", body.join(", ")))
    }

    /// The angular tick VALUES.
    ///
    /// With category labels, one per slot. Otherwise nice numbers over the
    /// period — **with the duplicate endpoint dropped when the axis closes**,
    /// because the tick at the end of a period is the tick at its start and
    /// two labels would stack at the origin spoke.
    fn angular_ticks(&self, style: &ChartStyle) -> Vec<f64> {
        if let Some(cats) = &self.angular_labels {
            return (0..cats.len()).map(index_value).collect();
        }
        let (lo, hi) = self.angular.period();
        let mut ticks: Vec<f64> = nice_ticks(lo, hi, style.x_ticks)
            .into_iter()
            .filter(|t| *t >= lo && *t <= hi)
            .collect();
        if self.angular.closes()
            && ticks.len() > 1
            && let (Some(&first), Some(&last)) = (ticks.first(), ticks.last())
            && (last - first - self.angular.span()).abs() < f64::EPSILON
        {
            ticks.pop();
        }
        ticks
    }

    /// How an angular tick is labelled — its category name, or the number.
    fn angular_format(&self, ticks: &[f64]) -> TickFormat {
        match &self.angular_labels {
            Some(cats) => TickFormat::Category(cats.clone()),
            None => TickFormat::Step(tick_step(ticks)),
        }
    }

    /// The plot frame: centre, radius, and both axes. The ONE definition the
    /// grid, the marks and the inspect hit-test read.
    fn plot(&self, rect: Rect, style: &ChartStyle) -> PolarPlot {
        let (left, right, top, bottom) = plot_rect(rect, style.margin);
        let cx = f32::midpoint(left, right);
        let cy = f32::midpoint(top, bottom);
        let radius = ((right - left).min(bottom - top) / 2.0 * RADIUS_FRAC).max(1.0);

        // The radial axis measures the visible samples' y — the value axis of
        // a cartesian chart, on a pixel range that runs from the centre out.
        let measured = self.radial_extent();
        let raw = self
            .radial_domain
            .or(measured)
            .unwrap_or_else(|| kind_extent(&self.radial_kind));
        let dom = axis_domain(self.radial_domain, raw, style.y_ticks, &self.radial_kind);
        PolarPlot {
            cx,
            cy,
            radius,
            angular: self.angular,
            radial: axis_scale(dom, (0.0, radius), &self.radial_kind),
        }
    }

    /// The radial extent of the visible samples, `None` when there are none.
    ///
    /// A linear radial axis starts at zero rather than at the data's minimum:
    /// a radius encodes magnitude as a distance from the centre, so a
    /// non-zero centre makes a ring's area lie about its value — the same
    /// argument that denies [`BarChart`](crate::BarChart) a log axis, applied
    /// to the other end. A pinned domain overrides it, and a log radius takes
    /// only the positive part (R1528's rule).
    fn radial_extent(&self) -> Option<(f64, f64)> {
        let mut span: Option<(f64, f64)> = None;
        for s in self.series.iter().filter(|s| s.visible) {
            for p in &s.points {
                if !self.radial_kind.defines(p.y) {
                    continue;
                }
                span = Some(match span {
                    Some((lo, hi)) => (lo.min(p.y), hi.max(p.y)),
                    None => (p.y, p.y),
                });
            }
        }
        let (lo, hi) = span?;
        match self.radial_kind {
            AxisKind::Log(_) => Some((lo, hi)),
            AxisKind::Linear | AxisKind::Time | AxisKind::Category(_) => Some((lo.min(0.0), hi)),
        }
    }
}

/// A stroked circle of `r` px about `(cx, cy)` — a grid ring, the rim, or an
/// inspect halo. The [`circle_commands`] geometry the line and scatter
/// markers already share, stroked rather than filled.
fn ring_node(cx: f32, cy: f32, r: f32, stroke: Stroke, tag: String) -> Scene {
    let bbox = Rect::new(
        to_u32(cx - r - to_f32(stroke.width)),
        to_u32(cy - r - to_f32(stroke.width)),
        to_u32(r.mul_add(2.0, to_f32(stroke.width) * 2.0)).max(1),
        to_u32(r.mul_add(2.0, to_f32(stroke.width) * 2.0)).max(1),
    );
    let ox = to_f32(bbox.x);
    let oy = to_f32(bbox.y);
    Scene::Path(
        PathNode::new(
            bbox,
            circle_commands(cx - ox, cy - oy, r),
            PathStyle::stroked(stroke),
        )
        .with_tag(tag)
        .with_layout(absolute(bbox)),
    )
}

/// A series polyline, closed back to its first sample when the axis closes.
///
/// [`polygon_node`] always closes and [`stroke_path`] never does, so neither
/// expresses "closed iff the axis is periodic" — which is the fact this chart
/// exists to carry.
fn series_path(pixels: &[(f32, f32)], closed: bool, stroke: Stroke, tag: String) -> Scene {
    if closed {
        return polygon_node(pixels, PathStyle::stroked(stroke), tag);
    }
    stroke_path(pixels, stroke, tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_probe::{count_prefix, find, has, tags, text_of};
    use core::f32::consts::PI;

    const RECT: Rect = Rect::new(0, 0, 480, 480);

    fn wind() -> Vec<Series> {
        vec![Series::new(
            "gusts",
            vec![
                DataPoint::new(0.0, 4.0),
                DataPoint::new(90.0, 9.0),
                DataPoint::new(180.0, 2.0),
                DataPoint::new(270.0, 6.0),
            ],
        )]
    }

    fn compass_chart() -> PolarChart {
        PolarChart::new(wind(), AngularScale::new((0.0, 360.0)))
    }

    /// ★ The round's claim in the paint: on a CLOSED axis the series polygon
    /// carries the segment from the last sample back to the first, and on a
    /// sector it does not. A Qt radar gets that segment by appending the
    /// first point again — a duplicate in the data the model does not have.
    #[test]
    fn r1568_a_closed_axis_closes_the_series_by_derivation() {
        let style = ChartStyle::default();
        let scene = compass_chart().build(RECT, &style);
        let Some(Scene::Path(p)) = find(&scene, "chart.series.0") else {
            panic!("the series is a path")
        };
        assert_eq!(
            p.commands.len(),
            5,
            "four samples and a Close — the loop is derived, not supplied"
        );

        // Counterfactual: the SAME four samples on a sector do not close.
        let sector = PolarChart::new(wind(), AngularScale::new((0.0, 360.0)).with_sweep(PI))
            .build(RECT, &style);
        let Some(Scene::Path(p)) = find(&sector, "chart.series.0") else {
            panic!("the series is a path")
        };
        assert_eq!(p.commands.len(), 4, "four samples, no Close");
    }

    /// ★ A bearing outside the period is PLACED and reported as wrapped, not
    /// dropped. Qt's angular axis is an ordinary `QValueAxis`, so 370 there
    /// is out of range and draws nothing.
    #[test]
    fn r1568_a_wrapped_bearing_is_placed_and_reported() {
        let style = ChartStyle::default();
        let mut s = wind();
        s[0].points.push(DataPoint::new(370.0, 7.0));
        let chart = PolarChart::new(s, AngularScale::new((0.0, 360.0)));

        let scene = chart.build(RECT, &style);
        assert!(
            has(&scene, "chart.point.0.4"),
            "the wrapped sample is drawn"
        );
        assert!(chart.off_scale().is_empty(), "and it is not off-scale");

        let wrapped = chart.wrapped();
        assert_eq!(wrapped.len(), 1, "{wrapped:?}");
        assert!(
            (wrapped[0].point.x - 370.0).abs() < 1e-9,
            "reported verbatim"
        );

        // ...and it lands where 10 degrees lands.
        let plot = chart.plot(RECT, &style);
        let a = plot.map(370.0, 7.0).expect("placed");
        let b = plot.map(10.0, 7.0).expect("placed");
        assert!(
            (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01,
            "{a:?} {b:?}"
        );
    }

    /// ★ On a SECTOR the same bearing is genuinely off-scale, so wrapping is
    /// a property of the sweep rather than of the value.
    #[test]
    fn r1568_a_sector_reports_the_same_bearing_as_off_scale() {
        let mut s = wind();
        s[0].points.push(DataPoint::new(370.0, 7.0));
        let chart = PolarChart::new(s, AngularScale::new((0.0, 360.0)).with_sweep(PI));
        assert_eq!(chart.wrapped().len(), 0, "a sector wraps nothing");
        let off = chart.off_scale();
        assert_eq!(off.len(), 1, "{off:?}");
        assert!((off[0].point.x - 370.0).abs() < 1e-9);
        assert!(
            !has(
                &chart.build(RECT, &ChartStyle::default()),
                "chart.point.0.4"
            ),
            "and it draws no mark"
        );
    }

    /// ★ A closed axis emits ONE tick where the period's two ends meet. Qt
    /// would draw both and stack two labels at 12 o'clock.
    ///
    /// The period is `0 .. 300` and NOT the compass's `0 .. 360`, and that is
    /// the test's own correction: `nice_ticks` never lands on 360 for a
    /// six-tick target, so asserting "360 is not a tick" there passes whether
    /// or not the seam is de-duplicated — a vacuous assertion the first draft
    /// shipped. A 300 period puts a tick on both ends, so the drop is
    /// observable.
    #[test]
    fn r1568_a_closed_axis_does_not_label_its_seam_twice() {
        let style = ChartStyle::default();
        let seamed = |scale: AngularScale| PolarChart::new(wind(), scale).angular_ticks(&style);

        let closed = seamed(AngularScale::new((0.0, 300.0)));
        assert!(closed.contains(&0.0), "{closed:?}");
        assert!(
            !closed.contains(&300.0),
            "300 IS 0 on a closed axis and must not be a second tick: {closed:?}"
        );

        // Counterfactual: the same period on a SECTOR keeps both ends,
        // because there they are two places.
        let sector = seamed(AngularScale::new((0.0, 300.0)).with_sweep(PI));
        assert!(
            sector.contains(&0.0) && sector.contains(&300.0),
            "{sector:?}"
        );
        assert_eq!(
            sector.len(),
            closed.len() + 1,
            "exactly one tick separates them: {closed:?} vs {sector:?}"
        );

        // The visible consequence, on the compass: no two angular labels ever
        // land on the same spoke.
        let chart = compass_chart();
        let scene = chart.build(RECT, &style);
        let count = count_prefix(&scene, "chart.label.a.");
        assert_eq!(
            count,
            chart.angular_ticks(&style).len(),
            "one label per tick"
        );
        let mut seen: Vec<(u32, u32)> = Vec::new();
        for k in 0..count {
            let Some(Scene::Text(t)) = find(&scene, &format!("chart.label.a.{k}")) else {
                panic!("label {k}")
            };
            let at = t.layout.absolute_position.expect("placed");
            assert!(!seen.contains(&at), "two labels stacked at {at:?}");
            seen.push(at);
        }
    }

    /// ★ The radar form: one spoke per category, named, and the polygon
    /// closes between the last category and the first.
    #[test]
    fn r1568_a_radar_names_its_spokes_and_closes_its_loop() {
        let cats = Categories::new(["speed", "range", "armour", "cost", "crew"]);
        let series = vec![Series::new(
            "prototype",
            (0..5)
                .map(|i| DataPoint::new(index_value(i), 3.0 + index_value(i % 3)))
                .collect(),
        )];
        let chart = PolarChart::radar(series, cats);
        assert!(
            (chart.angular().span() - 5.0).abs() < 1e-9,
            "period is 0..n"
        );
        assert!(chart.angular().closes());

        let scene = chart.build(RECT, &ChartStyle::default());
        assert_eq!(count_prefix(&scene, "chart.spoke."), 5, "one per category");
        assert_eq!(text_of(&scene, "chart.label.a.0"), Some("speed"));
        assert_eq!(text_of(&scene, "chart.label.a.4"), Some("crew"));
        assert!(has(&scene, "chart.area.0"), "a radar is filled by default");
        let Some(Scene::Path(p)) = find(&scene, "chart.series.0") else {
            panic!("the series is a path")
        };
        assert_eq!(p.commands.len(), 6, "five vertices and a Close");
    }

    /// ★ The nearest-sample hit-test measures AROUND the circle. At 355
    /// degrees the sample at 0 is five degrees away and the one at 270 is
    /// eighty-five; a `|a - b|` nearest — the cartesian one — would answer
    /// 270.
    #[test]
    fn r1568_the_hit_test_measures_the_short_way_round() {
        let chart = compass_chart();
        let picked = chart
            .nearest_in(&chart.series()[0], 355.0)
            .expect("a sample");
        assert!(
            (picked.x - 0.0).abs() < 1e-9,
            "expected the sample at 0, got {}",
            picked.x
        );

        // Counterfactual: on a SECTOR the short way round does not exist, and
        // the linear answer is the right one.
        let sector = PolarChart::new(wind(), AngularScale::new((0.0, 360.0)).with_sweep(PI));
        let picked = sector
            .nearest_in(&sector.series()[0], 355.0)
            .expect("a sample");
        assert!((picked.x - 270.0).abs() < 1e-9, "got {}", picked.x);
    }

    /// ★ A linear radial axis starts at the centre with zero: a radius
    /// encodes magnitude as a distance, so a non-zero centre would make a
    /// ring lie about its value.
    #[test]
    fn r1568_a_linear_radius_starts_at_zero() {
        let style = ChartStyle::default();
        let series = vec![Series::new(
            "offset",
            vec![DataPoint::new(0.0, 100.0), DataPoint::new(180.0, 120.0)],
        )];
        let chart = PolarChart::new(series, AngularScale::new((0.0, 360.0)));
        let (lo, _) = chart.plot(RECT, &style).radial.domain();
        assert!(lo.abs() < f64::EPSILON, "the centre is zero, got {lo}");

        // A pinned domain overrides it, which is how a caller asks for the
        // zoomed reading deliberately.
        let pinned = PolarChart::new(
            vec![Series::new(
                "offset",
                vec![DataPoint::new(0.0, 100.0), DataPoint::new(180.0, 120.0)],
            )],
            AngularScale::new((0.0, 360.0)),
        )
        .with_radial_domain(90.0, 130.0);
        assert!((pinned.plot(RECT, &style).radial.domain().0 - 90.0).abs() < 1e-9);
    }

    /// ★ A radial value a LOG axis cannot place draws no mark and is
    /// reported — R1528's stance, on the radial axis.
    #[test]
    fn r1568_a_radial_value_off_a_log_axis_is_reported() {
        let series = vec![Series::new(
            "latency",
            vec![
                DataPoint::new(0.0, 0.0),
                DataPoint::new(120.0, 10.0),
                DataPoint::new(240.0, 100.0),
            ],
        )];
        let chart = PolarChart::new(series.clone(), AngularScale::new((0.0, 360.0))).radial_log();
        let off = chart.off_scale();
        assert_eq!(off.len(), 1, "only the zero: {off:?}");
        assert!(off[0].point.y.abs() < f64::EPSILON);
        let scene = chart.build(RECT, &ChartStyle::default());
        assert!(!has(&scene, "chart.point.0.0"), "the zero draws nothing");
        assert!(has(&scene, "chart.point.0.1"), "the others are unaffected");

        // Counterfactual: on a linear radius the same sample is ordinary.
        let linear = PolarChart::new(series, AngularScale::new((0.0, 360.0)));
        assert!(linear.off_scale().is_empty());
        assert!(has(
            &linear.build(RECT, &ChartStyle::default()),
            "chart.point.0.0"
        ));
    }

    /// ★ The grid is rings and spokes, and the rim exists only where the axis
    /// closes — a full circle on a sector would claim angles the axis does
    /// not carry.
    #[test]
    fn r1568_the_grid_is_rings_and_spokes_and_the_rim_follows_the_axis() {
        let style = ChartStyle::default();
        let scene = compass_chart().build(RECT, &style);
        assert!(count_prefix(&scene, "chart.ring.") > 0);
        assert!(count_prefix(&scene, "chart.spoke.") > 0);
        assert!(has(&scene, "chart.rim"));

        let sector = PolarChart::new(wind(), AngularScale::new((0.0, 360.0)).with_sweep(PI))
            .build(RECT, &style);
        assert!(
            !has(&sector, "chart.rim"),
            "a sector draws no full-circle rim"
        );
        assert!(
            count_prefix(&sector, "chart.spoke.") > 0,
            "but keeps spokes"
        );
    }

    /// ★ The winding is a declaration, and it reaches the paint: the same
    /// bearing lands on opposite sides of the vertical.
    #[test]
    fn r1568_the_winding_reaches_the_painted_position() {
        let style = ChartStyle::default();
        let compass = compass_chart();
        let mathematical = PolarChart::new(wind(), AngularScale::new((0.0, 360.0)).mathematical());
        assert_eq!(mathematical.angular().winding(), Winding::CounterClockwise);

        let clockwise = compass.plot(RECT, &style);
        let widdershins = mathematical.plot(RECT, &style);
        let a = clockwise.map(90.0, 9.0).expect("placed");
        let b = widdershins.map(90.0, 9.0).expect("placed");
        assert!(
            a.0 > clockwise.cx,
            "clockwise 90 is to the RIGHT of centre: {a:?}"
        );
        assert!(b.1 < clockwise.cy, "the other convention moves it: {b:?}");
        assert!((a.0 - b.0).abs() > 1.0, "{a:?} vs {b:?}");
    }

    /// ★ The scrub names the bearing and every series' value there, and the
    /// tooltip and the a11y readout come from one derivation.
    #[test]
    fn r1568_the_scrub_states_the_bearing_and_the_values() {
        let style = ChartStyle::default();
        let chart = compass_chart().inspect(Some(88.0));
        let readout = chart.inspect_readout(RECT, &style).expect("scrubbed");
        assert!(readout.contains("gusts"), "{readout}");

        let scene = chart.build(RECT, &style);
        for tag in [
            "chart.inspect.spoke",
            "chart.inspect.ring.0",
            "chart.inspect.tooltip",
            "chart.inspect.header",
            "chart.inspect.value.0",
        ] {
            assert!(has(&scene, tag), "missing {tag}");
        }

        // A bearing OUTSIDE the period still scrubs, to its wrapped twin.
        let wrapped = compass_chart().inspect(Some(448.0));
        assert!(has(&wrapped.build(RECT, &style), "chart.inspect.spoke"));
        assert_eq!(
            wrapped.inspect_readout(RECT, &style),
            compass_chart()
                .inspect(Some(88.0))
                .inspect_readout(RECT, &style),
            "448 is 88"
        );

        // Counterfactual: with no scrub there is no overlay at all.
        assert_eq!(
            count_prefix(&compass_chart().build(RECT, &style), "chart.inspect."),
            0
        );
    }

    /// ★ An empty chart builds a legible empty plot rather than panicking or
    /// collapsing its axis.
    #[test]
    fn r1568_an_empty_chart_still_builds() {
        let style = ChartStyle::default();
        let empty = PolarChart::new(Vec::new(), AngularScale::new((0.0, 360.0)));
        let scene = empty.build(RECT, &style);
        assert!(has(&scene, "chart.rim"));
        assert_eq!(count_prefix(&scene, "chart.series."), 0);
        assert!(empty.off_scale().is_empty() && empty.wrapped().is_empty());
        assert_eq!(
            tags(&compass_chart().build_fill((0, 0), &style)),
            vec!["chart".to_string()]
        );
        // A hidden series drops its geometry and keeps its legend slot.
        let mut hidden = wind();
        hidden[0].visible = false;
        let scene = PolarChart::new(hidden, AngularScale::new((0.0, 360.0))).build(RECT, &style);
        assert_eq!(count_prefix(&scene, "chart.series."), 0);
        assert!(has(&scene, "chart.legend.0.label"));
    }
}
