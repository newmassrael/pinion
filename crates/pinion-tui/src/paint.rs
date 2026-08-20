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
//! goes through [`pinion_core::CellMetric`] (the R968 §5.41
//! cell-native metric).
//! This adapter renders against [`crate::text_layout::CELL`] — the 8×16
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

use crate::text_layout::{CellTextLayout, GapPad, grapheme_cells};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene, TextGridNode, TextNode};
use pinion_core::style::{BoxStyle, Color, FontStyle, FontWeight, TextStyle};
use pinion_core::term_grid::{CellAttrs, CellWidth, TermColor, effective_terms};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect as TuiRect;
use ratatui::style::{Color as TuiColor, Modifier, Style};
use unicode_segmentation::UnicodeSegmentation;

/// R968 §5.41 — the cell-native metric this TUI paint adapter renders
/// against. Sourced from [`CellMetric::DEFAULT`] (the behaviour-preserving
/// 8×16 baseline the pre-R968 `PIXEL_PER_CELL_*` constants carried). It
/// positions every node — including a `Scene::TextGrid`, whose own node-local
/// metric sizes Vello glyphs but is irrelevant to a character buffer (R994
/// maps grid cells 1:1) — with the shared `Rect` geometry staying in logical
/// pixels per the R968 ratify.
/// R1344 §5.41 — re-export of the crate's single cell metric
/// ([`crate::text_layout::CELL`]). The paint walker floors `rect` back to cells
/// with the same metric the layout pass budgeted them with; naming it twice is
/// how those two silently drift.
use crate::text_layout::CELL;

// R51.130 §5.41 — Unicode light box-drawing set (U+2500..U+2518).
// Lifted as `\u{XXXX}`-escaped constants so the Rust source carries
// only ASCII codepoints; the actual glyph lives in this comment only.
// Set: ─ │ ┌ ┐ └ ┘ (horizontal, vertical, top-left, top-right,
// bottom-left, bottom-right). The light weight is intentional —
// heavy / double variants would mis-match the single-cell border
// thickness the paint walker draws, since `BoxStyle::border.width` has
// no sub-cell resolution to map onto.
//
// R1514 corrects the rest of what this comment used to say. It filed
// `rounded` with heavy and double, and reported `corner_radius` as
// having nothing to map onto — but the light ARC set (U+256D..U+2570)
// is the same weight as the corners above, so the medium does carry a
// rounded corner and only this walker does not. That is a gap, not a
// property of the terminal, and it is recorded as one in
// `pinion-shell/tests/backend_parity.rs`. A reason that is not true is
// worse than no reason: it retires the question.
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
    // R1426 §5.41 — the STEADY-phase entry (`cursor_blink_on = true`): a
    // TextGrid cursor always paints. The ~40 test call sites + any non-animating
    // caller keep this two-arg signature; the live TUI shell calls
    // `to_buffer_with_cursor_phase` with its per-window blink clock's phase.
    to_buffer_with_cursor_phase(scene, buf, true);
}

/// R1426 §5.41 §5.28 — [`to_buffer`] with the render-time terminal-cursor blink
/// PHASE threaded in. `cursor_blink_on` is `true` on the visible half of the
/// blink and `false` on the hidden half; it gates only a
/// [blinking-mode](pinion_core::term_grid::GridCursor::blink) cursor (a steady
/// cursor ignores it) via the shared
/// [`GridCursor::shown_this_phase`](pinion_core::term_grid::GridCursor::shown_this_phase)
/// predicate — the same one the Vello backend uses, so the two never drift. On
/// the off-phase the inverted cursor cell is simply not painted, so the plain
/// cell shows (the terminal off-phase); the phase is never folded into the
/// scene, so `scene/snapshot` still reports the cursor `visible`/`blink` as
/// data (§2 #7). pinion paints the cursor as an inverted cell and keeps the
/// host terminal's hardware cursor hidden (the R1424 OSC-12 model), so this is
/// pinion's painted-cell projection blink, not a delegation to the host
/// terminal's own (settings-driven) cursor blink.
pub fn to_buffer_with_cursor_phase(scene: &Scene, buf: &mut Buffer, cursor_blink_on: bool) {
    let clip = CellClip::from_buf(buf.area);
    to_buffer_inner(scene, buf, clip, (0, 0), cursor_blink_on);
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
fn to_buffer_inner(
    scene: &Scene,
    buf: &mut Buffer,
    clip: CellClip,
    offset_px: (i32, i32),
    cursor_blink_on: bool,
) {
    match scene {
        Scene::Container(c) => paint_container(
            c,
            scene.clip_window(),
            buf,
            clip,
            offset_px,
            cursor_blink_on,
        ),
        Scene::Text(t) => paint_text_inner(t, buf, clip, offset_px),
        Scene::Box(b) => paint_box(b, buf, clip, offset_px),
        Scene::Scroll(s) => {
            let new_clip = match scene.clip_window() {
                Some(window) => clip.intersect(cell_clip_of(window, offset_px)),
                None => clip,
            };
            if new_clip.is_empty() {
                // Viewport entirely off-screen — skip the recursion
                // (and the shape work it would do) entirely.
                return;
            }
            // another declarative toolkit the child offset in pixels (same shape as the
            // Vello adapter's `child_transform = parent *
            // translate(viewport.xy - offset.xy)`): content-intrinsic
            // (0, 0) lands at `(viewport.x - scroll.offset_x,
            //  viewport.y - scroll.offset_y)` in the parent frame,
            // then the parent offset shifts it to screen.
            let cx = i64::from(offset_px.0) + i64::from(s.viewport.x) - i64::from(s.offset_x);
            let cy = i64::from(offset_px.1) + i64::from(s.viewport.y) - i64::from(s.offset_y);
            let child_offset = (clamp_to_i32(cx), clamp_to_i32(cy));
            to_buffer_inner(&s.content, buf, new_clip, child_offset, cursor_blink_on);
        }
        // R994 §5.41 §2 #6 — the cell-native grid's TUI sibling of the
        // Vello glyph paint (R991-R993): each grid cell maps 1:1 onto a
        // ratatui character cell.
        Scene::TextGrid(n) => paint_text_grid_inner(n, buf, clip, offset_px, cursor_blink_on),
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
fn paint_container(
    c: &ContainerNode,
    window: Option<pinion_core::scene::Rect>,
    buf: &mut Buffer,
    clip: CellClip,
    offset_px: (i32, i32),
    cursor_blink_on: bool,
) {
    paint_box_style(&c.rect, &c.style, buf, clip, offset_px);
    // (R1685 §5.45) `window` is `Scene::clip_window` — `Some` when this
    // container declares `Overflow::Hidden`. The container's own box is
    // painted first and unclipped (the border cells are the box, not its
    // contents); the children see the narrowed clip. The cell arithmetic is
    // the scroll arm's, shared rather than copied, because a clip that
    // rounded to cells one way here and another way there would put the two
    // clipping kinds a half-cell apart on the same terminal.
    let child_clip = match window {
        Some(window) => clip.intersect(cell_clip_of(window, offset_px)),
        None => clip,
    };
    if child_clip.is_empty() {
        return;
    }
    for child in &c.children {
        to_buffer_inner(child, buf, child_clip, offset_px, cursor_blink_on);
    }
}

/// (R1685 §5.45 §5.41) A clip window in this node's own pixel frame, projected
/// onto the screen cell grid.
///
/// i64 throughout to absorb the adversarial offset case (R55.E.1 mirror:
/// `with_offset(i32::MAX, ...)`) without intermediate wrap. Distinct `left_*` /
/// `top_*` naming avoids the `clippy::similar_names` trip the `sx0`/`sy0`
/// shorthand would otherwise hit.
fn cell_clip_of(window: pinion_core::scene::Rect, offset_px: (i32, i32)) -> CellClip {
    let left_px = i64::from(window.x) + i64::from(offset_px.0);
    let top_px = i64::from(window.y) + i64::from(offset_px.1);
    let right_px = left_px + i64::from(window.w);
    let bottom_px = top_px + i64::from(window.h);
    CellClip {
        x0: pixels_to_cell_floor(left_px, CELL.cell_w()),
        y0: pixels_to_cell_floor(top_px, CELL.cell_h()),
        x1: pixels_to_cell_floor(right_px, CELL.cell_w()),
        y1: pixels_to_cell_floor(bottom_px, CELL.cell_h()),
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
/// rect's edge cells. Pixel-`width` is intentionally ignored — a cell
/// is discrete, so no sub-cell border thickness exists at this
/// resolution, and the `placement` axis is flat for the same reason.
/// `corner_radius` is ignored too, but that one is a GAP rather than a
/// limit of the medium (R1514: the light arc glyphs U+256D..U+2570
/// carry a rounded corner at exactly the weight drawn here); see
/// `pinion-shell/tests/backend_parity.rs`, which records it as such.
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
/// R1344 §5.41 — the node's **own** `rect` bounds the text, not just
/// the ancestor clip. Two boundaries compose:
///
/// * `rect.w` is the wrap budget AND the horizontal bound. Lines come
///   from [`CellTextLayout::wrap_px`] — the same SSOT the measure pass
///   ([`crate::text_layout`]) sizes the node with, so the box always
///   holds exactly the rows painted here.
/// * `rect.h` bounds the row count; rows past the box are dropped.
///
/// Pre-R1344 neither was read: text painted from `rect.x` straight to
/// the ancestor clip edge on ONE row, so a paragraph truncated to its
/// first row and a long line overwrote its siblings' cells. Both are
/// §2 #6 divergences from the Vello backend, which wraps at `rect.w`
/// via parley and never leaves its box.
///
/// Vertical walk: each wrapped line maps to one cell row; rows outside
/// the clip strip are skipped individually (a multi-row node may
/// straddle the clip). Horizontal walk: each grapheme either fits
/// inside both bounds (write) or straddles one (skip but still advance
/// the column cursor so the rest of the line lays out correctly).
fn paint_text_inner(t: &TextNode, buf: &mut Buffer, clip: CellClip, offset_px: (i32, i32)) {
    let buf_area = buf.area;
    let screen_col_px = i64::from(t.rect.x) + i64::from(offset_px.0);
    let screen_row_px = i64::from(t.rect.y) + i64::from(offset_px.1);
    let cell_col = pixels_to_cell_floor(screen_col_px, CELL.cell_w());
    let cell_row = pixels_to_cell_floor(screen_row_px, CELL.cell_h());

    // The node's own box, in cells, intersected with the inherited
    // clip. `rect.w` / `rect.h` floor to whole cells: a box 2.5 cells
    // wide holds 2, matching `CellTextLayout`'s px→cell conversion.
    let layout = CellTextLayout::new(CELL);
    let box_cols = t.rect.w / CELL.cell_w();
    let box_rows = t.rect.h / CELL.cell_h();
    // R1551 §5.36 — place the lines BEFORE the clip, because a negative CSS
    // `text-indent` protrudes past the box's start edge (parley: "Negative
    // values cause the line to protrude beyond the start edge") and a clip
    // pinned at the box would erase on the terminal exactly what the pixel
    // backend draws. The widening is bounded by the declared outdent, so it can
    // never exceed what the paragraph asked for.
    // ★ R1655 §2 #6 — the overflow policy, resolved ONCE: the line ranges below
    // index the string that is placed, and every slice taken from them has to
    // come from that same string.
    let cut = crate::text_layout::painted_text(&t.content, box_cols, &t.style);
    let content: &str = cut.as_deref().unwrap_or(&t.content);
    let lines = layout.place_px(content, t.rect.w, &t.style);
    let outdent = lines
        .iter()
        .map(|l| l.start_col().min(0))
        .min()
        .unwrap_or(0);
    let node_clip = CellClip {
        x0: cell_col.saturating_add(outdent),
        y0: cell_row,
        x1: cell_col.saturating_add(clamp_to_i32(i64::from(box_cols))),
        y1: cell_row.saturating_add(clamp_to_i32(i64::from(box_rows))),
    };
    let clip = clip.intersect(node_clip);
    if clip.is_empty() {
        return;
    }

    for (line_idx, line) in lines.into_iter().enumerate() {
        let row =
            cell_row.saturating_add(clamp_to_i32(i64::try_from(line_idx).unwrap_or(i64::MAX)));
        // Rows below the box / clip: every later line is lower still.
        if row >= clip.y1 {
            break;
        }
        if row < clip.y0 {
            continue;
        }
        // `line_text` drops the trailing UAX #14 break codepoint, so no
        // control byte can reach a cell (a raw `\n` in the buffer would
        // emit a line feed mid-frame and corrupt the terminal).
        // The string the lines were PLACED from — see `painted_text`.
        let text = layout.line_text(content, line.range);
        paint_text_line(
            t,
            text,
            line.range.start,
            row,
            cell_col.saturating_add(line.start_col()),
            line.gap_pad,
            buf,
            buf_area,
            clip,
        );
    }
}

/// R1344 §5.41 — paint one wrapped line's graphemes across `row`.
///
/// `line_start` is the line's byte offset within `t.content`, so each
/// cluster's absolute offset still resolves its `TextNode.runs` span
/// (R1337 rich text) after wrapping.
#[allow(
    clippy::too_many_arguments,
    reason = "the cumulative-state walker's context: node + line slice + placement + clip"
)]
fn paint_text_line(
    t: &TextNode,
    text: &str,
    line_start: usize,
    row: i32,
    cell_col: i32,
    gap_pad: GapPad,
    buf: &mut Buffer,
    buf_area: TuiRect,
    clip: CellClip,
) {
    let mut col = cell_col;
    // R1551 §5.36 — CSS `text-align: justify` on a cell grid: the leftover
    // cells were divided across this line's inter-word gaps at placement time,
    // and here each gap simply advances further. Counting gaps the same way the
    // placement did (runs of `U+0020`, so a double space is one gap) is what
    // makes the painted right edge land where the placement computed it.
    let mut gap_index = 0u32;
    let mut in_gap = false;
    // R1337 §5.41 §2#6 — `grapheme_indices` (not `graphemes`) so each
    // cluster's UTF-8 byte offset resolves which `TextNode.runs` span
    // (rich text) governs it; empty `runs` falls back to `t.style`.
    for (byte_off, grapheme) in text.grapheme_indices(true) {
        if !gap_pad.is_none() {
            if grapheme == " " {
                if !in_gap {
                    // Pad at the gap's FIRST space, before drawing it, so the
                    // widened gap sits between the two words rather than after
                    // a run of drawn spaces.
                    col = col.saturating_add(
                        i32::try_from(gap_pad.for_gap(gap_index)).unwrap_or(i32::MAX),
                    );
                    gap_index = gap_index.saturating_add(1);
                    in_gap = true;
                }
            } else {
                in_gap = false;
            }
        }
        // R1344 — the SSOT advance rule, shared with the measure pass.
        // Zero-width: joiners / combining marks (the segmenter emits
        // these as separate clusters before joining) and control
        // characters no cell can hold. Skipping both keeps the painted
        // width equal to the measured width.
        let g_width = i32::try_from(grapheme_cells(grapheme)).unwrap_or(i32::MAX);
        if g_width == 0 {
            continue;
        }
        // Past the right bound (node box ∩ ancestor clip) — truncate.
        // R51.111+ adds the ellipsis policy.
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
        if let Some((bx, by)) = cell_to_buf_xy(col, row, buf_area) {
            let style = effective_text_style(t, line_start + byte_off);
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
/// Cost is `O(runs)` per grapheme — a linear attribute-list scan, like
/// Pango's `PangoAttrList` (an arbitrary-overlap linear list, "fine at
/// paragraph scale"); the empty-`runs` fast path (every single-style
/// node) is `O(1)`. Those frameworks resolve to `O(1)` per position at
/// layout-build time and cache it (parley `RangedBuilder` → `Layout`,
/// Pango's itemize step) — but what that cache buys is skipping **glyph
/// shaping**, and the terminal backend does none (no fonts / glyphs /
/// rasterisation — only a width lookup + `set_symbol`), so the cost
/// that justifies the cache is absent here. Measured, this scan is
/// sub-µs even on a 100-char / 30-run code line (a few hundred ns over
/// the single-style baseline), on damage-driven repaints — so any
/// resolve-once optimisation belongs with a future §5.36 TUI text cache
/// alongside the rest of the per-frame text work, not a bespoke
/// per-frame flatten here (which would allocate every frame and slow
/// the common few-run path). A naive advancing cursor is unsound
/// regardless: the contract fixes *list* order, not `start` order.
///
/// R1546 — the resolution rule itself is now
/// [`pinion_core::scene::effective_style_at`], lifted when this became one of
/// three copies of the same last-match walk. Everything above is why the
/// terminal backend does it
/// per grapheme rather than caching it, which is a property of this caller and
/// stays here.
fn effective_text_style(t: &TextNode, byte_off: usize) -> &TextStyle {
    pinion_core::scene::effective_style_at(&t.style, &t.runs, byte_off)
}

/// The pre-R1546 body, kept as the oracle the lift is checked against rather
/// than as a second implementation: `effective_style_at` must answer what this
/// answered for every offset a text node can be asked about.
#[cfg(test)]
fn effective_text_style_pre_lift(t: &TextNode, byte_off: usize) -> &TextStyle {
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
/// R1340 — weight maps onto the terminal's two intensity steps the
/// way every ANSI renderer does (`ansi_up`, ratatui's own `BOLD` ↔
/// `font-weight: bold` HTML map, and this codebase's `paint_adapter`
/// `CellAttrs.bold` → `FontWeight::BOLD`): **SGR 1 "bold"** for the CSS
/// `bold` keyword and above (`>= 700`), **SGR 2 "faint"**
/// (`Modifier::DIM`) for lighter-than-normal (`< 400`). `400..700`
/// (medium / semi-bold) has no terminal intensity and stays normal —
/// the terminal offers only these two steps, so the mapping follows
/// the `bold` / `normal` keyword boundaries rather than inventing
/// intermediate intensities. Italic and oblique map to the one
/// terminal italic; underline / strikethrough map straight across.
/// All are additive, so default-styled text sets no modifier and
/// stays byte-identical to the pre-R1337 (and ratatui-native) output.
fn apply_text_style(cell: &mut ratatui::buffer::Cell, style: &TextStyle) {
    let fg = style.fg_color;
    // Apply only a visible, non-black colour. Black (at any alpha) is
    // the framework default and inherits the terminal's theme fg; the
    // RGB-triplet test (not full-colour equality) keeps opaque and
    // semi-transparent black consistent.
    if fg.a > 0 && (fg.r, fg.g, fg.b) != (0, 0, 0) {
        cell.set_fg(color_to_tui(fg));
    }
    // R1546 §2#6 — the declared run background. No default-inherits rule
    // mirrors the foreground's, and the asymmetry is the point: an ABSENT
    // background is `None` and sets nothing, so the cell keeps the terminal's
    // theme. A background reaching here was asked for by name, and the black
    // that `fg_color` has to treat as "unset" is, for a background, an ordinary
    // colour somebody may well want.
    //
    // This is the capability the terminal cell has had all along — `TermCell`
    // carries `fg` AND `bg`, and the §5.41 grid arm has written both since
    // R994 — while a `Scene::Text` run could only speak the foreground. One
    // declaration now reaches both backends.
    if let Some(bg) = style.bg_color {
        cell.set_bg(color_to_tui(bg));
    }
    let mut modifier = Modifier::empty();
    // CSS weight → the terminal's two intensity steps: SGR 1 bold at
    // the CSS `bold` boundary (>= 700), SGR 2 faint below normal
    // (< 400). Terminals with no faint support degrade it to normal.
    if style.font_weight.0 >= FontWeight::BOLD.0 {
        modifier |= Modifier::BOLD;
    } else if style.font_weight.0 < FontWeight::NORMAL.0 {
        modifier |= Modifier::DIM;
    }
    if matches!(style.font_style, FontStyle::Italic | FontStyle::Oblique(_)) {
        modifier |= Modifier::ITALIC;
    }
    // R1540 — ratatui's `Modifier` has ONE underline bit, so every drawn form
    // collapses to it here. `is_on` is exactly the query
    // `UnderlineStyle` documents for a backend that cannot distinguish forms;
    // the SGR 4:x forms reach a real terminal through the cell-grid arm, which
    // writes the escape itself.
    if style.decoration.underline.is_on() {
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
    // R1399 — ratatui's `Modifier` has one `UNDERLINED` bit, so every drawn
    // SGR 4:x style (single / double / curly / dotted / dashed) collapses to
    // it here (the TUI's best approximation); the underline *colour* is
    // applied separately on the `Style` at the cell-paint site.
    if attrs.underline.is_on() {
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
/// [`pinion_core::CellMetric`] is irrelevant here (it sizes Vello
/// glyphs). The grid's
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
    cursor_blink_on: bool,
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
    // Crop an over-large producer buffer to the node's winsize (the buffer
    // dims and the winsize are distinct facts that diverge during an
    // in-flight resize, R974.1) — so both backends show the same logical
    // window of cells (§2 #6).
    //
    // R1542 — `n.cols()` / `n.rows()` are no longer necessarily derived from
    // `rect`: a producer sized by some other authority declares its winsize
    // and this clamp then honours the declaration. The Vello adapter grew the
    // identical clamp in the same round; before it, that arm leaned on its
    // `rect` clip, which agrees with this only while the winsize IS the
    // derivation.
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
            let (mut fg, mut bg) = (cell.fg, cell.bg);
            // The cursor inverts its cell (the reversed head renders two columns
            // wide for a wide glyph, matching the Vello span). A character
            // buffer has no sub-cell bar / underline shape — the DECSCUSR shape
            // is a hardware-cursor concern (left to a shell-level slice).
            // R1426 §5.41 — the render-time blink phase gates a blinking-mode
            // cursor through the shared `shown_this_phase` predicate (a steady
            // cursor ignores the phase): on the off-phase the inverted cell is
            // not painted, so the plain cell shows — the terminal off-phase.
            if cursor.shown_this_phase(cursor_blink_on) && cursor.col == col && cursor.row == row {
                match cursor.cursor_color {
                    // R1424 §5.41 — an explicit OSC-12 cursor colour paints the
                    // cursor block in that colour with the glyph reading through
                    // in the cell's effective background, matching the Vello
                    // adapter's block cursor. The cell's own SGR-7 reverse is
                    // resolved here (into `eff_bg`) rather than left to the
                    // REVERSED modifier, so the explicit colours land verbatim.
                    Some(cc) => {
                        // R1749 — the swap comes from `term_grid`, not from a
                        // second spelling of it here. Measured at R1749: this
                        // line and the Vello adapter's `effective_terms` were
                        // the same rule written twice, and this one was the
                        // half that only computed a background — so an
                        // attribute that also swapped would have been honoured
                        // by one renderer and silently ignored by this one.
                        let (_, eff_bg) = effective_terms(cell);
                        bg = TermColor::Rgb(cc);
                        fg = eff_bg;
                        modifier.remove(Modifier::REVERSED);
                    }
                    // The universally-available reverse-block cursor (no explicit
                    // colour): toggle REVERSED so an already-reversed cell still
                    // reads distinct.
                    None => modifier ^= Modifier::REVERSED,
                }
            }
            // A blank cell still carries its colours; render it as a space.
            let symbol = if cell.cluster.is_empty() {
                " "
            } else {
                cell.cluster.as_ref()
            };
            let mut style = Style::default()
                .fg(term_color_to_tui(fg))
                .bg(term_color_to_tui(bg))
                .add_modifier(modifier);
            // R1399 — an explicit SGR-58 underline colour rides on the
            // ratatui `Style` (0.29 `underline_color`); the SGR-59 default
            // (`None`) is left unset so the terminal tracks the foreground.
            if let Some(uc) = cell.underline_color {
                style = style.underline_color(term_color_to_tui(uc));
            }
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
    use unicode_width::UnicodeWidthStr;

    /// Construct a minimal single-line `TextNode` for tests at pixel
    /// `(x, y)` with the given content, sized to hold it exactly: one
    /// row tall and as wide as the content measures in cells.
    ///
    /// R1344 §5.41 — this now does what its name and its pre-R1344
    /// docstring always *claimed* ("width / height set to the
    /// content's grapheme count × cell metrics"). It previously
    /// hardcoded `w = 100` (12.5 cells) regardless of content, which
    /// only worked because the walker ignored `rect.w` entirely — a
    /// fixture artifact of the bug. Now that the box bounds the text,
    /// a fixture that under-sizes its content would clip it, so the
    /// helper measures through the same SSOT the painter uses.
    ///
    /// Callers testing clip / wrap behaviour build their own rect.
    fn text_at(x: u32, y: u32, content: &str) -> TextNode {
        let cols = u32::try_from(crate::text_layout::cell_width(content)).unwrap_or(u32::MAX);
        let mut node = TextNode::default();
        node.content = content.to_owned();
        node.rect = Rect::new(x, y, cols * CELL.cell_w(), CELL.cell_h());
        node
    }

    // ----- R1554 §5.39 §2 #6 — the disabled fade reaches the CELLS -----

    /// The §5.39 cascade fades a disabled region's colours in the scene, which
    /// is what carries the fade into the terminal: a `TermCell` has no alpha
    /// and [`color_to_tui`] drops it, so an alpha-reduction fade — the smaller
    /// change — would have dimmed the window and left the terminal at full
    /// contrast.
    ///
    /// Run, not assumed. The scene here is built and cascaded exactly as a
    /// producer's is, then painted through the real `to_buffer`, and the cell
    /// the label lands in is read back.
    #[test]
    fn r1554_a_disabled_region_paints_faded_glyphs_into_the_terminal() {
        use pinion_core::scene_disabled::resolve_disabled;
        use pinion_core::style::{BoxStyle, Color, LayoutStyle};

        let ink = Color::rgb(0x1a, 0x1a, 0x1a);
        let backdrop = Color::rgb(0xff, 0xff, 0xff);
        let build = |disabled: bool| {
            let mut label = text_at(0, 0, "Verbose");
            label.style.fg_color = ink;
            let region = ContainerNode::new(vec![Scene::Text(label)])
                .with_layout(LayoutStyle::new().with_disabled(disabled));
            let mut root = ContainerNode::new(vec![Scene::Container(region)])
                .with_style(BoxStyle::filled(backdrop));
            root.rect = Rect::new(0, 0, 40 * CELL.cell_w(), CELL.cell_h());
            let mut scene = Scene::Container(root);
            resolve_disabled(&mut scene);
            let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 1));
            to_buffer(&scene, &mut buf);
            buf
        };

        let live = build(false);
        let gated = build(true);
        let cell = |b: &Buffer| b[(0, 0)].fg;
        assert_ne!(
            cell(&gated),
            cell(&live),
            "a terminal cell inside a disabled region is not painted with the \
             ink a live one is",
        );
        assert_eq!(
            cell(&live),
            color_to_tui(ink),
            "and the live arm is the declared ink, so the difference above is \
             the fade and not some other divergence",
        );
        assert_eq!(
            cell(&gated),
            color_to_tui(ink.lerp(backdrop, pinion_core::widgets::interaction::DISABLED)),
            "the cell carries the M3 disabled ink over the region's backdrop \
             — the same value the window paints",
        );
    }

    /// R1546 §5.36 — the lifted resolution answers what the pre-lift body
    /// answered, at every offset a text node can be asked about.
    ///
    /// The oracle is the removed code itself, kept in the file, so this is a
    /// comparison rather than a restatement: a lift that changed first-match to
    /// last-match, or moved a boundary by one byte, disagrees here. The
    /// overlapping and adjacent runs below are the cases where the two rules
    /// differ; a non-overlapping list would let a first-match lift pass.
    #[test]
    fn r1546_the_lifted_style_resolution_answers_what_the_pre_lift_body_did() {
        let mut node = text_at(0, 0, "Row label");
        let base = TextStyle::new();
        let mut a = base.clone();
        a.fg_color = pinion_core::style::Color::rgb(0xAA, 0, 0);
        let mut b = base.clone();
        b.fg_color = pinion_core::style::Color::rgb(0, 0, 0xBB);
        let mut c = base.clone();
        c.fg_color = pinion_core::style::Color::rgb(0, 0xCC, 0);
        node.runs = vec![
            pinion_core::scene::StyleRun::new(0, 6, a),
            // Overlaps the first — the case that separates last-match from
            // first-match.
            pinion_core::scene::StyleRun::new(4, 7, b),
            // Abuts the second — the case that separates an inclusive `end`
            // from an exclusive one.
            pinion_core::scene::StyleRun::new(7, 9, c),
        ];
        for off in 0..=node.content.len() + 2 {
            assert_eq!(
                effective_text_style(&node, off),
                effective_text_style_pre_lift(&node, off),
                "offset {off}",
            );
        }
    }

    /// R1546 §2#6 — a run's declared background reaches the terminal cell.
    ///
    /// The capability gap this closes: `TermCell` has carried `fg` AND `bg`
    /// since the §5.41 grid arm existed, while a `Scene::Text` run could speak
    /// only the foreground — so one document was paintable with a highlight in
    /// a terminal grid and not as text.
    #[test]
    fn r1546_a_run_background_reaches_the_terminal_cell() {
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 10));
        let mut node = text_at(0, 0, "Row label");
        let mut hl = TextStyle::new();
        hl.bg_color = Some(pinion_core::style::Color::rgb(0xFF, 0xF1, 0x76));
        node.runs = vec![pinion_core::scene::StyleRun::new(0, 3, hl)];
        to_buffer(&Scene::Text(node), &mut buf);

        let painted = ratatui::style::Color::Rgb(0xFF, 0xF1, 0x76);
        assert_eq!(buf[(0, 0)].symbol(), "R");
        assert_eq!(buf[(0, 0)].bg, painted, "the declared byte is highlighted");
        assert_eq!(buf[(2, 0)].bg, painted);
        // Outside the run the cell keeps the terminal's own background —
        // `None` sets nothing rather than painting a default.
        assert_eq!(buf[(3, 0)].symbol(), " ");
        assert_eq!(
            buf[(4, 0)].bg,
            ratatui::style::Color::Reset,
            "an undeclared byte inherits the terminal theme",
        );
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

    /// R1543 §5.39 §2 #6 — a mnemonic's underline reaches the TERMINAL cell.
    ///
    /// The GUI half of this claim is
    /// `pinion_text::cache::tests::r1543_a_mnemonic_underlines_only_its_own_character`,
    /// which drives the real shaper. Both painters read the same derived
    /// [`StyleRun`], which is exactly why the mnemonic needed no per-backend
    /// paint code — but "needed none" is a claim, and R1542's lesson is that a
    /// capability added to a node has to be OBSERVED at every painter, not
    /// inferred from sharing a field.
    #[test]
    fn r1543_a_mnemonic_underlines_only_its_own_character() {
        use ratatui::style::Modifier;

        let mut node = TextNode::mnemonic_styled(
            "&File",
            pinion_core::scene::Rect::default(),
            pinion_core::style::TextStyle::new(),
        );
        node.rect = pinion_core::scene::Rect::new(0, 0, 200, 20);
        assert_eq!(node.content, "File", "the marker is not painted");

        let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 1));
        to_buffer(&Scene::Text(node), &mut buf);

        assert!(
            buf[(0, 0)].modifier.contains(Modifier::UNDERLINED),
            "the marked F is underlined"
        );
        for x in 1..4 {
            assert!(
                !buf[(x, 0)].modifier.contains(Modifier::UNDERLINED),
                "cell {x} is not: the run covers ONE character, not the label"
            );
        }
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

    /// R1340 — CSS weight maps onto the terminal's two intensity steps
    /// per the ANSI-renderer convention: `>= 700` (CSS `bold`) → SGR 1
    /// bold; `< 400` (lighter than normal) → SGR 2 faint (`DIM`);
    /// `400..700` (medium / semi-bold) has no terminal step and stays
    /// normal. Locks the full documented mapping.
    #[test]
    fn font_weight_maps_to_bold_and_faint() {
        use pinion_core::style::FontWeight;
        use ratatui::style::Modifier;

        let render = |w: FontWeight| {
            let mut n = text_at(0, 0, "X");
            n.style.font_weight = w;
            let mut buf = Buffer::empty(TuiRect::new(0, 0, 40, 1));
            to_buffer(&Scene::Text(n), &mut buf);
            buf[(0, 0)].modifier
        };

        // Light (< 400) → faint, not bold.
        let light = render(FontWeight::LIGHT);
        assert!(light.contains(Modifier::DIM), "300 → faint");
        assert!(!light.contains(Modifier::BOLD), "300 not bold");
        // Medium / SemiBold (400..700) → neither step.
        assert_eq!(render(FontWeight::MEDIUM), Modifier::empty(), "500 normal");
        let semi = render(FontWeight::SEMI_BOLD);
        assert!(!semi.contains(Modifier::BOLD), "600 not bold");
        assert!(!semi.contains(Modifier::DIM), "600 not faint");
        // Bold (>= 700) → bold, not faint.
        let bold = render(FontWeight::BOLD);
        assert!(bold.contains(Modifier::BOLD), "700 bold");
        assert!(!bold.contains(Modifier::DIM), "700 not faint");
        // Normal (400) → neither.
        assert_eq!(render(FontWeight::NORMAL), Modifier::empty(), "400 normal");
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

    /// R1424 §5.41 §2 #6 — an explicit OSC-12 cursor colour on the TUI arm.
    /// A `GridCursor::with_cursor_color` cursor paints its cell block in that
    /// colour (`bg`) with the glyph reading through in the cell's effective
    /// background (`fg`), NOT the plain reverse-block toggle — matching the
    /// Vello adapter's coloured block cursor. The cell's own SGR-7 reverse is
    /// resolved into the effective background rather than left to `REVERSED`.
    #[test]
    fn r1424_text_grid_paints_explicit_cursor_color() {
        use pinion_core::CellMetric;
        use pinion_core::scene::TextGridNode;
        use pinion_core::style::Color;
        use pinion_core::term_grid::{CursorShape, GridBuffer, GridCursor, TermCell, TermColor};

        let green = Color::rgb(0x2e, 0xcc, 0x71);
        // Cell (0,0): plain fg=indexed 1 / bg=indexed 0, under a green cursor.
        // Cell (1,0): SGR-7 reverse fg=indexed 3 / bg=indexed 5, under a green
        //             cursor — the effective background is the reversed one.
        let buffer = GridBuffer::new(2, 1)
            .with_row(
                0,
                [
                    TermCell::new("X", TermColor::Indexed(1), TermColor::Indexed(0)),
                    TermCell::new("Y", TermColor::Indexed(3), TermColor::Indexed(5))
                        .with_attrs(CellAttrs::empty().with_reverse(true)),
                ],
            )
            .with_cursor(GridCursor::new(0, 0, CursorShape::Block, true).with_cursor_color(green));
        let mut node = TextGridNode::new(CellMetric::DEFAULT).with_cells(buffer);
        node.rect = Rect::new(0, 0, 16, 16);
        let scene = Scene::TextGrid(node);
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 8, 4));
        to_buffer(&scene, &mut buf);

        // (0,0) — the cursor block is the OSC-12 green; the glyph reads through
        // in the cell's (non-reversed) background (indexed 0). REVERSED is
        // cleared — the coloured cursor is not a plain inversion.
        assert_eq!(buf[(0, 0)].symbol(), "X");
        assert_eq!(buf[(0, 0)].bg, TuiColor::Rgb(0x2e, 0xcc, 0x71));
        assert_eq!(buf[(0, 0)].fg, TuiColor::Indexed(0));
        assert!(!buf[(0, 0)].modifier.contains(Modifier::REVERSED));

        // A green cursor lands its dims on the whole grid, but (1,0) has no
        // cursor, so it renders normally (SGR-7 REVERSED, its own colours).
        // Move the cursor there to prove the effective-bg resolution honours
        // the cell's own reverse.
        let buffer2 = GridBuffer::new(2, 1)
            .with_row(
                0,
                [
                    TermCell::new("X", TermColor::Indexed(1), TermColor::Indexed(0)),
                    TermCell::new("Y", TermColor::Indexed(3), TermColor::Indexed(5))
                        .with_attrs(CellAttrs::empty().with_reverse(true)),
                ],
            )
            .with_cursor(GridCursor::new(1, 0, CursorShape::Block, true).with_cursor_color(green));
        let mut node2 = TextGridNode::new(CellMetric::DEFAULT).with_cells(buffer2);
        node2.rect = Rect::new(0, 0, 16, 16);
        let scene2 = Scene::TextGrid(node2);
        let mut buf2 = Buffer::empty(TuiRect::new(0, 0, 8, 4));
        to_buffer(&scene2, &mut buf2);
        // (1,0) — cursor block still green; the effective background of a
        // reversed cell is its foreground (indexed 3), so the glyph reads
        // through in indexed 3. REVERSED cleared (the reverse is resolved).
        assert_eq!(buf2[(1, 0)].symbol(), "Y");
        assert_eq!(buf2[(1, 0)].bg, TuiColor::Rgb(0x2e, 0xcc, 0x71));
        assert_eq!(buf2[(1, 0)].fg, TuiColor::Indexed(3));
        assert!(!buf2[(1, 0)].modifier.contains(Modifier::REVERSED));
    }

    /// R1426 §5.41 §2 #6 — the render-time blink PHASE gates a blinking-mode
    /// cursor in the TUI painter: on the off-phase the inverted cell is not
    /// painted (the plain cell shows), while a steady cursor and every other
    /// cell are unaffected — the pixel-equivalent proof of the cross-backend
    /// `GridCursor::shown_this_phase` gate (the ratatui cell IS the pixel).
    #[test]
    fn r1426_blink_phase_gates_the_inverted_cursor_cell() {
        use pinion_core::CellMetric;
        use pinion_core::scene::TextGridNode;
        use pinion_core::term_grid::{CursorShape, GridBuffer, GridCursor, TermCell, TermColor};

        // A 2x1 grid: a plain glyph at (0,0), and the cursor cell 'C' at (1,0).
        fn grid(cursor: GridCursor) -> Scene {
            let buffer = GridBuffer::new(2, 1)
                .with_row(
                    0,
                    [
                        TermCell::new("A", TermColor::Default, TermColor::Default),
                        TermCell::new("C", TermColor::Default, TermColor::Default),
                    ],
                )
                .with_cursor(cursor);
            let mut node = TextGridNode::new(CellMetric::DEFAULT).with_cells(buffer);
            node.rect = Rect::new(0, 0, 16, 16);
            Scene::TextGrid(node)
        }

        let paint = |scene: &Scene, on: bool| {
            let mut buf = Buffer::empty(TuiRect::new(0, 0, 8, 4));
            to_buffer_with_cursor_phase(scene, &mut buf, on);
            buf
        };

        // A BLINKING cursor: reversed on the ON phase, plain on the OFF phase.
        let blinking = grid(GridCursor::new(1, 0, CursorShape::Block, true).with_blink(true));
        let on = paint(&blinking, true);
        assert_eq!(on[(1, 0)].symbol(), "C");
        assert!(
            on[(1, 0)].modifier.contains(Modifier::REVERSED),
            "blinking cursor inverts its cell on the ON phase",
        );
        let off = paint(&blinking, false);
        assert_eq!(off[(1, 0)].symbol(), "C", "the cell content is unchanged");
        assert!(
            !off[(1, 0)].modifier.contains(Modifier::REVERSED),
            "blinking cursor is not painted on the OFF phase — the plain cell shows",
        );
        // The non-cursor cell (0,0) is identical across phases — the phase gates
        // only the cursor overlay, nothing else in the grid.
        assert_eq!(on[(0, 0)].modifier, off[(0, 0)].modifier);
        assert!(!off[(0, 0)].modifier.contains(Modifier::REVERSED));

        // A STEADY cursor is reversed on BOTH phases (the phase never gates it).
        let steady = grid(GridCursor::new(1, 0, CursorShape::Block, true));
        assert!(
            paint(&steady, true)[(1, 0)]
                .modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            paint(&steady, false)[(1, 0)]
                .modifier
                .contains(Modifier::REVERSED),
            "a steady cursor is drawn regardless of the blink phase",
        );

        // The default two-arg `to_buffer` paints the STEADY phase (a blinking
        // cursor is drawn — the non-animating / test entry).
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 8, 4));
        to_buffer(&blinking, &mut buf);
        assert!(
            buf[(1, 0)].modifier.contains(Modifier::REVERSED),
            "to_buffer forwards the steady ON phase, so the cursor paints",
        );
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

    /// R1542 §5.41 §2 #6 — a declared winsize bounds what the TUI paints,
    /// and the sub-grid margin is left untouched.
    ///
    /// This arm has clamped to `n.cols()` / `n.rows()` since R995, so R1542
    /// changed no code here — it changed what those two answer. The guard is
    /// what makes that a property rather than a coincidence: the buffer
    /// delivers 4 columns, the producer was told to hold 3, and the 4th must
    /// not reach the terminal buffer even though the node's rect spans it.
    ///
    /// Its Vello sibling is `pinion-shell`'s
    /// `r1542_declared_winsize_leaves_the_sub_grid_margin_to_the_gutter_fill`,
    /// which asserts the same column is the palette default background rather
    /// than the delivered cell. Before R1542 the two backends disagreed about
    /// it: this one clamped, that one leaned on the rect clip and painted it.
    #[test]
    fn r1542_declared_winsize_bounds_what_the_tui_paints() {
        use pinion_core::CellMetric;
        use pinion_core::scene::TextGridNode;
        use pinion_core::term_grid::{GridBuffer, TermCell, TermColor};

        fn cell(c: &'static str) -> TermCell {
            TermCell::new(c, TermColor::Default, TermColor::Default)
        }
        let buffer =
            GridBuffer::new(4, 1).with_row(0, vec![cell("a"), cell("b"), cell("c"), cell("d")]);
        // The rect spans all four columns (32px / 8px), so only the
        // declaration excludes the last one.
        let mut node = TextGridNode::new(CellMetric::DEFAULT)
            .with_cells(buffer)
            .with_winsize(3, 1);
        node.rect = Rect::new(0, 0, 32, 16);
        assert_eq!((node.cols(), node.rows()), (3, 1), "precondition");
        let scene = Scene::TextGrid(node);

        let mut buf = Buffer::empty(TuiRect::new(0, 0, 8, 4));
        to_buffer(&scene, &mut buf);

        for (col, want) in [(0u16, "a"), (1, "b"), (2, "c")] {
            assert_eq!(
                buf[(col, 0u16)].symbol(),
                want,
                "column {col} is inside the declared grid",
            );
        }
        assert_eq!(
            buf[(3u16, 0u16)].symbol(),
            " ",
            "column 3 was DELIVERED but not declared: it is the sub-grid \
             margin, and the terminal cell must be left as the parent drew it",
        );
    }

    // ─────────────────────────────────────────────────────────────
    // R1344 §5.41 §2 #6 — text box + wrap regressions.
    //
    // Every test here fails against the pre-R1344 walker, which read
    // only `rect.x` / `rect.y`, never wrapped, and wrote whatever
    // `unicode-segmentation` handed it straight into a cell. The
    // defects were found by a consumer targeting BOTH backends from
    // one `view()` (the first to localize a pinion app): its prose
    // rendered as a paragraph on Vello and as one truncated row on the
    // terminal.
    // ─────────────────────────────────────────────────────────────

    /// Render `scene` into a `cols`×`rows` buffer and return the rows as
    /// trimmed strings — the shape most of these assertions want.
    fn render_rows(scene: &Scene, cols: u16, rows: u16) -> Vec<String> {
        let mut buf = Buffer::empty(TuiRect::new(0, 0, cols, rows));
        to_buffer(scene, &mut buf);
        (0..rows)
            .map(|y| {
                (0..cols)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    /// A `TextNode` with an explicitly authored box, for the wrap / clip
    /// tests (`text_at` sizes itself to its content instead).
    fn text_in_box(x: u32, y: u32, w: u32, h: u32, content: &str) -> TextNode {
        let mut node = TextNode::default();
        node.content = content.to_owned();
        node.rect = Rect::new(x, y, w, h);
        node
    }

    // ---- D1: a control character must never reach a cell ----

    #[test]
    fn r1344_hard_line_break_starts_a_new_row_and_never_inks_a_control_char() {
        // Pre-R1344: `"\n".width() == 1` (unicode-width scores C0
        // controls as ONE cell, not zero), so the walker's zero-width
        // skip missed it and `set_symbol("\n")` put a raw U+000A in a
        // cell. ratatui's crossterm backend `Print`s the symbol with no
        // sanitisation, so a line feed hit the terminal mid-frame: the
        // cursor jumped and the frame corrupted.
        let scene = Scene::Text(text_in_box(0, 0, 240, 64, "line1\nline2"));
        let rows = render_rows(&scene, 30, 4);
        assert_eq!(rows[0], "line1", "hard break ends row 0");
        assert_eq!(rows[1], "line2", "and the tail starts row 1");

        let mut buf = Buffer::empty(TuiRect::new(0, 0, 30, 4));
        to_buffer(&scene, &mut buf);
        for y in 0..4u16 {
            for x in 0..30u16 {
                let sym = buf[(x, y)].symbol();
                assert!(
                    !sym.chars().any(char::is_control),
                    "cell ({x},{y}) holds control char {sym:?} — it would reach the terminal",
                );
            }
        }
    }

    #[test]
    fn r1344_every_hard_break_form_and_blank_line_survives_paint() {
        // CRLF, lone CR, and the Unicode separators are all UAX #14
        // mandatory breaks; a blank line must stay a blank row rather
        // than collapse.
        for (content, want) in [
            ("a\r\nb", vec!["a", "b"]),
            ("a\u{2028}b", vec!["a", "b"]),
            ("a\n\nb", vec!["a", "", "b"]),
        ] {
            let scene = Scene::Text(text_in_box(0, 0, 240, 64, content));
            let rows = render_rows(&scene, 30, 4);
            for (i, w) in want.iter().enumerate() {
                assert_eq!(&rows[i], w, "row {i} of {content:?}");
            }
        }
    }

    #[test]
    fn r1344_interior_tab_is_not_inked() {
        // A tab is not a line break, so the breaker passes it through —
        // but a raw HT in a cell would move the terminal cursor just as
        // a LF would. `cell_width` scores it 0 and the walker skips it,
        // which is why the two must agree (see `text_layout`).
        let scene = Scene::Text(text_in_box(0, 0, 240, 16, "a\tb"));
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 30, 1));
        to_buffer(&scene, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "a");
        assert_eq!(buf[(1, 0)].symbol(), "b", "the tab occupies no cell");
    }

    // ---- D2: soft wrap at rect.w ----

    #[test]
    fn r1344_paragraph_soft_wraps_at_the_box_width() {
        // Pre-R1344 this rendered as ONE row truncated at the terminal
        // edge, while Vello wrapped the same node into a paragraph —
        // the §2 #6 divergence a dual-backend consumer hits first.
        let scene = Scene::Text(text_in_box(
            0,
            0,
            240, // 30 cells
            64,  // 4 rows
            "the quick brown fox jumps over the lazy dog again",
        ));
        let rows = render_rows(&scene, 30, 4);
        assert_eq!(rows[0], "the quick brown fox jumps");
        assert_eq!(rows[1], "over the lazy dog again");
        assert!(rows[2].is_empty(), "no third row needed");
    }

    #[test]
    fn r1344_wrap_tracks_the_box_not_the_terminal() {
        // The budget is the NODE's width. Same text, same terminal, two
        // boxes → two different wraps. Pre-R1344 both produced the same
        // single truncated row, because only the terminal edge bounded
        // the text.
        let text = "alpha beta gamma delta";
        let wide = render_rows(&Scene::Text(text_in_box(0, 0, 240, 64, text)), 30, 4);
        let narrow = render_rows(&Scene::Text(text_in_box(0, 0, 80, 64, text)), 30, 4);
        assert_eq!(
            wide[0], "alpha beta gamma delta",
            "30-cell box holds it all"
        );
        // "alpha beta " measures 11 cells against a 10-cell box, so the
        // break lands after "alpha ". Trailing-whitespace hang (the UAX
        // #14 refinement that lets a line's trailing spaces exceed the
        // width) is a deferral of pinion's breaker, inherited here — see
        // `pinion_text_unicode::wrap`. Not a claim of parity with parley,
        // which wraps the pixel backend and has its own break points.
        assert_eq!(narrow[0], "alpha", "10-cell box wraps after 'alpha '");
        assert_eq!(narrow[1], "beta");
    }

    #[test]
    fn r1344_wide_graphemes_wrap_at_two_cells_each() {
        // CJK is 2 cells: a 4-cell box holds exactly two per row. The
        // wrap budget must count display cells, not chars or bytes.
        let ga = "\u{AC00}"; // 가
        let text = format!("{ga}{ga}{ga}{ga}");
        let scene = Scene::Text(text_in_box(0, 0, 32, 64, &text)); // 4 cells
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 20, 4));
        to_buffer(&scene, &mut buf);
        // Asserted per cell, not per row string: a wide grapheme owns cell
        // N and ratatui blanks its continuation cell N+1 (the R1336
        // convention), so a naive row join reads "가 가".
        for row in 0..2u16 {
            assert_eq!(buf[(0, row)].symbol(), ga, "row {row} col 0");
            assert_eq!(buf[(2, row)].symbol(), ga, "row {row} col 2");
            assert_eq!(
                buf[(4, row)].symbol(),
                " ",
                "row {row}: only two wide graphemes fit a 4-cell box",
            );
        }
    }

    #[test]
    fn r1344_an_unbreakable_token_overflows_rather_than_vanishing() {
        // The breaker's documented overflow rule: a segment with no
        // interior opportunity is emitted on its own line even though it
        // exceeds the budget. Paint then clips it to the box — but it
        // must never silently drop the row or loop.
        let scene = Scene::Text(text_in_box(0, 0, 40, 32, "wwwwwwwwwwwwww short"));
        let rows = render_rows(&scene, 20, 2);
        assert_eq!(rows[0], "wwwww", "clipped to the 5-cell box, not dropped");
        assert_eq!(rows[1], "short");
    }

    // ---- D3: the node's box bounds the text ----

    #[test]
    fn r1344_text_never_paints_outside_its_own_box() {
        // Pre-R1344 `rect.w` / `rect.h` were never read: text ran from
        // `rect.x` to the ancestor clip edge on one row.
        let scene = Scene::Text(text_in_box(0, 0, 40, 16, "AAAAAAAAAABBBBBBBBBB"));
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 30, 3));
        to_buffer(&scene, &mut buf);
        for x in 5..30u16 {
            assert_eq!(
                buf[(x, 0)].symbol(),
                " ",
                "col {x} is outside the 5-cell box and must stay blank",
            );
        }
    }

    #[test]
    fn r1344_rect_h_bounds_the_row_count() {
        // A box one row tall shows one row, however many the text wraps
        // to — the vertical half of the same bound.
        let scene = Scene::Text(text_in_box(0, 0, 80, 16, "alpha beta gamma delta"));
        let rows = render_rows(&scene, 20, 4);
        assert_eq!(rows[0], "alpha");
        assert!(rows[1].is_empty(), "row 1 is outside a 1-row box");
    }

    #[test]
    fn r1344_long_text_does_not_destroy_a_sibling_pane() {
        // The concrete harm D3 caused: a left pane's prose ran straight
        // across the pane boundary and interleaved with its neighbour
        // ("LEFT PANE PROSERIGHT IS LONG"). Two panes, rects fully
        // resolved — exactly the shape the TUI contract expects.
        let mut root = ContainerNode::default();
        root.rect = Rect::new(0, 0, 240, 32);

        let mut left = ContainerNode::default();
        left.rect = Rect::new(0, 0, 120, 32); // 15 cells
        left.children.push(Scene::Text(text_in_box(
            0,
            0,
            120,
            32,
            "LEFT PANE PROSE THAT IS LONG",
        )));

        let mut right = ContainerNode::default();
        right.rect = Rect::new(120, 0, 120, 32);
        right
            .children
            .push(Scene::Text(text_in_box(120, 0, 120, 16, "RIGHT")));

        root.children.push(Scene::Container(left));
        root.children.push(Scene::Container(right));

        let mut buf = Buffer::empty(TuiRect::new(0, 0, 30, 2));
        to_buffer(&Scene::Container(root), &mut buf);
        let row0: String = (0..30).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(
            row0.starts_with("LEFT PANE"),
            "left pane keeps its text: {row0:?}",
        );
        assert_eq!(
            &row0[15..20],
            "RIGHT",
            "the right pane's cells are intact: {row0:?}",
        );
    }

    // ---- measure/paint SSOT coherence ----

    #[test]
    fn r1344_painted_rows_equal_measured_rows() {
        // The invariant that keeps a laid-out box and its content in
        // agreement: whatever height the measure pass gives a node, the
        // paint walker fills exactly that many rows. If these two ever
        // drift, a box is sized for N rows and shows N±1.
        use crate::text_layout::CellTextLayout;
        use pinion_runtime::layout::TextMeasure;

        let layout = CellTextLayout::default();
        for text in [
            "short",
            "the quick brown fox jumps over the lazy dog",
            "hard\nbreaks\nhere",
            "가나다 mixed 폭 width text that must wrap somewhere",
        ] {
            let box_w = 80u32; // 10 cells
            let measured = layout
                .measure_text(text, &TextStyle::default(), &[], Some(box_w), false)
                .expect("cell layout always measures");
            let measured_h = measured.height;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let measured_rows = (measured_h as u32) / CELL.cell_h();

            // Paint into a box tall enough to hold every measured row.
            let scene = Scene::Text(text_in_box(
                0,
                0,
                box_w,
                measured_rows * CELL.cell_h(),
                text,
            ));
            let rows = render_rows(&scene, 10, u16::try_from(measured_rows + 2).unwrap());
            let painted = rows.iter().filter(|r| !r.is_empty()).count();
            let expected = u32::try_from(
                (0..measured_rows)
                    .filter(|i| {
                        let l = layout.wrap_px(text, box_w);
                        layout
                            .line_text(text, l[*i as usize])
                            .chars()
                            .next()
                            .is_some()
                    })
                    .count(),
            )
            .unwrap();
            assert_eq!(
                u32::try_from(painted).unwrap(),
                expected,
                "painted rows must match the measured layout for {text:?}",
            );
        }
    }

    #[test]
    fn r1344_empty_content_paints_nothing_and_stays_in_its_box() {
        let scene = Scene::Text(text_in_box(0, 0, 80, 16, ""));
        let rows = render_rows(&scene, 10, 2);
        assert!(rows.iter().all(String::is_empty), "empty text inks nothing");
    }

    #[test]
    fn r1344_zero_width_box_paints_nothing_but_does_not_panic() {
        // An unresolved / collapsed box. The layout still reports the
        // text's hard structure (see `CellTextLayout::wrap`), but paint
        // clips every row away horizontally.
        let scene = Scene::Text(text_in_box(0, 0, 0, 16, "abc\ndef"));
        let rows = render_rows(&scene, 10, 2);
        assert!(
            rows.iter().all(String::is_empty),
            "nothing fits a 0-cell box"
        );
    }

    // ---- rich text survives wrapping (R1337 interaction) ----

    #[test]
    fn r1344_style_runs_still_resolve_after_a_wrap() {
        // R1337 mapped `TextNode.runs` onto cells by byte offset. Wrapping
        // splits content into line slices, so the walker must offset each
        // line's byte positions back into the ORIGINAL content or every
        // row after the first picks up the wrong run.
        use pinion_core::scene::StyleRun;

        let mut red = TextStyle::new();
        red.fg_color = Color::rgb(0xff, 0, 0);
        let mut node = text_in_box(0, 0, 40, 32, "aaaa bbbb");
        // Colour only the second word (bytes 5..9), which lands on row 1.
        node.runs = vec![StyleRun::new(5, 9, red)];
        let mut buf = Buffer::empty(TuiRect::new(0, 0, 10, 2));
        to_buffer(&Scene::Text(node), &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "a", "row 0 is the first word");
        assert_eq!(buf[(0, 1)].symbol(), "b", "row 1 is the second word");
        assert_eq!(
            buf[(0, 1)].fg,
            TuiColor::Rgb(0xff, 0, 0),
            "the run must follow its bytes onto the wrapped row",
        );
        assert_ne!(
            buf[(0, 0)].fg,
            TuiColor::Rgb(0xff, 0, 0),
            "row 0 is outside the run",
        );
    }
    /// ★ R1655 §2 #6 — the eliding arms reach the TERMINAL BUFFER, through the
    /// painter rather than past it.
    ///
    /// The first version of this test called the policy directly and a
    /// counterfactual that disabled it in the paint path reported PASSED: a
    /// capability verified only through a bypass is not verified (R1649.1's
    /// lesson, one crate over). This reads the cells.
    #[test]
    fn r1655_an_eliding_arm_reaches_the_terminal_cells() {
        use pinion_core::style::TextOverflow;
        let row = |overflow| {
            let mut label = text_at(0, 0, "demo/units/1/pose");
            label.style.overflow = overflow;
            // Eight cells wide for a seventeen-cell string, and TWO cells
            // tall — a one-cell box clips the wrap away and the control case
            // would prove nothing.
            label.rect = Rect::new(0, 0, 8 * CELL.cell_w(), 2 * CELL.cell_h());
            let mut buf = Buffer::empty(TuiRect::new(0, 0, 8, 2));
            paint_text(&label, &mut buf);
            (0..2)
                .map(|r| {
                    (0..8)
                        .map(|c| buf[(c, r)].symbol().to_owned())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        };

        let elided = row(TextOverflow::Ellipsis);
        assert!(
            elided[0].contains('\u{2026}'),
            "the first row says the string was cut: {elided:?}"
        );
        assert!(
            elided[1].trim().is_empty(),
            "★ and there is no SECOND row — an eliding arm is single-line, so \
             nothing lands on whatever the next widget occupies: {elided:?}"
        );
        assert!(
            elided[0].starts_with("demo"),
            "what survived is the head: {elided:?}"
        );

        let wrapped = row(TextOverflow::Visible);
        assert!(
            !wrapped[1].trim().is_empty(),
            "the default arm still wraps, which is what makes the check above \
             mean something: {wrapped:?}"
        );
    }
}
