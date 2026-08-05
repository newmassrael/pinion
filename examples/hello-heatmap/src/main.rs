// R1008 §5.16 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-heatmap` — R1408 §5.41 — a **sequential-magnitude heatmap over
//! [`Scene::TextGrid`]** with a **bare-hover cell inspector**.
//!
//! A fixed `12 x 8` intensity matrix (values `0..=100`) renders as a grid of
//! squarish colour cells: each data cell fills `6 x 3` character cells whose
//! background is the value mapped through a **sequential ramp** — one ramp,
//! `surface → accent` (the low end blends into the surface, the high end is the
//! saturated accent — the dataviz single-hue magnitude rule; never a rainbow).
//! Each cell also prints its value centred in a **maximum-contrast ink** (chosen
//! per cell from the background's luminance), so identity is never colour-alone —
//! the accessibility floor a heatmap needs — and a legend strip shows the ramp
//! end to end.
//!
//! ## The novel thing: a bare-hover cell inspector
//!
//! Moving the pointer over the grid — **with no button held** — reveals the
//! cell under it: the hovered data cell reverse-videos (R974) and a status line
//! names its `(row, col)` and value. This is the third consumer of
//! [`External::wants_hover_move`] (after `hello-hyperlink`'s link highlight and
//! `hello-crosshair`'s chart crosshair), and — the reason it is built now — the
//! **third consumer of the rect-fraction → cell hit-test**, which forced the
//! lift of [`CellMetric::frac_to_cell`] (the `frac_to_px` + `px_to_cell`
//! composite `hello-hex-dump` and `hello-hyperlink` had reconstructed by hand).
//! The router forwards a `[0, 1]` fraction over the grid's rect on every bare
//! hover; `pointer_move` turns it into a cell in one call, and a `PointerLeave`
//! (the router boundary `send`) clears it, so the inspector is alive only over
//! the grid.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! The highlight is cell *attributes*, so `scene/snapshot` reports the hovered
//! block in `grid_rows` (its runs carry `reverse`). The [`HeatmapOracle`]
//! exposes the model so a client reads it without a pixel: `query rows` / `cols`
//! / `max_value`, `query hovered_row` / `hovered_col` / `hovered_value`, the
//! `invoke value_at "r,c"` oracle over the whole matrix, and the no-pixel drive
//! `intervene hovered_cell "r,c"` (or `Null` to clear). See
//! `tools/demos/r1408_heatmap.py`.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ColorScale, readable_ink};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, TextGridNode, TextNode};
use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{
    CellAttrs, CellMetric, Frame, GridBuffer, Scene, TermCell, TermColor, WidgetCore,
};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloHeatmapRenderer, HelloHeatmapRendererError);

const THEME_TAG: &str = "app";
/// The grid's paint tag AND the primary [`HeatmapOracle`]'s registration tag
/// (addressed over RPC as `/external/<field>`).
const GRID_TAG: &str = "heatmap";

// --- The matrix ------------------------------------------------------------

/// Data columns / rows of the intensity matrix.
const COLS: usize = 12;
const ROWS: usize = 8;
/// The value range's top — a cell holds `0..=MAX`.
const MAX: u8 = 100;

/// Character cells per data cell — `6 x 3` reads as a squarish block at the
/// 8x16 [`CellMetric::DEFAULT`] cell.
const CELL_CW: usize = 6;
const CELL_CH: usize = 3;

/// The character-grid extent: `COLS * CELL_CW` cols x `ROWS * CELL_CH` rows.
const GCOLS: usize = COLS * CELL_CW;
const GROWS: usize = ROWS * CELL_CH;

/// The grid's pixel extent, locked to the char grid at the 8x16 cell so
/// `frac_to_cell` resolves exactly (`GCOLS * 8`, `GROWS * 16`).
#[allow(
    clippy::cast_possible_truncation,
    reason = "GCOLS * 8 is a small const"
)]
const GRID_W: u32 = (GCOLS * 8) as u32;
#[allow(
    clippy::cast_possible_truncation,
    reason = "GROWS * 16 is a small const"
)]
const GRID_H: u32 = (GROWS * 16) as u32;
const GRID_POS: (u32, u32) = (24, 44);

const WIN_W: u32 = GRID_W + 48;
const WIN_H: u32 = GRID_H + 108;

const TITLE_FONT_PX: u32 = 16;
const STATUS_FONT_PX: u32 = 12;

/// The fixed intensity matrix. A deterministic pattern (a rising diagonal ridge
/// plus a hotspot near the centre) so the ramp shows a legible gradient rather
/// than noise. Values are `0..=MAX`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "small indices to f32 and a clamped 0..=1 value * MAX back to u8"
)]
fn matrix() -> [[u8; COLS]; ROWS] {
    let mut m = [[0u8; COLS]; ROWS];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            // A diagonal ramp (0 at top-left → MAX at bottom-right) plus a
            // gaussian-ish hotspot around (row 2, col 8).
            let diag = (r + c) as f32 / (ROWS + COLS - 2) as f32; // 0..1
            let dr = r as f32 - 2.0;
            let dc = c as f32 - 8.0;
            let hot = (-(dr * dr + dc * dc) / 6.0).exp(); // 0..1 peak at (2,8)
            let v = (0.62 * diag + 0.55 * hot).min(1.0);
            *cell = (v * f32::from(MAX)).round() as u8;
        }
    }
    m
}

// --- The colour ramp -------------------------------------------------------

/// The resolved ramp + the two ink colours, from the theme.
///
/// R1436 — the ramp is now a [`ColorScale`], and the ink choice is
/// [`readable_ink`]; this demo hand-rolled both (plus its own sRGB EOTF and
/// WCAG contrast pair) before `pinion-chart` carried them. The colours are
/// unchanged: the ramp is still the theme's `surface → accent`, because a
/// heatmap wired into the app's chrome is a legitimate use of a two-stop
/// sequential scale — what moved to the crate is the machinery, not the taste.
#[derive(Debug, Clone)]
struct Palette {
    /// The sequential ramp, `surface → accent`. A faint floor is added at the
    /// low end (see [`cell_color`]) so even a `0` cell stays visible.
    ramp: ColorScale,
    /// Ink on a light (low) cell.
    ink_dark: Color,
    /// Ink on a saturated (high) cell.
    ink_light: Color,
}

/// The sequential cell colour for `value`: the ramp's low → high direction
/// (dataviz magnitude rule), with a `0.15` floor so the lowest cells are not
/// invisible against the surface. The floor is this demo's aesthetic choice,
/// which is why it lives here and not in the scale; the interpolation itself is
/// the crate's (linear-light, via [`ColorScale::sample`]).
fn cell_color(value: u8, p: &Palette) -> Color {
    let t = 0.15 + 0.85 * (f32::from(value) / f32::from(MAX));
    p.ramp.sample(t)
}

/// The ink for a cell: whichever theme ink (`ink_dark` / `ink_light`) has the
/// higher WCAG contrast against the cell's background — COMPUTED per cell, not
/// assumed. A fixed light-half / dark-half threshold fails a `surface → accent`
/// ramp whose darkest step (the accent) is only mid-luminance, so dark ink is
/// the more legible choice across almost the whole ramp; only the top cells (a
/// near-accent background) flip to the light ink.
fn cell_ink(value: u8, p: &Palette) -> Color {
    readable_ink(cell_color(value, p), p.ink_dark, p.ink_light)
}

// --- The hovered cell ------------------------------------------------------

/// The hovered data cell `(row, col)`, or `None` when the pointer is off the
/// grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hover {
    row: usize,
    col: usize,
}

/// The data cell a character `(col, row)` addresses — a char cell divides into
/// its `CELL_CW x CELL_CH` data cell. `None` past the matrix.
fn cell_at_char(gcol: usize, grow: usize) -> Option<Hover> {
    let col = gcol / CELL_CW;
    let row = grow / CELL_CH;
    (col < COLS && row < ROWS).then_some(Hover { row, col })
}

/// Read the hovered cell from the primary [`HeatmapOracle`] in the state scene.
fn read_hover(scene: &Scene) -> Option<Hover> {
    let intro = scene
        .find_external_with_tag(GRID_TAG)?
        .handle
        .introspect()?;
    let field = |name: &str| match intro.query(name)? {
        IntrospectValue::Int(i) => usize::try_from(i).ok(),
        _ => None,
    };
    Some(Hover {
        row: field("hovered_row")?,
        col: field("hovered_col")?,
    })
}

// --- The view --------------------------------------------------------------

/// The value string a cell prints, right-of-centre in its `CELL_CW`-wide block.
fn value_label(value: u8) -> String {
    let width = CELL_CW;
    format!("{value:^width$}")
}

/// Build the heatmap [`GridBuffer`]: every data cell fills its `CELL_CW x
/// CELL_CH` char cells with the ramp background, prints its value on the middle
/// char row, and reverse-videos when it is the hovered cell.
fn heatmap_buffer(m: &[[u8; COLS]; ROWS], hover: Option<Hover>, p: &Palette) -> GridBuffer {
    let mut buf = GridBuffer::new(
        u16::try_from(GCOLS).unwrap_or(0),
        u16::try_from(GROWS).unwrap_or(0),
    );
    for grow in 0..GROWS {
        let mut cells = vec![TermCell::new(" ", TermColor::Default, TermColor::Default); GCOLS];
        for (gcol, slot) in cells.iter_mut().enumerate() {
            let Some(cell) = cell_at_char(gcol, grow) else {
                continue;
            };
            let value = m[cell.row][cell.col];
            let bg = TermColor::Rgb(cell_color(value, p));
            let fg = TermColor::Rgb(cell_ink(value, p));
            let hovered = hover == Some(cell);
            let attrs = CellAttrs::empty().with_reverse(hovered);
            // The value prints on the middle char row of the data cell; the
            // other rows are the bare colour block.
            let on_value_row = grow % CELL_CH == CELL_CH / 2;
            let glyph = if on_value_row {
                let label = value_label(value);
                label.chars().nth(gcol % CELL_CW).unwrap_or(' ').to_string()
            } else {
                " ".to_string()
            };
            *slot = TermCell::new(glyph, fg, bg).with_attrs(attrs);
        }
        buf = buf.with_row(u16::try_from(grow).unwrap_or(0), cells);
    }
    buf
}

/// The legend strip's track — a thin gradient bar under the grid.
fn legend_track() -> Rect {
    Rect::new(GRID_POS.0, GRID_POS.1 + GRID_H + 10, GRID_W, 12)
}

/// Build the legend gradient as its own `1 x GCOLS` [`GridBuffer`] — the ramp
/// sampled left (0) to right (MAX).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "a 0..=1 sample * MAX is a non-negative in-range u8"
)]
fn legend_buffer(p: &Palette) -> GridBuffer {
    let mut cells = Vec::with_capacity(GCOLS);
    for gcol in 0..GCOLS {
        let value = ((gcol as f32 / (GCOLS - 1) as f32) * f32::from(MAX)).round() as u8;
        cells.push(TermCell::new(
            " ",
            TermColor::Default,
            TermColor::Rgb(cell_color(value, p)),
        ));
    }
    GridBuffer::new(u16::try_from(GCOLS).unwrap_or(0), 1).with_row(0, cells)
}

/// view-fn (§6.3): pure sync mapping of the hovered cell to the scene.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(hover: Option<Hover>, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let surface = theme.resolve(ColorRole::Surface);
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let accent = theme.resolve(ColorRole::Accent);
    let palette = Palette {
        ramp: ColorScale::sequential(vec![surface, accent]),
        ink_dark: on_surface,
        ink_light: theme.resolve(ColorRole::OnAccent),
    };

    let m = matrix();
    let grid = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_tag(GRID_TAG)
            .with_cells(heatmap_buffer(&m, hover, &palette))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(GRID_POS.0, GRID_POS.1)
                    .with_size(Size::px(GRID_W, GRID_H))
                    .with_focusable(true),
            ),
    );

    let legend = Scene::TextGrid(
        TextGridNode::new(CellMetric::DEFAULT)
            .with_cells(legend_buffer(&palette))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(legend_track().x, legend_track().y)
                    .with_size(Size::px(GRID_W, 16)),
            ),
    );

    let title = Scene::Text(
        TextNode::styled(
            "Heatmap — hover a cell to inspect its value",
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(24, 14)),
    );

    let status_text = hover.map_or_else(
        || format!("hover a cell  |  ramp: 0 → {MAX} (surface → accent)"),
        |h| {
            format!(
                "row {} col {} = {}  |  ramp: 0 → {MAX} (surface → accent)",
                h.row, h.col, m[h.row][h.col],
            )
        },
    );
    let status = Scene::Text(
        TextNode::styled(
            status_text,
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(on_surface_muted),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(24, WIN_H - 24)),
    );

    Scene::Container(
        ContainerNode::new(vec![grid, legend, title, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

// --- The oracle (primary external) -----------------------------------------

/// Parse a `"row,col"` pair (the `value_at` / `hovered_cell` wire form).
fn parse_cell(s: &str) -> Option<(usize, usize)> {
    let (a, b) = s.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// The heatmap's matrix + the hovered cell, as the interactive primary
/// external. The read half is an introspectable oracle (an AI client reads the
/// matrix + the hover without a pixel); the write half is the **bare-hover
/// inspector** — a plain hover moves the cursor, `PointerLeave` clears it — plus
/// the no-pixel `intervene hovered_cell`.
#[derive(Debug, Clone)]
struct HeatmapOracle {
    m: [[u8; COLS]; ROWS],
    hover: Option<Hover>,
}

impl HeatmapOracle {
    fn new() -> Self {
        Self {
            m: matrix(),
            hover: None,
        }
    }

    fn value_of(&self, h: Hover) -> u8 {
        self.m[h.row][h.col]
    }
}

impl External for HeatmapOracle {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// The inspector follows the BARE hover (no button), so the external opts
    /// into hover-move but NOT pointer capture (the `hello-crosshair` shape).
    fn wants_hover_move(&self) -> bool {
        true
    }

    fn wants_pointer_capture(&self) -> bool {
        false
    }

    /// Each hover move delivers a `[0, 1]` rect fraction; the lifted
    /// [`CellMetric::frac_to_cell`] turns it into a character cell, and
    /// [`cell_at_char`] maps that to the data cell under the pointer (or `None`
    /// off the matrix).
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let (gcol, grow) = CellMetric::DEFAULT.frac_to_cell(x_rel, y_rel, GRID_W, GRID_H);
        self.hover = cell_at_char(usize::from(gcol), usize::from(grow));
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for HeatmapOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("rows", "int"),
                    SchemaField::new("cols", "int"),
                    SchemaField::new("max_value", "int"),
                    SchemaField::new("hovered_row", "int"),
                    SchemaField::new("hovered_col", "int"),
                    SchemaField::new("hovered_value", "int"),
                    SchemaField::new("value_at", "string"),
                    SchemaField::new("hovered_cell", "string"),
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let int = |n: usize| IntrospectValue::Int(i64::try_from(n).unwrap_or(0));
        match path {
            "rows" => Some(int(ROWS)),
            "cols" => Some(int(COLS)),
            "max_value" => Some(int(usize::from(MAX))),
            "hovered_row" => Some(self.hover.map_or(IntrospectValue::Null, |h| int(h.row))),
            "hovered_col" => Some(self.hover.map_or(IntrospectValue::Null, |h| int(h.col))),
            "hovered_value" => Some(self.hover.map_or(IntrospectValue::Null, |h| {
                int(usize::from(self.value_of(h)))
            })),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The AI-first, no-pixel hover channel: a "row,col" string sets the
            // hovered cell; Null clears it.
            "hovered_cell" => match value {
                IntrospectValue::Null => {
                    self.hover = None;
                    Ok(())
                }
                IntrospectValue::Text(ref s) => {
                    let (row, col) = parse_cell(s).ok_or(InterveneError::TypeMismatch)?;
                    if row >= ROWS || col >= COLS {
                        return Err(InterveneError::out_of_range(format!(
                            "no cell ({row}, {col}) in this heatmap (it is {ROWS} x {COLS})"
                        )));
                    }
                    self.hover = Some(Hover { row, col });
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "rows" | "cols" | "max_value" | "hovered_row" | "hovered_col" | "hovered_value" => {
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
            // A "row,col" cell -> its value ("v"), or "none" off the matrix —
            // the AI reads the whole matrix without a pixel.
            "value_at" => match args {
                IntrospectValue::Text(ref s) => {
                    let (row, col) = parse_cell(s).ok_or(InvokeError::TypeMismatch)?;
                    let out = if row < ROWS && col < COLS {
                        self.m[row][col].to_string()
                    } else {
                        "none".to_owned()
                    };
                    Ok(IntrospectValue::Text(out))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // The router's pointer boundary events (R884): a leave / cancel ends
            // the hover (the inspector is alive only over the grid).
            "send" => {
                if let IntrospectValue::Text(ref name) = args {
                    if matches!(name.as_str(), "PointerLeave" | "PointerCancel") {
                        self.hover = None;
                    }
                }
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- The binding -----------------------------------------------------------

struct HeatmapView;

impl WidgetCore for HeatmapView {
    /// The hovered data cell (the primary external).
    type State = Option<Hover>;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(HeatmapOracle::new())
    }

    fn tag() -> &'static str {
        GRID_TAG
    }

    fn read_state(scene: &Scene) -> Option<Hover> {
        read_hover(scene)
    }

    fn view(state: Option<Hover>, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-heatmap (R1408 §5.41 bare-hover cell inspector)"
    }

    /// The inspector is hover- and RPC-driven; no keyboard channel.
    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        false
    }

    fn fmt_state_log(state: &Option<Hover>) -> String {
        state.map_or_else(
            || "hover: none".to_owned(),
            |h| format!("hover: row {} col {}", h.row, h.col),
        )
    }
}

impl WidgetA11y for HeatmapView {
    /// The heatmap as a `group` whose value names the hovered cell + its value,
    /// so an AT reads the inspector without the pixels.
    fn access_node(state: &Option<Hover>, _focused: Option<&str>) -> Vec<AccessNode> {
        let m = matrix();
        let value = state.map_or_else(
            || "no cell hovered".to_owned(),
            |h| format!("row {} column {} is {}", h.row, h.col, m[h.row][h.col]),
        );
        vec![
            AccessNode::new(GRID_TAG, AriaRole::Group)
                .with_name("Intensity heatmap")
                .with_value(AccessValue::Text(value)),
        ]
    }
}

impl WidgetView for HeatmapView {
    type Renderer = HelloHeatmapRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<HeatmapView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::test_fixtures::assert_out_of_range_saying;
    // The WCAG pair the ramp assertions measure with — crate helpers now
    // (R1436), used only by these tests.
    use pinion_chart::{contrast_ratio, relative_luminance};

    fn c(n: usize) -> u16 {
        u16::try_from(n).unwrap()
    }

    #[test]
    fn grid_pixels_match_the_cell_layout() {
        // The pixel extent is locked to the char-grid counts at the 8x16 cell.
        assert_eq!(GRID_W, u32::try_from(GCOLS).unwrap() * 8);
        assert_eq!(GRID_H, u32::try_from(GROWS).unwrap() * 16);
        assert_eq!(GCOLS, COLS * CELL_CW);
        assert_eq!(GROWS, ROWS * CELL_CH);
    }

    #[test]
    fn matrix_is_in_range_and_rises_to_the_far_corner() {
        let m = matrix();
        for row in &m {
            for &v in row {
                assert!(v <= MAX, "every value is within 0..=MAX");
            }
        }
        // The diagonal ramp makes the far corner brighter than the origin (the
        // global peak is the hotspot near (2, 8), not a corner).
        assert!(
            m[ROWS - 1][COLS - 1] > m[0][0],
            "far corner brighter than the origin",
        );
        // The hotspot is the brightest cell.
        assert!(m[2][8] >= m[ROWS - 1][COLS - 1], "the hotspot is the peak");
    }

    #[test]
    fn cell_at_char_maps_char_cells_to_data_cells() {
        // The first data cell owns char cells [0,CELL_CW) x [0,CELL_CH).
        assert_eq!(cell_at_char(0, 0), Some(Hover { row: 0, col: 0 }));
        assert_eq!(
            cell_at_char(CELL_CW - 1, CELL_CH - 1),
            Some(Hover { row: 0, col: 0 }),
        );
        // The next char column crosses into data column 1.
        assert_eq!(cell_at_char(CELL_CW, 0), Some(Hover { row: 0, col: 1 }));
        // The last data cell.
        assert_eq!(
            cell_at_char(GCOLS - 1, GROWS - 1),
            Some(Hover {
                row: ROWS - 1,
                col: COLS - 1,
            }),
        );
        // Past the matrix.
        assert_eq!(cell_at_char(GCOLS, 0), None);
        assert_eq!(cell_at_char(0, GROWS), None);
    }

    /// The real light-theme ramp shape: a light surface low end, a saturated
    /// (mid-luminance) accent high end.
    fn test_palette() -> Palette {
        Palette {
            ramp: ColorScale::sequential(vec![
                Color::rgb(0xff, 0xff, 0xff), // surface
                Color::rgb(0x19, 0x76, 0xd2), // accent
            ]),
            ink_dark: Color::rgb(0x1a, 0x1a, 0x1a),
            ink_light: Color::rgb(0xff, 0xff, 0xff),
        }
    }

    #[test]
    fn cell_color_ramp_is_monotonic_in_luminance() {
        // The sequential magnitude ordering: luminance falls as the value rises
        // (the low end is the surface, the high end the saturated accent), so
        // the ramp reads as one ordered scale. Sampled at a coarse step so the
        // per-sample luminance drop dwarfs `u8` channel quantization (a strict
        // per-unit check could false-fail on a rounding tie — zero-flake).
        let p = test_palette();
        let mut prev = relative_luminance(cell_color(0, &p));
        for v in (20..=MAX).step_by(20) {
            let cur = relative_luminance(cell_color(v, &p));
            assert!(cur < prev, "luminance falls as the value rises (by {v})");
            prev = cur;
        }
    }

    #[test]
    fn cell_ink_picks_the_higher_contrast_ink() {
        // At every ramp step the chosen ink is the one with the higher WCAG
        // contrast against the cell background — computed, not a fixed threshold.
        let p = test_palette();
        for v in 0..=MAX {
            let bg = cell_color(v, &p);
            let ink = cell_ink(v, &p);
            let other = if ink == p.ink_dark {
                p.ink_light
            } else {
                p.ink_dark
            };
            assert!(
                contrast_ratio(bg, ink) >= contrast_ratio(bg, other),
                "cell_ink picks the higher-contrast ink (at {v})",
            );
        }
        // On this light-ended ramp the dark ink wins across the low + middle of
        // the range (the mid-luminance accent only makes the light ink win near
        // the very top) — the fix for the old fixed-threshold bug.
        assert_eq!(cell_ink(0, &p), p.ink_dark, "the lightest cell -> dark ink");
        assert_eq!(cell_ink(50, &p), p.ink_dark, "a mid cell -> dark ink");
    }

    #[test]
    fn pointer_move_resolves_the_cell_under_the_cursor() {
        let mut o = HeatmapOracle::new();
        assert_eq!(o.hover, None, "no hover before any pointer");
        // A hover over data cell (row 0, col 0)'s centre resolves to it.
        o.pointer_move(0.0, 0.0);
        assert_eq!(o.hover, Some(Hover { row: 0, col: 0 }));
        // The far corner resolves to the last data cell.
        o.pointer_move(1.0, 1.0);
        assert_eq!(
            o.hover,
            Some(Hover {
                row: ROWS - 1,
                col: COLS - 1,
            }),
        );
        // A PointerLeave clears it.
        o.invoke("send", IntrospectValue::Text("PointerLeave".into()))
            .unwrap();
        assert_eq!(o.hover, None, "leave clears the hover");
    }

    #[test]
    fn oracle_reports_the_model_and_the_hover() {
        let mut o = HeatmapOracle::new();
        assert_eq!(o.query("rows"), Some(IntrospectValue::Int(8)));
        assert_eq!(o.query("cols"), Some(IntrospectValue::Int(12)));
        assert_eq!(o.query("max_value"), Some(IntrospectValue::Int(100)));
        assert_eq!(o.query("hovered_row"), Some(IntrospectValue::Null));
        assert_eq!(o.query("nope"), None);

        // value_at reads the whole matrix; it matches the internal matrix.
        let m = matrix();
        assert_eq!(
            o.invoke("value_at", IntrospectValue::Text("3,5".into())),
            Ok(IntrospectValue::Text(m[3][5].to_string())),
        );
        assert_eq!(
            o.invoke("value_at", IntrospectValue::Text("9,9".into())),
            Ok(IntrospectValue::Text("none".into())),
            "off the matrix",
        );

        // intervene hovered_cell drives the hover with no pixel; the queries
        // reflect it.
        o.intervene("hovered_cell", IntrospectValue::Text("2,8".into()))
            .unwrap();
        assert_eq!(o.query("hovered_row"), Some(IntrospectValue::Int(2)));
        assert_eq!(o.query("hovered_col"), Some(IntrospectValue::Int(8)));
        assert_eq!(
            o.query("hovered_value"),
            Some(IntrospectValue::Int(i64::from(m[2][8]))),
        );
        // Null clears; out-of-range and read-only are rejected.
        o.intervene("hovered_cell", IntrospectValue::Null).unwrap();
        assert_eq!(o.query("hovered_row"), Some(IntrospectValue::Null));
        assert_out_of_range_saying(
            &o.intervene("hovered_cell", IntrospectValue::Text("8,0".into())),
            "no cell (8, 0) in this heatmap",
        );
        assert_eq!(
            o.intervene("rows", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly),
        );
    }

    #[test]
    fn view_reverse_videos_the_hovered_cell_only() {
        let hover = Hover { row: 2, col: 8 };
        let scene = pinion_core::Owner::new().run(|| view(Some(hover), &Frame::new()));
        let Scene::Container(root) = &scene else {
            panic!("container");
        };
        let grid = root
            .children
            .iter()
            .find_map(|ch| match ch {
                Scene::TextGrid(n) if n.tag.as_deref() == Some(GRID_TAG) => Some(n.cells().clone()),
                _ => None,
            })
            .expect("the heatmap grid");
        // A char cell inside the hovered data cell is reversed…
        let (gc, gr) = (8 * CELL_CW, 2 * CELL_CH);
        assert!(
            grid.cell(c(gc), c(gr)).unwrap().attrs.reverse,
            "the hovered cell is reversed",
        );
        // …and a char cell in a neighbour is not.
        assert!(
            !grid.cell(c(0), c(0)).unwrap().attrs.reverse,
            "an un-hovered cell is not reversed",
        );
    }

    #[test]
    fn view_carries_the_grid_tag() {
        let scene = pinion_core::Owner::new().run(|| view(None, &Frame::new()));
        assert!(scene.contains_tag(GRID_TAG));
    }
}
