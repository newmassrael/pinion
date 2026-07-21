// R1008 §5.16 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-hex-dump` — R1400 / R1401 / R1402 §5.41 — a **hex/byte-dump viewer
//! over [`Scene::TextGrid`]** with a byte-range brush that highlights across the
//! hex column AND the ascii gutter at once (R1400), a click-a-byte cursor that
//! inspects a single byte (R1401), and a **press-drag that selects a byte
//! range** (R1402, reverse-video, with a `selection_hex` readout).
//!
//! A fixed byte buffer renders as the classic three-region dump — an offset
//! column, the 16 bytes as `8 + 8` hex pairs, and an ascii gutter (each
//! non-printable byte shown as `.`) — laid out cell-for-cell in one
//! [`Scene::TextGrid`]. This is the second real data consumer of the cell
//! projection (after `hello-grid-pointer`), and the fifth consumer of the
//! [`Brush`] substrate (R1394).
//!
//! ## The novel thing: a two-region linked highlight
//!
//! A byte in a hex dump appears in TWO disjoint cell regions of the same
//! grid — its two hex digits and its one ascii character. Selecting a
//! *field* (a contiguous byte range) must therefore highlight both regions
//! together, or the selection reads as two unrelated smears. R1400 wires a
//! primary [`Brush`] over the byte offsets `[0, N]`: dragging its overview
//! strip selects a contiguous byte window `[lo, hi)`, and the view paints a
//! distinct background on every selected byte's TWO hex cells and its ONE
//! ascii cell. The rest of the dump stays as context (default background).
//! This is the cross-filter matrix (R1384-R1398) extended to a NEW
//! geometry: a hex grid whose two views of each datum stay in lockstep —
//! exactly the field selection a protocol inspector (Wireshark, a `dlt`
//! trace) or a hex editor draws when you click a packet field.
//!
//! ## Two orthogonal selections: a field brush and a drag selection
//!
//! The **brush** (a sibling `RangeSliderExternal`, R1400) selects a byte
//! *field* via the overview strip — the two-region background highlight. The
//! **selection** (R1401 → R1402) selects bytes by direct manipulation: a click
//! resolves the byte via [`byte_at_cell`] (the inverse of [`hex_col`] /
//! [`ascii_col`], reusing the R1008 [`CellMetric::px_to_cell`] pointer→cell
//! substrate as its second consumer), and a **press-drag** extends the range —
//! a press latches an anchor, each move extends the focus. The `[start, end)`
//! bytes reverse-video and the focus byte rings with a [`GridCursor`] (R975); a
//! plain click is the collapsed single-byte case. The [`HexDumpOracle`] (the
//! root external) owns the selection: the router drives `pointer_move` +
//! `invoke("send", "PointerDown"/"PointerUp")`, and an AI client drives it with
//! no pixel via `scene/intervene /external/cursor_byte` (a collapsed byte) or
//! `scene/invoke /external/select_range "start,end"` (a range), reading it back
//! at `cursor_byte` / `selection_start` / `selection_end` / `selection_hex`. The
//! brush window is `scene/intervene /hex_brush/external/{low,high}`.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! The highlight is cell *background*, so `scene/snapshot` reports it in
//! `grid_rows`: a selected byte's hex + ascii runs carry an `rgb` `bg`
//! while context cells stay `default`. So a client verifies the linked
//! highlight — the SAME byte set lit in both regions — without a pixel.
//! The primary [`HexDumpOracle`] exposes the layout so the mapping is not
//! guessed: `query("/external/byte_count")` / `bytes_per_row` / `row_count`
//! / `total_cols`, and the invoke oracles `hex_cell` / `ascii_cell` map a
//! byte index to its `"col,row"`, `byte_window` maps a `"low,high"` brush
//! fraction pair to its `"lo,hi"` byte range (the same SSOT the view
//! paints from). See `tools/demos/r1400_hex_dump.py`.

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{Brush, BrushStripColors};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, TextGridNode, TextNode};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::range_slider::RangeSliderExternal;
use pinion_core::{
    CellAttrs, CellMetric, CursorShape, Frame, GridBuffer, GridCursor, Scene, TermCell, TermColor,
    WidgetCore,
};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloHexDumpRenderer, HelloHexDumpRendererError);

const WIN_W: u32 = 660;
const WIN_H: u32 = 240;
const THEME_TAG: &str = "app";

/// The grid's paint tag **and** the primary [`HexDumpOracle`]'s registration
/// tag — the oracle is addressed over RPC as `/external/<field>`, and the
/// tag also lets `scene/snapshot` find the grid by name.
const GRID_TAG: &str = "hex_dump";
/// The sibling brush tag — the overview strip's hit-test root and the range
/// external's registration tag. Over RPC the window is
/// `scene/intervene /hex_brush/external/{low,high}`.
const BRUSH_TAG: &str = "hex_brush";

const TITLE_FONT_PX: u32 = 16;
const STATUS_FONT_PX: u32 = 12;

// --- The byte buffer -------------------------------------------------------

/// The dumped buffer's length — 128 bytes = 8 rows at 16 bytes/row.
const SAMPLE_LEN: usize = 128;

/// Build the fixed sample buffer: a 16-byte header (a `PIN\x01` magic, a
/// big-endian length field, a `payload:` tag) followed by a readable ascii
/// message, with a few control / high bytes so the gutter's non-printable
/// `.` substitution is exercised. Padded to [`SAMPLE_LEN`] with `NUL`.
fn sample() -> Vec<u8> {
    let mut v = Vec::with_capacity(SAMPLE_LEN);
    // Header row (16 bytes): magic, a length field, and a field tag.
    v.extend_from_slice(&[0x50, 0x49, 0x4e, 0x01]); // "PIN" + 0x01 (non-printable)
    v.extend_from_slice(&[0x00, 0x00, 0x00, 0x2c]); // length = 44, big-endian
    v.extend_from_slice(b"payload:");
    // A readable message so the ascii gutter is legible; the '|' and ':'
    // are printable, the trailing '\n' + 0xff are not (gutter shows '.').
    v.extend_from_slice(b"A Scene::TextGrid renders bytes as offset|hex|ascii. Brush to select.\n");
    v.push(0xff);
    v.resize(SAMPLE_LEN, 0x00);
    v
}

// --- The dump layout (columns) ---------------------------------------------

/// Bytes per dump row.
const BYTES_PER_ROW: usize = 16;
/// The offset column is `{:08x}` — 8 hex digits.
const OFFSET_COLS: usize = 8;
/// First hex-pair column: the offset (8) plus a 2-space gutter.
const HEX_START: usize = OFFSET_COLS + 2;
/// The left ascii-gutter bar `|`.
const ASCII_BAR_L: usize = 60;
/// The first ascii-gutter character column.
const ASCII_START: usize = 61;
/// The right ascii-gutter bar `|`.
const ASCII_BAR_R: usize = 77;
/// Total grid columns (one past the right bar).
const TOTAL_COLS: usize = 78;

/// Grid rows for [`SAMPLE_LEN`].
const ROWS: usize = SAMPLE_LEN.div_ceil(BYTES_PER_ROW);

/// The grid's absolute placement + pixel extent, at the `CellMetric::DEFAULT`
/// 8x16 cell — sized so `rect.w / 8` derives exactly [`TOTAL_COLS`] (`78 * 8`)
/// and `rect.h / 16` exactly [`ROWS`] (`8 * 16`). The
/// `grid_pixels_match_the_cell_layout` test locks these to the column / row
/// counts.
const GRID_POS: (u32, u32) = (16, 40);
const GRID_W: u32 = 624;
const GRID_H: u32 = 128;

/// The first (high-nibble) hex column of the `j`-th byte in a row. Each byte
/// is `2` digits + a trailing space (`3` columns); an extra space splits the
/// two 8-byte groups, so bytes `8..16` shift right by one.
const fn hex_col(j: usize) -> usize {
    HEX_START + j * 3 + if j >= 8 { 1 } else { 0 }
}

/// The ascii-gutter column of the `j`-th byte in a row.
const fn ascii_col(j: usize) -> usize {
    ASCII_START + j
}

/// The byte a `(col, row)` cell addresses — the inverse of [`hex_col`] /
/// [`ascii_col`]. A byte owns three cells (its two hex digits + its ascii
/// glyph); every other column (the offset field, the group gaps, the gutter
/// bars, the padding) belongs to no byte. `None` off the buffer or on a
/// non-byte cell — the click-to-select hit-test the pointer resolves.
fn byte_at_cell(col: usize, row: usize) -> Option<usize> {
    if row >= ROWS {
        return None;
    }
    (0..BYTES_PER_ROW)
        .find_map(|j| {
            let hc = hex_col(j);
            (col == hc || col == hc + 1 || col == ascii_col(j)).then(|| row * BYTES_PER_ROW + j)
        })
        .filter(|&idx| idx < SAMPLE_LEN)
}

// --- The byte-range brush (Brush substrate, R1394) -------------------------

/// The boot window highlights the `payload:` tag + the message start (bytes
/// `8..24`), so the linked hex+ascii highlight is visible before any drag.
const BOOT_BYTE_LO: usize = 8;
const BOOT_BYTE_HI: usize = 24;

/// The boot brush window as fractions of the buffer.
#[allow(
    clippy::cast_precision_loss,
    reason = "small byte counts -> f32 are exact"
)]
fn boot_window() -> (f32, f32) {
    (
        BOOT_BYTE_LO as f32 / SAMPLE_LEN as f32,
        BOOT_BYTE_HI as f32 / SAMPLE_LEN as f32,
    )
}

/// The byte-offset brush — the [`Brush`] substrate (R1394) over the extent
/// `[0, N]`, whose FIFTH consumer this is (a call to the lifted SSOT).
#[allow(
    clippy::cast_precision_loss,
    reason = "SAMPLE_LEN (128) -> f64 is exact"
)]
fn brush() -> Brush {
    Brush::new(BRUSH_TAG, (0.0, SAMPLE_LEN as f64))
}

/// The brush as the **primary** external (a [`RangeSliderExternal`] under
/// [`BRUSH_TAG`]), seeded to the [`boot_window`] field. The fn
/// [`WidgetCore::create_extra_externals`] points at.
fn brush_extras() -> Vec<ExtraExternal> {
    let (low, high) = boot_window();
    vec![ExtraExternal::new(
        BRUSH_TAG,
        Box::new(RangeSliderExternal::with_values(low, high)),
    )]
}

/// Read the brush window `(low, high)` fractions from the sibling external
/// ([`Brush::read`]); a missing external falls back to the full span.
fn read_brush(scene: &Scene) -> (f32, f32) {
    brush().read(scene)
}

/// Read the drag [`Selection`] from the primary [`HexDumpOracle`] (tagged
/// [`GRID_TAG`] in the state scene); `None` when nothing is selected.
fn read_selection(scene: &Scene) -> Option<Selection> {
    let intro = scene
        .find_external_with_tag(GRID_TAG)?
        .handle
        .introspect()?;
    let field = |name: &str| -> Option<usize> {
        match intro.query(name)? {
            IntrospectValue::Int(i) => usize::try_from(i).ok(),
            _ => None,
        }
    };
    Some(Selection {
        start: field("selection_start")?,
        end: field("selection_end")?,
        focus: field("cursor_byte")?,
    })
}

/// Map the brush fractions onto the selected byte range `[lo, hi)` — the SSOT
/// the view paints from and [`HexDumpOracle`]'s `byte_window` oracle reports.
/// Rounds each domain edge to the nearest byte boundary and clamps to
/// `[0, N]` with `lo <= hi`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "domain() clamps to [0, N]; round then clamp keeps it in usize range"
)]
fn selected_bytes(low: f32, high: f32) -> (usize, usize) {
    let (x_lo, x_hi) = brush().domain(low, high);
    let lo = (x_lo.round() as usize).min(SAMPLE_LEN);
    let hi = (x_hi.round() as usize).min(SAMPLE_LEN);
    (lo.min(hi), hi.max(lo))
}

// --- Cell colours ----------------------------------------------------------

/// The three resolved cell colours the dump paints with — resolved from the
/// theme in `view`, or hand-set in tests.
#[derive(Debug, Clone, Copy)]
struct CellColors {
    /// The offset column + gutter bars + non-printable `.` (a dim line-number
    /// tint).
    muted: TermColor,
    /// A selected cell's background (the accent).
    highlight: TermColor,
    /// A selected cell's foreground (on the accent).
    on_highlight: TermColor,
}

/// A blank (space) cell in the default fg/bg — the between-field padding.
fn blank_cell() -> TermCell {
    TermCell::new(" ", TermColor::Default, TermColor::Default)
}

/// The ascii glyph + its default (non-highlight) foreground for a byte: the
/// printable char itself in the default fg, or a `.` in the muted tint.
fn ascii_glyph(byte: u8, colors: &CellColors) -> (String, TermColor) {
    if (0x20..0x7f).contains(&byte) {
        ((byte as char).to_string(), TermColor::Default)
    } else {
        (".".to_owned(), colors.muted)
    }
}

/// One hex digit (`0-9a-f`) as a `String`.
fn hex_digit(nibble: u8) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    (HEX[(nibble & 0x0f) as usize] as char).to_string()
}

/// Build the hex-dump [`GridBuffer`]. Two orthogonal visual channels ride each
/// byte's three cells (two hex digits + its ascii glyph): the `brush_sel`
/// (`lo..hi`) fills the background (R1400 field highlight), and `reverse_sel`
/// (`start..end`) reverse-videos the cells (R1402 drag selection). A byte can
/// be in both — the accent background reversed — since the two selections are
/// independent gestures (the minimap field vs the direct byte selection).
fn hex_dump_buffer(
    bytes: &[u8],
    brush_sel: core::ops::Range<usize>,
    reverse_sel: Option<(usize, usize)>,
    colors: &CellColors,
) -> GridBuffer {
    let mut buf = GridBuffer::new(
        u16::try_from(TOTAL_COLS).unwrap_or(0),
        u16::try_from(ROWS).unwrap_or(0),
    );
    for r in 0..ROWS {
        let base = r * BYTES_PER_ROW;
        let mut cells = vec![blank_cell(); TOTAL_COLS];

        // Offset column: the row's start offset as 8 hex digits.
        let offset = format!("{base:08x}");
        for (i, ch) in offset.chars().take(OFFSET_COLS).enumerate() {
            cells[i] = TermCell::new(ch.to_string(), colors.muted, TermColor::Default);
        }

        // The 16 bytes: hex pair + ascii glyph, each carrying the brush fill
        // (background) and/or the drag-selection (reverse video).
        for j in 0..BYTES_PER_ROW {
            let idx = base + j;
            let Some(&byte) = bytes.get(idx) else { break };
            let brushed = brush_sel.contains(&idx);
            let reversed = reverse_sel.is_some_and(|(s, e)| (s..e).contains(&idx));
            let attrs = CellAttrs::empty().with_reverse(reversed);
            let (hex_fg, hl_bg) = if brushed {
                (colors.on_highlight, colors.highlight)
            } else {
                (TermColor::Default, TermColor::Default)
            };

            let hc = hex_col(j);
            cells[hc] = TermCell::new(hex_digit(byte >> 4), hex_fg, hl_bg).with_attrs(attrs);
            cells[hc + 1] = TermCell::new(hex_digit(byte), hex_fg, hl_bg).with_attrs(attrs);

            let (glyph, glyph_fg) = ascii_glyph(byte, colors);
            let (ink, fill) = if brushed {
                (colors.on_highlight, colors.highlight)
            } else {
                (glyph_fg, TermColor::Default)
            };
            cells[ascii_col(j)] = TermCell::new(glyph, ink, fill).with_attrs(attrs);
        }

        // The ascii-gutter bars.
        cells[ASCII_BAR_L] = TermCell::new("|", colors.muted, TermColor::Default);
        cells[ASCII_BAR_R] = TermCell::new("|", colors.muted, TermColor::Default);

        buf = buf.with_row(u16::try_from(r).unwrap_or(0), cells);
    }
    buf
}

/// The overview strip's track — the byte-offset minimap, aligned under the
/// grid and spanning its width.
fn strip_track() -> Rect {
    Rect::new(GRID_POS.0, GRID_POS.1 + GRID_H + 8, GRID_W, 14)
}

/// The drag selection: a byte range `[start, end)` whose live end (the byte the
/// cursor rings) is `focus`. A single click is a collapsed selection
/// (`start + 1 == end`, `focus == start`) — the R1401 single-byte inspect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Selection {
    start: usize,
    end: usize,
    focus: usize,
}

/// The selected bytes as a lowercase hex string (`"50494e01"`) — what a hex
/// editor would copy. Empty for an out-of-range range.
fn selection_hex(bytes: &[u8], start: usize, end: usize) -> String {
    use std::fmt::Write;
    let mut hex = String::new();
    for b in bytes.get(start..end).unwrap_or(&[]) {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

/// view-fn (§6.3): pure sync mapping. `(low, high)` is the brushed byte-offset
/// window (the R1400 field background); `sel` is the drag selection (R1402) —
/// its `[start, end)` bytes reverse-video and its `focus` byte rings with the
/// [`GridCursor`].
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(low: f32, high: f32, sel: Option<Selection>, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);
    let accent = theme.resolve(ColorRole::Accent);

    let colors = CellColors {
        muted: TermColor::Rgb(on_surface_muted),
        highlight: TermColor::Rgb(accent),
        on_highlight: TermColor::Rgb(theme.resolve(ColorRole::OnAccent)),
    };
    let bytes = sample();
    let (lo, hi) = selected_bytes(low, high);

    // The brush field background (R1400) + the drag selection reverse-video
    // (R1402), plus a block GridCursor on the selection's focus byte.
    let reverse_sel = sel.map(|s| (s.start, s.end));
    let mut cells = hex_dump_buffer(&bytes, lo..hi, reverse_sel, &colors);
    let cursor = sel.map_or_else(GridCursor::default, |s| {
        GridCursor::new(
            u16::try_from(hex_col(s.focus % BYTES_PER_ROW)).unwrap_or(0),
            u16::try_from(s.focus / BYTES_PER_ROW).unwrap_or(0),
            CursorShape::Block,
            true,
        )
    });
    cells = cells.with_cursor(cursor);

    let grid = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(GRID_TAG)
            .with_cells(cells)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(GRID_POS.0, GRID_POS.1)
                    .with_size(Size::px(GRID_W, GRID_H))
                    // Focusable so a click / drag routes to the oracle's hit-test.
                    .with_focusable(true),
            ),
    );

    let title = Scene::Text(
        TextNode::styled(
            "Hex dump — brush a field, drag across bytes to select them",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(16, 12)),
    );

    let strip = brush().strip(
        strip_track(),
        low,
        high,
        BrushStripColors {
            track_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
            accent,
        },
        "Hex dump byte-range brush",
    );

    let sel_note = sel.map_or_else(
        || "  |  selection: none (drag across bytes)".to_owned(),
        |s| {
            format!(
                "  |  selection 0x{:x}..0x{:x} ({} bytes) = {}",
                s.start,
                s.end,
                s.end - s.start,
                selection_hex(&bytes, s.start, s.end),
            )
        },
    );
    let status = Scene::Text(
        TextNode::styled(
            format!(
                "field 0x{lo:x}..0x{hi:x} ({} of {SAMPLE_LEN} bytes){sel_note}",
                hi - lo
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(on_surface_muted),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(16, WIN_H - 22)),
    );

    Scene::Container(
        ContainerNode::new(vec![grid, strip, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

// --- The layout oracle (primary external) ----------------------------------

/// Parse a byte-index argument (accepts a JSON string or int).
fn parse_index(v: &IntrospectValue) -> Option<usize> {
    match v {
        IntrospectValue::Text(s) => s.trim().parse().ok(),
        IntrospectValue::Int(i) => usize::try_from(*i).ok(),
        _ => None,
    }
}

/// Parse a `"low,high"` fraction pair (the `byte_window` argument wire form).
fn parse_pair(s: &str) -> Option<(f32, f32)> {
    let (a, b) = s.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Parse a `"col,row"` cell pair (the `byte_at_cell` argument wire form).
fn parse_cell(s: &str) -> Option<(usize, usize)> {
    let (a, b) = s.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// A byte index (the `hex_cell` / `ascii_cell` argument) -> its `"col,row"` in
/// the hex column or the ascii gutter (via `col_of`), or `"none"` when the
/// index is past the buffer.
fn cell_for(
    args: &IntrospectValue,
    col_of: fn(usize) -> usize,
) -> Result<IntrospectValue, InvokeError> {
    let b = parse_index(args).ok_or(InvokeError::TypeMismatch)?;
    if b >= SAMPLE_LEN {
        return Ok(IntrospectValue::Text("none".to_owned()));
    }
    let col = col_of(b % BYTES_PER_ROW);
    let row = b / BYTES_PER_ROW;
    Ok(IntrospectValue::Text(format!("{col},{row}")))
}

/// The dump's layout + the drag selection, as the interactive primary external.
/// The read half is an introspectable oracle so an AI client reads the
/// byte↔cell mapping directly (`hex_cell` / `ascii_cell` / `byte_at_cell` /
/// `byte_window`) rather than reverse-engineering the geometry from a snapshot.
/// The write half is the **drag selection**: a press latches an anchor byte,
/// each drag move extends the focus, and the range `[start, end)` reverse-videos
/// (R1402) — the hex-editor's select-bytes gesture, of which a plain click is
/// the collapsed single-byte case (the R1401 inspect). The selection also
/// drives via `intervene cursor_byte` (a collapsed byte) / `invoke select_range`
/// (a range), so an AI client selects with no pixel.
#[derive(Debug, Clone)]
struct HexDumpOracle {
    /// The dumped bytes (so `cursor_value` / `selection_hex` report them).
    bytes: Vec<u8>,
    /// The live selection `(anchor, focus)` bytes, or `None` when nothing is
    /// selected. The range is `[min, max]`; `focus` is the drag's live end.
    selection: Option<(usize, usize)>,
    /// Set by a `PointerDown` send: the next [`pointer_move`](Self::pointer_move)
    /// latches a fresh anchor (a new gesture) rather than extending the focus.
    pending_anchor: bool,
}

impl HexDumpOracle {
    fn new() -> Self {
        Self {
            bytes: sample(),
            selection: None,
            pending_anchor: false,
        }
    }

    /// The selection as an inclusive-start / exclusive-end byte range
    /// `(start, end)`, normalising the `(anchor, focus)` order.
    fn range(&self) -> Option<(usize, usize)> {
        self.selection.map(|(a, f)| (a.min(f), a.max(f) + 1))
    }

    /// The selection's focus (the drag's live end — the byte the cursor rings).
    fn focus(&self) -> Option<usize> {
        self.selection.map(|(_, f)| f)
    }

    /// Set a collapsed selection at byte `b` (a click / an `intervene`).
    fn select_byte(&mut self, b: usize) {
        self.selection = Some((b, b));
        self.pending_anchor = false;
    }
}

impl External for HexDumpOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Take pointer capture so a press-drag keeps forwarding
    /// [`pointer_move`](Self::pointer_move) while held (the select-bytes arc),
    /// and the router delivers `PointerDown` / `PointerUp` via `invoke("send")`.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// The router delivers a `[0, 1]` fraction over the grid's rect on the press
    /// (click-to-position) and each move while held. Reconstruct grid pixels,
    /// floor to the cell, and resolve the byte: a fresh press (`pending_anchor`,
    /// or no prior selection) latches an anchor; a continued drag extends the
    /// focus. A non-byte cell deselects a fresh press but is ignored mid-drag
    /// (dragging over the gutter never collapses the selection).
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let x_px = CellMetric::frac_to_px(x_rel, GRID_W);
        let y_px = CellMetric::frac_to_px(y_rel, GRID_H);
        let (col, row) = CellMetric::DEFAULT.px_to_cell(x_px, y_px);
        let fresh = self.pending_anchor || self.selection.is_none();
        match byte_at_cell(usize::from(col), usize::from(row)) {
            Some(b) if fresh => self.select_byte(b),
            Some(b) => {
                let anchor = self.selection.map_or(b, |(a, _)| a);
                self.selection = Some((anchor, b));
            }
            None if fresh => {
                self.selection = None;
                self.pending_anchor = false;
            }
            None => {}
        }
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for HexDumpOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("byte_count", "int"),
                    SchemaField::new("bytes_per_row", "int"),
                    SchemaField::new("row_count", "int"),
                    SchemaField::new("total_cols", "int"),
                    // The selection's focus byte (writable = a collapsed
                    // select), its hex value, and the hex cell it rings.
                    SchemaField::new("cursor_byte", "int"),
                    SchemaField::new("cursor_value", "string"),
                    SchemaField::new("cursor_cell", "string"),
                    // The drag selection: its byte range, length, and hex bytes.
                    SchemaField::new("selection_start", "int"),
                    SchemaField::new("selection_end", "int"),
                    SchemaField::new("selection_len", "int"),
                    SchemaField::new("selection_hex", "string"),
                    SchemaField::new("hex_cell", "string"),
                    SchemaField::new("ascii_cell", "string"),
                    SchemaField::new("byte_at_cell", "string"),
                    SchemaField::new("select_range", "string"),
                    SchemaField::new("byte_window", "string"),
                    // The router's pointer press / release symbolic events.
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let int = |n: usize| IntrospectValue::Int(i64::try_from(n).unwrap_or(0));
        let range = self.range();
        match path {
            "byte_count" => Some(int(SAMPLE_LEN)),
            "bytes_per_row" => Some(int(BYTES_PER_ROW)),
            "row_count" => Some(int(ROWS)),
            "total_cols" => Some(int(TOTAL_COLS)),
            // The selection's focus byte, or Null before any select.
            "cursor_byte" => Some(self.focus().map_or(IntrospectValue::Null, int)),
            // The focus byte's value as two hex digits, or Null.
            "cursor_value" => Some(
                self.focus()
                    .and_then(|b| self.bytes.get(b))
                    .map_or(IntrospectValue::Null, |&byte| {
                        IntrospectValue::Text(format!("{byte:02x}"))
                    }),
            ),
            // The hex cell the cursor rings ("col,row"), or Null.
            "cursor_cell" => Some(self.focus().map_or(IntrospectValue::Null, |b| {
                IntrospectValue::Text(format!(
                    "{},{}",
                    hex_col(b % BYTES_PER_ROW),
                    b / BYTES_PER_ROW
                ))
            })),
            // The selection range [start, end), its length, and its hex bytes.
            "selection_start" => Some(range.map_or(IntrospectValue::Null, |(s, _)| int(s))),
            "selection_end" => Some(range.map_or(IntrospectValue::Null, |(_, e)| int(e))),
            "selection_len" => Some(range.map_or(IntrospectValue::Null, |(s, e)| int(e - s))),
            "selection_hex" => Some(range.map_or(IntrospectValue::Null, |(s, e)| {
                IntrospectValue::Text(selection_hex(&self.bytes, s, e))
            })),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The AI-first, no-pixel collapsed-select channel: an Int (or
            // numeric string) selects that single byte; Null deselects.
            "cursor_byte" => match value {
                IntrospectValue::Null => {
                    self.selection = None;
                    self.pending_anchor = false;
                    Ok(())
                }
                ref v => {
                    let b = parse_index(v).ok_or(InterveneError::TypeMismatch)?;
                    if b >= SAMPLE_LEN {
                        return Err(InterveneError::OutOfRange);
                    }
                    self.select_byte(b);
                    Ok(())
                }
            },
            "byte_count" | "bytes_per_row" | "row_count" | "total_cols" | "cursor_value"
            | "cursor_cell" | "selection_start" | "selection_end" | "selection_len"
            | "selection_hex" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "hex_cell" => cell_for(&args, hex_col),
            "ascii_cell" => cell_for(&args, ascii_col),
            // A "col,row" cell -> the byte it addresses ("b"), or "none" for a
            // non-byte cell — the inverse of hex_cell / ascii_cell, and the
            // exact hit-test a click resolves.
            "byte_at_cell" => match args {
                IntrospectValue::Text(ref s) => {
                    let (col, row) = parse_cell(s).ok_or(InvokeError::TypeMismatch)?;
                    Ok(IntrospectValue::Text(
                        byte_at_cell(col, row).map_or_else(|| "none".to_owned(), |b| b.to_string()),
                    ))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // A "low,high" brush fraction pair -> the "lo,hi" byte range it
            // selects — the exact SSOT the view highlights from.
            "byte_window" => match args {
                IntrospectValue::Text(ref s) => {
                    let (low, high) = parse_pair(s).ok_or(InvokeError::TypeMismatch)?;
                    let (lo, hi) = selected_bytes(low, high);
                    Ok(IntrospectValue::Text(format!("{lo},{hi}")))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // The AI-first, no-pixel RANGE select: a "start,end" pair (inclusive
            // start / exclusive end) sets the selection to those bytes (focus at
            // the last), or "none" clears it. Returns the normalised "start,end".
            "select_range" => match args {
                IntrospectValue::Text(ref s) if s.trim() == "none" => {
                    self.selection = None;
                    self.pending_anchor = false;
                    Ok(IntrospectValue::Text("none".to_owned()))
                }
                IntrospectValue::Text(ref s) => {
                    let (start, end) = parse_cell(s).ok_or(InvokeError::TypeMismatch)?;
                    if end <= start || end > SAMPLE_LEN {
                        return Err(InvokeError::Rejected);
                    }
                    self.selection = Some((start, end - 1));
                    self.pending_anchor = false;
                    Ok(IntrospectValue::Text(format!("{start},{end}")))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // The router's pointer press / release symbolic events (R884): a
            // press latches a fresh anchor for the next pointer_move; a release
            // (or leave / cancel) just ends the gesture (the selection commits).
            "send" => {
                if let IntrospectValue::Text(ref name) = args {
                    match name.as_str() {
                        "PointerDown" => self.pending_anchor = true,
                        "PointerUp" | "PointerLeave" | "PointerCancel" => {
                            self.pending_anchor = false;
                        }
                        _ => {}
                    }
                }
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- The binding -----------------------------------------------------------

/// The binding. The `RangeSliderExternal` brush is the byte-range window
/// (primary interaction, via the extras slot); the [`HexDumpOracle`] is the
/// root external (the layout SSOT). A manual [`WidgetCore`] — the brush is
/// drag- and RPC-driven with no keyboard channel.
struct HexDumpView;

impl WidgetCore for HexDumpView {
    /// The brush window `(low, high)` fractions (sibling external) + the drag
    /// [`Selection`] (primary external).
    type State = (f32, f32, Option<Selection>);
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(HexDumpOracle::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        brush_extras()
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    fn read_state(scene: &Scene) -> (f32, f32, Option<Selection>) {
        let (low, high) = read_brush(scene);
        (low, high, read_selection(scene))
    }

    fn view(state: (f32, f32, Option<Selection>), frame: &Frame) -> Scene {
        view(state.0, state.1, state.2, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-hex-dump (R1402 §5.41 hex brush + drag byte selection)"
    }

    /// The brush is drag- and RPC-driven and the selection is press-drag- and
    /// RPC-driven; no keyboard channel, so no key is consumed here.
    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        false
    }

    fn fmt_state_log(state: &(f32, f32, Option<Selection>)) -> String {
        let (lo, hi) = selected_bytes(state.0, state.1);
        format!(
            "brush {:.3}..{:.3} / field {lo}..{hi} / selection {:?}",
            state.0, state.1, state.2
        )
    }
}

impl WidgetA11y for HexDumpView {
    /// The hex view as a `group` whose value names the selected byte range, plus
    /// the brush window as a `Slider` node whose `AccessValue::Text` states the
    /// field range (a two-thumb range has no single-`Float` shape, as
    /// `hello-autoscale-y` does).
    fn access_node(
        state: &(f32, f32, Option<Selection>),
        focused: Option<&str>,
    ) -> Vec<AccessNode> {
        let (lo, hi) = selected_bytes(state.0, state.1);
        let sel_txt = state.2.map_or_else(
            || "no bytes selected".to_owned(),
            |s| format!("selected bytes {} to {}", s.start, s.end),
        );
        vec![
            AccessNode::new(GRID_TAG, AriaRole::Group)
                .with_name("Hex dump")
                .with_value(AccessValue::Text(sel_txt)),
            AccessNode::new(BRUSH_TAG, AriaRole::Slider)
                .with_name("Hex dump byte-range brush".to_string())
                .with_value(AccessValue::Text(format!("bytes {lo} to {hi}")))
                .with_state(AccessState {
                    focused: focused == Some(BRUSH_TAG),
                    ..AccessState::default()
                }),
        ]
    }
}

impl WidgetView for HexDumpView {
    type Renderer = HelloHexDumpRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<HexDumpView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::style::Color;

    /// A column / row index as the `u16` the [`GridBuffer`] accessors take.
    fn c(n: usize) -> u16 {
        u16::try_from(n).unwrap()
    }

    /// Fixed cell colours for the layout tests — the accent highlight is any
    /// non-default `Rgb`, distinct from the default context background.
    fn test_colors() -> CellColors {
        CellColors {
            muted: TermColor::Rgb(Color::rgb(0x88, 0x88, 0x88)),
            highlight: TermColor::Rgb(Color::rgb(0x19, 0x76, 0xd2)),
            on_highlight: TermColor::Rgb(Color::rgb(0xff, 0xff, 0xff)),
        }
    }

    #[test]
    fn hex_columns_lay_out_two_eight_byte_groups() {
        // byte 0 opens the hex region at HEX_START; each byte spans 3 columns.
        assert_eq!(hex_col(0), 10);
        assert_eq!(hex_col(7), 31); // last of group one: 10 + 7*3
        // The group gap: byte 8 shifts right by one extra space.
        assert_eq!(hex_col(8), 35); // 10 + 8*3 + 1
        assert_eq!(hex_col(15), 56); // 10 + 15*3 + 1
        // The two hex digits of the last byte sit within the gutter's left edge.
        assert!(hex_col(15) + 1 < ASCII_BAR_L);
    }

    #[test]
    fn ascii_columns_sit_between_the_gutter_bars() {
        assert_eq!(ascii_col(0), ASCII_START);
        assert_eq!(ascii_col(15), 76);
        assert!(ASCII_BAR_L < ascii_col(0));
        assert!(ascii_col(15) < ASCII_BAR_R);
        assert_eq!(ASCII_BAR_R + 1, TOTAL_COLS);
    }

    #[test]
    fn sample_is_the_expected_length_with_the_header_intact() {
        let b = sample();
        assert_eq!(b.len(), SAMPLE_LEN);
        // The magic + 0x01 non-printable.
        assert_eq!(&b[0..4], &[0x50, 0x49, 0x4e, 0x01]);
        // The big-endian length field (44).
        assert_eq!(&b[4..8], &[0x00, 0x00, 0x00, 0x2c]);
        // The field tag.
        assert_eq!(&b[8..16], b"payload:");
    }

    #[test]
    fn grid_pixels_match_the_cell_layout() {
        // The pixel extent is locked to the column / row counts at the
        // 8x16 CellMetric::DEFAULT cell (no cast).
        assert_eq!(GRID_W, u32::try_from(TOTAL_COLS).unwrap() * 8);
        assert_eq!(GRID_H, u32::try_from(ROWS).unwrap() * 16);
        assert_eq!(ROWS, 8);
        assert_eq!(TOTAL_COLS, 78);
    }

    #[test]
    fn buffer_dimensions_derive_from_the_layout() {
        let buf = hex_dump_buffer(&sample(), 0..0, None, &test_colors());
        assert_eq!(buf.cols(), c(TOTAL_COLS));
        assert_eq!(buf.rows(), c(ROWS));
    }

    #[test]
    fn row_zero_offset_and_hex_and_ascii_render() {
        let buf = hex_dump_buffer(&sample(), 0..0, None, &test_colors());
        // Offset column: "00000000".
        let offset: String = (0..OFFSET_COLS)
            .map(|col| buf.cell(c(col), 0).unwrap().cluster.clone())
            .collect();
        assert_eq!(offset, "00000000");
        // Byte 0 = 0x50 = "50".
        assert_eq!(buf.cell(c(hex_col(0)), 0).unwrap().cluster, "5");
        assert_eq!(buf.cell(c(hex_col(0)) + 1, 0).unwrap().cluster, "0");
        // Its ascii glyph is 'P'.
        assert_eq!(buf.cell(c(ascii_col(0)), 0).unwrap().cluster, "P");
    }

    #[test]
    fn non_printable_bytes_show_a_dot_in_the_gutter() {
        let buf = hex_dump_buffer(&sample(), 0..0, None, &test_colors());
        // Byte 3 is 0x01 (non-printable) -> '.' in the muted tint.
        let cell = buf.cell(c(ascii_col(3)), 0).unwrap();
        assert_eq!(cell.cluster, ".");
        assert_eq!(cell.fg, test_colors().muted);
        // But its hex pair is the real value "01".
        assert_eq!(buf.cell(c(hex_col(3)), 0).unwrap().cluster, "0");
        assert_eq!(buf.cell(c(hex_col(3)) + 1, 0).unwrap().cluster, "1");
    }

    #[test]
    fn selection_highlights_both_the_hex_and_ascii_regions() {
        let colors = test_colors();
        // Select bytes 8..12 (row 0, second group).
        let buf = hex_dump_buffer(&sample(), 8..12, None, &colors);
        for b in 8..12usize {
            let hc = c(hex_col(b));
            // Both hex digit cells carry the highlight background.
            assert_eq!(buf.cell(hc, 0).unwrap().bg, colors.highlight, "hex hi {b}");
            assert_eq!(
                buf.cell(hc + 1, 0).unwrap().bg,
                colors.highlight,
                "hex lo {b}"
            );
            assert_eq!(
                buf.cell(hc, 0).unwrap().fg,
                colors.on_highlight,
                "hex fg {b}"
            );
            // And the mirrored ascii cell — the linked two-region highlight.
            assert_eq!(
                buf.cell(c(ascii_col(b)), 0).unwrap().bg,
                colors.highlight,
                "ascii hi {b}"
            );
        }
        // Bytes outside the window stay context (default background).
        assert_eq!(buf.cell(c(hex_col(7)), 0).unwrap().bg, TermColor::Default);
        assert_eq!(buf.cell(c(ascii_col(7)), 0).unwrap().bg, TermColor::Default);
        assert_eq!(buf.cell(c(hex_col(12)), 0).unwrap().bg, TermColor::Default);
    }

    #[test]
    fn selected_bytes_maps_the_brush_window_to_a_byte_range() {
        // Full span -> the whole buffer.
        assert_eq!(selected_bytes(0.0, 1.0), (0, SAMPLE_LEN));
        // The boot window rounds onto its byte boundaries.
        let (blo, bhi) = boot_window();
        assert_eq!(selected_bytes(blo, bhi), (BOOT_BYTE_LO, BOOT_BYTE_HI));
        // A half window -> the first half.
        assert_eq!(selected_bytes(0.0, 0.5), (0, 64));
        // A reversed pair is the same window.
        assert_eq!(selected_bytes(0.5, 0.25), selected_bytes(0.25, 0.5));
    }

    #[test]
    fn oracle_reports_the_layout_and_the_mappings() {
        let mut o = HexDumpOracle::new();
        assert_eq!(o.query("byte_count"), Some(IntrospectValue::Int(128)));
        assert_eq!(o.query("bytes_per_row"), Some(IntrospectValue::Int(16)));
        assert_eq!(o.query("row_count"), Some(IntrospectValue::Int(8)));
        assert_eq!(o.query("total_cols"), Some(IntrospectValue::Int(78)));
        assert_eq!(o.query("nope"), None);

        // hex_cell / ascii_cell map a byte to its "col,row".
        assert_eq!(
            o.invoke("hex_cell", IntrospectValue::Text("0".into())),
            Ok(IntrospectValue::Text(format!("{},0", hex_col(0))))
        );
        // Byte 20 is on row 1, column 4.
        assert_eq!(
            o.invoke("ascii_cell", IntrospectValue::Text("20".into())),
            Ok(IntrospectValue::Text(format!("{},1", ascii_col(4))))
        );
        // Past the buffer -> "none".
        assert_eq!(
            o.invoke("hex_cell", IntrospectValue::Text("128".into())),
            Ok(IntrospectValue::Text("none".into()))
        );
        // byte_window mirrors selected_bytes.
        assert_eq!(
            o.invoke("byte_window", IntrospectValue::Text("0.0,0.5".into())),
            Ok(IntrospectValue::Text("0,64".into()))
        );
        // Bad argument types.
        assert_eq!(
            o.invoke("hex_cell", IntrospectValue::Null),
            Err(InvokeError::TypeMismatch)
        );
        assert_eq!(
            o.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn oracle_guards_readonly_and_unknown_paths() {
        let mut o = HexDumpOracle::new();
        assert_eq!(
            o.intervene("byte_count", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            o.intervene("nope", IntrospectValue::Null),
            Err(InterveneError::UnknownPath)
        );
    }

    #[test]
    fn view_carries_the_grid_and_brush_tags() {
        let (low, high) = boot_window();
        let scene = pinion_core::Owner::new().run(|| view(low, high, None, &Frame::new()));
        assert!(scene.contains_tag(GRID_TAG));
        assert!(scene.contains_tag(BRUSH_TAG));
    }

    #[test]
    fn byte_at_cell_is_the_inverse_of_hex_and_ascii_col() {
        // Every byte's three cells (two hex digits + the ascii glyph) resolve
        // back to that byte; the round-trip is exact.
        for b in 0..SAMPLE_LEN {
            let (j, row) = (b % BYTES_PER_ROW, b / BYTES_PER_ROW);
            assert_eq!(byte_at_cell(hex_col(j), row), Some(b), "hex-hi byte {b}");
            assert_eq!(
                byte_at_cell(hex_col(j) + 1, row),
                Some(b),
                "hex-lo byte {b}"
            );
            assert_eq!(byte_at_cell(ascii_col(j), row), Some(b), "ascii byte {b}");
        }
        // Non-byte cells resolve to nothing: the offset column, a group-gap
        // space, and the gutter bars.
        assert_eq!(byte_at_cell(0, 0), None, "offset column");
        assert_eq!(byte_at_cell(hex_col(0) + 2, 0), None, "inter-byte space");
        assert_eq!(byte_at_cell(ASCII_BAR_L, 0), None, "left gutter bar");
        assert_eq!(byte_at_cell(ASCII_BAR_R, 0), None, "right gutter bar");
        assert_eq!(byte_at_cell(hex_col(0), ROWS), None, "past the last row");
    }

    /// A grid-relative pixel as the `[0, 1]` fraction the router delivers.
    #[allow(clippy::cast_precision_loss)]
    fn frac(px: usize, extent: u32) -> f32 {
        px as f32 / extent as f32
    }

    /// Send a `PointerDown` to the oracle (a press latches a fresh anchor).
    fn press(o: &mut HexDumpOracle) {
        o.invoke("send", IntrospectValue::Text("PointerDown".into()))
            .unwrap();
    }

    /// The `[0, 1]` fraction over the grid at byte `b`'s hex-cell centre.
    fn byte_frac(b: usize) -> (f32, f32) {
        let (col, row) = (hex_col(b % BYTES_PER_ROW), b / BYTES_PER_ROW);
        (frac(col * 8 + 4, GRID_W), frac(row * 16 + 8, GRID_H))
    }

    #[test]
    fn pointer_move_resolves_the_byte_under_the_cursor() {
        let mut o = HexDumpOracle::new();
        assert_eq!(o.focus(), None, "no selection before any pointer");
        // A press then a move over byte 8's hex cell selects it (collapsed).
        press(&mut o);
        let (fx, fy) = byte_frac(8);
        o.pointer_move(fx, fy);
        assert_eq!(o.focus(), Some(8), "click on byte 8's hex cell");
        assert_eq!(o.range(), Some((8, 9)), "a click is a 1-byte selection");
        // A fresh press on the offset column deselects.
        press(&mut o);
        o.pointer_move(0.0, frac(8, GRID_H));
        assert_eq!(o.focus(), None, "click on the offset column deselects");
    }

    #[test]
    fn press_drag_selects_and_extends_a_byte_range() {
        let mut o = HexDumpOracle::new();
        // Press on byte 4, drag to byte 9 (same row) -> range [4, 10).
        press(&mut o);
        let (ax, ay) = byte_frac(4);
        o.pointer_move(ax, ay);
        assert_eq!(o.range(), Some((4, 5)), "anchor at byte 4");
        let (bx, by) = byte_frac(9);
        o.pointer_move(bx, by);
        assert_eq!(o.range(), Some((4, 10)), "drag to byte 9 extends the range");
        assert_eq!(o.focus(), Some(9), "the focus is the drag's live end");
        // A backward drag (before the anchor) normalises the range.
        let (cx, cy) = byte_frac(1);
        o.pointer_move(cx, cy);
        assert_eq!(o.range(), Some((1, 5)), "backward drag keeps anchor 4");
        assert_eq!(o.focus(), Some(1));
        // Dragging onto a non-byte cell (the gutter) does NOT collapse it.
        o.pointer_move(frac(ASCII_BAR_L * 8 + 4, GRID_W), frac(8, GRID_H));
        assert_eq!(o.range(), Some((1, 5)), "gutter drag keeps the last range");
        // A NEW press (after release) starts a fresh selection.
        o.invoke("send", IntrospectValue::Text("PointerUp".into()))
            .unwrap();
        press(&mut o);
        let (dx, dy) = byte_frac(18); // row 1
        o.pointer_move(dx, dy);
        assert_eq!(o.range(), Some((18, 19)), "new press = fresh 1-byte range");
    }

    #[test]
    fn select_range_invoke_and_selection_queries() {
        let mut o = HexDumpOracle::new();
        assert_eq!(o.query("selection_start"), Some(IntrospectValue::Null));
        assert_eq!(o.query("selection_hex"), Some(IntrospectValue::Null));
        // Select the header's 4-byte length field [4, 8) = 00 00 00 2c.
        assert_eq!(
            o.invoke("select_range", IntrospectValue::Text("4,8".into())),
            Ok(IntrospectValue::Text("4,8".into())),
        );
        assert_eq!(o.query("selection_start"), Some(IntrospectValue::Int(4)));
        assert_eq!(o.query("selection_end"), Some(IntrospectValue::Int(8)));
        assert_eq!(o.query("selection_len"), Some(IntrospectValue::Int(4)));
        assert_eq!(
            o.query("selection_hex"),
            Some(IntrospectValue::Text("0000002c".into())),
            "the length field's bytes",
        );
        assert_eq!(
            o.query("cursor_byte"),
            Some(IntrospectValue::Int(7)),
            "focus is the last selected byte",
        );
        // "none" clears it.
        assert_eq!(
            o.invoke("select_range", IntrospectValue::Text("none".into())),
            Ok(IntrospectValue::Text("none".into())),
        );
        assert_eq!(o.query("selection_start"), Some(IntrospectValue::Null));
        // Empty / out-of-range ranges are rejected.
        assert_eq!(
            o.invoke("select_range", IntrospectValue::Text("5,5".into())),
            Err(InvokeError::Rejected),
        );
        assert_eq!(
            o.invoke("select_range", IntrospectValue::Text("120,200".into())),
            Err(InvokeError::Rejected),
        );
    }

    #[test]
    fn intervene_cursor_byte_sets_and_clears_and_queries_reflect_it() {
        let mut o = HexDumpOracle::new();
        // Boot: no cursor.
        assert_eq!(o.query("cursor_byte"), Some(IntrospectValue::Null));
        assert_eq!(o.query("cursor_value"), Some(IntrospectValue::Null));

        // Select byte 0 (0x50) — the queries reflect it.
        o.intervene("cursor_byte", IntrospectValue::Int(0)).unwrap();
        assert_eq!(o.query("cursor_byte"), Some(IntrospectValue::Int(0)));
        assert_eq!(
            o.query("cursor_value"),
            Some(IntrospectValue::Text("50".into())),
            "byte 0 is 0x50",
        );
        assert_eq!(
            o.query("cursor_cell"),
            Some(IntrospectValue::Text(format!("{},0", hex_col(0)))),
        );

        // Null deselects.
        o.intervene("cursor_byte", IntrospectValue::Null).unwrap();
        assert_eq!(o.query("cursor_byte"), Some(IntrospectValue::Null));

        // Out of range and read-only slots are rejected.
        assert_eq!(
            o.intervene("cursor_byte", IntrospectValue::Int(128)),
            Err(InterveneError::OutOfRange),
        );
        assert_eq!(
            o.intervene("byte_count", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly),
        );
    }

    #[test]
    fn byte_at_cell_invoke_maps_a_cell_to_its_byte() {
        let mut o = HexDumpOracle::new();
        assert_eq!(
            o.invoke(
                "byte_at_cell",
                IntrospectValue::Text(format!("{},1", hex_col(4)))
            ),
            Ok(IntrospectValue::Text("20".into())),
            "row 1 col hex(4) = byte 20",
        );
        assert_eq!(
            o.invoke("byte_at_cell", IntrospectValue::Text("0,0".into())),
            Ok(IntrospectValue::Text("none".into())),
            "the offset column is no byte",
        );
        assert_eq!(
            o.invoke("byte_at_cell", IntrospectValue::Int(3)),
            Err(InvokeError::TypeMismatch),
        );
    }

    #[test]
    fn view_rings_the_focus_and_reverse_videos_the_selection() {
        let (low, high) = boot_window();
        // No selection -> the default hidden cursor, no reversed cells.
        let plain = pinion_core::Owner::new().run(|| view(low, high, None, &Frame::new()));
        assert_eq!(grid_cursor(&plain), GridCursor::default());

        // Select bytes [18, 22) with the focus at 20 (row 1).
        let sel = Selection {
            start: 18,
            end: 22,
            focus: 20,
        };
        let scene = pinion_core::Owner::new().run(|| view(low, high, Some(sel), &Frame::new()));
        // The focus byte (20 = row 1 col 4) rings with a visible block.
        let cur = grid_cursor(&scene);
        assert!(cur.visible, "the focus byte's cursor is visible");
        assert_eq!(cur.shape, CursorShape::Block);
        assert_eq!((cur.col, cur.row), (c(hex_col(4)), 1), "focus byte 20");
        // Every selected byte's hex + ascii cells are reverse-video; the
        // neighbours just outside the range are not.
        let buf = grid_buffer(&scene);
        for b in 18..22u16 {
            let hc = c(hex_col(usize::from(b) % BYTES_PER_ROW));
            assert!(
                buf.cell(hc, 1).unwrap().attrs.reverse,
                "byte {b} hex reversed"
            );
            let ac = c(ascii_col(usize::from(b) % BYTES_PER_ROW));
            assert!(
                buf.cell(ac, 1).unwrap().attrs.reverse,
                "byte {b} ascii reversed"
            );
        }
        assert!(
            !buf.cell(c(hex_col(1)), 1).unwrap().attrs.reverse,
            "byte 17 (before the range) is not reversed"
        );
        assert!(
            !buf.cell(c(hex_col(6)), 1).unwrap().attrs.reverse,
            "byte 22 (past the range) is not reversed"
        );
    }

    /// The `GridBuffer` carried by the hex-dump grid in a built scene.
    fn grid_buffer(scene: &Scene) -> GridBuffer {
        let Scene::Container(root) = scene else {
            panic!("view root is a Container");
        };
        root.children
            .iter()
            .find_map(|child| match child {
                Scene::TextGrid(n) if n.tag.as_deref() == Some(GRID_TAG) => Some(n.cells().clone()),
                _ => None,
            })
            .expect("the hex-dump grid is present")
    }

    /// The `GridCursor` carried by the hex-dump grid in a built scene.
    fn grid_cursor(scene: &Scene) -> GridCursor {
        let Scene::Container(root) = scene else {
            panic!("view root is a Container");
        };
        root.children
            .iter()
            .find_map(|child| match child {
                Scene::TextGrid(n) if n.tag.as_deref() == Some(GRID_TAG) => {
                    Some(n.cells().cursor())
                }
                _ => None,
            })
            .expect("the hex-dump grid is present")
    }
}
