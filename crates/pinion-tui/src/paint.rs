//! R51.110.0 / R51.115 §5.41 — Scene → `ratatui::Buffer` paint
//! walker.
//!
//! R51.110.0 first cut painted `TextNode.content` only. R51.115
//! extends the arm coverage to `BoxNode` and `ContainerNode`'s
//! `BoxStyle`: background fill (cell `bg` colour) and border
//! (Unicode box-drawing characters). `PathNode` / `ImageNode` stay
//! carry-forward — the unicode-art mapping waits for a binding
//! that needs them (Tier-1 widgets are all text + box).
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

/// Walk `scene` and write its painted state into `buf`. Recurses
/// through [`Scene::Container`] children, paints [`Scene::Text`]
/// nodes via [`paint_text`], skips other primitives (R51.111+
/// extends the arm coverage). The walker is idempotent on `buf` —
/// repeated calls with the same scene produce the same cell state;
/// callers reset `buf` between frames if they want a clean redraw.
pub fn to_buffer(scene: &Scene, buf: &mut Buffer) {
    match scene {
        Scene::Container(c) => paint_container(c, buf),
        Scene::Text(t) => paint_text(t, buf),
        Scene::Box(b) => paint_box(b, buf),
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
fn paint_container(c: &ContainerNode, buf: &mut Buffer) {
    paint_box_style(&c.rect, &c.style, buf);
    for child in &c.children {
        to_buffer(child, buf);
    }
}

/// R51.115 §5.41 — paint a [`BoxNode`] (standalone rect + style).
fn paint_box(b: &BoxNode, buf: &mut Buffer) {
    paint_box_style(&b.rect, &b.style, buf);
}

/// R51.115 §5.41 — apply a [`BoxStyle`] over `rect`'s cell-space
/// projection: background fill first (cell `bg`), then a
/// single-cell Unicode box-drawing border on the rect's edge cells
/// (`corner_radius` and pixel-`width` are intentionally ignored —
/// TUI cells are discrete, no sub-cell border thickness or rounded
/// corners exist at this resolution; the `placement` axis is also
/// flat because there's no sub-cell offset).
fn paint_box_style(rect: &Rect, style: &BoxStyle, buf: &mut Buffer) {
    let (x0, y0) = pixel_to_cell_origin(rect.x, rect.y);
    let (x1, y1) = pixel_to_cell_origin(rect.x.saturating_add(rect.w), rect.y.saturating_add(rect.h));
    let buf_area = buf.area;
    let x0 = x0.min(buf_area.width);
    let x1 = x1.min(buf_area.width);
    let y0 = y0.min(buf_area.height);
    let y1 = y1.min(buf_area.height);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    // Background fill — only when the colour is not fully
    // transparent. `Color::TRANSPARENT` (default) leaves every cell
    // untouched, matching the Vello adapter's `a == 0` short-circuit.
    if style.fill.a > 0 {
        let bg = color_to_tui(style.fill);
        for y in y0..y1 {
            for x in x0..x1 {
                buf[(buf_area.x + x, buf_area.y + y)].set_bg(bg);
            }
        }
    }

    // Border — light single-line box-drawing characters when the
    // rect has room for distinct edges (width ≥ 2 and height ≥ 2;
    // single-cell rects degenerate to a corner glyph which would
    // look like a stray character, so we skip).
    if let Some(border) = style.border
        && border.color.a > 0
        && x1 > x0 + 1
        && y1 > y0 + 1
    {
        let fg = color_to_tui(border.color);
        let left = x0;
        let right = x1 - 1;
        let top = y0;
        let bottom = y1 - 1;
        // Horizontal edges (excluding corners).
        for x in (left + 1)..right {
            buf[(buf_area.x + x, buf_area.y + top)]
                .set_symbol(BOX_HORIZONTAL)
                .set_fg(fg);
            buf[(buf_area.x + x, buf_area.y + bottom)]
                .set_symbol(BOX_HORIZONTAL)
                .set_fg(fg);
        }
        // Vertical edges (excluding corners).
        for y in (top + 1)..bottom {
            buf[(buf_area.x + left, buf_area.y + y)]
                .set_symbol(BOX_VERTICAL)
                .set_fg(fg);
            buf[(buf_area.x + right, buf_area.y + y)]
                .set_symbol(BOX_VERTICAL)
                .set_fg(fg);
        }
        // Corners.
        buf[(buf_area.x + left, buf_area.y + top)]
            .set_symbol(BOX_TOP_LEFT)
            .set_fg(fg);
        buf[(buf_area.x + right, buf_area.y + top)]
            .set_symbol(BOX_TOP_RIGHT)
            .set_fg(fg);
        buf[(buf_area.x + left, buf_area.y + bottom)]
            .set_symbol(BOX_BOTTOM_LEFT)
            .set_fg(fg);
        buf[(buf_area.x + right, buf_area.y + bottom)]
            .set_symbol(BOX_BOTTOM_RIGHT)
            .set_fg(fg);
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
pub fn paint_text(t: &TextNode, buf: &mut Buffer) {
    let (cell_x, cell_y) = pixel_to_cell_origin(t.rect.x, t.rect.y);
    let buf_area = buf.area;
    if cell_x >= buf_area.width || cell_y >= buf_area.height {
        return;
    }
    let mut col = cell_x;
    for grapheme in t.content.graphemes(true) {
        let g_width = u16::try_from(grapheme.width()).unwrap_or(u16::MAX);
        if g_width == 0 {
            // Zero-width joiner / combining mark — the segmenter
            // emits these as separate clusters before joining; the
            // simplest correct behaviour at this layer is to skip
            // (ratatui's `set_symbol` would clobber the cell).
            continue;
        }
        if col >= buf_area.width || col + g_width > buf_area.width {
            // Right-edge truncation. R51.111+ adds ellipsis policy.
            break;
        }
        let abs_x = buf_area.x + col;
        let abs_y = buf_area.y + cell_y;
        buf[(abs_x, abs_y)].set_symbol(grapheme);
        col += g_width;
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
}
