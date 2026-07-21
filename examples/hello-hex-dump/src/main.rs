// R1008 §5.16 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-hex-dump` — R1400 §5.41 — a **hex/byte-dump viewer over
//! [`Scene::TextGrid`]** with a byte-range brush that highlights across the
//! hex column AND the ascii gutter at once.
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
//! ## Why the brush is the only control
//!
//! Brushing the byte window is the one interaction, so — like
//! `hello-autoscale-y` — the binding makes the `RangeSliderExternal` its
//! **primary** external through the [`Brush`] extras slot, and the
//! [`HexDumpOracle`] (the layout SSOT) is the root external. A drag on the
//! strip routes to the brush; over RPC the window is `scene/intervene
//! /hex_brush/external/{low,high}`.
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
use pinion_core::{CellMetric, Frame, GridBuffer, Scene, TermCell, TermColor, WidgetCore};
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

/// Build the hex-dump [`GridBuffer`]: offset column, `8 + 8` hex pairs, and
/// the ascii gutter, with the bytes in `sel` (`lo..hi`) highlighted in BOTH
/// the hex column and the gutter (the two-region linked field highlight).
fn hex_dump_buffer(bytes: &[u8], sel: core::ops::Range<usize>, colors: &CellColors) -> GridBuffer {
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

        // The 16 bytes: hex pair + ascii glyph, each highlighted iff selected.
        for j in 0..BYTES_PER_ROW {
            let idx = base + j;
            let Some(&byte) = bytes.get(idx) else { break };
            let selected = sel.contains(&idx);
            let (hex_fg, hl_bg) = if selected {
                (colors.on_highlight, colors.highlight)
            } else {
                (TermColor::Default, TermColor::Default)
            };

            let hc = hex_col(j);
            cells[hc] = TermCell::new(hex_digit(byte >> 4), hex_fg, hl_bg);
            cells[hc + 1] = TermCell::new(hex_digit(byte), hex_fg, hl_bg);

            let (glyph, glyph_fg) = ascii_glyph(byte, colors);
            let (ink, fill) = if selected {
                (colors.on_highlight, colors.highlight)
            } else {
                (glyph_fg, TermColor::Default)
            };
            cells[ascii_col(j)] = TermCell::new(glyph, ink, fill);
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

/// view-fn (§6.3): pure sync mapping. `(low, high)` is the brushed byte-offset
/// window that highlights the selected bytes in both the hex and ascii regions.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(low: f32, high: f32, _frame: &Frame) -> Scene {
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
    let (lo, hi) = selected_bytes(low, high);

    let grid = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(GRID_TAG)
            .with_cells(hex_dump_buffer(&sample(), lo..hi, &colors))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(GRID_POS.0, GRID_POS.1)
                    .with_size(Size::px(GRID_W, GRID_H)),
            ),
    );

    let title = Scene::Text(
        TextNode::styled(
            "Hex dump — brush the strip to select a byte field (lit in hex + ascii)",
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

    let status = Scene::Text(
        TextNode::styled(
            format!(
                "selected bytes 0x{lo:x}..0x{hi:x}  ({} of {SAMPLE_LEN} bytes)",
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

/// The dump's layout as an introspectable oracle — the primary external, so an
/// AI client reads the byte↔cell mapping and the brush→byte-range mapping
/// directly instead of reverse-engineering the column geometry from a
/// snapshot. Stateless: every answer is a pure function of the layout
/// constants (`hex_cell` / `ascii_cell`) or the brush fractions
/// (`byte_window`, the [`selected_bytes`] SSOT).
#[derive(Debug, Clone, Copy)]
struct HexDumpOracle;

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
                    SchemaField::new("hex_cell", "string"),
                    SchemaField::new("ascii_cell", "string"),
                    SchemaField::new("byte_window", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let int = |n: usize| IntrospectValue::Int(i64::try_from(n).unwrap_or(0));
        match path {
            "byte_count" => Some(int(SAMPLE_LEN)),
            "bytes_per_row" => Some(int(BYTES_PER_ROW)),
            "row_count" => Some(int(ROWS)),
            "total_cols" => Some(int(TOTAL_COLS)),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "byte_count" | "bytes_per_row" | "row_count" | "total_cols" => {
                Err(InterveneError::ReadOnly)
            }
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
    /// The brush window `(low, high)` fractions, read from the sibling external.
    type State = (f32, f32);
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(HexDumpOracle)
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        brush_extras()
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    fn read_state(scene: &Scene) -> (f32, f32) {
        read_brush(scene)
    }

    fn view(state: (f32, f32), frame: &Frame) -> Scene {
        view(state.0, state.1, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-hex-dump (R1400 §5.41 hex/ascii byte-range brush)"
    }

    /// The brush is drag- and RPC-driven (the chart-family convention); it has
    /// no keyboard channel, so no key is consumed here.
    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        false
    }

    fn fmt_state_log(state: &(f32, f32)) -> String {
        let (lo, hi) = selected_bytes(state.0, state.1);
        format!("brush {:.3}..{:.3} / bytes {lo}..{hi}", state.0, state.1)
    }
}

impl WidgetA11y for HexDumpView {
    /// The hex view as a `group`, plus the brush window as a `Slider` node whose
    /// `AccessValue::Text` states the selected byte range (a two-thumb range has
    /// no single-`Float` shape, as `hello-autoscale-y` does).
    fn access_node(state: &(f32, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (lo, hi) = selected_bytes(state.0, state.1);
        vec![
            AccessNode::new(GRID_TAG, AriaRole::Group).with_name("Hex dump"),
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
        let buf = hex_dump_buffer(&sample(), 0..0, &test_colors());
        assert_eq!(buf.cols(), c(TOTAL_COLS));
        assert_eq!(buf.rows(), c(ROWS));
    }

    #[test]
    fn row_zero_offset_and_hex_and_ascii_render() {
        let buf = hex_dump_buffer(&sample(), 0..0, &test_colors());
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
        let buf = hex_dump_buffer(&sample(), 0..0, &test_colors());
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
        let buf = hex_dump_buffer(&sample(), 8..12, &colors);
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
        let mut o = HexDumpOracle;
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
        let mut o = HexDumpOracle;
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
        let scene = pinion_core::Owner::new().run(|| view(low, high, &Frame::new()));
        assert!(scene.contains_tag(GRID_TAG));
        assert!(scene.contains_tag(BRUSH_TAG));
    }
}
