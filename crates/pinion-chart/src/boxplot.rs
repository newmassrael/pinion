//! R1553 — the box plot: the crate's first chart whose datum has **extent**.
//!
//! Every other builder here places a datum at a position. A
//! [`Distribution`] occupies a span of the value axis and carries interior
//! landmarks, so one datum emits a box, a median, two whiskers, two caps and
//! a mark per outlier. Tukey's schema (*Exploratory Data Analysis*, 1977),
//! optionally notched after `McGill`, Tukey & Larsen (1978).
//!
//! # What it shares, and what it does not
//!
//! The x-axis is the [`CategoryScale`] R1545 made an axis kind — the same
//! one the bar chart's slots are, so a box plot windows
//! ([`x_window`](BoxPlotChart::x_window)) and hit-tests through the code a
//! bar chart does. The value axis is a full [`ValueScale`] resolved through
//! [`crate::plot`]'s own `axis_domain` / `axis_scale`, so
//! [`y_log`](BoxPlotChart::y_log) is R1528's logarithmic axis rather than a
//! second implementation of one. Unlike the bar chart, a log value axis is
//! *legitimate* here: a bar encodes magnitude as a length from zero and a
//! log axis has no zero, while a box plot encodes five **positions**, which
//! a log axis maps as well as any other. Latency is the case that matters —
//! the distribution a professional tool most often boxes is the one a linear
//! axis flattens.
//!
//! # Introspection
//!
//! Under `tag_prefix` (default `"chart"`), per distribution `i`:
//! `chart.box.{i}` (the box, notched or not), `chart.median.{i}`,
//! `chart.whisker.{i}.lo` / `.hi`, `chart.cap.{i}.lo` / `.hi`, and
//! `chart.outlier.{i}.{j}` — one node per outlier, which is the half of the
//! form the toolkit's five-slot box set cannot represent. The shared cartesian
//! furniture keeps its usual tags (`chart.bg`, `chart.grid.y.{k}`,
//! `chart.grid.minor.y.{k}`, `chart.axis.x` / `.y`, `chart.label.y.{k}`),
//! and the category labels are `chart.xlabel.{i}` — the bar chart's
//! per-CATEGORY cardinality, not line's per-numeric-tick one.
//!
//! A landmark the value axis cannot place emits **no node** and is reported
//! by [`off_scale`](BoxPlotChart::off_scale) — R1528's stance, applied to a
//! datum with extent: a zero lower whisker on a log axis is absent from the
//! picture and named in the report, rather than pinned to the domain floor
//! where it would read as a real measurement.
//!
//! # Coordinate contract
//!
//! Identical to every other builder here: [`build_fill`](BoxPlotChart::build_fill)
//! is layout-native, [`build`](BoxPlotChart::build) pins to a caller rect.
//! Read the crate-level "Known limitations" — `Scene::Path` does not render
//! on TUI, and a box plot is almost entirely paths.

use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{Color, PathStyle, Stroke};

use crate::density::{Density, count_as_f64};
use crate::distribution::{
    Distribution, DistributionSource, SummaryPosition, distribution_bounds,
    positive_distribution_bounds,
};
use crate::draw::{
    CalloutRow, absolute, box_node, callout, category_label_node, fill_parent, marker_node,
    outline_box, plot_rect, polygon_node, stroke_path, to_f32, to_u32,
};
// R1626 — the crate's one f64 -> f32 narrowing, rather than a third copy of it.
use crate::palette::CategoricalPalette;
use crate::plot::{
    axis_domain, axis_format, axis_minor_ticks, axis_scale, axis_ticks, kind_extent, tick_pixels,
};
use crate::scale::to_f32 as narrow_value;
use crate::scale::{
    AxisKind, Categories, CategoryScale, CategoryWindow, DEFAULT_LOG_BASE, ValueScale,
};
use crate::style::ChartStyle;
use crate::ticks::{TickFormat, tick_step};

/// The fraction of a category slot the box occupies. The toolkit's
/// `boxWidth` defaults to `0.5`; a little wider reads better
/// against this crate's gap-free category bands.
const BOX_WIDTH_FRAC: f32 = 0.6;

/// The whisker cap's width as a fraction of the box's — the conventional
/// half-width, which keeps the cap subordinate to the box it terminates.
const CAP_WIDTH_FRAC: f32 = 0.5;

/// How far each side of the box is drawn in at the notch waist, as a
/// fraction of the box width. `0.25` per side leaves the waist half as wide
/// as the box, the proportion R's `boxplot(notch = TRUE)` draws.
const NOTCH_INSET_FRAC: f32 = 0.25;

/// Which landmark of a distribution a value is — the addressing an
/// off-scale report needs, since an outlier is not one of the five.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandmarkKind {
    /// One of the five summary landmarks.
    Summary(SummaryPosition),
    /// The outlier at this index in [`Distribution::outliers`].
    Outlier(usize),
}

/// One value a box plot's value axis cannot place (R1553).
///
/// The [`OffScale`](crate::OffScale) of a datum with extent. A point either
/// is or is not on the axis; a distribution can have four landmarks the axis
/// carries and a fifth it does not, so the report names *which*.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffScaleLandmark {
    /// Index of the distribution, in the chart's own order.
    pub distribution: usize,
    /// Which of its landmarks this is.
    pub landmark: LandmarkKind,
    /// The value the axis could not place.
    pub value: f64,
}

/// A vertical box plot over labelled [`Distribution`]s.
/// R1626 — which **mark** a distribution is drawn as.
///
/// A box plot and a violin are two readings of one sample set, so this is a
/// property of the chart and the category axis, the value axis and its
/// [`off_scale`](BoxPlotChart::off_scale) report, the window, the palette and
/// the inspect readout are all shared. The reference toolkit has a box plot
/// series and no violin at all.
///
/// **A violin needs a density, and a density needs samples.** A distribution
/// built from a pre-computed summary has none, so it keeps its box whatever
/// this says — and [`without_density`](BoxPlotChart::without_density) names
/// every one that did, rather than leaving a reader to wonder why one
/// category looks different.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DistributionMark {
    /// The box, its median, whiskers, caps and outliers. The default.
    #[default]
    Box,
    /// The mirrored density outline alone.
    Violin,
    /// The density outline with the box drawn inside it — the conventional
    /// form, and the one that answers both questions at once: the shape is
    /// the estimate, the box inside it is made of numbers the data actually
    /// took.
    ViolinWithBox,
}

impl DistributionMark {
    /// Every mark, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 3] = [Self::Box, Self::Violin, Self::ViolinWithBox];

    /// Stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Box => "box",
            Self::Violin => "violin",
            Self::ViolinWithBox => "violin+box",
        }
    }

    /// Whether this mark draws the density outline.
    #[must_use]
    pub const fn draws_violin(self) -> bool {
        matches!(self, Self::Violin | Self::ViolinWithBox)
    }

    /// Whether this mark draws the box, median, whiskers and outliers.
    #[must_use]
    pub const fn draws_box(self) -> bool {
        matches!(self, Self::Box | Self::ViolinWithBox)
    }
}

/// R1626 — how a violin's width is scaled against its neighbours.
///
/// Not cosmetic: it decides what the picture *claims*. Every density
/// integrates to one, so drawing each at its own full width says "compare the
/// shapes"; drawing them on one scale says "compare the distributions"; and
/// weighting by sample count says "and this one rests on forty samples while
/// that one rests on four thousand", which the outline otherwise hides
/// completely.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViolinScale {
    /// Each violin fills its slot — its own peak is the full half-width.
    /// The common default, and the one that makes a five-sample violin look
    /// exactly as authoritative as a five-thousand-sample one.
    #[default]
    Width,
    /// One scale for every violin, so equal areas read as equal areas. The
    /// densities already integrate to one, so this is the honest comparison
    /// of shapes.
    Area,
    /// [`Area`](Self::Area) weighted by sample count, so a violin's width
    /// says how much data is behind it.
    Count,
}

impl ViolinScale {
    /// Every scaling, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 3] = [Self::Width, Self::Area, Self::Count];

    /// Stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Width => "width",
            Self::Area => "area",
            Self::Count => "count",
        }
    }
}

pub struct BoxPlotChart {
    distributions: Vec<Distribution>,
    palette: CategoricalPalette,
    y_domain: Option<(f64, f64)>,
    y_kind: AxisKind,
    notched: bool,
    inspect: Option<f32>,
    /// R1545's category axis, derived from the distribution labels once at
    /// construction — held rather than rebuilt per frame for the reason
    /// [`crate::bar`] holds its own: rebuilding clones `n` heap strings on
    /// every paint.
    categories: Categories,
    x_window: Option<CategoryWindow>,
    mark: DistributionMark,
    violin_scale: ViolinScale,
    tag_prefix: String,
}

impl BoxPlotChart {
    /// A box plot over `distributions`, palette-coloured, with an auto
    /// linear value axis, no notch, no inspect overlay, every category in
    /// view, and the `"chart"` tag prefix.
    #[must_use]
    pub fn new(distributions: Vec<Distribution>) -> Self {
        let categories = Categories::new(distributions.iter().map(|d| d.label().to_owned()));
        Self {
            distributions,
            palette: CategoricalPalette::default(),
            y_domain: None,
            y_kind: AxisKind::Linear,
            notched: false,
            inspect: None,
            categories,
            x_window: None,
            mark: DistributionMark::default(),
            violin_scale: ViolinScale::default(),
            tag_prefix: "chart".to_string(),
        }
    }

    /// The distributions this chart was built with.
    #[must_use]
    pub fn distributions(&self) -> &[Distribution] {
        &self.distributions
    }

    /// This chart's x-axis category list — the entry point for resolving an
    /// [`x_window`](Self::x_window) by name (see
    /// [`BarChart::categories`](crate::BarChart::categories)).
    #[must_use]
    pub const fn categories(&self) -> &Categories {
        &self.categories
    }

    /// Show only `window`'s slice of the category axis (the toolkit
    /// `setRange`). The boxes, their labels and the
    /// inspect hit-test all narrow together, because all three derive from
    /// the axis.
    #[must_use]
    pub const fn x_window(mut self, window: CategoryWindow) -> Self {
        self.x_window = Some(window);
        self
    }

    /// The categories currently in view in a chart of `rect` under `style`.
    #[must_use]
    pub fn visible_categories(&self, rect: Rect, style: &ChartStyle) -> Option<CategoryWindow> {
        self.geom(rect, style).x.visible()
    }

    /// Override the default box-colour palette.
    #[must_use]
    pub fn with_palette(mut self, palette: CategoricalPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Pin the value-axis domain instead of deriving it from the data.
    #[must_use]
    pub fn with_y_domain(mut self, lo: f64, hi: f64) -> Self {
        self.y_domain = Some((lo, hi));
        self
    }

    /// Make the value axis logarithmic at the default base 10 (R1528's
    /// [`LogScale`](crate::LogScale)) — the axis a latency distribution
    /// wants, and the one the bar chart is deliberately denied.
    ///
    /// The auto-domain then measures only the strictly positive landmarks
    /// and snaps to whole decades; a landmark at or below zero has no pixel
    /// and is reported by [`off_scale`](Self::off_scale).
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

    /// Draw each box with the McGill-Tukey-Larsen waist at
    /// [`Distribution::notch`] — two boxes whose notches do not overlap have
    /// significantly different medians at roughly 95%.
    ///
    /// A distribution with no notch (a pre-computed summary, which carries
    /// no sample count) keeps its plain rectangular box, so a chart mixing
    /// derived and supplied data shows exactly which of its boxes the test
    /// applies to. The toolkit has no equivalent at any setting.
    #[must_use]
    pub const fn notched(mut self, notched: bool) -> Self {
        self.notched = notched;
        self
    }

    /// Show the inspect overlay (a ring around the focused box and a summary
    /// tooltip) at `fraction` — the cursor's position as a fraction
    /// `0.0..=1.0` across the chart `rect` width, exactly as
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

    /// Every landmark this chart's value axis cannot place, in distribution
    /// order then landmark order (R1553).
    ///
    /// Depends only on the axis KIND and the pinned domain, not on the
    /// geometry, so a consumer can caption the omission without laying the
    /// chart out. Empty on a linear axis with finite data — which is the
    /// counterfactual worth keeping in mind when reading a non-empty one.
    #[must_use]
    pub fn off_scale(&self) -> Vec<OffScaleLandmark> {
        use SummaryPosition as P;
        let mut out = Vec::new();
        for (i, d) in self.distributions.iter().enumerate() {
            let five = [
                (P::LowerExtreme, d.lower_whisker()),
                (P::LowerQuartile, d.q1()),
                (P::Median, d.median()),
                (P::UpperQuartile, d.q3()),
                (P::UpperExtreme, d.upper_whisker()),
            ];
            for (at, value) in five {
                if !self.y_kind.defines(value) {
                    out.push(OffScaleLandmark {
                        distribution: i,
                        landmark: LandmarkKind::Summary(at),
                        value,
                    });
                }
            }
            for (j, &value) in d.outliers().iter().enumerate() {
                if !self.y_kind.defines(value) {
                    out.push(OffScaleLandmark {
                        distribution: i,
                        landmark: LandmarkKind::Outlier(j),
                        value,
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
    /// R1626 — draw the distributions as `mark`. See [`DistributionMark`].
    #[must_use]
    pub const fn with_mark(mut self, mark: DistributionMark) -> Self {
        self.mark = mark;
        self
    }

    /// Which mark the distributions are drawn as.
    #[must_use]
    pub const fn mark(&self) -> DistributionMark {
        self.mark
    }

    /// R1626 — how the violins' widths are scaled against each other.
    #[must_use]
    pub const fn with_violin_scale(mut self, scale: ViolinScale) -> Self {
        self.violin_scale = scale;
        self
    }

    /// The violin width scaling in force.
    #[must_use]
    pub const fn violin_scale(&self) -> ViolinScale {
        self.violin_scale
    }

    /// R1626 — every distribution that cannot be drawn as a violin, by index,
    /// because it carries no density.
    ///
    /// A summary-sourced distribution is the ordinary case: five pre-computed
    /// numbers were never samples, so no estimate belongs to them. Such a
    /// category keeps its box under a violin mark, and this is what lets a
    /// caller caption that rather than leaving the odd one out unexplained.
    ///
    /// Depends only on the data, so it answers without a layout — the same
    /// contract [`off_scale`](Self::off_scale) has.
    #[must_use]
    pub fn without_density(&self) -> Vec<usize> {
        self.distributions
            .iter()
            .enumerate()
            .filter(|(_, d)| d.density().is_none())
            .map(|(i, _)| i)
            .collect()
    }

    /// R1626 — the largest density any drawn violin reaches, which is the
    /// denominator [`ViolinScale::Area`] and [`ViolinScale::Count`] share.
    ///
    /// `None` when no distribution carries a density.
    fn shared_peak(&self) -> Option<f64> {
        let mut peak: Option<f64> = None;
        for d in &self.distributions {
            if let Some(density) = d.density() {
                let p = density.peak() * self.count_weight(density);
                peak = Some(peak.map_or(p, |cur: f64| cur.max(p)));
            }
        }
        peak
    }

    /// The weight `scale` gives a density's peak.
    fn count_weight(&self, density: &Density) -> f64 {
        match self.violin_scale {
            ViolinScale::Count => {
                let biggest = self
                    .distributions
                    .iter()
                    .filter_map(Distribution::density)
                    .map(|d| count_as_f64(d.count()))
                    .fold(1.0, f64::max);
                count_as_f64(density.count()) / biggest
            }
            _ => 1.0,
        }
    }

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
        // A log value axis needs its per-decade subdivisions, for R1528's
        // reason: evenly spaced decade lines read as a linear axis without
        // them. A linear axis produces none, so this is one call either way.
        let minor_pos = tick_pixels(&g.y, &axis_minor_ticks(&g.y));
        children.extend(crate::draw::minor_gridlines(
            frame,
            &[],
            &minor_pos,
            style,
            &self.tag_prefix,
        ));
        children.extend(crate::draw::gridlines(
            frame,
            &[],
            &y_pos,
            style,
            &self.tag_prefix,
        ));
        children.extend(crate::draw::axes(frame, style, &self.tag_prefix));

        let size = style.label_size_px.max(1);
        let x_format = axis_format(&g.x_axis(), &[]);
        for i in visible_indices(&g) {
            children.extend(self.marks_for(&g, i, style));
            children.push(category_label_node(
                &g.x,
                i,
                g.left,
                g.bottom,
                &x_format,
                style.label,
                size,
                &self.tag_prefix,
            ));
        }

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

    /// Every mark of distribution `i`: the box, its median, the two whiskers
    /// with their caps, and one node per outlier.
    ///
    /// A landmark the axis cannot place drops its own mark and nothing else,
    /// so a log axis with a zero whisker still draws the box — and
    /// [`off_scale`](Self::off_scale) names what is missing.
    fn marks_for(&self, g: &BoxGeom, i: usize, style: &ChartStyle) -> Vec<Scene> {
        let Some(dist) = self.distributions.get(i) else {
            return Vec::new();
        };
        let color = self.palette.color(i);
        let stroke = Stroke::new(color, style.series_width.max(1));
        let mut out = Vec::new();

        // R1626 — the violin outline first, so the box (when both are drawn)
        // sits on top of it. A distribution with no density draws no violin
        // and keeps its box; `without_density` names it.
        if self.mark.draws_violin()
            && let Some(outline) = self.violin_outline(g, i)
        {
            out.push(polygon_node(
                &outline,
                PathStyle::filled(color.with_alpha(style.area_alpha)).with_stroke(stroke),
                format!("{}.violin.{i}", self.tag_prefix),
            ));
        }
        if !self.draws_box_for(i) {
            return out;
        }

        let Some(bx) = self.box_geometry(g, i) else {
            return out;
        };

        out.push(polygon_node(
            &bx.outline,
            PathStyle::filled(color.with_alpha(style.area_alpha)).with_stroke(stroke),
            format!("{}.box.{i}", self.tag_prefix),
        ));
        out.push(stroke_path(
            &[
                (bx.median_left, bx.median_y),
                (bx.median_right, bx.median_y),
            ],
            stroke,
            format!("{}.median.{i}", self.tag_prefix),
        ));

        // The whiskers hang off the box edges the axis DID place, so an
        // upper whisker survives a lower one the axis cannot carry.
        for (end, from, tag) in [
            (dist.upper_whisker(), bx.top, "hi"),
            (dist.lower_whisker(), bx.bottom, "lo"),
        ] {
            let Some(y) = g.y.map(end) else { continue };
            out.push(stroke_path(
                &[(bx.center, from), (bx.center, y)],
                stroke,
                format!("{}.whisker.{i}.{tag}", self.tag_prefix),
            ));
            out.push(stroke_path(
                &[(bx.cap_left, y), (bx.cap_right, y)],
                stroke,
                format!("{}.cap.{i}.{tag}", self.tag_prefix),
            ));
        }

        let radius = (style.marker_radius / 2).max(2);
        for (j, &v) in dist.outliers().iter().enumerate() {
            let Some(y) = g.y.map(v) else { continue };
            out.push(marker_node(
                bx.center,
                y,
                radius,
                color,
                format!("{}.outlier.{i}.{j}", self.tag_prefix),
            ));
        }
        out
    }

    /// The pixel geometry of distribution `i`'s box — `None` when the axis
    /// carries no category there, or cannot place the box's own edges (a
    /// non-positive quartile on a log axis), which is the one landmark the
    /// rest of the mark is measured from.
    /// R1626 — whether distribution `i` draws its box.
    ///
    /// Under a violin-only mark, a distribution that HAS no density still
    /// does: the alternative is a blank slot, which reads as "no data" when
    /// the truth is "this category was summarised upstream". The blank is the
    /// one answer certainly wrong, the same judgement R1621 made about an
    /// unmeasurable display.
    fn draws_box_for(&self, i: usize) -> bool {
        if self.mark.draws_box() {
            return true;
        }
        self.distributions
            .get(i)
            .is_some_and(|d| d.density().is_none())
    }

    /// R1626 — the mirrored density outline of distribution `i`, in pixels.
    ///
    /// Walks the density grid up the left side and back down the right, so
    /// the ring closes without a seam. A grid point the value axis cannot
    /// place is skipped rather than clamped — a logarithmic axis has no pixel
    /// for a density estimated below zero, and drawing one at the plot floor
    /// would put mass where the axis says none can be.
    fn violin_outline(&self, g: &BoxGeom, i: usize) -> Option<Vec<(f32, f32)>> {
        let density = self.distributions.get(i)?.density()?;
        let (slot_lo, slot_hi) = g.x.band(i)?;
        let center = f32::midpoint(slot_lo, slot_hi);
        let half = g.box_w / 2.0;
        let peak = match self.violin_scale {
            ViolinScale::Width => density.peak(),
            _ => self.shared_peak()?,
        };
        if peak <= 0.0 {
            return None;
        }
        let weight = self.count_weight(density);

        let mut left: Vec<(f32, f32)> = Vec::with_capacity(density.grid().len());
        let mut right: Vec<(f32, f32)> = Vec::with_capacity(density.grid().len());
        for &(value, d) in density.grid() {
            let Some(y) = g.y.map(value) else { continue };
            let w = narrow_value(d * weight / peak) * half;
            left.push((center - w, y));
            right.push((center + w, y));
        }
        if left.len() < 2 {
            return None;
        }
        right.reverse();
        left.extend(right);
        Some(left)
    }

    fn box_geometry(&self, g: &BoxGeom, i: usize) -> Option<BoxRect> {
        let d = self.distributions.get(i)?;
        let (slot_lo, slot_hi) = g.x.band(i)?;
        let center = f32::midpoint(slot_lo, slot_hi);
        let half = g.box_w / 2.0;
        let (left, right) = (center - half, center + half);
        let top = g.y.map(d.q3())?;
        let bottom = g.y.map(d.q1())?;
        let median_y = g.y.map(d.median())?;

        // The waist, when notched AND the distribution can answer one. A
        // pre-computed summary carries no sample count, so it keeps a plain
        // box — which is the visible difference between a box a reader can
        // apply the test to and one they cannot.
        let waist = self
            .notched
            .then(|| d.notch())
            .flatten()
            .and_then(|(lo, hi)| Some((g.y.map(lo)?, g.y.map(hi)?)));
        let inset = g.box_w * NOTCH_INSET_FRAC;
        let outline = match waist {
            Some((lo_y, hi_y)) => vec![
                (left, top),
                (right, top),
                (right, hi_y),
                (right - inset, median_y),
                (right, lo_y),
                (right, bottom),
                (left, bottom),
                (left, lo_y),
                (left + inset, median_y),
                (left, hi_y),
            ],
            None => vec![(left, top), (right, top), (right, bottom), (left, bottom)],
        };
        let (median_left, median_right) = if waist.is_some() {
            (left + inset, right - inset)
        } else {
            (left, right)
        };
        let cap_half = half * CAP_WIDTH_FRAC;
        Some(BoxRect {
            center,
            left,
            right,
            top,
            bottom,
            median_y,
            median_left,
            median_right,
            cap_left: center - cap_half,
            cap_right: center + cap_half,
            outline,
        })
    }

    /// The plot geometry, value scale, tick set and slot metrics every mark
    /// and the inspect hit-test derive from — the ONE definition of "where
    /// does distribution `i` sit", so the painted box and the inspect ring
    /// can never disagree.
    fn geom(&self, rect: Rect, style: &ChartStyle) -> BoxGeom {
        let (left, right, top, bottom) = plot_rect(rect, style.margin);
        // The value axis measures the marks it will DRAW — outliers
        // included, since an outlier the domain does not reach would be
        // painted outside the plot. A logarithmic axis measures only the
        // positive part, R1528's rule.
        let measured = match self.y_kind {
            AxisKind::Log(_) => positive_distribution_bounds(&self.distributions),
            AxisKind::Linear | AxisKind::Time | AxisKind::Category(_) => {
                distribution_bounds(&self.distributions)
            }
        };
        // The fallback when nothing is measurable is the extent the KIND
        // declares — `crate::plot`'s own, so an empty box plot and an empty
        // line chart land on the same legible axis rather than two.
        let raw = self
            .y_domain
            .or(measured)
            .unwrap_or_else(|| kind_extent(&self.y_kind));
        let dom = axis_domain(self.y_domain, raw, style.y_ticks, &self.y_kind);
        let y = axis_scale(dom, (bottom, top), &self.y_kind);
        let y_ticks = axis_ticks(&y, style.y_ticks);

        let domain = self
            .x_window
            .map_or_else(|| self.categories.extent(), CategoryWindow::domain);
        let x = CategoryScale::new(self.categories.clone(), domain, (left, right));
        let box_w = (x.band_width() * BOX_WIDTH_FRAC).max(1.0);
        BoxGeom {
            left,
            right,
            top,
            bottom,
            y,
            y_ticks,
            x,
            box_w,
        }
    }

    /// The slot index the inspect cursor is over — the axis's own
    /// [`CategoryScale::nearest`], so the hit-test follows a window.
    fn resolve_focus(&self, g: &BoxGeom, rect: Rect) -> Option<usize> {
        let fraction = self.inspect?;
        if g.right - g.left <= 0.0 {
            return None;
        }
        let cursor_px = to_f32(rect.x) + fraction.clamp(0.0, 1.0) * to_f32(rect.w);
        g.x.nearest(cursor_px.clamp(g.left, g.right))
    }

    /// Resolve the inspect overlay: a ring framing the focused box, plus a
    /// tooltip stating the summary AND its provenance.
    fn resolve_inspect(&self, g: &BoxGeom, rect: Rect, style: &ChartStyle) -> Option<BoxInspect> {
        let idx = self.resolve_focus(g, rect)?;
        let d = self.distributions.get(idx)?;
        let b = self.box_geometry(g, idx);
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
        // Every number in the tooltip goes through the AXIS's own readout
        // format, so a log-axis box reads `1.2k` in the same vocabulary its
        // tick labels use rather than in a second one invented here.
        let fmt = |v: f64| g.y_format().readout(v);
        let mut rows = vec![
            self.row("median", &fmt(d.median()), style.tooltip_fg, "median"),
            self.row(
                "IQR",
                &format!("{}\u{2013}{}", fmt(d.q1()), fmt(d.q3())),
                style.tooltip_fg,
                "iqr",
            ),
            self.row(
                "whiskers",
                &format!(
                    "{}\u{2013}{}",
                    fmt(d.lower_whisker()),
                    fmt(d.upper_whisker())
                ),
                style.tooltip_fg,
                "whiskers",
            ),
        ];
        rows.push(match d.source() {
            DistributionSource::Samples { count, method, .. } => self.row(
                "n",
                &format!("{count} ({})", method.name()),
                style.label,
                "n",
            ),
            DistributionSource::Summary => self.row("n", "pre-computed", style.label, "n"),
        });
        if !d.outliers().is_empty() {
            rows.push(self.row(
                "outliers",
                &d.outliers().len().to_string(),
                self.palette.color(idx),
                "outliers",
            ));
        }
        let anchor = b.as_ref().map_or(g.left, |b| b.center);
        let tooltip = callout(
            anchor,
            g.right,
            g.top,
            d.label(),
            format!("{}.inspect.header", self.tag_prefix),
            &rows,
            style,
            format!("{}.inspect.tooltip", self.tag_prefix),
        );
        Some(BoxInspect { highlight, tooltip })
    }

    /// One `label  value` tooltip row under this chart's tag prefix.
    fn row(&self, name: &str, value: &str, color: Color, tag: &str) -> CalloutRow {
        CalloutRow {
            text: format!("{name} {value}"),
            color,
            tag: format!("{}.inspect.{tag}", self.tag_prefix),
        }
    }

    /// The inspect readout as one line — the same summary the tooltip paints,
    /// stated for a screen reader. `None` when nothing is inspected.
    ///
    /// It names the sample count and the quantile method, which is what
    /// separates a box a reader can weigh from a picture: a box over six
    /// samples and one over sixty thousand are the same rectangle. The
    /// toolkit's charts implement no accessibility interface at all, so a
    /// toolkit box plot announces nothing.
    #[must_use]
    pub fn inspect_readout(&self, rect: Rect, style: &ChartStyle) -> Option<String> {
        let g = self.geom(rect, style);
        let idx = self.resolve_focus(&g, rect)?;
        let d = self.distributions.get(idx)?;
        Some(d.readout(tick_step(&g.y_ticks)))
    }
}

/// The plot geometry, value scale, tick set and slot metrics a box plot body
/// derives everything from. One resolve, shared by the marks, the gridlines
/// and the inspect hit-test.
struct BoxGeom {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    y: ValueScale,
    y_ticks: Vec<f64>,
    x: CategoryScale,
    box_w: f32,
}

impl BoxGeom {
    /// The categorical x-axis as a [`ValueScale`], for the shared axis
    /// furniture that takes one.
    fn x_axis(&self) -> ValueScale {
        ValueScale::Category(self.x.clone())
    }

    /// The value axis's label format — per-magnitude on a log axis, the
    /// constant tick step on a linear one (R1528).
    fn y_format(&self) -> TickFormat {
        axis_format(&self.y, &self.y_ticks)
    }
}

/// One distribution's resolved pixel geometry.
struct BoxRect {
    center: f32,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    median_y: f32,
    median_left: f32,
    median_right: f32,
    cap_left: f32,
    cap_right: f32,
    /// The box outline — four corners, or ten when notched.
    outline: Vec<(f32, f32)>,
}

/// The resolved inspect overlay, split so the ring paints over the marks and
/// the tooltip above everything.
struct BoxInspect {
    highlight: Option<Scene>,
    tooltip: Vec<Scene>,
}

/// The category indices this geometry draws — the axis's visible window.
fn visible_indices(g: &BoxGeom) -> impl Iterator<Item = usize> {
    g.x.visible().into_iter().flat_map(|w| w.lo()..=w.hi())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distribution::{DistributionError, QuantileMethod};
    use crate::scene_probe::{count_prefix, find, has, tags};

    const RECT: Rect = Rect::new(0, 0, 640, 360);

    fn samples(base: f64) -> Vec<f64> {
        (0..12).map(|i| base + f64::from(i)).collect()
    }

    fn three() -> Vec<Distribution> {
        ["alpha", "beta", "gamma"]
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a three-element test index is exact in f64"
                )]
                let base = i as f64 * 10.0;
                Distribution::from_samples(name, &samples(base), QuantileMethod::Tukey)
                    .expect("twelve finite samples")
            })
            .collect()
    }

    fn dense(name: &str, base: f64, n: usize) -> Distribution {
        // A bimodal set, so the violin has a shape a box cannot show.
        let mut s = 999u64;
        let samples: Vec<f64> = (0..n)
            .map(|i| {
                s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let jitter = f64::from(u32::try_from((s >> 40) % 100).expect("below 100")) / 50.0;
                base + if i % 2 == 0 { jitter } else { 20.0 + jitter }
            })
            .collect();
        Distribution::from_samples_with_density(
            name,
            &samples,
            QuantileMethod::Tukey,
            crate::Kernel::Gaussian,
            crate::Bandwidth::Silverman,
        )
        .expect("a dense fixture is estimable")
    }

    /// R1626 — a violin is drawn from the density, mirrored about the slot
    /// centre, and it is a DIFFERENT shape from the box.
    #[test]
    fn r1626_a_violin_is_the_mirrored_density_and_not_the_box() {
        let style = ChartStyle::default();
        let chart = BoxPlotChart::new(vec![dense("a", 0.0, 200), dense("b", 5.0, 200)])
            .with_mark(DistributionMark::Violin);
        let scene = chart.build(RECT, &style);
        assert_eq!(chart.mark(), DistributionMark::Violin);
        assert_eq!(count_prefix(&scene, "chart.violin."), 2);
        assert_eq!(count_prefix(&scene, "chart.box."), 0, "violin only");

        for i in 0..2 {
            let Some(Scene::Path(p)) = find(&scene, &format!("chart.violin.{i}")) else {
                panic!("violin {i} is a path");
            };
            let b = pinion_core::path_data::bounds(&p.commands).expect("has bounds");
            assert!(b.width() > 0.0 && b.height() > 0.0, "violin {i}: {b:?}");
            // Mirrored: the outline is symmetric about its own centre, which
            // a one-sided density plot would not be.
            let mid = f32::midpoint(b.min_x, b.max_x);
            let xs: Vec<f32> = p
                .commands
                .iter()
                .filter_map(|c| match c {
                    pinion_core::scene::PathCommand::MoveTo(q)
                    | pinion_core::scene::PathCommand::LineTo(q) => Some(q.x),
                    _ => None,
                })
                .collect();
            for x in &xs {
                let mirrored = 2.0f32.mul_add(mid, -x);
                assert!(
                    xs.iter().any(|o| (o - mirrored).abs() < 1.0),
                    "violin {i} is symmetric: {x} has no mirror",
                );
            }
        }
    }

    /// ★ R1626 — the outline's width IS the density, which is the whole
    /// difference between a violin and a bar.
    ///
    /// FOUND BY A COUNTERFACTUAL: replacing the per-point width with a
    /// constant was caught by nothing. Symmetry survives it, containment
    /// survives it, and the scaling comparison survives it, because all
    /// three ask about the outline's extremes rather than its profile. This
    /// asks about the profile: a density has a peak and two tails, so the
    /// half-width must be largest somewhere in the middle and smallest at
    /// the ends.
    #[test]
    fn r1626_the_outline_narrows_where_the_density_does() {
        let style = ChartStyle::default();
        let dist = dense("a", 0.0, 400);
        let density = dist.density().expect("dense fixture").clone();
        let scene = BoxPlotChart::new(vec![dist])
            .with_mark(DistributionMark::Violin)
            .build(RECT, &style);
        let Some(Scene::Path(p)) = find(&scene, "chart.violin.0") else {
            panic!("violin 0 is a path");
        };
        let pts: Vec<(f32, f32)> = p
            .commands
            .iter()
            .filter_map(|c| match c {
                pinion_core::scene::PathCommand::MoveTo(q)
                | pinion_core::scene::PathCommand::LineTo(q) => Some((q.x, q.y)),
                _ => None,
            })
            .collect();
        let b = pinion_core::path_data::bounds(&p.commands).expect("bounds");
        let centre = f32::midpoint(b.min_x, b.max_x);

        // The half-width at each y, taken from the left half of the ring.
        let half = pts.len() / 2;
        let widths: Vec<f32> = pts[..half].iter().map(|&(x, _)| centre - x).collect();
        assert!(widths.len() > 8, "a real profile, not two points");
        let peak = widths.iter().copied().fold(f32::MIN, f32::max);
        let tail = widths
            .first()
            .copied()
            .unwrap_or(0.0)
            .max(widths.last().copied().unwrap_or(0.0));
        assert!(
            peak > tail * 3.0,
            "the outline is far wider at its peak than at its tails: {peak} vs {tail}",
        );
        // ...and a constant-width outline would have exactly one width.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a quarter-pixel bucket of a plot-width fraction"
        )]
        let bucket = |w: &f32| (w * 4.0).round() as i64;
        let distinct = widths
            .iter()
            .map(bucket)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(
            distinct.len() > 5,
            "the width varies along the outline: {} distinct values",
            distinct.len(),
        );
        // The widest point sits where the density peaks, not at the middle
        // of the value range — which is what makes it an estimate rather
        // than an ornament.
        let peak_at = widths
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .expect("non-empty");
        let expected = density
            .grid()
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
            .map(|(i, _)| i)
            .expect("non-empty");
        let slack = density.grid().len() / 10;
        assert!(
            peak_at.abs_diff(expected) <= slack.max(2),
            "the widest point is where the density peaks: {peak_at} vs {expected}",
        );
    }

    /// R1626 — the combined mark draws BOTH, and the box on top.
    #[test]
    fn r1626_the_combined_mark_keeps_the_numbers_inside_the_estimate() {
        let style = ChartStyle::default();
        let scene = BoxPlotChart::new(vec![dense("a", 0.0, 200)])
            .with_mark(DistributionMark::ViolinWithBox)
            .build(RECT, &style);
        assert!(has(&scene, "chart.violin.0"), "the estimate");
        assert!(has(&scene, "chart.box.0"), "and the numbers");
        assert!(has(&scene, "chart.median.0"));
        let painted = tags(&scene);
        let vi = painted.iter().position(|t| t == "chart.violin.0");
        let bi = painted.iter().position(|t| t == "chart.box.0");
        assert!(vi < bi, "the box paints over the violin: {vi:?} {bi:?}");
    }

    /// ★ R1626 — a distribution with no samples cannot be a violin, keeps its
    /// box, and is REPORTED.
    ///
    /// The blank slot is the one answer certainly wrong: it reads as "no
    /// data" when the truth is "summarised upstream".
    #[test]
    fn r1626_a_summary_cannot_be_a_violin_and_says_so() {
        let style = ChartStyle::default();
        let summary = Distribution::from_summary("upstream", 1.0, 3.0, 5.0, 7.0, 9.0)
            .expect("ordered summary");
        assert!(summary.density().is_none());
        assert_eq!(
            summary.clone().with_density(
                crate::Density::from_samples(
                    &[1.0, 2.0, 3.0],
                    crate::Kernel::Gaussian,
                    crate::Bandwidth::Silverman,
                    64,
                )
                .expect("estimable"),
            ),
            Err(DistributionError::DensityWithoutSamples),
            "an estimate beside five pre-computed numbers describes other data",
        );

        let chart = BoxPlotChart::new(vec![dense("a", 0.0, 200), summary])
            .with_mark(DistributionMark::Violin);
        assert_eq!(chart.without_density(), vec![1], "the report names it");
        let scene = chart.build(RECT, &style);
        assert!(has(&scene, "chart.violin.0"), "the sampled one is a violin");
        assert!(!has(&scene, "chart.violin.1"), "the summary is not");
        assert!(
            has(&scene, "chart.box.1"),
            "and it keeps its box rather than leaving a blank slot",
        );
        assert!(
            !has(&scene, "chart.box.0"),
            "while the violin-drawn one does not gain one",
        );
    }

    /// ★ R1626 — the width scaling decides what the picture CLAIMS, and the
    /// three readings differ measurably.
    #[test]
    fn r1626_the_width_scaling_changes_what_the_picture_claims() {
        let style = ChartStyle::default();
        // Two categories, one with far fewer samples than the other.
        let dists = vec![dense("many", 0.0, 400), dense("few", 0.0, 20)];
        let widths = |scale| {
            let scene = BoxPlotChart::new(dists.clone())
                .with_mark(DistributionMark::Violin)
                .with_violin_scale(scale)
                .build(RECT, &style);
            (0..2)
                .map(|i| {
                    let Some(Scene::Path(p)) = find(&scene, &format!("chart.violin.{i}")) else {
                        panic!("violin {i}");
                    };
                    pinion_core::path_data::bounds(&p.commands)
                        .expect("has bounds")
                        .width()
                })
                .collect::<Vec<f32>>()
        };

        let w = widths(ViolinScale::Width);
        assert!(
            (w[0] - w[1]).abs() < 1.0,
            "Width fills every slot, whatever is behind it: {w:?}",
        );
        let c = widths(ViolinScale::Count);
        assert!(
            c[1] < c[0] * 0.5,
            "Count says how much data is behind each: {c:?}",
        );
        // Area sits between: equal areas, so the peaks differ but not by n.
        let a = widths(ViolinScale::Area);
        assert!(a.iter().all(|v| *v > 0.0), "{a:?}");
        assert!(
            c[1] < a[1],
            "Count is narrower than Area for the small sample: {a:?} vs {c:?}",
        );
    }

    /// R1626 — every mark and every scaling in the census paints, so a new
    /// one cannot be added and left unpainted.
    #[test]
    fn r1626_every_mark_and_scaling_in_the_census_paints() {
        let style = ChartStyle::default();
        let dists = vec![dense("a", 0.0, 200), dense("b", 5.0, 200)];
        for mark in DistributionMark::ALL {
            let scene = BoxPlotChart::new(dists.clone())
                .with_mark(mark)
                .build(RECT, &style);
            assert_eq!(
                count_prefix(&scene, "chart.violin.") > 0,
                mark.draws_violin(),
                "{mark:?} draws its violin exactly when it says it does",
            );
            assert_eq!(
                count_prefix(&scene, "chart.box.") > 0,
                mark.draws_box(),
                "{mark:?} draws its box exactly when it says it does",
            );
        }
        for scale in ViolinScale::ALL {
            let scene = BoxPlotChart::new(dists.clone())
                .with_mark(DistributionMark::Violin)
                .with_violin_scale(scale)
                .build(RECT, &style);
            assert_eq!(count_prefix(&scene, "chart.violin."), 2, "{scale:?}");
        }
        let mut names: Vec<&str> = DistributionMark::ALL.iter().map(|m| m.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), DistributionMark::ALL.len());
        let mut scales: Vec<&str> = ViolinScale::ALL.iter().map(|s| s.name()).collect();
        scales.sort_unstable();
        scales.dedup();
        assert_eq!(scales.len(), ViolinScale::ALL.len());
    }

    /// R1626 — the mark is a rendering: it does not move the categories.
    #[test]
    fn r1626_the_mark_does_not_move_the_categories() {
        let style = ChartStyle::default();
        let dists = vec![dense("a", 0.0, 200), dense("b", 5.0, 200)];
        let centres = |mark: DistributionMark| {
            let chart = BoxPlotChart::new(dists.clone()).with_mark(mark);
            let scene = chart.build(RECT, &style);
            (0..2)
                .map(|i| {
                    let tag = if mark.draws_violin() {
                        format!("chart.violin.{i}")
                    } else {
                        format!("chart.box.{i}")
                    };
                    let Some(Scene::Path(p)) = find(&scene, &tag) else {
                        panic!("{tag}");
                    };
                    let b = pinion_core::path_data::bounds(&p.commands).expect("bounds");
                    f32::midpoint(b.min_x, b.max_x) + crate::draw::to_f32(p.rect.x)
                })
                .collect::<Vec<f32>>()
        };
        let boxes = centres(DistributionMark::Box);
        let violins = centres(DistributionMark::Violin);
        for (i, (a, b)) in boxes.iter().zip(violins.iter()).enumerate() {
            assert!((a - b).abs() < 1.0, "category {i} moved: {a} vs {b}");
        }
    }

    /// ★ One datum emits a whole schema — box, median, two whiskers, two
    /// caps — where every other chart in this crate emits one mark. That is
    /// the round's claim, asserted as node cardinality.
    #[test]
    fn r1553_one_distribution_emits_the_whole_schema() {
        let scene = BoxPlotChart::new(three()).build(RECT, &ChartStyle::default());
        let tags = tags(&scene);
        for expected in [
            "chart.box.0",
            "chart.median.0",
            "chart.whisker.0.lo",
            "chart.whisker.0.hi",
            "chart.cap.0.lo",
            "chart.cap.0.hi",
            "chart.xlabel.0",
        ] {
            assert!(tags.contains(&expected.to_string()), "missing {expected}");
        }
        // Three distributions -> three of each.
        assert_eq!(count_prefix(&scene, "chart.box."), 3);
        assert_eq!(count_prefix(&scene, "chart.whisker."), 6);
        assert_eq!(count_prefix(&scene, "chart.cap."), 6);
        // These samples are uniform, so nothing is beyond the fence — the
        // counterfactual for the outlier test below.
        assert_eq!(count_prefix(&scene, "chart.outlier."), 0);
    }

    /// ★ An outlier is its own addressable mark. The toolkit's box set has
    /// five slots and no per-outlier geometry, so this is the half of Tukey's
    /// form a toolkit box plot cannot draw at all.
    #[test]
    fn r1553_each_outlier_is_its_own_node() {
        let mut with_far = samples(0.0);
        with_far.push(500.0);
        with_far.push(600.0);
        let d = Distribution::from_samples("far", &with_far, QuantileMethod::Tukey)
            .expect("finite samples");
        assert_eq!(d.outliers().len(), 2, "two samples past the fence");

        let scene = BoxPlotChart::new(vec![d]).build(RECT, &ChartStyle::default());
        assert!(has(&scene, "chart.outlier.0.0"));
        assert!(has(&scene, "chart.outlier.0.1"));
        assert_eq!(
            count_prefix(&scene, "chart.outlier."),
            2,
            "one node per outlier, no more"
        );
    }

    /// ★ The notch is drawn only where the statistic EXISTS. A derived
    /// distribution grows a waist (ten outline points instead of four); a
    /// pre-computed summary keeps its plain box under the very same
    /// `notched(true)` chart, because it carries no sample count.
    #[test]
    fn r1553_the_notch_is_drawn_only_where_n_exists() {
        let derived = Distribution::from_samples("derived", &samples(0.0), QuantileMethod::Tukey)
            .expect("finite samples");
        let supplied =
            Distribution::from_summary("supplied", 0.0, 3.0, 6.0, 9.0, 12.0).expect("ordered");
        let style = ChartStyle::default();

        let chart = BoxPlotChart::new(vec![derived, supplied]).notched(true);
        let g = chart.geom(RECT, &style);
        assert_eq!(
            chart.box_geometry(&g, 0).expect("placed").outline.len(),
            10,
            "the derived box has a waist"
        );
        assert_eq!(
            chart.box_geometry(&g, 1).expect("placed").outline.len(),
            4,
            "the supplied summary keeps a plain box"
        );

        // Counterfactual: with the notch off, the derived box is plain too —
        // so ten points is the notch's doing, not the sample path's.
        let plain = BoxPlotChart::new(vec![
            Distribution::from_samples("derived", &samples(0.0), QuantileMethod::Tukey)
                .expect("finite samples"),
        ]);
        let g = plain.geom(RECT, &style);
        assert_eq!(plain.box_geometry(&g, 0).expect("placed").outline.len(), 4);
    }

    /// ★ Switching the quantile method moves the painted box. The method is
    /// not a label on the data — it is what the box IS.
    #[test]
    fn r1553_the_quantile_method_moves_the_painted_box() {
        let style = ChartStyle::default();
        let quartet = [1.0, 2.0, 3.0, 4.0];
        let top_of = |m| {
            let chart = BoxPlotChart::new(vec![
                Distribution::from_samples("q", &quartet, m).expect("finite samples"),
            ])
            // Pinned so the domain cannot absorb the difference: the axis is
            // identical and only the box moves.
            .with_y_domain(0.0, 5.0);
            let g = chart.geom(RECT, &style);
            chart.box_geometry(&g, 0).expect("placed").bottom
        };
        let tukey = top_of(QuantileMethod::Tukey);
        let linear = top_of(QuantileMethod::Linear);
        let exclusive = top_of(QuantileMethod::Exclusive);
        assert!(
            (tukey - linear).abs() > 0.5 && (tukey - exclusive).abs() > 0.5,
            "three methods, three box bottoms: {tukey} / {linear} / {exclusive}"
        );
        // Q1 ascends exclusive < tukey < linear, and the y-axis is inverted,
        // so the box BOTTOM descends in pixels the other way.
        assert!(
            exclusive > tukey && tukey > linear,
            "pixel order follows the quartile order: {exclusive} / {tukey} / {linear}"
        );
    }

    /// ★ A landmark a logarithmic axis cannot place draws no mark and is
    /// REPORTED — R1528's stance on a datum with extent. The counterfactual
    /// is the same data on a linear axis, where nothing is off-scale.
    #[test]
    fn r1553_a_landmark_off_a_log_axis_is_reported_not_placed() {
        let d = Distribution::from_summary("zeroed", 0.0, 2.0, 5.0, 9.0, 20.0).expect("ordered");
        let chart = BoxPlotChart::new(vec![d]).y_log();
        let off = chart.off_scale();
        assert_eq!(off.len(), 1, "only the zero whisker: {off:?}");
        assert_eq!(
            off[0].landmark,
            LandmarkKind::Summary(SummaryPosition::LowerExtreme)
        );
        assert!((off[0].value - 0.0).abs() < f64::EPSILON);

        let logged = chart.build(RECT, &ChartStyle::default());
        assert!(
            !has(&logged, "chart.whisker.0.lo"),
            "the unplaceable whisker draws nothing"
        );
        assert!(
            has(&logged, "chart.whisker.0.hi"),
            "the other whisker is unaffected"
        );
        assert!(has(&logged, "chart.box.0"), "the box itself still draws");

        // Counterfactual: on a linear axis the same distribution is wholly
        // placeable, so this is a property of the AXIS, not of the data.
        let linear = BoxPlotChart::new(vec![
            Distribution::from_summary("zeroed", 0.0, 2.0, 5.0, 9.0, 20.0).expect("ordered"),
        ]);
        assert!(linear.off_scale().is_empty());
        assert!(has(
            &linear.build(RECT, &ChartStyle::default()),
            "chart.whisker.0.lo"
        ));
    }

    /// ★ The value axis reaches the OUTLIERS, not just the box — a mark the
    /// domain did not cover would be painted outside the plot.
    #[test]
    fn r1553_the_auto_domain_covers_the_outliers() {
        let mut with_far = samples(0.0);
        with_far.push(500.0);
        let chart = BoxPlotChart::new(vec![
            Distribution::from_samples("far", &with_far, QuantileMethod::Tukey)
                .expect("finite samples"),
        ]);
        let g = chart.geom(RECT, &ChartStyle::default());
        let (_, hi) = g.y.domain();
        assert!(hi >= 500.0, "the domain reaches the outlier, got {hi}");
        // And the outlier's pixel is inside the plot area.
        let y = g.y.map(500.0).expect("placed");
        assert!(y >= g.top && y <= g.bottom, "outlier pixel {y} is in-plot");
    }

    /// ★ The box is ONE node kind whether or not it is notched, so a
    /// consumer reading `chart.box.{i}` never has to branch on a flag it
    /// cannot see.
    #[test]
    fn r1553_the_box_is_one_node_kind_notched_or_not() {
        let style = ChartStyle::default();
        for notched in [false, true] {
            let scene = BoxPlotChart::new(three())
                .notched(notched)
                .build(RECT, &style);
            let node = find(&scene, "chart.box.0").expect("the box is present");
            assert!(
                matches!(node, Scene::Path(_)),
                "notched={notched} box is {node:?}"
            );
        }
    }

    /// ★ The scrub reads the summary AND its provenance, and the tooltip and
    /// the a11y readout come from the one derivation, so they cannot state
    /// different numbers.
    #[test]
    fn r1553_the_scrub_states_the_summary_and_its_provenance() {
        let style = ChartStyle::default();
        let chart = BoxPlotChart::new(three()).inspect(Some(0.5));
        let readout = chart.inspect_readout(RECT, &style).expect("focused");
        assert!(readout.starts_with("beta: "), "{readout}");
        assert!(readout.contains("n 12 (tukey)"), "{readout}");

        let scrubbed = chart.build(RECT, &style);
        for expected in [
            "chart.inspect.highlight",
            "chart.inspect.tooltip",
            "chart.inspect.header",
            "chart.inspect.median",
            "chart.inspect.iqr",
            "chart.inspect.whiskers",
            "chart.inspect.n",
        ] {
            assert!(has(&scrubbed, expected), "missing {expected}");
        }
        // No outliers in this data -> no outlier row (present-if-nonempty).
        assert!(!has(&scrubbed, "chart.inspect.outliers"));

        // Counterfactual: with no scrub there is no overlay at all.
        let quiet = BoxPlotChart::new(three()).build(RECT, &style);
        assert_eq!(count_prefix(&quiet, "chart.inspect."), 0);
    }

    /// ★ A category window narrows what is DRAWN, not only what is domained
    /// — the R1545 property, reaching a chart R1545 did not have.
    #[test]
    fn r1553_a_category_window_narrows_the_drawn_boxes() {
        let style = ChartStyle::default();
        let chart = BoxPlotChart::new(three());
        let window = chart
            .categories()
            .window("beta", "gamma")
            .expect("both names exist");
        let scene = chart.x_window(window).build(RECT, &style);
        let tags = tags(&scene);
        assert!(!tags.contains(&"chart.box.0".to_string()), "alpha is out");
        assert!(tags.contains(&"chart.box.1".to_string()));
        assert!(tags.contains(&"chart.box.2".to_string()));
        // The surviving slots keep their OWN labels (an off-by-one would show).
        assert_eq!(
            BoxPlotChart::new(three())
                .x_window(window)
                .visible_categories(RECT, &style)
                .map(|w| (w.lo(), w.hi())),
            Some((1, 2))
        );
    }

    /// ★ An empty chart builds a legible empty plot rather than panicking or
    /// collapsing its axis.
    #[test]
    fn r1553_an_empty_chart_still_builds() {
        let scene = BoxPlotChart::new(Vec::new()).build(RECT, &ChartStyle::default());
        assert!(has(&scene, "chart.axis.x"));
        assert_eq!(count_prefix(&scene, "chart.box."), 0);
        assert!(BoxPlotChart::new(Vec::new()).off_scale().is_empty());
        // `build_fill` at zero size is the bootstrap sentinel, not a panic.
        let empty = BoxPlotChart::new(three()).build_fill((0, 0), &ChartStyle::default());
        assert_eq!(tags(&empty), vec!["chart".to_string()]);
    }
}
