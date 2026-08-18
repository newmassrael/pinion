//! The donut / pie chart builder — projects a list of labelled [`Slice`]s
//! into a retained [`Scene`] of filled annular sectors (a donut), a legend,
//! and an optional scrub inspector.
//!
//! # Why a third builder rather than a `LineChart` / `BarChart` mode
//!
//! A donut is the crate's first PART-OF-WHOLE form: it has no value axis, no
//! gridlines, and no categorical or numeric x — a slice's size is its share of
//! the total, drawn as an angular sector, not a height against a baseline. So
//! it shares NONE of the axis furniture line + bar + scatter share (a donut has
//! no axes — R1377's scatter, not the donut, was the third axis-based chart that
//! settled the axis-furniture lift). What it DOES reuse is the crate's genuinely
//! cross-cutting core: the categorical [`palette`](crate::palette), the shared
//! [`legend_row`](crate::draw) leaf (the swatch + label loop line and scatter
//! also emit), the shared [`arc_beziers`](crate::draw) arc its sectors are drawn
//! from (the same arc the line + scatter circle markers close into a full
//! circle, R1377), and the inspect [`callout`](crate::draw) tooltip (the same
//! box the line + bar + scatter inspectors use).
//!
//! # Geometry
//!
//! Slices sweep CLOCKWISE from the top (12 o'clock). Each sector is a filled
//! [`Scene::Path`]: its outer arc — and, for a donut, its inner arc — is a
//! cubic-Bézier arc approximation (the same technique the line chart's marker
//! circle uses, generalised to a partial arc split into <=90-degree segments,
//! `k = 4/3 * tan(delta/4)`); a pie closes those arcs to the centre with
//! straight radial edges instead. Every slice's Path shares ONE node rect (the
//! donut's bounding box), with the commands rebased onto that rect's origin
//! (R1358 — a `Scene::Path` is positioned by its rect, its commands are
//! rect-local), so the whole donut docks / flexes / resizes like the other
//! charts.
//!
//! # Introspection
//!
//! Every node carries a tag under `tag_prefix` (default `"chart"`):
//! `chart.bg`, `chart.slice.{i}` (one filled sector per positive slice — a
//! zero / non-finite / negative slice contributes no sector, present-if-drawn),
//! and `chart.legend.{i}.swatch` / `chart.legend.{i}.label`. When
//! [`inspect`](DonutChart::inspect) is set the overlay adds
//! `chart.inspect.highlight` (the focused sector re-stroked as a ring),
//! `chart.inspect.tooltip` (the callout box), `chart.inspect.header` (the
//! focused slice label) and `chart.inspect.value` (its value + percent share).
//!
//! # Coordinate contract
//!
//! Identical to [`crate::line`] / [`crate::bar`]: [`DonutChart::build_fill`] is
//! the **layout-native** entry point (fill-parent root, children in the chart's
//! own `(0,0)..(w,h)` frame), [`DonutChart::build`] pins to a caller rect. Read
//! the crate-level "Known limitations": the `build_fill` measured-rect seam is
//! Vello-only, and `Scene::Path` (every sector) does not render on TUI.

use core::f32::consts::PI;

use pinion_core::Scene;
use pinion_core::derivation::DerivationSet;
use pinion_core::scene::{ContainerNode, PathCommand, PathNode, PathPoint, Rect};
use pinion_core::style::{Color, PathStyle, Stroke};

use crate::derivations;
use crate::legend::{ChartLegend, Legend, LegendEntry, LegendInteraction, LegendSeat};

use crate::draw::{
    CalloutRow, absolute, arc_beziers, box_node, callout, fill_parent, to_f32, to_u32,
};
use crate::palette::CategoricalPalette;
use crate::style::ChartStyle;

/// The fraction of the outer radius left as the centre hole. `0.0` = a solid
/// pie; the default `0.55` = a donut whose ring is wide enough to read.
const DEFAULT_INNER_RATIO: f32 = 0.55;

/// One slice: a `label`, its `value` (its share of the total), and an optional
/// per-slice `color` override (else the palette colour by index).
#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    /// The slice's label (shown in the legend + the inspect tooltip).
    pub label: String,
    /// The slice's value — its share of the total is `value / sum`.
    pub value: f64,
    /// An optional colour override for THIS slice.
    pub color: Option<Color>,
    /// Whether this slice is part of the ring (R1722). A hidden slice leaves
    /// the total, so the ring **re-normalises** around the slices that remain —
    /// which is what a part-of-whole picture must do, and the difference from
    /// [`Series::visible`](crate::Series::visible), where hiding a line leaves
    /// the others' geometry untouched. It keeps its legend slot and its palette
    /// index either way, so showing it again lands it where it was.
    pub visible: bool,
}

impl Slice {
    /// A slice with the default (palette) colour, in the ring.
    #[must_use]
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        Self {
            label: label.into(),
            value,
            color: None,
            visible: true,
        }
    }

    /// Whether this slice is part of the ring. See [`Slice::visible`].
    #[must_use]
    pub fn shown(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Override this slice's colour.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

/// A donut (or pie) chart over labelled [`Slice`]s — the crate's part-of-whole
/// form. Reuses the categorical `palette`, the legend leaf primitives, and the
/// inspect `callout` tooltip with [`LineChart`](crate::LineChart) /
/// [`BarChart`](crate::BarChart).
pub struct DonutChart {
    slices: Vec<Slice>,
    palette: CategoricalPalette,
    inner_ratio: f32,
    inspect: Option<f32>,
    /// R1722 — what may be done to the legend. See [`crate::Legend`].
    legend_interaction: LegendInteraction,
    tag_prefix: String,
}

impl DonutChart {
    /// A donut chart over `slices`, using the default palette, the default
    /// centre-hole ratio (a donut), no inspect overlay, and the `"chart"` tag
    /// prefix.
    #[must_use]
    pub fn new(slices: Vec<Slice>) -> Self {
        Self {
            slices,
            palette: CategoricalPalette::default(),
            inner_ratio: DEFAULT_INNER_RATIO,
            inspect: None,
            legend_interaction: LegendInteraction::default(),
            tag_prefix: "chart".to_string(),
        }
    }

    /// Override the default slice-colour palette (a slice's own
    /// [`with_color`](Slice::with_color) still wins).
    #[must_use]
    pub fn with_palette(mut self, palette: CategoricalPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Set the centre-hole ratio `0.0..1.0` (`0.0` = a solid pie, the default
    /// `0.55` = a donut). Values are clamped to `0.0..=0.9` so the ring never
    /// collapses.
    #[must_use]
    pub fn with_inner_ratio(mut self, ratio: f32) -> Self {
        self.inner_ratio = ratio.clamp(0.0, 0.9);
        self
    }

    /// Show the inspect overlay (a ring on the focused slice + a value tooltip)
    /// at `fraction` — the cursor's position as a fraction `0.0..=1.0`. Mirrors
    /// [`BarChart::inspect`](crate::BarChart::inspect): the fraction
    /// SCRUBS the slices in order (`fraction * n` -> slice index), the natural
    /// output of a pointer-capture scrub, since pinion forwards a 1-D captured
    /// position (an angular geometric hover would need a 2-D pointer external).
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

    /// The slices this chart was built with.
    #[must_use]
    pub fn slices(&self) -> &[Slice] {
        &self.slices
    }

    /// Build the chart PINNED to `rect`. See [`crate::LineChart::build`]
    /// — same contract; prefer [`Self::build_fill`] for anything layout-placed.
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
    /// measures. See [`crate::LineChart::build_fill`].
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

    /// R1629 §2 #7 — what this drawing did that the drawing cannot give back:
    /// **nothing**, and the empty set is how it says so.
    ///
    /// Every setting [`DonutChart`] takes is either visible in the drawing it
    /// produces (a colour, a fill, a marker) or an explicit domain the caller
    /// asked for and can see the edges of. It joins nothing between points,
    /// estimates nothing, and drops no datum an axis could not carry — so
    /// there is no disagreement between the picture and its two sources to
    /// report.
    ///
    /// Published rather than withheld, because "I ran my reports and found
    /// nothing" and "I do not answer this question" are different facts and a
    /// client acts differently on each.
    #[must_use]
    pub fn derivations(&self) -> DerivationSet {
        DerivationSet::over(derivations::domain::SLOT)
    }

    /// The chart body, authored in `rect`'s frame — the ONE builder both entry
    /// points wrap.
    fn build_body(&self, rect: Rect, style: &ChartStyle) -> ContainerNode {
        let geom = self.geom(rect, style);
        let (highlight, tooltip) = match self.resolve_inspect(&geom, style) {
            Some(i) => (Some(i.highlight), i.tooltip),
            None => (None, Vec::new()),
        };

        let mut children: Vec<Scene> = Vec::new();
        if let Some(bg) = style.background {
            children.push(box_node(rect, bg, format!("{}.bg", self.tag_prefix)));
        }

        // One filled sector per drawn slice, in sweep order, tagged by the
        // slice's own index rather than by its position among the drawn ones.
        //
        // (R1722) The distinction became reachable when a legend could hide a
        // slice: tagging by draw order renumbers every sector after a dropped
        // one, so an agent that hid a slice and then read `slice.1` would get a
        // different slice than before it pressed. The colour was already taken
        // from `seg.slice` for the same reason, and this is R1379's rule for a
        // hidden series — its index survives — applied to the other hiding rule.
        for seg in &geom.segments {
            let color = self.slices[seg.slice]
                .color
                .unwrap_or_else(|| self.palette.color(seg.slice));
            children.push(sector_node(
                &geom,
                seg.a0,
                seg.a1,
                PathStyle::filled(color),
                format!("{}.slice.{}", self.tag_prefix, seg.slice),
            ));
        }

        // The highlight ring sits over the sectors but under the legend + tooltip.
        if let Some(highlight) = highlight {
            children.push(highlight);
        }

        // Legend: swatch + label per ORIGINAL slice (kept even for a
        // zero/omitted slice, so the reader still sees the category).
        if style.legend {
            children.extend(self.legend_scene(rect, style));
        }

        children.extend(tooltip);
        derivations::chart_root(children, self.tag_prefix.clone(), self.derivations())
    }

    /// The donut geometry: the centre + radii (in the rect's own frame) and the
    /// per-drawn-slice angular segments. The ONE definition the sectors, the
    /// highlight ring, and the inspect hit-test all read (so the ring lands on
    /// its slice).
    fn geom(&self, rect: Rect, style: &ChartStyle) -> DonutGeom {
        // A donut has no axes, so — unlike line / bar — it does NOT inherit the
        // asymmetric axis-gutter margins (a left `y-label` gutter would shove the
        // circle off-centre). It centres HORIZONTALLY in the full rect with a
        // uniform inset, and reserves a legend band along the bottom.
        let inset = to_f32(style.margin.top);
        let legend_band = to_f32(self.legend_band(style));
        let avail_w = to_f32(rect.w).max(1.0) - inset * 2.0;
        let avail_h = (to_f32(rect.h) - inset - legend_band).max(1.0);
        let r_out = (avail_w.min(avail_h) / 2.0).max(1.0);
        let r_in = r_out * self.inner_ratio;
        let cx = to_f32(rect.x) + to_f32(rect.w) / 2.0;
        let cy = to_f32(rect.y) + inset + avail_h / 2.0;
        // The bounding box the sectors' Paths share (their rect); commands are
        // rebased onto its u32 origin (R1358 pixel-exact).
        let ox = to_u32(cx - r_out);
        let oy = to_u32(cy - r_out);
        let bbox = Rect::new(ox, oy, to_u32(r_out * 2.0) + 1, to_u32(r_out * 2.0) + 1);

        let total: f64 = self
            .slices
            .iter()
            .filter(|s| s.visible && s.value.is_finite() && s.value > 0.0)
            .map(|s| s.value)
            .sum();
        let mut segments = Vec::new();
        if total > 0.0 {
            let mut a = 0.0_f32;
            for (i, s) in self.slices.iter().enumerate() {
                if s.visible && s.value.is_finite() && s.value > 0.0 {
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "an angular sweep in 0..2pi narrows to f32 for the pixel geometry; the sub-degree loss is invisible"
                    )]
                    let sweep = ((s.value / total) as f32) * 2.0 * PI;
                    segments.push(Segment {
                        slice: i,
                        a0: a,
                        a1: a + sweep,
                    });
                    a += sweep;
                }
            }
        }

        DonutGeom {
            cx: cx - to_f32(ox),
            cy: cy - to_f32(oy),
            r_out,
            r_in,
            bbox,
            total,
            segments,
        }
    }

    /// Resolve which drawn segment the inspect cursor is over: `fraction * n`
    /// SCRUBS the slices in draw order. `None` when inspection is off / there
    /// are no drawn slices.
    fn resolve_focus(&self, geom: &DonutGeom) -> Option<usize> {
        let fraction = self.inspect?;
        let n = geom.segments.len();
        if n == 0 {
            return None;
        }
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "fraction is 0..=1 and n a small slice count; the product is a valid segment index, clamped below"
        )]
        let idx = (fraction.clamp(0.0, 1.0) * n as f32).floor() as usize;
        Some(idx.min(n - 1))
    }

    /// Resolve the inspect overlay: the focused sector re-stroked as a ring +
    /// a value tooltip (`{label}` header + a `{value} ({pct}%)` row).
    fn resolve_inspect(&self, geom: &DonutGeom, style: &ChartStyle) -> Option<DonutInspect> {
        let seg_idx = self.resolve_focus(geom)?;
        let seg = geom.segments[seg_idx];
        let slice = &self.slices[seg.slice];
        let highlight = sector_node(
            geom,
            seg.a0,
            seg.a1,
            PathStyle::stroked(Stroke::new(style.crosshair, 2)),
            format!("{}.inspect.highlight", self.tag_prefix),
        );
        // The tooltip is anchored at the slice's mid-angle point on the outer
        // radius (in the sector rect's frame -> window via the bbox origin).
        let mid = midpoint(f64::midpoint(f64::from(seg.a0), f64::from(seg.a1)));
        let anchor_x = to_f32(geom.bbox.x) + geom.cx + geom.r_out * mid.0;
        let rows = vec![CalloutRow {
            text: percent_text(slice.value, geom.total),
            color: style.tooltip_fg,
            tag: format!("{}.inspect.value", self.tag_prefix),
        }];
        let tooltip = callout(
            anchor_x,
            to_f32(geom.bbox.x + geom.bbox.w),
            to_f32(geom.bbox.y),
            &slice.label,
            format!("{}.inspect.header", self.tag_prefix),
            &rows,
            style,
            format!("{}.inspect.tooltip", self.tag_prefix),
        );
        Some(DonutInspect { highlight, tooltip })
    }

    /// The inspect readout as one line — the focus label + its value + percent
    /// share, or `None` when nothing is inspected. Wired into the scrub
    /// control's `WidgetA11y` node (R1355 parity, now for the donut).
    #[must_use]
    pub fn inspect_readout(&self, rect: Rect, style: &ChartStyle) -> Option<String> {
        let geom = self.geom(rect, style);
        let seg_idx = self.resolve_focus(&geom)?;
        let slice = &self.slices[geom.segments[seg_idx].slice];
        Some(format!(
            "{} = {}",
            slice.label,
            percent_text(slice.value, geom.total)
        ))
    }

    /// The bottom space reserved for the legend row (so the donut centres
    /// ABOVE it, never under it). When the legend is off / there are no slices,
    /// only the bottom margin is reserved.
    fn legend_band(&self, style: &ChartStyle) -> u32 {
        if style.legend && !self.slices.is_empty() {
            style.label_size_px.max(1) + style.margin.bottom + 6
        } else {
            style.margin.bottom
        }
    }

    /// Declare what may be done to this chart's legend (R1722).
    #[must_use]
    pub fn with_legend(mut self, interaction: LegendInteraction) -> Self {
        self.legend_interaction = interaction;
        self
    }
}

/// One drawn angular segment: which original slice, and its `[a0, a1]` sweep
/// (radians, clockwise from the top).
#[derive(Debug, Clone, Copy)]
struct Segment {
    slice: usize,
    a0: f32,
    a1: f32,
}

/// The resolved donut geometry (centre in the sectors' shared rect frame,
/// radii, bbox, total, and the per-drawn-slice segments).
struct DonutGeom {
    cx: f32,
    cy: f32,
    r_out: f32,
    r_in: f32,
    bbox: Rect,
    total: f64,
    segments: Vec<Segment>,
}

/// The resolved inspect overlay: the focused sector re-stroked as a ring + a
/// value tooltip.
struct DonutInspect {
    highlight: Scene,
    tooltip: Vec<Scene>,
}

/// The unit direction (sin, -cos) of an angle measured CLOCKWISE from the top,
/// so `0` points up and the sweep runs clockwise in screen coordinates (y down).
fn midpoint(angle: f64) -> (f32, f32) {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "an angle in 0..2pi narrows to f32 for pixel geometry"
    )]
    let a = angle as f32;
    (a.sin(), -a.cos())
}

/// A filled or stroked annular-sector (donut) / circular-sector (pie) Path,
/// tagged, sharing the donut's bbox rect (commands rebased onto its origin).
fn sector_node(geom: &DonutGeom, a0: f32, a1: f32, style: PathStyle, tag: String) -> Scene {
    let commands = sector_commands(geom.cx, geom.cy, geom.r_out, geom.r_in, a0, a1);
    Scene::Path(
        PathNode::new(geom.bbox, commands, style)
            .with_tag(tag)
            .with_layout(absolute(geom.bbox)),
    )
}

/// The path commands for one sector, rect-local to `(cx, cy)`. A donut sector
/// (`r_in >= 1`) is outer-arc + inner-arc-reversed + close; a pie sector
/// (`r_in < 1`) is centre + outer-arc + close.
fn sector_commands(cx: f32, cy: f32, r_out: f32, r_in: f32, a0: f32, a1: f32) -> Vec<PathCommand> {
    let p = |r: f32, a: f32| {
        let (dx, dy) = (a.sin(), -a.cos());
        PathPoint::new(cx + r * dx, cy + r * dy)
    };
    let mut cmds = Vec::new();
    if r_in < 1.0 {
        // Pie: centre -> outer start -> outer arc -> close (back to centre).
        cmds.push(PathCommand::MoveTo(PathPoint::new(cx, cy)));
        cmds.push(PathCommand::LineTo(p(r_out, a0)));
        arc_beziers(cx, cy, r_out, a0, a1, &mut cmds);
        cmds.push(PathCommand::Close);
    } else {
        // Donut: outer arc -> inner end -> inner arc reversed -> close.
        cmds.push(PathCommand::MoveTo(p(r_out, a0)));
        arc_beziers(cx, cy, r_out, a0, a1, &mut cmds);
        cmds.push(PathCommand::LineTo(p(r_in, a1)));
        arc_beziers(cx, cy, r_in, a1, a0, &mut cmds);
        cmds.push(PathCommand::Close);
    }
    cmds
}

/// A slice value + its percent share of the total, e.g. `"12 (30%)"`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a percent 0..=100 rounds into u32 for display"
)]
fn percent_text(value: f64, total: f64) -> String {
    let pct = if total > 0.0 {
        (value / total * 100.0).round() as u32
    } else {
        0
    };
    let v = crate::ticks::format_si(value);
    format!("{v} ({pct}%)")
}

impl ChartLegend for DonutChart {
    /// A **centred** row in the reserved bottom band, which is the one place a
    /// chart's legend seat differs in this crate.
    ///
    /// (R1396) The row may use the full width, so `avail = rect.w`;
    /// [`Legend::width`] gives what it actually takes after the shared fit
    /// (shrink, then drop to `+N`), and centring by THAT keeps a narrow donut's
    /// legend inside the chart instead of overrunning its right edge.
    fn legend_seat(&self, rect: Rect, style: &ChartStyle) -> LegendSeat {
        let band = self.legend_band(style);
        let avail = rect.w;
        LegendSeat {
            x: rect.x + rect.w.saturating_sub(self.legend().width(avail)) / 2,
            y: rect.y + rect.h.saturating_sub(band).saturating_add(3),
            avail,
        }
    }

    /// One entry per slice, on when the slice is in the ring.
    ///
    /// Because a donut is a part-of-whole picture, turning one off
    /// **re-normalises** the others — see [`Slice::visible`], which is the one
    /// place this crate's two hiding rules differ.
    fn legend(&self) -> Legend {
        Legend::new(
            &self.tag_prefix,
            "Slices",
            self.slices
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    LegendEntry::new(
                        s.color.unwrap_or_else(|| self.palette.color(i)),
                        s.label.clone(),
                    )
                    .shown(s.visible)
                })
                .collect(),
        )
        .with_interaction(self.legend_interaction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_probe::{find, text_of};
    use pinion_core::scene::Rect;

    fn count_prefix(scene: &Scene, prefix: &str) -> usize {
        let mut n = usize::from(scene.tag().is_some_and(|t| t.starts_with(prefix)));
        if let Scene::Container(c) = scene {
            for ch in &c.children {
                n += count_prefix(ch, prefix);
            }
        }
        n
    }

    fn three() -> Vec<Slice> {
        vec![
            Slice::new("a", 30.0),
            Slice::new("b", 50.0),
            Slice::new("c", 20.0),
        ]
    }

    #[test]
    fn one_sector_per_positive_slice_plus_legend() {
        let scene =
            DonutChart::new(three()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        for i in 0..3 {
            assert!(
                find(&scene, &format!("chart.slice.{i}")).is_some(),
                "sector {i}"
            );
            assert!(
                find(&scene, &format!("chart.legend.{i}.swatch")).is_some(),
                "legend {i}"
            );
        }
        assert!(
            find(&scene, "chart.slice.3").is_none(),
            "no phantom 4th sector"
        );
    }

    #[test]
    fn r1722_a_hidden_slice_leaves_the_ring_without_renumbering_the_others() {
        // The index a sector carries is its SLICE's, not its position among the
        // drawn ones. Hiding the first slice must therefore remove `slice.0` and
        // leave `slice.1` / `slice.2` where they were — otherwise an agent that
        // pressed a legend entry would find every sector after it renamed.
        let mut slices = three();
        slices[0] = slices[0].clone().shown(false);
        let scene =
            DonutChart::new(slices).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(
            find(&scene, "chart.slice.0").is_none(),
            "the hidden slice draws no sector"
        );
        assert!(find(&scene, "chart.slice.1").is_some(), "and 1 is still 1");
        assert!(find(&scene, "chart.slice.2").is_some(), "and 2 is still 2");
        // Its legend entry stays — it is the toggle back on.
        assert!(find(&scene, "chart.legend.0.swatch").is_some());
    }

    #[test]
    fn r1722_the_donut_seats_its_legend_centred_in_the_bottom_band() {
        // The one thing about a legend that IS per-chart. A donut's row is
        // centred under the ring, not started at a left margin the way the
        // cartesian charts' rows are, and it is centred by the width the row
        // actually takes after the shared fit rather than by its ideal width —
        // otherwise a narrow donut pushes its own legend off its right edge.
        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        let chart = DonutChart::new(three());
        let seat = chart.legend_seat(rect, &style);
        let row = chart.legend().width(seat.avail);
        assert_eq!(
            seat.x,
            rect.x + (rect.w - row) / 2,
            "the row is centred by the width it takes"
        );
        assert!(
            seat.x + row <= rect.x + rect.w,
            "and stays inside the chart"
        );
        assert!(seat.y > rect.y + rect.h / 2, "in the BOTTOM band");
    }

    #[test]
    fn r1722_a_hidden_slice_leaves_the_total_the_shares_are_taken_of() {
        // The re-normalisation, stated as a NUMBER rather than as "the geometry
        // moved". Hiding a 30 out of 30/50/20 must make the 50 read as 50/70,
        // not 50/100 — and a sweep comparison cannot tell those apart, because
        // dropping a slice shifts every later start angle either way.
        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        let readout = |slices: Vec<Slice>| {
            DonutChart::new(slices)
                // Scrub onto the SECOND drawn sector in both builds: with `a`
                // hidden that is `c`, with everything shown it is `b`.
                .inspect(Some(0.5))
                .inspect_readout(rect, &style)
                .expect("the scrub lands on a drawn sector")
        };
        assert_eq!(readout(three()), "b = 50 (50%)", "50 of 100");
        let mut short = three();
        short[0] = short[0].clone().shown(false);
        assert_eq!(
            readout(short),
            "c = 20 (29%)",
            "and 20 of the 70 that is left, not of the 100 that is gone"
        );
    }

    #[test]
    fn r1722_hiding_a_slice_regrows_the_ring_around_what_is_left() {
        // The part-of-whole rule, and the one place this crate's two hiding
        // rules differ: a hidden series leaves its neighbours' geometry alone,
        // a hidden slice does not, because the remaining parts must still sum
        // to the whole.
        let rect = Rect::new(0, 0, 400, 300);
        let style = ChartStyle::default();
        let sweep_of = |slices: Vec<Slice>| {
            let scene = DonutChart::new(slices).build(rect, &style);
            let Some(Scene::Path(p)) = find(&scene, "chart.slice.1") else {
                panic!("slice 1 is drawn in both")
            };
            p.commands.clone()
        };
        let whole = sweep_of(three());
        let mut short = three();
        short[0] = short[0].clone().shown(false);
        assert_ne!(
            whole,
            sweep_of(short),
            "the surviving slices take the hidden one's share"
        );
    }

    #[test]
    fn a_sector_is_a_closed_filled_bezier_path() {
        let scene =
            DonutChart::new(three()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let Scene::Path(p) = find(&scene, "chart.slice.0").unwrap() else {
            panic!("sector is a path")
        };
        assert!(p.style.fill.is_some(), "the sector is filled");
        assert!(p.style.stroke.is_none(), "and not stroked");
        assert!(
            matches!(p.commands.first(), Some(PathCommand::MoveTo(_))),
            "starts with a MoveTo"
        );
        assert!(
            matches!(p.commands.last(), Some(PathCommand::Close)),
            "and is closed"
        );
        assert!(
            p.commands
                .iter()
                .any(|c| matches!(c, PathCommand::CurveTo { .. })),
            "the arc is drawn as cubic Béziers"
        );
    }

    #[test]
    fn a_zero_or_non_finite_slice_draws_no_sector_but_keeps_its_legend() {
        let slices = vec![
            Slice::new("ok", 10.0),
            Slice::new("zero", 0.0),
            Slice::new("nan", f64::NAN),
        ];
        let scene =
            DonutChart::new(slices).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        // Only the one positive slice draws a sector (index 0 in draw order).
        assert!(
            find(&scene, "chart.slice.0").is_some(),
            "the positive slice"
        );
        assert!(
            find(&scene, "chart.slice.1").is_none(),
            "no sector for the zero / NaN slices"
        );
        // …but every original slice keeps a legend entry.
        for i in 0..3 {
            assert!(
                find(&scene, &format!("chart.legend.{i}.swatch")).is_some(),
                "legend {i} survives"
            );
        }
    }

    #[test]
    fn a_pie_sector_starts_at_the_centre() {
        // inner_ratio 0 -> a pie: the sector's first drawn point is the centre.
        let scene = DonutChart::new(vec![Slice::new("whole", 1.0)])
            .with_inner_ratio(0.0)
            .build(Rect::new(0, 0, 400, 400), &ChartStyle::default());
        let Scene::Path(p) = find(&scene, "chart.slice.0").unwrap() else {
            panic!("pie sector is a path")
        };
        let PathCommand::MoveTo(m) = p.commands[0] else {
            panic!("pie starts with MoveTo(centre)")
        };
        // The path is rect-local, so the window point is rect.origin + command.
        // A donut has no axes, so it centres HORIZONTALLY in the full 400-wide
        // rect (cx = 200), and vertically above the reserved legend band.
        let (wx, wy) = (to_f32(p.rect.x) + m.x, to_f32(p.rect.y) + m.y);
        assert!(
            (wx - 200.0).abs() < 2.0,
            "a pie's centre is horizontally centred in the rect (x=200), got {wx}"
        );
        assert!(
            (40.0..360.0).contains(&wy),
            "its centre sits in the plot region above the legend band, got {wy}"
        );
    }

    #[test]
    fn full_circle_arc_samples_lie_on_the_circle() {
        // One slice = the whole ring. Its outer arc sweeps 0..2pi as 4 quarter
        // Béziers. Checking only the segment ENDPOINTS would be near-vacuous —
        // they sit on the circle by construction (`end: point(a)`) regardless of
        // the control points, so a `k = 0` octagon (straight chords) would pass.
        // So we also sample each Bézier at its MIDPOINT (t = 0.5), where a wrong
        // curvature bulges ~0.29r off the circle. That is what proves the arc is
        // a real circle, not a distorted blob.
        let scene = DonutChart::new(vec![Slice::new("all", 1.0)])
            .build(Rect::new(0, 0, 400, 400), &ChartStyle::default());
        let Scene::Path(p) = find(&scene, "chart.slice.0").unwrap() else {
            panic!("slice is a path")
        };
        let PathCommand::MoveTo(start) = p.commands[0] else {
            panic!("outer arc starts with MoveTo")
        };
        // Collect the 4 outer quarter segments as (P0, C1, C2, P3) tuples.
        let mut prev = (start.x, start.y);
        let mut ends = Vec::new();
        let mut segs = Vec::new();
        for c in p.commands.iter().skip(1).take(4) {
            let PathCommand::CurveTo { c1, c2, end } = c else {
                panic!("the outer arc is drawn as Béziers")
            };
            segs.push((prev, (c1.x, c1.y), (c2.x, c2.y), (end.x, end.y)));
            ends.push((end.x, end.y));
            prev = (end.x, end.y);
        }
        // Centre = mean of the 4 quarter endpoints; radius from an endpoint.
        let cx = ends.iter().map(|q| q.0).sum::<f32>() / 4.0;
        let cy = ends.iter().map(|q| q.1).sum::<f32>() / 4.0;
        let dist = |(x, y): (f32, f32)| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        let r = dist(ends[0]);
        assert!(r > 50.0, "the radius is the donut's outer radius, got {r}");
        // Cubic Bézier at t = 0.5: 1/8 P0 + 3/8 C1 + 3/8 C2 + 1/8 P3.
        for (p0, c1, c2, p3) in segs {
            let mid = (
                0.125 * p0.0 + 0.375 * c1.0 + 0.375 * c2.0 + 0.125 * p3.0,
                0.125 * p0.1 + 0.375 * c1.1 + 0.375 * c2.1 + 0.125 * p3.1,
            );
            assert!(
                (dist(mid) - r).abs() < 1.0,
                "the mid-arc point is on the circle (r={r}), got {:.2} — a wrong \
                 control-point / k factor would bulge off it",
                dist(mid)
            );
        }
    }

    #[test]
    fn no_inspect_overlay_by_default() {
        let scene =
            DonutChart::new(three()).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(find(&scene, "chart.inspect.highlight").is_none());
        assert!(find(&scene, "chart.inspect.tooltip").is_none());
    }

    #[test]
    fn inspect_scrubs_the_slices_and_shows_percent() {
        // Three slices summing to 100: a=30 -> 30%, b=50 -> 50%, c=20 -> 20%.
        for (fraction, label, pct) in [(0.0_f32, "a", "30%"), (0.5, "b", "50%"), (1.0, "c", "20%")]
        {
            let scene = DonutChart::new(three())
                .inspect(Some(fraction))
                .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
            assert!(
                find(&scene, "chart.inspect.highlight").is_some(),
                "a ring at {fraction}"
            );
            assert_eq!(
                text_of(&scene, "chart.inspect.header"),
                Some(label),
                "fraction {fraction} focuses slice {label}"
            );
            let value = text_of(&scene, "chart.inspect.value").expect("a value row");
            assert!(
                value.contains(pct),
                "slice {label} shows its {pct} share, got {value:?}"
            );
        }
    }

    #[test]
    fn inspect_highlight_is_a_stroked_ring_on_the_focused_slice() {
        let scene = DonutChart::new(three())
            .inspect(Some(0.5))
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let Scene::Path(ring) = find(&scene, "chart.inspect.highlight").unwrap() else {
            panic!("highlight is a path")
        };
        assert!(ring.style.stroke.is_some(), "the ring is stroked");
        assert!(
            ring.style.fill.is_none(),
            "and not filled (it frames, not covers)"
        );
        let Scene::Path(slice) = find(&scene, "chart.slice.1").unwrap() else {
            panic!("slice is a path")
        };
        // The ring traces the SAME sector as the focused slice: same rect, same
        // command count (one is filled, one stroked, geometry identical).
        assert_eq!(ring.rect, slice.rect, "ring shares the sector's rect");
        assert_eq!(
            ring.commands.len(),
            slice.commands.len(),
            "ring traces the same sector geometry"
        );
    }

    #[test]
    fn inspect_readout_names_the_slice_and_its_share() {
        let readout = DonutChart::new(three())
            .inspect(Some(0.5))
            .inspect_readout(Rect::new(0, 0, 400, 300), &ChartStyle::default())
            .expect("a readout when inspecting");
        assert!(readout.starts_with("b = "), "names the focus: {readout:?}");
        assert!(readout.contains("50%"), "carries the share: {readout:?}");
        assert!(
            DonutChart::new(three())
                .inspect_readout(Rect::new(0, 0, 400, 300), &ChartStyle::default())
                .is_none(),
            "no readout when inspection is off"
        );
    }

    #[test]
    fn build_fill_zero_size_is_an_empty_but_tagged_root() {
        let scene = DonutChart::new(three()).build_fill((0, 0), &ChartStyle::default());
        let Scene::Container(root) = &scene else {
            panic!("the fill-parent root is a container")
        };
        assert_eq!(root.tag.as_deref(), Some("chart"));
        assert!(root.children.is_empty(), "no body until a size feeds back");
    }

    #[test]
    fn empty_or_all_zero_draws_no_sectors() {
        let scene = DonutChart::new(vec![Slice::new("z", 0.0)])
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(count_prefix(&scene, "chart.slice."), 0, "no sectors");
        assert!(find(&scene, "chart").is_some(), "but the root still builds");
    }
}
