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
//! goes through [`CellMetric`] (the R968 §5.41 cell-native metric).
//! This adapter renders against [`CellMetric::DEFAULT`] — the 8×16
//! bitmap font baseline — so behaviour is byte-unchanged from the
//! pre-R968 `PIXEL_PER_CELL_*` constants it replaced. A
//! [`Scene::TextGrid`] (R994) maps each of
//! its cells 1:1 onto a character cell — its own node-local pixel metric
//! sizes Vello glyphs, but a character buffer has no sub-cell resolution, so
//! only the grid's `rect` origin is mapped (through `CELL`).
//!
//! The local `pixels_to_cell_floor` is the signed/i64 pixel→cell map
//! the scroll cascade needs — content scrolled past the viewport's left
//! or top edge lands at negative pixels, so the cell index needs
//! `div_euclid` flooring toward `-∞` rather than a truncating `/`. That
//! signed variant stays in this adapter (the Vello backend clips in
//! pixels and never needs it).
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

use pinion_core::CellMetric;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene, TextGridNode, TextNode};
use pinion_core::style::{BoxStyle, Color, FontStyle, FontWeight, TextStyle};
use pinion_core::term_grid::{CellAttrs, CellWidth, TermColor};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect as TuiRect;
use ratatui::style::{Color as TuiColor, Modifier, Style};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// R968 §5.41 — the cell-native metric this TUI paint adapter renders
/// against. Sourced from [`CellMetric::DEFAULT`] (the behaviour-preserving
/// 8×16 baseline the pre-R968 `PIXEL_PER_CELL_*` constants carried). It
/// positions every node — including a `Scene::TextGrid`, whose own node-local
/// metric sizes Vello glyphs but is irrelevant to a character buffer (R994
/// maps grid cells 1:1) — with the shared `Rect` geometry staying in logical
/// pixels per the R968 ratify.
const CELL: CellMetric = CellMetric::DEFAULT;

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
/// forwards into `to_buffer_inner` with a full-buffer clip and
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
                x0: pixels_to_cell_floor(left_px, CELL.cell_w()),
                y0: pixels_to_cell_floor(top_px, CELL.cell_h()),
                x1: pixels_to_cell_floor(right_px, CELL.cell_w()),
                y1: pixels_to_cell_floor(bottom_px, CELL.cell_h()),
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
            let cx = i64::from(offset_px.0) + i64::from(s.viewport.x) - i64::from(s.offset_x);
            let cy = i64::from(offset_px.1) + i64::from(s.viewport.y) - i64::from(s.offset_y);
            let child_offset = (clamp_to_i32(cx), clamp_to_i32(cy));
            to_buffer_inner(&s.content, buf, new_clip, child_offset);
        }
        // R994 §5.41 §2 #6 — the cell-native grid's TUI sibling of the
        // Vello glyph paint (R991-R993): each grid cell maps 1:1 onto a
        // ratatui character cell.
        Scene::TextGrid(n) => paint_text_grid_inner(n, buf, clip, offset_px),
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
    let cell_left = pixels_to_cell_floor(left_px, CELL.cell_w());
    let cell_top = pixels_to_cell_floor(top_px, CELL.cell_h());
    let cell_right = pixels_to_cell_floor(right_px, CELL.cell_w());
    let cell_bottom = pixels_to_cell_floor(bottom_px, CELL.cell_h());
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
        write_border_cell(
            buf,
            buf_area,
            clip,
            edge_left,
            edge_bottom,
            BOX_BOTTOM_LEFT,
            fg,
        );
        write_border_cell(
            buf,
            buf_area,
            clip,
            edge_right,
            edge_bottom,
            BOX_BOTTOM_RIGHT,
            fg,
        );
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
/// from `t.rect`. Iterates grapheme clusters; a narrow cluster is
/// written into one cell, a wide (CJK / fullwidth) cluster into its
/// head cell while the column cursor advances by the cluster's
/// display width. The wide cluster's continuation cell is left as
/// `Buffer::empty`'s blank `" "` — matching ratatui's own
/// `Buffer::set_stringn`, which resets continuation cells via
/// `Cell::reset()` to `" "` (a space, NOT an empty string). That
/// blank is never drawn: `Buffer::diff` skips it via the head cell's
/// width, so the wide glyph occupies both columns cleanly (R1336
/// locks this against the misdiagnosed CJK-continuation GAP report).
/// Truncates at the buffer's right edge silently — R51.111+ adds
/// the ellipsis / soft-wrap policy once the §5.36 text layout cache
/// surface stabilises for the TUI backend.
///
/// R51.189 §5.45 R55.E.2 — public surface unchanged. The body
/// forwards into `paint_text_inner` with a full-buffer clip and
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
    let cell_col = pixels_to_cell_floor(screen_col_px, CELL.cell_w());
    let cell_row = pixels_to_cell_floor(screen_row_px, CELL.cell_h());
    // Quick vertical reject — entire line outside the clip strip.
    if cell_row < clip.y0 || cell_row >= clip.y1 {
        return;
    }
    let mut col = cell_col;
    // R1337 §5.41 §2#6 — `grapheme_indices` (not `graphemes`) so each
    // cluster's UTF-8 byte offset resolves which `TextNode.runs` span
    // (rich text) governs it; empty `runs` falls back to `t.style`.
    for (byte_off, grapheme) in t.content.grapheme_indices(true) {
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
            let style = effective_text_style(t, byte_off);
            let cell = &mut buf[(bx, by)];
            cell.set_symbol(grapheme);
            apply_text_style(cell, style);
        }
        col = col.saturating_add(g_width);
    }
}

/// R1337 §5.36 §5.41 — the effective [`TextStyle`] for the grapheme
/// starting at UTF-8 `byte_off`: the `StyleRun` whose `[start, end)`
/// covers that byte, else the node's base `style`. Keying on the
/// cluster's *start* byte assigns a grapheme wholly to one run.
///
/// R1339 — matches the `StyleRun` **last-push-wins** contract: runs
/// apply in list order over the base style and, where they overlap, a
/// later run wins for the shared bytes (parley's range resolution the
/// Vello backend drives through `layout_with_runs`). Hence `rev().find`
/// — the *last* run in list order that covers the byte; for the
/// well-formed non-overlapping case that is the sole covering run, so
/// this is identical to a forward scan there.
///
/// Cost is `O(runs)` per grapheme. The empty-`runs` fast path (every
/// single-style node) is `O(1)`, and `runs` are not guaranteed sorted
/// by `start` (the contract fixes *list* order, not byte order), so an
/// `O(1)`-amortised advancing cursor is unsound without a sort/flatten
/// pass — deferred until a many-run TUI text node forces it.
fn effective_text_style(t: &TextNode, byte_off: usize) -> &TextStyle {
    let b = u32::try_from(byte_off).unwrap_or(u32::MAX);
    t.runs
        .iter()
        .rev()
        .find(|run| run.start <= b && b < run.end)
        .map_or(&t.style, |run| &run.style)
}

/// R1337 §2#6 — apply a [`TextStyle`]'s terminal-representable
/// attributes to a cell so the same `TextNode` style fields that the
/// Vello backend renders (`paint_adapter::paint_text`) also drive the
/// TUI, closing the "one scene, two backends" style-drop where only
/// the symbol was written pre-R1337. The mapping is the terminal's
/// representable subset, not a pixel-equal reproduction (see below).
///
/// Foreground: the framework-default `fg_color` is opaque black
/// (`TextStyle::new`), but a terminal's default foreground is
/// theme-driven (light-on-dark or dark-on-light). Forcing literal
/// black would be invisible on a dark terminal, so **any black**
/// (the default, at any alpha) — and any fully transparent colour —
/// inherits the terminal's own foreground (leaving the cell's
/// `Reset` fg), exactly as the `TermColor::Default` grid path does.
/// Any *explicit* non-black, visible colour is honoured verbatim.
/// This is a deliberate divergence from Vello (which paints literal
/// black glyphs): default text follows the terminal theme instead.
///
/// Weight / style / decoration collapse onto the terminal's single
/// bold / italic / underline / strikethrough intensities: CSS
/// weight `>= 700` (`FontWeight::BOLD`) is bold (500 / 600 have no
/// terminal intensity and stay normal); italic and oblique both map
/// to the one terminal italic. All are additive, so default-styled
/// text sets no modifier and stays byte-identical to the pre-R1337
/// (and ratatui-native) unstyled output.
fn apply_text_style(cell: &mut ratatui::buffer::Cell, style: &TextStyle) {
    let fg = style.fg_color;
    // Apply only a visible, non-black colour. Black (at any alpha) is
    // the framework default and inherits the terminal's theme fg; the
    // RGB-triplet test (not full-colour equality) keeps opaque and
    // semi-transparent black consistent.
    if fg.a > 0 && (fg.r, fg.g, fg.b) != (0, 0, 0) {
        cell.set_fg(color_to_tui(fg));
    }
    let mut modifier = Modifier::empty();
    if style.font_weight.0 >= FontWeight::BOLD.0 {
        modifier |= Modifier::BOLD;
    }
    if matches!(style.font_style, FontStyle::Italic | FontStyle::Oblique(_)) {
        modifier |= Modifier::ITALIC;
    }
    if style.decoration.underline {
        modifier |= Modifier::UNDERLINED;
    }
    if style.decoration.strikethrough {
        modifier |= Modifier::CROSSED_OUT;
    }
    if !modifier.is_empty() {
        cell.set_style(Style::default().add_modifier(modifier));
    }
}

/// R994 §5.41 — map a terminal [`TermColor`] to a ratatui [`TuiColor`]. The
/// TUI delegates `Default` and `Indexed` to the **host terminal's** own
/// palette (the natural terminal behaviour — the user's theme applies),
/// unlike the Vello backend which has no terminal and resolves every colour
/// through pinion's [`Palette`](pinion_core::term_grid::Palette). Truecolor
/// passes through verbatim. This is a deliberate, documented backend
/// divergence: both render the same SGR colour *model*, each at the right
/// altitude.
fn term_color_to_tui(c: TermColor) -> TuiColor {
    match c {
        TermColor::Default => TuiColor::Reset,
        TermColor::Indexed(i) => TuiColor::Indexed(i),
        // Truecolor reuses the `Color` -> `TuiColor::Rgb` SSOT.
        TermColor::Rgb(rgb) => color_to_tui(rgb),
    }
}

/// R994 §5.41 — map the SGR [`CellAttrs`] flags onto ratatui [`Modifier`]
/// bits. The host terminal applies reverse / dim / blink itself, so every
/// flag (including `blink`, which the Vello backend defers as a timing
/// concern) maps straight through — the TUI sibling is simpler than the
/// Vello paint precisely because the terminal does the work. This reads the
/// same [`CellAttrs`] the Vello path does, but the *target* diverges
/// (`Modifier` bits vs `FontWeight` / manual swap), so only the bool reads
/// are shared — no abstraction to lift.
fn cell_attrs_to_modifier(attrs: CellAttrs) -> Modifier {
    let mut m = Modifier::empty();
    if attrs.bold {
        m |= Modifier::BOLD;
    }
    if attrs.dim {
        m |= Modifier::DIM;
    }
    if attrs.italic {
        m |= Modifier::ITALIC;
    }
    if attrs.underline {
        m |= Modifier::UNDERLINED;
    }
    if attrs.blink {
        m |= Modifier::SLOW_BLINK;
    }
    if attrs.reverse {
        m |= Modifier::REVERSED;
    }
    if attrs.hidden {
        m |= Modifier::HIDDEN;
    }
    if attrs.strikethrough {
        m |= Modifier::CROSSED_OUT;
    }
    m
}

/// R994 §5.41 §2 #6 — paint one retained [`Scene::TextGrid`] into the
/// ratatui [`Buffer`]: the cell-native projection's TUI sibling of the Vello
/// glyph paint (R991-R993), completing the §2 #6 GUI / TUI dual for the grid.
///
/// Each grid cell maps **1:1** onto one ratatui character cell — a terminal
/// projection is already a character grid, so the node's GUI pixel
/// [`CellMetric`] is irrelevant here (it sizes Vello glyphs). The grid's
/// `rect` only positions its origin, mapped through the buffer's [`CELL`]
/// metric exactly like [`paint_text_inner`]; the scroll cascade's `clip` /
/// `offset_px` carry through unchanged.
///
/// Per cell: the grapheme `cluster` is the symbol, `fg` / `bg` map through
/// [`term_color_to_tui`] (the host terminal resolves indexed / default), and
/// the SGR attrs become [`Modifier`] bits via [`cell_attrs_to_modifier`]
/// (the terminal applies reverse / dim / blink). A [`CellWidth::Trailer`] is
/// skipped — the wide head's symbol spans two columns in the terminal, just
/// as [`paint_text_inner`] leaves the spill-over cell for the wide grapheme.
///
/// The cursor inverts its cell (toggling [`Modifier::REVERSED`], so an
/// already-reversed cell still reads distinct). A character buffer has no
/// sub-cell bar / underline geometry, so the DECSCUSR *shape* — honoured by
/// the Vello backend (R993) — is a hardware-cursor concern (the host TTY's
/// own cursor) left to a future shell-level slice; the buffer shows the
/// universally-available reverse-block.
///
/// R995 §2 #6 — the cross-backend cell-structure consistency this arm shares
/// with the Vello `paint_text_grid` (which cell inks a glyph / reads reversed
/// / forms a wide span, colour staying backend-resolved) is regression-pinned
/// by `r995_text_grid_cross_consistency_tui` (below) and its Vello sibling in
/// `pinion-shell`, both driving the one shared
/// `pinion_core::test_fixtures::text_grid_consistency_buffer`.
fn paint_text_grid_inner(
    n: &TextGridNode,
    buf: &mut Buffer,
    clip: CellClip,
    offset_px: (i32, i32),
) {
    let grid = n.cells();
    if grid.is_empty() {
        return;
    }
    let buf_area = buf.area;
    let cursor = grid.cursor();
    // The grid's top-left in screen cells (the buffer's own metric). Each
    // grid cell then occupies one buffer character cell.
    let screen_col_px = i64::from(n.rect.x) + i64::from(offset_px.0);
    let screen_row_px = i64::from(n.rect.y) + i64::from(offset_px.1);
    let origin_col = pixels_to_cell_floor(screen_col_px, CELL.cell_w());
    let origin_row = pixels_to_cell_floor(screen_row_px, CELL.cell_h());
    // Crop an over-large producer buffer to the node's rect-derived winsize,
    // matching the Vello adapter's clip-to-`rect` (the buffer dims and the
    // layout-derived winsize are distinct facts that diverge during an
    // in-flight resize, R974.1) — so both backends show the same logical
    // window of cells (§2 #6).
    let max_rows = grid.rows().min(n.rows());
    let max_cols = grid.cols().min(n.cols());
    for row in 0..max_rows {
        let cell_row = origin_row.saturating_add(i32::from(row));
        if cell_row < clip.y0 || cell_row >= clip.y1 {
            continue;
        }
        for col in 0..max_cols {
            let Some(cell) = grid.cell(col, row) else {
                continue;
            };
            // The wide head carries the glyph; the trailer is the terminal's
            // implicit spill-over cell (left untouched, like wide text). The
            // producer reports the cursor at the logical (head) column, never a
            // trailer, so skipping trailers here never drops a cursor.
            if cell.width == CellWidth::Trailer {
                continue;
            }
            let cell_col = origin_col.saturating_add(i32::from(col));
            if cell_col < clip.x0 || cell_col >= clip.x1 {
                continue;
            }
            let Some((bx, by)) = cell_to_buf_xy(cell_col, cell_row, buf_area) else {
                continue;
            };
            let mut modifier = cell_attrs_to_modifier(cell.attrs);
            // The cursor inverts its cell (the reversed head renders two columns
            // wide for a wide glyph, matching the Vello span). A character
            // buffer has no sub-cell bar / underline shape — the DECSCUSR shape
            // is a hardware-cursor concern (left to a shell-level slice).
            if cursor.visible && cursor.col == col && cursor.row == row {
                modifier ^= Modifier::REVERSED;
            }
            // A blank cell still carries its colours; render it as a space.
            let symbol = if cell.cluster.is_empty() {
                " "
            } else {
                cell.cluster.as_ref()
            };
            let style = Style::default()
                .fg(term_color_to_tui(cell.fg))
                .bg(term_color_to_tui(cell.bg))
                .add_modifier(modifier);
            let bcell = &mut buf[(bx, by)];
            bcell.set_symbol(symbol);
            bcell.set_style(style);
        }
    }
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
        // (1, 0) is the wide grapheme's continuation cell. The walker
        // writes the glyph into the head cell only and advances `col`
        // by the grapheme width, leaving this cell as `Buffer::empty`'s
        // blank `" "`. That is EXACTLY ratatui's own convention: its
        // `Buffer::set_stringn` resets continuation cells via
        // `Cell::reset()`, which sets the symbol to `" "` (a space) —
        // NOT to an empty string. The blank is never drawn on screen
        // because `Buffer::diff` skips it via the head cell's width
        // (see `cjk_wide_continuation_matches_ratatui`). R1336.
        assert_eq!(buf[(1, 0)].symbol(), " ");
        assert_eq!(buf[(2, 0)].symbol(), "X");
    }

    /// R1336 — regression lock refuting the 2026-07-14 GAP report
    /// `pinion-gap-tui-cjk-continuation-cell-not-blanked`. The report
    /// claimed (a) ratatui's convention is to leave a wide grapheme's
    /// continuation cell as an EMPTY string `""`, and (b) pinion's
    /// `" "` continuation misaligns CJK on a real terminal. Both are
    /// false for ratatui 0.29, and this test proves it two ways:
    ///
    /// 1. pinion's buffer is byte-identical (symbol AND width) to
    ///    ratatui's OWN `Buffer::set_string` for the same text — so
    ///    pinion follows ratatui's continuation convention exactly
    ///    (`" "`, via `Cell::reset()`), not a divergent one.
    /// 2. A fresh-screen `Buffer::diff` (what the crossterm backend
    ///    actually draws) emits each glyph at its correct column and
    ///    NEVER emits a continuation cell — so no space is ever drawn
    ///    over the right half of a wide glyph. No misalignment.
    ///
    /// This guards against a future "fix" that rewrites continuation
    /// cells to `""`: that would silently diverge from ratatui's
    /// `set_string` for zero rendering benefit (the diff skips the
    /// cell either way). The AI-introspection path that DOES want an
    /// empty continuation is the `TextGrid`/`GridBuffer` RPC snapshot
    /// (its `CellWidth::Trailer` carries `""` — a different, pinion-
    /// owned surface), NOT a raw ratatui `Buffer` dump.
    #[test]
    fn cjk_wide_continuation_matches_ratatui() {
        use ratatui::style::Style;
        // The consumer's exact line, mixing wide CJK, narrow ASCII,
        // spaces and punctuation.
        let line = "D01 낮 · 갯들 (뻘이 열렸다)";
        let w = 40u16;

        let mut pin = Buffer::empty(TuiRect::new(0, 0, w, 1));
        to_buffer(&Scene::Text(text_at(0, 0, line)), &mut pin);

        // ratatui's own native render of the identical string.
        let mut rat = Buffer::empty(TuiRect::new(0, 0, w, 1));
        rat.set_string(0, 0, line, Style::default());

        // (1) Cell-for-cell identity: symbol and display width.
        for x in 0..w {
            assert_eq!(
                pin[(x, 0)].symbol(),
                rat[(x, 0)].symbol(),
                "symbol mismatch vs ratatui native at col {x}"
            );
            assert_eq!(
                pin[(x, 0)].symbol().width(),
                rat[(x, 0)].symbol().width(),
                "width mismatch vs ratatui native at col {x}"
            );
        }

        // (2) What a real terminal draws on a fresh screen. The
        // crossterm backend prints exactly `previous.diff(current)`;
        // on a blank screen `previous` is all spaces.
        let blank = Buffer::empty(TuiRect::new(0, 0, w, 1));
        let drawn: Vec<(u16, String)> = blank
            .diff(&pin)
            .into_iter()
            .map(|(x, _y, c)| (x, c.symbol().to_owned()))
            .collect();

        // Every drawn cell sits at its correct column, and no drawn
        // symbol is a bare continuation space sitting immediately
        // after a wide glyph (which would prove overdraw). We rebuild
        // the expected column→glyph placement from the source line and
        // assert the draw list matches it exactly.
        let mut expected: Vec<(u16, String)> = Vec::new();
        let mut col = 0u16;
        for g in line.graphemes(true) {
            let gw = u16::try_from(g.width()).unwrap_or(u16::MAX);
            if gw == 0 {
                continue;
            }
            // A run of spaces collapses into the blank background, so
            // ratatui's diff omits space cells that equal the prior
            // (blank) buffer. Only non-space glyphs are drawn.
            if g != " " {
                expected.push((col, g.to_owned()));
            }
            col += gw;
        }
        assert_eq!(
            drawn, expected,
            "terminal draw list must place each glyph at its wide-aware \
             column with no continuation overdraw"
        );
    }

    /// R1337 §2#6 — an explicitly-styled `TextNode` renders its
    /// colour / weight / italic / decoration in the TUI, matching the
    /// attributes the Vello backend draws (was dropped pre-R1337).
    #[test]
    fn text_style_maps_to_terminal_attrs() {
        use pinion_core::style::{Color, FontStyle, FontWeight, TextDecoration};
        use ratatui::style::{Color as TuiColor, Modifier};

        let mut node = text_at(0, 0, "A");
        node.style.fg_color = Color::rgb(0xff, 0, 0);
        node.style.font_weight = FontWeight::BOLD;
        node.style.font_style = FontStyle::Italic;
        node.style.decoration = TextDecoration::both();

        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 1));
        to_buffer(&Scene::Text(node), &mut buf);

        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), "A");
        assert_eq!(cell.fg, TuiColor::Rgb(0xff, 0, 0));
        assert!(cell.modifier.contains(Modifier::BOLD));
        assert!(cell.modifier.contains(Modifier::ITALIC));
        assert!(cell.modifier.contains(Modifier::UNDERLINED));
        assert!(cell.modifier.contains(Modifier::CROSSED_OUT));
    }

    /// R1337 — the framework-default `fg_color` (opaque black) must
    /// NOT force a black foreground: a terminal's default fg is
    /// theme-driven, so default text keeps the cell's `Reset` fg and
    /// stays readable on a dark terminal. Guards the R1336 ratatui
    /// parity and the pre-R1337 behaviour against regression.
    #[test]
    fn default_black_text_inherits_terminal_fg() {
        use ratatui::style::{Color as TuiColor, Modifier};
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 1));
        to_buffer(&Scene::Text(text_at(0, 0, "A")), &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "A");
        assert_eq!(buf[(0, 0)].fg, TuiColor::Reset);
        assert_eq!(buf[(0, 0)].modifier, Modifier::empty());
    }

    /// R1337 — a fully transparent `fg_color` (a == 0) paints no
    /// colour; the cell inherits the terminal default (mirrors the
    /// alpha short-circuit the box / grid paths use).
    #[test]
    fn transparent_text_fg_is_not_applied() {
        use pinion_core::style::Color;
        use ratatui::style::Color as TuiColor;
        let mut node = text_at(0, 0, "A");
        node.style.fg_color = Color::rgba(0xff, 0, 0, 0);
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 1));
        to_buffer(&Scene::Text(node), &mut buf);
        assert_eq!(buf[(0, 0)].fg, TuiColor::Reset);
    }

    /// R1337 §5.36 — rich-text `runs` colour each grapheme by the
    /// span covering its start byte; bytes outside every run fall
    /// back to the node's base style (here default → terminal fg).
    #[test]
    fn rich_text_runs_style_per_grapheme() {
        use pinion_core::scene::StyleRun;
        use pinion_core::style::{Color, TextStyle};
        use ratatui::style::Color as TuiColor;

        let mut red = TextStyle::new();
        red.fg_color = Color::rgb(0xff, 0, 0);
        let mut green = TextStyle::new();
        green.fg_color = Color::rgb(0, 0xff, 0);

        let mut node = text_at(0, 0, "RGB");
        node.runs = vec![StyleRun::new(0, 1, red), StyleRun::new(1, 2, green)];

        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 1));
        to_buffer(&Scene::Text(node), &mut buf);

        assert_eq!(buf[(0, 0)].fg, TuiColor::Rgb(0xff, 0, 0), "run 0 red");
        assert_eq!(buf[(1, 0)].fg, TuiColor::Rgb(0, 0xff, 0), "run 1 green");
        // Byte 2 is outside every run → base style (default black) →
        // terminal default fg.
        assert_eq!(buf[(2, 0)].fg, TuiColor::Reset, "uncovered → base");
    }

    /// R1339 — overlapping `runs` resolve **last-push-wins** per the
    /// `StyleRun` contract (matching parley / the Vello backend), NOT
    /// first-match. Two runs both cover byte 0; the one later in list
    /// order wins. Guards the R1337 divergence the reviewer flagged.
    #[test]
    fn rich_text_runs_overlap_is_last_push_wins() {
        use pinion_core::scene::StyleRun;
        use pinion_core::style::{Color, TextStyle};
        use ratatui::style::Color as TuiColor;

        let mut red = TextStyle::new();
        red.fg_color = Color::rgb(0xff, 0, 0);
        let mut green = TextStyle::new();
        green.fg_color = Color::rgb(0, 0xff, 0);

        let mut node = text_at(0, 0, "AB");
        // Both runs cover byte 0; `green` is later in list order → wins.
        node.runs = vec![StyleRun::new(0, 2, red), StyleRun::new(0, 1, green)];

        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 1));
        to_buffer(&Scene::Text(node), &mut buf);
        assert_eq!(
            buf[(0, 0)].fg,
            TuiColor::Rgb(0, 0xff, 0),
            "byte 0: later run (green) wins the overlap"
        );
        // Byte 1 is covered only by `red`.
        assert_eq!(buf[(1, 0)].fg, TuiColor::Rgb(0xff, 0, 0), "byte 1: red");
    }

    /// R1337 — a coloured WIDE (CJK) glyph: the head cell carries the
    /// symbol + colour, the continuation cell stays default. Because
    /// `Buffer::diff` skips the continuation via the head's width, the
    /// terminal draws the wide glyph once in the head's colour across
    /// both columns — the intersection of R1336 (wide) and R1337
    /// (colour), which neither round tested alone.
    #[test]
    fn colored_wide_cjk_glyph_head_carries_color() {
        use pinion_core::style::Color;
        use ratatui::style::Color as TuiColor;
        let mut node = text_at(0, 0, "한X");
        node.style.fg_color = Color::rgb(0, 0, 0xff);
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 1));
        to_buffer(&Scene::Text(node), &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "한");
        assert_eq!(buf[(0, 0)].fg, TuiColor::Rgb(0, 0, 0xff), "head coloured");
        // Continuation cell untouched; never drawn (diff skips by width).
        assert_eq!(buf[(1, 0)].symbol(), " ");
        assert_eq!(buf[(1, 0)].fg, TuiColor::Reset, "continuation default");
        // Fresh-screen draw list: only the coloured head + narrow "X".
        let blank = Buffer::empty(TuiRect::new(0, 0, 40, 1));
        let cols: Vec<u16> = blank.diff(&buf).into_iter().map(|(x, _, _)| x).collect();
        assert_eq!(cols, vec![0, 2], "continuation column not drawn");
    }

    /// R1337 — the bold threshold is CSS 700: `SEMI_BOLD` (600) has no
    /// terminal intensity and stays normal; exactly `BOLD` (700) sets
    /// the modifier. Locks the documented weight mapping.
    #[test]
    fn font_weight_bold_threshold_is_700() {
        use pinion_core::style::FontWeight;
        use ratatui::style::Modifier;
        let mut semi = text_at(0, 0, "S");
        semi.style.font_weight = FontWeight::SEMI_BOLD;
        let mut bold = text_at(0, 0, "B");
        bold.style.font_weight = FontWeight::BOLD;

        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 1));
        to_buffer(&Scene::Text(semi), &mut buf);
        assert!(
            !buf[(0, 0)].modifier.contains(Modifier::BOLD),
            "600 stays normal"
        );
        let mut buf2 = Buffer::empty(TuiRect::new(0, 0, 40, 1));
        to_buffer(&Scene::Text(bold), &mut buf2);
        assert!(buf2[(0, 0)].modifier.contains(Modifier::BOLD), "700 bold");
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
        let scroll =
            ScrollNode::new(Rect::new(0, 0, 80, 32), Scene::Container(content)).with_offset(0, 16);
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
        let inner_scroll = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 160, 160), inner_box));
        // Outer viewport (0, 0, 32, 32) = cell rect (0, 0, 4, 2).
        let outer_scroll = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 32, 32), inner_scroll));
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
        let scroll =
            ScrollNode::new(Rect::new(0, 0, 32, 32), content).with_offset(i32::MAX, i32::MAX);
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

    /// R994 §5.41 §2 #6 — the `Scene::TextGrid` TUI arm: each cell maps 1:1
    /// onto a ratatui cell with its symbol, host-resolved colours, SGR
    /// modifiers, wide-char spill-over, and a reverse-block cursor.
    #[test]
    fn r994_text_grid_paints_cells_attrs_wide_cursor() {
        use pinion_core::CellMetric;
        use pinion_core::scene::TextGridNode;
        use pinion_core::style::Color;
        use pinion_core::term_grid::{CursorShape, GridBuffer, GridCursor, TermCell, TermColor};

        // 한 (U+D55C) — a wide cluster; escaped per the non-ASCII source rule.
        const HAN: &str = "\u{D55C}";
        let e = CellAttrs::empty;
        // A 4x2 grid at pixel origin (0,0) → buffer origin cell (0,0) at the
        // 8x16 default metric, so grid cell (c,r) lands on buffer cell (c,r).
        let head = TermCell::new(HAN, TermColor::Indexed(2), TermColor::Default).wide();
        let buffer = GridBuffer::new(4, 2)
            .with_row(
                0,
                [
                    TermCell::new("A", TermColor::Indexed(1), TermColor::Indexed(0))
                        .with_attrs(e().with_bold(true)),
                    TermCell::new(
                        "B",
                        TermColor::Rgb(Color::rgb(0xff, 0, 0)),
                        TermColor::Default,
                    )
                    .with_attrs(e().with_italic(true).with_underline(true)),
                    TermCell::new(" ", TermColor::Default, TermColor::Default)
                        .with_attrs(e().with_reverse(true)),
                    TermCell::new("D", TermColor::Default, TermColor::Default)
                        .with_attrs(e().with_blink(true)),
                ],
            )
            .with_row(
                1,
                [
                    head.clone(),
                    head.trailer(),
                    TermCell::new("C", TermColor::Default, TermColor::Default),
                    TermCell::blank(),
                ],
            )
            .with_cursor(GridCursor::new(2, 1, CursorShape::Block, true));
        let mut node = TextGridNode::new(CellMetric::DEFAULT).with_cells(buffer);
        node.rect = Rect::new(0, 0, 32, 32);
        let scene = Scene::TextGrid(node);

        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        to_buffer(&scene, &mut buf);

        // (0,0) bold 'A'; indexed fg/bg pass through to the host palette.
        assert_eq!(buf[(0, 0)].symbol(), "A");
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(0, 0)].fg, TuiColor::Indexed(1));
        assert_eq!(buf[(0, 0)].bg, TuiColor::Indexed(0));

        // (1,0) italic + underline 'B'; truecolor fg passes through; the
        // default bg maps to the terminal's Reset (its own default).
        assert_eq!(buf[(1, 0)].symbol(), "B");
        assert!(
            buf[(1, 0)]
                .modifier
                .contains(Modifier::ITALIC | Modifier::UNDERLINED)
        );
        assert_eq!(buf[(1, 0)].fg, TuiColor::Rgb(0xff, 0, 0));
        assert_eq!(buf[(1, 0)].bg, TuiColor::Reset);

        // (2,0) SGR reverse -> REVERSED (no cursor here).
        assert!(buf[(2, 0)].modifier.contains(Modifier::REVERSED));

        // (3,0) blink -> SLOW_BLINK (the host terminal blinks; Vello defers it).
        assert!(buf[(3, 0)].modifier.contains(Modifier::SLOW_BLINK));

        // (0,1) wide head carries the glyph; the trailer (1,1) is left as the
        // terminal's spill-over cell (the default space).
        assert_eq!(buf[(0, 1)].symbol(), HAN);
        assert_eq!(buf[(0, 1)].fg, TuiColor::Indexed(2));
        assert_eq!(buf[(1, 1)].symbol(), " ");

        // (2,1) 'C' carries no SGR reverse, but the visible cursor sits here,
        // so the cell reverses (the cursor inverts its cell in a buffer).
        assert_eq!(buf[(2, 1)].symbol(), "C");
        assert!(buf[(2, 1)].modifier.contains(Modifier::REVERSED));
    }

    /// R995 §5.41 §2 #6 — cross-backend consistency (TUI half). Drives the
    /// shared [`text_grid_consistency_buffer`] through the TUI painter and
    /// asserts every cell's observable structure agrees with the model-derived
    /// [`expected_text_grid_cell_facts`]. The Vello half (`pinion-shell`
    /// headless-GPU) renders the *same* buffer and asserts the same model, so
    /// the two backends are pinned consistent through one source of truth — the
    /// ratatui buffer is exact, so this half pins the full structure (the GPU
    /// half can only observe a font-robust subset).
    ///
    /// Colour is deliberately *not* a cross-backend fact (the TUI hands indexed
    /// / default colours to the host terminal; Vello resolves them through the
    /// pinion palette) — the contract is cell-structure identity, not pixels.
    ///
    /// R1.6 glyph-paint campaign residue (honest, deferred beyond this closer):
    ///
    /// - **Font policy** — open question (monospace fallback, CJK / emoji
    ///   fallback) on the Vello side; the TUI defers glyph shaping to the host
    ///   terminal entirely.
    /// - **`blink`** — applied here (the host terminal blinks via
    ///   `Modifier::SLOW_BLINK`); the Vello backend defers it as a timing slice.
    /// - **Cursor shape** — `Bar` / `Underline` render as shaped beams in Vello
    ///   (R993) but as a reverse-block here (a character buffer has no sub-cell
    ///   shape; DECSCUSR shape is a hardware-cursor concern). Only the `Block`
    ///   cursor inverts the cell identically in both, so the fixture uses it.
    /// - **Pointer → cell hit-test** (R1.8) — mapping a pointer position back to
    ///   a grid `(col, row)` is a later input slice; not part of paint.
    #[test]
    fn r995_text_grid_cross_consistency_tui() {
        use pinion_core::CellMetric;
        use pinion_core::scene::TextGridNode;
        use pinion_core::test_fixtures::{
            TEXT_GRID_WIDE_HEAD, expected_text_grid_cell_facts, text_grid_consistency_buffer,
        };

        let buffer = text_grid_consistency_buffer();
        // 8×16 default metric, rect (0,0,32,48) → 4 cols × 3 rows, origin cell
        // (0,0): grid cell (c,r) lands on buffer cell (c,r).
        let mut node = TextGridNode::new(CellMetric::DEFAULT).with_cells(buffer.clone());
        node.rect = Rect::new(0, 0, 32, 48);
        let scene = Scene::TextGrid(node);

        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        to_buffer(&scene, &mut buf);

        // Every cell's observable structure must equal the model's facts.
        for row in 0..3u16 {
            for col in 0..4u16 {
                let f = expected_text_grid_cell_facts(&buffer, col, row);
                let bcell = &buf[(col, row)];
                // Visible ink: a non-blank symbol the terminal does not conceal.
                let observed_ink =
                    !bcell.modifier.contains(Modifier::HIDDEN) && !bcell.symbol().trim().is_empty();
                assert_eq!(
                    observed_ink,
                    f.inks_glyph,
                    "cell ({col},{row}) inks_glyph: symbol {:?} hidden={}",
                    bcell.symbol(),
                    bcell.modifier.contains(Modifier::HIDDEN),
                );
                assert_eq!(
                    bcell.modifier.contains(Modifier::REVERSED),
                    f.reversed,
                    "cell ({col},{row}) reversed",
                );
            }
        }

        // Wide head shows its grapheme; the trailer is the terminal spill cell
        // (left as the default space, so the head's glyph occupies both cols).
        assert_eq!(buf[(0, 1)].symbol(), TEXT_GRID_WIDE_HEAD, "wide head glyph");
        assert_eq!(buf[(1, 1)].symbol(), " ", "trailer is the spill cell");

        // SGR attrs map straight to ratatui Modifier bits (the host applies
        // them) — pin the representative set the fixture carries.
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD), "(0,0) bold");
        assert!(
            buf[(1, 0)]
                .modifier
                .contains(Modifier::ITALIC | Modifier::UNDERLINED),
            "(1,0) italic+underline",
        );
        assert!(
            buf[(3, 0)]
                .modifier
                .contains(Modifier::SLOW_BLINK | Modifier::CROSSED_OUT),
            "(3,0) blink+strikethrough",
        );
        assert!(
            buf[(2, 1)].modifier.contains(Modifier::HIDDEN),
            "(2,1) hidden"
        );

        // Indexed / truecolor / default colours pass through to the host
        // terminal verbatim (the documented backend divergence — Vello would
        // palette-resolve these instead).
        assert_eq!(buf[(2, 0)].fg, TuiColor::Indexed(2), "(2,0) indexed fg");
        assert_eq!(
            buf[(1, 0)].fg,
            TuiColor::Rgb(0xff, 0, 0),
            "(1,0) truecolor fg"
        );
        assert_eq!(buf[(3, 0)].fg, TuiColor::Reset, "(3,0) default fg → Reset");
    }
}
