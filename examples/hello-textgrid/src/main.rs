//! `hello-textgrid` — R972 §5.41 — the **cell-native `TextGrid` geometry
//! scaffold**, the first real consumer of the [`CellMetric`] coordinate
//! substrate (R968 ratify → R970 metric type → R971 authoritative dims).
//!
//! Two [`Scene::TextGrid`] nodes sit at fixed absolute positions:
//!
//!   * **`htg_default`** — the behaviour-preserving 8×16 bitmap baseline
//!     ([`CellMetric::DEFAULT`]), sized `640×384` → **80 × 24** cells;
//!   * **`htg_measured`** — a *font-derived* grid (R1003): `view` measures the
//!     real monospace cell at `MEASURED_FONT_PX` through the view-time
//!     [`measured_monospace_cell`](pinion_core::measured_monospace_cell) seam
//!     (seeded by the shell) and pins that size, so the glyph is snug
//!     (`cell_w == advance`). Headless (no provider) it falls back to a
//!     producer-picked `(9, 18)` on the fit path.
//!
//! ## What this proves (and what it defers)
//!
//! The first two grids stay **empty geometry scaffolds**: each carries a
//! node-local [`CellMetric`] plus its layout-resolved pixel [`Rect`] and
//! derives its `(cols, rows)` from that rect via the metric — the R969
//! one-directional `(rows, cols)` SSOT (layout → dims, never fed back).
//!
//! R973 adds a third grid, **`htg_content`**, that carries a cell-content
//! [`GridBuffer`] projection — the first S5 data-model slice: the
//! terminal colour model ([`TermColor`]: default / indexed / truecolor)
//! resolved through the grid's [`Palette`](pinion_core::Palette). Its
//! cells exercise all three colour forms: an ANSI 16-colour bar
//! (indexed backgrounds), a default-coloured label, a truecolor RGB row,
//! and a mixed indexed-cube / grayscale-ramp row.
//!
//! R974 adds a fourth grid, **`htg_attrs`**, carrying SGR display
//! attributes ([`CellAttrs`]: bold / dim / italic / underline / blink /
//! reverse / hidden / strikethrough) — one flag per cell, plus
//! combinations and a `reverse` cell whose stored colours are reported
//! unswapped (the swap is a paint-time transform, still deferred).
//!
//! R975 adds a fifth grid, **`htg_cursor`**, carrying the grid
//! [`GridCursor`]: a visible vertical **bar** cursor at the input column
//! of a prompt line (row 1), proving the cursor's cell position / shape /
//! visibility travel with the projection. The other grids carry the
//! default (hidden, home, block) cursor.
//!
//! R976 adds a sixth grid, **`htg_wide`**, carrying wide-cluster cells
//! ([`CellWidth`]): each CJK ideograph (世界) and fullwidth Latin form (Ａ)
//! occupies two columns — a head cell ([`TermCell::wide`]) plus a
//! continuation trailer ([`TermCell::trailer`]) that carries no glyph. The
//! snapshot's row text holds each wide glyph once even though it spans two
//! columns, and the style runs mark `wide` / `trailer` so a client maps
//! glyphs to columns. The other grids stay all-narrow.
//!
//! R977 adds a seventh grid, **`htg_alt`**, tagged [`ScreenKind::Alternate`]:
//! a tiny fullscreen-app projection (a file line + a reverse-video status
//! bar) with its own block cursor, proving the screen-kind discriminator
//! travels with the projection and each screen carries its own cursor. The
//! other grids stay on the main screen.
//!
//! R978 adds an eighth grid, **`htg_damage`**, carrying per-row monotonic
//! damage [generations](GridBuffer::row_generation): three streamed output
//! lines, the newest stamped highest. A streaming client re-reads only the
//! rows whose generation exceeds its last-seen high-water mark — the
//! incremental-update model that completes the S5 data model. The other
//! grids leave every row at generation 0.
//!
//! **Glyph paint** (R991 §5.41 §2 #6) renders these grids on the Vello
//! backend: each cell's palette-resolved background fills its rect and the
//! grapheme cluster paints in the resolved foreground, with `reverse`
//! (fg<->bg swap), `hidden` (glyph suppressed), and wide clusters (the
//! head's background spans both columns; the glyph is drawn once at the
//! head, suppressed on the trailer) honoured — the window now shows the
//! terminal. **R992 §5.41** adds the typographic SGR attributes: `bold` /
//! `italic` set the glyph weight / slant, `dim` faints the foreground, and
//! `underline` / `strikethrough` stroke a full-cell rule — so the
//! `htg_attrs` grid now shows each SGR flag. **R993 §5.41** paints the
//! [`GridCursor`] overlay (block / bar / underline shapes), so `htg_cursor`
//! shows its bar and `htg_alt` its block. **R994 §5.41** adds the ratatui
//! TUI arm (§2 #6 GUI / TUI dual), so these grids also render in the
//! terminal backend. Only the Vello `blink` slice (the TUI gets it free from
//! the host terminal) remains; the cell *data model* below remains the
//! AI-first witness, read as data.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` reports each grid's `rect`, `cell_w` / `cell_h`, and
//! derived `cols` / `rows`. From those an AI client reconstructs the
//! whole cell↔pixel mapping with no OCR: the winsize round-trip is
//! `cols == floor(rect.w / cell_w)` (and the pixel extent the cells
//! span, `cols * cell_w`, fits within `rect.w`). The R972 demo
//! (`tools/r972_textgrid.py`) asserts this for both metrics. For
//! `htg_content`, `grid_rows` reports each row's text and its
//! palette-resolved style runs — `tools/r973_textgrid_cells.py` asserts
//! every colour form resolves correctly. For `htg_cursor`, `cursor`
//! reports the cursor's `(col, row)`, `shape`, and `visible` —
//! `tools/demos/r975_textgrid_cursor.py` asserts it (and that the other
//! grids carry the default hidden cursor). For `htg_wide`, each style run
//! carries a `width` (`narrow` / `wide` / `trailer`) and the row text
//! holds each wide glyph once — `tools/demos/r976_textgrid_wide.py`
//! asserts the head / trailer structure and glyph-to-column mapping. For
//! `htg_alt`, the grid's `screen` is `alternate` while every other grid is
//! `main` — `tools/demos/r977_textgrid_altscreen.py` asserts it. For
//! `htg_damage`, each row carries a `generation` and a client computes its
//! changed-set from a baseline — `tools/demos/r978_textgrid_damage.py`
//! asserts the incremental-read model.

use pinion_a11y::{AccessNode, WidgetA11y};
use pinion_core::external::{External, StubExternal};
use pinion_core::scene::{ContainerNode, TextGridNode};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::{
    CellAttrs, CellMetric, CursorShape, Frame, GridBuffer, GridCursor, Scene, ScreenKind, TermCell,
    TermColor, WidgetCore,
};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTextGridRenderer, HelloTextGridRendererError);

/// Window size — large enough to hold both grids with a margin.
const WIN_W: u32 = 680;
const WIN_H: u32 = 840;
/// Shared [`ThemeProvider`] cache key.
const THEME_TAG: &str = "app";
/// Paint-root + [`StubExternal`] anchor tag (the `V::tag()` the composite
/// paint-root convention attaches to the root container).
const ROOT_TAG: &str = "htg";

/// Tag of the 8×16-baseline grid, and its placement + extent.
const DEFAULT_TAG: &str = "htg_default";
const DEFAULT_POS: (u32, u32) = (16, 16);
const DEFAULT_SIZE: (u32, u32) = (640, 384); // 80 cols × 24 rows @ 8×16

/// Tag of the measured-metric (9×18) grid, and its placement + extent.
const MEASURED_TAG: &str = "htg_measured";
const MEASURED_POS: (u32, u32) = (16, 432);
const MEASURED_SIZE: (u32, u32) = (360, 360); // 40 cols × 20 rows @ 9×18
/// Font size the measured-metric grid renders at (R1003). When the shell has
/// seeded the font-metrics seam (windowed run), `view` measures the real
/// monospace cell at this size via
/// [`measured_monospace_cell`](pinion_core::measured_monospace_cell) and pins
/// it with [`TextGridNode::with_font_size_px`] → snug (`cell_w == advance`).
const MEASURED_FONT_PX: u32 = 18;

/// Headless fallback cell for the measured-metric grid: used only when no font
/// provider is seeded (the unit tests, RPC snapshot) and
/// `measured_monospace_cell` returns `None`. A hardcoded `cell_w` does NOT
/// match the platform advance, so this fallback renders on the fit path (R1002
/// finding: only the measured `cell_w == advance` is snug). The windowed run
/// uses the seam above instead.
const MEASURED_CELL: (u32, u32) = (9, 18);

/// Tag of the R973 cell-content grid, and its placement + extent.
const CONTENT_TAG: &str = "htg_content";
const CONTENT_POS: (u32, u32) = (400, 440);
/// `16 × 4` cells at the `8×16` baseline metric → `128 × 64` px. The rect
/// is sized to derive exactly the projection's dimensions, mirroring a
/// producer that sized its buffer to the notified winsize.
const CONTENT_COLS: u16 = 16;
const CONTENT_ROWS: u16 = 4;
const CONTENT_SIZE: (u32, u32) = (128, 64);

/// Build the R973 cell-content projection: the producer-assembled
/// [`GridBuffer`] the `htg_content` grid shows. Exercises every
/// [`TermColor`] form so the snapshot consumer can witness the palette
/// resolution (R969 "resolve at paint time").
fn content_buffer() -> GridBuffer {
    GridBuffer::new(CONTENT_COLS, CONTENT_ROWS)
        // Row 0 — the ANSI 16-colour bar: each cell's background is
        // palette index `i` (`0..16`), foreground left default.
        .with_row(
            0,
            (0..CONTENT_COLS).map(|i| {
                // `i < 16`, so the cast never truncates the index.
                let index = u8::try_from(i).unwrap_or(0);
                TermCell::new(" ", TermColor::Default, TermColor::Indexed(index))
            }),
        )
        // Row 1 — a default-coloured label. The 7 glyphs plus the 9
        // trailing blanks all carry default fg/bg, so the snapshot
        // collapses the whole row into a single style run.
        .with_row(
            1,
            "Default"
                .chars()
                .map(|c| TermCell::new(c.to_string(), TermColor::Default, TermColor::Default)),
        )
        // Row 2 — truecolor: direct 24-bit RGB foregrounds, palette-free.
        .with_row(
            2,
            [
                TermCell::new("R", TermColor::Rgb(Color::rgb(0xff, 0x00, 0x00)), TermColor::Default),
                TermCell::new("G", TermColor::Rgb(Color::rgb(0x00, 0xff, 0x00)), TermColor::Default),
                TermCell::new("B", TermColor::Rgb(Color::rgb(0x00, 0x00, 0xff)), TermColor::Default),
            ],
        )
        // Row 3 — mixed indexed: white-on-blue ANSI, a colour-cube red
        // foreground, and the darkest grayscale-ramp foreground.
        .with_row(
            3,
            [
                TermCell::new("#", TermColor::Indexed(15), TermColor::Indexed(4)),
                TermCell::new("g", TermColor::Indexed(196), TermColor::Default),
                TermCell::new("y", TermColor::Indexed(232), TermColor::Default),
            ],
        )
}

/// Tag of the R974 SGR-attribute grid, and its placement + extent.
const ATTRS_TAG: &str = "htg_attrs";
const ATTRS_POS: (u32, u32) = (400, 520);
/// `8 × 2` cells at the `8×16` baseline metric → `64 × 32` px.
const ATTRS_COLS: u16 = 8;
const ATTRS_ROWS: u16 = 2;
const ATTRS_SIZE: (u32, u32) = (64, 32);

/// Build the R974 SGR-attribute projection. Row 0 shows each attribute in
/// isolation (one flag per cell); row 1 shows combinations and the
/// `reverse` flag carried alongside stored colours (the swap is applied
/// at paint time, still deferred).
fn attrs_buffer() -> GridBuffer {
    let e = CellAttrs::empty;
    GridBuffer::new(ATTRS_COLS, ATTRS_ROWS)
        // Row 0 — one SGR flag per cell, in declaration order.
        .with_row(
            0,
            [
                TermCell::new("B", TermColor::Default, TermColor::Default).with_attrs(e().with_bold(true)),
                TermCell::new("D", TermColor::Default, TermColor::Default).with_attrs(e().with_dim(true)),
                TermCell::new("I", TermColor::Default, TermColor::Default).with_attrs(e().with_italic(true)),
                TermCell::new("U", TermColor::Default, TermColor::Default).with_attrs(e().with_underline(true)),
                TermCell::new("K", TermColor::Default, TermColor::Default).with_attrs(e().with_blink(true)),
                TermCell::new("R", TermColor::Default, TermColor::Default).with_attrs(e().with_reverse(true)),
                TermCell::new("H", TermColor::Default, TermColor::Default).with_attrs(e().with_hidden(true)),
                TermCell::new("S", TermColor::Default, TermColor::Default).with_attrs(e().with_strikethrough(true)),
            ],
        )
        // Row 1 — combinations + reverse with stored colours (not swapped).
        .with_row(
            1,
            [
                TermCell::new("a", TermColor::Default, TermColor::Default)
                    .with_attrs(e().with_bold(true).with_italic(true)),
                TermCell::new("b", TermColor::Default, TermColor::Default)
                    .with_attrs(e().with_underline(true).with_strikethrough(true)),
                TermCell::new("c", TermColor::Indexed(1), TermColor::Indexed(15))
                    .with_attrs(e().with_reverse(true)),
            ],
        )
}

/// Tag of the R975 cursor grid, and its placement + extent.
const CURSOR_TAG: &str = "htg_cursor";
const CURSOR_POS: (u32, u32) = (400, 600);
/// `8 × 2` cells at the `8×16` baseline metric → `64 × 32` px.
const CURSOR_COLS: u16 = 8;
const CURSOR_ROWS: u16 = 2;
const CURSOR_SIZE: (u32, u32) = (64, 32);
/// Where the bar cursor sits: the input column (col 2, past the `"$ "`
/// prompt) on the prompt line (row 1). Non-zero on both axes so the
/// snapshot witnesses real cell addressing, not a trivial home position.
const CURSOR_AT: (u16, u16) = (2, 1);

/// Build the R975 cursor projection: a two-row prompt buffer with a
/// **visible bar** cursor at the input column of the prompt line. A real
/// producer reports its emulator's effective cursor like this each frame;
/// here the position / shape / visibility are set explicitly so the
/// snapshot consumer can witness the whole [`GridCursor`].
fn cursor_buffer() -> GridBuffer {
    let default = |c: char| TermCell::new(c.to_string(), TermColor::Default, TermColor::Default);
    GridBuffer::new(CURSOR_COLS, CURSOR_ROWS)
        // Row 0 — a label line (exactly 8 glyphs), so the cursor on row 1
        // is provably not at the home row.
        .with_row(0, "line one".chars().map(default))
        // Row 1 — the prompt; the bar cursor waits at the input column.
        .with_row(1, "$ ".chars().map(default))
        .with_cursor(GridCursor::new(
            CURSOR_AT.0,
            CURSOR_AT.1,
            CursorShape::Bar,
            true,
        ))
}

/// Tag of the R976 wide-char grid, and its placement + extent.
const WIDE_TAG: &str = "htg_wide";
const WIDE_POS: (u32, u32) = (400, 680);
/// `8 × 2` cells at the `8×16` baseline metric → `64 × 32` px.
const WIDE_COLS: u16 = 8;
const WIDE_ROWS: u16 = 2;
const WIDE_SIZE: (u32, u32) = (64, 32);

// CJK ideographs / fullwidth Latin occupy two terminal columns. Kept as
// `\u{..}` escapes per the non-ASCII-literal rule (raw glyphs in docs only):
//   世 U+4E16, 界 U+754C (East Asian Wide); Ａ U+FF21 (Fullwidth Latin A).
const CJK_SHI: &str = "\u{4e16}";
const CJK_JIE: &str = "\u{754c}";
const FW_A: &str = "\u{ff21}";

/// Build the R976 wide-char projection: each wide cluster is a head cell
/// (marked [`TermCell::wide`]) plus a continuation [`TermCell::trailer`].
/// A real producer marks the cells its emulator computed as wide; here the
/// two known-wide forms (CJK ideograph, fullwidth Latin) are marked
/// explicitly so the snapshot consumer can witness the head / trailer
/// run structure and the glyph-to-column mapping.
fn wide_buffer() -> GridBuffer {
    let shi = TermCell::new(CJK_SHI, TermColor::Indexed(1), TermColor::Default).wide();
    let jie = TermCell::new(CJK_JIE, TermColor::Indexed(2), TermColor::Default).wide();
    let fw_a = TermCell::new(FW_A, TermColor::Indexed(4), TermColor::Default).wide();
    let narrow = |c: &'static str| TermCell::new(c, TermColor::Default, TermColor::Default);
    GridBuffer::new(WIDE_COLS, WIDE_ROWS)
        // Row 0 — two wide CJK ideographs: 4 columns, each a head + trailer.
        .with_row(0, [shi.clone(), shi.trailer(), jie.clone(), jie.trailer()])
        // Row 1 — narrow A, a wide fullwidth Ａ (head + trailer), narrow B.
        .with_row(1, [narrow("A"), fw_a.clone(), fw_a.trailer(), narrow("B")])
}

/// Tag of the R977 alternate-screen grid, and its placement + extent.
const ALT_TAG: &str = "htg_alt";
const ALT_POS: (u32, u32) = (400, 740);
/// `8 × 2` cells at the `8×16` baseline metric → `64 × 32` px.
const ALT_COLS: u16 = 8;
const ALT_ROWS: u16 = 2;
const ALT_SIZE: (u32, u32) = (64, 32);
/// The app's cursor on the alternate screen — a visible block at home,
/// distinct from `htg_cursor`'s bar on the main screen (each screen
/// carries its own cursor, R975).
const ALT_CURSOR_AT: (u16, u16) = (0, 0);

/// Build the R977 alternate-screen projection: a tiny fullscreen-app view
/// (a file name line + a reverse-video status bar) tagged
/// [`ScreenKind::Alternate`], with its own block cursor. A real producer
/// reports the alternate screen when projecting a fullscreen app (`vim`,
/// `htop`, a pager); the other grids stay on the main screen.
fn alt_buffer() -> GridBuffer {
    let default = |c: char| TermCell::new(c.to_string(), TermColor::Default, TermColor::Default);
    // The status bar is reverse-video (reusing the R974 SGR attrs).
    let status = |c: char| default(c).with_attrs(CellAttrs::empty().with_reverse(true));
    GridBuffer::new(ALT_COLS, ALT_ROWS)
        // Row 0 — the open file (8 glyphs).
        .with_row(0, "file.txt".chars().map(default))
        // Row 1 — a reverse-video status bar (vim-style ruler, 8 glyphs).
        .with_row(1, " 1,1 Top".chars().map(status))
        .with_screen(ScreenKind::Alternate)
        .with_cursor(GridCursor::new(
            ALT_CURSOR_AT.0,
            ALT_CURSOR_AT.1,
            CursorShape::Block,
            true,
        ))
}

/// Tag of the R978 damage grid, and its placement + extent.
const DAMAGE_TAG: &str = "htg_damage";
const DAMAGE_POS: (u32, u32) = (400, 784);
/// `8 × 3` cells at the `8×16` baseline metric → `64 × 48` px.
const DAMAGE_COLS: u16 = 8;
const DAMAGE_ROWS: u16 = 3;
const DAMAGE_SIZE: (u32, u32) = (64, 48);
/// Per-row damage generations: three output lines, the last printed most
/// recently (the highest monotonic stamp).
const DAMAGE_GENS: (u64, u64, u64) = (10, 20, 30);

/// Build the R978 damage projection: three streamed output lines, each
/// stamped with a monotonic row generation (row 2, the newest line, has
/// the highest). A streaming client re-reads only the rows whose
/// generation exceeds its last-seen high-water mark — pinion stores the
/// producer's stamps, the client computes its own incremental delta.
fn damage_buffer() -> GridBuffer {
    let default = |c: char| TermCell::new(c.to_string(), TermColor::Default, TermColor::Default);
    GridBuffer::new(DAMAGE_COLS, DAMAGE_ROWS)
        .with_row(0, "line 1".chars().map(default))
        .with_row(1, "line 2".chars().map(default))
        .with_row(2, "line 3".chars().map(default))
        .with_row_generation(0, DAMAGE_GENS.0)
        .with_row_generation(1, DAMAGE_GENS.1)
        .with_row_generation(2, DAMAGE_GENS.2)
}

/// Build one absolutely-positioned grid: the absolute layout removes it
/// from flow and gives it exactly its own `Size`, so the layout pass
/// resolves a deterministic pixel `Rect` and the derived `(cols, rows)`
/// are stable regardless of window chrome. `font_px` pins the Vello glyph
/// size for a font-derived (measured) grid (R1002 SSOT) — `None` leaves the
/// paint adapter to fit a font into the cell.
fn grid(
    tag: &'static str,
    metric: CellMetric,
    pos: (u32, u32),
    size: (u32, u32),
    font_px: Option<u32>,
) -> Scene {
    let mut node = TextGridNode::new(metric).with_tag(tag).with_layout(
        LayoutStyle::new()
            .with_absolute_position(pos.0, pos.1)
            .with_size(Size::px(size.0, size.1)),
    );
    if let Some(px) = font_px {
        node = node.with_font_size_px(px);
    }
    Scene::TextGrid(node)
}

/// view-fn (§6.3): pure sync `() -> Scene`. A plain surface-filled root
/// (tagged [`ROOT_TAG`]) holding the two geometry grids.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    // R1003 §5.36 — the measured-metric grid consumes the view-time font seam:
    // when the shell has seeded a provider (windowed run), measure the real
    // monospace cell at `MEASURED_FONT_PX` and pin that size so the grid is
    // snug (cell_w == advance). Headless (no provider — the unit tests, RPC),
    // `measured_monospace_cell` returns None and we fall back to the
    // producer-picked `MEASURED_CELL` on the fit path.
    let (measured, measured_font) = match pinion_core::measured_monospace_cell(MEASURED_FONT_PX) {
        Some(cell) => (cell, Some(MEASURED_FONT_PX)),
        None => (
            CellMetric::new(MEASURED_CELL.0, MEASURED_CELL.1)
                .expect("fallback monospace metric is non-zero"),
            None,
        ),
    };

    // The R973 content grid: an `8×16` baseline grid carrying the cell
    // projection. Sized to derive exactly `CONTENT_COLS × CONTENT_ROWS`.
    let content = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(CONTENT_TAG)
            .with_cells(content_buffer())
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(CONTENT_POS.0, CONTENT_POS.1)
                    .with_size(Size::px(CONTENT_SIZE.0, CONTENT_SIZE.1)),
            ),
    );

    // The R974 SGR-attribute grid (also 8×16 baseline), carrying the
    // attribute projection. Isolated from `htg_content` so the R973
    // colour demo stays regression-free.
    let attrs = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(ATTRS_TAG)
            .with_cells(attrs_buffer())
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(ATTRS_POS.0, ATTRS_POS.1)
                    .with_size(Size::px(ATTRS_SIZE.0, ATTRS_SIZE.1)),
            ),
    );

    // The R975 cursor grid (8×16 baseline), carrying a prompt projection
    // with a visible bar cursor. Isolated from the other grids so their
    // demos stay regression-free (they keep the default hidden cursor).
    let cursor = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(CURSOR_TAG)
            .with_cells(cursor_buffer())
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(CURSOR_POS.0, CURSOR_POS.1)
                    .with_size(Size::px(CURSOR_SIZE.0, CURSOR_SIZE.1)),
            ),
    );

    // The R976 wide-char grid (8×16 baseline), carrying a projection with
    // wide CJK / fullwidth clusters (head + trailer). Isolated from the
    // other grids so their demos stay regression-free (all-narrow).
    let wide = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(WIDE_TAG)
            .with_cells(wide_buffer())
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(WIDE_POS.0, WIDE_POS.1)
                    .with_size(Size::px(WIDE_SIZE.0, WIDE_SIZE.1)),
            ),
    );

    // The R977 alternate-screen grid (8×16 baseline): a fullscreen-app
    // projection tagged ScreenKind::Alternate, with its own block cursor.
    // Every other grid stays on the main screen (the additive default).
    let alt = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(ALT_TAG)
            .with_cells(alt_buffer())
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(ALT_POS.0, ALT_POS.1)
                    .with_size(Size::px(ALT_SIZE.0, ALT_SIZE.1)),
            ),
    );

    // The R978 damage grid (8×16 baseline): three streamed output lines
    // carrying per-row monotonic damage generations. The other grids leave
    // every row at the default generation 0 (additive).
    let damage = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(DAMAGE_TAG)
            .with_cells(damage_buffer())
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(DAMAGE_POS.0, DAMAGE_POS.1)
                    .with_size(Size::px(DAMAGE_SIZE.0, DAMAGE_SIZE.1)),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![
            grid(DEFAULT_TAG, CellMetric::DEFAULT, DEFAULT_POS, DEFAULT_SIZE, None),
            grid(MEASURED_TAG, measured, MEASURED_POS, MEASURED_SIZE, measured_font),
            content,
            attrs,
            cursor,
            wide,
            alt,
            damage,
        ])
        .with_tag(ROOT_TAG)
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface))),
    )
}

struct TextGridView;

impl WidgetCore for TextGridView {
    type State = ();
    type Event = ();

    /// Display-only: the only addressable anchor is the no-op
    /// [`StubExternal`] at [`ROOT_TAG`]. The grids paint on Vello (R991);
    /// there is no interaction yet (no input routing into the grid).
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-textgrid (R972 §5.41 cell-native TextGrid scaffold)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display-only (geometry scaffold, no widget state)".to_string()
    }
}

impl WidgetA11y for TextGridView {
    /// No a11y nodes yet. The cell data model is read via the AI-first
    /// `scene/snapshot` path (`grid_rows`); the screen-reader a11y tree (a
    /// `grid` / `treegrid` role with per-cell nodes) is the remaining
    /// per-cell a11y slice — returning empty is the honest state.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        Vec::new()
    }
}

impl WidgetView for TextGridView {
    type Renderer = HelloTextGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TextGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::Rect;
    use pinion_core::CellWidth;

    /// The derived dims are a pure function of `rect` + metric — the
    /// R969 layout-derived SSOT. The running shell fills `rect` via the
    /// layout pass; here we set it directly to pin the winsize floor.
    #[test]
    fn default_grid_derives_80x24_from_its_rect() {
        let mut g = TextGridNode::new(CellMetric::DEFAULT);
        g.rect = Rect::new(DEFAULT_POS.0, DEFAULT_POS.1, DEFAULT_SIZE.0, DEFAULT_SIZE.1);
        assert_eq!(g.cell_metric(), CellMetric::DEFAULT);
        assert_eq!(g.cols(), 80);
        assert_eq!(g.rows(), 24);
    }

    #[test]
    fn measured_grid_derives_40x20_from_its_rect() {
        let metric = CellMetric::new(MEASURED_CELL.0, MEASURED_CELL.1).expect("non-zero");
        let mut g = TextGridNode::new(metric);
        g.rect = Rect::new(MEASURED_POS.0, MEASURED_POS.1, MEASURED_SIZE.0, MEASURED_SIZE.1);
        assert_eq!(g.cols(), 40); // 360 / 9
        assert_eq!(g.rows(), 20); // 360 / 18
    }

    #[test]
    fn empty_rect_has_zero_dims() {
        // Before the layout pass fills `rect`, a fresh grid is 0×0 — the
        // winsize is strictly layout-derived (no speculative default).
        let g = TextGridNode::new(CellMetric::DEFAULT);
        assert_eq!((g.cols(), g.rows()), (0, 0));
    }

    /// Read the `htg_measured` grid's pinned Vello font size out of a built
    /// scene — `Some` on the R1003 seam path (a provider measured the cell),
    /// `None` on the headless fallback (fit path).
    fn measured_grid_font_size(scene: &Scene) -> Option<u32> {
        let Scene::Container(root) = scene else {
            panic!("view root is a Container");
        };
        root.children.iter().find_map(|child| match child {
            Scene::TextGrid(n) if n.tag.as_deref() == Some(MEASURED_TAG) => n.font_size_px(),
            _ => None,
        })
    }

    /// R1003 §5.36 — when a font-metrics provider is seeded on the scope (the
    /// windowed shell), `view` takes the seam path: it measures the real cell
    /// and pins `MEASURED_FONT_PX`, so the grid is snug (`cell_w == advance`).
    /// The example's forcing consumer of the seam.
    #[test]
    fn measured_grid_takes_seam_path_when_provider_present() {
        use pinion_core::{CellMetric, MonospaceMetrics, Owner};
        // A minimal stand-in provider (no pinion-text dep needed): any
        // non-None measurement drives `view` onto the seam path.
        struct FakeMetrics;
        impl MonospaceMetrics for FakeMetrics {
            fn monospace_cell(&self, font_size_px: u32) -> Option<CellMetric> {
                CellMetric::new(font_size_px / 2, font_size_px)
            }
        }
        let owner = Owner::new();
        owner.provide_monospace_metrics(std::rc::Rc::new(FakeMetrics));
        let scene = owner.run(|| view((), &Frame::new()));
        assert_eq!(
            measured_grid_font_size(&scene),
            Some(MEASURED_FONT_PX),
            "seam path must pin the measured font size",
        );
    }

    /// R1003 §5.36 — with no provider (headless / RPC / this unit test),
    /// `measured_monospace_cell` returns None and `view` falls back to the
    /// producer-picked `MEASURED_CELL` on the fit path (no pinned font size).
    #[test]
    fn measured_grid_falls_back_without_provider() {
        let scene = pinion_core::Owner::new().run(|| view((), &Frame::new()));
        assert_eq!(measured_grid_font_size(&scene), None, "fallback is the fit path");
    }

    #[test]
    fn view_root_carries_the_paint_root_tag() {
        let scene = pinion_core::Owner::new().run(|| view((), &Frame::new()));
        assert!(scene.contains_tag(ROOT_TAG));
        assert!(scene.contains_tag(DEFAULT_TAG));
        assert!(scene.contains_tag(MEASURED_TAG));
        assert!(scene.contains_tag(CONTENT_TAG));
        assert!(scene.contains_tag(ATTRS_TAG));
        assert!(scene.contains_tag(CURSOR_TAG));
        assert!(scene.contains_tag(WIDE_TAG));
        assert!(scene.contains_tag(ALT_TAG));
        assert!(scene.contains_tag(DAMAGE_TAG));
    }

    #[test]
    fn damage_buffer_stamps_increasing_row_generations() {
        let b = damage_buffer();
        assert_eq!((b.cols(), b.rows()), (DAMAGE_COLS, DAMAGE_ROWS));
        assert_eq!(b.row_generation(0), Some(DAMAGE_GENS.0));
        assert_eq!(b.row_generation(1), Some(DAMAGE_GENS.1));
        assert_eq!(b.row_generation(2), Some(DAMAGE_GENS.2));
        // The newest output line (row 2) has the highest generation.
        assert!(b.row_generation(2).unwrap() > b.row_generation(0).unwrap());

        // A client whose high-water mark is row 0's generation re-reads
        // only the newer rows — the incremental-update model.
        let changed: Vec<u16> = (0..b.rows())
            .filter(|&r| b.row_generation(r).unwrap_or(0) > DAMAGE_GENS.0)
            .collect();
        assert_eq!(changed, vec![1, 2]);
    }

    #[test]
    fn alt_buffer_is_the_alternate_screen_with_its_own_cursor() {
        let b = alt_buffer();
        assert_eq!((b.cols(), b.rows()), (ALT_COLS, ALT_ROWS));
        // The headline: this projection is the alternate screen.
        assert_eq!(b.screen(), ScreenKind::Alternate);

        // It carries its own cursor — a visible block at home, distinct
        // from htg_cursor's bar on the main screen.
        let cur = b.cursor();
        assert_eq!((cur.col, cur.row), ALT_CURSOR_AT);
        assert_eq!(cur.shape, CursorShape::Block);
        assert!(cur.visible);

        // Row 0 is the file line; row 1 is a reverse-video status bar.
        assert_eq!(b.cell(0, 0).unwrap().cluster, "f");
        assert!(!b.cell(0, 0).unwrap().attrs.reverse); // content not reversed
        assert!(b.cell(0, 1).unwrap().attrs.reverse); // status bar reversed
        assert_eq!(b.cell(1, 1).unwrap().cluster, "1");

        // And the default-constructed (main) buffer contrasts.
        assert_eq!(GridBuffer::new(1, 1).screen(), ScreenKind::Main);
    }

    #[test]
    fn wide_buffer_pairs_each_wide_cluster_with_a_trailer() {
        let b = wide_buffer();
        assert_eq!((b.cols(), b.rows()), (WIDE_COLS, WIDE_ROWS));

        // Row 0 — two wide CJK ideographs, each head (col even) + trailer.
        assert_eq!(b.cell(0, 0).unwrap().cluster, CJK_SHI);
        assert_eq!(b.cell(0, 0).unwrap().width, CellWidth::Wide);
        assert_eq!(b.cell(1, 0).unwrap().width, CellWidth::Trailer);
        assert_eq!(b.cell(1, 0).unwrap().cluster, ""); // trailer carries no glyph
        // The trailer shares the head's colour (so the bg paints across).
        assert_eq!(b.cell(1, 0).unwrap().fg, b.cell(0, 0).unwrap().fg);
        assert_eq!(b.cell(2, 0).unwrap().cluster, CJK_JIE);
        assert_eq!(b.cell(3, 0).unwrap().width, CellWidth::Trailer);

        // Row 1 — narrow A, wide fullwidth Ａ (head + trailer), narrow B.
        assert_eq!(b.cell(0, 1).unwrap().width, CellWidth::Narrow);
        assert_eq!(b.cell(1, 1).unwrap().cluster, FW_A);
        assert_eq!(b.cell(1, 1).unwrap().width, CellWidth::Wide);
        assert_eq!(b.cell(2, 1).unwrap().width, CellWidth::Trailer);
        assert_eq!(b.cell(3, 1).unwrap().cluster, "B");
        assert_eq!(b.cell(3, 1).unwrap().width, CellWidth::Narrow);
    }

    #[test]
    fn cursor_buffer_is_8x2_with_a_visible_bar_at_the_input_column() {
        let b = cursor_buffer();
        assert_eq!((b.cols(), b.rows()), (CURSOR_COLS, CURSOR_ROWS));

        // The cursor: a visible bar at the prompt's input column (col 2,
        // row 1) — non-zero on both axes.
        let cur = b.cursor();
        assert_eq!((cur.col, cur.row), CURSOR_AT);
        assert_eq!(cur.shape, CursorShape::Bar);
        assert!(cur.visible);
        // It sits within the 8×2 grid.
        assert!(cur.col < b.cols() && cur.row < b.rows());

        // Row 0 is the 8-glyph label; row 1 is the prompt. The cursor is
        // on row 1, provably not the home row.
        assert_eq!(b.cell(0, 0).unwrap().cluster, "l");
        assert_eq!(b.cell(0, 1).unwrap().cluster, "$");
        assert_eq!(b.cell(1, 1).unwrap().cluster, " ");
        // The input cell under the cursor is still blank (the cursor is
        // grid state, not a cell mutation).
        assert_eq!(b.cell(2, 1), Some(&TermCell::blank()));
    }

    #[test]
    fn scaffold_grids_carry_the_default_hidden_cursor() {
        // The colour / attrs / geometry grids do not set a cursor, so each
        // carries the default (hidden, home, block) — proving the cursor
        // is opt-in per projection.
        assert_eq!(content_buffer().cursor(), GridCursor::default());
        assert_eq!(attrs_buffer().cursor(), GridCursor::default());
        assert!(!content_buffer().cursor().visible);
    }

    #[test]
    fn attrs_buffer_is_8x2_with_one_flag_per_cell() {
        let b = attrs_buffer();
        assert_eq!((b.cols(), b.rows()), (ATTRS_COLS, ATTRS_ROWS));

        // Row 0 — one SGR flag per cell, in declaration order.
        assert!(b.cell(0, 0).unwrap().attrs.bold);
        assert!(b.cell(2, 0).unwrap().attrs.italic);
        assert!(b.cell(5, 0).unwrap().attrs.reverse);
        assert!(b.cell(7, 0).unwrap().attrs.strikethrough);
        // Each cell carries exactly one flag (bold cell is not also italic).
        assert!(!b.cell(0, 0).unwrap().attrs.italic);

        // Row 1 — combinations + reverse with stored (unswapped) colours.
        let combo = b.cell(0, 1).unwrap();
        assert!(combo.attrs.bold && combo.attrs.italic);
        let rev = b.cell(2, 1).unwrap();
        assert!(rev.attrs.reverse);
        assert_eq!(rev.fg, TermColor::Indexed(1)); // stored, not swapped
        assert_eq!(rev.bg, TermColor::Indexed(15));
    }

    #[test]
    fn content_buffer_is_16x4_with_every_color_form() {
        let b = content_buffer();
        assert_eq!((b.cols(), b.rows()), (CONTENT_COLS, CONTENT_ROWS));

        // Row 0 — ANSI 16-colour bar: each cell's bg is its column index.
        for i in 0..CONTENT_COLS {
            let idx = u8::try_from(i).expect("< 16");
            assert_eq!(b.cell(i, 0).unwrap().bg, TermColor::Indexed(idx));
            assert_eq!(b.cell(i, 0).unwrap().fg, TermColor::Default);
        }

        // Row 1 — default-coloured label.
        assert_eq!(b.cell(0, 1).unwrap().cluster, "D");
        assert_eq!(b.cell(6, 1).unwrap().cluster, "t");
        assert_eq!(b.cell(0, 1).unwrap().fg, TermColor::Default);
        assert_eq!(b.cell(7, 1).unwrap(), &TermCell::blank()); // trailing blank

        // Row 2 — truecolor foregrounds.
        assert_eq!(
            b.cell(0, 2).unwrap().fg,
            TermColor::Rgb(Color::rgb(0xff, 0x00, 0x00))
        );
        assert_eq!(b.cell(1, 2).unwrap().cluster, "G");

        // Row 3 — mixed indexed.
        assert_eq!(b.cell(0, 3).unwrap().fg, TermColor::Indexed(15));
        assert_eq!(b.cell(0, 3).unwrap().bg, TermColor::Indexed(4));
        assert_eq!(b.cell(2, 3).unwrap().fg, TermColor::Indexed(232));
    }

    #[test]
    fn empty_grids_carry_no_cell_projection() {
        // The two scaffold grids stay empty (geometry-only), so the R972
        // demo's assertions are regression-free.
        let g = TextGridNode::new(CellMetric::DEFAULT);
        assert!(g.cells().is_empty());
    }
}
