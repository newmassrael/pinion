//! The treemap builder — projects a list of weighted [`Tile`]s into a retained
//! [`Scene`] of area-encoded rectangles laid out by the **squarified** algorithm,
//! with in-tile labels and an optional scrub inspector.
//!
//! # Why a fourth builder rather than a `DonutChart` mode
//!
//! A treemap is the crate's SECOND part-of-whole form, but a fundamentally
//! different one from the [`DonutChart`](crate::DonutChart): a donut encodes a
//! share as an ANGLE, a treemap as an AREA. Area-encoding stays legible for far
//! more items than a donut's crowded thin sectors, and — unlike any angular
//! form — it TILES its whole frame, which is why an asset browser / disk-usage
//! panel (many weighted leaves filling a rectangle) is a treemap, not a pie. So
//! it has no axes, no gridlines, and no arc geometry; what it reuses is the
//! crate's genuinely cross-cutting core: the categorical
//! [`palette`](crate::palette), the shared [`box_node`](crate::draw) /
//! [`label_node`](crate::draw) leaves, the [`outline_box`](crate::draw)
//! highlight ring (lifted from the bar chart, R1382 — the treemap is its second
//! consumer), and the inspect [`callout`](crate::draw) tooltip the line / bar /
//! scatter / donut inspectors all emit.
//!
//! # Layout — the squarified algorithm (Bruls–Huizing–van Wijk 2000)
//!
//! The distinctive new machinery. Naive "slice-and-dice" treemaps cut the frame
//! along one axis per level, producing long thin slivers whose area is hard to
//! judge and whose labels do not fit. The squarified algorithm instead packs
//! tiles into rows along the frame's SHORTER side, greedily extending a row
//! while the next tile lowers the row's WORST aspect ratio and starting a new
//! row against the remaining sub-frame once it would raise it — keeping every
//! tile as close to square as a one-pass packing achieves. See [`squarify`] /
//! [`worst`]. Values are laid out largest-first (the precondition for the low
//! aspect ratios), so a tile's `chart.tile.{i}` index is its RANK by value, not
//! its position in the input.
//!
//! # Two variables: area AND colour (R1439)
//!
//! [`Tile::with_color_value`] + [`Treemap::color_by`] turn the treemap into the
//! two-variable display it was invented as (Wattenberg's Map of the Market,
//! 1998): the rectangle's AREA encodes one measure, its COLOUR an independent
//! second one. That needs a channel of its own — `value` is already spent on
//! area, and colouring by it would encode one number twice — which is the one
//! place this differs from the scatter's value encoding (R1438), whose `x`/`y`
//! are positional and whose `value` was therefore free.
//!
//! The legend follows the encoding, as it does on the scatter: colours that mean
//! magnitude cannot be explained by a swatch row, so the chart emits a colour
//! bar — a VERTICAL one, in a gutter carved off the right, because a treemap has
//! no axis band to lay a horizontal bar in. See [`crate::draw::BarAxis`] for why
//! a vertical bar is not a rotated horizontal one.
//!
//! # Introspection
//!
//! Every node carries a tag under `tag_prefix` (default `"chart"`): `chart.bg`,
//! one `chart.tile.{i}` filled box per positive tile (in descending-value draw
//! order — a zero / non-finite / negative tile contributes no box), and a
//! `chart.tile.{i}.label` text node for each tile large enough to hold a label
//! (a small tile is left unlabelled — the scrub inspector still names it). When
//! [`inspect`](Treemap::inspect) is set the overlay adds
//! `chart.inspect.highlight` (a ring framing the focused tile),
//! `chart.inspect.tooltip` (the callout box), `chart.inspect.header` (the
//! focused tile label) and `chart.inspect.value` (its value + percent share).
//! A value encoding adds `chart.colorbar.strip` (the gradient strip, whose stops
//! ride in the scene as data) and `chart.colorbar.tick.{k}`.
//!
//! # Coordinate contract
//!
//! Identical to [`crate::line`] / [`crate::donut`]: [`Treemap::build_fill`] is
//! the **layout-native** entry point (fill-parent root, children in the chart's
//! own `(0,0)..(w,h)` frame), [`Treemap::build`] pins to a caller rect. Read the
//! crate-level "Known limitations": the `build_fill` measured-rect seam is
//! Vello-only. (A treemap draws its tiles as [`Scene::Box`] and its labels as
//! [`Scene::Text`], both of which the TUI backend DOES render, so — unlike the
//! path-based charts — a treemap's tiles survive on TUI; only the
//! `build_fill` reactive seam stays GUI-only.)

use pinion_core::Scene;
use pinion_core::scene::{ContainerNode, Rect};
use pinion_core::style::{Color, TextAlign};

use crate::color_scale::{ColorBy, ColorScale, readable_ink};
use crate::draw::{
    BarAxis, CalloutRow, absolute, box_node, callout, color_bar, fill_parent, label_box_h,
    label_node, outline_box, to_f32, to_u32, vertical_bar_width,
};
use crate::palette::CategoricalPalette;
use crate::style::ChartStyle;

/// Uniform inset (px) between the chart `rect` and the tiled area — a thin
/// gutter so the treemap does not butt against the panel chrome.
const FRAME_INSET: u32 = 6;

/// Gap (px) carved from each tile's right + bottom edge so adjacent tiles read
/// as distinct rather than as one contiguous colour field.
const TILE_GAP: u32 = 2;

/// Minimum drawn tile WIDTH (px) for an in-tile label to be emitted. Below it
/// the label would overflow the tile, so it is omitted (the scrub inspector
/// still names the tile).
const MIN_LABEL_W: u32 = 48;

/// Width (px) of the vertical colour bar's gradient strip (R1439). Narrow: the
/// bar is a legend, and every px it takes comes off the tiled area.
const COLOR_BAR_STRIP_W: u32 = 14;

/// Gap (px) between the tiled frame and the colour bar's strip.
const COLOR_BAR_INSET: u32 = 8;

/// Narrowest tiled frame (px) worth keeping once the colour bar has taken its
/// gutter. Below this the bar is DROPPED and the treemap falls back to its
/// area-only form.
///
/// The legend loses, not the data: a treemap whose tiles have been squeezed to a
/// sliver so its legend can fit has stopped being a treemap. This is the
/// shrink-then-drop policy [`legend_fit`](crate::draw) applies to swatch rows,
/// at its blunt end — the bar has no intermediate width to shrink to, because
/// its labels are already at the narrowest slot that reads as a number.
const MIN_TILED_W: u32 = 96;

/// The light ink for an in-tile label, used on a dark tile — PURE white.
const TILE_INK_LIGHT: Color = Color::rgb(0xFF, 0xFF, 0xFF);

/// The dark ink for an in-tile label, used on a light tile — PURE black.
///
/// Both inks were the softer near-white / near-black a chrome palette prefers
/// until R1439 measured them. A value-encoded tile can be ANY colour on a
/// continuous ramp, and both shipped ramps pass through a mid-luminance band
/// (viridis's teal around `#26818E`, blue-orange's blue around `#347AB5`) where
/// contrast is scarce: the soft pair bottoms out at **4.29:1** there, under the
/// WCAG 4.5 body-text bar. Pure black/white lifts that floor to **4.60:1**, and
/// that is the CEILING — on those backgrounds no ink of any colour does better,
/// because contrast is monotone in luminance and these are its extremes. So the
/// thin margin is a property of the ramps, not a choice left on the table; a
/// softer ink would simply be unreadable there.
const TILE_INK_DARK: Color = Color::rgb(0x00, 0x00, 0x00);

/// One tile: a `label`, its `value` (its share of the total AND the area it is
/// drawn with), an optional independent `color_value` for the colour encoding,
/// and an optional per-tile `color` override.
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    /// The tile's label (drawn inside the tile when it fits + shown in the
    /// inspect tooltip).
    pub label: String,
    /// The tile's value — its share of the total is `value / sum`, and its drawn
    /// area is proportional to it.
    pub value: f64,
    /// R1439 — the tile's SECOND measure, the one
    /// [`Treemap::color_by`] encodes as colour. `None` (the default) leaves the
    /// tile on its categorical colour.
    pub color_value: Option<f64>,
    /// An optional colour override for THIS tile.
    pub color: Option<Color>,
}

impl Tile {
    /// A tile with the default (palette) colour.
    #[must_use]
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        Self {
            label: label.into(),
            value,
            color_value: None,
            color: None,
        }
    }

    /// R1439 — attach the SECOND measure, the one a
    /// [`color_by`](Treemap::color_by) encoding turns into this tile's colour.
    ///
    /// A separate channel because `value` is already SPENT: it is the tile's
    /// area. Colouring a treemap by the variable it is already sized by encodes
    /// one number twice and adds no information — the reader learns nothing from
    /// the colour that the rectangle did not already say. The classic treemap
    /// (Wattenberg's Map of the Market, 1998) is a **two-variable** display for
    /// exactly this reason: size for magnitude, colour for a second, independent
    /// measure (weight and change, bytes and age, count and error rate).
    ///
    /// This is why a scatter's `DataPoint` needs no such split (R1438): its `x`
    /// and `y` are positional, so its `value` was free to carry the colour.
    #[must_use]
    pub fn with_color_value(mut self, value: f64) -> Self {
        self.color_value = Some(value);
        self
    }

    /// Override this tile's colour.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

/// R1439 — the `(min, max)` of the [`Tile::color_value`] channel across `tiles`,
/// or `None` when not one tile carries a finite second measure.
///
/// The auto colour-domain a value-encoded treemap maps against, and the same
/// function a consumer calls to label its own legend or to pin a shared domain
/// across two treemaps. The area channel's own bounds are NOT this — a treemap's
/// `value` is normalised to the frame, never mapped to a scale.
#[must_use]
pub fn color_value_bounds(tiles: &[Tile]) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut seen = false;
    for v in tiles
        .iter()
        .filter_map(|t| t.color_value)
        .filter(|v| v.is_finite())
    {
        seen = true;
        lo = lo.min(v);
        hi = hi.max(v);
    }
    seen.then_some((lo, hi))
}

/// A treemap over weighted [`Tile`]s — the crate's area-encoded part-of-whole
/// form. Reuses the categorical `palette`, the shared draw leaves, the
/// `outline_box` highlight ring, and the inspect `callout` tooltip with the
/// other chart types.
pub struct Treemap {
    tiles: Vec<Tile>,
    palette: CategoricalPalette,
    inspect: Option<f32>,
    color_by: Option<ColorBy>,
    color_domain: Option<(f64, f64)>,
    tag_prefix: String,
}

impl Treemap {
    /// A treemap over `tiles`, using the default palette, no inspect overlay,
    /// and the `"chart"` tag prefix.
    #[must_use]
    pub fn new(tiles: Vec<Tile>) -> Self {
        Self {
            tiles,
            palette: CategoricalPalette::default(),
            inspect: None,
            color_by: None,
            color_domain: None,
            tag_prefix: "chart".to_string(),
        }
    }

    /// R1439 — colour every tile by its [`Tile::color_value`] second measure on
    /// a SEQUENTIAL ramp, making the treemap a two-variable display: area for
    /// one number, colour for another.
    ///
    /// This is the treemap's defining capability rather than a decoration. An
    /// area-only treemap answers "how big"; the reader can already see that from
    /// the rectangle. Colouring by an independent measure answers "and how is it
    /// doing" in the same glance, which is why a market map, a disk-usage map
    /// and a service map all use one — the big rectangle that is ALSO the wrong
    /// colour is the thing you were looking for.
    ///
    /// Turning it on swaps the legend the same way [`ScatterChart::color_by`]
    /// does (R1438): the tile colours no longer name categories, so the chart
    /// draws a colour bar over the value domain — here a VERTICAL one, standing
    /// in a gutter at the right, since a treemap has no axis band to lay a
    /// horizontal bar in and every tile it does have is load-bearing area.
    ///
    /// The domain comes from the data ([`color_value_bounds`]) unless pinned
    /// with [`with_color_domain`](Self::with_color_domain). A tile carrying no
    /// second measure keeps its categorical colour — the encoding covers the
    /// tiles that have the channel and does not invent a measure for the rest.
    ///
    /// [`ScatterChart::color_by`]: crate::ScatterChart::color_by
    #[must_use]
    pub fn color_by(mut self, scale: ColorScale) -> Self {
        self.color_by = Some(ColorBy::Sequential(scale));
        self
    }

    /// R1439 — colour every tile by its [`Tile::color_value`] on a DIVERGING
    /// ramp anchored at `neutral` (a target, a baseline, zero).
    ///
    /// The sibling of [`color_by`](Self::color_by) for a SIGNED second measure,
    /// which is the common case for this chart: a market map's daily change, a
    /// budget map's variance, a fleet map's drift from nominal. Each wing
    /// normalises on its own width (R1436), so on the asymmetric domain real
    /// data almost always has (a bad day is not the mirror of a good one) the
    /// neutral still lands on the ramp's centre colour instead of a third of the
    /// way up it — and the colour bar seats its neutral tick at the same
    /// fraction, so the legend reports the encoding rather than an even split.
    #[must_use]
    pub fn color_by_diverging(mut self, scale: ColorScale, neutral: f64) -> Self {
        self.color_by = Some(ColorBy::Diverging { scale, neutral });
        self
    }

    /// R1439 — pin the colour domain instead of deriving it from the data.
    ///
    /// Worth pinning whenever two treemaps must be comparable (the same colour
    /// has to mean the same measure in both) or when the scale should span a
    /// known operating range rather than this sample's own extremes.
    #[must_use]
    pub fn with_color_domain(mut self, lo: f64, hi: f64) -> Self {
        self.color_domain = Some((lo, hi));
        self
    }

    /// Override the default tile-colour palette (a tile's own
    /// [`with_color`](Tile::with_color) still wins).
    #[must_use]
    pub fn with_palette(mut self, palette: CategoricalPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Show the inspect overlay (a ring on the focused tile + a value tooltip)
    /// at `fraction` — the cursor's position as a fraction `0.0..=1.0`. Mirrors
    /// [`DonutChart::inspect`](crate::DonutChart::inspect): the fraction SCRUBS
    /// the tiles in draw order (`fraction * n` -> tile index), the natural output
    /// of a pointer-capture scrub, since pinion forwards a 1-D captured position
    /// (a 2-D geometric hover over the tile grid would need a 2-D pointer
    /// external).
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

    /// The tiles this treemap was built with.
    #[must_use]
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// Build the treemap PINNED to `rect`. See [`crate::LineChart::build`] —
    /// same contract; prefer [`Self::build_fill`] for anything layout-placed.
    #[must_use]
    pub fn build(&self, rect: Rect, style: &ChartStyle) -> Scene {
        Scene::Container(
            self.build_body(Rect::new(0, 0, rect.w, rect.h), style)
                .with_layout(absolute(rect)),
        )
    }

    /// Build the treemap as a **layout-native** subtree (R1360 contract): the
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

    /// The chart body, authored in `rect`'s frame — the ONE builder both entry
    /// points wrap.
    fn build_body(&self, rect: Rect, style: &ChartStyle) -> ContainerNode {
        let geom = self.geom(rect);
        let (highlight, tooltip) = match self.resolve_inspect(&geom, style) {
            Some(i) => (i.highlight, i.tooltip),
            None => (None, Vec::new()),
        };

        let mut children: Vec<Scene> = Vec::new();
        if let Some(bg) = style.background {
            children.push(box_node(rect, bg, format!("{}.bg", self.tag_prefix)));
        }

        let size = style.label_size_px.max(1);
        for (i, p) in geom.placed.iter().enumerate() {
            let tile = &self.tiles[p.tile];
            let color = self.tile_color(p.tile);
            children.push(box_node(
                p.rect,
                color,
                format!("{}.tile.{i}", self.tag_prefix),
            ));
            // In-tile label when the tile is large enough to hold it — drawn in a
            // colour chosen for contrast against the tile fill so it reads on both
            // light and dark palette entries.
            if p.rect.w >= MIN_LABEL_W && p.rect.h >= size + 6 {
                let pad = 4;
                children.push(label_node(
                    tile.label.clone(),
                    p.rect.x + pad,
                    p.rect.y + pad,
                    p.rect.w.saturating_sub(pad * 2),
                    TextAlign::Start,
                    contrast_text(color),
                    size,
                    format!("{}.tile.{i}.label", self.tag_prefix),
                ));
            }
        }

        // The colour bar goes in the gutter `geom` already reserved for it, so
        // it can never paint over a tile.
        children.extend(self.color_bar_column(&geom, style));

        // The highlight ring sits over the tiles (so it frames the focused one)
        // but under the tooltip.
        if let Some(highlight) = highlight {
            children.push(highlight);
        }
        children.extend(tooltip);
        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
    }

    /// R1439 — the active colour domain: pinned, else measured off the tiles,
    /// else `None` when no tile carries the second measure (in which case there
    /// is nothing to encode and the treemap stays categorical).
    fn resolved_color_domain(&self) -> Option<(f64, f64)> {
        self.color_by.as_ref()?;
        self.color_domain
            .or_else(|| color_value_bounds(&self.tiles))
    }

    /// The fill for the tile at ORIGINAL index `idx`.
    ///
    /// Resolution order — encoding, then override, then palette. The encoding
    /// wins over a per-tile [`with_color`](Tile::with_color) wherever the tile
    /// actually carries the measure, because the colour bar publishes a
    /// colour→value claim for the whole chart and a tile painted off-scale would
    /// make that claim false. An override still colours the tiles the encoding
    /// does not reach, which is exactly where it stays meaningful.
    fn tile_color(&self, idx: usize) -> Color {
        let tile = &self.tiles[idx];
        self.encoded_color(tile)
            .or(tile.color)
            .unwrap_or_else(|| self.palette.color(idx))
    }

    /// The value-encoded colour for `tile`, or `None` when this treemap is not
    /// colouring by value, has no domain to map against, or the tile carries no
    /// finite second measure.
    fn encoded_color(&self, tile: &Tile) -> Option<Color> {
        let encoding = self.color_by.as_ref()?;
        let domain = self.resolved_color_domain()?;
        let value = tile.color_value.filter(|v| v.is_finite())?;
        Some(encoding.resolve(value, domain))
    }

    /// R1439 — the VERTICAL colour bar: the value-encoding legend, standing in
    /// the right-hand gutter [`geom`](Self::geom) reserved for it.
    ///
    /// Vertical because a treemap has nowhere else to put it. The other charts
    /// lay a horizontal bar across a legend band above the plot; a treemap has
    /// no such band — it tiles its entire frame, which is the whole point of the
    /// form — so the bar has to take its space from the side, where a narrow
    /// column costs the least area. That is also the orientation a reader
    /// expects of a value ramp: high at the top.
    fn color_bar_column(&self, geom: &TreemapGeom, style: &ChartStyle) -> Vec<Scene> {
        let (Some(encoding), Some(domain)) = (self.color_by.as_ref(), self.resolved_color_domain())
        else {
            return Vec::new();
        };
        let stops = encoding.bar_stops(domain);
        if stops.is_empty() || geom.bar_gutter == 0 {
            return Vec::new();
        }
        // The strip stands in the gutter, inset from the tiles. Its ENDS are
        // pulled in by half a label box, because a tick centred on an end must
        // fit inside the chart: without the inset the bottom label hangs below
        // the frame — the R1396 "a chart paints only inside its own rect" rule,
        // on the other axis.
        let end_inset = label_box_h(style.label_size_px.max(1)).div_ceil(2);
        let strip = Rect::new(
            geom.frame.x + geom.frame.w + COLOR_BAR_INSET,
            geom.frame.y + end_inset,
            COLOR_BAR_STRIP_W,
            geom.frame.h.saturating_sub(end_inset * 2).max(1),
        );
        color_bar(
            &stops,
            &encoding.bar_ticks(domain),
            strip,
            BarAxis::Vertical,
            style,
            &self.tag_prefix,
        )
    }

    /// The treemap geometry: the inset frame, the total, and the per-drawn-tile
    /// rectangles from the squarified layout — the ONE definition the painted
    /// tiles, the labels, the highlight ring, and the inspect hit-test all read
    /// (so the ring lands exactly on its tile).
    fn geom(&self, rect: Rect) -> TreemapGeom {
        // The inset frame the tiles pack into (a thin uniform gutter). Style-
        // independent today — the treemap has no per-style layout knob (no axes,
        // no legend band); `build_body` / `resolve_inspect` pass style only for
        // the tile / label / overlay COLOURS, not the geometry.
        //
        // R1439 — a live value encoding additionally takes a column off the
        // right for the vertical colour bar, so the tiles re-pack into the
        // narrower frame instead of being painted over by the legend. On a
        // chart too narrow to afford both, the BAR is dropped (see
        // [`MIN_TILED_W`]) rather than the gutter overrunning the frame.
        let want_gutter = COLOR_BAR_INSET + vertical_bar_width(COLOR_BAR_STRIP_W);
        let inner_w = rect.w.saturating_sub(FRAME_INSET * 2);
        let bar_gutter =
            if self.resolved_color_domain().is_some() && inner_w >= want_gutter + MIN_TILED_W {
                want_gutter
            } else {
                0
            };
        let frame = Rect::new(
            rect.x + FRAME_INSET,
            rect.y + FRAME_INSET,
            rect.w
                .saturating_sub(FRAME_INSET * 2)
                .saturating_sub(bar_gutter),
            rect.h.saturating_sub(FRAME_INSET * 2),
        );
        let (fx, fy, fw, fh) = (
            to_f32(frame.x),
            to_f32(frame.y),
            to_f32(frame.w),
            to_f32(frame.h),
        );

        // Positive finite tiles, largest first (the squarified precondition).
        let mut items: Vec<(usize, f64)> = self
            .tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| t.value.is_finite() && t.value > 0.0)
            .map(|(i, t)| (i, t.value))
            .collect();
        items.sort_by(|a, b| b.1.total_cmp(&a.1));

        let total: f64 = items.iter().map(|(_, v)| *v).sum();
        let placed = if items.is_empty() || fw <= 0.0 || fh <= 0.0 || total <= 0.0 {
            Vec::new()
        } else {
            // Normalise the values to px² so the aspect ratios `worst` compares
            // are in true screen proportions.
            let frame_area = fw * fh;
            #[allow(
                clippy::cast_possible_truncation,
                reason = "value f64 -> f32 area fraction; display-bounded magnitudes"
            )]
            let areas: Vec<f32> = items
                .iter()
                .map(|(_, v)| (*v / total) as f32 * frame_area)
                .collect();
            let rects = squarify(&areas, (fx, fy, fw, fh));
            rects
                .iter()
                .zip(&items)
                .map(|(&(x, y, w, h), &(orig, _))| {
                    // Carve the inter-tile gap from the right + bottom edge, then
                    // round to a device rect (min 1px so every tile stays
                    // addressable).
                    let dw = to_u32(w).saturating_sub(TILE_GAP).max(1);
                    let dh = to_u32(h).saturating_sub(TILE_GAP).max(1);
                    Placed {
                        tile: orig,
                        rect: Rect::new(to_u32(x), to_u32(y), dw, dh),
                    }
                })
                .collect()
        };

        TreemapGeom {
            placed,
            total,
            frame,
            bar_gutter,
        }
    }

    /// Resolve which drawn tile the inspect cursor is over: `fraction * n` SCRUBS
    /// the tiles in draw (descending-value) order. `None` when inspection is off
    /// / there are no drawn tiles.
    fn resolve_focus(&self, geom: &TreemapGeom) -> Option<usize> {
        let fraction = self.inspect?;
        let n = geom.placed.len();
        if n == 0 {
            return None;
        }
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "fraction is 0..=1 and n a small tile count; the product is a valid tile index, clamped below"
        )]
        let idx = (fraction.clamp(0.0, 1.0) * n as f32).floor() as usize;
        Some(idx.min(n - 1))
    }

    /// Resolve the inspect overlay: the focused tile framed by a ring + a value
    /// tooltip (`{label}` header + a `{value} ({pct}%)` row).
    fn resolve_inspect(&self, geom: &TreemapGeom, style: &ChartStyle) -> Option<TreemapInspect> {
        let idx = self.resolve_focus(geom)?;
        let p = &geom.placed[idx];
        let tile = &self.tiles[p.tile];
        let highlight = Some(outline_box(
            p.rect,
            style.crosshair,
            format!("{}.inspect.highlight", self.tag_prefix),
        ));
        let anchor_x = to_f32(p.rect.x) + to_f32(p.rect.w) / 2.0;
        let rows = vec![CalloutRow {
            text: percent_text(tile.value, geom.total),
            color: style.tooltip_fg,
            tag: format!("{}.inspect.value", self.tag_prefix),
        }];
        let tooltip = callout(
            anchor_x,
            to_f32(geom.frame.x + geom.frame.w),
            to_f32(geom.frame.y),
            &tile.label,
            format!("{}.inspect.header", self.tag_prefix),
            &rows,
            style,
            format!("{}.inspect.tooltip", self.tag_prefix),
        );
        Some(TreemapInspect { highlight, tooltip })
    }

    /// The inspect readout as one line — the focus label + its value + percent
    /// share, or `None` when nothing is inspected. A consumer wires this into
    /// the scrub control's `WidgetA11y` node (the R1355 parity, now for the
    /// treemap). Takes no `ChartStyle` (unlike the donut's) because the
    /// treemap's geometry — and so which tile a fraction resolves — is
    /// style-independent.
    #[must_use]
    pub fn inspect_readout(&self, rect: Rect) -> Option<String> {
        let geom = self.geom(rect);
        let idx = self.resolve_focus(&geom)?;
        let tile = &self.tiles[geom.placed[idx].tile];
        Some(format!(
            "{} = {}",
            tile.label,
            percent_text(tile.value, geom.total)
        ))
    }
}

/// One placed tile: which ORIGINAL tile (into `self.tiles`, for its colour +
/// label), and its final drawn device rect (post-gap, post-round). The rect is
/// the SSOT the painted box, the label, and the highlight ring share.
struct Placed {
    tile: usize,
    rect: Rect,
}

/// The resolved treemap geometry: the placed tiles (descending-value draw
/// order), the value total (for percent shares), and the inset frame (the
/// tooltip's clamp bounds).
struct TreemapGeom {
    placed: Vec<Placed>,
    total: f64,
    frame: Rect,
    /// R1439 — width (px) taken off the right of the frame for the vertical
    /// colour bar, `0` when no value encoding is live. Carried on the geometry
    /// rather than recomputed at the draw site so the tiles and the bar cannot
    /// disagree about who owns that column.
    bar_gutter: u32,
}

/// The resolved inspect overlay: the focused tile's ring + a value tooltip.
struct TreemapInspect {
    highlight: Option<Scene>,
    tooltip: Vec<Scene>,
}

/// The maximum aspect ratio (>= 1) of the rectangles in `row` — a slice of tile
/// areas (px²) — if they were laid along a strip of length `w` px. The Bruls–
/// Huizing–van Wijk (2000) `worst` function: a row summing to `s` with largest
/// area `rmax` and smallest `rmin` has its thinnest and fattest tiles at aspect
/// ratios `w²·rmax / s²` and `s² / (w²·rmin)`; the worse of the two is what
/// [`squarify`] minimises when deciding whether to extend the row.
fn worst(row: &[f32], w: f32) -> f32 {
    let s: f32 = row.iter().sum();
    if s <= 0.0 || w <= 0.0 {
        return f32::INFINITY;
    }
    let rmax = row.iter().copied().fold(0.0_f32, f32::max);
    let rmin = row.iter().copied().fold(f32::INFINITY, f32::min);
    let w2 = w * w;
    let s2 = s * s;
    (w2 * rmax / s2).max(s2 / (w2 * rmin))
}

/// The squarified treemap layout (Bruls–Huizing–van Wijk 2000): tile `areas`
/// (px², summing to the frame's area, largest first) into `frame` = `(x, y, w,
/// h)` as rectangles whose aspect ratios stay as close to square as a greedy
/// row-packing achieves. Returns one `(x, y, w, h)` per input area, IN THE SAME
/// ORDER. Tiles are laid in rows along the frame's SHORTER side: a row is
/// extended while adding the next tile lowers its [`worst`] aspect ratio, and a
/// fresh row is started against the remaining sub-frame once it would raise it.
fn squarify(areas: &[f32], frame: (f32, f32, f32, f32)) -> Vec<(f32, f32, f32, f32)> {
    let (mut fx, mut fy, mut fw, mut fh) = frame;
    let mut out: Vec<(f32, f32, f32, f32)> = Vec::with_capacity(areas.len());
    let mut i = 0;
    while i < areas.len() {
        let short = fw.min(fh);
        if short <= 0.0 {
            // No paintable space left — emit degenerate rects for the remainder
            // so the output stays index-parallel with the input.
            out.extend(areas[i..].iter().map(|_| (fx, fy, 0.0, 0.0)));
            break;
        }
        // Grow the row [i, end) while the next tile improves (does not worsen)
        // the row's worst aspect ratio.
        let mut end = i + 1;
        while end < areas.len() && worst(&areas[i..=end], short) <= worst(&areas[i..end], short) {
            end += 1;
        }
        let row = &areas[i..end];
        let row_sum: f32 = row.iter().sum();
        let thickness = row_sum / short;
        if fw <= fh {
            // A horizontal band across the top: it spans the full width (= short)
            // and is `thickness` deep; tiles fill it left to right.
            let mut cx = fx;
            for &a in row {
                let tw = a / thickness;
                out.push((cx, fy, tw, thickness));
                cx += tw;
            }
            fy += thickness;
            fh -= thickness;
        } else {
            // A vertical band down the left: it spans the full height (= short)
            // and is `thickness` wide; tiles fill it top to bottom.
            let mut cy = fy;
            for &a in row {
                let th = a / thickness;
                out.push((fx, cy, thickness, th));
                cy += th;
            }
            fx += thickness;
            fw -= thickness;
        }
        i = end;
    }
    out
}

/// A readable text colour for a label drawn ON `bg` — light ink on a dark tile,
/// dark ink on a light one.
///
/// R1439 replaced a hand-rolled version of this (a re-typed BT.709 luminance sum
/// plus a `0.179` threshold) with the crate's [`readable_ink`], which R1436
/// lifted out of `hello-heatmap` after finding the same code there. The
/// threshold form was defensible for the eight fixed palette colours it was
/// written for; it is not for a value encoding, where a tile can be ANY colour
/// on a continuous ramp. `0.179` is the crossover for pure white against pure
/// black, so the moment the inks are anything else — as they are here — the
/// threshold sits slightly off the true crossover and mis-picks the ink for
/// backgrounds inside that band. Measuring both ratios and taking the larger is
/// correct for every background by construction, which is what a ramp needs.
fn contrast_text(bg: Color) -> Color {
    readable_ink(bg, TILE_INK_LIGHT, TILE_INK_DARK)
}

/// A tile value + its percent share of the total, e.g. `"12 (30%)"`. The
/// part-of-whole share readout — identical in shape to the donut's (the second
/// consumer of this format; a third part-of-whole chart would lift it to a
/// shared helper).
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

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::Rect;
    use pinion_core::style::SizeValue;

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

    fn count_prefix(scene: &Scene, prefix: &str) -> usize {
        let mut n = usize::from(scene.tag().is_some_and(|t| t.starts_with(prefix)));
        if let Scene::Container(c) = scene {
            for ch in &c.children {
                n += count_prefix(ch, prefix);
            }
        }
        n
    }

    fn text_of<'a>(scene: &'a Scene, tag: &str) -> Option<&'a str> {
        match find(scene, tag)? {
            Scene::Text(t) => Some(t.content.as_str()),
            _ => None,
        }
    }

    fn tile_rect(scene: &Scene, tag: &str) -> Rect {
        let Scene::Box(b) = find(scene, tag).unwrap_or_else(|| panic!("tile {tag} present")) else {
            panic!("tile {tag} is a box")
        };
        b.rect
    }

    /// Eight varied assets, already descending — so a tile's draw index equals
    /// its input index, which keeps the assertions readable. Sum = 680.
    fn assets() -> Vec<Tile> {
        vec![
            Tile::new("Textures", 240.0),
            Tile::new("Meshes", 180.0),
            Tile::new("Audio", 96.0),
            Tile::new("Animations", 62.0),
            Tile::new("Shaders", 44.0),
            Tile::new("Scripts", 30.0),
            Tile::new("Materials", 20.0),
            Tile::new("Fonts", 8.0),
        ]
    }

    #[test]
    fn one_box_per_positive_tile_no_phantom() {
        let scene = Treemap::new(assets()).build(Rect::new(0, 0, 640, 400), &ChartStyle::default());
        for i in 0..assets().len() {
            let Scene::Box(_) = find(&scene, &format!("chart.tile.{i}")).unwrap() else {
                panic!("tile {i} is a box")
            };
        }
        assert!(
            find(&scene, &format!("chart.tile.{}", assets().len())).is_none(),
            "no phantom tile past the input"
        );
    }

    #[test]
    fn a_zero_or_non_finite_tile_draws_no_box() {
        let tiles = vec![
            Tile::new("ok", 10.0),
            Tile::new("zero", 0.0),
            Tile::new("nan", f64::NAN),
        ];
        let scene = Treemap::new(tiles).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        // Only the one positive tile draws (index 0 in draw order); the zero /
        // NaN tiles contribute nothing.
        assert_eq!(
            count_prefix(&scene, "chart.tile."),
            2, // the box + its in-tile label
            "only the positive tile (box + label) is drawn"
        );
        assert!(find(&scene, "chart.tile.0").is_some(), "the positive tile");
        assert!(find(&scene, "chart.tile.1").is_none(), "no second tile");
    }

    #[test]
    fn tiles_partition_the_frame() {
        // A treemap TILES its whole frame — the drawn tiles fill most of it and
        // every tile sits inside the inset frame. (Some area is lost to the
        // inter-tile gaps + rounding, hence the 0.6 floor rather than ~1.0.)
        const W: u32 = 600;
        const H: u32 = 400;
        let scene = Treemap::new(assets()).build(Rect::new(0, 0, W, H), &ChartStyle::default());
        let frame = Rect::new(
            FRAME_INSET,
            FRAME_INSET,
            W - FRAME_INSET * 2,
            H - FRAME_INSET * 2,
        );
        let frame_area = f64::from(frame.w) * f64::from(frame.h);
        let mut covered = 0.0_f64;
        for i in 0..assets().len() {
            let r = tile_rect(&scene, &format!("chart.tile.{i}"));
            assert!(
                r.x >= frame.x && r.y >= frame.y,
                "tile {i} starts inside the frame"
            );
            assert!(
                r.x + r.w <= frame.x + frame.w + TILE_GAP
                    && r.y + r.h <= frame.y + frame.h + TILE_GAP,
                "tile {i} ends inside the frame"
            );
            covered += f64::from(r.w) * f64::from(r.h);
        }
        assert!(
            covered >= frame_area * 0.6,
            "the tiles fill most of the frame ({covered} of {frame_area})"
        );
    }

    #[test]
    fn bigger_value_gets_bigger_area() {
        let scene = Treemap::new(assets()).build(Rect::new(0, 0, 600, 400), &ChartStyle::default());
        let area = |i: usize| {
            let r = tile_rect(&scene, &format!("chart.tile.{i}"));
            u64::from(r.w) * u64::from(r.h)
        };
        // Textures (240) is the largest tile, Fonts (8) the smallest.
        assert!(area(0) > area(7), "the largest value gets the largest tile");
        assert!(
            area(0) > area(1) && area(1) > area(2),
            "tile area tracks value in rank order"
        );
    }

    #[test]
    fn squarified_keeps_aspect_ratios_low() {
        // The whole point of squarified over slice-and-dice: no long thin
        // slivers. Every tile's aspect ratio (long side / short side) stays
        // modest. A naive one-axis slice of 8 items into a 3:2 frame would push
        // the smallest tile past 10:1; squarified keeps them all well under.
        let scene = Treemap::new(assets()).build(Rect::new(0, 0, 600, 400), &ChartStyle::default());
        for i in 0..assets().len() {
            let r = tile_rect(&scene, &format!("chart.tile.{i}"));
            let (w, h) = (f64::from(r.w.max(1)), f64::from(r.h.max(1)));
            let aspect = (w / h).max(h / w);
            assert!(
                aspect < 5.0,
                "tile {i} stays roughly square (aspect {aspect:.2} < 5)"
            );
        }
    }

    #[test]
    fn a_large_tile_carries_an_in_tile_label() {
        // The dominant tile is big enough to hold its label in-tile.
        let scene = Treemap::new(assets()).build(Rect::new(0, 0, 600, 400), &ChartStyle::default());
        assert_eq!(
            text_of(&scene, "chart.tile.0.label"),
            Some("Textures"),
            "the largest tile is labelled in place"
        );
    }

    #[test]
    fn a_tiny_tile_is_left_unlabelled() {
        // One dominant tile + a sliver: the sliver is below the label size gate,
        // so it draws a box but no in-tile label (the scrub still names it).
        let tiles = vec![Tile::new("huge", 1000.0), Tile::new("x", 1.0)];
        let scene = Treemap::new(tiles).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert!(
            find(&scene, "chart.tile.0.label").is_some(),
            "huge labelled"
        );
        assert!(
            find(&scene, "chart.tile.1").is_some(),
            "the sliver still draws a box"
        );
        assert!(
            find(&scene, "chart.tile.1.label").is_none(),
            "but the sliver is too small for an in-tile label"
        );
    }

    #[test]
    fn no_inspect_overlay_by_default() {
        let scene = Treemap::new(assets()).build(Rect::new(0, 0, 600, 400), &ChartStyle::default());
        assert!(find(&scene, "chart.inspect.highlight").is_none());
        assert!(find(&scene, "chart.inspect.tooltip").is_none());
        assert!(find(&scene, "chart.inspect.header").is_none());
        assert!(find(&scene, "chart.inspect.value").is_none());
    }

    #[test]
    fn inspect_scrubs_the_tiles_and_shows_percent() {
        // Eight tiles by rank: 0.0 -> Textures (240/680 = 35%), 1.0 -> Fonts
        // (8/680 = 1%). The fraction scrubs the tiles in descending-value order.
        for (fraction, label, pct) in [(0.0_f32, "Textures", "35%"), (1.0, "Fonts", "1%")] {
            let scene = Treemap::new(assets())
                .inspect(Some(fraction))
                .build(Rect::new(0, 0, 600, 400), &ChartStyle::default());
            assert!(
                find(&scene, "chart.inspect.highlight").is_some(),
                "a ring at {fraction}"
            );
            assert_eq!(
                text_of(&scene, "chart.inspect.header"),
                Some(label),
                "fraction {fraction} focuses tile {label}"
            );
            let value = text_of(&scene, "chart.inspect.value").expect("a value row");
            assert!(
                value.contains(pct),
                "tile {label} shows its {pct} share, got {value:?}"
            );
        }
    }

    #[test]
    fn inspect_highlight_frames_exactly_the_focused_tile() {
        // The geom SSOT: the ring's rect equals the focused tile's own box rect,
        // so the ring can never drift off its tile.
        let scene = Treemap::new(assets())
            .inspect(Some(0.0)) // -> Textures (tile 0)
            .build(Rect::new(0, 0, 600, 400), &ChartStyle::default());
        let Scene::Box(ring) = find(&scene, "chart.inspect.highlight").expect("highlight") else {
            panic!("highlight is a box")
        };
        assert!(ring.style.border.is_some(), "the highlight is a ring");
        assert!(
            ring.style.fill == Color::TRANSPARENT,
            "the ring frames, it does not cover"
        );
        assert_eq!(
            ring.rect,
            tile_rect(&scene, "chart.tile.0"),
            "the ring frames exactly the focused tile"
        );
    }

    #[test]
    fn inspect_readout_names_the_tile_and_its_share() {
        let readout = Treemap::new(assets())
            .inspect(Some(0.0))
            .inspect_readout(Rect::new(0, 0, 600, 400))
            .expect("a readout when inspecting");
        assert!(
            readout.starts_with("Textures = "),
            "names the focus: {readout:?}"
        );
        assert!(readout.contains("35%"), "carries the share: {readout:?}");
        assert!(
            Treemap::new(assets())
                .inspect_readout(Rect::new(0, 0, 600, 400))
                .is_none(),
            "no readout when inspection is off"
        );
    }

    #[test]
    fn inspect_tooltip_flips_left_at_the_right_edge_and_stays_in_the_frame() {
        // A tile near the right edge would push a right-placed tooltip off the
        // frame; the callout flips it left so it stays inside the chart width.
        const W: u32 = 600;
        // Two tiles: the second (smaller) tends to sit to the right. Scrub to it.
        let scene = Treemap::new(assets())
            .inspect(Some(0.95))
            .build(Rect::new(0, 0, W, 400), &ChartStyle::default());
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

    #[test]
    fn build_fill_zero_size_is_an_empty_but_tagged_root() {
        let scene = Treemap::new(assets()).build_fill((0, 0), &ChartStyle::default());
        let Scene::Container(root) = &scene else {
            panic!("the fill-parent root is a container")
        };
        assert_eq!(root.tag.as_deref(), Some("chart"));
        assert!(root.children.is_empty(), "no body until a size feeds back");
    }

    #[test]
    fn a_pinned_tag_prefix_renames_every_node() {
        let scene = Treemap::new(assets())
            .with_tag_prefix("assets")
            .inspect(Some(0.5))
            .build(Rect::new(0, 0, 600, 400), &ChartStyle::default());
        assert!(find(&scene, "assets.tile.0").is_some());
        assert!(find(&scene, "assets.inspect.highlight").is_some());
        assert_eq!(
            count_prefix(&scene, "chart."),
            0,
            "no default prefix leaks through"
        );
    }

    #[test]
    fn empty_or_all_zero_draws_no_tiles_but_the_root_builds() {
        let scene = Treemap::new(vec![Tile::new("z", 0.0)])
            .build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        assert_eq!(count_prefix(&scene, "chart.tile."), 0, "no tiles");
        assert!(find(&scene, "chart").is_some(), "but the root still builds");
    }

    #[test]
    fn a_per_tile_colour_override_wins_over_the_palette() {
        let red = Color::rgb(0xE0, 0x40, 0x40);
        let tiles = vec![Tile::new("a", 100.0), Tile::new("b", 60.0).with_color(red)];
        let scene = Treemap::new(tiles).build(Rect::new(0, 0, 400, 300), &ChartStyle::default());
        let fill = |i: usize| {
            let Scene::Box(b) = find(&scene, &format!("chart.tile.{i}")).unwrap() else {
                panic!("tile is a box")
            };
            b.style.fill
        };
        assert_eq!(fill(1), red, "the overridden tile carries its own colour");
        assert_ne!(fill(0), red, "the default tile does not");
    }

    // --- R1439: the second variable ------------------------------------------

    /// Tiles whose AREA rank and COLOUR rank deliberately disagree — the whole
    /// point of a two-variable treemap, and the shape every test below needs.
    fn two_variable_tiles() -> Vec<Tile> {
        vec![
            // biggest area, worst change
            Tile::new("alpha", 100.0).with_color_value(-4.0),
            // middle area, on target
            Tile::new("bravo", 60.0).with_color_value(0.0),
            // smallest area, best change
            Tile::new("charlie", 30.0).with_color_value(12.0),
        ]
    }

    fn built(map: &Treemap) -> Scene {
        map.build(Rect::new(0, 0, 400, 300), &ChartStyle::default())
    }

    fn tile_fill(scene: &Scene, i: usize) -> Color {
        let Scene::Box(b) = find(scene, &format!("chart.tile.{i}")).expect("tile present") else {
            panic!("tile is a box")
        };
        b.style.fill
    }

    fn strip_stops(scene: &Scene) -> Vec<(f32, Color)> {
        let Some(Scene::Box(b)) = find(scene, "chart.colorbar.strip") else {
            panic!("the colour bar strip is a box")
        };
        b.style
            .gradient
            .as_ref()
            .expect("the strip carries a real gradient")
            .stops
            .iter()
            .map(|s| (s.offset, s.color))
            .collect()
    }

    #[test]
    fn r1439_color_value_bounds_measures_only_the_second_channel() {
        let tiles = two_variable_tiles();
        assert_eq!(color_value_bounds(&tiles), Some((-4.0, 12.0)));
        // The area channel is NOT the colour domain — a treemap normalises area
        // to its frame and never maps it to a scale.
        assert!(
            tiles.iter().map(|t| t.value).all(|v| v > 0.0),
            "the area values are positive and unrelated to the colour bounds"
        );
        assert_eq!(
            color_value_bounds(&[Tile::new("plain", 1.0)]),
            None,
            "a tile with no second measure yields no domain"
        );
        assert_eq!(
            color_value_bounds(&[Tile::new("bad", 1.0).with_color_value(f64::NAN)]),
            None,
            "a non-finite measure is skipped, not propagated"
        );
    }

    /// The encoding ranks the second measure, so a tile's colour has nothing to
    /// do with its area — the property that makes the display two-variable.
    #[test]
    fn r1439_colour_ranks_the_second_measure_not_the_area() {
        let scene = built(&Treemap::new(two_variable_tiles()).color_by(ColorScale::viridis()));
        // Draw order is by DESCENDING AREA: 0=alpha (-4), 1=bravo (0), 2=charlie (+12).
        let ends = (tile_fill(&scene, 0), tile_fill(&scene, 2));
        assert_eq!(
            ends.0,
            ColorScale::viridis().map(-4.0, -4.0, 12.0),
            "the largest tile takes the colour of the LOWEST measure"
        );
        assert_eq!(
            ends.1,
            ColorScale::viridis().map(12.0, -4.0, 12.0),
            "the smallest tile takes the colour of the HIGHEST measure"
        );
        assert_ne!(ends.0, ends.1, "and those are different colours");
    }

    /// Equal-area tiles with different measures separate by colour; equal
    /// measures with different areas do not. Colour tracks the measure alone.
    #[test]
    fn r1439_equal_areas_still_differ_by_measure() {
        let tiles = vec![
            Tile::new("a", 50.0).with_color_value(0.0),
            Tile::new("b", 50.0).with_color_value(10.0),
            Tile::new("c", 20.0).with_color_value(10.0),
        ];
        let scene = built(&Treemap::new(tiles).color_by(ColorScale::viridis()));
        assert_ne!(
            tile_fill(&scene, 0),
            tile_fill(&scene, 1),
            "same area, different measure -> different colour"
        );
        assert_eq!(
            tile_fill(&scene, 1),
            tile_fill(&scene, 2),
            "different area, same measure -> same colour"
        );
    }

    /// The encoding wins over a per-tile override where the tile carries the
    /// measure, so the colour bar's colour->value claim stays true for every
    /// tile it covers; the override still colours a tile the encoding misses.
    #[test]
    fn r1439_the_encoding_outranks_an_override_but_not_an_unmeasured_tile() {
        let red = Color::rgb(0xE0, 0x40, 0x40);
        let tiles = vec![
            Tile::new("measured", 100.0)
                .with_color_value(5.0)
                .with_color(red),
            Tile::new("unmeasured", 60.0).with_color(red),
        ];
        let scene = built(&Treemap::new(tiles).color_by(ColorScale::viridis()));
        assert_ne!(
            tile_fill(&scene, 0),
            red,
            "the measured tile takes the encoding, not the override"
        );
        assert_eq!(
            tile_fill(&scene, 0),
            ColorScale::viridis().map(5.0, 5.0, 5.0),
            "and it is the ramp colour for its measure"
        );
        assert_eq!(
            tile_fill(&scene, 1),
            red,
            "the unmeasured tile keeps its override"
        );
    }

    /// A value encoding replaces nothing categorical here (a treemap has no
    /// swatch row) but it DOES add the bar — and takes the space for it out of
    /// the tiled frame rather than painting over a tile.
    #[test]
    fn r1439_the_bar_takes_its_gutter_from_the_tiles() {
        let plain = built(&Treemap::new(two_variable_tiles()));
        let encoded = built(&Treemap::new(two_variable_tiles()).color_by(ColorScale::viridis()));
        assert!(
            find(&plain, "chart.colorbar.strip").is_none(),
            "no bar without an encoding"
        );
        let Some(Scene::Box(strip)) = find(&encoded, "chart.colorbar.strip") else {
            panic!("the encoded treemap has a bar")
        };
        assert!(strip.rect.h > 0 && strip.rect.w > 0, "the strip is visible");

        let widest = |scene: &Scene| {
            (0..3)
                .map(|i| {
                    let r = tile_rect(scene, &format!("chart.tile.{i}"));
                    r.x + r.w
                })
                .max()
                .expect("three tiles")
        };
        assert!(
            widest(&encoded) < widest(&plain),
            "the tiles re-pack into a narrower frame ({} < {})",
            widest(&encoded),
            widest(&plain)
        );
        assert!(
            widest(&encoded) <= strip.rect.x,
            "and no tile reaches into the bar's column"
        );
    }

    /// ★ The vertical bar is not a rotated horizontal one: the value axis runs
    /// UP while the gradient paints DOWN, so the stops are mirrored. Reverting
    /// that mirror would silently publish an upside-down legend.
    #[test]
    fn r1439_a_vertical_bar_mirrors_its_stops_so_high_paints_at_the_top() {
        let scale = ColorScale::viridis();
        let scene = built(&Treemap::new(two_variable_tiles()).color_by(scale.clone()));
        let stops = strip_stops(&scene);
        assert!(stops.len() >= 2, "a ramp has stops");
        // Offsets ascend (the form a gradient is defined on) …
        assert!(
            stops.windows(2).all(|w| w[0].0 <= w[1].0),
            "gradient stops ascend: {stops:?}"
        );
        // … while the COLOURS descend the ramp, because offset 0 is the top of a
        // vertical gradient and the top of a value axis is its HIGH end.
        assert_eq!(
            stops.first().expect("a first stop").1,
            *scale.stops().last().expect("a last ramp colour"),
            "the ramp's high colour paints at the strip's TOP"
        );
        assert_eq!(
            stops.last().expect("a last stop").1,
            scale.stops()[0],
            "and its low colour at the BOTTOM"
        );
    }

    /// The bar's neutral tick is seated by the encoding, and — being vertical —
    /// measured from the BOTTOM. On this asymmetric domain (-4 .. +12) the
    /// neutral is a quarter of the way up, so its label sits three quarters of
    /// the way DOWN the strip.
    #[test]
    fn r1439_the_diverging_neutral_seats_from_the_bottom() {
        let scene = built(
            &Treemap::new(two_variable_tiles())
                .color_by_diverging(ColorScale::blue_orange(), 0.0)
                .with_color_domain(-4.0, 12.0),
        );
        let stops = strip_stops(&scene);
        assert_eq!(stops.len(), 3, "blue_orange has three stops");
        // 0.25 of the domain, mirrored into gradient space = 0.75.
        assert!(
            (stops[1].0 - 0.75).abs() < 1e-3,
            "the neutral stop sits three quarters DOWN, got {}",
            stops[1].0
        );

        let Some(Scene::Box(strip)) = find(&scene, "chart.colorbar.strip") else {
            panic!("strip present")
        };
        // A label carries its seat in the LAYOUT (`label_node` leaves the text
        // node's own rect at the origin and places the box absolutely).
        let tick_y = |k: usize| {
            let Some(Scene::Text(t)) = find(&scene, &format!("chart.colorbar.tick.{k}")) else {
                panic!("tick {k} present")
            };
            t.layout
                .absolute_position
                .expect("a tick label is placed")
                .1
        };
        // tick 0 = domain low (bottom), tick 1 = neutral, tick 2 = domain high (top).
        assert!(tick_y(2) < tick_y(1), "the high tick is above the neutral");
        assert!(tick_y(1) < tick_y(0), "the neutral is above the low tick");
        let span = f64::from(strip.rect.h);
        let neutral_from_top = f64::from(tick_y(1) - strip.rect.y);
        assert!(
            (neutral_from_top / span - 0.75).abs() < 0.05,
            "the neutral label seats three quarters down, got {}",
            neutral_from_top / span
        );
    }

    /// ★ Every node the bar emits paints INSIDE the chart's own rect — the
    /// R1396 rule on the other axis. The bottom tick is the one that catches it:
    /// centred on the strip's low end, its box hangs below the frame unless the
    /// strip is inset by half a label box and the label is seated by its BOX
    /// height rather than its glyph size.
    #[test]
    fn r1439_the_vertical_bar_paints_only_inside_the_chart_rect() {
        let chart = Rect::new(20, 12, 420, 260);
        let scene = Treemap::new(two_variable_tiles())
            .color_by_diverging(ColorScale::blue_orange(), 0.0)
            .build(chart, &ChartStyle::default());
        let mut checked = 0;
        for tag in [
            "chart.colorbar.strip",
            "chart.colorbar.tick.0",
            "chart.colorbar.tick.1",
            "chart.colorbar.tick.2",
        ] {
            let node = find(&scene, tag).unwrap_or_else(|| panic!("{tag} present"));
            // The chart authors in its own (0,0) frame and is placed by the
            // wrapping absolute layout, so containment is against (0,0,w,h).
            let bounds = node_bounds(node, tag);
            assert!(
                bounds.x + bounds.w <= chart.w && bounds.y + bounds.h <= chart.h,
                "{tag} at {bounds:?} escapes the {}x{} chart",
                chart.w,
                chart.h
            );
            checked += 1;
        }
        assert_eq!(checked, 4, "the strip and all three ticks were checked");
    }

    /// The box a node occupies: a filled box carries its own rect, a label its
    /// absolute layout position plus the px box size [`label_node`] gave it.
    fn node_bounds(node: &Scene, tag: &str) -> Rect {
        match node {
            Scene::Box(b) => b.rect,
            Scene::Text(text) => {
                let (x, y) = text
                    .layout
                    .absolute_position
                    .unwrap_or_else(|| panic!("label {tag} is placed"));
                let px = |value: SizeValue| match value {
                    SizeValue::Px(px) => px,
                    other => panic!("label {tag} is sized in px, got {other:?}"),
                };
                Rect::new(
                    x,
                    y,
                    px(text.layout.size.width),
                    px(text.layout.size.height),
                )
            }
            _ => panic!("{tag} is a box or a label"),
        }
    }

    /// A chart too narrow to seat both the bar and a usable tiled frame DROPS
    /// the bar — the legend loses, not the data — and nothing paints outside.
    #[test]
    fn r1439_a_too_narrow_chart_drops_the_bar_rather_than_overhanging() {
        let narrow = Rect::new(0, 0, 120, 200);
        let scene = Treemap::new(two_variable_tiles())
            .color_by(ColorScale::viridis())
            .build(narrow, &ChartStyle::default());
        assert!(
            find(&scene, "chart.colorbar.strip").is_none(),
            "the bar is dropped below the minimum tiled width"
        );
        assert!(
            find(&scene, "chart.tile.0").is_some(),
            "and the tiles survive — the data outranks the legend"
        );
        // One px past the threshold the bar comes back, so the drop is a
        // boundary and not a blanket refusal.
        let wide = Rect::new(
            0,
            0,
            FRAME_INSET * 2 + MIN_TILED_W + COLOR_BAR_INSET + vertical_bar_width(COLOR_BAR_STRIP_W),
            200,
        );
        let scene = Treemap::new(two_variable_tiles())
            .color_by(ColorScale::viridis())
            .build(wide, &ChartStyle::default());
        assert!(
            find(&scene, "chart.colorbar.strip").is_some(),
            "exactly at the threshold the bar fits"
        );
    }

    /// ★ The in-tile ink clears WCAG body-text contrast across the WHOLE of
    /// every ramp the crate ships — the property a luminance threshold could
    /// only promise for the fixed palette it was tuned on.
    ///
    /// This test is why [`TILE_INK_LIGHT`] / [`TILE_INK_DARK`] are pure rather
    /// than the softer near-white / near-black they started as: those measured
    /// 4.29:1 on both ramps' mid-luminance band, i.e. FAILING, and the failure
    /// is invisible to inspection because it happens mid-ramp rather than at an
    /// endpoint you would think to look at.
    #[test]
    fn r1439_in_tile_ink_clears_wcag_across_every_shipped_ramp() {
        let worst_over = |sample: &dyn Fn(f32) -> Color| {
            (0..=200).fold(f32::INFINITY, |worst, step| {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "0..=200 is exact in f32; this is a sampling loop"
                )]
                let bg = sample(step as f32 / 200.0);
                worst.min(crate::color_scale::contrast_ratio(bg, contrast_text(bg)))
            })
        };
        let viridis = ColorScale::viridis();
        let blue_orange = ColorScale::blue_orange();
        // The categorical palette the UNencoded treemap uses is measured on the
        // same footing: it turns out to reach the scarce band too, so it is not
        // the easy case the ramps' hard case is contrasted against.
        let palette = CategoricalPalette::default();
        let worst_ratio = worst_over(&|t| viridis.sample(t))
            .min(worst_over(&|t| blue_orange.sample(t)))
            .min((0..16).fold(f32::INFINITY, |worst, i| {
                let bg = palette.color(i);
                worst.min(crate::color_scale::contrast_ratio(bg, contrast_text(bg)))
            }));
        assert!(
            worst_ratio >= 4.5,
            "in-tile labels must clear WCAG 4.5:1 everywhere, worst was {worst_ratio}"
        );

        // ★ Counterfactual: the SOFT inks this replaced do NOT clear the bar.
        // Without this the change would be indistinguishable from a taste
        // preference, and the assertion above would pass either way.
        let soft = |bg: Color| {
            readable_ink(
                bg,
                Color::rgb(0xF5, 0xF7, 0xFA),
                Color::rgb(0x14, 0x18, 0x1E),
            )
        };
        let worst_soft = (0..=200).fold(f32::INFINITY, |worst, step| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "0..=200 is exact in f32; this is a sampling loop"
            )]
            let bg = viridis.sample(step as f32 / 200.0);
            worst.min(crate::color_scale::contrast_ratio(bg, soft(bg)))
        });
        assert!(
            worst_soft < 4.5,
            "the soft inks FAIL mid-ramp ({worst_soft}) — this is what pure ones fix"
        );

        // ★ And the ramps' thin margin is the CEILING, not a choice left on the
        // table: on the worst background no ink beats the pure extremes. Without
        // this, "4.6 is the best available" would be an unchecked excuse.
        let worst_bg = Color::rgb(0x27, 0x80, 0x8E);
        let best_possible = crate::color_scale::contrast_ratio(worst_bg, TILE_INK_LIGHT)
            .max(crate::color_scale::contrast_ratio(worst_bg, TILE_INK_DARK));
        for probe in [
            Color::rgb(0x80, 0x80, 0x80),
            Color::rgb(0x30, 0x30, 0x30),
            Color::rgb(0xE0, 0xE0, 0xE0),
            Color::rgb(0xFF, 0x00, 0x00),
            Color::rgb(0x00, 0xFF, 0x00),
        ] {
            assert!(
                crate::color_scale::contrast_ratio(worst_bg, probe) <= best_possible,
                "no ink beats the pure extremes on the scarce-contrast background"
            );
        }
    }
}
