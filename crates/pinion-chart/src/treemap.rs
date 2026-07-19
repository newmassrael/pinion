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

use crate::draw::{
    CalloutRow, absolute, box_node, callout, fill_parent, label_node, outline_box, to_f32, to_u32,
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

/// One tile: a `label`, its `value` (its share of the total AND the area it is
/// drawn with), and an optional per-tile `color` override (else the palette
/// colour by the tile's original index).
#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    /// The tile's label (drawn inside the tile when it fits + shown in the
    /// inspect tooltip).
    pub label: String,
    /// The tile's value — its share of the total is `value / sum`, and its drawn
    /// area is proportional to it.
    pub value: f64,
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
            color: None,
        }
    }

    /// Override this tile's colour.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

/// A treemap over weighted [`Tile`]s — the crate's area-encoded part-of-whole
/// form. Reuses the categorical `palette`, the shared draw leaves, the
/// `outline_box` highlight ring, and the inspect `callout` tooltip with the
/// other chart types.
pub struct Treemap {
    tiles: Vec<Tile>,
    palette: CategoricalPalette,
    inspect: Option<f32>,
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
            tag_prefix: "chart".to_string(),
        }
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
            let color = tile.color.unwrap_or_else(|| self.palette.color(p.tile));
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

        // The highlight ring sits over the tiles (so it frames the focused one)
        // but under the tooltip.
        if let Some(highlight) = highlight {
            children.push(highlight);
        }
        children.extend(tooltip);
        ContainerNode::new(children).with_tag(self.tag_prefix.clone())
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
        let frame = Rect::new(
            rect.x + FRAME_INSET,
            rect.y + FRAME_INSET,
            rect.w.saturating_sub(FRAME_INSET * 2),
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

/// A readable text colour for a label drawn ON `bg`: near-white on a dark tile,
/// near-black on a light one, chosen by the tile's WCAG relative luminance in
/// LINEAR light (`0.2126 R + 0.7152 G + 0.0722 B`). The `0.179` threshold is the
/// luminance at which white-on-bg and black-on-bg give equal WCAG contrast
/// ratio, so each tile always takes the higher-contrast text — the Okabe-Ito
/// palette spans both (dark blue / black take white; yellow / sky take dark).
fn contrast_text(bg: Color) -> Color {
    let lin = bg.to_linear();
    let luminance = 0.2126 * lin.x + 0.7152 * lin.y + 0.0722 * lin.z;
    if luminance < 0.179 {
        Color::rgb(0xF5, 0xF7, 0xFA)
    } else {
        Color::rgb(0x14, 0x18, 0x1E)
    }
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
}
