//! R51.110.0 §5.41 — Scene → `ratatui::Buffer` paint walker.
//!
//! Text-first cut: walks the pinion-core `Scene` tree and writes
//! `TextNode.content` into the target `ratatui::buffer::Buffer` at
//! cell coordinates derived from the node's pixel rect. Other
//! primitive variants (`BoxNode` background, borders, `PathNode`,
//! `ImageNode`) are intentionally skipped this round; the
//! incremental `Box` border + bg fill arms land R51.111 alongside
//! the hello-button TUI dogfood evaluation.
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

use pinion_core::scene::{Scene, TextNode};
use ratatui::buffer::Buffer;
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

/// Walk `scene` and write its painted state into `buf`. Recurses
/// through [`Scene::Container`] children, paints [`Scene::Text`]
/// nodes via [`paint_text`], skips other primitives (R51.111+
/// extends the arm coverage). The walker is idempotent on `buf` —
/// repeated calls with the same scene produce the same cell state;
/// callers reset `buf` between frames if they want a clean redraw.
pub fn to_buffer(scene: &Scene, buf: &mut Buffer) {
    match scene {
        Scene::Container(c) => {
            for child in &c.children {
                to_buffer(child, buf);
            }
        }
        Scene::Text(t) => paint_text(t, buf),
        // R51.110.0 first cut — every other Scene variant maps to a
        // no-op:
        //
        // - `Box` / `Path` / `Image`: paint coverage lands R51.111+
        //   alongside the hello-button TUI dogfood (Box border + bg
        //   first, then Path/Image unicode-art).
        // - `Effect` / `External`: §3 capability boundary — escape
        //   primitives stay invisible to the cell grid by design (AI
        //   introspect reaches them via the symbolic RPC channel).
        // - Future variants: `pinion_core::Scene` is
        //   `#[non_exhaustive]` (§5.2 hedge) so SemVer minor
        //   additions reach the wildcard arm as a no-op until an
        //   explicit handler lands.
        _ => {}
    }
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
    fn box_node_is_skipped_first_cut() {
        // R51.110.0 first cut: Box mapping not yet land. Verify the
        // walker skips without panic — R51.111+ adds the border / bg
        // arms once hello-button TUI dogfood drives the design.
        use pinion_core::scene::BoxNode;
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        let scene = Scene::Box(BoxNode::default());
        to_buffer(&scene, &mut buf);
        // Buffer untouched.
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn pixel_to_cell_origin_saturates_at_u16_max() {
        // Extreme pixel values saturate cleanly.
        let (cx, cy) = pixel_to_cell_origin(u32::MAX, u32::MAX);
        assert_eq!(cx, u16::MAX);
        assert_eq!(cy, u16::MAX);
    }
}
