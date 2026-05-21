//! R51.110.0 / R51.115 / R51.189 §5.41 §5.45 — Scene → `ratatui::Buffer`
//! paint walker with scroll viewport clipping.
//!
//! R51.110.0 first cut painted `TextNode.content` only. R51.115
//! extends the arm coverage to `BoxNode` and `ContainerNode`'s
//! `BoxStyle`: background fill (cell `bg` colour) and border
//! (Unicode box-drawing characters). `PathNode` / `ImageNode` stay
//! carry-forward — the unicode-art mapping waits for a binding
//! that needs them (Tier-1 widgets are all text + box).
//!
//! R51.189 R55.E.2 §5.45 — `Scene::Scroll` arm lands with a
//! cumulative-state cascade (`CellClip` + pixel-space offset) so
//! content inside a scroll container is offset into the viewport
//! and cells outside the viewport are skipped. Mirrors the Vello
//! R51.188 `to_vello_inner` transform shape — the public
//! [`to_buffer`] forwards `(full-clip, no-offset)` and the
//! recursion seam composes both at every Scroll arm.
//!
//! ## Pixel → cell conversion
//!
//! pinion-core's [`pinion_core::scene::Rect`] is u32 pixel-space
//! geometry (DPI-aware logical pixels, same axis as Vello's render
//! target). Terminal cells are character-grid units; the conversion
//! divides by [`PIXEL_PER_CELL_X`] / [`PIXEL_PER_CELL_Y`] which match
//! a stock 8×16 bitmap font cell (the de-facto industry baseline for
//! terminal layout math). The constants are a placeholder mapping
//! pending the §5.41 R51.111+ cell-native coord axis — once the
//! substrate-incompleteness-signal trigger (mismatched cell widths
//! against a real terminal's reported size) fires, the conversion
//! shifts to the application binding's `WidgetViewTui::initial_size`
//! cell hint.
//!
//! R51.189 adds [`pixels_to_cell_floor`] for the signed/i64 path —
//! scroll cascades can move content's screen-space origin to
//! negative pixels (content scrolled past the viewport's left or
//! top edge), so the cell index needs `div_euclid` flooring rather
//! than the truncating `/` the unsigned [`pixel_to_cell_origin`]
//! uses. Both helpers coexist: input mapping keeps unsigned, paint
//! cascade uses signed.
//!
//! ## Grapheme cluster walk
//!
//! `TextNode.content` is arbitrary UTF-8. The walker iterates
//! grapheme clusters via `unicode-segmentation`, computing the
//! display width of each via `unicode-width` (CJK / fullwidth Latin
//! / some emoji = 2 cells; narrow ASCII = 1 cell; zero-width joiners
//! / combining marks = 0 cells and merge into the preceding cell).
//! The column cursor advances by the cluster width so wide graphemes
//! reserve their second cell implicitly.

use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{BoxStyle, Color};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect as TuiRect;
use ratatui::style::Color as TuiColor;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// R51.110.0 §5.41 — pixels per terminal cell column (placeholder
/// 8×16 bitmap font baseline). The cell-native axis lands R51.111+
/// once the first hello-button TUI dogfood reports a mismatch with
/// the real terminal's reported character cell size.
pub const PIXEL_PER_CELL_X: u32 = 8;

/// R51.110.0 §5.41 — pixels per terminal cell row (placeholder
/// 8×16 bitmap font baseline). See [`PIXEL_PER_CELL_X`].
pub const PIXEL_PER_CELL_Y: u32 = 16;

// R51.130 §5.41 — Unicode light box-drawing set (U+2500..U+2518).
// Lifted as `\u{XXXX}`-escaped constants so the Rust source carries
// only ASCII codepoints; the actual glyph lives in this comment only.
// Set: ─ │ ┌ ┐ └ ┘ (horizontal, vertical, top-left, top-right,
// bottom-left, bottom-right). The light weight is intentional —
// heavy / double / rounded variants would mis-match the single-cell
// border thickness the paint walker draws (TUI cells are discrete;
// `BoxStyle::border.width` and `corner_radius` have no sub-cell
// resolution to map onto).
const BOX_HORIZONTAL: &str = "\u{2500}";
const BOX_VERTICAL: &str = "\u{2502}";
const BOX_TOP_LEFT: &str = "\u{250C}";
const BOX_TOP_RIGHT: &str = "\u{2510}";
const BOX_BOTTOM_LEFT: &str = "\u{2514}";
const BOX_BOTTOM_RIGHT: &str = "\u{2518}";

/// (R51.189 §5.45 R55.E.2) Cell-grid clip rect in absolute screen
/// cell coords (half-open, signed). Narrowed at every
/// [`Scene::Scroll`] arm so cells outside the viewport never reach
/// `Buffer::write`. Signed integers allow scrolled-out content's
/// negative cell positions to be representable without `u32` wrap
/// (mirrors the R51.181 [`Scene::hit_test`] `i64` promotion).
#[derive(Clone, Copy, Debug)]
struct CellClip {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl CellClip {
    /// Build a clip covering the entire ratatui `Buffer.area`. Used
    /// at [`to_buffer`] entry — every cell is initially writable;
    /// `Scene::Scroll` arms can only narrow this further.
    fn from_buf(area: TuiRect) -> Self {
        Self {
            x0: 0,
            y0: 0,
            x1: i32::from(area.width),
            y1: i32::from(area.height),
        }
    }

    /// Narrow `self` to the half-open intersection with `other`.
    fn intersect(self, other: Self) -> Self {
        Self {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
        }
    }

    const fn is_empty(self) -> bool {
        self.x0 >= self.x1 || self.y0 >= self.y1
    }

    const fn contains_cell(self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

/// (R51.189 §5.45 R55.E.2) Convert a signed pixel coordinate to its
/// cell index, flooring toward `-∞` so negative pixels (scrolled-out
/// content) round down rather than toward zero. `i32` saturation
/// guards against the adversarial offset case (e.g.
/// `with_offset(i32::MAX, i32::MAX)` mirroring R55.E.1).
fn pixels_to_cell_floor(px: i64, cell_size: u32) -> i32 {
    let cell_i64 = i64::from(cell_size);
    let cell_pos = px.div_euclid(cell_i64);
    i32::try_from(cell_pos).unwrap_or(if cell_pos > 0 { i32::MAX } else { i32::MIN })
}

/// (R51.189 §5.45 R55.E.2) Saturating `i64 → i32` clamp for the
/// composed scroll offset. Mirrors Vello R51.188's reliance on
/// `Affine` translation absorbing extreme values: TUI's pixel
/// offset is `i32`, but the composition (`offset + viewport - scroll`)
/// goes through `i64` to avoid wrap on adversarial inputs.
fn clamp_to_i32(v: i64) -> i32 {
    i32::try_from(v).unwrap_or(if v > 0 { i32::MAX } else { i32::MIN })
}

/// (R51.189 §5.45 R55.E.2) Translate an absolute screen-cell coord
/// to a `(u16, u16)` ratatui buffer position. Returns `None` when
/// the cell is negative or outside `buf_area`. Combined with the
/// per-write clip check, this is the final safety net: even a
/// pathologically deep scroll cascade cannot panic on `buf[(x, y)]`.
fn cell_to_buf_xy(x: i32, y: i32, buf_area: TuiRect) -> Option<(u16, u16)> {
    let bx = u16::try_from(x).ok()?;
    let by = u16::try_from(y).ok()?;
    if bx >= buf_area.width || by >= buf_area.height {
        return None;
    }
    Some((buf_area.x.saturating_add(bx), buf_area.y.saturating_add(by)))
}

/// Walk `scene` and write its painted state into `buf`. Recurses
/// through [`Scene::Container`] children, paints [`Scene::Text`]
/// nodes via [`paint_text`], skips other primitives (R51.111+
/// extends the arm coverage). The walker is idempotent on `buf` —
/// repeated calls with the same scene produce the same cell state;
/// callers reset `buf` between frames if they want a clean redraw.
///
/// R51.189 §5.45 R55.E.2 — public surface unchanged. The body
/// forwards into [`to_buffer_inner`] with a full-buffer clip and
/// no offset; [`Scene::Scroll`] children compose both as the
/// recursion descends.
pub fn to_buffer(scene: &Scene, buf: &mut Buffer) {
    let clip = CellClip::from_buf(buf.area);
    to_buffer_inner(scene, buf, clip, (0, 0));
}

/// (R51.189 §5.45 R55.E.2) Cumulative-state recursive walker.
///
/// `clip` is the current screen-cell clip rect (narrowed by every
/// ancestor [`Scene::Scroll`] viewport intersection); `offset_px` is
/// the cumulative pixel-space translation (composed from each
/// ancestor `Scene::Scroll`'s `viewport.xy - offset.xy` shift, in
/// the same shape as the Vello R51.188 adapter's `child_transform`).
///
/// Mirrors the Vello `to_vello_inner` recursion seam — public API
/// preservation plus a single internal seam that composes the clip
/// and translation at every Scroll arm. Leaf paint primitives
/// ([`paint_box_style`] / [`paint_text_inner`]) apply both at the
/// `Buffer::write` call site so content paints offset into the
/// viewport without mutating the scene tree.
fn to_buffer_inner(scene: &Scene, buf: &mut Buffer, clip: CellClip, offset_px: (i32, i32)) {
    match scene {
        Scene::Container(c) => paint_container(c, buf, clip, offset_px),
        Scene::Text(t) => paint_text_inner(t, buf, clip, offset_px),
        Scene::Box(b) => paint_box(b, buf, clip, offset_px),
        Scene::Scroll(s) => {
            // Viewport in screen pixels = parent's pixel frame +
            // cumulative offset. i64 throughout to absorb the
            // adversarial offset case (R55.E.1 mirror:
            // `with_offset(i32::MAX, ...)`) without intermediate wrap.
            // Distinct `left_*` / `top_*` etc. naming avoids the
            // clippy::similar_names trip the `sx0`/`sy0` shorthand
            // would otherwise hit.
            let left_px = i64::from(s.viewport.x) + i64::from(offset_px.0);
            let top_px = i64::from(s.viewport.y) + i64::from(offset_px.1);
            let right_px = left_px + i64::from(s.viewport.w);
            let bottom_px = top_px + i64::from(s.viewport.h);
            let viewport_clip = CellClip {
                x0: pixels_to_cell_floor(left_px, PIXEL_PER_CELL_X),
                y0: pixels_to_cell_floor(top_px, PIXEL_PER_CELL_Y),
                x1: pixels_to_cell_floor(right_px, PIXEL_PER_CELL_X),
                y1: pixels_to_cell_floor(bottom_px, PIXEL_PER_CELL_Y),
            };
            let new_clip = clip.intersect(viewport_clip);
            if new_clip.is_empty() {
                // Viewport entirely off-screen — skip the recursion
                // (and the shape work it would do) entirely.
                return;
            }
            // Compose the child offset in pixels (same shape as the
            // Vello adapter's `child_transform = parent *
            // translate(viewport.xy - offset.xy)`): content-intrinsic
            // (0, 0) lands at `(viewport.x - scroll.offset_x,
            //  viewport.y - scroll.offset_y)` in the parent frame,
            // then the parent offset shifts it to screen.
            let cx = i64::from(offset_px.0)
                + i64::from(s.viewport.x)
                - i64::from(s.offset_x);
            let cy = i64::from(offset_px.1)
                + i64::from(s.viewport.y)
                - i64::from(s.offset_y);
            let child_offset = (clamp_to_i32(cx), clamp_to_i32(cy));
            to_buffer_inner(&s.content, buf, new_clip, child_offset);
        }
        // R51.115 — `Path` / `Image` still skipped (unicode-art
        // mapping carries on the substrate-incompleteness-signal
        // trigger once a binding actually needs them — every
        // Tier-1 widget paints as text + box only).
        //
        // `Effect` / `External` — §3 capability boundary — escape
        // primitives stay invisible to the cell grid by design (AI
        // introspect reaches them via the symbolic RPC channel).
        //
        // Future variants — `pinion_core::Scene` is
        // `#[non_exhaustive]` (§5.2 hedge) so SemVer minor
        // additions reach the wildcard arm as a no-op until an
        // explicit handler lands.
        _ => {}
    }
}

/// R51.115 §5.41 — paint a [`ContainerNode`]: apply the
/// container's own [`BoxStyle`] (bg fill + border) before recursing
/// into children. The order matches the Vello paint adapter — the
/// container's background fills, then its border draws, then
/// children paint on top — so the visual stack is identical across
/// the two backends.
///
/// R51.189 — `clip` + `offset_px` carry the ancestor scroll
/// cascade state and pass straight through to children + the
/// box-style call.
fn paint_container(c: &ContainerNode, buf: &mut Buffer, clip: CellClip, offset_px: (i32, i32)) {
    paint_box_style(&c.rect, &c.style, buf, clip, offset_px);
    for child in &c.children {
        to_buffer_inner(child, buf, clip, offset_px);
    }
}

/// R51.115 §5.41 — paint a [`BoxNode`] (standalone rect + style).
/// R51.189 — `clip` + `offset_px` pass through to the box-style call.
fn paint_box(b: &BoxNode, buf: &mut Buffer, clip: CellClip, offset_px: (i32, i32)) {
    paint_box_style(&b.rect, &b.style, buf, clip, offset_px);
}

/// R51.115 / R51.189 §5.41 §5.45 — apply a [`BoxStyle`] over
/// `rect`'s cell-space projection: background fill first (cell
/// `bg`), then a single-cell Unicode box-drawing border on the
/// rect's edge cells (`corner_radius` and pixel-`width` are
/// intentionally ignored — TUI cells are discrete, no sub-cell
/// border thickness or rounded corners exist at this resolution;
/// the `placement` axis is also flat because there's no sub-cell
/// offset).
///
/// R51.189 R55.E.2 — `offset_px` shifts the rect into screen
/// pixels before the cell projection, and every cell write is
/// clipped against `clip` ∩ `buf.area`. The pixel→cell conversion
/// uses [`pixels_to_cell_floor`] (signed `i64` path) so a
/// scroll-shifted rect with negative pixel origin still rounds the
/// way `div_euclid` floors.
fn paint_box_style(
    rect: &Rect,
    style: &BoxStyle,
    buf: &mut Buffer,
    clip: CellClip,
    offset_px: (i32, i32),
) {
    let buf_area = buf.area;
    // Screen pixel rect via i64 (overflow-free under adversarial
    // offsets), then floor to signed cell indices. Distinct
    // `left_*` / `top_*` etc. naming dodges the
    // `clippy::similar_names` trip the `sx0`/`sy0` shorthand
    // would otherwise hit at the pedantic baseline.
    let left_px = i64::from(rect.x) + i64::from(offset_px.0);
    let top_px = i64::from(rect.y) + i64::from(offset_px.1);
    let right_px = left_px + i64::from(rect.w);
    let bottom_px = top_px + i64::from(rect.h);
    let cell_left = pixels_to_cell_floor(left_px, PIXEL_PER_CELL_X);
    let cell_top = pixels_to_cell_floor(top_px, PIXEL_PER_CELL_Y);
    let cell_right = pixels_to_cell_floor(right_px, PIXEL_PER_CELL_X);
    let cell_bottom = pixels_to_cell_floor(bottom_px, PIXEL_PER_CELL_Y);
    if cell_left >= cell_right || cell_top >= cell_bottom {
        return;
    }

    // Background fill — iterate the cell range intersected against
    // both `clip` and the buffer bounds. `Color::TRANSPARENT`
    // (default) leaves every cell untouched, matching the Vello
    // adapter's `a == 0` short-circuit.
    if style.fill.a > 0 {
        let bg = color_to_tui(style.fill);
        let fill_left = cell_left.max(clip.x0).max(0);
        let fill_top = cell_top.max(clip.y0).max(0);
        let fill_right = cell_right.min(clip.x1).min(i32::from(buf_area.width));
        let fill_bottom = cell_bottom.min(clip.y1).min(i32::from(buf_area.height));
        if fill_left < fill_right && fill_top < fill_bottom {
            for y in fill_top..fill_bottom {
                for x in fill_left..fill_right {
                    if let Some((bx, by)) = cell_to_buf_xy(x, y, buf_area) {
                        buf[(bx, by)].set_bg(bg);
                    }
                }
            }
        }
    }

    // Border — light single-line box-drawing characters when the
    // rect has room for distinct edges (width ≥ 2 and height ≥ 2;
    // single-cell rects degenerate to a corner glyph which would
    // look like a stray character, so we skip). Each cell write
    // checks `clip` + `buf_area` before writing.
    if let Some(border) = style.border
        && border.color.a > 0
        && cell_right > cell_left + 1
        && cell_bottom > cell_top + 1
    {
        let fg = color_to_tui(border.color);
        let edge_left = cell_left;
        let edge_right = cell_right - 1;
        let edge_top = cell_top;
        let edge_bottom = cell_bottom - 1;
        for x in (edge_left + 1)..edge_right {
            write_border_cell(buf, buf_area, clip, x, edge_top, BOX_HORIZONTAL, fg);
            write_border_cell(buf, buf_area, clip, x, edge_bottom, BOX_HORIZONTAL, fg);
        }
        for y in (edge_top + 1)..edge_bottom {
            write_border_cell(buf, buf_area, clip, edge_left, y, BOX_VERTICAL, fg);
            write_border_cell(buf, buf_area, clip, edge_right, y, BOX_VERTICAL, fg);
        }
        write_border_cell(buf, buf_area, clip, edge_left, edge_top, BOX_TOP_LEFT, fg);
        write_border_cell(buf, buf_area, clip, edge_right, edge_top, BOX_TOP_RIGHT, fg);
        write_border_cell(buf, buf_area, clip, edge_left, edge_bottom, BOX_BOTTOM_LEFT, fg);
        write_border_cell(buf, buf_area, clip, edge_right, edge_bottom, BOX_BOTTOM_RIGHT, fg);
    }
}

/// (R51.189 §5.45 R55.E.2) Per-cell border write helper. Encodes
/// the clip + buffer-bounds checks so the border draw loop in
/// [`paint_box_style`] stays a flat enumeration of edge positions.
fn write_border_cell(
    buf: &mut Buffer,
    buf_area: TuiRect,
    clip: CellClip,
    x: i32,
    y: i32,
    sym: &str,
    fg: TuiColor,
) {
    if !clip.contains_cell(x, y) {
        return;
    }
    if let Some((bx, by)) = cell_to_buf_xy(x, y, buf_area) {
        buf[(bx, by)].set_symbol(sym).set_fg(fg);
    }
}

/// R51.115 §5.41 — convert a pinion sRGB+alpha colour to a ratatui
/// truecolour. The alpha channel is dropped because terminal cells
/// have no alpha — the caller short-circuits on `a == 0` so a
/// transparent colour never reaches this conversion.
fn color_to_tui(c: Color) -> TuiColor {
    TuiColor::Rgb(c.r, c.g, c.b)
}

/// Paint a single `TextNode` into `buf` at cell coordinates derived
/// from `t.rect`. Iterates grapheme clusters; each cluster is
/// written into one cell (narrow) or one + one-skip pair (wide).
/// Truncates at the buffer's right edge silently — R51.111+ adds
/// the ellipsis / soft-wrap policy once the §5.36 text layout cache
/// surface stabilises for the TUI backend.
///
/// R51.189 §5.45 R55.E.2 — public surface unchanged. The body
/// forwards into [`paint_text_inner`] with a full-buffer clip and
/// no offset; the scroll cascade reaches text via the internal
/// recursion path.
pub fn paint_text(t: &TextNode, buf: &mut Buffer) {
    let clip = CellClip::from_buf(buf.area);
    paint_text_inner(t, buf, clip, (0, 0));
}

/// (R51.189 §5.45 R55.E.2) Cumulative-state text painter. The
/// scroll cascade reaches here via [`to_buffer_inner`]'s Text arm;
/// the public [`paint_text`] forwards `(full-clip, no-offset)`.
///
/// Vertical reject: a single-line `TextNode` either falls on a row
/// inside the clip or is entirely skipped. Horizontal walk: each
/// grapheme either fits inside the clip horizontally (write) or
/// straddles a boundary (skip but still advance the column cursor
/// so the rest of the line lays out correctly).
fn paint_text_inner(t: &TextNode, buf: &mut Buffer, clip: CellClip, offset_px: (i32, i32)) {
    let buf_area = buf.area;
    let screen_col_px = i64::from(t.rect.x) + i64::from(offset_px.0);
    let screen_row_px = i64::from(t.rect.y) + i64::from(offset_px.1);
    let cell_col = pixels_to_cell_floor(screen_col_px, PIXEL_PER_CELL_X);
    let cell_row = pixels_to_cell_floor(screen_row_px, PIXEL_PER_CELL_Y);
    // Quick vertical reject — entire line outside the clip strip.
    if cell_row < clip.y0 || cell_row >= clip.y1 {
        return;
    }
    let mut col = cell_col;
    for grapheme in t.content.graphemes(true) {
        let g_width_usize = grapheme.width();
        let g_width = i32::try_from(g_width_usize).unwrap_or(i32::MAX);
        if g_width == 0 {
            // Zero-width joiner / combining mark — the segmenter
            // emits these as separate clusters before joining; the
            // simplest correct behaviour at this layer is to skip
            // (ratatui's `set_symbol` would clobber the cell).
            continue;
        }
        // Past right clip edge — truncate. R51.111+ adds the
        // ellipsis policy.
        if col >= clip.x1 || col.saturating_add(g_width) > clip.x1 {
            break;
        }
        if col < clip.x0 {
            // Before left clip edge — advance silently. Wide
            // graphemes straddling the left boundary skip rather
            // than render a half-cell.
            col = col.saturating_add(g_width);
            continue;
        }
        if let Some((bx, by)) = cell_to_buf_xy(col, cell_row, buf_area) {
            buf[(bx, by)].set_symbol(grapheme);
        }
        col = col.saturating_add(g_width);
    }
}

/// R51.110.0 §5.41 — convert pixel-space (x, y) to cell-space
/// (column, row), saturating at `u16::MAX` so the ratatui buffer's
/// `Rect` (which uses u16 coords) cannot overflow. Used by the
/// paint walker before each cell write.
#[must_use]
pub fn pixel_to_cell_origin(px: u32, py: u32) -> (u16, u16) {
    let cell_x = u16::try_from(px / PIXEL_PER_CELL_X).unwrap_or(u16::MAX);
    let cell_y = u16::try_from(py / PIXEL_PER_CELL_Y).unwrap_or(u16::MAX);
    (cell_x, cell_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
    use ratatui::layout::Rect as TuiRect;

    /// Construct a minimal `TextNode` for tests at pixel `(x, y)`
    /// with the given content. Width / height fields are set to the
    /// content's grapheme count × cell metrics; the paint walker
    /// only consults `rect.x` / `rect.y` for placement.
    fn text_at(x: u32, y: u32, content: &str) -> TextNode {
        let mut node = TextNode::default();
        node.content = content.to_owned();
        node.rect = Rect::new(x, y, 100, 16);
        node
    }

    #[test]
    fn ascii_text_paints_at_cell_coords() {
        // (0, 0) pixel → (0, 0) cell.
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        let scene = Scene::Text(text_at(0, 0, "hello"));
        to_buffer(&scene, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "h");
        assert_eq!(buf[(1, 0)].symbol(), "e");
        assert_eq!(buf[(2, 0)].symbol(), "l");
        assert_eq!(buf[(3, 0)].symbol(), "l");
        assert_eq!(buf[(4, 0)].symbol(), "o");
    }

    #[test]
    fn pixel_origin_scales_by_cell_size() {
        // Pixel (16, 32) at 8×16 cell metrics = cell (2, 2).
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        let scene = Scene::Text(text_at(16, 32, "X"));
        to_buffer(&scene, &mut buf);
        assert_eq!(buf[(2, 2)].symbol(), "X");
    }

    #[test]
    fn cjk_grapheme_consumes_two_cells() {
        // CJK wide graphemes (e.g. '한') occupy 2 cells. The walker
        // advances the column cursor by 2 so the next grapheme lands
        // at the correct position.
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        let scene = Scene::Text(text_at(0, 0, "한X"));
        to_buffer(&scene, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "한");
        // (1, 0) is the "wide grapheme spillover" cell — ratatui
        // typically leaves this as the source cell's continuation;
        // we don't assert its exact symbol since ratatui's
        // rendering convention covers it.
        assert_eq!(buf[(2, 0)].symbol(), "X");
    }

    #[test]
    fn out_of_bounds_text_skips_cleanly() {
        // Pixel (1000, 0) → cell (125, 0); buffer is 40 wide. The
        // walker must skip without panicking.
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        let scene = Scene::Text(text_at(1000, 0, "off-screen"));
        to_buffer(&scene, &mut buf);
        // Buffer left untouched at (0, 0).
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn right_edge_truncation_stops_painting() {
        // Buffer 5 wide; text "hello world" should truncate at "hello".
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 5, 1));
        let scene = Scene::Text(text_at(0, 0, "hello world"));
        to_buffer(&scene, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "h");
        assert_eq!(buf[(4, 0)].symbol(), "o");
        // Cell (5, 0) is out of the buffer's area — accessing it
        // would panic, so we just confirm the buffer width matches.
        assert_eq!(buf.area.width, 5);
    }

    #[test]
    fn container_recurses_into_children() {
        // Container with one Text child paints the child.
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        let mut container = ContainerNode::default();
        container.rect = Rect::new(0, 0, 320, 160);
        container.children.push(Scene::Text(text_at(8, 16, "hi")));
        let scene = Scene::Container(container);
        to_buffer(&scene, &mut buf);
        // Pixel (8, 16) → cell (1, 1).
        assert_eq!(buf[(1, 1)].symbol(), "h");
        assert_eq!(buf[(2, 1)].symbol(), "i");
    }

    #[test]
    fn empty_content_is_no_op() {
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        let scene = Scene::Text(text_at(0, 0, ""));
        to_buffer(&scene, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn default_box_node_is_transparent() {
        // R51.115 — a default `BoxNode` has `fill = TRANSPARENT`
        // (alpha 0) and no border. The walker must leave the buffer
        // untouched; this preserves the R51.110.0 "first cut"
        // contract where empty boxes are invisible.
        use pinion_core::scene::BoxNode;
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        let scene = Scene::Box(BoxNode::default());
        to_buffer(&scene, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn box_with_opaque_fill_sets_cell_background() {
        // R51.115 — opaque fill colour paints into the cell `bg`
        // attribute. Verifies the fill covers every cell inside the
        // rect, alpha drops cleanly.
        use pinion_core::scene::BoxNode;
        use pinion_core::style::{BoxStyle, Color};
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 10, 5));
        let mut box_node = BoxNode::default();
        // Pixel rect (0, 0, 16, 16) = cell rect (0, 0, 2, 1).
        box_node.rect = Rect::new(0, 0, 16, 16);
        box_node.style = BoxStyle::filled(Color::rgb(0x20, 0x30, 0x40));
        let scene = Scene::Box(box_node);
        to_buffer(&scene, &mut buf);
        let expected = ratatui::style::Color::Rgb(0x20, 0x30, 0x40);
        assert_eq!(buf[(0, 0)].bg, expected);
        assert_eq!(buf[(1, 0)].bg, expected);
        // Cells outside the rect untouched.
        assert_eq!(buf[(2, 0)].bg, ratatui::style::Color::Reset);
    }

    #[test]
    fn box_with_border_draws_unicode_corners_and_edges() {
        // R51.115 — `BoxStyle::with_border` produces a single-line
        // border using the U+250C..U+2518 light box-drawing set.
        use pinion_core::scene::BoxNode;
        use pinion_core::style::{Border, BoxStyle, Color};
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 10, 5));
        let mut box_node = BoxNode::default();
        // Pixel rect (0, 0, 32, 48) = cell rect (0, 0, 4, 3).
        // Corners at (0, 0) ┌, (3, 0) ┐, (0, 2) └, (3, 2) ┘.
        box_node.rect = Rect::new(0, 0, 32, 48);
        box_node.style =
            BoxStyle::default().with_border(Border::new(Color::rgb(0xff, 0xff, 0xff), 1));
        let scene = Scene::Box(box_node);
        to_buffer(&scene, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "┌");
        assert_eq!(buf[(3, 0)].symbol(), "┐");
        assert_eq!(buf[(0, 2)].symbol(), "└");
        assert_eq!(buf[(3, 2)].symbol(), "┘");
        // Top + bottom edges between corners.
        assert_eq!(buf[(1, 0)].symbol(), "─");
        assert_eq!(buf[(2, 0)].symbol(), "─");
        assert_eq!(buf[(1, 2)].symbol(), "─");
        // Left + right edges between corners.
        assert_eq!(buf[(0, 1)].symbol(), "│");
        assert_eq!(buf[(3, 1)].symbol(), "│");
    }

    #[test]
    fn box_single_cell_rect_skips_border() {
        // R51.115 — a 1×1 cell rect cannot host distinct corners +
        // edges; the walker must skip rather than emit a stray glyph.
        use pinion_core::scene::BoxNode;
        use pinion_core::style::{Border, BoxStyle, Color};
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 10, 5));
        let mut box_node = BoxNode::default();
        box_node.rect = Rect::new(0, 0, 8, 16);
        box_node.style =
            BoxStyle::default().with_border(Border::new(Color::rgb(0xff, 0xff, 0xff), 1));
        let scene = Scene::Box(box_node);
        to_buffer(&scene, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn container_paints_its_own_style_before_children() {
        // R51.115 — containers carry their own `BoxStyle`; the
        // walker paints the container's fill / border, then
        // recurses into children so text overlays the fill.
        use pinion_core::scene::ContainerNode;
        use pinion_core::style::{BoxStyle, Color};
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 10, 5));
        let mut child = TextNode::default();
        child.content = "Hi".to_owned();
        child.rect = Rect::new(0, 0, 16, 16);
        let mut container = ContainerNode::default();
        container.rect = Rect::new(0, 0, 32, 32);
        container.style = BoxStyle::filled(Color::rgb(0x10, 0x20, 0x30));
        container.children.push(Scene::Text(child));
        let scene = Scene::Container(container);
        to_buffer(&scene, &mut buf);
        let expected_bg = ratatui::style::Color::Rgb(0x10, 0x20, 0x30);
        // Cell (0, 0) carries the text 'H' AND the container fill.
        assert_eq!(buf[(0, 0)].symbol(), "H");
        assert_eq!(buf[(0, 0)].bg, expected_bg);
        assert_eq!(buf[(1, 0)].symbol(), "i");
        assert_eq!(buf[(1, 0)].bg, expected_bg);
    }

    #[test]
    fn pixel_to_cell_origin_saturates_at_u16_max() {
        // Extreme pixel values saturate cleanly.
        let (cx, cy) = pixel_to_cell_origin(u32::MAX, u32::MAX);
        assert_eq!(cx, u16::MAX);
        assert_eq!(cy, u16::MAX);
    }

    // ----- R51.189 §5.45 R55.E.2 TUI paint clipping tests -----

    #[test]
    fn r55_e2_scroll_arm_paints_content_inside_viewport() {
        // The scroll's content is a filled BoxNode whose pixel rect
        // exactly matches the viewport (no scroll offset). After
        // walking the scroll, every cell inside the viewport carries
        // the box's fill — cells outside the viewport (but inside
        // the buffer) remain untouched.
        use pinion_core::scene::{BoxNode, ScrollNode};
        use pinion_core::style::Color;
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 10, 5));
        // Viewport at (0, 0, 32, 32) = cell rect (0, 0, 4, 2).
        let content = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 32, 32),
            Color::rgb(0x10, 0x20, 0x30),
        ));
        let scroll = ScrollNode::new(Rect::new(0, 0, 32, 32), content);
        let scene = Scene::Scroll(scroll);
        to_buffer(&scene, &mut buf);
        let expected = ratatui::style::Color::Rgb(0x10, 0x20, 0x30);
        // Inside the viewport — fill applied.
        assert_eq!(buf[(0, 0)].bg, expected);
        assert_eq!(buf[(3, 1)].bg, expected);
        // Outside the viewport (but inside the buffer) — untouched.
        assert_eq!(buf[(4, 0)].bg, ratatui::style::Color::Reset);
        assert_eq!(buf[(0, 2)].bg, ratatui::style::Color::Reset);
    }

    #[test]
    fn r55_e2_scroll_clips_overshooting_content() {
        // Content rect exceeds the viewport. The cells outside the
        // viewport stay untouched even though the content's pixel
        // rect would naively cover them. Verifies the clip narrows
        // the box-fill iteration.
        use pinion_core::scene::{BoxNode, ScrollNode};
        use pinion_core::style::Color;
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 20, 10));
        // Viewport (0, 0, 32, 32) = cell rect (0, 0, 4, 2).
        // Content (0, 0, 200, 200) = cell rect (0, 0, 25, 12)
        // (truncated by buffer to 20 × 10 without clip).
        let content = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 200, 200),
            Color::rgb(0x42, 0x42, 0x42),
        ));
        let scroll = ScrollNode::new(Rect::new(0, 0, 32, 32), content);
        let scene = Scene::Scroll(scroll);
        to_buffer(&scene, &mut buf);
        let expected = ratatui::style::Color::Rgb(0x42, 0x42, 0x42);
        // Inside viewport — filled.
        assert_eq!(buf[(0, 0)].bg, expected);
        assert_eq!(buf[(3, 1)].bg, expected);
        // Outside viewport but inside buffer — must be untouched.
        // Without the R51.189 clip, the box fill would reach here.
        assert_eq!(buf[(4, 0)].bg, ratatui::style::Color::Reset);
        assert_eq!(buf[(10, 5)].bg, ratatui::style::Color::Reset);
        assert_eq!(buf[(0, 2)].bg, ratatui::style::Color::Reset);
    }

    #[test]
    fn r55_e2_scroll_offset_shifts_content_into_viewport() {
        // Content is a TextNode at pixel (0, 0). With a scroll
        // offset of (0, 16) (one cell row down), the text shifts up
        // by one cell row — i.e. ends up at cell (0, -1) in
        // viewport-local space, which is outside the viewport and
        // therefore invisible.
        //
        // To verify the shift wires correctly: put two text rows in
        // a Container, offset the scroll by (0, 16) so the first
        // row scrolls off-screen and the second row lands at the
        // viewport top.
        use pinion_core::scene::{ContainerNode, ScrollNode};
        let mut row0 = TextNode::default();
        row0.content = "row0".to_owned();
        row0.rect = Rect::new(0, 0, 100, 16);
        let mut row1 = TextNode::default();
        row1.content = "row1".to_owned();
        row1.rect = Rect::new(0, 16, 100, 16);
        let mut content = ContainerNode::default();
        content.rect = Rect::new(0, 0, 100, 64);
        content.children.push(Scene::Text(row0));
        content.children.push(Scene::Text(row1));
        let scroll = ScrollNode::new(Rect::new(0, 0, 80, 32), Scene::Container(content))
            .with_offset(0, 16);
        let scene = Scene::Scroll(scroll);
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 20, 5));
        to_buffer(&scene, &mut buf);
        // Cell (0, 0) carries 'r' from row1 (row0 scrolled past the
        // viewport top, row1 shifted to cell row 0).
        assert_eq!(buf[(0, 0)].symbol(), "r");
        assert_eq!(buf[(1, 0)].symbol(), "o");
        assert_eq!(buf[(2, 0)].symbol(), "w");
        assert_eq!(buf[(3, 0)].symbol(), "1");
    }

    #[test]
    fn r55_e2_nested_scroll_clips_compose() {
        // Outer scroll clips a 4×2 viewport; inner scroll inside
        // requests a 10×10 viewport at cell (0, 0). The inner clip
        // gets narrowed to the outer's 4×2 intersection. A filled
        // box inside the inner scroll only writes inside that
        // intersection.
        use pinion_core::scene::{BoxNode, ScrollNode};
        use pinion_core::style::Color;
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 20, 10));
        let inner_box = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 200, 200),
            Color::rgb(0x99, 0x99, 0x99),
        ));
        // Inner viewport (0, 0, 160, 160) = cell rect (0, 0, 20, 10).
        let inner_scroll = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 160, 160),
            inner_box,
        ));
        // Outer viewport (0, 0, 32, 32) = cell rect (0, 0, 4, 2).
        let outer_scroll = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 32, 32),
            inner_scroll,
        ));
        to_buffer(&outer_scroll, &mut buf);
        let expected = ratatui::style::Color::Rgb(0x99, 0x99, 0x99);
        // Inside outer viewport — must be filled (inner permits it).
        assert_eq!(buf[(0, 0)].bg, expected);
        assert_eq!(buf[(3, 1)].bg, expected);
        // Past the outer viewport (would be inside the inner alone)
        // — must remain untouched. This is the composition check.
        assert_eq!(buf[(4, 0)].bg, ratatui::style::Color::Reset);
        assert_eq!(buf[(0, 2)].bg, ratatui::style::Color::Reset);
        assert_eq!(buf[(10, 5)].bg, ratatui::style::Color::Reset);
    }

    #[test]
    fn r55_e2_scroll_arm_survives_offset_overshoot() {
        // R55.E.1 mirror — adversarial offset (`i32::MAX`) shifts
        // the content's screen-pixel origin to the saturating edge.
        // The walker must complete without panic; `i64` arithmetic
        // + `clamp_to_i32` + `pixels_to_cell_floor` saturation +
        // `cell_to_buf_xy` `try_from` guard are the safety net.
        use pinion_core::scene::{BoxNode, ScrollNode};
        use pinion_core::style::Color;
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 10, 5));
        let content = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 50, 50),
            Color::rgb(0, 0xff, 0),
        ));
        let scroll = ScrollNode::new(Rect::new(0, 0, 32, 32), content)
            .with_offset(i32::MAX, i32::MAX);
        let scene = Scene::Scroll(scroll);
        to_buffer(&scene, &mut buf);
        // Content scrolled past the viewport entirely — every cell
        // should be untouched, but the key invariant is no panic.
        assert_eq!(buf[(0, 0)].bg, ratatui::style::Color::Reset);
    }

    #[test]
    fn r55_e2_scroll_empty_viewport_skips_recursion() {
        // A scroll whose viewport falls entirely outside the buffer
        // must skip the recursion. We can't directly observe the
        // skip from outside; the proof is no panic + no cells
        // changed.
        use pinion_core::scene::{BoxNode, ScrollNode};
        use pinion_core::style::Color;
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 10, 5));
        let content = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 32, 32),
            Color::rgb(0xff, 0, 0),
        ));
        // Viewport at pixel (1000, 1000) = cell (125, 62), well
        // outside the 10×5 cell buffer.
        let scroll = ScrollNode::new(Rect::new(1000, 1000, 32, 32), content);
        let scene = Scene::Scroll(scroll);
        to_buffer(&scene, &mut buf);
        assert_eq!(buf[(0, 0)].bg, ratatui::style::Color::Reset);
        assert_eq!(buf[(9, 4)].bg, ratatui::style::Color::Reset);
    }
}
