//! R56.1.b.2 §5.36 §5.38 — closed-form caret geometry lookup against
//! a shaped parley [`Layout`]. Pure function: takes a layout reference
//! and a byte offset, returns the cursor rectangle in layout-space.
//!
//! Substrate primitive for the R56.1.b.1 hello-textfield first visible
//! consumer — the application's paint backend reads the caret position
//! through this helper instead of re-implementing parley cursor math
//! per binding.
//!
//! ## Why a separate helper
//!
//! parley exposes [`parley::Cursor::geometry`] which returns a
//! [`parley::BoundingBox`] (f64 four-corner). The pinion paint
//! pipeline uses f32 coordinates throughout, and the `BoundingBox`
//! corner-pair shape (`x0` / `y0` / `x1` / `y1`) is less ergonomic
//! than the top-left + size form. This helper bridges the two: f64
//! → f32 conversion + corner-pair → top-left/size rewrite, returning
//! a pinion-flavored [`CaretRect`] that the §5.38 R56.1.b
//! [`caret_rect`](pinion_core::widgets::text_field::caret_rect)
//! integer helper can downstream-cast into the paint scene's
//! `Rect` (u32).
//!
//! The split keeps the type boundary clean: pinion-text owns the
//! parley wrap, pinion-core owns the paint-space integer rect, and
//! the binding's application code converts f32 → u32 at the seam
//! (saturating cast — overflow is layout out-of-bounds, the binding
//! decides clamp policy).

use crate::Layout;
use parley::{Affinity, BreakReason, Cursor, Selection};

/// R762 §5.36 §5.38 — closed-form **pixel → byte** hit-test: the inverse
/// of [`caret_rect_for_byte_offset`]. Maps a layout-space point
/// `(x, y)` to the UTF-8 byte offset of the nearest caret insertion
/// position, wrapping [`parley::Cursor::from_point`] +
/// [`parley::Cursor::index`].
///
/// This is the substrate the paint backend reads when a pointer-down /
/// drag lands inside a text field: the binding converts the click from
/// window-local pixels to text-local layout-space (subtracting the
/// field's post-layout origin + padding + horizontal scroll), then this
/// helper returns the byte offset to feed
/// [`TextEditState::set_caret`](pinion_core::widgets::text_edit::TextEditState::set_caret)
/// (click-to-position) or
/// [`set_selection`](pinion_core::widgets::text_edit::TextEditState::set_selection)
/// (drag-select).
///
/// ## Style-agnostic (works over styled runs)
///
/// `layout` is a fully-shaped parley [`Layout`]; parley already spans
/// the R713 [`StyleRun`](pinion_core::scene::StyleRun) multi-style runs
/// inside one layout (it splits shaping per run but exposes a single
/// cluster space). So the same hit-test serves both the single-style
/// `TextField` and a future multi-style rich-text editor — the byte
/// offset it returns is independent of which run the point fell in.
///
/// ## Clamping
///
/// `parley::Cursor::from_point` is total: a point left of / above the
/// text clamps to byte 0, a point right of / below the last line clamps
/// to `text.len()`. The returned offset always lands on a char
/// boundary (parley resolves to a cluster edge), so it is safe to pass
/// straight into the char-boundary-clamping
/// [`TextEditState`](pinion_core::widgets::text_edit::TextEditState)
/// setters without a second clamp.
///
/// ## Example
///
/// ```ignore
/// // Round-trips with `caret_rect_for_byte_offset`:
/// let layout = cache.layout("hello", &style, None);
/// let caret = pinion_text::caret_rect_for_byte_offset(layout, 3, 1.0);
/// let mid_y = caret.y + caret.height * 0.5;
/// let byte = pinion_text::byte_offset_for_point(layout, caret.x, mid_y);
/// assert_eq!(byte, 3);
/// ```
#[must_use]
pub fn byte_offset_for_point(layout: &impl TextLayout, x: f32, y: f32) -> usize {
    layout.byte_at_point(x, y)
}

/// Caret rectangle in layout-space (f32 coordinates).
///
/// `x` / `y` are the top-left corner of the cursor rect; `width` /
/// `height` are the rect dimensions. Layout-space is parley's
/// coordinate system — the application's paint backend translates
/// to pixel coordinates via the same transform it uses for glyph
/// runs.
///
/// `#[non_exhaustive]` matches the rest of the pinion type surface
/// convention ([`Scene`](pinion_core::scene::Scene),
/// [`ScrollBarGeometry`](pinion_core::widgets::scrollbar::ScrollBarGeometry),
/// etc.). A future field for vertical-text / RTL anchor hints lands
/// in a minor bump without a `SemVer` major break.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretRect {
    /// Left edge of the caret in layout-space pixels.
    pub x: f32,
    /// Top edge of the caret (line top).
    pub y: f32,
    /// Caret width — same value passed into
    /// [`caret_rect_for_byte_offset`] (parley does not clamp the
    /// caller-provided width on the hot path).
    pub width: f32,
    /// Caret height — derived from the line metrics that contain
    /// the cursor's byte offset, not from the caller. Spans the
    /// full line box (canonical text-input caret shape on every
    /// platform).
    pub height: f32,
}

impl CaretRect {
    /// R56.2.c §5.36 — public constructor for `#[non_exhaustive]`
    /// [`CaretRect`]. The struct's `#[non_exhaustive]` attribute
    /// prevents downstream crates from using the struct-literal
    /// shape `CaretRect { x, y, width, height }`; this constructor
    /// is the textbook accessor pattern for non-exhaustive types
    /// (matches the `Modifiers::new` shape pinion-core uses).
    ///
    /// Use sites: application-side
    /// [`WidgetView::ime_caret_rect`](https://docs.rs/pinion-shell/latest/pinion_shell/trait.WidgetView.html#method.ime_caret_rect)
    /// impls (e.g. `examples/hello-textfield`) that synthesise a
    /// caret rect in *window-local logical-pixel* coordinates
    /// (the trait return frame) by summing the field's post-layout
    /// window origin + padding + the
    /// [`caret_rect_for_byte_offset`] result. The substrate's own
    /// `caret_rect_for_byte_offset` returns text-local coordinates;
    /// the application is the only caller that needs to construct
    /// the window-coord variant by hand.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// R56.1.b.2 §5.36 §5.38 — closed-form caret rectangle for the byte
/// offset `byte_index` in `layout`. Wraps
/// [`parley::Cursor::from_byte_index`] +
/// [`parley::Cursor::geometry`] to return a backend-agnostic
/// f32-typed [`CaretRect`].
///
/// `caret_width` is the visual width of the caret in layout-space
/// pixels (typically `1.0` for Hi-DPI displays where AA softens
/// single-pixel lines, `2.0` for integer-scaled Lo-DPI displays).
/// The width passes through to parley unchanged (parley uses it as
/// the geometry rect width directly).
///
/// `byte_index` is clamped by parley to `[0, text.len()]` — the
/// out-of-range path lands on the closest valid char boundary
/// (parley's `Cursor::from_byte_index` contract). Callers must
/// still supply a char-boundary-safe offset for the in-range path;
/// the
/// [`TextEditState`](pinion_core::widgets::text_edit::TextEditState)
/// reactive store maintains this invariant via
/// `clamp_to_char_boundary` (R56.1.b §5.22).
///
/// Affinity defaults to [`Affinity::Downstream`] — the canonical
/// text-input "caret follows insertion point" semantic. Bidi / line-
/// boundary edge cases where `Upstream` is required (RTL mixed text
/// at a logical line break, end-of-line ambiguity in soft-wrap)
/// arrive with the R56.1.f selection axis (which also exposes the
/// affinity parameter through a dedicated entry point so callers
/// can stay on the simple downstream-default path for single-line
/// input).
///
/// ## Example
///
/// ```ignore
/// // After shaping "hello" via LayoutCache::layout:
/// let layout: &pinion_text::Layout = cache.layout("hello", &style, None);
/// let caret = pinion_text::caret_rect_for_byte_offset(layout, 5, 1.0);
/// // caret.x is the x-pixel position of the caret after the last 'o'.
/// // caret.y / caret.height reflect the line that contains byte 5.
/// ```
#[must_use]
pub fn caret_rect_for_byte_offset(
    layout: &impl TextLayout,
    byte_index: usize,
    caret_width: f32,
) -> CaretRect {
    layout.caret_rect(byte_index, caret_width)
}

/// R764 §5.36 §5.22 — per-line selection rectangles for the byte range
/// `[start, end)` against a shaped [`Layout`], wrapping
/// [`parley::Selection::geometry`]. A single-line range yields one
/// rect; a range that spans hard line breaks (or soft wraps) yields one
/// rect per visual line — the partial first line, full middle lines,
/// and partial last line a text editor paints as the selection band.
///
/// This is the multi-line generalisation of the single-band selection
/// rect the §5.38 `TextField` paint computed by hand from two
/// [`caret_rect_for_byte_offset`] calls (which is only correct when
/// both ends sit on the same line). Returns rects in layout-space f32;
/// the paint backend translates to pixels with the same transform it
/// uses for glyph runs. An empty (collapsed) range returns no rects.
///
/// The ends are passed in any order — parley swaps internally so the
/// caller may hand `(anchor, caret)` directly without normalising.
#[must_use]
pub fn selection_rects_for_range(
    layout: &impl TextLayout,
    start: usize,
    end: usize,
) -> Vec<CaretRect> {
    layout.selection_rects(start, end)
}

/// R764 §5.36 §5.22 / R766 — byte offset after moving the caret `delta`
/// visual lines from `byte` (vertical caret navigation: `ArrowUp` =
/// `-1`, `ArrowDown` = `+1`), holding a persistent **goal column**.
///
/// Returns `(new_byte, goal_x)`: the resolved char-boundary offset plus
/// the layout-space `x` the move aimed for. The caller persists
/// `goal_x` (in
/// [`TextEditState::set_goal_column`](pinion_core::widgets::text_edit::TextEditState::set_goal_column))
/// and feeds it back as `goal_x` on the next move in the run so the
/// caret returns to the original column after crossing a short line —
/// the canonical "goal column" contract. Pass `goal_x = None` to seed a
/// fresh run from the caret's current column.
///
/// ## Why goal-column lives here, not inside the parley `Selection`
///
/// parley's [`Selection`] carries its own `h_pos`
/// and threads it through consecutive [`move_lines`](parley::Selection::move_lines)
/// calls. pinion's caret is a geometry-free byte offset reshaped each
/// frame, so a fresh `Selection::from_byte_index` is built every call
/// and parley's `h_pos` is lost — each move would re-seed the column
/// from the (possibly short-line-clamped) current caret and drift. We
/// therefore persist the goal `x` ourselves and resolve in two steps:
///
/// 1. [`move_lines`](parley::Selection::move_lines) finds the target
///    visual line and handles the document-boundary clamp (up from the
///    first line / down from the last) exactly — its specialty.
/// 2. If that landed on a *different* line band (interior move), we
///    override the drifted column by hit-testing `goal_x` at the centre
///    of the resolved line via [`byte_offset_for_point`]. At a document
///    boundary (no line change) parley's `line_start` / `line_end`
///    result is already canonical, so it is returned as-is.
///
/// The line band is detected from [`caret_rect_for_byte_offset`] (whose
/// `height` spans the full line box): a `y` shift over half a line box
/// means the band changed. This assumes the uniform line height of a
/// single-style textarea and the `±1` deltas keyboard navigation emits
/// (the only caller). The returned offset is a char boundary, safe for
/// the [`TextEditState`](pinion_core::widgets::text_edit::TextEditState)
/// setters.
#[must_use]
pub fn byte_offset_for_line_move(
    layout: &impl TextLayout,
    byte: usize,
    delta: isize,
    goal_x: Option<f32>,
) -> (usize, f32) {
    layout.line_move(byte, delta, goal_x)
}

/// R766 §5.36 §5.22 — byte offset of the **visual** line boundary
/// containing `byte`: the start (`end = false`, the `Home` key) or end
/// (`end = true`, the `End` key) of the wrapped visual line, wrapping
/// [`parley::Selection::line_start`] / [`line_end`](parley::Selection::line_end).
///
/// "Visual" (not logical) is the canonical multi-line `Home` / `End`:
/// on a soft-wrapped paragraph the caret moves to the start / end of
/// the *displayed* row, not the whole hard-break-delimited line, so it
/// must be resolved against the shaped [`Layout`] — like
/// [`byte_offset_for_line_move`], it cannot live on the geometry-free
/// [`TextEditState`](pinion_core::widgets::text_edit::TextEditState).
/// A single-line field has one visual line, so the boundaries coincide
/// with byte `0` / `text.len()` (the geometry-free
/// [`move_home`](pinion_core::widgets::text_edit::TextEditState::move_home)
/// / `move_end` path it keeps).
///
/// Selection extension (`Shift+Home` / `Shift+End`) is left to the
/// caller, which feeds the returned offset to `set_selection` against
/// the retained anchor (mirror of the `ArrowUp` / `ArrowDown` shift
/// path). The returned offset is a char boundary.
#[must_use]
pub fn byte_offset_for_line_boundary(layout: &impl TextLayout, byte: usize, end: bool) -> usize {
    layout.line_boundary(byte, end)
}

/// R956 §5.36 §5.22 — geometry of one **visual line** in a shaped
/// [`Layout`]: its top edge, box height, and whether it begins a new
/// *logical* (hard-`\n`-delimited) line.
///
/// A "visual line" is parley's [`parley::Line`] — the unit a layout
/// breaks into, counting soft-wrap rows separately. A logical line that
/// soft-wraps into three displayed rows is three visual lines, only the
/// first of which has [`starts_logical_line`](Self::starts_logical_line)
/// set — exactly the row a line-number gutter paints its number on.
///
/// `y` / `height` are layout-space f32 (parley `block_min_coord` and the
/// `block_max_coord - block_min_coord` span), the same coordinate frame
/// [`caret_rect_for_byte_offset`] returns — a caret on visual line *i*
/// shares that line's `y` and `height`, so a gutter built from these
/// metrics aligns row-for-row with the painted glyphs and the caret box.
///
/// `#[non_exhaustive]` matches the [`CaretRect`] convention: a later
/// field (baseline offset, first byte of the line) lands additively
/// without a `SemVer` major break.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VisualLineMetric {
    /// Top edge of the visual line box in layout-space pixels
    /// (parley `LineMetrics::block_min_coord`).
    pub y: f32,
    /// Height of the visual line box
    /// (`block_max_coord - block_min_coord`) — the full line box a
    /// gutter number / current-line highlight spans.
    pub height: f32,
    /// `true` when this visual line is the **first** row of a logical
    /// (hard-`\n`-delimited) line. The first line of the layout starts
    /// logical line 0; any line whose predecessor terminated at an
    /// explicit `\n` ([`parley::BreakReason::Explicit`]) starts the next
    /// one. Soft-wrap (`Regular`) and long-word (`Emergency`) breaks
    /// continue the current logical line, so their following rows leave
    /// this `false` — the gutter shows one number per logical line, on
    /// its first displayed row.
    pub starts_logical_line: bool,
}

impl VisualLineMetric {
    /// R1077 §5.36 §5.37 — public constructor for `#[non_exhaustive]`
    /// [`VisualLineMetric`] (mirrors [`CaretRect::new`]). A second
    /// [`TextLayout`] implementor in another crate (the §5.37
    /// `SelfHostedLayout` in `pinion-runtime`) builds visual-line metrics
    /// from its own per-line geometry, which the struct-literal form
    /// forbids across the crate boundary.
    #[must_use]
    pub const fn new(y: f32, height: f32, starts_logical_line: bool) -> Self {
        Self {
            y,
            height,
            starts_logical_line,
        }
    }
}

/// R956 §5.36 §5.22 — per-**visual-line** geometry for a shaped
/// [`Layout`], one [`VisualLineMetric`] per displayed row in top-to-bottom
/// order. Wraps [`parley::Layout::lines`] + [`parley::Line::metrics`] /
/// [`break_reason`](parley::Line::break_reason).
///
/// This is the line-metrics sibling of the caret / selection geometry
/// helpers: where [`caret_rect_for_byte_offset`] answers "where is byte
/// *b*?" and [`selection_rects_for_range`] answers "which rows does range
/// *[s, e)* cover?", this answers "where is every line?" — the substrate a
/// line-number gutter, a current-line highlight, or viewport row-culling
/// reads. The returned `y` / `height` share the layout-space frame of the
/// caret rect, so a gutter aligns with the painted text without a second
/// shaping pass (the caller borrows the same shared
/// [`LayoutCache`](crate::LayoutCache) `Layout`).
///
/// An empty layout (no text) still yields one metric: parley lays out a
/// single empty line carrying the resolved style's line box, so a gutter
/// shows line "1" for an empty document.
///
/// ## Example
///
/// ```ignore
/// // "first\nsecond" shaped via LayoutCache::layout:
/// let lines = pinion_text::visual_line_metrics(layout);
/// assert_eq!(lines.len(), 2);                       // two visual lines
/// assert!(lines[0].starts_logical_line);            // line 1
/// assert!(lines[1].starts_logical_line);            // line 2 (after \n)
/// assert!(lines[1].y > lines[0].y);                 // second sits lower
/// ```
#[must_use]
pub fn visual_line_metrics(layout: &impl TextLayout) -> Vec<VisualLineMetric> {
    layout.visual_lines()
}

/// R987 §5.22 §5.36 — the vertical span `(top_y, height)` of the **logical**
/// line the caret sits on: every visual row of a soft-wrapped line, in the
/// same layout-space frame as [`visual_line_metrics`]. A current-line
/// highlight reads this so a band covers the whole wrapped line (the VS Code
/// / `IntelliJ` behaviour) instead of only the caret's visual row.
///
/// `caret_y` is the caret rect's `y` ([`caret_rect_for_byte_offset`]); the
/// matching visual row is the one whose `[y, y + height)` contains it. From
/// there the span grows back to the row that
/// [`starts_logical_line`](VisualLineMetric::starts_logical_line) and forward
/// to the row before the next logical-line start, so soft-wrap (`Regular`)
/// and long-word (`Emergency`) continuation rows are included while a hard
/// `\n` ends the span. A non-wrapped logical line is a single row, so the
/// span equals that row's box (the pre-R987 behaviour).
///
/// Returns `None` when `metrics` is empty or no row contains `caret_y` (the
/// caller falls back to the caret's own row box).
#[must_use]
pub fn logical_line_span(metrics: &[VisualLineMetric], caret_y: f32) -> Option<(f32, f32)> {
    // R1031 §5.37 — the *last* row whose box contains `caret_y`, not the first.
    // Visual line boxes can overlap when the resolved font's line height leaves
    // the natural box taller than the advance step (e.g. DejaVu Sans Mono's
    // first row has a negative top, so its box extends down into the second
    // row's top). `caret_y` is the caret row's top, so the owning row is the
    // last one starting at-or-above it; a first-match `position` would pick the
    // overflowing previous row and collapse line N onto line N-1.
    let idx = metrics
        .iter()
        .rposition(|m| caret_y >= m.y && caret_y < m.y + m.height)?;
    let mut start = idx;
    while start > 0 && !metrics[start].starts_logical_line {
        start -= 1;
    }
    let mut end = idx;
    while end + 1 < metrics.len() && !metrics[end + 1].starts_logical_line {
        end += 1;
    }
    let top = metrics[start].y;
    let bottom = metrics[end].y + metrics[end].height;
    Some((top, bottom - top))
}

/// R1077 §5.36 §5.37 — the shaper-agnostic caret / hit-test / line-metric
/// surface a [`TextField`](pinion_core::widgets::text_field) reads, so the
/// editable-text geometry stops naming one concrete shaper. parley
/// ([`Layout`]) is the first implementor; the self-hosted
/// §5.37 engine is the second (a `SelfHostedLayout` wrapper in
/// `pinion-runtime`, the crate that sees both shapers). The free functions
/// in this module ([`caret_rect_for_byte_offset`] etc.) are thin generic
/// delegators over this trait, so every existing call site keeps compiling
/// while the implementor becomes a choice rather than a hard-wired type.
///
/// Crate home (R1078.1 audit): the trait lives here in `pinion-text` because the
/// caret functions it abstracts already did, and `pinion-text` is the lowest crate
/// both shapers can reach (`pinion-text-font`, the §5.37 engine, is a sibling that
/// does not dep this crate — hence the §5.37 impl is a newtype in `pinion-runtime`,
/// the crate that deps both). The §5.36 and §5.37 plan has parley superseded; when
/// the parley impl is eventually removed, `pinion-text` persists as the contract layer
/// (trait + [`CaretRect`] + [`VisualLineMetric`]) and only its parley `impl` is
/// dropped — so the trait does not need to relocate. Revisit only if `pinion-text`
/// is itself dissolved.
///
/// The named surface is exactly the directive's three capabilities — advance,
/// cluster boundary, visual-line metric: [`caret_rect`](Self::caret_rect)
/// (advance → where is byte *b*), [`byte_at_point`](Self::byte_at_point)
/// (cluster boundary → which byte at a point), and
/// [`visual_lines`](Self::visual_lines) (visual-line metric).
/// [`selection_rects`](Self::selection_rects) is the fourth required method
/// because each shaper resolves a multi-line selection from its own native
/// per-line byte range (parley's `Selection::geometry`, §5.37's
/// `ShapedLine::range`), not derivable from the three primitives without
/// leaking per-line extents.
///
/// [`line_move`](Self::line_move) and [`line_boundary`](Self::line_boundary)
/// are **derived** vertical-navigation defaults expressed purely in terms of
/// the required methods — a second implementor inherits them for free.
/// parley overrides both to keep its bespoke `Selection` navigation
/// byte-identical to the pre-R1077 helpers.
pub trait TextLayout {
    /// Caret rectangle (layout-space f32) for the caret insertion position
    /// at `byte_index`, `caret_width` wide. See
    /// [`caret_rect_for_byte_offset`].
    fn caret_rect(&self, byte_index: usize, caret_width: f32) -> CaretRect;

    /// UTF-8 byte offset of the nearest caret position to the layout-space
    /// point `(x, y)`. See [`byte_offset_for_point`].
    fn byte_at_point(&self, x: f32, y: f32) -> usize;

    /// Per-visual-line selection bands for the byte range `[start, end)`.
    /// See [`selection_rects_for_range`].
    fn selection_rects(&self, start: usize, end: usize) -> Vec<CaretRect>;

    /// Per-visual-line geometry, top-to-bottom. See [`visual_line_metrics`].
    fn visual_lines(&self) -> Vec<VisualLineMetric>;

    /// Byte offset after moving the caret `delta` visual lines from `byte`,
    /// holding the goal column `goal_x`. Derived default; see
    /// [`byte_offset_for_line_move`] for the contract. An overshoot past the
    /// first / last visual line snaps to that line's start / end (parley's
    /// document-boundary behaviour), reproduced here from the primitives.
    fn line_move(&self, byte: usize, delta: isize, goal_x: Option<f32>) -> (usize, f32) {
        let here = self.caret_rect(byte, 1.0);
        let gx = goal_x.unwrap_or(here.x);
        if delta == 0 {
            return (byte, gx);
        }
        let lines = self.visual_lines();
        if lines.is_empty() {
            return (byte, gx);
        }
        // The owning visual row is the *last* whose box contains the caret's
        // top, mirroring [`logical_line_span`] (overlapping line boxes).
        let cur = lines
            .iter()
            .rposition(|m| here.y >= m.y && here.y < m.y + m.height)
            .unwrap_or(0);
        #[allow(
            clippy::cast_possible_wrap,
            reason = "visual line counts fit isize in every realistic document"
        )]
        let target = cur as isize + delta;
        if target < 0 {
            let m = &lines[0];
            return (self.byte_at_point(0.0, m.y + m.height * 0.5), gx);
        }
        let last = lines.len() - 1;
        #[allow(
            clippy::cast_sign_loss,
            reason = "target >= 0 is checked on the line above"
        )]
        let target = target as usize;
        if target > last {
            let m = &lines[last];
            return (self.byte_at_point(f32::MAX, m.y + m.height * 0.5), gx);
        }
        let m = &lines[target];
        (self.byte_at_point(gx, m.y + m.height * 0.5), gx)
    }

    /// Byte offset of the start (`end = false`, `Home`) or end (`end = true`,
    /// `End`) of the visual line containing `byte`. Derived default; see
    /// [`byte_offset_for_line_boundary`]. Resolves by hit-testing the line's
    /// far-left / far-right edge at the caret's vertical midpoint.
    fn line_boundary(&self, byte: usize, end: bool) -> usize {
        let here = self.caret_rect(byte, 1.0);
        let y_mid = here.y + here.height * 0.5;
        let x = if end { f32::MAX } else { 0.0 };
        self.byte_at_point(x, y_mid)
    }
}

impl TextLayout for Layout {
    fn caret_rect(&self, byte_index: usize, caret_width: f32) -> CaretRect {
        let cursor = Cursor::from_byte_index(self, byte_index, Affinity::Downstream);
        // f64 -> f32 narrowing — parley's BoundingBox holds f64 corners for
        // sub-pixel precision, but the pinion paint pipeline rounds to u32 at
        // the §5.38 caret_rect seam anyway.
        let bbox = cursor.geometry(self, caret_width);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "layout-space coords fit f32 in every realistic UI viewport"
        )]
        let (x, y) = (bbox.x0 as f32, bbox.y0 as f32);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "layout-space dims fit f32 in every realistic UI viewport"
        )]
        let (width, height) = (bbox.width() as f32, bbox.height() as f32);
        CaretRect {
            x,
            y,
            width,
            height,
        }
    }

    fn byte_at_point(&self, x: f32, y: f32) -> usize {
        Cursor::from_point(self, x, y).index()
    }

    fn selection_rects(&self, start: usize, end: usize) -> Vec<CaretRect> {
        if start == end {
            return Vec::new();
        }
        let selection = Selection::new(
            Cursor::from_byte_index(self, start, Affinity::Downstream),
            Cursor::from_byte_index(self, end, Affinity::Downstream),
        );
        selection
            .geometry(self)
            .into_iter()
            .map(|(bbox, _line_ix)| {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "layout-space coords fit f32 in every realistic UI viewport"
                )]
                CaretRect::new(
                    bbox.x0 as f32,
                    bbox.y0 as f32,
                    bbox.width() as f32,
                    bbox.height() as f32,
                )
            })
            .collect()
    }

    fn visual_lines(&self) -> Vec<VisualLineMetric> {
        let mut out = Vec::new();
        // The first line always opens logical line 0; thereafter a line opens
        // a new logical line iff its predecessor was terminated by a hard
        // `\n` (parley `BreakReason::Explicit`).
        let mut starts_logical_line = true;
        for line in self.lines() {
            let metrics = line.metrics();
            out.push(VisualLineMetric {
                y: metrics.block_min_coord,
                height: metrics.block_max_coord - metrics.block_min_coord,
                starts_logical_line,
            });
            starts_logical_line = line.break_reason() == BreakReason::Explicit;
        }
        out
    }

    // parley overrides the derived navigation with its bespoke `Selection`
    // machinery so the geometry stays byte-identical to the pre-R1077 free
    // functions (goal-column drift correction + document-boundary clamp).
    fn line_move(&self, byte: usize, delta: isize, goal_x: Option<f32>) -> (usize, f32) {
        let here = self.caret_rect(byte, 1.0);
        let gx = goal_x.unwrap_or(here.x);
        if delta == 0 {
            return (byte, gx);
        }
        // Step 1: parley resolves the target line + document-boundary clamp.
        let moved = Selection::from_byte_index(self, byte, Affinity::Downstream)
            .move_lines(self, delta, false)
            .focus()
            .index();
        let there = self.caret_rect(moved, 1.0);
        // No band change ⇒ document boundary: keep parley's canonical result.
        if (there.y - here.y).abs() <= here.height * 0.5 {
            return (moved, gx);
        }
        // Step 2: interior move — override the drifted column with the goal,
        // hit-testing at the centre of the resolved line.
        let corrected = self.byte_at_point(gx, there.y + there.height * 0.5);
        (corrected, gx)
    }

    fn line_boundary(&self, byte: usize, end: bool) -> usize {
        let selection = Selection::from_byte_index(self, byte, Affinity::Downstream);
        let moved = if end {
            selection.line_end(self, false)
        } else {
            selection.line_start(self, false)
        };
        moved.focus().index()
    }
}

#[cfg(test)]
mod tests {
    //! R56.1.b.2 §5.36 §5.38 — `caret_rect_for_byte_offset` regression
    //! battery. Uses [`LayoutCache::layout`](crate::LayoutCache::layout)
    //! to build real parley shaped runs, then asserts on the
    //! geometry produced by the closed-form helper.

    use super::{
        CaretRect, VisualLineMetric, byte_offset_for_line_boundary, byte_offset_for_line_move,
        byte_offset_for_point, caret_rect_for_byte_offset, logical_line_span,
        selection_rects_for_range, visual_line_metrics,
    };
    use crate::LayoutCache;
    use pinion_core::style::TextStyle;

    /// Shape `text` at a canonical 16-px style for the test battery.
    /// Returns a freshly-built cache + the layout-key context the
    /// helper queries against (cache is borrowed by the caller so
    /// the `&Layout` lives across the helper call).
    fn shape(text: &'static str) -> LayoutCache {
        let mut cache = LayoutCache::new();
        let style = TextStyle::default();
        let _ = cache.layout(text, &style, None);
        cache
    }

    /// Re-fetch the layout for `text` from `cache`. Builds the same
    /// key the cache used on insert; the LRU hit returns the same
    /// `&Layout` the helper accepts.
    fn layout_for<'a>(cache: &'a mut LayoutCache, text: &str) -> &'a crate::Layout {
        let style = TextStyle::default();
        cache.layout(text, &style, None)
    }

    // R764 §5.36 §5.22 — selection_rects_for_range + line move.

    #[test]
    fn r764_selection_collapsed_range_has_no_rects() {
        let mut cache = shape("hello");
        let layout = layout_for(&mut cache, "hello");
        assert!(
            selection_rects_for_range(layout, 3, 3).is_empty(),
            "a collapsed (start == end) range yields no selection rects",
        );
    }

    #[test]
    fn r764_selection_single_line_is_one_band() {
        let mut cache = shape("hello");
        let layout = layout_for(&mut cache, "hello");
        let rects = selection_rects_for_range(layout, 1, 4);
        assert_eq!(rects.len(), 1, "a single-line selection is one band");
        assert!(rects[0].width > 0.0, "band has positive width");
        assert!(rects[0].height > 0.0, "band spans the line box");
    }

    #[test]
    fn r764_selection_spanning_newline_is_multiple_bands() {
        // "abc\nxyz" — byte 1 (line 0) .. byte 6 (line 1) spans the
        // hard line break, so parley yields one rect per visual line.
        let mut cache = shape("abc\nxyz");
        let layout = layout_for(&mut cache, "abc\nxyz");
        let rects = selection_rects_for_range(layout, 1, 6);
        assert!(
            rects.len() >= 2,
            "a selection across a newline yields a band per line (got {})",
            rects.len(),
        );
        // The bands sit on distinct lines (increasing y).
        assert!(
            rects[1].y > rects[0].y,
            "the second band is on a lower line (y {} > {})",
            rects[1].y,
            rects[0].y,
        );
    }

    #[test]
    fn r764_line_move_down_lands_on_next_line() {
        // "abc\nxyz": line 0 = bytes 0..3, '\n' = byte 3, line 1 = 4..7.
        let mut cache = shape("abc\nxyz");
        let layout = layout_for(&mut cache, "abc\nxyz");
        let (moved, _) = byte_offset_for_line_move(layout, 1, 1, None);
        assert!(
            (4..=7).contains(&moved),
            "ArrowDown from line 0 lands on line 1 (byte {moved} in 4..=7)",
        );
    }

    #[test]
    fn r764_line_move_up_lands_on_previous_line() {
        let mut cache = shape("abc\nxyz");
        let layout = layout_for(&mut cache, "abc\nxyz");
        let (moved, _) = byte_offset_for_line_move(layout, 5, -1, None);
        assert!(
            moved <= 3,
            "ArrowUp from line 1 lands on line 0 (byte {moved} <= 3)"
        );
    }

    #[test]
    fn r764_line_move_up_from_first_line_clamps_to_start() {
        let mut cache = shape("abc\nxyz");
        let layout = layout_for(&mut cache, "abc\nxyz");
        // Moving up from the first line clamps to the line start (byte 0).
        assert_eq!(byte_offset_for_line_move(layout, 2, -1, None).0, 0);
    }

    // R766 §5.22 — goal-column persistence + visual line boundaries.

    #[test]
    fn r766_goal_column_restores_after_crossing_short_line() {
        // line 0 = 8 wide, line 1 = 2 wide (short), line 2 = 8 wide.
        // Start at column 5 of line 0; ArrowDown into the short line
        // clamps the column to its end, ArrowDown again into the long
        // line must restore column 5 because the goal rode along.
        let text = "aaaaaaaa\nbb\ncccccccc";
        let mut cache = shape(text);
        let layout = layout_for(&mut cache, text);
        let start = 5;
        let orig_x = caret_rect_for_byte_offset(layout, start, 1.0).x;
        let (m1, gx) = byte_offset_for_line_move(layout, start, 1, None);
        let (m2, _) = byte_offset_for_line_move(layout, m1, 1, Some(gx));
        let final_x = caret_rect_for_byte_offset(layout, m2, 1.0).x;
        // R835 — assert the goal-column property by restored COLUMN index
        // (font-independent byte arithmetic), not exact pixels: a hard-coded
        // px tolerance is tuned to one machine's default font and breaks
        // under a different system font (CI). The goal must ride along and
        // restore the caret to ~column 5 of the long third line, NOT leave
        // it clamped at the short line's column 2.
        let line2_start = text.rfind('\n').map_or(0, |i| i + 1);
        let restored_col = m2 - line2_start;
        assert!(
            (4..=6).contains(&restored_col),
            "goal column rides along and restores to ~5 (got col {restored_col}, \
             not the short-line clamp at 2); orig_x {orig_x} final_x {final_x} gx {gx}",
        );
    }

    #[test]
    fn r766_without_goal_the_column_drifts_to_the_short_line() {
        // Same fixture; the contrast case — re-seeding the goal each
        // move (goal_x = None) drifts the column to the short line's
        // end, landing left of the original column on the long line.
        let text = "aaaaaaaa\nbb\ncccccccc";
        let mut cache = shape(text);
        let layout = layout_for(&mut cache, text);
        let start = 5;
        let orig_x = caret_rect_for_byte_offset(layout, start, 1.0).x;
        let (m1, _) = byte_offset_for_line_move(layout, start, 1, None);
        let (m2, _) = byte_offset_for_line_move(layout, m1, 1, None);
        let drift_x = caret_rect_for_byte_offset(layout, m2, 1.0).x;
        assert!(
            drift_x < orig_x - 1.0,
            "without a persisted goal the column drifts left to the short \
             line's end (orig {orig_x}, drift {drift_x})",
        );
    }

    #[test]
    fn r766_line_boundary_home_and_end_on_hard_line() {
        // "abc\nxyz": line 1 = bytes 4..7. Home from inside line 1
        // lands at its start (4), End at its end (7).
        let mut cache = shape("abc\nxyz");
        let layout = layout_for(&mut cache, "abc\nxyz");
        assert_eq!(
            byte_offset_for_line_boundary(layout, 6, false),
            4,
            "Home moves to the visual line start",
        );
        assert_eq!(
            byte_offset_for_line_boundary(layout, 5, true),
            7,
            "End moves to the visual line end",
        );
    }

    #[test]
    fn r766_line_boundary_is_visual_not_logical_under_soft_wrap() {
        // One hard line that soft-wraps into ≥2 visual rows. Home on a
        // caret in the second visual row must land *after* byte 0 (the
        // start of that visual row), proving the boundary is visual.
        let text = "the quick brown fox jumps over the lazy dog";
        let mut cache = LayoutCache::new();
        let style = TextStyle::default();
        let _ = cache.layout(text, &style, Some(90));
        let layout = cache.layout(text, &style, Some(90));
        // Pick a caret near the end (guaranteed to be on a later row).
        let caret = text.len() - 3;
        let home = byte_offset_for_line_boundary(layout, caret, false);
        let end = byte_offset_for_line_boundary(layout, caret, true);
        assert!(
            home > 0,
            "Home on a wrapped row lands at that row's start, not byte 0 (got {home})",
        );
        assert!(
            end <= text.len() && end > home,
            "End on the same row lands after Home and within the buffer \
             (home {home}, end {end}, len {})",
            text.len(),
        );
    }

    // ────────────────────────────────────────────────────────────────
    // R956 §5.36 §5.22 — visual_line_metrics (per-visual-line geometry)
    // ────────────────────────────────────────────────────────────────

    #[test]
    fn r956_visual_line_metrics_single_line_is_one_logical_start() {
        let mut cache = shape("hello");
        let layout = layout_for(&mut cache, "hello");
        let lines = visual_line_metrics(layout);
        assert_eq!(lines.len(), 1, "one visual line for unwrapped single line");
        assert!(
            lines[0].starts_logical_line,
            "the only line opens logical line 0"
        );
        assert!(lines[0].height > 0.0, "line box has positive height");
    }

    #[test]
    fn r956_visual_line_metrics_empty_layout_still_has_one_line() {
        // An empty document shows gutter "1": parley lays out one empty
        // line carrying the resolved style's line box.
        let mut cache = shape("");
        let layout = layout_for(&mut cache, "");
        let lines = visual_line_metrics(layout);
        assert_eq!(lines.len(), 1, "empty text is still one (empty) line");
        assert!(lines[0].starts_logical_line);
    }

    #[test]
    fn r956_visual_line_metrics_counts_hard_lines_in_order() {
        // "abc\nxyz" — two logical lines, each its own visual line, both
        // logical-line starts, the second below the first.
        let mut cache = shape("abc\nxyz");
        let layout = layout_for(&mut cache, "abc\nxyz");
        let lines = visual_line_metrics(layout);
        assert_eq!(lines.len(), 2, "two hard lines = two visual lines");
        assert!(lines[0].starts_logical_line && lines[1].starts_logical_line);
        assert!(
            lines[1].y > lines[0].y,
            "the second line sits lower (y {} > {})",
            lines[1].y,
            lines[0].y
        );
    }

    #[test]
    fn r956_visual_line_metrics_empty_line_between_is_its_own_logical_line() {
        // "a\n\nb" — three logical lines including the blank middle one,
        // so a gutter numbers 1 / 2 / 3 (the blank line keeps its number).
        let mut cache = shape("a\n\nb");
        let layout = layout_for(&mut cache, "a\n\nb");
        let lines = visual_line_metrics(layout);
        assert_eq!(
            lines.len(),
            3,
            "the blank middle line is its own visual line"
        );
        assert!(
            lines.iter().all(|l| l.starts_logical_line),
            "every hard-break line opens a logical line",
        );
    }

    #[test]
    fn r956_visual_line_metrics_soft_wrap_continues_one_logical_line() {
        // One hard line that soft-wraps into ≥2 visual rows: only the
        // first row is a logical-line start, so a gutter paints a single
        // number for the wrapped paragraph.
        let text = "the quick brown fox jumps over the lazy dog";
        let mut cache = LayoutCache::new();
        let style = pinion_core::style::TextStyle::default();
        let _ = cache.layout(text, &style, Some(90));
        let layout = cache.layout(text, &style, Some(90));
        let lines = visual_line_metrics(layout);
        assert!(
            lines.len() >= 2,
            "the long line wraps onto ≥2 rows (got {})",
            lines.len()
        );
        assert!(
            lines[0].starts_logical_line,
            "the first row opens the logical line"
        );
        assert!(
            lines[1..].iter().all(|l| !l.starts_logical_line),
            "soft-wrapped continuation rows do not open a new logical line",
        );
        let logical = lines.iter().filter(|l| l.starts_logical_line).count();
        assert_eq!(
            logical, 1,
            "the whole wrapped paragraph is exactly one logical line"
        );
    }

    #[test]
    fn r956_visual_line_metrics_y_aligns_with_caret_rect() {
        // The gutter aligns with the painted glyphs because the line
        // metric `y` shares the caret rect's layout-space frame: the y of
        // a line equals the y of a caret placed at that line's first byte.
        let text = "abc\nxyz";
        let mut cache = shape(text);
        let layout = layout_for(&mut cache, text);
        let lines = visual_line_metrics(layout);
        // line 0 first byte = 0, line 1 first byte = 4 ("abc\n").
        for (line_ix, byte) in [(0usize, 0usize), (1, 4)] {
            let caret = caret_rect_for_byte_offset(layout, byte, 1.0);
            assert!(
                (lines[line_ix].y - caret.y).abs() < 0.5,
                "line {line_ix} metric y {} matches caret y {} at byte {byte}",
                lines[line_ix].y,
                caret.y,
            );
        }
    }

    #[test]
    fn r956_visual_line_metric_is_copy_value() {
        let lines = {
            let mut cache = shape("x");
            visual_line_metrics(layout_for(&mut cache, "x"))
        };
        let first = lines[0];
        let copy = first; // Copy
        assert_eq!(first, copy);
    }

    // R987 — logical-line span (hand-built metrics: deterministic, no font
    // dependency). Each row is a 10px box; only a row that starts a logical
    // line carries `starts_logical_line`.
    fn rows(spec: &[bool]) -> Vec<VisualLineMetric> {
        spec.iter()
            .enumerate()
            .map(|(i, &starts)| VisualLineMetric {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "small test indices are exact as f32"
                )]
                y: (i as f32) * 10.0,
                height: 10.0,
                starts_logical_line: starts,
            })
            .collect()
    }

    #[test]
    fn r987_logical_line_span_wrapped_line_covers_all_rows() {
        // One logical line wrapped into three visual rows (only row 0 starts).
        let m = rows(&[true, false, false]);
        for caret_y in [5.0, 15.0, 25.0] {
            assert_eq!(
                logical_line_span(&m, caret_y),
                Some((0.0, 30.0)),
                "caret on any wrapped row spans the whole 0..30 logical line",
            );
        }
    }

    #[test]
    fn r987_logical_line_span_isolates_each_hard_line() {
        // Two logical lines, each a single visual row.
        let m = rows(&[true, true]);
        assert_eq!(
            logical_line_span(&m, 5.0),
            Some((0.0, 10.0)),
            "caret on line 0"
        );
        assert_eq!(
            logical_line_span(&m, 15.0),
            Some((10.0, 10.0)),
            "caret on line 1"
        );
    }

    #[test]
    fn r987_logical_line_span_groups_only_the_caret_line() {
        // Logical line 0 wraps rows 0..1; logical line 1 is row 2.
        let m = rows(&[true, false, true]);
        assert_eq!(
            logical_line_span(&m, 15.0),
            Some((0.0, 20.0)),
            "wrapped line 0 = rows 0..1"
        );
        assert_eq!(
            logical_line_span(&m, 25.0),
            Some((20.0, 10.0)),
            "line 1 = row 2 only"
        );
    }

    #[test]
    fn r987_logical_line_span_boundary_belongs_to_lower_row() {
        // A caret_y exactly at a row boundary lands on the lower row.
        let m = rows(&[true, true]);
        assert_eq!(
            logical_line_span(&m, 10.0),
            Some((10.0, 10.0)),
            "y=10 is row 1's top"
        );
    }

    #[test]
    fn r987_logical_line_span_empty_or_miss_is_none() {
        assert_eq!(logical_line_span(&[], 5.0), None, "no rows");
        assert_eq!(
            logical_line_span(&rows(&[true]), 50.0),
            None,
            "caret below all rows"
        );
    }

    #[test]
    fn r1031_logical_line_span_overlapping_boxes_resolve_to_lower_row() {
        // R1031 §5.37 — DejaVu-class metrics: the natural line box is taller
        // than the advance step, so consecutive row boxes OVERLAP (row 0 here
        // has a negative top and its box [-1, 21) extends into row 1's top at
        // y=20). Each row is a hard logical line. The caret-row top for line 1
        // (y=20) still falls inside row 0's box, so a first-match lookup would
        // collapse line 1 onto line 0 (the r962/r987 demo regression under
        // DejaVu Sans Mono). The span must isolate each line.
        let m = vec![
            VisualLineMetric {
                y: -1.0,
                height: 22.0,
                starts_logical_line: true,
            },
            VisualLineMetric {
                y: 20.0,
                height: 22.0,
                starts_logical_line: true,
            },
            VisualLineMetric {
                y: 41.0,
                height: 22.0,
                starts_logical_line: true,
            },
        ];
        assert_eq!(logical_line_span(&m, -1.0), Some((-1.0, 22.0)), "line 0");
        assert_eq!(
            logical_line_span(&m, 20.0),
            Some((20.0, 22.0)),
            "line 1 must resolve to its own row, not collapse onto overlapping line 0",
        );
        assert_eq!(logical_line_span(&m, 41.0), Some((41.0, 22.0)), "line 2");
    }

    #[test]
    fn r56_1_b_2_caret_at_byte_zero_of_empty_text_is_origin_anchored() {
        let mut cache = shape("");
        let layout = layout_for(&mut cache, "");
        let r = caret_rect_for_byte_offset(layout, 0, 1.0);
        // Empty layout — caret lands at the layout origin (x ≈ 0).
        // Parley puts the empty-line caret at the line's left edge.
        assert!(
            r.x.abs() < 0.5,
            "caret at byte 0 of empty text sits at x≈0 (got {})",
            r.x,
        );
        assert!(r.height > 0.0, "caret height reflects line box");
    }

    #[test]
    fn r56_1_b_2_caret_at_byte_zero_sits_at_layout_x_origin() {
        let mut cache = shape("hello");
        let layout = layout_for(&mut cache, "hello");
        let r = caret_rect_for_byte_offset(layout, 0, 1.0);
        // Byte 0 = before the first glyph = layout x origin.
        assert!(r.x.abs() < 0.5, "caret at byte 0 sits at x≈0 (got {})", r.x,);
    }

    #[test]
    fn r56_1_b_2_caret_at_end_byte_sits_past_last_glyph() {
        let mut cache = shape("hello");
        let layout = layout_for(&mut cache, "hello");
        let r = caret_rect_for_byte_offset(layout, 5, 1.0);
        // Byte 5 = after the last 'o' = positive x past the glyph run.
        assert!(
            r.x > 0.0,
            "caret at end byte advances past origin (got {})",
            r.x,
        );
    }

    #[test]
    fn r56_1_b_2_caret_advances_monotonically_across_bytes() {
        let mut cache = shape("abcde");
        let layout = layout_for(&mut cache, "abcde");
        // Each byte advances strictly: x0 < x1 < x2 < ... < x5.
        // ASCII letters guarantee monotonic per-byte advance — no
        // grapheme-cluster pitfalls on this test fixture.
        let mut prev_x: f32 = f32::NEG_INFINITY;
        for i in 0..=5usize {
            let r = caret_rect_for_byte_offset(layout, i, 1.0);
            assert!(
                r.x >= prev_x,
                "caret x must be non-decreasing across byte offsets (got {} ≤ {} at {})",
                r.x,
                prev_x,
                i,
            );
            prev_x = r.x;
        }
    }

    #[test]
    fn r56_1_b_2_caret_width_passes_through_unchanged() {
        let mut cache = shape("x");
        let layout = layout_for(&mut cache, "x");
        let one = caret_rect_for_byte_offset(layout, 0, 1.0);
        let two = caret_rect_for_byte_offset(layout, 0, 2.0);
        assert!((one.width - 1.0).abs() < 0.5, "width 1.0 round-trips");
        assert!((two.width - 2.0).abs() < 0.5, "width 2.0 round-trips");
    }

    #[test]
    fn r56_1_b_2_caret_height_matches_line_height() {
        let mut cache = shape("hello");
        let layout = layout_for(&mut cache, "hello");
        let r = caret_rect_for_byte_offset(layout, 0, 1.0);
        // Layout was shaped at TextStyle default (16 px font size) —
        // line height is the parley-derived metric, which is >= the
        // font size for any reasonable font (default leading ≈ 1.2x).
        assert!(
            r.height >= 16.0,
            "caret height covers full line box (got {})",
            r.height,
        );
    }

    #[test]
    fn r56_1_b_2_caret_y_is_finite() {
        // Parley's geometry box is anchored on the line's logical
        // bounding box — the y coordinate can be negative when the
        // line has negative leading or when the layout origin is at
        // the baseline. The invariant is that y is finite (not NaN
        // / Infinity) so paint-side clamping has a sane input.
        let mut cache = shape("hello");
        let layout = layout_for(&mut cache, "hello");
        let r = caret_rect_for_byte_offset(layout, 0, 1.0);
        assert!(r.y.is_finite(), "caret y must be finite (got {})", r.y);
    }

    #[test]
    fn r56_1_b_2_caret_rect_is_copy_value() {
        // CaretRect is Copy + PartialEq — pin the Copy / PartialEq
        // contract so downstream callers can keep the cursor rect
        // as a `Cell<CaretRect>` if needed.
        let r = CaretRect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        let copy = r;
        assert_eq!(r, copy);
    }

    #[test]
    fn r56_1_b_2_caret_at_oversized_offset_clamps_to_end() {
        // Parley's Cursor::from_byte_index contract: out-of-range
        // indices clamp to the closest valid boundary. For "hi"
        // (len 2), index 999 lands at byte 2 (end).
        let mut cache = shape("hi");
        let layout = layout_for(&mut cache, "hi");
        let end = caret_rect_for_byte_offset(layout, 2, 1.0);
        let beyond = caret_rect_for_byte_offset(layout, 999, 1.0);
        assert!(
            (end.x - beyond.x).abs() < 0.5,
            "out-of-range offset clamps to end (end.x={}, beyond.x={})",
            end.x,
            beyond.x,
        );
    }

    #[test]
    fn r56_1_b_2_caret_rect_across_multibyte_char_does_not_panic() {
        // "안녕" is 6 UTF-8 bytes (2 Korean syllables × 3 bytes).
        // Char boundaries at 0, 3, 6. Helper must not panic at any
        // of them (TextEditState clamp_to_char_boundary already
        // gates against non-boundary offsets in production paths).
        let mut cache = shape("\u{C548}\u{B155}");
        let layout = layout_for(&mut cache, "\u{C548}\u{B155}");
        let _ = caret_rect_for_byte_offset(layout, 0, 1.0);
        let _ = caret_rect_for_byte_offset(layout, 3, 1.0);
        let _ = caret_rect_for_byte_offset(layout, 6, 1.0);
    }

    #[test]
    fn r56_2_c_caret_rect_new_constructs_with_four_fields() {
        // R56.2.c — public constructor for the `#[non_exhaustive]`
        // `CaretRect`. Downstream crates (application-side
        // `WidgetView::ime_caret_rect` impls in particular) need to
        // synthesise window-coord `CaretRect`s by hand; the
        // non-exhaustive attribute would otherwise reject the
        // struct-literal shape from outside the crate.
        let r = CaretRect::new(12.5, 8.0, 2.0, 18.0);
        assert!((r.x - 12.5).abs() < f32::EPSILON);
        assert!((r.y - 8.0).abs() < f32::EPSILON);
        assert!((r.width - 2.0).abs() < f32::EPSILON);
        assert!((r.height - 18.0).abs() < f32::EPSILON);
    }

    #[test]
    fn r56_2_c_caret_rect_new_is_const_eligible() {
        // R56.2.c — constructor is `const fn` so callers can build
        // compile-time-known CaretRects (e.g. test fixtures, default
        // sentinel values). Verifies the const-eligibility by
        // assigning to a `const` binding.
        const RECT: CaretRect = CaretRect::new(0.0, 0.0, 1.0, 1.0);
        assert!((RECT.x).abs() < f32::EPSILON);
        assert!((RECT.width - 1.0).abs() < f32::EPSILON);
    }

    // ────────────────────────────────────────────────────────────────
    // R762 §5.36 §5.38 — byte_offset_for_point (pixel → byte hit-test)
    // ────────────────────────────────────────────────────────────────

    /// Mid-line y for a layout's first line — every single-line fixture
    /// here shares one line, so the caret rect at byte 0 gives a y that
    /// sits inside it.
    fn mid_y(layout: &crate::Layout) -> f32 {
        let r = caret_rect_for_byte_offset(layout, 0, 1.0);
        r.y + r.height * 0.5
    }

    #[test]
    fn r762_hit_test_point_before_origin_clamps_to_zero() {
        let mut cache = shape("hello");
        let layout = layout_for(&mut cache, "hello");
        let y = mid_y(layout);
        assert_eq!(byte_offset_for_point(layout, -50.0, y), 0);
    }

    #[test]
    fn r762_hit_test_point_far_right_clamps_to_text_len() {
        let mut cache = shape("hello");
        let layout = layout_for(&mut cache, "hello");
        let y = mid_y(layout);
        assert_eq!(byte_offset_for_point(layout, 100_000.0, y), "hello".len());
    }

    #[test]
    fn r762_hit_test_round_trips_with_caret_rect_ascii() {
        // The inverse contract: hit-testing at the caret x of byte `i`
        // returns `i`. ASCII guarantees one byte per cluster + monotone
        // advance, so every boundary round-trips exactly.
        let mut cache = shape("abcde");
        let layout = layout_for(&mut cache, "abcde");
        let y = mid_y(layout);
        for i in 0..="abcde".len() {
            let caret = caret_rect_for_byte_offset(layout, i, 1.0);
            let hit = byte_offset_for_point(layout, caret.x, y);
            assert_eq!(hit, i, "hit-test at caret x of byte {i} must return {i}");
        }
    }

    #[test]
    fn r762_hit_test_midglyph_resolves_to_nearest_boundary() {
        // A point in the right half of a glyph resolves to the trailing
        // boundary (downstream affinity); the left half to the leading
        // boundary. We assert the point just past a glyph's start sits
        // on a valid boundary <= the next one.
        let mut cache = shape("abcde");
        let layout = layout_for(&mut cache, "abcde");
        let y = mid_y(layout);
        let a = caret_rect_for_byte_offset(layout, 1, 1.0); // after 'a'
        let near_start = byte_offset_for_point(layout, a.x - a.height * 0.1, y);
        assert!(
            near_start <= 1,
            "left-of-boundary hit stays <= 1 (got {near_start})"
        );
    }

    #[test]
    fn r762_hit_test_multibyte_lands_on_char_boundary() {
        // "안녕" = 6 bytes, char boundaries at 0/3/6. Hit-testing at the
        // caret x of byte 3 returns 3 (a boundary), never a mid-codepoint
        // index.
        let mut cache = shape("\u{C548}\u{B155}");
        let layout = layout_for(&mut cache, "\u{C548}\u{B155}");
        let y = mid_y(layout);
        let caret = caret_rect_for_byte_offset(layout, 3, 1.0);
        let hit = byte_offset_for_point(layout, caret.x, y);
        assert!(
            "\u{C548}\u{B155}".is_char_boundary(hit),
            "hit offset {hit} must be a char boundary",
        );
        assert_eq!(hit, 3, "hit at the mid caret x returns the 3-byte boundary");
    }

    #[test]
    fn r762_hit_test_empty_text_returns_zero() {
        let mut cache = shape("");
        let layout = layout_for(&mut cache, "");
        let y = mid_y(layout);
        assert_eq!(byte_offset_for_point(layout, 0.0, y), 0);
        assert_eq!(byte_offset_for_point(layout, 50.0, y), 0);
    }
}
