//! R707 §5.50 — backend-agnostic data-table paint composition.
//!
//! Composes the data-grid visual: a header band (one
//! [`columnheader`](pinion_core::scene) cell per column) above a body of
//! data rows, each row a horizontal strip of cells. The selected row is
//! washed with an accent tint; odd rows carry a subtle
//! [`ColorRole::SurfaceContainer`] zebra stripe; the active-descendant
//! cell gets the shell's R694 focus-ring treatment.
//!
//! ## Naming
//!
//! Mirrors [`crate::datepicker`]: a [`TableStyle`] carrier struct with
//! [`TableStyle::m3`] defaults and a [`view_table`] fn that produces a
//! [`Scene`] fragment the binding's outer view-fn wraps in its root
//! container.
//!
//! ## Structure (tags)
//!
//! - The header band is tagged `"<tag>_hrow"` (the header `row` node);
//!   each column header inside it is a presentational cell tagged
//!   `"<tag>_ch<col>"` (column `0..cols`) so the binding's `access_node`
//!   walker can attach the header `row` + `columnheader` nodes.
//! - Each data row is a strip tagged `"<tag>_row<row>"` so the
//!   `access_node` walker can attach the `row` node (and so the strip
//!   carries the selected / zebra fill).
//! - Each real cell is a hit-test target tagged `"<tag>#<row>_<col>"`
//!   (the R51.41 composite paint convention → the
//!   `InputRouter` `'#'`-split routes a
//!   click on cell `(r, c)` to the table's `"<r>_<c>:<EventName>"` send).

use std::ops::Range;
use std::rc::Rc;

use pinion_core::Scene;
use pinion_core::composite_tag::{GridSendKey, GridTag};
use pinion_core::scene::{
    ContainerNode, ImageNode, Rect, ScrollAxis, ScrollNode, TextNode, TextRole,
};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, Fit, FlexDirection, ImageStyle, JustifyContent,
    LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::column_widths::{columns_width_before, visible_columns};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::virtual_list::{VisibleWindow, compute_visible_range, content_height};

use crate::virtual_list::{assemble_windowed_flex, uniform_slots};

/// R707 §5.50 — Material-3 data-table paint dimensions. Mirrors the
/// [`crate::datepicker::DatePickerStyle`] carrier pattern so binding
/// callers see a uniform `Style` surface across the widget catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableStyle {
    /// Uniform column width in logical pixels (default 120). Per-column
    /// widths / column resizing are deferred axes; v1 tiles columns at a
    /// fixed width.
    pub col_width: u32,
    /// Data-row height in logical pixels (default 36 ≈ M3 dense list
    /// row).
    pub row_height: u32,
    /// Header-band height in logical pixels (default 40).
    pub header_height: u32,
    /// Cell label font size in logical pixels (default 14 ≈ M3
    /// `body-medium`).
    pub label_size_px: u32,
    /// Header label font size in logical pixels (default 14 ≈ M3
    /// `title-small`).
    pub header_size_px: u32,
    /// Horizontal text inset inside each cell in logical pixels
    /// (default 12).
    pub cell_pad_x: u32,
    /// Outer block padding on all four sides in logical pixels
    /// (default 8).
    pub block_pad: u32,
    /// Outer block corner radius in logical pixels (default 12 = M3
    /// large shape token).
    pub corner_radius: u32,
    /// R786 §5.27 — width of the column-resize grabber zone at each header
    /// cell's right edge, in logical pixels (default 8). The grabber occupies
    /// this much of the cell's trailing edge (the sort-click label area takes
    /// the rest); a press inside it starts a live resize drag. Only drawn when
    /// the grid is `resizable` (a width model + per-column
    /// [`ColumnResizeExternal`](pinion_core::widgets::column_widths::ColumnResizeExternal)s
    /// are wired); a non-resizable grid renders the full-width header exactly as
    /// before.
    pub resize_handle_w: u32,
    /// (R1020 §5.39) Keyboard focus stop. When `true`, [`view_table`] /
    /// [`view_virtual_table`] mark the table's tag-carrying outer Container
    /// `.with_focusable(true)` so the scene-derived §5.39 enumeration collects
    /// its tag as a Tab stop. Default `true` (R1030 fail-safe, web
    /// native-element model): a table is a Tab stop by default; a decorative or
    /// modal-scoped table opts out with `.with_focusable(false)`. Mirrors
    /// [`ButtonStyle::focusable`](crate::button::ButtonStyle::focusable).
    pub focusable: bool,
    /// R1535 §5.27 — side length, in logical pixels, of the square a
    /// [`CellDecoration::Swatch`] paints (default 10). Qt's default delegate
    /// sizes a decoration from the view's `iconSize`; this is the grid-wide
    /// equivalent, held here with the other cell dimensions so a decorated
    /// column cannot pick a size the rest of the grid does not know about.
    pub decoration_px: u32,
    /// R1535 §5.27 — gap, in logical pixels, between a cell's decoration and
    /// its display text (default 8). Only spent when the cell **has** a
    /// decoration, so an undecorated cell's label sits exactly where it did
    /// before R1535.
    pub decoration_gap_px: u32,
}

/// R786 §5.27 — visible divider line width inside the resize grabber, in
/// logical pixels (an M3 hairline). The grabber is a [`TableStyle::resize_handle_w`]
/// transparent hit zone with this thin [`ColorRole::Outline`] line hugging its
/// right edge — the painted column boundary the user grabs.
const RESIZE_DIVIDER_W: u32 = 1;

/// R1535 §5.27 — corner radius of a [`CellDecoration::Swatch`], in logical
/// pixels. A softened **square**, not a disc: Qt paints a `QColor` decoration
/// as a filled rectangle, and a square reads as a sample of the colour where a
/// disc reads as a status light — this role carries whichever the model means,
/// so the shape must not editorialise.
const SWATCH_RADIUS: u32 = 2;

impl TableStyle {
    /// R707 §5.50 — Material-3 data-table defaults.
    #[must_use]
    pub const fn m3() -> Self {
        Self {
            col_width: 120,
            row_height: 36,
            header_height: 40,
            label_size_px: 14,
            header_size_px: 14,
            cell_pad_x: 12,
            block_pad: 8,
            corner_radius: 12,
            resize_handle_w: 8,
            focusable: true,
            decoration_px: 10,
            decoration_gap_px: 8,
        }
    }

    /// (R1020 §5.39 / R1030) Override this table's keyboard focus stop (default `true`).
    /// A grid binding sets this true; a decorative / modal-scoped table leaves
    /// it false. See [`Self::focusable`].
    #[must_use]
    pub const fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }
}

impl Default for TableStyle {
    fn default() -> Self {
        Self::m3()
    }
}

/// R707 §5.50 — the tabular data a [`view_table`] call renders.
///
/// Groups the column headers and the row-major cell text into one unit
/// so the paint signature stays under the readable argument budget
/// (mirror of [`crate::datepicker::DisplayedMonth`]). Cell text is
/// borrowed: the binding owns an immutable dataset and forwards slices.
#[derive(Clone, Copy)]
pub struct TableData<'a> {
    /// Column header labels; `headers.len()` is the column count.
    pub headers: &'a [&'a str],
    /// Row-major cell text **in visual (display) order**. Each inner slice
    /// is one visual row; a cell beyond a row's length renders blank.
    pub rows: &'a [&'a [&'a str]],
    /// R730 §5.40 — the data-row index for each visual row (the table's
    /// [`order()`](pinion_core::widgets::table::TableExternal::order)
    /// permutation): `row_ids[v]` is the data row whose cells are in
    /// `rows[v]`. Cells / strips tag by `row_ids[v]` and the selection /
    /// per-row state lookups index by it, so the table's data-indexed
    /// selection survives a sort with no remap. An empty slice (or a
    /// too-short one) falls back to identity (`row_ids[v] == v`) — the
    /// unsorted R707 behaviour.
    pub row_ids: &'a [usize],
    /// R1536 §5.27 — the `Qt::DecorationRole` accessor, the eager surface's
    /// peer of [`GridModel::decoration`]: `decoration(index)` returns the mark
    /// beside that cell's text, or `None`.
    ///
    /// It is an accessor here — not a matrix beside `rows` — because a
    /// decoration is a per-cell answer and the virtualized grid already asks
    /// for it that way; two shapes for one role would be the divergence class,
    /// not a style choice. `None` (the field's `Default`) is every pre-R1536
    /// caller, painting byte-identically.
    ///
    /// Until R1536 this surface answered the role with nothing at all, which
    /// left two cell-paint contracts in one tree — the shape R1530 left on the
    /// header axis and R1532 on the delegate axis.
    pub decoration: Option<&'a dyn Fn(CellIndex) -> Option<CellDecoration>>,
}

impl core::fmt::Debug for TableData<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TableData")
            .field("headers", &self.headers)
            .field("rows", &self.rows)
            .field("row_ids", &self.row_ids)
            .field("decoration", &self.decoration.map(|_| "<fn>"))
            .finish()
    }
}

/// R952 §5.38 — the selection inputs for [`view_table`], bundled to keep it
/// under the argument budget (the [`TableData`] precedent for grouping related
/// inputs). The two selection models a grid can carry, one struct: per-row
/// (`SelectRows`) + cell range (`SelectItems`).
#[derive(Clone, Copy)]
pub struct TableSelection<'a> {
    /// Per-row selection bitmap, indexed by **data** row (parallel to
    /// `row_states`): a `true` row strip is washed with the accent tint. One
    /// path serves single-select (one bit) and R735 multi-select.
    pub rows: &'a [bool],
    /// R952 — the selected cell rectangle `(row0, col0, row1, col1)` (data
    /// coords, inclusive) from
    /// [`TableExternal::cell_selection_bounds`](pinion_core::widgets::table::TableExternal::cell_selection_bounds),
    /// or `None`. Drawn as a bordered accent overlay (the spreadsheet cell
    /// selection), distinct from the per-row `rows` wash.
    pub cells: Option<(usize, usize, usize, usize)>,
}

/// R707 §5.50 — row-strip fill for `state` + `selected` + zebra parity
/// via the M3 state-layer overlay matrix.
///
/// A selected row washes [`ColorRole::Surface`] toward
/// [`ColorRole::Accent`] (0.16 — an M3 "selected container" tint, since
/// the palette has no dedicated `secondaryContainer` role); an
/// unselected odd row carries a subtle [`ColorRole::SurfaceContainer`]
/// zebra stripe; an unselected even row is transparent (shows the block
/// fill). Hover (0.08) / pressed (0.12) overlays lerp toward
/// [`ColorRole::OnSurface`]; disabled fades toward
/// [`ColorRole::Surface`] (0.38). Identical weights to
/// [`crate::datepicker::day_cell_fill`] so the catalog's interactive
/// surfaces share one state-layer language.
#[must_use]
pub fn row_fill(theme: &Theme, state: RadioState, selected: bool, row_index: usize) -> Color {
    let base = if selected {
        theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), 0.16)
    } else if row_index % 2 == 1 {
        theme.resolve(ColorRole::SurfaceContainer)
    } else {
        // Transparent so an unselected even row shows the block fill.
        Color::rgba(0, 0, 0, 0)
    };
    crate::state_layer::state_layer(base, state, theme)
}

/// R1536 §5.27 — the [`CellDecoration::Icon`] node: the source drawn at the
/// same square a [`CellDecoration::Swatch`] occupies.
///
/// `Fit::Contain` rather than the [`ImageStyle`]
/// default `Fill`: an icon that is not square must keep its aspect, because a
/// stretched glyph is a different glyph.
fn icon_node(source: &str, side: u32, tag: &str) -> Scene {
    Scene::Image(
        ImageNode::styled(
            source.to_string(),
            Rect::default(),
            ImageStyle::default().with_fit(Fit::Contain),
        )
        .with_tag(tag.to_string())
        .with_layout(decoration_layout(side)),
    )
}

/// R1536 §5.27 — the layout every decoration node takes, whichever
/// [`CellDecoration`] arm produced it.
///
/// One function because the three rules are the contract, not per-arm taste:
/// the declared square; **`flex-shrink: 0`**, so a decoration keeps its size in
/// a tight cell and the *text* is what gives way (Qt draws at `iconSize` and
/// elides the label — measured before this: a 10px swatch painted 6px in a 75px
/// column); and **pointer-transparency**, so the click target stays the cell
/// even though the mark carries a tag (independent axes — `pinion_overlay`'s
/// focus ring is both).
fn decoration_layout(side: u32) -> LayoutStyle {
    LayoutStyle::new()
        .with_size(Size::px(side, side))
        .with_flex_shrink(0.0)
        .with_pointer_transparent(true)
}

/// R1535 §5.27 — the [`CellDecoration::Swatch`] node: a filled square of
/// [`TableStyle::decoration_px`] a side, laid out before the cell's label.
///
/// **Tagged and pointer-transparent**, which are independent axes (R1536): the
/// tag makes the mark *addressable* — `scene/snapshot` can be asked for cell
/// `(r, c)`'s decoration by name instead of walking the cell's children by
/// position — while pointer-transparency keeps the click *target* the cell, as
/// [`VirtualTableData::delegate`]'s own bars do. R1535 gave it neither, on the
/// reasoning that a tag would shadow the cell; `pinion_overlay`'s focus ring is
/// tagged and pointer-transparent, so that reasoning was wrong.
fn swatch_node(color: Color, side: u32, tag: &str) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(tag.to_string())
            .with_style(BoxStyle::filled(color).with_corner_radius(SWATCH_RADIUS))
            .with_layout(decoration_layout(side)),
    )
}

/// R952 §5.38 — alpha of the cell range selection wash (a faint accent fill so
/// the selected cells read as a group without obscuring their text — the
/// legibility reason the highlight is a border + low-alpha wash, not an opaque
/// fill).
const CELL_SEL_WASH_ALPHA: u8 = 0x33;
/// R952 §5.38 — width (px) of the cell range selection border (the crisp
/// accent rectangle around the selection, the spreadsheet affordance).
const CELL_SEL_BORDER_W: u32 = 2;

/// R952 §5.38 — the cell range selection highlight: one accent-bordered,
/// faintly-accent-washed rectangle absolutely positioned over the selected
/// cells (`bounds` = `(row0, col0, row1, col1)`, data coords, inclusive). A
/// border + low-alpha wash (not an opaque fill) keeps the cell text legible —
/// the spreadsheet selection affordance. Positioned in the table container's
/// content box: the header band occupies the first [`TableStyle::header_height`],
/// then each row is [`TableStyle::row_height`] tall; columns are the cumulative
/// `widths`. Tagged `"<tag>_cellsel"` (presentational, inert to hit routing) so
/// an AI client reads the selection's pixel rect back from the paint scene. The
/// caller renders this only for a visually-contiguous (unsorted) selection — a
/// sorted view keeps the data-indexed selection RPC-readable but omits the
/// rectangle, since the selected cells are no longer contiguous on screen.
fn cell_selection_overlay(
    tag: &str,
    bounds: (usize, usize, usize, usize),
    widths: &[u32],
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let (r0, c0, r1, c1) = bounds;
    // `absolute_position` is relative to the container's padding box (CSS
    // model), so the cells — laid out inside `block_pad` of padding — start at
    // `block_pad`; the overlay adds it back to line up with them.
    let x: u32 = style.block_pad + widths.iter().take(c0).sum::<u32>();
    let w: u32 = widths.iter().skip(c0).take(c1 - c0 + 1).sum();
    let y =
        style.block_pad + style.header_height + u32::try_from(r0).unwrap_or(0) * style.row_height;
    let h = u32::try_from(r1 - r0 + 1).unwrap_or(0) * style.row_height;
    let accent = theme.resolve(ColorRole::Accent);
    let wash = accent.with_alpha(CELL_SEL_WASH_ALPHA);
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(format!("{tag}_cellsel"))
            .with_style(BoxStyle::filled(wash).with_border(Border::new(accent, CELL_SEL_BORDER_W)))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(x, y)
                    .with_size(Size::px(w, h))
                    // R954 §5.38 — the highlight is a passive overlay drawn on
                    // top of the cells; it must be pointer-transparent so a
                    // click on a cell *inside* the current selection still
                    // routes to that cell (a `SelectItems` grid re-selects it),
                    // rather than the overlay swallowing the hit.
                    .with_pointer_transparent(true),
            ),
    )
}

/// R785 §5.27 — resolve the per-column widths to a full-length vector (one
/// entry per column). `col_widths` supplies the
/// [`ColumnWidths`](pinion_core::widgets::column_widths::ColumnWidths) model;
/// a column beyond its length (or `None` entirely — every grid before R785,
/// and the eager [`view_table`]) falls back to the uniform
/// [`TableStyle::col_width`]. Resolving once here lets the header / row
/// builders take a single `&[u32]` slice rather than threading the `Option`
/// plus a separate column count.
fn resolve_widths(cols: usize, col_widths: Option<&[u32]>, style: &TableStyle) -> Vec<u32> {
    (0..cols)
        .map(|c| {
            col_widths
                .and_then(|w| w.get(c))
                .copied()
                .unwrap_or(style.col_width)
        })
        .collect()
}

/// R785 §5.27 — a column's identity + width for the header-cell builder,
/// bundled so [`header_cell`] stays under the argument budget (the R775
/// [`VirtualTableData`] precedent for grouping related inputs).
///
/// R786 §5.27 — `resizable` rides along (uniform across the header, but the
/// per-cell builder is where it is consumed): when set, [`header_cell`] reserves
/// [`TableStyle::resize_handle_w`] of the cell's trailing edge for a resize
/// grabber and tags it `"<tag>_ch<col>#resize"`.
#[derive(Clone, Copy)]
struct ColCell {
    col: usize,
    width: u32,
    resizable: bool,
}

/// R786 §5.27 — the resize grabber painted at a header cell's trailing edge: a
/// [`TableStyle::resize_handle_w`]-wide hit zone tagged `"<tag>_ch<col>#resize"`
/// with a [`RESIZE_DIVIDER_W`] [`ColorRole::Outline`] hairline hugging its right
/// edge (the visible column boundary the user grabs). The `'#'`-split routes a
/// press here to the [`ColumnResizeExternal`](pinion_core::widgets::column_widths::ColumnResizeExternal)
/// registered at the primary `"<tag>_ch<col>"`.
fn resize_handle(tag: &str, col: usize, outline: Color, style: &TableStyle) -> Scene {
    let divider = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_style(BoxStyle::filled(outline))
            .with_layout(
                LayoutStyle::new().with_size(Size::px(RESIZE_DIVIDER_W, style.header_height)),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![divider])
            .with_tag(format!("{}#resize", GridTag::col_header(tag, col)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    // The hairline hugs the right edge = the column boundary; the
                    // rest of the zone is a transparent grab margin to its left.
                    .with_justify(JustifyContent::End)
                    .with_size(Size::px(style.resize_handle_w, style.header_height)),
            ),
    )
}

/// R730 — one clickable column header. The outer container keeps the
/// presentational `"<tag>_ch<col>"` tag (the binding's `access_node`
/// walker attaches the `columnheader` node + bounds there, unchanged
/// since R707); it wraps an inner container carrying the **composite**
/// `"<tag>#h<col>"` tag so a click on the header routes through the
/// R51.42 `'#'`-split to the table's `"h<col>:<EventName>"` sort wire.
/// When `col` is the active sort key the inner row also paints a sort
/// glyph (▲ ascending / ▼ descending) after the label.
fn header_cell(
    tag: &str,
    click_tag: &str,
    cell: ColCell,
    label: &str,
    sort: Option<(usize, bool)>,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let ColCell {
        col,
        width,
        resizable,
    } = cell;
    let fg = theme.resolve(ColorRole::OnSurface);
    // R786 — reserve the trailing grabber width from the clickable / label area
    // so the cell's total width stays `width` (the data cells' width). A
    // non-resizable header is byte-identical to the R785 layout (full `width`).
    let content_w = if resizable {
        width.saturating_sub(style.resize_handle_w)
    } else {
        width
    };
    // R1536 — a column header's label is the header's CONTENT, not decoration,
    // by the same rule the data cell's is. The sort glyph beside it IS
    // decoration and keeps its presentational role, which is exactly the
    // distinction `TextRole` was introduced (R51.81) to draw.
    let label_node = Scene::Text(TextNode::styled(
        label.to_string(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.header_size_px)
            .with_fg(fg),
    ));
    let mut inner_children = vec![label_node];
    // R886.1 — active-column decision + glyph through the two SSOTs
    // (`col_sort_dir` / `glyph::sort_glyph`); this site was one of the
    // five private copies of the pair.
    if let Some(glyph) =
        crate::glyph::sort_glyph(pinion_core::widgets::grid_sort::col_sort_dir(sort, col))
    {
        inner_children.push(Scene::Text(
            TextNode::styled(
                glyph.to_string(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(style.header_size_px)
                    .with_fg(fg),
            )
            .with_role(TextRole::Presentational),
        ));
    }
    let inner = Scene::Container(
        ContainerNode::new(inner_children)
            // R777.1 — the clickable header sub-key via the GridSendKey SSOT.
            // R778 — the click routes to `click_tag` (the sort anchor),
            // which may differ from the presentational `tag` (the a11y /
            // paint-root anchor) when sort is a separate coordinator.
            .with_tag(format!(
                "{click_tag}#{}",
                GridSendKey::Header { col }.encode()
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_gap(4)
                    .with_size(Size::px(content_w, style.header_height))
                    .with_padding(Rect::new(style.cell_pad_x, 0, style.cell_pad_x, 0)),
            ),
    );
    // R786 — the clickable/label content + (when resizable) the trailing
    // resize grabber, tiling the cell's full `width` (content_w + handle_w).
    let mut cell_children = vec![inner];
    if resizable {
        cell_children.push(resize_handle(
            tag,
            col,
            theme.resolve(ColorRole::Outline),
            style,
        ));
    }
    Scene::Container(
        ContainerNode::new(cell_children)
            .with_tag(GridTag::col_header(tag, col))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    )
}

/// R1523 §5.27 §5.45 — the width of the columns **outside** a windowed pane,
/// held as a leading + trailing pair.
///
/// When the column axis is windowed, a row builds only the cells the horizontal
/// viewport exposes — but it must still *occupy* the full content width, or the
/// layout pass would measure the shrunken row and publish a horizontal `max_x`
/// that cannot reach the un-built columns (the scroll would bound itself to the
/// window it came from). The pad is that missing extent, rendered as one empty
/// container on each side: the windowed cells then land at exactly the x they
/// would have had with every column built, and the row's natural width is
/// unchanged.
///
/// This is the canonical column-virtualization recipe — `TanStack` Table spells
/// the same pair `virtualPaddingLeft` / `virtualPaddingRight` — chosen over
/// absolutely-positioning each cell (the technique the *row* axis uses) for two
/// reasons: the row strip keeps its flex-row shape, so the eager [`view_table`]
/// and the windowed grid keep **one** row builder; and the natural width stays
/// correct without a second width sizer to keep in step with the pad.
///
/// [`NONE`](Self::NONE) is the un-windowed pane (both zero), which emits no
/// spacer nodes at all — so a grid narrower than its viewport paints a
/// byte-identical scene to the pre-R1523 one.
#[derive(Clone, Copy)]
struct ColumnPad {
    /// Total width of the columns before the window.
    lead: u32,
    /// Total width of the columns after the window.
    trail: u32,
}

impl ColumnPad {
    /// No columns outside the pane — every column is built.
    const NONE: Self = Self { lead: 0, trail: 0 };

    /// The pad around `window` for a grid whose columns are `widths`.
    fn around(widths: &[u32], window: &VisibleWindow) -> Self {
        let end = (window.first + window.count).min(widths.len());
        Self {
            lead: columns_width_before(widths, window.first),
            trail: widths[end..].iter().copied().sum(),
        }
    }

    /// The `[lead?, cells…, trail?]` child list for a windowed row of
    /// `cell_height`-tall cells. A zero-width side contributes no node.
    fn wrap(self, cells: Vec<Scene>, cell_height: u32) -> Vec<Scene> {
        let mut out = Vec::with_capacity(cells.len() + 2);
        out.extend(pad_node(self.lead, cell_height));
        out.extend(cells);
        out.extend(pad_node(self.trail, cell_height));
        out
    }
}

/// R1524 / R1535 — one pane's row: **both** per-cell roles for data row `row`,
/// [`CellIndex`]-addressed and asked once per column in `span`, in column order.
///
/// The single place the per-cell contract meets the per-row strip
/// [`data_row`] paints, shared by all three panes that paint one (the unsplit
/// grid, and the frozen split's pinned + scrolling panes) so none of them can
/// decide on its own to ask for a column it will not paint — which is the
/// defect R1524 closes, and it was three call sites wide.
///
/// R1535 added the decoration role, and it is asked **here**, beside the
/// display role, rather than through a second function of the same shape. The
/// two roles' answers are indexed into positionally by [`data_row`], so
/// fetching them separately would let a pane ask for the display role over one
/// column span and the decoration role over another and paint a cell with its
/// neighbour's mark. Asking both against one `span` in one pass makes that
/// misalignment unrepresentable instead of merely unlikely — and it is the
/// call shape Qt's `data(index, role)` has anyway: one index, every role.
///
/// This replaces R1523's `clamped`, which existed to tolerate a row builder
/// that returned fewer cells than the grid had columns. A per-cell contract
/// cannot express a short row, so the blank-cell rule is now `cell` returning
/// an empty `String` and there is no length to reconcile.
fn pane_cells(
    cell: &mut impl FnMut(CellIndex) -> String,
    decoration: &mut impl FnMut(CellIndex) -> Option<CellDecoration>,
    row: usize,
    span: Range<usize>,
) -> (Vec<String>, Vec<Option<CellDecoration>>) {
    span.map(|col| {
        let index = CellIndex { row, col };
        (cell(index), decoration(index))
    })
    .unzip()
}

/// R1530 — one pane's header labels: [`GridModel::header`] asked once per
/// column in `span`, in column order. The header-band peer of [`pane_cells`],
/// and the single place the per-section contract meets the slice
/// [`header_row`] paints, so neither the unsplit grid nor either frozen pane
/// can ask for a section it will not paint.
fn header_texts(header: &mut impl FnMut(usize) -> String, span: Range<usize>) -> Vec<String> {
    span.map(header).collect()
}

/// The shared [`spacer`](crate::spacer::spacer), or **nothing** when `width` is
/// 0 — so a grid whose columns fit its viewport emits no pad node at all and
/// paints a byte-identical scene to the pre-R1523 one.
fn pad_node(width: u32, height: u32) -> Option<Scene> {
    (width > 0).then(|| crate::spacer::spacer(width, height))
}

/// R786 §5.27 — the header's per-column layout: the resolved widths plus
/// whether each column carries a resize grabber. Bundled so [`header_row`] stays
/// under the argument budget (the [`ColCell`] / [`VirtualTableData`] precedent).
#[derive(Clone, Copy)]
struct ColumnLayout<'a> {
    widths: &'a [u32],
    resizable: bool,
    /// R1523 — the extent of the columns this pane windowed out.
    pad: ColumnPad,
    /// R859 — absolute table-column index of `widths[0]`. `0` for an
    /// unsplit grid (and the frozen pane); `frozen_cols` for the scrolled
    /// pane, so its header cells keep their original `"{tag}_ch{col}"`
    /// tags even though the slice starts mid-table.
    col_base: usize,
    /// R859 — the header-row container tag. `"{tag}_hrow"` for the
    /// unsplit / scrolled header; `"{tag}_fhrow"` for the frozen pane, so
    /// the two split panes never emit a duplicate container tag into the
    /// paint scene (hit-routing / introspection stay tag-unique).
    container_tag: &'a str,
}

/// R859 §5.27 — the per-pane column descriptor a [`data_row`] needs when a
/// frozen-column grid splits each row into a frozen-left and a scrolling
/// pane. Bundles the container tag + the absolute column base + the pane's
/// width slice so `data_row` stays under the argument budget (the
/// [`ColumnLayout`] precedent).
#[derive(Clone, Copy)]
struct RowPane<'a> {
    /// The data-row strip container tag: `"{tag}_row{id}"` for the
    /// unsplit / scrolled pane, `"{tag}_frow{id}"` for the frozen pane.
    container_tag: &'a str,
    /// Absolute table-column index of `widths[0]` (`0` for the frozen /
    /// unsplit pane, `frozen_cols` for the scrolled pane), so each cell
    /// keeps its original `GridSendKey::Cell { col }` send-wire tag.
    col_base: usize,
    /// This pane's per-column widths (the frozen or scrolled slice).
    widths: &'a [u32],
    /// R1523 — the extent of the columns this pane windowed out.
    pad: ColumnPad,
    /// R1532 — this pane's per-column paint delegates, aligned with
    /// [`Self::widths`] (so index `j` is absolute column `col_base + j`).
    /// `None` at a position takes the built-in [`text_cell_painter`].
    /// Resolved once per pane by `GridRender::painters`, because a column's
    /// delegate cannot vary by row.
    painters: &'a [Option<CellPainter<'a>>],
    /// R1535 — this **row**'s per-column `Qt::DecorationRole` answers, aligned
    /// with [`Self::widths`] (so index `j` is absolute column `col_base + j`).
    /// Unlike [`Self::painters`] this varies by row, which is the whole reason
    /// a decoration is a model role and not a delegate; it rides here because
    /// [`RowPane`] is already built per row (it carries the row's own
    /// container tag). An **empty** slice means "no cell in this row is
    /// decorated" — the eager [`view_table`], which exposes no decoration
    /// model, passes it.
    decorations: &'a [Option<CellDecoration>],
    /// R1532 — the palette, for a delegate that resolves its own roles. It
    /// travels with [`Self::painters`] rather than as its own argument
    /// because it exists here *for* them: the built-in painter takes its
    /// colour from the row's already-resolved `fg`.
    theme: &'a Theme,
}

/// The header band: one clickable `columnheader` cell per column ([
/// `header_cell`]) on a raised [`ColorRole::SurfaceContainerHigh`]
/// surface. `sort` drives the active column's sort glyph; `layout` supplies the
/// per-column widths + whether the columns are resizable (R786).
fn header_row(
    tag: &str,
    click_tag: &str,
    headers: &[&str],
    sort: Option<(usize, bool)>,
    layout: ColumnLayout<'_>,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let cells: Vec<Scene> = headers
        .iter()
        .enumerate()
        .map(|(i, label)| {
            // R859 — `widths` / `headers` are this pane's slice; the
            // absolute table column is `col_base + i` so the `_ch{col}`
            // tag + sort-glyph match stay anchored to the original index.
            let col = layout.col_base + i;
            let width = layout.widths.get(i).copied().unwrap_or(style.col_width);
            header_cell(
                tag,
                click_tag,
                ColCell {
                    col,
                    width,
                    resizable: layout.resizable,
                },
                label,
                sort,
                theme,
                style,
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(layout.pad.wrap(cells, style.header_height))
            // Tagged `"<tag>_hrow"` (or `"<tag>_fhrow"` for the R859
            // frozen pane) so the binding's `access_node` walker can
            // attach the header `row` node (and resolve its bounds); not a
            // composite `'#'` tag, so it is inert to hit routing.
            .with_tag(layout.container_tag.to_string())
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHigh),
            ))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    )
}

/// One data row: a horizontal strip tagged `"<tag>_row<row>"`, filled
/// `fill`, holding one cell per column tagged `"<tag>#<row>_<col>"` with
/// foreground `fg`. The caller precomputes `fill` ([`row_fill`]) and `fg`
/// from the row's interaction state so this stays under the argument
/// budget.
fn data_row(
    tag: &str,
    data_id: usize,
    cells_text: &[&str],
    fill: Color,
    fg: Color,
    pane: RowPane<'_>,
    style: &TableStyle,
) -> Scene {
    let cols = pane.widths.len();
    // R730 §5.40 — cells / strip are tagged by **data-row id** (not the
    // visual position), so a click on a sorted row routes to the right
    // data row's `"<data_id>_<col>"` send wire and the table's
    // data-indexed selection stays correct without a remap. When unsorted
    // (`data_id == visual`) the tags are identical to the R707 scheme.
    // R859 — `cells_text` / `pane.widths` are this pane's slice; the
    // absolute table column is `pane.col_base + j` so each cell keeps its
    // original `GridSendKey::Cell { col }` send-wire tag across the split.
    let cells: Vec<Scene> = (0..cols)
        .map(|j| {
            let col = pane.col_base + j;
            // R1532 — the render context this column's painter is given. The
            // built-in `text_cell_painter` is reached through the same call as
            // a custom one, so an undelegated column is not a separate path
            // that could drift from the delegated one.
            let render = CellRender {
                index: CellIndex { row: data_id, col },
                text: cells_text.get(j).copied().unwrap_or(""),
                // R1535 — an out-of-range index is the undecorated answer, the
                // same fallback the text takes; the eager caller's empty slice
                // reaches it for every column.
                decoration: pane.decorations.get(j).and_then(Option::as_ref),
                root: tag,
                width: pane.widths.get(j).copied().unwrap_or(style.col_width),
                height: style.row_height,
                fg,
                theme: pane.theme,
                style,
                // R777.1 — the cell sub-key via the GridSendKey SSOT.
                tag: &format!("{tag}#{}", GridSendKey::Cell { row: data_id, col }.encode()),
            };
            pane.painters
                .get(j)
                .copied()
                .flatten()
                .map_or_else(|| text_cell_painter(&render), |paint| paint(&render))
        })
        .collect();
    Scene::Container(
        ContainerNode::new(pane.pad.wrap(cells, style.row_height))
            .with_tag(pane.container_tag.to_string())
            .with_style(BoxStyle::filled(fill))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    )
}

/// Foreground color for a row's cells given its interaction `state`:
/// muted when disabled, the standard on-surface color otherwise.
fn row_fg(theme: &Theme, state: RadioState) -> Color {
    if matches!(state, RadioState::Disabled) {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::OnSurface)
    }
}

/// R707 §5.50 — compose the M3 data-table paint scene fragment.
///
/// # Arguments
///
/// - `tag` — composite paint-root tag; header cells are `"<tag>_ch<col>"`,
///   row strips `"<tag>_row<row>"`, and data cells `"<tag>#<row>_<col>"`.
/// - `data` — the [`TableData`] (column headers + row-major cell text);
///   paired with
///   [`TableExternal`](pinion_core::widgets::table::TableExternal)'s
///   `rows` / `cols` / `cell.<r>.<c>` introspect slots.
/// - `selection` — the [`TableSelection`] bundle: `rows` is the per-row
///   selection bitmap (indexed by **data** row, parallel to `row_states`; a
///   `true` strip washes accent — single- or R735 multi-select), and `cells`
///   is the R952 selected cell rectangle `(row0, col0, row1, col1)` (data
///   coords, inclusive) drawn as a bordered accent overlay over those cells
///   (the spreadsheet selection, rendered only for an unsorted view — see the
///   overlay note below), or `None`.
/// - `row_states` — per-row [`RadioState`] interaction projections,
///   indexed by row (the binding reads them from the table's per-row
///   `state.<r>` introspect slots). A row outside the slice bounds
///   defaults to [`RadioState::Idle`].
/// - `theme` — current [`Theme`] palette; the binding resolves it via
///   [`pinion_core::theme::use_theme`] and forwards.
/// - `style` — [`TableStyle`] dimension carrier.
///
/// # R730 sort
///
/// `data.row_ids` carries each visual row's data-row index (the table's
/// [`order()`](pinion_core::widgets::table::TableExternal::order)
/// permutation) so cells / strips tag by data id and the data-indexed
/// selection survives a sort with no remap (identity fallback when
/// empty). `sort` is the active sort key `(col, ascending)` for the
/// header glyph.
///
/// # Returns
///
/// A [`Scene::Container`] (Column) holding the header band and the data
/// rows, carrying `tag` on the outer block so the composite root is
/// paint-addressable.
#[must_use]
pub fn view_table(
    tag: &str,
    data: TableData<'_>,
    selection: TableSelection<'_>,
    row_states: &[RadioState],
    sort: Option<(usize, bool)>,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let row_selected = selection.rows;
    let cell_selection = selection.cells;
    let cols = data.headers.len();
    // The eager table is one combined coordinator (header + cells route to
    // the same `TableExternal`), so the clickable header anchor is `tag`.
    // Eager tables are uniform-width (no per-column model), so every column
    // resolves to `style.col_width`.
    let widths = resolve_widths(cols, None, style);
    // The eager table is uniform-width and not user-resizable (no width model);
    // the header keeps its full-width R707 layout.
    let hrow_tag = GridTag::header_row(tag);
    let header = header_row(
        tag,
        tag,
        data.headers,
        sort,
        ColumnLayout {
            widths: &widths,
            resizable: false,
            // The eager table has no viewport to window against — it builds
            // every row, so it builds every column.
            pad: ColumnPad::NONE,
            col_base: 0,
            container_tag: &hrow_tag,
        },
        theme,
        style,
    );
    let mut children: Vec<Scene> = Vec::with_capacity(data.rows.len() + 1);
    children.push(header);
    for (visual, cells_text) in data.rows.iter().enumerate() {
        // Data-row id for this visual position (identity fallback).
        let data_id = data.row_ids.get(visual).copied().unwrap_or(visual);
        let state = row_states.get(data_id).copied().unwrap_or(RadioState::Idle);
        let selected = row_selected.get(data_id).copied().unwrap_or(false);
        // Zebra parity is **visual** so the stripe pattern stays stable
        // across re-sorts; selection / state are **data-indexed**.
        let fill = row_fill(theme, state, selected, visual);
        let fg = row_fg(theme, state);
        let row_tag = GridTag::data_row(tag, data_id);
        let decorations: Vec<Option<CellDecoration>> = data.decoration.map_or_else(
            || vec![None; widths.len()],
            |ask| {
                (0..widths.len())
                    .map(|col| ask(CellIndex { row: data_id, col }))
                    .collect()
            },
        );
        children.push(data_row(
            tag,
            data_id,
            cells_text,
            fill,
            fg,
            RowPane {
                container_tag: &row_tag,
                col_base: 0,
                widths: &widths,
                pad: ColumnPad::NONE,
                // R1532 — the eager `view_table` exposes no delegate on its
                // own surface (`VirtualTableData` is the virtualized grid's
                // carrier), so every column takes the built-in text painter.
                // Recorded as carry: this leaves two cell-paint surfaces in
                // one tree, the shape R1530 left on the header axis.
                painters: &[],
                // R1536 — the eager surface answers the decoration role too,
                // through `TableData::decoration`. Asked once per painted cell
                // of this row, the same rule the virtualized grid follows, so
                // the two surfaces cannot disagree about when the role is
                // consulted.
                decorations: &decorations,
                theme,
            },
            style,
        ));
    }
    // R952 §5.38 — the cell range selection highlight, on top of the rows. Only
    // an unsorted view maps the data-coord rectangle to one contiguous screen
    // rectangle; a sorted view keeps the data-indexed selection RPC-readable but
    // omits the overlay (the selected cells are scattered across the sort).
    if let Some(bounds) = cell_selection {
        if sort.is_none() {
            children.push(cell_selection_overlay(tag, bounds, &widths, theme, style));
        }
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(tag.to_string())
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow))
                    .with_corner_radius(style.corner_radius),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    // (R1020 §5.39) Carry the binding's focus-stop opt-in onto
                    // the tag-carrying outer block so the scene-derived
                    // enumeration collects this table's tag as a Tab stop.
                    .with_focusable(style.focusable)
                    .with_padding(Rect::new(
                        style.block_pad,
                        style.block_pad,
                        style.block_pad,
                        style.block_pad,
                    )),
            ),
    )
}

/// R775 §5.27 — the "what to render" inputs for a [`view_virtual_table`],
/// paralleling [`TableData`] for the eager [`view_table`]. Bundling the
/// column + dataset descriptors keeps the assembly fn under the argument
/// budget; the per-row cell text comes from the `build_cells` closure
/// (only the windowed rows are built).
// `Debug` is hand-written (below) because the R998 `row_style` resolver is a
// `&dyn Fn`, which is `Copy` but not `Debug`.
#[derive(Clone, Copy)]
pub struct VirtualTableData<'a> {
    /// R1530 — total column count (Qt `QAbstractItemModel::columnCount`),
    /// decoupled from the rendered window count exactly as [`Self::item_count`]
    /// is on the row axis.
    ///
    /// Until R1530 this was `headers: &[&str]`, and the count was that slice's
    /// length. That welded the extent to the labels: a grid could only learn
    /// how many columns it had by being handed every one of their names, so a
    /// 200-column grid materialized 200 labels each frame to paint five. The
    /// row axis never had the defect — `item_count` has always been a number
    /// beside a windowed accessor — and the two axes now say the same thing the
    /// same way. The labels come from [`GridModel::header`], asked per section.
    pub column_count: usize,
    /// Total data-row count (decoupled from the rendered window count).
    pub item_count: usize,
    /// Rows built beyond the strict visible window on each side.
    pub overscan: usize,
    /// R778 §5.40 — the active sort key `(col, ascending)` for the header
    /// glyph, or `None` (no glyph). A display-only grid passes `None`.
    pub sort: Option<(usize, bool)>,
    /// R778 §5.40 — the anchor the clickable column headers route their
    /// pointer arc to (`"<sort_tag>#h<col>"`). `None` keeps the headers on
    /// the paint-root `tag` (the R777 default, where the selection
    /// coordinator harmlessly ignores `h<col>`); a sortable grid passes the
    /// [`GridSortExternal`](pinion_core::widgets::grid_sort::GridSortExternal)
    /// anchor so header clicks drive the sort proxy (sort ⊥ selection).
    pub sort_tag: Option<&'a str>,
    /// R778 §5.40 — the visual→source row permutation
    /// ([`GridSortState::order`](pinion_core::widgets::grid_sort::GridSortState::order)).
    /// `None` is the identity (display order); `Some(order)` windows over
    /// `order.len()` view positions and resolves visual position `view_pos`
    /// to source row `order[view_pos]`, so a re-sort reorders the rows while
    /// the data-indexed cell tags / selection stay correct.
    pub order: Option<&'a [usize]>,
    /// R785 §5.27 — per-column widths (logical px), the
    /// [`ColumnWidths`](pinion_core::widgets::column_widths::ColumnWidths)
    /// snapshot. `None` tiles every column at the uniform
    /// [`TableStyle::col_width`] (every grid before R785); `Some(widths)` sizes
    /// each column individually and the grid's content width becomes their sum
    /// (so widening a column past the viewport engages the R784 horizontal
    /// scroll). A column index beyond `widths.len()` falls back to the uniform
    /// width.
    pub col_widths: Option<&'a [u32]>,
    /// R786 §5.27 — when `true`, each header cell reserves
    /// [`TableStyle::resize_handle_w`] of its trailing edge for a live-drag
    /// resize grabber tagged `"<tag>_ch<col>#resize"`. The binding must register
    /// one [`ColumnResizeExternal`](pinion_core::widgets::column_widths::ColumnResizeExternal)
    /// per column (via
    /// [`column_resize_externals`](pinion_core::widgets::column_widths::column_resize_externals))
    /// so the grabber's capture drives the shared [`ColumnWidths`](pinion_core::widgets::column_widths::ColumnWidths) model;
    /// dragging a border widens the column, growing the R784 horizontal scroll.
    /// `false` keeps the full-width R785 header (no grabber, no width reserved).
    pub resizable: bool,
    /// R859 §5.27 — number of leading columns to **freeze** (pin) against
    /// the R784 horizontal scroll, the spreadsheet "frozen row-headers"
    /// pattern a DCC asset-browser / scene-outliner needs (the name column
    /// stays visible while metadata columns scroll sideways).
    ///
    /// `0` (the only mode before R859) renders the single-scroll R784 grid
    /// **byte-identically** — every pre-R859 caller is unchanged. A value
    /// `1..cols` splits the grid into a frozen-left pane (columns
    /// `0..frozen_cols`, pinned) and a scrolling pane (columns
    /// `frozen_cols..`, sliding under the shared `h_scroll`); the two share
    /// the vertical `body` scroll via a linked-scroll
    /// [follower](pinion_core::scene::ScrollNode::follower) so they stay in
    /// vertical lockstep. A value `>= cols` is clamped to `cols - 1` (at
    /// least one column must remain scrollable for the freeze to mean
    /// anything).
    pub frozen_cols: usize,
    /// R998 §5.40 — an optional per-row coloring resolver: `row_style(source)`
    /// returns `Some((bg, fg))` to paint that **source** data row with a
    /// declarative style rule's tint (Wireshark / dlt-class row coloring), or
    /// `None` to keep the default zebra fill. The binding wires it from a
    /// [`RowStyleState`](pinion_core::widgets::row_style::RowStyleState):
    /// `Some(&|src| rules.resolve(|c| cells(src)[c]).map(|t| (t.bg, t.fg)))`.
    /// Resolved **per source row**, so a coloured row stays coloured across a
    /// re-sort; selection still wins the highlight (precedence: selection >
    /// rule > zebra). `None` (every pre-R998 caller) renders byte-identically.
    pub row_style: Option<&'a dyn Fn(usize) -> Option<(Color, Color)>>,
    /// R1532 §5.27 — per-**column** paint delegates (Qt
    /// `QAbstractItemView::setItemDelegateForColumn`): `delegate(col)` returns
    /// the [`CellPainter`] that draws that column's cells, or `None` for the
    /// built-in text painter.
    ///
    /// The column axis's answer to what [`Self::row_style`] is on the row
    /// axis, and the reason it exists is that the grid could paint exactly one
    /// thing. [`GridModel::cell`] answers with a `String`, so every column was
    /// a label: a size column could not be a bar, a visibility column could not
    /// be a mark, a swatch column could not be a swatch. That is the extension
    /// point of every Model/View framework — Qt's `QStyledItemDelegate`, whose
    /// documented purpose is precisely "a column that is not text" — and a
    /// DCC/IDE grid is mostly made of such columns.
    ///
    /// **Asked once per painted column, not per cell.** The delegate for a
    /// column is a property of the column, so resolving it per cell would ask
    /// the same question once per row for an answer that cannot change — the
    /// per-section discipline R1530 gave the header axis. A column outside the
    /// painted window is never asked at all.
    ///
    /// `None` (every pre-R1532 caller) paints byte-identically: the built-in
    /// painter *is* the previous code, reached through the same call the
    /// delegate is, so the default is not a special case that could drift from
    /// the delegated path.
    pub delegate: Option<&'a dyn Fn(usize) -> Option<CellPainter<'a>>>,
}

/// R1532 §5.27 — what a [`CellPainter`] is given: everything about the one
/// cell it draws.
///
/// Qt hands its delegate `(painter, styleOption, index)`; this carries the
/// same three things in the shape a structured-scene framework can use. There
/// is no painter — §2 #1 forbids an opaque paint callback — so a delegate
/// *returns* a [`Scene`] instead of drawing into one, which is what keeps a
/// custom column as introspectable through `scene/snapshot` as a text one.
///
/// [`Self::text`] is the model's answer for this cell, already fetched. A
/// delegate that wants the raw datum parses it, or closes over its own model —
/// the same choice Qt's delegate has between `index.data()` and reaching past
/// it. Handing it over rather than making the delegate re-ask keeps
/// [`GridModel::cell`] invoked exactly once per painted cell, which is the
/// R1524 guarantee.
pub struct CellRender<'a> {
    /// Which cell (Qt's `QModelIndex`), with the **absolute** column.
    pub index: CellIndex,
    /// The model's text for this cell (Qt `Qt::DisplayRole`).
    pub text: &'a str,
    /// R1535 — the model's decoration for this cell (Qt
    /// `Qt::DecorationRole`), or `None` when it has none.
    ///
    /// Fetched by the same rule [`Self::text`] is — [`GridModel::decoration`]
    /// asked once per painted cell — so a delegate that wants to place the mark
    /// itself gets the model's answer rather than re-asking for it.
    /// R1536 — borrowed, not owned: the answer now carries a `String`, and a
    /// cell that is painted is a cell whose row was just fetched, so the
    /// painter reads the fetched answer instead of cloning it per frame.
    pub decoration: Option<&'a CellDecoration>,
    /// The cell's box width in logical px — this column's resolved width.
    pub width: u32,
    /// The cell's box height in logical px (the row height).
    pub height: u32,
    /// The row's resolved foreground colour, after its interaction state and
    /// any [`VirtualTableData::row_style`] rule. A delegate that inks anything
    /// should start here so a disabled or rule-coloured row stays consistent.
    pub fg: Color,
    /// The palette, for a delegate that resolves its own roles.
    pub theme: &'a Theme,
    /// The grid's dimensions, for a delegate that wants the same paddings and
    /// label size as the built-in painter.
    pub style: &'a TableStyle,
    /// The cell's composite hit-test tag (`"<root>#<row>_<col>"`). A delegate
    /// **must** put this on the container it returns, or the cell stops being
    /// addressable by pointer input and by RPC; [`text_cell_painter`] shows the
    /// shape.
    pub tag: &'a str,
    /// R1536 — the grid's paint-root tag, the base every
    /// [`GridTag`] address is built from.
    ///
    /// Carried separately from [`Self::tag`] rather than parsed back out of it:
    /// a delegate that wants to address a part of the cell it draws should ask
    /// the same SSOT the grid does, not re-derive the root by splitting a
    /// composite tag on `'#'` (which is decoding, and the encode SSOT exists so
    /// nobody does that).
    pub root: &'a str,
}

/// R1532 §5.27 — how one column's cells are painted (Qt
/// `QStyledItemDelegate::paint`), wired per column through
/// [`VirtualTableData::delegate`].
pub type CellPainter<'a> = &'a dyn Fn(&CellRender<'_>) -> Scene;

/// R1535 §5.27 — what a cell's `Qt::DecorationRole` answer can be: the mark the
/// built-in painter draws **beside** the display text.
///
/// The grid's second data role, and the reason it is one is that R1532's
/// delegate could not express it. A delegate belongs to a **column** — Qt's
/// `setItemDelegateForColumn`, and a column's painter cannot vary by row — so a
/// column whose mark differs per row (a status colour, a layer colour, a
/// severity) had to be delegated wholesale and then re-derive the text the
/// model had already answered with. A role is asked per **cell**, which is the
/// axis the datum actually varies on.
///
/// # Why an enum with one arm
///
/// `Qt::DecorationRole` is a *variant*: a `QColor`, a `QPixmap`, or a `QIcon`.
/// Naming the role's type as a sum keeps the arms pinion cannot paint yet
/// **absent** rather than misrepresented — a `Color` newtype would assert that
/// a decoration is a colour, which is not what the role means. The icon arm is
/// reachable (`Scene::Image` exists and the shell caches image sources), and it
/// is deliberately not added here: it would ship an arm no consumer paints, and
/// the round that adds it should be the round that has one.
///
/// # Why the answer carries a meaning (R1536)
///
/// Qt's decoration role is **appearance only** — a colour or an icon. What the
/// mark *means* is a different role (`Qt::AccessibleTextRole`), which the item
/// view does not wire to the decoration, so a rendered Qt cell cannot be asked
/// what its mark stands for: `QAccessibleTableCell::text(Name)` returns the
/// display string and the decoration contributes nothing. A status column that
/// is only a colour is, to a Qt screen-reader user, an empty cell.
///
/// Here the two travel together, so they cannot drift and a client that can see
/// the mark can also read it. That is not Qt parity; Qt is the floor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellDecoration {
    /// Qt's `QColor` decoration — a filled square in the cell's leading edge,
    /// [`TableStyle::decoration_px`] a side.
    /// Qt's `QIcon` / `QPixmap` decoration — an image drawn at the same square,
    /// resolved through the shell's image cache
    /// like any other [`Scene::Image`] source (a filesystem path, or the R1404
    /// `memory://<key>` scheme for a producer-registered RGBA buffer).
    ///
    /// The peer arm, not a replacement: Qt's decoration role accepts a colour
    /// **or** an icon, and a grid wants both — a layer colour is a swatch, a
    /// file type is an icon. `meaning` is read by exactly the same rule as
    /// [`Self::Swatch::meaning`]; an icon that restates its cell's text is as
    /// decorative as a colour that does.
    Icon {
        /// The image source, e.g. `"memory://type-folder"`.
        source: String,
        /// What the icon means — see [`Self::Swatch::meaning`].
        meaning: String,
    },
    Swatch {
        /// The ink.
        color: Color,
        /// What the mark **means** — HTML's `alt`, and read by exactly its
        /// rule.
        ///
        /// **Empty is the decorative answer**, not a missing one: when the mark
        /// restates something the cell's own text already says, announcing it
        /// makes a screen reader say the status twice, so `alt=""` is the
        /// *correct* markup and this is its equivalent. A mark that carries
        /// information the text does not — a colour-only status column — gives
        /// it here, and the cell's accessible name composes
        /// `"<meaning> <text>"`, which is what the browser does for
        /// `<td><img alt="Overdue"> 3 days</td>`.
        ///
        /// So the model states which of the two it is, because the model is the
        /// only thing that knows. A framework guessing (always announce / never
        /// announce) would be wrong for half its consumers.
        meaning: String,
    },
}

impl CellDecoration {
    /// R1536 — what this mark means, whichever arm it is. Empty is the
    /// decorative answer (HTML `alt=""`); see [`Self::Swatch::meaning`].
    ///
    /// Exists so the painter reads the meaning without matching the arm: the
    /// meaning is a property of the ROLE's answer, not of how it is drawn, and
    /// a `match` at the read site would have to grow an arm every time the
    /// variant list does — which is exactly how one arm gets forgotten.
    #[must_use]
    pub fn meaning(&self) -> &str {
        match self {
            Self::Swatch { meaning, .. } | Self::Icon { meaning, .. } => meaning,
        }
    }
}

/// R1532 §5.27 — the built-in painter: a left-aligned label in the row's
/// foreground colour, preceded (R1535) by the cell's
/// [`Qt::DecorationRole`](CellDecoration) mark when it has one. Qt's
/// `QStyledItemDelegate`, which paints exactly these two roles.
///
/// The tag is the composite hit-test tag (`"<root>#<row>_<col>"`); the
/// keyboard-focus ring is the shell's job (R694 `paint_focus_ring` over the
/// active-descendant cell), so no focus state is threaded here.
///
/// Public because a delegate that only *extends* the default should compose
/// with it rather than re-derive a cell's padding, size and tag placement, and
/// because it is the worked example of the one rule a painter must follow (the
/// container carries [`CellRender::tag`]).
///
/// An undecorated cell emits the pre-R1535 node **exactly**: no swatch, no gap
/// spacer, one text child. The decoration is additive, so a grid that answers
/// `None` everywhere paints a byte-identical scene.
#[must_use]
pub fn text_cell_painter(c: &CellRender<'_>) -> Scene {
    let mut children = Vec::with_capacity(3);
    let mut meaning = "";
    if let Some(decoration) = c.decoration {
        let tag = GridTag::cell_decoration(c.root, c.index.row, c.index.col);
        let side = c.style.decoration_px;
        children.push(match decoration {
            CellDecoration::Swatch { color, .. } => swatch_node(*color, side, &tag),
            CellDecoration::Icon { source, .. } => icon_node(source, side, &tag),
        });
        // The gap is a spacer rather than padding on the mark so the
        // undecorated cell keeps its node shape unchanged — see `pad_node`,
        // the same "emit nothing when it would be a zero-width box" rule.
        children.extend(pad_node(c.style.decoration_gap_px, c.height));
        meaning = decoration.meaning();
    }
    // R1536 §5.40 — the cell's label is the cell's **content**, so it is NOT
    // presentational. `TextRole::Presentational` exists (R51.81) for decoration
    // glyphs a name derivation must skip past to reach the linguistic label —
    // a checkbox's tick, a slider's caret. A data cell has no such label to
    // reach: this text is the only thing it says.
    //
    // Marking it presentational made every `gridcell` unnameable, while
    // `pinion_a11y::tree_view` documented the opposite contract in so many
    // words ("gridcell name comes from the painted text, not the builder").
    // The write path and the read path disagreed, and the AT tree took the
    // paint's answer: silence.
    children.push(Scene::Text(TextNode::styled(
        c.text.to_string(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(c.style.label_size_px)
            .with_fg(c.fg),
    )));
    let mut cell = ContainerNode::new(children)
        .with_tag(c.tag.to_string())
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Start)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(c.width, c.height))
                .with_padding(Rect::new(c.style.cell_pad_x, 0, c.style.cell_pad_x, 0)),
        );
    // R1536 — a mark that carries meaning joins the cell's accessible name,
    // ahead of the label, exactly as a browser names
    // `<td><img alt="Overdue"> 3 days</td>`. Set only when there is something
    // to add: the derivation's own name-from-contents path already produces the
    // label, so a decorative (`alt=""`) mark leaves the AT tree byte-identical
    // and cannot make a screen reader say the status twice.
    if !meaning.is_empty() {
        cell = cell.with_aria_label(compose_cell_name(meaning, c.text));
    }
    Scene::Container(cell)
}

/// R1536 §5.40 — the accessible name of a cell whose mark means something:
/// `"<meaning> <text>"`, or just the meaning when the cell has no text.
///
/// Split out because the join is the part with a rule in it — a mark-only cell
/// must not be named `"Flagged "` with a trailing space, and a client that
/// string-matches an announced name would not notice.
fn compose_cell_name(meaning: &str, text: &str) -> String {
    if text.is_empty() {
        meaning.to_string()
    } else {
        format!("{meaning} {text}")
    }
}

impl core::fmt::Debug for VirtualTableData<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VirtualTableData")
            .field("column_count", &self.column_count)
            .field("item_count", &self.item_count)
            .field("overscan", &self.overscan)
            .field("sort", &self.sort)
            .field("sort_tag", &self.sort_tag)
            .field("order", &self.order)
            .field("col_widths", &self.col_widths)
            .field("resizable", &self.resizable)
            .field("frozen_cols", &self.frozen_cols)
            .field("row_style", &self.row_style.map(|_| "<fn>"))
            .field("delegate", &self.delegate.map(|_| "<fn>"))
            .finish()
    }
}

/// R784 §5.45 — the two single-axis [`ScrollState`]s a virtualized
/// data-grid drives, bundled so [`view_virtual_table`] stays under the
/// argument budget (the [`VirtualTableData`] precedent for grouping
/// related inputs). The grid composes them as a nested pair: an outer
/// [`ScrollAxis::Horizontal`] scroll over the body's inner
/// [`ScrollAxis::Vertical`] scroll, so each holder owns exactly one axis
/// and the header (which sits between the two) stays vertically pinned
/// while tracking the horizontal offset.
#[derive(Clone, Copy)]
pub struct GridScroll<'a> {
    /// Vertical body window: the rows scroll under the frozen header and
    /// virtualize against this state's measured viewport height.
    pub body: &'a Rc<ScrollState>,
    /// Horizontal scroll shared by the header and the body: its
    /// `offset_x` slides the whole column, its `max_x` is the column
    /// total minus the viewport width (0 — no scroll — when columns fit).
    pub horizontal: &'a Rc<ScrollState>,
}

/// R1524 §5.27 — the address of **one cell**: the unit
/// [`view_virtual_table`] asks its consumer for.
///
/// This is the Model/View address every framework with a virtualized grid
/// carries — Qt's `QModelIndex` (the argument to
/// `QAbstractItemModel::data`), Flutter's `TableVicinity` (the argument to
/// `TableView.builder`'s `cellBuilder`) — and it is a *struct* for the same
/// reason both of those are: the two coordinates are the same type, so a
/// positional `(row, col)` pair silently accepts them swapped. Naming them
/// also lets `col` state the property that a windowed grid makes load-bearing
/// and that R1523's frozen pane had to convert between internally — it is
/// **absolute**.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellIndex {
    /// The **data**-row index — already resolved through the sort
    /// permutation, so a consumer indexes its dataset directly and never
    /// sees a visual position (the R730 data-indexed convention the
    /// selection predicate follows).
    pub row: usize,
    /// The **absolute** table-column index: `0` is the grid's first column
    /// whatever the column window or the frozen split. A consumer therefore
    /// answers for the column it was asked about with no knowledge of either.
    pub col: usize,
}

/// R1530 §5.27 — the two questions a virtualized grid asks its model, bundled
/// as one parameter.
///
/// Qt answers both from a single `QAbstractItemModel` — `data(index)` for a
/// cell and `headerData(section, orientation)` for a column label — and they
/// travel together here for the same reason: they are the same kind of thing
/// (an accessor the grid invokes once per painted unit), so a grid that
/// windows one and not the other is asking its consumer to do work it will
/// throw away. Before R1530 only `cell` was an accessor; the headers arrived as
/// a slice of **every** column, because [`VirtualTableData`] read the column
/// count off its length.
///
/// The counts stay in [`VirtualTableData`] (`column_count` beside
/// `item_count`), where the extents of the two axes are stated together; this
/// holds the *accessors*. Selection is deliberately not here — Qt keeps it
/// in a separate `QItemSelectionModel`, and so does `view_virtual_table`'s
/// `is_selected`.
///
/// # Roles (R1535)
///
/// Qt reaches every one of these through a single `data(index, role)` because a
/// C++ model needs one virtual entry point, and pays for it with `QVariant` —
/// an untyped hole every caller must unwrap. Here each role is its **own typed
/// accessor**, so a role's answer type is exact and a model that cannot answer
/// one is unrepresentable rather than returning an invalid variant. That is the
/// shape R1530 already chose when it split Qt's `headerData` out as
/// [`Self::header`] instead of folding it into `cell`.
///
/// Three of Qt's roles are answered so far — `DisplayRole` ([`Self::cell`]),
/// `DisplayRole` on the header axis ([`Self::header`]) and `DecorationRole`
/// ([`Self::decoration`]). `EditRole` and `ToolTipRole` are not: the first
/// belongs with the delegate's editing half (which does not exist yet — R1532
/// gave the delegate paint only), the second needs a per-cell hover path.
pub struct GridModel<C, H, D> {
    /// Invoked once per **painted cell** with that cell's [`CellIndex`],
    /// returning its text (Qt `data(QModelIndex)`, Flutter `cellBuilder`).
    pub cell: C,
    /// R1530 — invoked once per **painted column header** with that column's
    /// absolute section index, returning its label (Qt `headerData(section,
    /// Qt::Horizontal, Qt::DisplayRole)`).
    ///
    /// A section outside the painted window is never asked, so the cost of the
    /// header band scales with the column window rather than with the table —
    /// the property the cell axis gained in R1524. A column with no label
    /// returns an empty `String`.
    pub header: H,
    /// R1535 — invoked once per **painted cell** with that cell's
    /// [`CellIndex`], returning the mark drawn beside its text (Qt
    /// `data(index, Qt::DecorationRole)`), or `None` for an undecorated cell.
    ///
    /// Asked per cell rather than per column because that is the axis the
    /// answer varies on: a status colour differs row by row, which is precisely
    /// what R1532's per-column [`VirtualTableData::delegate`] cannot express.
    /// A grid with no decorated column passes [`no_decoration`] and pays
    /// nothing — the closure monomorphizes to a constant `None` and the painter
    /// emits the pre-R1535 node.
    pub decoration: D,
}

/// R1535 §5.27 — the [`GridModel::decoration`] accessor for a grid where no
/// cell carries a mark: the `Qt::DecorationRole` every un-decorated model
/// answers with an invalid `QVariant`.
///
/// A named function rather than `|_| None` at nineteen call sites so the "this
/// grid answers no decoration role" statement is greppable, and so the
/// undecorated case reads as a decision rather than as a closure that happens
/// to return nothing.
#[must_use]
pub fn no_decoration(_: CellIndex) -> Option<CellDecoration> {
    None
}

/// R1530 §5.27 — the [`GridModel::header`] accessor for a grid whose labels
/// are a **fixed list**.
///
/// The adapter from the per-section contract to the shape most grids have:
/// a `const HEADERS: [&str; N]`. Eleven bindings hold one, and each would
/// otherwise spell the identical closure — mechanical wiring with no
/// per-binding opinion in it, so obligation 3b lifts it here rather than
/// leaving eleven copies to drift.
///
/// This does **not** reintroduce what R1530 removed. The defect was a grid
/// that could only learn its extent by being handed every label, which forced
/// the caller to *materialize* them; a `&'static` array is already there and
/// costs nothing per frame. A grid whose labels are computed — the 200-column
/// case — passes a closure that computes only the sections it is asked about.
///
/// A section past the list answers with an empty label, the contract's
/// blank-label rule, rather than panicking a paint pass.
pub fn header_from_slice<'a>(labels: &'a [&'a str]) -> impl Fn(usize) -> String + 'a {
    move |col| labels.get(col).copied().unwrap_or_default().to_string()
}

/// R1524 §5.27 — the whole `rows x cols` matrix, materialized from a
/// [`CellIndex`]-addressed source.
///
/// The adapter from the per-cell contract [`view_virtual_table`] asks through
/// to the **eager** matrix a model wants seeded — a
/// [`GridSortState`](pinion_core::widgets::grid_sort::GridSortState) or a
/// `RowSearchState` holds every row so it can sort / filter / search rows the
/// viewport has never exposed. Both shapes are legitimate: the grid paints a
/// window, the model reasons over the set.
///
/// It exists so those two shapes come from **one** formula. Six bindings seed a
/// model beside a grid, and each had derived the matrix from its own per-cell
/// function with an identical nested map (obligation 3b — mechanical wiring, no
/// per-binding opinion in it). Deriving it here means a binding cannot seed its
/// model from a formula that has drifted from the one its cells are painted
/// with; before R1524 the two were separate functions and nothing held them
/// together.
#[must_use]
pub fn materialize_cells(
    rows: usize,
    cols: usize,
    mut cell: impl FnMut(CellIndex) -> String,
) -> Vec<Vec<String>> {
    (0..rows)
        .map(|row| (0..cols).map(|col| cell(CellIndex { row, col })).collect())
        .collect()
}

/// R775 §5.27 — flex-viewport (`AutoSizer`) **virtualized data-grid**.
///
/// The R707 [`view_table`] builds one strip per row eagerly — fine at a
/// dozen rows, impossible at the 10_000-row grids a DCC / IDE inspector
/// needs. This peer composes the same M3 header + cell visuals but
/// virtualizes the body over the R774 `AutoSizer` substrate: a fixed
/// header band above a `flex_grow` [`ScrollNode`] whose content
/// materializes cell rows for **only** the window the runtime-*measured*
/// viewport height exposes. Resize the window and the rendered row count
/// tracks it; scroll and the band slides — exactly the list virtualization
/// of [`view_flex_virtual_list`](crate::virtual_list::view_flex_virtual_list),
/// but each windowed slot is a multi-column data row built by
/// `data_row`. The shared windowed-sizer + flex-`ScrollNode` shape is
/// reused from `crate::virtual_list` (one source of truth, so the
/// scroll-bound wiring cannot diverge between the list and the grid).
///
/// R784 §5.45 — the grid now scrolls **horizontally** too, with a
/// frozen header. The whole `[header, body]` column is wrapped in an
/// outer [`ScrollAxis::Horizontal`] [`ScrollNode`] driven by
/// `h_scroll`: when the column total (`headers.len() × style.col_width`)
/// exceeds the viewport width the grid scrolls sideways, and because
/// the header band and the body share that one horizontal scroll the
/// header tracks the body's horizontal offset exactly. The header
/// stays *vertically* pinned because it sits above the inner vertical
/// body scroll (the R784 nested single-axis composition: outer
/// horizontal over `[frozen header, inner vertical body]`). The
/// surface frame is lifted out of the horizontal scroll so the rounded
/// block stays put while the content slides under it.
///
/// # Parameters
///
/// - `tag` — composite paint-root tag; same cell / header / row tag scheme
///   as [`view_table`] (`"<tag>_hrow"`, `"<tag>_ch<col>"`,
///   `"<tag>_row<id>"`, `"<tag>#<id>_<col>"`).
/// - `scroll` — the reactive [`ScrollState`] for the **vertical** body
///   window; the body windows against its
///   [`measured_viewport`](pinion_core::widgets::scroll::ScrollState::measured_viewport)
///   height and the layout pass publishes the flex-measured extent back.
/// - `h_scroll` — the reactive [`ScrollState`] for the **horizontal**
///   scroll shared by the header and the body. Its `offset_x` slides the
///   whole column; `max_x` is the column total minus the viewport width
///   (0 when the columns fit, so a narrow grid is unaffected). Drive it
///   with `scene/set_scroll_offset` on its tag, or wheel / arrow input.
/// - `data` — the [`VirtualTableData`] (both axes' extents + overscan).
/// - `theme` / `style` — palette + [`TableStyle`] dimensions.
/// - `is_selected` — a predicate over the **data-row** index: a `true` row's
///   strip is washed with the accent tint (the same `row_fill` selection
///   path the eager [`view_table`] uses). This is the data-indexed
///   generalization of a single selected index — single-select passes
///   `|id| selected == Some(id)`, R782 multi-select passes set membership,
///   and a display-only grid passes `|_| false`. One virtualized-grid paint
///   path serves all three (no parallel `_multi` body to diverge from the
///   windowed-sizer geometry). It is invoked only for the windowed rows.
/// - `model` — the [`GridModel`]: `cell` (R1524, once per painted cell),
///   `header` (R1530, once per painted column header) and `decoration` (R1535,
///   once per painted cell — [`no_decoration`] when no column carries a mark).
///   This is the Model/View
///   contract of a two-axis virtualized grid (Qt `data(index)` /
///   `headerData(section)`, Flutter `cellBuilder`): the grid asks for exactly
///   what it paints, so a 200-column grid showing five columns asks for five
///   cells per row and five labels rather than building 200 of each and
///   keeping five. Until R1524 `cell` was a per-*row* builder returning every
///   column's text — which meant R1523 could window the scene tree but not the
///   work of filling it — and until R1530 the labels were not asked for at
///   all: they arrived as a slice of the whole table, because the column count
///   was that slice's length. Anything with nothing to show returns an empty
///   `String`; because the grid asks per unit there is no "came back short"
///   case to define.
#[must_use]
pub fn view_virtual_table(
    tag: &str,
    scroll: GridScroll<'_>,
    data: VirtualTableData<'_>,
    theme: &Theme,
    style: &TableStyle,
    is_selected: impl Fn(usize) -> bool,
    model: GridModel<
        impl FnMut(CellIndex) -> String,
        impl FnMut(usize) -> String,
        impl FnMut(CellIndex) -> Option<CellDecoration>,
    >,
) -> Scene {
    let cols = data.column_count;
    // R785 — resolve the per-column widths once (uniform `style.col_width`
    // fallback when no width model is wired). Content width is their sum, so
    // widening a column grows the horizontal scroll extent (R784).
    let widths = resolve_widths(cols, data.col_widths, style);
    // R778 — window over the *view* length: the sort permutation's
    // `order.len()` when sorted, else the raw `item_count` (identity). Each
    // visual position resolves to its source row through `order` (the 1-D
    // `view_virtual_list` + `ViewOrderState` pairing, now multi-column).
    let view_len = data.order.map_or(data.item_count, <[usize]>::len);
    // AutoSizer: window against the runtime-measured clip height. The
    // header sits OUTSIDE the vertical body scroll (frozen), so the
    // body's measured height is the window minus the header band.
    let (_, measured_h) = scroll.body.measured_viewport();
    let window = compute_visible_range(
        scroll.body.offset_y(),
        measured_h,
        view_len,
        style.row_height,
        data.overscan,
    );
    let total_h = content_height(view_len, style.row_height);

    let render = GridRender {
        tag,
        // R778 — clickable headers route to the sort anchor (`sort_tag`)
        // when a sort coordinator is wired, else stay on `tag` (R777).
        click_tag: data.sort_tag.unwrap_or(tag),
        data: &data,
        theme,
        style,
        widths: &widths,
        window: &window,
        total_h,
    };

    // R859 — clamp the freeze to `[0, cols - 1]`: at least one column must
    // stay scrollable for the freeze to mean anything. `0` (every pre-R859
    // caller) renders the single-scroll R784 grid byte-identically.
    let frozen_cols = data.frozen_cols.min(cols.saturating_sub(1));
    // `model` / `is_selected` are consumed by exactly one branch (an
    // `if`/`else`), so each helper takes them by value without a re-move.
    let content = if frozen_cols == 0 {
        render.render_unsplit(scroll, &is_selected, model)
    } else {
        render.render_frozen(scroll, frozen_cols, &is_selected, model)
    };

    Scene::Container(
        ContainerNode::new(vec![content])
            .with_tag(tag.to_string())
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow))
                    .with_corner_radius(style.corner_radius),
            )
            // flex_grow so the grid fills its parent (the AutoSizer
            // contract); the inner pane(s) then claim the framed interior
            // and the body scroll claims the height left after the header.
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_flex_grow(1.0)
                    // (R1020 §5.39) Carry the binding's focus-stop opt-in onto
                    // the tag-carrying outer block so the scene-derived
                    // enumeration collects this grid's tag as a Tab stop.
                    .with_focusable(style.focusable)
                    .with_padding(Rect::new(
                        style.block_pad,
                        style.block_pad,
                        style.block_pad,
                        style.block_pad,
                    )),
            ),
    )
}

/// R897 §5.27 — the content-agnostic virtualized grid body shared by the
/// read-only [`view_virtual_table`] and the editable `hello-data-grid`:
/// window over `view_len` uniform-`row_pitch` rows, build only the visible
/// ones through `build_row`, and wrap the `[header, body]` column in the
/// outer horizontal scroll. The consumer owns each row's **content** (a
/// string data row, or the editable grid's interspersed group-header /
/// rich-cell rows); the substrate owns the windowing geometry
/// (`uniform_slots` slot-tops + the `total_h` sizer) and the nested
/// single-axis scroll wiring (`assemble_windowed_flex` vertical body inside
/// the outer [`h_scrolled_column`] horizontal scroll), so the two grids cannot
/// diverge on the scroll-bound geometry (R758 divergence-is-a-bug).
///
/// `window` / `total_h` come from the caller's [`compute_visible_range`] /
/// [`content_height`] over the same `view_len`; `content_w` is the row width
/// (the column-width sum) the windowed sizer frames. `build_row` is invoked
/// once per **visible** view position and returns that row's full Scene (the
/// substrate only positions it at `view_pos · row_pitch`), so an off-window row
/// is never built — this is the actual virtualization, not an eager clip.
#[must_use]
pub fn view_virtual_grid_body(
    scroll: GridScroll<'_>,
    window: &VisibleWindow,
    content_w: u32,
    total_h: u32,
    row_pitch: u32,
    header: Scene,
    build_row: impl FnMut(usize) -> Scene,
) -> Scene {
    let slots = uniform_slots(window, content_w, row_pitch, build_row);
    let body = assemble_windowed_flex(scroll.body, content_w, total_h, slots, false);
    h_scrolled_column(scroll.horizontal, header, body)
}

/// R859 §5.27 — the shared "what + how to render" context for the two
/// [`view_virtual_table`] body assemblies (the unsplit R784 grid and the
/// frozen-column split). Bundling the borrowed inputs keeps each assembly
/// method under the argument + line budgets while sharing the window /
/// widths / theme exactly (a divergence between the two would be a paint
/// bug, not a style choice).
struct GridRender<'a, 'd> {
    tag: &'a str,
    click_tag: &'a str,
    /// R1532 — the data's own lifetime is a **second** parameter, not `'a`.
    /// [`VirtualTableData::delegate`] names its lifetime in a `Fn` return
    /// type, which makes `VirtualTableData<'d>` invariant in `'d`; welding it
    /// to the borrow of the struct would then require the caller's `data`
    /// local to outlive its own parameters.
    data: &'a VirtualTableData<'d>,
    theme: &'a Theme,
    style: &'a TableStyle,
    widths: &'a [u32],
    window: &'a VisibleWindow,
    total_h: u32,
}

/// R784 / R859 §5.45 — wrap a `[header, body]` column in the outer
/// horizontal scroll the data-grid uses: the header tracks `horizontal`'s
/// offset while staying vertically pinned above the body, and the column
/// flex-grows to claim its parent's interior. Once the `[header, body]`
/// content is wider than the parent the column slides sideways; while it
/// fits, `horizontal`'s `max_x` is 0 and the wrap is inert.
///
/// Shared by three consumers so the R784 horizontal-scroll wrapping cannot
/// diverge between them (a divergence would mis-scroll one — R758
/// "divergence-is-a-bug"): the read-only unsplit grid (`render_unsplit`),
/// the frozen grid's scrolling pane (`frozen_split_panes`), and — R896 —
/// the editable `hello-data-grid`, which wraps its eager `[header, rows]`
/// column to scroll its widened columns sideways (the body there is the eager
/// row column, not a windowed `ScrollNode`; the wrap is body-agnostic).
#[must_use]
pub fn h_scrolled_column(horizontal: &Rc<ScrollState>, header: Scene, body: Scene) -> Scene {
    let column = Scene::Container(
        ContainerNode::new(vec![header, body])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    );
    Scene::Scroll(
        ScrollNode::from_state(Rc::clone(horizontal), Rect::default(), column)
            .with_axis(ScrollAxis::Horizontal)
            .with_layout(LayoutStyle::new().with_flex_grow(1.0)),
    )
}

/// R860 §5.27 — one pane of a frozen split: its header band, its width, and
/// its windowed body slots (already built by the caller for that pane's
/// column subset). Bundled so [`frozen_split_panes`] stays under the
/// argument budget. `pub(crate)` so the tree-grid (`tree_view`) reuses the
/// same frozen-split assembler as the data-grid.
pub(crate) struct SplitPane {
    pub header: Scene,
    pub width: u32,
    pub slots: Vec<Scene>,
}

/// R859 / R860 §5.45 — assemble a **frozen split** data surface: a
/// fixed-width frozen pane (header + follower vertical body) beside a
/// flex-grow scrolling pane (header + primary vertical body inside the
/// outer horizontal scroll). The two panes share the vertical `body`
/// `ScrollState` — the scrolling pane's inner scroll is the primary that
/// publishes bounds, the frozen pane's is a
/// [follower](pinion_core::scene::ScrollNode::as_follower) — so they scroll
/// in vertical lockstep without the measured-viewport oscillation a second
/// publisher would cause.
///
/// Shared by the frozen-column **data-grid** ([`GridRender::render_frozen`],
/// R859) and the **tree-grid** ([`view_virtual_treegrid`](crate::tree_view::view_virtual_treegrid), R860): both feed
/// pre-built slots (data rows vs tree-cell rows), so the linked-scroll
/// follower wiring + the fixed-width/flex-grow pane geometry are one source
/// of truth (a divergence would mis-scroll one of the two — R758
/// "divergence-is-a-bug").
pub(crate) fn frozen_split_panes(
    scroll: GridScroll<'_>,
    total_h: u32,
    frozen: SplitPane,
    scrolled: SplitPane,
) -> Scene {
    // Frozen pane: header + follower V-body, pinned at `frozen.width` (width
    // fixed, height stretches via the Row's `AlignItems::Stretch`).
    let frozen_body =
        assemble_windowed_flex(scroll.body, frozen.width, total_h, frozen.slots, true);
    let frozen_pane = Scene::Container(
        ContainerNode::new(vec![frozen.header, frozen_body]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_size(Size::width_px(frozen.width)),
        ),
    );
    // Scrolling pane: an R784 horizontally-scrolled `[header, body]` column,
    // flex-growing into the width left of the frozen pane; its inner V body
    // is the body PRIMARY.
    let scroll_body =
        assemble_windowed_flex(scroll.body, scrolled.width, total_h, scrolled.slots, false);
    let scroll_pane = h_scrolled_column(scroll.horizontal, scrolled.header, scroll_body);
    Scene::Container(
        ContainerNode::new(vec![frozen_pane, scroll_pane]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_flex_grow(1.0),
        ),
    )
}

impl<'d> GridRender<'_, 'd> {
    /// Visual position -> source data row (identity when unsorted) — the
    /// R778 sort-permutation resolution shared by both bodies.
    fn source_of(&self, view_pos: usize) -> usize {
        self.data
            .order
            .map_or(view_pos, |o| o.get(view_pos).copied().unwrap_or(view_pos))
    }

    /// R859 — the per-row visual inputs shared by every slot closure (the
    /// unsplit body and both frozen split panes): the source data row, the
    /// strip `fill`, and the cell `fg`. Selection / state are **data-indexed**
    /// (`is_selected(source)`, so they survive a re-sort); zebra parity is
    /// **visual** (`view_pos`, so the stripe pattern is stable across
    /// re-sorts — the eager `view_table` convention). Lifted here so the
    /// three slot closures cannot disagree on the fill / fg derivation.
    fn row_inputs(
        &self,
        view_pos: usize,
        is_selected: &impl Fn(usize) -> bool,
    ) -> (usize, Color, Color) {
        let source = self.source_of(view_pos);
        let selected = is_selected(source);
        // R998 — precedence: selection highlight > matched coloring rule >
        // zebra stripe. A selected row keeps the accent fill so the selection
        // stays visible over any rule tint; otherwise a row-style rule (per
        // SOURCE row, so it survives a re-sort) overrides the zebra default.
        if !selected {
            if let Some((bg, fg)) = self.data.row_style.and_then(|resolve| resolve(source)) {
                return (source, bg, fg);
            }
        }
        let fill = row_fill(self.theme, RadioState::Idle, selected, view_pos);
        let fg = row_fg(self.theme, RadioState::Idle);
        (source, fill, fg)
    }

    /// R1530 §5.27 — one pane's header band: the sections it lays out, asked
    /// for and then painted.
    ///
    /// Shared by the three panes that paint a header band (the unsplit grid,
    /// and the frozen split's pinned + scrolling panes), so none of them can
    /// ask for a section it will not paint — the [`pane_cells`] discipline on
    /// the header axis.
    ///
    /// The span is **derived** from `layout` rather than passed beside it:
    /// every pane's sections are exactly `col_base .. col_base + widths.len()`,
    /// so a second statement of it could only ever disagree — the shape R1524
    /// removed from the cell axis when it made "row came back short"
    /// inexpressible.
    fn header_band(
        &self,
        header: &mut impl FnMut(usize) -> String,
        layout: ColumnLayout<'_>,
    ) -> Scene {
        let span = layout.col_base..layout.col_base + layout.widths.len();
        let labels = header_texts(header, span);
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        header_row(
            self.tag,
            self.click_tag,
            &refs,
            self.data.sort,
            layout,
            self.theme,
            self.style,
        )
    }

    /// R1523 §5.27 §5.45 — the columns the horizontal viewport exposes within
    /// `widths`, on the [`visible_columns`] SSOT.
    ///
    /// The column-axis peer of the `window` this render already carries for the
    /// row axis, and derived the same way: from a **measured** viewport, so a
    /// resize re-windows without the caller restating anything. `offset` is the
    /// horizontal scroll offset already relative to this pane's content, which
    /// is why the frozen pane (pinned, never scrolled) does not call this at
    /// all.
    fn column_window(&self, horizontal: &Rc<ScrollState>, widths: &[u32]) -> VisibleWindow {
        let (viewport_w, _) = horizontal.measured_viewport();
        visible_columns(
            widths,
            horizontal.offset_x(),
            viewport_w,
            self.data.overscan,
        )
    }

    /// R784 single-scroll grid (the only mode before R859): one
    /// `total_w`-wide `[header, body]` column inside an outer horizontal
    /// scroll.
    ///
    /// R1523 — both axes are now windowed: the header band and every row build
    /// only the columns `column_window` selected, padded to the full `total_w`
    /// so the horizontal scroll still bounds against all of them. A grid whose
    /// columns fit its viewport windows all of them and pads by zero, painting
    /// the pre-R1523 scene exactly.
    ///
    /// R1524 — and the *asking* is windowed too: `cell` is invoked for the
    /// `span` columns only, so the consumer's per-cell work scales with the
    /// window rather than with the table. R1530 — `header` likewise, so the
    /// header band costs one label per painted column instead of one per
    /// column in the table.
    /// R1532 §5.27 — resolve `span`'s per-column painters, once per pane.
    ///
    /// Aligned with the pane's `widths` slice, so index `j` is absolute column
    /// `span.start + j` — the same indexing `data_row` uses. Asked once per
    /// **painted column** rather than once per painted cell: a column's
    /// delegate cannot vary by row, so resolving it per cell would repeat one
    /// answer `window.len()` times (the per-section discipline R1530 gave the
    /// header axis). A grid with no delegate wired allocates the vector and
    /// fills it with `None`, which is one allocation per pane per frame
    /// against the branch it replaces.
    fn painters(&self, span: core::ops::Range<usize>) -> Vec<Option<CellPainter<'d>>> {
        span.map(|col| self.data.delegate.and_then(|d| d(col)))
            .collect()
    }

    fn render_unsplit(
        &self,
        scroll: GridScroll<'_>,
        is_selected: &impl Fn(usize) -> bool,
        model: GridModel<
            impl FnMut(CellIndex) -> String,
            impl FnMut(usize) -> String,
            impl FnMut(CellIndex) -> Option<CellDecoration>,
        >,
    ) -> Scene {
        let GridModel {
            mut cell,
            mut header,
            mut decoration,
        } = model;
        let total_w: u32 = self.widths.iter().copied().sum();
        let cols = self.column_window(scroll.horizontal, self.widths);
        let span = cols.first..cols.first + cols.count;
        let pad = ColumnPad::around(self.widths, &cols);
        let hrow_tag = GridTag::header_row(self.tag);
        // R784 — the frozen header above the inner vertical body scroll, the
        // whole `total_w`-wide column slid by the outer horizontal scroll. No
        // surface fill here — the frame in `view_virtual_table` owns it so the
        // rounded block stays put while the content scrolls.
        let band = self.header_band(
            &mut header,
            ColumnLayout {
                widths: &self.widths[span.clone()],
                resizable: self.data.resizable,
                pad,
                col_base: cols.first,
                container_tag: &hrow_tag,
            },
        );
        let painters = self.painters(span.clone());
        // R897 — the windowing + nested single-axis scroll machinery is the
        // shared `view_virtual_grid_body` SSOT (`top = view_pos · row_height`,
        // R775.1); only the row *content* (a multi-cell `data_row`) is built
        // here. The editable `hello-data-grid` drives the same primitive with
        // group-header / rich-cell rows (its virtualization consumer).
        view_virtual_grid_body(
            scroll,
            self.window,
            total_w,
            self.total_h,
            self.style.row_height,
            band,
            |view_pos| {
                let (source, fill, fg) = self.row_inputs(view_pos, is_selected);
                let (cells_text, decos) =
                    pane_cells(&mut cell, &mut decoration, source, span.clone());
                let cell_refs: Vec<&str> = cells_text.iter().map(String::as_str).collect();
                let row_tag = GridTag::data_row(self.tag, source);
                data_row(
                    self.tag,
                    source,
                    &cell_refs,
                    fill,
                    fg,
                    RowPane {
                        container_tag: &row_tag,
                        col_base: cols.first,
                        widths: &self.widths[span.clone()],
                        pad,
                        painters: &painters,
                        decorations: &decos,
                        theme: self.theme,
                    },
                    self.style,
                )
            },
        )
    }

    /// R859 frozen-left-column grid: a fixed-width frozen pane (columns
    /// `0..frozen_cols`, pinned) beside a flex-grow scrolling pane (columns
    /// `frozen_cols..`, an R784 grid). The two share the vertical `body`
    /// scroll — the scrolling pane's inner V scroll is the primary, the
    /// frozen pane's a follower — so they scroll in vertical lockstep
    /// without the shared-state oscillation a second publisher would cause.
    ///
    /// Two `uniform_slots` passes keep the slot-top geometry on its R775.1 SSOT
    /// instead of hand-rolling a loop. R1524 — because each pane now asks only
    /// for **its own** columns (`0..frozen_cols` here, the window within
    /// `frozen_cols..` there), the two passes no longer each rebuild the whole
    /// row: the split costs one `cell` call per painted cell, exactly as the
    /// unsplit grid does.
    ///
    /// R1523 — only the **scrolling** pane windows its columns. A frozen column
    /// is pinned against the horizontal scroll, so it is on screen at every
    /// offset by definition and windowing it could only ever remove something
    /// visible. The window is therefore computed inside the scrolling pane's own
    /// column space (`frozen_cols..`), which is also the space its horizontal
    /// offset is measured in.
    fn render_frozen(
        &self,
        scroll: GridScroll<'_>,
        frozen_cols: usize,
        is_selected: &impl Fn(usize) -> bool,
        model: GridModel<
            impl FnMut(CellIndex) -> String,
            impl FnMut(usize) -> String,
            impl FnMut(CellIndex) -> Option<CellDecoration>,
        >,
    ) -> Scene {
        let GridModel {
            mut cell,
            mut header,
            mut decoration,
        } = model;
        let frozen_w: u32 = self.widths[..frozen_cols].iter().copied().sum();
        let scrolled = &self.widths[frozen_cols..];
        let scroll_w: u32 = scrolled.iter().copied().sum();
        let row_pitch = self.style.row_height;
        // The scrolling pane's window, in its own column space, then lifted to
        // absolute table columns for the header labels / cell texts.
        let cols = self.column_window(scroll.horizontal, scrolled);
        let rel = cols.first..cols.first + cols.count;
        let abs = frozen_cols + rel.start..frozen_cols + rel.end;
        let scroll_pad = ColumnPad::around(scrolled, &cols);
        // R1532 — each pane resolves its own columns' painters, in absolute
        // coordinates, exactly as it asks for its own labels and texts.
        let frozen_painters = self.painters(0..frozen_cols);
        let scroll_painters = self.painters(abs.clone());

        let frozen_slots = uniform_slots(self.window, frozen_w, row_pitch, |view_pos| {
            let (source, fill, fg) = self.row_inputs(view_pos, is_selected);
            // R1524 — the pinned pane asks for the pinned columns only.
            let (cells_text, decos) =
                pane_cells(&mut cell, &mut decoration, source, 0..frozen_cols);
            let cell_refs: Vec<&str> = cells_text.iter().map(String::as_str).collect();
            // R859 — distinct `_frow{id}` container tag so the split panes
            // never emit a duplicate strip tag (per-cell `_{col}` tags stay
            // unique by absolute column).
            let frow_tag = GridTag::frozen_data_row(self.tag, source);
            data_row(
                self.tag,
                source,
                &cell_refs,
                fill,
                fg,
                RowPane {
                    container_tag: &frow_tag,
                    col_base: 0,
                    widths: &self.widths[..frozen_cols],
                    // Pinned columns are never windowed out.
                    pad: ColumnPad::NONE,
                    painters: &frozen_painters,
                    decorations: &decos,
                    theme: self.theme,
                },
                self.style,
            )
        });
        let scroll_slots = uniform_slots(self.window, scroll_w, row_pitch, |view_pos| {
            let (source, fill, fg) = self.row_inputs(view_pos, is_selected);
            // R1524 — the scrolling pane asks for its column window, in
            // absolute coordinates (`abs`), so the consumer never learns that
            // this pane's own column space starts at `frozen_cols`.
            let (cells_text, decos) = pane_cells(&mut cell, &mut decoration, source, abs.clone());
            let cell_refs: Vec<&str> = cells_text.iter().map(String::as_str).collect();
            let row_tag = GridTag::data_row(self.tag, source);
            data_row(
                self.tag,
                source,
                &cell_refs,
                fill,
                fg,
                RowPane {
                    container_tag: &row_tag,
                    col_base: abs.start,
                    widths: &scrolled[rel.clone()],
                    pad: scroll_pad,
                    painters: &scroll_painters,
                    decorations: &decos,
                    theme: self.theme,
                },
                self.style,
            )
        });

        // Frozen pane header (left, columns `0..frozen_cols`). Frozen
        // columns do not horizontally scroll, so a resize grabber (which
        // grows the horizontal extent) is moot — `resizable: false`.
        let fhrow_tag = GridTag::frozen_header_row(self.tag);
        // R1530 — each pane asks for its own sections: the pinned
        // `0..frozen_cols` here, the window within `frozen_cols..` below, in
        // absolute coordinates (the `pane_texts` split, one axis over).
        let frozen_header = self.header_band(
            &mut header,
            ColumnLayout {
                widths: &self.widths[..frozen_cols],
                resizable: false,
                pad: ColumnPad::NONE,
                col_base: 0,
                container_tag: &fhrow_tag,
            },
        );
        // Scrolling pane header (right, the windowed slice of `frozen_cols..`).
        let hrow_tag = GridTag::header_row(self.tag);
        let scroll_header = self.header_band(
            &mut header,
            ColumnLayout {
                widths: &scrolled[rel.clone()],
                resizable: self.data.resizable,
                pad: scroll_pad,
                col_base: abs.start,
                container_tag: &hrow_tag,
            },
        );
        // R859 — the shared frozen-split assembler (also the tree-grid's,
        // R860): frozen pane (follower V-body) beside the scrolling pane
        // (primary V-body in the outer horizontal scroll), vertical lockstep.
        frozen_split_panes(
            scroll,
            self.total_h,
            SplitPane {
                header: frozen_header,
                width: frozen_w,
                slots: frozen_slots,
            },
            SplitPane {
                header: scroll_header,
                width: scroll_w,
                slots: scroll_slots,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    const HEADERS: [&str; 3] = ["Name", "Round", "Status"];
    const ROWS: [[&str; 3]; 3] = [
        ["Tabs", "R690", "Done"],
        ["Menu", "R691", "Done"],
        ["Table", "R707", "Active"],
    ];

    fn light() -> Theme {
        Theme::light()
    }

    fn data() -> ([&'static str; 3], Vec<&'static [&'static str]>) {
        let rows: Vec<&'static [&'static str]> = ROWS.iter().map(|r| &r[..]).collect();
        (HEADERS, rows)
    }

    fn all_idle() -> Vec<RadioState> {
        vec![RadioState::Idle; 3]
    }

    #[test]
    fn r707_style_m3_constants() {
        let s = TableStyle::m3();
        assert_eq!(s.col_width, 120);
        assert_eq!(s.row_height, 36);
        assert_eq!(s.header_height, 40);
        assert_eq!(s.corner_radius, 12);
    }

    #[test]
    fn r707_scene_carries_root_header_and_cell_tags() {
        let (headers, rows) = data();
        let scene = Owner::new().run(|| {
            view_table(
                "table",
                TableData {
                    headers: &headers,
                    rows: &rows,
                    row_ids: &[],
                    decoration: None,
                },
                TableSelection {
                    rows: &[],
                    cells: None,
                },
                &all_idle(),
                None,
                &light(),
                &TableStyle::m3(),
            )
        });
        assert!(scene.contains_tag("table"), "composite root tag present");
        assert!(scene.contains_tag("table_hrow"), "header row strip present");
        for col in 0..3 {
            assert!(
                scene.contains_tag(&format!("table_ch{col}")),
                "column header {col} present (presentational, a11y bounds)",
            );
            assert!(
                scene.contains_tag(&format!("table#h{col}")),
                "column header {col} clickable sort tag present",
            );
        }
        for row in 0..3 {
            assert!(
                scene.contains_tag(&format!("table_row{row}")),
                "row strip {row} present",
            );
            for col in 0..3 {
                assert!(
                    scene.contains_tag(&format!("table#{row}_{col}")),
                    "cell {row}_{col} present",
                );
            }
        }
        // No phantom 4th row / column.
        assert!(!scene.contains_tag("table_row3"));
        assert!(!scene.contains_tag("table#0_3"));
    }

    fn collect_text(scene: &Scene, out: &mut Vec<String>) {
        match scene {
            Scene::Text(t) => out.push(t.content.clone()),
            Scene::Container(c) => {
                for child in &c.children {
                    collect_text(child, out);
                }
            }
            // R784 — the grid header / cells now live inside the outer
            // horizontal ScrollNode, so descend through scroll content.
            Scene::Scroll(s) => collect_text(s.content.as_ref(), out),
            _ => {}
        }
    }

    #[test]
    fn r730_sort_glyph_only_on_active_column() {
        let (headers, rows) = data();
        let theme = light();
        let unsorted = Owner::new().run(|| {
            view_table(
                "table",
                TableData {
                    headers: &headers,
                    rows: &rows,
                    row_ids: &[],
                    decoration: None,
                },
                TableSelection {
                    rows: &[],
                    cells: None,
                },
                &all_idle(),
                None,
                &theme,
                &TableStyle::m3(),
            )
        });
        let mut t = Vec::new();
        collect_text(&unsorted, &mut t);
        assert!(
            !t.iter()
                .any(|s| s == crate::glyph::SORT_ASCENDING || s == crate::glyph::SORT_DESCENDING),
            "no glyph when unsorted"
        );

        let sorted = Owner::new().run(|| {
            view_table(
                "table",
                TableData {
                    headers: &headers,
                    rows: &rows,
                    row_ids: &[],
                    decoration: None,
                },
                TableSelection {
                    rows: &[],
                    cells: None,
                },
                &all_idle(),
                Some((1, true)),
                &theme,
                &TableStyle::m3(),
            )
        });
        let mut t2 = Vec::new();
        collect_text(&sorted, &mut t2);
        assert_eq!(
            t2.iter()
                .filter(|s| *s == crate::glyph::SORT_ASCENDING)
                .count(),
            1,
            "one ascending glyph"
        );
        assert!(
            !t2.iter().any(|s| s == crate::glyph::SORT_DESCENDING),
            "no descending glyph for ascending sort"
        );
    }

    #[test]
    fn r730_row_ids_tag_strips_by_data_id_in_visual_order() {
        // Caller passes rows already in visual (sorted) order with the
        // parallel data-id permutation; strips tag by data id.
        let (headers, all) = data();
        // Visual order = data rows [2, 0, 1].
        let reordered: Vec<&[&str]> = vec![all[2], all[0], all[1]];
        let scene = Owner::new().run(|| {
            view_table(
                "table",
                TableData {
                    headers: &headers,
                    rows: &reordered,
                    row_ids: &[2, 0, 1],
                    decoration: None,
                },
                TableSelection {
                    rows: &[],
                    cells: None,
                },
                &all_idle(),
                Some((0, true)),
                &light(),
                &TableStyle::m3(),
            )
        });
        let Scene::Container(root) = &scene else {
            panic!("root container")
        };
        // children[0] = header band; children[1..] = data rows in visual order.
        let strip_tag = |i: usize| {
            let Scene::Container(c) = &root.children[i] else {
                panic!("row strip")
            };
            c.tag.clone().unwrap()
        };
        assert_eq!(strip_tag(1), "table_row2", "1st visual row = data row 2");
        assert_eq!(strip_tag(2), "table_row0", "2nd visual row = data row 0");
        assert_eq!(strip_tag(3), "table_row1", "3rd visual row = data row 1");
    }

    #[test]
    fn r707_selected_row_fill_differs_from_unselected() {
        let theme = light();
        let unselected = row_fill(&theme, RadioState::Idle, false, 0);
        let selected = row_fill(&theme, RadioState::Idle, true, 0);
        assert_ne!(selected, unselected, "selected row washes differently");
        // Even unselected resting row is transparent (shows block fill).
        assert_eq!(unselected, Color::rgba(0, 0, 0, 0));
    }

    #[test]
    fn r707_zebra_stripe_on_odd_rows() {
        let theme = light();
        let even = row_fill(&theme, RadioState::Idle, false, 0);
        let odd = row_fill(&theme, RadioState::Idle, false, 1);
        assert_eq!(even, Color::rgba(0, 0, 0, 0), "even row transparent");
        assert_eq!(
            odd,
            theme.resolve(ColorRole::SurfaceContainer),
            "odd row carries the zebra stripe",
        );
    }

    #[test]
    fn r707_hover_overlay_lerps_toward_on_surface() {
        let theme = light();
        let expected = theme
            .resolve(ColorRole::Surface)
            .lerp(theme.resolve(ColorRole::Accent), 0.16)
            .lerp(theme.resolve(ColorRole::OnSurface), 0.08);
        assert_eq!(row_fill(&theme, RadioState::Hover, true, 0), expected);
    }

    // ── R775 flex-viewport virtualized data-grid ────────────────────

    const VT_HEADERS: [&str; 3] = ["Index", "Name", "Value"];
    const VT_N: usize = 10_000;

    /// R1524 — the three-column fixture dataset, as the per-cell contract asks
    /// for it. Named (not inlined per fixture) because the identical closure
    /// was three call sites wide, and a fixture that disagreed with the others
    /// about a cell's text would read as a windowing difference.
    fn vt_cell(c: CellIndex) -> String {
        match c.col {
            0 => c.row.to_string(),
            1 => format!("Row {}", c.row),
            _ => format!("v{}", c.row),
        }
    }

    /// R1524 — the per-column-width fixtures' dataset: the row index followed
    /// by two fixed markers, so a width assertion never depends on cell text.
    fn vt_marker_cell(c: CellIndex) -> String {
        match c.col {
            0 => c.row.to_string(),
            1 => "x".to_string(),
            _ => "y".to_string(),
        }
    }

    /// R1530 — the fixture column labels, as the per-section contract asks for
    /// them. A section past the fixture's three columns answers with an empty
    /// label rather than panicking, which is the contract's blank-label rule.
    fn vt_header(col: usize) -> String {
        VT_HEADERS.get(col).copied().unwrap_or_default().to_string()
    }

    /// R1523 — a **laid-out** horizontal scroll for the virtual-table fixtures.
    ///
    /// The layout pass publishes a measured viewport for every `Scene::Scroll`,
    /// so a fixture that renders a grid without one models a state the runtime
    /// never paints. That was invisible until R1523 windowed the column axis
    /// against exactly that width — an unmeasured (0) width windows *nothing*,
    /// which is the correct pre-measurement boot state and useless as a fixture.
    ///
    /// The width is wide enough to expose every column, because these fixtures
    /// are about per-column sizing, sort glyphs and the frozen split — the
    /// windowing behaviour gets its own fixtures with a deliberately narrow one
    /// (`run_vtable_narrow`), so a change in windowing cannot quietly rewrite
    /// what the unrelated tests are measuring.
    fn vt_hscroll(offset_x: i32) -> Rc<ScrollState> {
        /// Wider than any fixture's column total (the widest is 550).
        const EXPOSES_EVERY_COLUMN: u32 = 4_000;
        let h = Rc::new(ScrollState::with_tag("vtbl_h"));
        h.set_measured_viewport(EXPOSES_EVERY_COLUMN, 0);
        // Room to move so a non-zero `offset_x` actually applies.
        h.set_max(1000, 0);
        h.scroll_to(offset_x, 0);
        h
    }

    fn run_vtable(measured_h: u32, offset_y: i32) -> Scene {
        run_vtable_h(measured_h, offset_y, 0)
    }

    // ── R1532 §5.27 — a column declares how its cells are painted ───

    /// R1532 — a test painter: a container tagged as the contract requires,
    /// holding one marker text so a walk can tell a delegated cell from a
    /// text one, and one empty child so a structural assertion has something
    /// to count that the built-in painter never produces.
    use core::cell::Cell;

    fn marker_painter(c: &CellRender<'_>) -> Scene {
        Scene::Container(
            ContainerNode::new(vec![
                Scene::Text(TextNode::styled(
                    format!("<{}>", c.text),
                    Rect::default(),
                    TextStyle::new(),
                )),
                Scene::Container(
                    ContainerNode::new(Vec::new())
                        .with_tag(format!("gauge{}_{}", c.index.row, c.index.col)),
                ),
            ])
            .with_tag(c.tag.to_string()),
        )
    }

    /// R1532 — render the virtual table with `col` delegated to
    /// [`marker_painter`], counting how many times the delegate LOOKUP ran.
    fn run_vtable_delegated(col: usize, lookups: &Cell<usize>) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, 200);
        let pitch = i32::try_from(TableStyle::m3().row_height).unwrap();
        state.set_max(0, i32::try_from(VT_N).unwrap() * pitch);
        let h_state = vt_hscroll(0);
        let theme = light();
        let style = TableStyle::m3();
        let pick = |c: usize| {
            lookups.set(lookups.get() + 1);
            (c == col).then_some(&marker_painter as CellPainter<'_>)
        };
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll {
                    body: &state,
                    horizontal: &h_state,
                },
                VirtualTableData {
                    column_count: VT_HEADERS.len(),
                    item_count: VT_N,
                    overscan: 2,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: None,
                    resizable: false,
                    frozen_cols: 0,
                    row_style: None,
                    delegate: Some(&pick),
                },
                &theme,
                &style,
                |_| false,
                GridModel {
                    cell: vt_cell,
                    header: vt_header,
                    decoration: no_decoration,
                },
            )
        })
    }

    /// The seam itself: the delegated column's cells come from the painter,
    /// and every other column's from the built-in one.
    ///
    /// Both halves are load-bearing. Without the second, a delegate that
    /// captured *every* column would pass — and that is the plausible defect,
    /// since the lookup and the paint are separate steps and only the paint
    /// consults the column.
    #[test]
    fn r1532_a_delegated_column_paints_through_its_painter() {
        let lookups = Cell::new(0);
        let scene = run_vtable_delegated(1, &lookups);
        let mut texts = Vec::new();
        collect_text(&scene, &mut texts);
        assert!(
            texts.iter().any(|t| t == "<Row 0>"),
            "column 1 painted through the delegate, got {texts:?}",
        );
        assert!(
            !texts.iter().any(|t| t == "Row 0"),
            "and NOT also through the built-in painter — one cell, one painter",
        );
        assert!(
            texts.iter().any(|t| t == "v0"),
            "column 2 is undelegated and keeps the built-in text painter",
        );
        assert!(
            !texts.iter().any(|t| t == "<v0>"),
            "the delegate is asked per column, so column 2 never reaches it",
        );
    }

    /// The delegated cell keeps its composite tag, so it is still addressable
    /// by pointer routing and by every tag-addressed RPC. A custom column that
    /// silently stopped being clickable is the failure this contract's one
    /// rule exists to prevent.
    #[test]
    fn r1532_a_delegated_cell_keeps_its_hit_tag() {
        let lookups = Cell::new(0);
        let scene = run_vtable_delegated(1, &lookups);
        assert!(
            scene.contains_tag("vtbl#0_1"),
            "the delegated cell carries the same composite tag a text cell does",
        );
        assert!(
            scene.contains_tag("gauge0_1"),
            "and the painter's own children are in the tree beneath it",
        );
    }

    /// The delegate is resolved once per painted **column**, not once per
    /// painted cell — the per-section discipline R1530 gave the header axis.
    ///
    /// Stated as a comparison against the number of painted cells rather than
    /// as a constant, so it keeps its meaning when the fixture's window size
    /// changes. A per-cell resolution would make these two equal.
    #[test]
    fn r1532_the_delegate_is_resolved_per_column_not_per_cell() {
        let lookups = Cell::new(0);
        let scene = run_vtable_delegated(1, &lookups);
        let rows = tags_with_prefix(&scene, "vtbl_row").len();
        let cells = rows * VT_HEADERS.len();
        assert!(rows > 1, "premise: more than one row painted, got {rows}");
        assert_eq!(
            lookups.get(),
            VT_HEADERS.len(),
            "one lookup per painted column ({cells} cells painted)",
        );
    }

    /// A grid with no delegate wired paints exactly what it painted before
    /// R1532 — the built-in painter is reached through the same call as a
    /// custom one, so the default cannot drift into a separate path.
    #[test]
    fn r1532_an_undelegated_grid_is_unchanged() {
        let before = run_vtable(200, 0);
        let mut texts = Vec::new();
        collect_text(&before, &mut texts);
        assert!(
            texts.iter().any(|t| t == "Row 0") && texts.iter().any(|t| t == "v0"),
            "every column takes the built-in text painter, got {texts:?}",
        );
        assert!(
            !texts.iter().any(|t| t.starts_with('<')),
            "and nothing was delegated",
        );
    }

    /// A delegate whose lookup answers `None` for every column is the same
    /// grid as no delegate at all. Discriminating because the two reach the
    /// paint through different code — one resolves and finds nothing, the
    /// other never resolves — and a contract where those differ would make
    /// "wire a delegate for one column" silently restyle the rest.
    #[test]
    fn r1532_a_delegate_that_claims_nothing_changes_nothing() {
        let lookups = Cell::new(0);
        let claimed_none = run_vtable_delegated(VT_HEADERS.len(), &lookups);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        collect_text(&claimed_none, &mut a);
        collect_text(&run_vtable(200, 0), &mut b);
        assert_eq!(
            a, b,
            "a delegate that claims no column paints what no delegate paints",
        );
        assert_eq!(
            lookups.get(),
            VT_HEADERS.len(),
            "and it really was asked — otherwise this proves nothing",
        );
    }

    // ── R1535 §5.27 — a cell is asked for its decoration role ───────

    /// R1535 — the swatch a cell carries, found by walking to the cell's own
    /// tag and reading the fill of a **leaf** child container.
    ///
    /// Located through the cell's tag rather than by collecting every filled
    /// box in the grid, because the grid is full of filled boxes (row strips,
    /// the header band, the block frame) and a test that counted those would
    /// pass on a swatch painted into the wrong cell.
    ///
    /// The swatch is identified by its **square decoration size**, not merely
    /// by being an untagged leaf: the gap spacer beside it is also an untagged
    /// leaf container, so a looser predicate reports the spacer's transparent
    /// fill as a mark whenever the swatch is missing — which is precisely the
    /// case these tests exist to detect. (Measured: it did, until the size
    /// check was added.)
    fn find_tagged<'s>(scene: &'s Scene, tag: &str) -> Option<&'s ContainerNode> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(c);
                }
                c.children.iter().find_map(|ch| find_tagged(ch, tag))
            }
            Scene::Scroll(s) => find_tagged(s.content.as_ref(), tag),
            _ => None,
        }
    }

    /// R1535 — how many children the tagged cell has. The absolute shape a
    /// comparison between two grids cannot check (both go through the same
    /// painter, so a defect there moves both sides alike).
    fn cell_children(scene: &Scene, tag: &str) -> usize {
        find_tagged(scene, tag).map_or(0, |c| c.children.len())
    }

    /// R1536 — found by its **address**, `GridTag::cell_decoration`, rather
    /// than by a structural predicate over the cell's children.
    ///
    /// This is the point of the tag. R1535's version matched "an untagged empty
    /// container of the decoration size", which had to be tightened twice in
    /// one round because the gap spacer and the delegate's gauge bars are also
    /// untagged empty containers — every such predicate names a node by
    /// properties some other node kind can grow. An address cannot be
    /// accidentally satisfied.
    fn cell_swatch(scene: &Scene, root: &str, row: usize, col: usize) -> Option<Color> {
        find_tagged(scene, &GridTag::cell_decoration(root, row, col)).map(|c| c.style.fill)
    }

    /// R1536 — the accessible name the §5.40 derivation would give this cell:
    /// its `aria_label` when set, else its first descendant text. The same
    /// precedence `pinion_a11y::enrich_names_from_scene` applies, read here
    /// from the paint scene the shell hands it.
    fn cell_access_name(scene: &Scene, tag: &str) -> Option<String> {
        let cell = find_tagged(scene, tag)?;
        if let Some(label) = cell.aria_label.as_deref() {
            return Some(label.to_string());
        }
        cell.children.iter().find_map(|ch| match ch {
            Scene::Text(t) => Some(t.content.clone()),
            _ => None,
        })
    }

    /// R1536 — a decorative test mark keyed to `n`, so an assertion can name
    /// which cell's answer it is looking at. `meaning` empty = the `alt=""`
    /// arm; the tests that exercise the meaningful arm spell it out.
    fn test_swatch(n: u8) -> CellDecoration {
        CellDecoration::Swatch {
            color: Color::rgb(n, 0, 0),
            meaning: String::new(),
        }
    }

    /// R1535 — total nodes in a painted scene, so an "additive" claim can be
    /// checked as a count rather than only as text (the gap spacer carries no
    /// text, so a text comparison alone cannot see it).
    fn node_count(scene: &Scene) -> usize {
        1 + match scene {
            Scene::Container(c) => c.children.iter().map(node_count).sum(),
            Scene::Scroll(s) => node_count(s.content.as_ref()),
            _ => 0,
        }
    }

    /// R1535 — render the virtual table with `col` answering the decoration
    /// role, its colour a function of the **row**, counting how many times the
    /// role was asked.
    ///
    /// The colour varies by row on purpose: that is the property a per-column
    /// delegate cannot have, so a fixture with a per-column colour would let a
    /// wrong implementation pass every test below.
    fn run_vtable_decorated(col: usize, asks: &Cell<usize>) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, 200);
        let pitch = i32::try_from(TableStyle::m3().row_height).unwrap();
        state.set_max(0, i32::try_from(VT_N).unwrap() * pitch);
        let h_state = vt_hscroll(0);
        let theme = light();
        let style = TableStyle::m3();
        let decoration = |c: CellIndex| {
            asks.set(asks.get() + 1);
            (c.col == col).then(|| test_swatch(u8::try_from(c.row).unwrap_or(255)))
        };
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll {
                    body: &state,
                    horizontal: &h_state,
                },
                VirtualTableData {
                    column_count: VT_HEADERS.len(),
                    item_count: VT_N,
                    overscan: 2,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: None,
                    resizable: false,
                    frozen_cols: 0,
                    row_style: None,
                    delegate: None,
                },
                &theme,
                &style,
                |_| false,
                GridModel {
                    cell: vt_cell,
                    header: vt_header,
                    decoration,
                },
            )
        })
    }

    /// The seam itself: the decorated column's cells carry the model's mark,
    /// and no other column's do.
    ///
    /// Both halves are load-bearing — without the second, a painter that
    /// decorated *every* cell with the answer it got for one would pass.
    #[test]
    fn r1535_a_decorated_cell_carries_the_models_swatch() {
        let asks = Cell::new(0);
        let scene = run_vtable_decorated(1, &asks);
        assert_eq!(
            cell_swatch(&scene, "vtbl", 0, 1),
            Some(Color::rgb(0, 0, 0)),
            "the decorated cell carries the colour the model answered with",
        );
        assert_eq!(
            cell_swatch(&scene, "vtbl", 0, 2),
            None,
            "and a cell whose role answered `None` carries no mark at all",
        );
        assert_eq!(
            cell_children(&scene, "vtbl#0_1"),
            3,
            "the decorated cell is swatch + gap + label",
        );
        assert_eq!(
            cell_children(&scene, "vtbl#0_2"),
            1,
            "and the undecorated one is the label alone",
        );
    }

    /// **The reason this is a role and not a delegate.** Two rows of the same
    /// column carry different marks, which a per-column painter cannot express
    /// — R1532's delegate is resolved once for every row it draws.
    ///
    /// Asserted as an inequality between two observed colours plus each one's
    /// expected value: the inequality alone would pass on a swatch that varied
    /// for the wrong reason (say, by zebra parity).
    #[test]
    fn r1535_the_decoration_varies_by_row() {
        let asks = Cell::new(0);
        let scene = run_vtable_decorated(1, &asks);
        let (r0, r1) = (
            cell_swatch(&scene, "vtbl", 0, 1),
            cell_swatch(&scene, "vtbl", 1, 1),
        );
        assert_eq!(r0, Some(Color::rgb(0, 0, 0)), "row 0's own answer");
        assert_eq!(r1, Some(Color::rgb(1, 0, 0)), "row 1's own answer");
        assert_ne!(r0, r1, "so one column's mark is a function of the row");
    }

    /// The role is asked once per painted **cell** — the R1524 discipline the
    /// display role follows — not once per column the way a delegate is.
    ///
    /// Stated against the painted-cell count rather than a constant so it keeps
    /// its meaning when the fixture's window changes. A per-column resolution
    /// would make `asks` equal `VT_HEADERS.len()`, which is what R1532's
    /// delegate test asserts — the two contracts are deliberately opposite.
    #[test]
    fn r1535_the_decoration_is_asked_per_cell() {
        let asks = Cell::new(0);
        let scene = run_vtable_decorated(1, &asks);
        let rows = tags_with_prefix(&scene, "vtbl_row").len();
        assert!(rows > 1, "premise: more than one row painted, got {rows}");
        assert_eq!(
            asks.get(),
            rows * VT_HEADERS.len(),
            "one ask per painted cell ({rows} rows x {} columns)",
            VT_HEADERS.len(),
        );
    }

    /// The swatch must not shadow the cell it decorates: the cell keeps its
    /// composite tag, and the mark is pointer-transparent, so a click landing
    /// on the swatch still routes to the cell. A decoration that quietly made
    /// part of a cell unclickable is the failure this checks.
    #[test]
    fn r1535_the_swatch_does_not_shadow_the_cell() {
        let asks = Cell::new(0);
        let scene = run_vtable_decorated(1, &asks);
        assert!(
            scene.contains_tag("vtbl#0_1"),
            "the decorated cell carries the same composite tag a plain one does",
        );
        // Read out of the PAINTED tree, not from a freshly-built
        // `swatch_node`: a guard that constructs its own copy of the subject
        // tests the copy, and would keep passing if the paint path stopped
        // using the constructor.
        let mark = find_tagged(&scene, &GridTag::cell_decoration("vtbl", 0, 1))
            .expect("the painted cell carries a swatch");
        assert!(
            mark.layout.pointer_transparent,
            "and the mark is inert to hit routing",
        );
        // R1536 corrects this test's own reasoning. It used to assert the mark
        // carried NO tag, "to be hit by" — but a tag and hit-testability are
        // independent axes: `pinion_overlay::focus_ring` is tagged and
        // pointer-transparent. Being addressable is what makes the mark
        // answerable over RPC; being pointer-transparent is what keeps the cell
        // the click target. The mark is both, and the second is what this test
        // is actually about.
        assert!(
            !mark.tag.as_deref().unwrap_or_default().contains('#'),
            "and its address stays in the '_' presentational family, so it \
             never enters the composite click-router namespace",
        );
    }

    /// A grid whose model answers the role with `None` everywhere paints
    /// exactly what a grid with [`no_decoration`] paints — text, node shape and
    /// all. Discriminating because the two reach the painter through different
    /// values (`Some(fn)` returning `None` vs the constant), and because the
    /// gap spacer is emitted only when a mark is present: a painter that always
    /// spent the gap would separate these two trees.
    #[test]
    fn r1535_a_model_that_decorates_nothing_changes_nothing() {
        let asks = Cell::new(0);
        // A column index past the last one: the role is asked for every cell
        // and answers `None` every time.
        let claimed_none = run_vtable_decorated(VT_HEADERS.len(), &asks);
        let plain = run_vtable(200, 0);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        collect_text(&claimed_none, &mut a);
        collect_text(&plain, &mut b);
        assert_eq!(a, b, "same text");
        assert_eq!(
            cell_swatch(&claimed_none, "vtbl", 0, 1),
            None,
            "and no cell grew a mark",
        );
        assert_eq!(
            node_count(&claimed_none),
            node_count(&plain),
            "and the two model shapes paint the same number of nodes",
        );
        assert!(asks.get() > 0, "and it really was asked");
        // The claim above is a COMPARISON, and both grids reach the same
        // painter — so a defect in the painter itself moves both sides equally
        // and survives it. (Measured: making the gap spacer unconditional
        // passes every assertion above.) The claim "an undecorated cell emits
        // the pre-R1535 node" is about one cell's absolute shape, so it is
        // asserted as one.
        assert_eq!(
            cell_children(&claimed_none, "vtbl#0_1"),
            1,
            "an undecorated cell is exactly its label: no swatch, no gap",
        );
    }

    /// R1535 — the decoration reaches **both** panes of a frozen-column grid.
    ///
    /// Its own test because the frozen split builds its two panes through
    /// separate row closures, so a role wired into one and not the other is a
    /// live defect that every unsplit test above would miss (the R1523 shape:
    /// a change applied to one pane is a question about the other).
    #[test]
    fn r1535_both_frozen_panes_decorate() {
        let asks = Cell::new(0);
        let deco = |c: CellIndex| {
            asks.set(asks.get() + 1);
            Some(test_swatch(u8::try_from(c.col).unwrap_or(255)))
        };
        let scene = run_vtable_wide_with(
            600,
            0,
            2,
            GridModel {
                cell: |c: CellIndex| format!("r{}c{}", c.row, c.col),
                header: wide_header,
                decoration: deco,
            },
        );
        assert_eq!(
            cell_swatch(&scene, "vtbl", 0, 0),
            Some(Color::rgb(0, 0, 0)),
            "the pinned pane's cell is decorated",
        );
        assert_eq!(
            cell_swatch(&scene, "vtbl", 0, 2),
            Some(Color::rgb(2, 0, 0)),
            "and so is the scrolling pane's, with its own absolute column's answer",
        );
    }

    /// R1535 — a scrolled **unsplit** grid asks the role about the columns it
    /// is actually painting, in absolute coordinates.
    ///
    /// Its own test because every other one here runs at horizontal offset 0,
    /// where the column window starts at column 0 and a relative-vs-absolute
    /// confusion is invisible. (Measured: asking `0..count` instead of the
    /// window's own span passes all six of the other tests.) The frozen grid's
    /// panes are covered separately — this is the path that has no frozen pane
    /// to borrow correctness from.
    #[test]
    fn r1535_a_scrolled_grid_decorates_its_absolute_columns() {
        let scene = run_vtable_wide_with(
            560,
            4_000,
            0,
            GridModel {
                cell: |c: CellIndex| format!("r{}c{}", c.row, c.col),
                header: wide_header,
                decoration: |c: CellIndex| {
                    Some(test_swatch(u8::try_from(c.col % 256).unwrap_or(0)))
                },
            },
        );
        // Whichever columns the 560px window landed on, each painted cell's
        // mark must be its OWN column's answer — derived from the tag rather
        // than hard-coded, so the assertion survives a widths change.
        let cells = tags_with_prefix(&scene, "vtbl#0_");
        assert!(!cells.is_empty(), "premise: row 0 painted some cells");
        let mut checked = 0;
        for tag in &cells {
            let col: usize = tag.rsplit('_').next().unwrap().parse().unwrap();
            assert_eq!(
                cell_swatch(&scene, "vtbl", 0, col),
                Some(Color::rgb(u8::try_from(col % 256).unwrap_or(0), 0, 0)),
                "cell {tag} carries column {col}'s own answer",
            );
            checked += 1;
        }
        let first: usize = cells[0].rsplit('_').next().unwrap().parse().unwrap();
        assert!(
            first > 0,
            "premise: the window is scrolled off column 0, else this test \
             cannot tell absolute from relative (first painted column {first})",
        );
        assert!(checked > 1, "premise: more than one column checked");
    }

    // ── R1536 §5.27 §5.40 — a decoration states what it means ───────

    /// R1536 — render the virtual table with column 1 decorated by a mark whose
    /// `meaning` is `meaning`, and column 1's display text set to `text`.
    ///
    /// Both are parameters because the contract's two arms are exactly the two
    /// combinations: a mark beside text that restates it (decorative), and a
    /// mark that is the only thing in its cell (meaningful).
    fn run_vtable_meaning(meaning: &str, text: &'static str) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, 200);
        let pitch = i32::try_from(TableStyle::m3().row_height).unwrap();
        state.set_max(0, i32::try_from(VT_N).unwrap() * pitch);
        let h_state = vt_hscroll(0);
        let theme = light();
        let style = TableStyle::m3();
        let owned = meaning.to_string();
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll {
                    body: &state,
                    horizontal: &h_state,
                },
                VirtualTableData {
                    column_count: VT_HEADERS.len(),
                    item_count: VT_N,
                    overscan: 2,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: None,
                    resizable: false,
                    frozen_cols: 0,
                    row_style: None,
                    delegate: None,
                },
                &theme,
                &style,
                |_| false,
                GridModel {
                    cell: |c: CellIndex| {
                        if c.col == 1 {
                            text.to_string()
                        } else {
                            vt_cell(c)
                        }
                    },
                    header: vt_header,
                    decoration: |c: CellIndex| {
                        (c.col == 1).then(|| CellDecoration::Swatch {
                            color: Color::rgb(0, 0, 0),
                            meaning: owned.clone(),
                        })
                    },
                },
            )
        })
    }

    /// The mark is **addressable**: a client asks for cell `(r, c)`'s
    /// decoration by name instead of walking the cell's children by position.
    ///
    /// The address is per cell, so the negative half — a cell whose role
    /// answered `None` has no such node — is what proves the name is not
    /// emitted unconditionally.
    #[test]
    fn r1536_the_mark_has_an_address() {
        let asks = Cell::new(0);
        let scene = run_vtable_decorated(1, &asks);
        assert!(
            scene.contains_tag(&GridTag::cell_decoration("vtbl", 0, 1)),
            "the decorated cell's mark is addressable",
        );
        assert!(
            scene.contains_tag(&GridTag::cell_decoration("vtbl", 1, 1)),
            "and so is the next row's, at its own address",
        );
        assert!(
            !scene.contains_tag(&GridTag::cell_decoration("vtbl", 0, 2)),
            "an undecorated cell has no decoration node to address",
        );
    }

    /// R1536 — the `QIcon` arm paints an image at the decoration square, with
    /// the same address, meaning and layout rules the colour arm has.
    ///
    /// Both halves matter: an icon that lost the shared rules would be a second
    /// decoration contract, which is what having two arms is supposed to avoid.
    #[test]
    fn r1536_the_icon_arm_paints_an_image() {
        let side = TableStyle::m3().decoration_px;
        let node = icon_node("memory://k", side, "t_deco0_1");
        let Scene::Image(img) = &node else {
            panic!("the icon arm is an Image node, not a filled box")
        };
        assert_eq!(img.source, "memory://k", "the model's source reaches paint");
        assert_eq!(
            img.style.fit,
            Fit::Contain,
            "aspect preserved — a stretched glyph is a different glyph",
        );
        assert_eq!(img.tag.as_deref(), Some("t_deco0_1"), "same address rule");
        // `abs() < f32::EPSILON` rather than `== 0.0`: the value is a
        // literal here, but clippy rejects float equality on principle and the
        // principle is right — a computed shrink factor would compare wrong.
        assert!(
            img.layout.flex_shrink.abs() < f32::EPSILON,
            "same no-shrink rule"
        );
        assert!(img.layout.pointer_transparent, "same hit-transparency rule");
        let px = pinion_core::style::SizeValue::Px(side);
        assert_eq!(img.layout.size.width, px, "same declared square");
        // And the meaning accessor answers for both arms, so the painter never
        // has to match on which one it got.
        assert_eq!(
            CellDecoration::Icon {
                source: "s".into(),
                meaning: "Folder".into()
            }
            .meaning(),
            "Folder",
        );
        assert_eq!(
            CellDecoration::Swatch {
                color: Color::rgb(0, 0, 0),
                meaning: String::new()
            }
            .meaning(),
            "",
        );
    }

    /// R1536 — the EAGER `view_table` answers the decoration role too, so the
    /// tree no longer holds two cell-paint contracts that disagree about
    /// whether the role exists.
    #[test]
    fn r1536_the_eager_table_answers_the_decoration_role() {
        let (headers, rows) = data();
        let theme = light();
        let ask = |c: CellIndex| {
            (c.col == 1).then(|| CellDecoration::Swatch {
                color: Color::rgb(u8::try_from(c.row).unwrap_or(0), 0, 0),
                meaning: "Marked".to_string(),
            })
        };
        let render = |deco: Option<&dyn Fn(CellIndex) -> Option<CellDecoration>>| {
            Owner::new().run(|| {
                view_table(
                    "table",
                    TableData {
                        headers: &headers,
                        rows: &rows,
                        row_ids: &[],
                        decoration: deco,
                    },
                    TableSelection {
                        rows: &[],
                        cells: None,
                    },
                    &all_idle(),
                    None,
                    &theme,
                    &TableStyle::m3(),
                )
            })
        };
        let decorated = render(Some(&ask));
        assert_eq!(
            cell_swatch(&decorated, "table", 0, 1),
            Some(Color::rgb(0, 0, 0)),
            "the eager grid paints the model's mark, at the same address",
        );
        assert_eq!(
            cell_swatch(&decorated, "table", 1, 1),
            Some(Color::rgb(1, 0, 0)),
            "and per ROW, the axis that makes it a role",
        );
        assert_eq!(
            cell_swatch(&decorated, "table", 0, 0),
            None,
            "an undecorated column has no mark",
        );
        // Derived from the fixture, not spelled: the claim is that the meaning
        // precedes THIS cell's own text, and a literal would state the fixture.
        assert_eq!(
            cell_access_name(&decorated, "table#0_1").as_deref(),
            Some(format!("Marked {}", rows[0][1]).as_str()),
            "and the meaning joins the accessible name here too",
        );
        // The negative control: `None` is the pre-R1536 tree.
        let plain = render(None);
        assert_eq!(cell_swatch(&plain, "table", 0, 1), None);
        assert_eq!(cell_children(&plain, "table#0_1"), 1, "label alone");
    }

    /// R1536 — a data row is named from its cells, and that is intended.
    ///
    /// R1536 made the derivation reach inside a scroll, which gave every
    /// `AccessNode` in a grid a name — including the `row` containers, which had
    /// none before. That is a behaviour change nothing asked for, so it is
    /// stated here rather than left to be discovered: WAI-ARIA 1.2 lists `row`
    /// among the roles that support **name from content**, so a row taking its
    /// cells' text is conformant, not a leak. pinion's name-from-content is the
    /// FIRST text leaf rather than the concatenation of all of them (the
    /// long-standing `walk_for_text` rule), so the name is the row's leading
    /// cell — which for a data grid is the row header column.
    ///
    /// The load-bearing half is the second assertion: the row is named from a
    /// CELL and not from the grid or a neighbour.
    #[test]
    fn r1536_a_row_is_named_from_its_leading_cell() {
        let scene = run_vtable(200, 0);
        let row = find_tagged(&scene, "vtbl_row0").expect("row 0 is in the tree");
        assert!(
            row.aria_label.is_none(),
            "no override — the name comes from content, the ARIA rung this row \
             is entitled to",
        );
        let first_text = |c: &ContainerNode| -> Option<String> {
            fn dfs(s: &Scene) -> Option<String> {
                match s {
                    Scene::Text(t) => Some(t.content.clone()),
                    Scene::Container(c) => c.children.iter().find_map(dfs),
                    Scene::Scroll(s) => dfs(&s.content),
                    _ => None,
                }
            }
            c.children.iter().find_map(dfs)
        };
        let want = find_tagged(&scene, "vtbl#0_0")
            .and_then(first_text)
            .expect("the leading cell has text");
        assert_eq!(
            first_text(row).as_deref(),
            Some(want.as_str()),
            "the row names itself from its LEADING CELL's text",
        );
    }

    /// R1536 — the mark keeps its declared size when the cell is tight.
    ///
    /// Found by the demo, not by a unit test: at a 75px column the flex pass
    /// shrank a 10px swatch to 6px against its sibling label. Qt draws the
    /// decoration at `iconSize` and elides the *text*; a mark that silently
    /// resizes is a mark whose colour area — the only thing it encodes — is a
    /// function of the column width.
    #[test]
    fn r1536_the_mark_does_not_shrink() {
        let side = TableStyle::m3().decoration_px;
        let Scene::Container(mark) = swatch_node(Color::rgb(1, 2, 3), side, "t") else {
            panic!("a swatch is a container")
        };
        let px = pinion_core::style::SizeValue::Px(side);
        assert_eq!(mark.layout.size.width, px, "declared width");
        assert!(
            mark.layout.flex_shrink.abs() < f32::EPSILON,
            "and a flex-shrink of 0, so the flex pass cannot take it back",
        );
    }

    /// A mark that **restates the cell's text** is decorative: it contributes
    /// nothing to the accessible name, so a screen reader says the status once.
    /// HTML's `alt=""`, and the reason `meaning` is a model answer rather than
    /// something the framework guesses.
    #[test]
    fn r1536_a_decorative_mark_is_silent() {
        let scene = run_vtable_meaning("", "Active");
        assert_eq!(
            cell_access_name(&scene, "vtbl#0_1").as_deref(),
            Some("Active"),
            "the cell names itself from its own label, exactly as before R1535",
        );
        assert!(
            find_tagged(&scene, "vtbl#0_1").is_some_and(|c| c.aria_label.is_none()),
            "and no override was written at all — a decorative mark leaves the \
             AT tree byte-identical",
        );
    }

    /// A mark that carries information the text does not **joins the cell's
    /// accessible name**, ahead of the label — what a browser does for
    /// `<td><img alt="Overdue"> 3 days</td>`.
    ///
    /// This is the arm Qt has no answer for: `QAccessibleTableCell::text(Name)`
    /// returns the display role, and the decoration contributes nothing, so a
    /// colour-only status column is an empty cell to a screen-reader user.
    #[test]
    fn r1536_a_meaningful_mark_is_announced() {
        let scene = run_vtable_meaning("Overdue", "3 days");
        assert_eq!(
            cell_access_name(&scene, "vtbl#0_1").as_deref(),
            Some("Overdue 3 days"),
            "the mark's meaning precedes the label in the composed name",
        );
    }

    /// A **mark-only** cell — no display text — is named by its mark alone, with
    /// no trailing separator. Without this the cell is silent, which is the
    /// whole failure the meaning field exists to prevent.
    #[test]
    fn r1536_a_mark_only_cell_is_named_by_its_mark() {
        let scene = run_vtable_meaning("Flagged", "");
        assert_eq!(
            cell_access_name(&scene, "vtbl#0_1").as_deref(),
            Some("Flagged"),
            "named by the mark alone, and NOT `\"Flagged \"` — a client that \
             string-matches an announced name would not notice the space",
        );
        // The negative control: without the meaning this exact cell is silent.
        let silent = run_vtable_meaning("", "");
        assert_eq!(
            cell_access_name(&silent, "vtbl#0_1").as_deref(),
            Some(""),
            "premise: the same mark-only cell with a decorative mark has no \
             name to give — which is what makes the assertion above load-bearing",
        );
    }

    /// R784 — render the virtual table with an explicit horizontal
    /// offset so the frozen-header / h-scroll tests can drive the outer
    /// horizontal scroll.
    fn run_vtable_h(measured_h: u32, offset_y: i32, offset_x: i32) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, measured_h);
        let pitch = i32::try_from(TableStyle::m3().row_height).unwrap();
        state.set_max(0, i32::try_from(VT_N).unwrap() * pitch);
        state.scroll_to(0, offset_y);
        let h_state = vt_hscroll(offset_x);
        let theme = light();
        let style = TableStyle::m3();
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll {
                    body: &state,
                    horizontal: &h_state,
                },
                VirtualTableData {
                    column_count: VT_HEADERS.len(),
                    item_count: VT_N,
                    overscan: 2,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: None,
                    resizable: false,
                    frozen_cols: 0,
                    row_style: None,
                    delegate: None,
                },
                &theme,
                &style,
                |_| false,
                GridModel {
                    cell: vt_cell,
                    header: vt_header,
                    decoration: no_decoration,
                },
            )
        })
    }

    /// Every container tag in the scene that starts with `prefix`, in document
    /// order. (R1523 generalised this from the `count_vt_rows`-only walk: the
    /// column axis needs the tags themselves, not just how many there are.)
    fn tags_with_prefix(scene: &Scene, prefix: &str) -> Vec<String> {
        fn walk(scene: &Scene, prefix: &str, out: &mut Vec<String>) {
            match scene {
                Scene::Container(c) => {
                    if let Some(tag) = c.tag.as_deref().filter(|t| t.starts_with(prefix)) {
                        out.push(tag.to_string());
                    }
                    for child in &c.children {
                        walk(child, prefix, out);
                    }
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), prefix, out),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, prefix, &mut out);
        out
    }

    /// Count `vtbl_row<id>` data-row strips anywhere in the scene.
    fn count_vt_rows(scene: &Scene) -> usize {
        tags_with_prefix(scene, "vtbl_row").len()
    }

    /// Find the **body** (vertical) [`ScrollNode`]. R784 wraps the grid in
    /// an outer horizontal scroll, so the first scroll encountered is the
    /// horizontal one; the body is the nested `ScrollAxis::Vertical` node.
    fn find_vt_scroll(scene: &Scene) -> Option<&pinion_core::scene::ScrollNode> {
        match scene {
            Scene::Scroll(s) if s.axis == ScrollAxis::Vertical => Some(s),
            Scene::Scroll(s) => find_vt_scroll(s.content.as_ref()),
            Scene::Container(c) => c.children.iter().find_map(find_vt_scroll),
            _ => None,
        }
    }

    /// Find the **outer horizontal** [`ScrollNode`] (R784).
    fn find_vt_hscroll(scene: &Scene) -> Option<&pinion_core::scene::ScrollNode> {
        match scene {
            Scene::Scroll(s) if s.axis == ScrollAxis::Horizontal => Some(s),
            Scene::Scroll(s) => find_vt_hscroll(s.content.as_ref()),
            Scene::Container(c) => c.children.iter().find_map(find_vt_hscroll),
            _ => None,
        }
    }

    /// R859 — render the virtual table with `frozen_cols` leading columns
    /// pinned (and a non-zero horizontal offset so the split is exercised).
    fn run_vtable_frozen(measured_h: u32, offset_x: i32, frozen_cols: usize) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, measured_h);
        let pitch = i32::try_from(TableStyle::m3().row_height).unwrap();
        state.set_max(0, i32::try_from(VT_N).unwrap() * pitch);
        let h_state = vt_hscroll(offset_x);
        let theme = light();
        let style = TableStyle::m3();
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll {
                    body: &state,
                    horizontal: &h_state,
                },
                VirtualTableData {
                    column_count: VT_HEADERS.len(),
                    item_count: VT_N,
                    overscan: 2,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: None,
                    resizable: false,
                    frozen_cols,
                    row_style: None,
                    delegate: None,
                },
                &theme,
                &style,
                |_| false,
                GridModel {
                    cell: vt_cell,
                    header: vt_header,
                    decoration: no_decoration,
                },
            )
        })
    }

    /// Collect the `follower` flag of every vertical [`ScrollNode`] in
    /// document order (frozen pane first, scrolled pane second).
    fn collect_vertical_followers(scene: &Scene, out: &mut Vec<bool>) {
        match scene {
            Scene::Scroll(s) => {
                if s.axis == ScrollAxis::Vertical {
                    out.push(s.follower);
                }
                collect_vertical_followers(s.content.as_ref(), out);
            }
            Scene::Container(c) => {
                for ch in &c.children {
                    collect_vertical_followers(ch, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn r859_frozen_grid_splits_into_frozen_and_scrolling_headers() {
        // frozen_cols = 1 → column 0 ("Index") pinned, columns 1..3 scroll.
        let scene = run_vtable_frozen(360, 200, 1);
        // Two distinct header-row containers, never a duplicate tag.
        assert!(
            scene.contains_tag("vtbl_fhrow"),
            "frozen header band present"
        );
        assert!(
            scene.contains_tag("vtbl_hrow"),
            "scrolled header band present"
        );
        // Frozen header owns column 0; scrolled header owns columns 1 + 2.
        assert!(
            scene.contains_tag("vtbl_ch0"),
            "frozen pane carries col 0 header"
        );
        assert!(
            scene.contains_tag("vtbl_ch1"),
            "scrolled pane carries col 1 header"
        );
        assert!(
            scene.contains_tag("vtbl_ch2"),
            "scrolled pane carries col 2 header"
        );
    }

    #[test]
    fn r859_frozen_cells_keep_absolute_column_send_tags() {
        // The split must not renumber columns: the frozen pane's cell keeps
        // col 0, the scrolled pane's keep cols 1 + 2 (absolute), so a click
        // routes to the right data column across the freeze boundary.
        let scene = run_vtable_frozen(360, 200, 1);
        for col in 0..3 {
            let tag = format!("vtbl#{}", GridSendKey::Cell { row: 0, col }.encode());
            assert!(
                scene.contains_tag(&tag),
                "cell {tag} present across the split"
            );
        }
        // Frozen + scrolled data-row strips use distinct container tags.
        assert!(scene.contains_tag("vtbl_frow0"), "frozen pane row strip");
        assert!(scene.contains_tag("vtbl_row0"), "scrolled pane row strip");
    }

    #[test]
    fn r859_frozen_pane_body_is_follower_scrolled_pane_is_primary() {
        // The two panes share the vertical body scroll: exactly one is the
        // primary (publishes bounds) and one is the follower (R859) — the
        // linked-scroll invariant that avoids the measured-viewport
        // oscillation. Document order is [frozen (follower), scrolled].
        let scene = run_vtable_frozen(360, 0, 1);
        let mut followers = Vec::new();
        collect_vertical_followers(&scene, &mut followers);
        assert_eq!(followers.len(), 2, "two vertical scrolls (one per pane)");
        assert_eq!(
            followers.iter().filter(|&&f| f).count(),
            1,
            "exactly one follower; the other is the primary that owns bounds",
        );
        assert_eq!(
            followers,
            vec![true, false],
            "frozen pane follows, scrolled pane leads"
        );
    }

    #[test]
    fn r859_frozen_cols_zero_stays_unsplit_r784_grid() {
        // The default (every pre-R859 caller) renders no frozen pane.
        let scene = run_vtable_frozen(360, 0, 0);
        assert!(scene.contains_tag("vtbl_hrow"), "single header band");
        assert!(
            !scene.contains_tag("vtbl_fhrow"),
            "no frozen header band when unsplit"
        );
        let mut followers = Vec::new();
        collect_vertical_followers(&scene, &mut followers);
        assert_eq!(
            followers,
            vec![false],
            "one primary vertical scroll, no follower"
        );
    }

    #[test]
    fn r859_frozen_cols_clamped_to_keep_one_scrollable_column() {
        // frozen_cols >= cols is clamped to cols - 1 (= 2 here): columns 0
        // and 1 freeze, column 2 stays scrollable. Must not panic on the
        // over-range request, and the scrolled pane keeps column 2.
        let scene = run_vtable_frozen(360, 200, 99);
        assert!(
            scene.contains_tag("vtbl_fhrow"),
            "frozen pane exists (clamped, not unsplit)"
        );
        assert!(scene.contains_tag("vtbl_ch0"), "col 0 frozen");
        assert!(scene.contains_tag("vtbl_ch1"), "col 1 frozen");
        assert!(scene.contains_tag("vtbl_ch2"), "col 2 stays scrollable");
    }

    #[test]
    fn r775_virtual_table_windows_body_not_whole_dataset() {
        let scene = run_vtable(360, 0);
        let rendered = count_vt_rows(&scene);
        assert!(rendered >= 9, "covers the visible band, got {rendered}");
        assert!(rendered < 30, "windowed body, not {VT_N} rows: {rendered}");
    }

    #[test]
    fn r775_virtual_table_rendered_count_grows_with_measured_height() {
        let short = count_vt_rows(&run_vtable(360, 0));
        let tall = count_vt_rows(&run_vtable(720, 0));
        assert!(
            tall > short,
            "taller viewport => more body rows: {tall} vs {short}"
        );
        assert!(tall < 40, "still a window: {tall}");
    }

    #[test]
    fn r784_virtual_table_header_frozen_above_body_inside_h_scroll() {
        // R784 — the grid is `frame > h-scroll > column[header, body]`.
        // The header sits ABOVE the inner vertical body scroll but INSIDE
        // the outer horizontal scroll, so it is pinned vertically yet
        // tracks the body's horizontal offset.
        let scene = run_vtable(360, 0);
        let Scene::Container(frame) = &scene else {
            panic!("root frame is a Container")
        };
        let Scene::Scroll(h) = &frame.children[0] else {
            panic!("frame's only child is the outer horizontal ScrollNode")
        };
        assert_eq!(h.axis, ScrollAxis::Horizontal, "outer scroll is horizontal");
        let Scene::Container(column) = h.content.as_ref() else {
            panic!("the horizontal scroll wraps the [header, body] column")
        };
        let header_first = matches!(
            &column.children[0],
            Scene::Container(c) if c.tag.as_deref() == Some("vtbl_hrow")
        );
        assert!(
            header_first,
            "frozen header band is the column's first child"
        );
        let body_is_vertical_scroll = matches!(
            &column.children[1],
            Scene::Scroll(b) if b.axis == ScrollAxis::Vertical
        );
        assert!(
            body_is_vertical_scroll,
            "the body is a vertical ScrollNode below the header (pinned vertically)",
        );
    }

    #[test]
    fn r784_virtual_table_outer_scroll_is_horizontal_flex_grow_with_state() {
        let scene = run_vtable(360, 0);
        let h = find_vt_hscroll(&scene).expect("outer horizontal Scene::Scroll present");
        assert_eq!(
            h.axis,
            ScrollAxis::Horizontal,
            "outer scroll axis is horizontal"
        );
        assert!(
            (h.layout.flex_grow - 1.0).abs() < f32::EPSILON,
            "outer horizontal ScrollNode flex-grows to fill the framed interior",
        );
        assert!(h.state.is_some(), "outer scroll carries its ScrollState Rc");
        assert_eq!(
            h.tag.as_deref(),
            Some("vtbl_h"),
            "derived from the h_scroll tag"
        );
    }

    /// Walk to the container tagged `tag` and return its layout-sidecar pixel
    /// width (`None` if absent or not a `Px` size). R785 cells carry their
    /// per-column width here.
    fn cell_layout_width(scene: &Scene, tag: &str) -> Option<u32> {
        use pinion_core::style::SizeValue;
        fn walk(s: &Scene, tag: &str) -> Option<u32> {
            match s {
                Scene::Container(c) => {
                    if c.tag.as_deref() == Some(tag) {
                        if let SizeValue::Px(w) = c.layout.size.width {
                            return Some(w);
                        }
                    }
                    c.children.iter().find_map(|ch| walk(ch, tag))
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), tag),
                _ => None,
            }
        }
        walk(scene, tag)
    }

    #[test]
    fn r785_per_column_widths_size_each_cell() {
        // R785 — `col_widths: Some(..)` sizes each column's cell individually
        // (vs the uniform `style.col_width` fallback). The per-column width is
        // threaded into each data cell's layout sidecar.
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, 360);
        let h_state = vt_hscroll(0);
        let widths = [200u32, 50, 300];
        let scene = Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll {
                    body: &state,
                    horizontal: &h_state,
                },
                VirtualTableData {
                    column_count: VT_HEADERS.len(),
                    item_count: 5,
                    overscan: 1,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: Some(&widths),
                    resizable: false,
                    frozen_cols: 0,
                    row_style: None,
                    delegate: None,
                },
                &light(),
                &TableStyle::m3(),
                |_| false,
                GridModel {
                    cell: vt_marker_cell,
                    header: vt_header,
                    decoration: no_decoration,
                },
            )
        });
        assert_eq!(
            cell_layout_width(&scene, "vtbl#0_0"),
            Some(200),
            "col 0 cell width"
        );
        assert_eq!(
            cell_layout_width(&scene, "vtbl#0_1"),
            Some(50),
            "col 1 cell width"
        );
        assert_eq!(
            cell_layout_width(&scene, "vtbl#0_2"),
            Some(300),
            "col 2 cell width"
        );
    }

    #[test]
    fn r785_no_col_widths_falls_back_to_uniform() {
        // R785 — `col_widths: None` keeps the uniform `style.col_width` (the
        // pre-R785 behaviour every existing grid relies on).
        let scene = run_vtable(360, 0);
        let uniform = TableStyle::m3().col_width;
        assert_eq!(
            cell_layout_width(&scene, "vtbl#0_0"),
            Some(uniform),
            "uniform fallback"
        );
        assert_eq!(
            cell_layout_width(&scene, "vtbl#0_1"),
            Some(uniform),
            "uniform fallback"
        );
    }

    fn run_vtable_resizable(widths: &[u32], resizable: bool) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, 360);
        let h_state = vt_hscroll(0);
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll {
                    body: &state,
                    horizontal: &h_state,
                },
                VirtualTableData {
                    column_count: VT_HEADERS.len(),
                    item_count: 5,
                    overscan: 1,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: Some(widths),
                    resizable,
                    frozen_cols: 0,
                    row_style: None,
                    delegate: None,
                },
                &light(),
                &TableStyle::m3(),
                |_| false,
                GridModel {
                    cell: vt_marker_cell,
                    header: vt_header,
                    decoration: no_decoration,
                },
            )
        })
    }

    #[test]
    fn r786_resizable_header_paints_grabber_and_reserves_width() {
        // R786 — a resizable header reserves `resize_handle_w` of each cell's
        // trailing edge for a grabber tagged "<tag>_ch<col>#resize"; the
        // clickable content area shrinks by that much, but the DATA cells keep
        // the full per-column width (the grabber lives only in the header).
        let handle_w = TableStyle::m3().resize_handle_w;
        let scene = run_vtable_resizable(&[200, 50, 300], true);
        // One grabber per column, each `resize_handle_w` wide.
        for col in 0..3 {
            assert_eq!(
                cell_layout_width(&scene, &format!("vtbl_ch{col}#resize")),
                Some(handle_w),
                "col {col} grabber present and sized",
            );
        }
        // Header click/content area is the column width minus the grabber.
        assert_eq!(
            cell_layout_width(&scene, "vtbl#h0"),
            Some(200 - handle_w),
            "col 0 content"
        );
        assert_eq!(
            cell_layout_width(&scene, "vtbl#h1"),
            Some(50 - handle_w),
            "col 1 content"
        );
        // Data cells keep the full per-column width (no grabber).
        assert_eq!(
            cell_layout_width(&scene, "vtbl#0_0"),
            Some(200),
            "data cell full width"
        );
        assert_eq!(
            cell_layout_width(&scene, "vtbl#0_2"),
            Some(300),
            "data cell full width"
        );
    }

    #[test]
    fn r786_non_resizable_header_has_no_grabber() {
        // R786 — `resizable: false` is byte-identical to the R785 header: no
        // grabber tag, and the content area is the full column width.
        let scene = run_vtable_resizable(&[200, 50, 300], false);
        assert!(
            !scene.contains_tag("vtbl_ch0#resize"),
            "no grabber when not resizable"
        );
        assert_eq!(
            cell_layout_width(&scene, "vtbl#h0"),
            Some(200),
            "full-width content area"
        );
    }

    #[test]
    fn r784_header_and_body_share_one_horizontal_scroll() {
        // Frozen-header sync: the header (`vtbl_hrow`) and the body rows
        // (`vtbl_row*`) are BOTH inside the single outer horizontal scroll,
        // so one `offset_x` slides them together — the header can never
        // drift out of horizontal alignment with the columns below it.
        let scene = run_vtable(360, 0);
        let h = find_vt_hscroll(&scene).expect("outer horizontal scroll");
        assert!(
            h.content.contains_tag("vtbl_hrow"),
            "header band lives inside the horizontal scroll",
        );
        assert!(
            h.content.contains_tag("vtbl_row0"),
            "body rows live inside the same horizontal scroll",
        );
    }

    #[test]
    fn r775_virtual_table_body_scroll_is_flex_grow() {
        let scene = run_vtable(360, 0);
        let scroll = find_vt_scroll(&scene).expect("body is a Scene::Scroll");
        assert!(
            (scroll.layout.flex_grow - 1.0).abs() < f32::EPSILON,
            "body ScrollNode flex-grows to fill below the header",
        );
        assert!(scroll.state.is_some(), "scroll carries the state Rc");
    }

    #[test]
    fn r775_virtual_table_window_slides_with_offset() {
        let top = run_vtable(360, 0);
        let pitch = i32::try_from(TableStyle::m3().row_height).unwrap();
        let deep = run_vtable(360, 100 * pitch);
        // The deep window must not include row 0 (scrolled out), and the
        // top window must include it.
        let has_row0 = |scene: &Scene| {
            fn walk(s: &Scene) -> bool {
                match s {
                    Scene::Container(c) => {
                        c.tag.as_deref() == Some("vtbl_row0") || c.children.iter().any(walk)
                    }
                    Scene::Scroll(s) => walk(s.content.as_ref()),
                    _ => false,
                }
            }
            walk(scene)
        };
        assert!(has_row0(&top), "top window includes row 0");
        assert!(!has_row0(&deep), "deep window scrolled row 0 out");
    }

    // ── R778 data-grid sort at scale ────────────────────────────────────

    /// Render a small sorted virtual table: 4 rows, a `(col, asc)` sort
    /// glyph, a `sort_tag` header anchor, and an explicit `order`
    /// permutation. The measured height covers all 4 rows so the whole view
    /// renders (the permutation is what we assert on, not the windowing).
    fn run_vtable_sorted(sort: Option<(usize, bool)>, order: &[usize]) -> Scene {
        let state = Rc::new(ScrollState::new());
        let pitch = TableStyle::m3().row_height;
        state.set_measured_viewport(360, pitch * 8);
        state.set_max(
            0,
            i32::try_from(order.len()).unwrap() * i32::try_from(pitch).unwrap(),
        );
        let h_state = vt_hscroll(0);
        let theme = light();
        let style = TableStyle::m3();
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll {
                    body: &state,
                    horizontal: &h_state,
                },
                VirtualTableData {
                    column_count: VT_HEADERS.len(),
                    item_count: order.len(),
                    overscan: 2,
                    sort,
                    sort_tag: Some("vsort"),
                    order: Some(order),
                    col_widths: None,
                    resizable: false,
                    frozen_cols: 0,
                    row_style: None,
                    delegate: None,
                },
                &theme,
                &style,
                |_| false,
                GridModel {
                    cell: vt_cell,
                    header: vt_header,
                    decoration: no_decoration,
                },
            )
        })
    }

    #[test]
    fn r778_sorted_grid_strips_tag_by_source_in_visual_order() {
        // order = visual→source [2, 0, 3, 1]: the strip at visual position 0
        // is data row 2, etc. Cells / strips tag by **source** id.
        let order = [2usize, 0, 3, 1];
        let scene = run_vtable_sorted(Some((0, true)), &order);
        // Every source id present as a strip; visual order is the slot `top`.
        for &source in &order {
            assert!(
                scene.contains_tag(&format!("vtbl_row{source}")),
                "source row {source} strip present",
            );
            for col in 0..3 {
                assert!(
                    scene.contains_tag(&format!("vtbl#{source}_{col}")),
                    "cell {source}_{col} tagged by source id",
                );
            }
        }
    }

    #[test]
    fn r778_sorted_grid_header_routes_to_sort_tag_and_shows_glyph() {
        let scene = run_vtable_sorted(Some((1, false)), &[0, 1, 2, 3]);
        // Clickable headers route to the sort anchor, not the paint root.
        for col in 0..3 {
            assert!(
                scene.contains_tag(&format!("vsort#h{col}")),
                "header {col} clickable tag routes to the sort anchor",
            );
            assert!(
                !scene.contains_tag(&format!("vtbl#h{col}")),
                "header {col} does NOT route to the grid (select) anchor",
            );
        }
        // Descending glyph on the active column only.
        let mut text = Vec::new();
        collect_text(&scene, &mut text);
        assert_eq!(
            text.iter()
                .filter(|s| *s == crate::glyph::SORT_DESCENDING)
                .count(),
            1,
            "one descending glyph"
        );
        assert!(
            !text.iter().any(|s| s == crate::glyph::SORT_ASCENDING),
            "no ascending glyph for a descending sort"
        );
    }

    #[test]
    fn r778_display_grid_keeps_headers_on_the_grid_anchor() {
        // sort_tag = None (the R777 default) keeps headers on the paint root,
        // where the selection coordinator harmlessly ignores `h<col>`.
        let scene = run_vtable(360, 0);
        assert!(
            scene.contains_tag("vtbl#h0"),
            "display grid header stays on the grid anchor"
        );
    }

    // ── R1523 column-axis windowing ─────────────────────────────────

    /// 200 columns on a five-width cycle — 600px per five columns, 24,000px of
    /// content. Deliberately unequal: a window derived from a uniform pitch
    /// lands on the wrong column, so these fixtures discriminate against a
    /// re-derivation that divided by an average width.
    const WIDE_NCOLS: usize = 200;

    fn wide_widths() -> Vec<u32> {
        (0..WIDE_NCOLS)
            .map(|c| [150u32, 90, 120, 105, 135][c % 5])
            .collect()
    }

    fn wide_header(col: usize) -> String {
        format!("C{col:03}")
    }

    /// Render the wide grid with a **narrow** horizontal viewport, so the
    /// column axis genuinely windows. `frozen_cols` exercises the split path.
    fn run_vtable_wide(viewport_w: u32, offset_x: i32, frozen_cols: usize) -> Scene {
        run_vtable_wide_with(
            viewport_w,
            offset_x,
            frozen_cols,
            GridModel {
                cell: |c: CellIndex| format!("r{}c{}", c.row, c.col),
                header: wide_header,
                decoration: no_decoration,
            },
        )
    }

    /// [`run_vtable_wide`] with the caller's own [`GridModel`], so an R1524 /
    /// R1530 test can observe **which** cells and **which** header sections the
    /// grid asked for.
    fn run_vtable_wide_with(
        viewport_w: u32,
        offset_x: i32,
        frozen_cols: usize,
        model: GridModel<
            impl FnMut(CellIndex) -> String,
            impl FnMut(usize) -> String,
            impl FnMut(CellIndex) -> Option<CellDecoration>,
        >,
    ) -> Scene {
        let widths = wide_widths();
        let body = Rc::new(ScrollState::new());
        body.set_measured_viewport(viewport_w, 360);
        let h = Rc::new(ScrollState::with_tag("vtbl_h"));
        h.set_measured_viewport(viewport_w, 0);
        let total: u32 = widths.iter().copied().sum();
        h.set_max(i32::try_from(total.saturating_sub(viewport_w)).unwrap(), 0);
        h.scroll_to(offset_x, 0);
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll {
                    body: &body,
                    horizontal: &h,
                },
                VirtualTableData {
                    column_count: WIDE_NCOLS,
                    item_count: VT_N,
                    overscan: 0,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: Some(&widths),
                    resizable: false,
                    frozen_cols,
                    row_style: None,
                    delegate: None,
                },
                &light(),
                &TableStyle::m3(),
                |_| false,
                model,
            )
        })
    }

    /// The columns present in the header band, as absolute indices.
    fn header_cols(scene: &Scene) -> Vec<usize> {
        let mut cols: Vec<usize> = tags_with_prefix(scene, "vtbl_ch")
            .iter()
            .filter_map(|t| t.strip_prefix("vtbl_ch")?.parse().ok())
            .collect();
        cols.sort_unstable();
        cols
    }

    /// The columns present in data row `row`, as absolute indices.
    fn row_cols(scene: &Scene, row: usize) -> Vec<usize> {
        let prefix = format!("vtbl#{row}_");
        let mut cols: Vec<usize> = tags_with_prefix(scene, &prefix)
            .iter()
            .filter_map(|t| t.strip_prefix(&prefix)?.parse().ok())
            .collect();
        cols.sort_unstable();
        cols
    }

    /// The defect R1523 closes: at 200 columns and a 560px viewport the grid
    /// built **every** column for every windowed row, and merely positioned the
    /// off-screen ones outside the R784 viewport.
    #[test]
    fn r1523_column_axis_windows_against_the_measured_viewport() {
        let scene = run_vtable_wide(560, 0, 0);
        let cols = row_cols(&scene, 0);
        assert!(!cols.is_empty(), "the visible columns are built");
        assert!(
            cols.len() < WIDE_NCOLS / 10,
            "a 560px viewport over 24,000px of columns must window: got {} of {WIDE_NCOLS} \
             cells in row 0",
            cols.len(),
        );
        // Exactly the columns intersecting [0, 560): 0..=4 spans [0, 600).
        assert_eq!(cols, vec![0, 1, 2, 3, 4]);
    }

    /// The before/after of this round, measured rather than asserted from
    /// memory: the **same** grid rendered with a viewport wide enough to expose
    /// every column (which is what the pre-R1523 assembly produced at any
    /// viewport) and with the real 560px one.
    #[test]
    fn r1523_windowing_is_a_40x_reduction_in_cells() {
        let before = row_cols(&run_vtable_wide(24_000, 0, 0), 0);
        let after = row_cols(&run_vtable_wide(560, 0, 0), 0);
        assert_eq!(
            before.len(),
            WIDE_NCOLS,
            "un-windowed, one cell per column — the pre-R1523 cost",
        );
        assert_eq!(after.len(), 5, "windowed, one cell per visible column");
        assert!(
            before.len() / after.len() >= 40,
            "{}x fewer cells per row",
            before.len() / after.len(),
        );
    }

    // ── R1524 per-cell data contract ────────────────────────────────

    /// Every cell the grid asked for while rendering, in request order.
    fn asked_cells(viewport_w: u32, offset_x: i32, frozen_cols: usize) -> (Scene, Vec<CellIndex>) {
        let log = RefCell::new(Vec::new());
        let scene = run_vtable_wide_with(
            viewport_w,
            offset_x,
            frozen_cols,
            GridModel {
                cell: |c: CellIndex| {
                    log.borrow_mut().push(c);
                    format!("r{}c{}", c.row, c.col)
                },
                header: wide_header,
                decoration: no_decoration,
            },
        );
        (scene, log.into_inner())
    }

    /// R1530 — every header section the grid asked for while rendering, in
    /// request order.
    fn asked_sections(viewport_w: u32, offset_x: i32, frozen_cols: usize) -> (Scene, Vec<usize>) {
        let log = RefCell::new(Vec::new());
        let scene = run_vtable_wide_with(
            viewport_w,
            offset_x,
            frozen_cols,
            GridModel {
                cell: |c: CellIndex| format!("r{}c{}", c.row, c.col),
                header: |col: usize| {
                    log.borrow_mut().push(col);
                    wide_header(col)
                },
                decoration: no_decoration,
            },
        );
        (scene, log.into_inner())
    }

    /// Every data cell in the painted tree, as `(row, col)`.
    ///
    /// Decided by the `<row>_<col>` coordinate shape, not by the `vtbl#` prefix
    /// alone: the header band's cells live in the same composite space
    /// (`vtbl#h{col}`) and correspond to no data request.
    fn painted_cells(scene: &Scene) -> Vec<(usize, usize)> {
        tags_with_prefix(scene, "vtbl#")
            .iter()
            .filter_map(|t| {
                let (row, col) = t.strip_prefix("vtbl#")?.split_once('_')?;
                Some((row.parse().ok()?, col.parse().ok()?))
            })
            .collect()
    }

    /// **The defect R1524 closes.** R1523 windowed which cells reach the scene
    /// tree, but the grid still *asked* its consumer for every column of every
    /// windowed row and threw all but the window away — so the consumer's
    /// per-cell work stayed proportional to the table, not the window.
    ///
    /// Asserted as set **equality** in both directions rather than as a
    /// threshold: "asks for the cells it paints" is exactly that, and equality
    /// also rejects asking for too *few*, which an upper bound would pass.
    #[test]
    fn r1524_unsplit_asks_for_exactly_the_cells_it_paints() {
        let (scene, asked) = asked_cells(560, 0, 0);
        let painted = painted_cells(&scene);
        assert!(!painted.is_empty(), "the window holds cells");
        assert_eq!(
            asked.len(),
            painted.len(),
            "one request per painted cell: asked {}, painted {}",
            asked.len(),
            painted.len(),
        );
        let asked_set: BTreeSet<(usize, usize)> = asked.iter().map(|c| (c.row, c.col)).collect();
        assert_eq!(
            asked_set.len(),
            asked.len(),
            "no cell is asked for twice (the frozen split's old double build)",
        );
        assert_eq!(
            asked_set,
            painted.into_iter().collect::<BTreeSet<_>>(),
            "the asked-for set and the painted set are the same set",
        );
    }

    /// The frozen split is where the pre-R1524 contract cost the most: the
    /// per-row builder ran **once per pane**, so each pane rebuilt the whole
    /// row and every column was produced twice — 2 x 200 requests for the ~7
    /// columns the panes between them paint.
    ///
    /// Now each pane asks only for its own columns, so the split costs exactly
    /// what the unsplit grid does: one request per painted cell, no column
    /// twice.
    #[test]
    fn r1524_frozen_split_asks_each_painted_cell_once() {
        let (scene, asked) = asked_cells(560, 700, 2);
        let painted = painted_cells(&scene);
        assert!(!painted.is_empty(), "the split paints cells");
        let asked_set: BTreeSet<(usize, usize)> = asked.iter().map(|c| (c.row, c.col)).collect();
        assert_eq!(
            asked_set.len(),
            asked.len(),
            "a split grid asks for each cell once, not once per pane",
        );
        assert_eq!(
            asked_set,
            painted.into_iter().collect::<BTreeSet<_>>(),
            "both panes together ask for exactly the cells they paint",
        );
        // The pinned columns are asked for, and in their own right: they are on
        // screen at every offset, so they can never be windowed out.
        for col in 0..2 {
            assert!(
                asked.iter().any(|c| c.col == col),
                "frozen column {col} is asked for",
            );
        }
    }

    /// The magnitude, measured on the same fixture at two viewports rather than
    /// recalled: the wide viewport is what the pre-R1523 assembly produced at
    /// *any* viewport, and the per-row contract asked for that many whatever
    /// the window.
    #[test]
    fn r1524_requests_drop_by_the_window_ratio() {
        let (_, wide) = asked_cells(24_000, 0, 0);
        let (_, narrow) = asked_cells(560, 0, 0);
        let rows: BTreeSet<usize> = narrow.iter().map(|c| c.row).collect();
        assert!(!rows.is_empty(), "some rows are windowed");
        // Per windowed row: 200 columns un-windowed vs the 5 visible ones.
        assert_eq!(wide.len() / rows.len(), WIDE_NCOLS);
        assert_eq!(narrow.len() / rows.len(), 5);
        assert!(
            wide.len() / narrow.len() >= 40,
            "{}x fewer cell requests per frame",
            wide.len() / narrow.len(),
        );
    }

    // ── R1530 per-section header contract ───────────────────────────

    /// The text under the subtree tagged `tag`, or `None` if no such subtree.
    fn subtree_text(scene: &Scene, tag: &str) -> Option<Vec<String>> {
        if scene.tag() == Some(tag) {
            let mut out = Vec::new();
            collect_text(scene, &mut out);
            return Some(out);
        }
        match scene {
            Scene::Container(c) => c.children.iter().find_map(|ch| subtree_text(ch, tag)),
            Scene::Scroll(s) => subtree_text(s.content.as_ref(), tag),
            _ => None,
        }
    }

    /// **The defect R1530 closes.** R1523 windowed which header cells reach the
    /// scene tree, but the labels arrived as a slice of every column and the
    /// substrate sliced the window out of it — because [`VirtualTableData`]
    /// read its column count off that slice's length, so a grid could not learn
    /// its own extent without being handed all 200 names.
    ///
    /// Set **equality** in both directions, like its R1524 cell peer: asking
    /// for too few sections would pass an upper bound and paint blank headers.
    #[test]
    fn r1530_unsplit_asks_for_exactly_the_sections_it_paints() {
        let (scene, asked) = asked_sections(560, 0, 0);
        let painted = header_cols(&scene);
        assert!(!painted.is_empty(), "the window holds header cells");
        assert_eq!(
            asked.len(),
            painted.len(),
            "one request per painted header: asked {}, painted {}",
            asked.len(),
            painted.len(),
        );
        let asked_set: BTreeSet<usize> = asked.iter().copied().collect();
        assert_eq!(
            asked_set.len(),
            asked.len(),
            "no section is asked for twice"
        );
        assert_eq!(
            asked_set,
            painted.into_iter().collect::<BTreeSet<_>>(),
            "the asked-for sections and the painted ones are the same set",
        );
    }

    /// The section index a pane asks about is **absolute**, so a consumer
    /// answers with no knowledge of the window or the frozen split — the
    /// property [`CellIndex::col`] states for cells.
    ///
    /// Counts alone cannot see this: a pane asking with its own pane-relative
    /// index would ask for exactly as many sections as it paints and label
    /// every scrolled column wrong. The painted label is therefore held against
    /// the section it names.
    #[test]
    fn r1530_sections_are_absolute_across_the_window_and_the_split() {
        for (offset, frozen) in [(0i32, 0usize), (6_000, 0), (700, 2), (6_000, 2)] {
            let scene = run_vtable_wide(560, offset, frozen);
            let cols = header_cols(&scene);
            assert!(!cols.is_empty(), "offset {offset} paints headers");
            for col in cols {
                let label = subtree_text(&scene, &GridTag::col_header("vtbl", col))
                    .unwrap_or_else(|| panic!("header {col} is in the tree"));
                assert_eq!(
                    label.first().map(String::as_str),
                    Some(wide_header(col).as_str()),
                    "header {col} (offset {offset}, frozen {frozen}) carries its own label",
                );
            }
        }
    }

    /// The frozen split is where the per-slice contract cost the most: **both**
    /// panes indexed the same whole-table slice, so the consumer had to hold
    /// every label for a split that paints ~7. Each pane now asks for its own
    /// sections — the pinned ones and the window — and no section twice.
    #[test]
    fn r1530_frozen_split_asks_each_painted_section_once() {
        let (scene, asked) = asked_sections(560, 700, 2);
        let painted = header_cols(&scene);
        assert!(!painted.is_empty(), "the split paints headers");
        let asked_set: BTreeSet<usize> = asked.iter().copied().collect();
        assert_eq!(
            asked_set.len(),
            asked.len(),
            "a split grid asks for each section once, not once per pane",
        );
        assert_eq!(
            asked_set,
            painted.into_iter().collect::<BTreeSet<_>>(),
            "both panes together ask for exactly the sections they paint",
        );
        for col in 0..2 {
            assert!(
                asked.contains(&col),
                "frozen section {col} is asked for (it is never windowed out)",
            );
        }
    }

    /// The magnitude, measured on the same fixture at two viewports rather than
    /// recalled. The wide viewport is what the pre-R1530 binding built at
    /// *every* viewport: one label per column in the table, whatever the window.
    #[test]
    fn r1530_section_requests_drop_by_the_window_ratio() {
        let (_, wide) = asked_sections(24_000, 0, 0);
        let (_, narrow) = asked_sections(560, 0, 0);
        assert_eq!(wide.len(), WIDE_NCOLS, "un-windowed: one label per column");
        assert_eq!(narrow.len(), 5, "windowed: one label per visible column");
        assert!(
            wide.len() / narrow.len() >= 40,
            "{}x fewer header requests per frame",
            wide.len() / narrow.len(),
        );
    }

    /// The count is a number the grid is *told*, not one it derives from the
    /// labels: `aria-colcount` and the horizontal scroll extent are both drawn
    /// from `column_count`, and no label is asked for to establish it. A grid
    /// whose sections all answer with an empty label still spans all 200.
    #[test]
    fn r1530_extent_is_independent_of_the_labels() {
        let scene = run_vtable_wide_with(
            560,
            0,
            0,
            GridModel {
                cell: |c: CellIndex| format!("r{}c{}", c.row, c.col),
                header: |_: usize| String::new(),
                decoration: no_decoration,
            },
        );
        assert_eq!(
            header_cols(&scene),
            row_cols(&scene, 0),
            "the column window is unchanged by the labels being blank",
        );
        let last = *header_cols(&scene).last().expect("a window exists");
        assert!(
            last < WIDE_NCOLS - 1,
            "and the window is a window: it ends at {last}, not at the table's edge",
        );
    }

    /// Header and body window in lockstep. They share one horizontal scroll, so
    /// a disagreement would paint labels over the wrong columns — the failure
    /// R784's frozen-header sync exists to prevent, now on the windowed axis.
    #[test]
    fn r1523_header_windows_in_lockstep_with_the_body() {
        for offset in [0i32, 700, 6_000, 23_000] {
            let scene = run_vtable_wide(560, offset, 0);
            assert_eq!(
                header_cols(&scene),
                row_cols(&scene, 0),
                "header and body must window identically at offset {offset}",
            );
        }
    }

    /// Scrolling moves the window: far columns enter the tree and near ones
    /// leave it entirely — not merely off-screen.
    #[test]
    fn r1523_scrolling_moves_the_column_window() {
        let near = row_cols(&run_vtable_wide(560, 0, 0), 0);
        let far = row_cols(&run_vtable_wide(560, 6_000, 0), 0);
        assert!(
            far.iter().min() > near.iter().max(),
            "a 6000px scroll replaces the whole column set: {near:?} -> {far:?}",
        );
        // Column 50 starts at exactly 6000 (ten 600px cycles).
        assert_eq!(far.first(), Some(&50));
    }

    /// The pad accounts for exactly the columns the window left out, so the row
    /// still occupies the full content width and the horizontal scroll keeps
    /// bounding against all 200 columns.
    #[test]
    fn r1523_pad_accounts_for_every_windowed_out_column() {
        let widths = wide_widths();
        let total: u32 = widths.iter().copied().sum();
        for offset in [0i32, 700, 6_000, 23_000] {
            let window = visible_columns(&widths, offset, 560, 0);
            let pad = ColumnPad::around(&widths, &window);
            let inside: u32 = window.indices().map(|c| widths[c]).sum();
            assert_eq!(
                pad.lead + inside + pad.trail,
                total,
                "lead + windowed + trail must be the whole content width at offset {offset}",
            );
        }
    }

    /// A grid whose columns fit its viewport windows all of them and pads by
    /// zero — so it emits no spacer node at all and paints the pre-R1523 scene.
    /// The regression guarantee for the 3-column grids in the tree.
    #[test]
    fn r1523_grid_that_fits_emits_no_pad() {
        let narrow = ColumnPad::around(
            &[100, 100, 100],
            &visible_columns(&[100, 100, 100], 0, 500, 0),
        );
        assert_eq!(narrow.lead, 0);
        assert_eq!(narrow.trail, 0);
        assert!(
            pad_node(0, 40).is_none(),
            "a zero-width pad contributes no node, so the tree is unchanged",
        );
    }

    /// A frozen column is pinned against the horizontal scroll, so it is on
    /// screen at every offset — windowing it could only remove something
    /// visible. The frozen pane stays whole while the scrolling pane windows.
    #[test]
    fn r1523_frozen_pane_is_never_windowed() {
        for offset in [0i32, 6_000, 23_000] {
            let scene = run_vtable_wide(560, offset, 2);
            assert!(
                scene.contains_tag("vtbl_fhrow"),
                "frozen header pane present at offset {offset}",
            );
            for col in 0..2usize {
                assert!(
                    scene.contains_tag(&format!("vtbl_ch{col}")),
                    "pinned column {col} present at offset {offset}",
                );
            }
            // And the scrolling pane still windowed — the two panes did not
            // collapse into one un-windowed row.
            let cols = row_cols(&scene, 0);
            assert!(
                cols.len() < WIDE_NCOLS / 10,
                "scrolling pane still windows at offset {offset}: {} cells",
                cols.len(),
            );
        }
    }
}
