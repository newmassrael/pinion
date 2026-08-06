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
use pinion_core::cell_value::{CellEdit, CellValue, EditorForm};
use pinion_core::composite_tag::{GridSendKey, GridTag};
use pinion_core::scene::{
    ContainerNode, ImageNode, Rect, ScrollAxis, ScrollNode, TextNode, TextRole,
};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, Fit, FlexDirection, ImageStyle, JustifyContent,
    LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::cell_selection::GridSelection;
use pinion_core::widgets::column_widths::{columns_width_before, visible_columns};
use pinion_core::widgets::grid_edit::OpenCell;
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::text_field::TextFieldState;
use pinion_core::widgets::virtual_list::{VisibleWindow, compute_visible_range, content_height};
use pinion_core::widgets::virtual_select::SelectionExtent;

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
    /// [`Decoration::Swatch`] paints (default 10). Qt's default delegate
    /// sizes a decoration from the view's `iconSize`; this is the grid-wide
    /// equivalent, held here with the other cell dimensions so a decorated
    /// column cannot pick a size the rest of the grid does not know about.
    pub decoration_px: u32,
    /// R1535 §5.27 — gap, in logical pixels, between a cell's decoration and
    /// its display text (default 8). Only spent when the cell **has** a
    /// decoration, so an undecorated cell's label sits exactly where it did
    /// before R1535.
    pub decoration_gap_px: u32,
    /// R1548 §5.27 — width of the **vertical header band** in logical pixels
    /// (default 56), the row axis's peer of [`Self::col_width`].
    ///
    /// A dimension, held here with every other pixel, because the *presence* of
    /// the band is stated on the model ([`GridModel::rows`]) and a second
    /// statement of it beside this number could only ever contradict the first.
    /// Unread by a grid whose model answers no vertical axis.
    ///
    /// Stated rather than derived from the widest painted label. Qt derives it
    /// (`QHeaderView::ResizeToContents`) and pays with a header whose width
    /// changes as you scroll into longer labels; deriving it here would
    /// additionally need the measured text extent, which is known one frame
    /// late. 56px is Qt's own ballpark for a numeric vertical header.
    pub row_header_width: u32,
}

/// R786 §5.27 — visible divider line width inside the resize grabber, in
/// logical pixels (an M3 hairline). The grabber is a [`TableStyle::resize_handle_w`]
/// transparent hit zone with this thin [`ColorRole::Outline`] line hugging its
/// right edge — the painted column boundary the user grabs.
const RESIZE_DIVIDER_W: u32 = 1;

/// R1548 §5.27 — gap between the parts of one header section (its
/// `Qt::DecorationRole` mark, its `Qt::DisplayRole` label, and — on the column
/// axis — its sort glyph), in logical pixels.
///
/// One constant for **both** section axes: a row header lettered or spaced
/// differently from a column header would be a second style contract for one
/// band pair, which is the divergence [`HeaderAxis`] exists to prevent one level
/// up. Distinct from [`TableStyle::decoration_gap_px`], which is the *cell*
/// axis's — a cell's mark sits in body type beside body text.
const SECTION_GAP_PX: u32 = 4;

/// R1535 §5.27 — corner radius of a [`Decoration::Swatch`], in logical
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
            row_header_width: 56,
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
    pub decoration: Option<&'a dyn Fn(CellIndex) -> Option<Decoration>>,
    /// R1547 §5.27 — the **column** section axis's `Qt::DecorationRole`
    /// accessor, the eager surface's peer of [`HeaderAxis::decoration`]:
    /// `header_decoration(section)` returns the mark ahead of that column's
    /// label, or `None`.
    ///
    /// Present here for the reason R1536 gave [`Self::decoration`] its place: a
    /// role one of the two grid surfaces answers and the other does not is two
    /// contracts in one tree, and the consumer that hits the silent one reads
    /// the role as absent from the framework. `None` (the field's `Default`) is
    /// every pre-R1547 caller, painting byte-identically.
    ///
    /// Not folded into an [`EagerRowHeader`] beside [`Self::row_headers`]:
    /// this surface states its column labels as [`Self::headers`], a slice, so
    /// its column axis is already only half an accessor pair. Unifying the two
    /// is the open Model/View item R1530 named, and doing it here would be a
    /// second axis's work paid for on the first axis's round.
    pub header_decoration: Option<&'a dyn Fn(usize) -> Option<Decoration>>,
    /// R1548 §5.27 — the **vertical** section axis, the eager surface's peer of
    /// [`GridModel::rows`]: `Some` for a table that paints row headers (Qt
    /// `headerData(section, Qt::Vertical, …)`), `None` for one that does not.
    ///
    /// Present here by the rule R1547.1 paid for: a role one grid surface
    /// answers and the other does not is two contracts in one tree, and the
    /// consumer that lands on the silent one reads the whole axis as absent
    /// from the framework. `None` is every pre-R1548 caller, painting a
    /// byte-identical scene.
    ///
    /// The section index is the **data** row (`row_ids[v]`), not the visual
    /// position, exactly as the virtualized grid asks with its source row.
    pub row_headers: Option<EagerRowHeader<'a>>,
}

/// R1548 / R1562 §5.27 — a [`RowHeaderAxis`] as the **eager** surface can hold
/// one.
///
/// [`TableData`] is a `Copy` bundle of borrows, so it cannot carry the generic
/// closures [`GridModel`] does; the accessors arrive as `&dyn Fn` instead. Same
/// type, same role set, same corner, same painter — which is why one alias is
/// enough here rather than a second band type that would have to be kept in
/// agreement.
pub type EagerRowHeader<'a> =
    RowHeaderAxis<&'a dyn Fn(usize) -> String, &'a dyn Fn(usize) -> Option<Decoration>>;

impl core::fmt::Debug for TableData<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TableData")
            .field("headers", &self.headers)
            .field("rows", &self.rows)
            .field("row_ids", &self.row_ids)
            .field("decoration", &self.decoration.map(|_| "<fn>"))
            .field("header_decoration", &self.header_decoration.map(|_| "<fn>"))
            .field("row_headers", &self.row_headers.map(|_| "<axis>"))
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

/// R1536 §5.27 — the [`Decoration::Icon`] node: the source drawn at the
/// same square a [`Decoration::Swatch`] occupies.
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
/// [`Decoration`] arm produced it.
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

/// R1547 §5.27 — the painted node for a `Qt::DecorationRole` answer, whichever
/// arm it is: a [`Decoration::Swatch`]'s filled square or a
/// [`Decoration::Icon`]'s image, both at `side` and both addressed by `tag`.
///
/// The **only** place the arms are matched at a paint site. R1535 had one such
/// match, in `text_cell_painter`; R1547's header band would have been a second,
/// and `Decoration::meaning` already records why that is the wrong shape — a
/// `match` at a read site has to grow an arm every time the variant list does,
/// which is exactly how one arm gets forgotten. Obligation 3b: two mechanical
/// copies with no per-site opinion in either, so it lifts here rather than
/// waiting for a third to drift.
fn decoration_node(decoration: &Decoration, side: u32, tag: &str) -> Scene {
    match decoration {
        Decoration::Swatch { color, .. } => swatch_node(*color, side, tag),
        Decoration::Icon { source, .. } => icon_node(source, side, tag),
    }
}

/// R1535 §5.27 — the [`Decoration::Swatch`] node: a filled square of
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
    /// R1563 — how much of this column is selected, from the
    /// [`GridSelection`] question. [`SelectionExtent::Empty`] for every
    /// row-select grid, whose horizontal band is the sort control.
    selected: SelectionExtent,
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
    section: &SectionRoles,
    sort: Option<(usize, bool)>,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let ColCell {
        col,
        width,
        resizable,
        selected,
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
    // R1547 — the section's `Qt::DecorationRole` mark, ahead of its label and
    // INSIDE the clickable inner container: a mark on a sortable column must
    // not carve a dead zone out of the sort target. `decoration_layout` keeps it
    // pointer-transparent, so the press lands on the header either way.
    let (mut inner_children, aria_name) = section_content(
        section,
        &GridTag::header_decoration(tag, col),
        section_label_style(style, fg),
        style,
    );
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
                section_label_style(style, fg),
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
                    .with_gap(SECTION_GAP_PX)
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
    let mut header = ContainerNode::new(cell_children)
        .with_tag(GridTag::col_header(tag, col))
        .with_layout(LayoutStyle::new().flex(FlexDirection::Row));
    // R1563 — the section shows the selection through it, by the same
    // `section_fill` derivation the vertical band uses. Set only when the
    // column is involved: an unselected section keeps the band's own
    // `SurfaceContainerHigh`, and stating that colour a second time here would
    // be two declarations of one fill that a later theme change could split.
    if selected != SelectionExtent::Empty {
        header = header.with_style(BoxStyle::filled(section_fill(theme, selected, col)));
    }
    // R1547 §5.40 — a mark that carries meaning joins the `columnheader`'s
    // accessible name, ahead of the label, by exactly the rule R1536 gave the
    // cell. Set only when there is something to add, so a decorative (`alt=""`)
    // mark leaves the AT tree byte-identical.
    if let Some(name) = aria_name {
        header = header.with_aria_label(name);
    }
    Scene::Container(header)
}

/// R1548 §5.27 — the text style a section's label paints in, on either axis.
///
/// One function because the header type scale is a property of the *grid*, not
/// of an axis: a row header lettered differently from a column header would be
/// a second style contract for one band pair, which is what
/// [`HeaderAxis`] exists to prevent one level up.
fn section_label_style(style: &TableStyle, fg: Color) -> TextStyle {
    TextStyle::new()
        .with_size_px(style.header_size_px)
        .with_fg(fg)
}

/// R1548 §5.27 §5.40 — one section's painted content — its
/// `Qt::DecorationRole` mark, then its `Qt::DisplayRole` label — plus the
/// accessible name that content implies, on **either** axis.
///
/// The single painter [`HeaderAxis`]'s doc promises. Before R1548 this was
/// inline in `header_cell`; the row band would have been a second copy, and the
/// two would then have had to be kept agreeing about three separate things —
/// that the mark precedes the label, that a decoration is `decoration_px`
/// square and pointer-transparent, and that a mark carrying *meaning* joins the
/// section's accessible name while a decorative one does not. Obligation 3b:
/// mechanical, no per-axis opinion in it, so it lifts at the second consumer
/// rather than after a third has drifted.
///
/// The label is the section's **content**, not decoration, by the rule R1536
/// gave the data cell — which is why it is a plain [`TextNode`] and the sort
/// glyph `header_cell` appends after it is
/// [`Presentational`](TextRole::Presentational).
///
/// Returns `None` for the name when the section has no mark, or has one whose
/// [`meaning`](Decoration::meaning) is empty (a decorative `alt=""` mark): the
/// name is then left for §5.40 to derive from the painted label, and the AT
/// tree is byte-identical to an undecorated section's.
fn section_content(
    section: &SectionRoles,
    decoration_tag: &str,
    label: TextStyle,
    style: &TableStyle,
) -> (Vec<Scene>, Option<String>) {
    let mut children = Vec::with_capacity(3);
    let mut name = None;
    if let Some(decoration) = section.decoration.as_ref() {
        children.push(decoration_node(
            decoration,
            style.decoration_px,
            decoration_tag,
        ));
        let meaning = decoration.meaning();
        if !meaning.is_empty() {
            name = Some(compose_cell_name(meaning, section.label.as_str()));
        }
    }
    children.push(Scene::Text(TextNode::styled(
        section.label.clone(),
        Rect::default(),
        label,
    )));
    (children, name)
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
    decoration: &mut impl FnMut(CellIndex) -> Option<Decoration>,
    row: usize,
    span: Range<usize>,
) -> (Vec<String>, Vec<Option<Decoration>>) {
    span.map(|col| {
        let index = CellIndex { row, col };
        (cell(index), decoration(index))
    })
    .unzip()
}

/// R1547 §5.27 — one section's role answers: what a [`HeaderAxis`]'s two
/// accessors said about one section, held together.
///
/// A struct rather than the parallel-vector pair [`pane_cells`] returns,
/// because the two shapes fail differently: a header band is built once per
/// pane and indexed by position, so two vectors could be asked over two
/// different spans and paint a section with its neighbour's mark. Binding the
/// answers at the point they are asked makes that unrepresentable rather than
/// merely unlikely.
///
/// R1548 renamed this from `HeaderSection` — it holds *roles*, and the crate
/// already had a public
/// [`HeaderSection`](crate::column_header::HeaderSection) meaning a section's
/// painted appearance. It is also no longer column-specific: since R1548 the
/// row axis answers the same two roles into the same struct.
struct SectionRoles {
    /// `Qt::DisplayRole` — the label.
    label: String,
    /// `Qt::DecorationRole` — the mark ahead of the label, or `None`.
    decoration: Option<Decoration>,
}

impl SectionRoles {
    /// The answers of a section that has neither role — the contract's
    /// "no label" (an empty `String`) and "no mark" (`None`) in one value.
    const BLANK: Self = Self {
        label: String::new(),
        decoration: None,
    };

    /// R1548 — ask **one** section, at one address, for every role the axis
    /// answers.
    ///
    /// The single statement of "one address, every role" — the call shape Qt's
    /// `headerData(section, orientation, role)` has anyway. Both bands go
    /// through it: the column band over a contiguous span
    /// ([`ask_sections`]), the row band over the windowed rows in *sort* order,
    /// which is not a range and so cannot share the span form. What they must
    /// share is this — that a section's two answers come from one index — and
    /// they now do.
    fn ask<L, D>(axis: &mut HeaderAxis<L, D>, section: usize) -> Self
    where
        L: FnMut(usize) -> String,
        D: FnMut(usize) -> Option<Decoration>,
    {
        Self {
            label: (axis.label)(section),
            decoration: (axis.decoration)(section),
        }
    }
}

/// R1530 / R1547 / R1548 — one pane's sections: **both** section roles asked
/// once per section in `span`, in section order. The section-axis peer of
/// [`pane_cells`], and the single place the per-section contract meets the
/// slice a band paints, so no pane on either axis can ask for a section it
/// will not paint.
///
/// R1547 added the decoration role, and it is asked **here**, beside the
/// display role, for the reason `pane_cells` asks both cell roles in one pass:
/// one address, every role — the call shape `data(index, role)` has anyway.
///
/// R1548 — `axis` replaced the two loose accessors, so this function is now
/// axis-agnostic: the column band and the row band call it with `Qt::Horizontal`
/// and `Qt::Vertical` respectively and there is no third shape either could
/// drift into.
fn ask_sections<L, D>(axis: &mut HeaderAxis<L, D>, span: Range<usize>) -> Vec<SectionRoles>
where
    L: FnMut(usize) -> String,
    D: FnMut(usize) -> Option<Decoration>,
{
    span.map(|section| SectionRoles::ask(axis, section))
        .collect()
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
    /// R1563 — the [`GridSelection`] question, asked once per **painted**
    /// section for the column's extent.
    ///
    /// It rides here rather than as an eighth argument because that is what
    /// this bundle is for, and because it belongs with `col_base`: a section's
    /// extent is asked about its **absolute** column, and the two pieces that
    /// make an absolute column are already here.
    selection: &'a dyn GridSelection,
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
    decorations: &'a [Option<Decoration>],
    /// R1563 — which of this row's painted cells are selected **and not
    /// already covered by the row strip's own fill**, aligned with
    /// [`Self::widths`].
    ///
    /// An **empty** slice means "nothing here needs cell-level ink", which is
    /// every row-select grid: their selected rows are washed by the strip, so
    /// asking per cell would add a node to every cell of every grid to paint a
    /// colour that is already there. The caller narrows it because only the
    /// caller knows the row's extent — the same reason [`Self::editing`] is
    /// narrowed there.
    selected_cells: &'a [bool],
    /// R1532 — the palette, for a delegate that resolves its own roles. It
    /// travels with [`Self::painters`] rather than as its own argument
    /// because it exists here *for* them: the built-in painter takes its
    /// colour from the row's already-resolved `fg`.
    theme: &'a Theme,
    /// R1544 — the open editors, **already narrowed to this row and already
    /// resolved**: the members of [`GridEditing::open`] whose row is the one
    /// this pane is painting, each with the painter its column's editor
    /// delegate answered with.
    ///
    /// Narrowed and resolved by the caller rather than re-derived per cell for
    /// the reason `GridRender::painters` resolves the display delegates once
    /// per pane. R1571 made it a slice rather than an `Option`: with N editors
    /// open a row can host several — a property row with two editable columns
    /// is the ordinary case — and an **empty** slice is the same statement the
    /// `None` was, at the same cost.
    editing: &'a [CellEditorSlot<'a>],
}

/// The header band: one clickable `columnheader` cell per column ([
/// `header_cell`]) on a raised [`ColorRole::SurfaceContainerHigh`]
/// surface. `sort` drives the active column's sort glyph; `layout` supplies the
/// per-column widths + whether the columns are resizable (R786).
fn header_row(
    tag: &str,
    click_tag: &str,
    sections: &[SectionRoles],
    sort: Option<(usize, bool)>,
    layout: ColumnLayout<'_>,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let cells: Vec<Scene> = sections
        .iter()
        .enumerate()
        .map(|(i, section)| {
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
                    // Asked once per **painted** section, over the same window
                    // the band lays out — the discipline `ask_sections`
                    // enforces on the role axis.
                    selected: layout.selection.column(col),
                },
                section,
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

/// R1548 §5.27 — one **row** header cell: the vertical section axis's peer of
/// [`header_cell`], tagged `"<tag>_rh<row>"`.
///
/// Differs from the column cell in exactly what the axes differ in, and in
/// nothing else — the shared [`section_content`] draws the mark and the label:
///
/// - no sort glyph. Qt's vertical header can carry a sort indicator only
///   because `QHeaderView` is orientation-generic; a *row* is not a sort key in
///   any item view, so the glyph would be an indicator of nothing.
/// - no resize grabber. Qt resizes rows through the vertical header, but a row
///   here is `TableStyle::row_height` — one pitch for the whole grid, which the
///   windowing arithmetic (`uniform_slots`, `content_height`) is built on. A
///   per-row height is the variable-pitch axis, not this one.
///
/// `row` is the **data** row, not the visual position, so the tag and the mark
/// stay with their row across a sort — the same rule `data_row` follows.
/// R1562 / R1563 §5.27 — a header section's fill, **derived** from how much of
/// the line through it is selected.
///
/// Shared by both bands, so the vertical and the horizontal one cannot show the
/// same fact two ways — and derived from the selection rather than set by a
/// flag, so the band cannot say one thing while the body says another. Qt makes
/// this a view flag (`QHeaderView::highlightSections`) that **defaults to
/// false**: a Qt header is silent about the selection unless someone turns it
/// on, and once on it is a second statement that can be turned back off while
/// the rows stay washed.
///
/// [`SelectionExtent::Partial`] — R1563, unreachable before the column axis
/// existed — takes the highest surface tone rather than the accent: the line is
/// *involved* in the selection without being in it, and painting it with the
/// accent would make a row with one selected cell indistinguishable from a
/// selected record.
fn section_fill(theme: &Theme, extent: SelectionExtent, index: usize) -> Color {
    match extent {
        SelectionExtent::All => row_fill(theme, RadioState::Idle, true, index),
        SelectionExtent::Partial => theme.resolve(ColorRole::SurfaceContainerHighest),
        SelectionExtent::Empty => theme.resolve(ColorRole::SurfaceContainerHigh),
    }
}

/// R1563 §5.27 — which of a row's painted cells need selection ink of their
/// own: the ones that are selected while the row is **not** selected whole.
///
/// A fully selected row is already washed by its strip, so painting each of its
/// cells again would be the same colour twice and a wrapper node per cell. An
/// empty vector — the answer for every row-select grid, where a row is either
/// whole or untouched — costs the `data_row` loop one `get` per cell and adds
/// no nodes at all.
/// R1563 §5.27 — a per-row bool as the band's tri-state.
///
/// The eager [`view_table`] selects whole rows (`row_selected`), so its band
/// can only ever be at one end or the other. Stated by this conversion rather
/// than by [`RowSection`] keeping a bool for one caller's sake: the tri-state
/// is the band's contract, and a surface that cannot say
/// [`SelectionExtent::Partial`] says so here, once.
const fn whole_or_nothing(selected: bool) -> SelectionExtent {
    if selected {
        SelectionExtent::All
    } else {
        SelectionExtent::Empty
    }
}

fn cell_ink(selection: &dyn GridSelection, row: usize, span: std::ops::Range<usize>) -> Vec<bool> {
    if selection.row(row) != SelectionExtent::Partial {
        return Vec::new();
    }
    span.map(|col| selection.cell(row, col)).collect()
}

fn row_header_cell(
    tag: &str,
    row: usize,
    section: &RowSection<'_>,
    width: u32,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let RowSection {
        roles,
        selected,
        click_tag,
    } = *section;
    let fg = theme.resolve(ColorRole::OnSurface);
    let (children, aria_name) = section_content(
        roles,
        &GridTag::row_header_decoration(tag, row),
        section_label_style(style, fg),
        style,
    );
    // R1562 — the pressable region, by [`header_cell`]'s outer/inner split: the
    // outer keeps the presentational `"<tag>_rh<row>"` tag the a11y walker
    // resolves the `rowheader`'s bounds from, the inner carries the composite
    // `"<click_tag>#r<row>"` so the press routes through the R51.42 funnel.
    // Unconditional — a band is pressable wherever a coordinator owns the tag,
    // and where none does the send reaches nothing, which is the same nothing
    // Qt's `sectionsClickable(false)` produces without a second flag to keep in
    // agreement with the first.
    let inner = Scene::Container(
        ContainerNode::new(children)
            .with_tag(format!(
                "{click_tag}#{}",
                GridSendKey::RowHeader { row }.encode()
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_gap(SECTION_GAP_PX)
                    .with_size(Size::px(width, style.row_height))
                    .with_padding(Rect::new(style.cell_pad_x, 0, style.cell_pad_x, 0)),
            ),
    );
    // R1562 — the section's fill is DERIVED from whether its row is selected,
    // through the same `row_fill` the body strip beside it uses, so the band
    // cannot say one thing while the row says another. Qt makes this a view
    // flag, `QHeaderView::highlightSections`, which **defaults to false** — so a
    // Qt row header is silent about the selection unless someone turns it on,
    // and once on it is a second statement that can be turned back off while the
    // rows stay washed.
    let fill = section_fill(theme, selected, row);
    let mut cell = ContainerNode::new(vec![inner])
        .with_tag(GridTag::row_header(tag, row))
        .with_style(BoxStyle::filled(fill))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_size(Size::px(width, style.row_height)),
        );
    // §5.40 — a mark that carries meaning joins the `rowheader`'s accessible
    // name, by the same rule `header_cell` applies on the other axis. Qt's
    // `QAccessibleTableHeaderCell::text(Name)` answers from the
    // `Qt::DisplayRole` alone on both orientations, so a Qt row header whose
    // distinguishing information IS its glyph announces only the row's number.
    if let Some(name) = aria_name {
        cell = cell.with_aria_label(name);
    }
    Scene::Container(cell)
}

/// R1548 §5.27 §5.45 — the **vertical header band**: the corner cell, then one
/// [`row_header_cell`] per windowed row, as a pane pinned to the grid's left
/// edge (Qt `QTableView::verticalHeader()`).
///
/// # Why this is a wrapper and not a fourth pane inside the split
///
/// The band has to be pinned against the *horizontal* scroll and follow the
/// *vertical* one, which is precisely what
/// [`assemble_windowed_flex`]'s follower already is — so composing it as a
/// sibling of the whole existing content, rather than threading it through
/// `render_unsplit` and `render_frozen`, means the unsplit grid, the
/// frozen-column split and the eager surface all gain the axis from one place
/// and none of their assemblies changes. A grid with `row_header_width: None`
/// never calls this and paints a byte-identical scene to the pre-R1548 one.
///
/// `sections` is asked for **exactly** the windowed rows, in view order, by the
/// caller — the [`SectionRoles::ask`] discipline the column band follows — and
/// each entry carries the **data** row its answers came from, so the band and
/// the strip beside it cannot address different rows under a sort.
fn row_header_pane(
    tag: &str,
    scroll: &Rc<ScrollState>,
    window: &VisibleWindow,
    band: RowHeaderBand<'_>,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let RowHeaderBand {
        width,
        total_h,
        sections,
        corner,
        click_tag,
    } = band;
    let blank = SectionRoles::BLANK;
    let slots = uniform_slots(window, width, style.row_height, |view_pos| {
        let i = view_pos.saturating_sub(window.first);
        // Unreachable while the caller asks over the same window it paints; a
        // blank, unselected section at the view position's own index is the
        // contract's answer for "no label", so a mismatch degrades to an empty
        // header rather than panicking a paint pass.
        let (row, roles, selected) = sections.get(i).map_or(
            (view_pos, &blank, SelectionExtent::Empty),
            |(row, s, sel)| (*row, s, *sel),
        );
        row_header_cell(
            tag,
            row,
            &RowSection {
                roles,
                selected,
                click_tag,
            },
            width,
            theme,
            style,
        )
    });
    row_header_column(
        tag,
        BandCorner {
            width,
            corner,
            click_tag,
        },
        assemble_windowed_flex(scroll, width, total_h, slots, true),
        theme,
        style,
    )
}

/// R1562 §5.27 — one row-header section's inputs beyond its index and the
/// palette: what it answers, whether its row is selected, and where its press
/// goes. Bundled to keep [`row_header_cell`] under the argument budget (the
/// [`ColCell`] precedent on the other axis).
#[derive(Clone, Copy)]
struct RowSection<'a> {
    /// The `Qt::DisplayRole` / `Qt::DecorationRole` answers for this section.
    roles: &'a SectionRoles,
    /// R1563 — **how much** of this section's row is selected, from the same
    /// [`GridSelection`] question the body strip beside it is filled from.
    ///
    /// A tri-state rather than R1562's bool, because with a column axis a row
    /// can be partly selected and a bool has to round that to one of its
    /// neighbours. Qt cannot show it at all: `highlightSections` is a bool per
    /// section, so a row with two of two hundred columns selected paints
    /// identically to a fully selected one.
    selected: SelectionExtent,
    /// The anchor the section routes its pointer arc to
    /// (`"<click_tag>#r<row>"`).
    click_tag: &'a str,
}

/// R1548 §5.27 — the vertical band's shape: the corner cell above `body`, the
/// whole column pinned at `width`.
///
/// One function for both surfaces — the virtualized grid's windowed
/// [`ScrollNode`] body and the eager [`view_table`]'s plain column of cells —
/// so the corner cannot end up on one and not the other, and the two bands
/// cannot disagree about the width they occupy. The column-axis peer of
/// [`header_row`] serving both surfaces.
fn row_header_column(
    tag: &str,
    band: BandCorner<'_>,
    body: Scene,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let BandCorner {
        width,
        corner,
        click_tag,
    } = band;
    Scene::Container(
        ContainerNode::new(vec![
            header_corner(tag, click_tag, width, corner, theme, style),
            body,
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_size(Size::width_px(width)),
        ),
    )
}

/// R1562 §5.27 — what [`row_header_column`] needs to build the corner, bundled
/// so both surfaces pass one value and neither can supply the width without the
/// action (the [`ColumnLayout`] / [`RowHeaderBand`] precedent).
#[derive(Clone, Copy)]
struct BandCorner<'a> {
    /// The band's width in logical pixels — [`TableStyle::row_header_width`].
    width: u32,
    /// What a press on the corner does.
    corner: CornerAction,
    /// The anchor the corner routes its pointer arc to (`"<click_tag>#c"`) —
    /// the grid's selection coordinator, which is the paint root for every
    /// current caller.
    click_tag: &'a str,
}

/// R1548 / R1562 §5.27 §5.40 — the cell where the two section axes meet (Qt's
/// `QTableCornerButton`): `header_height` tall so the two bands' first rows
/// line up.
///
/// [`CornerAction::Inert`] paints the pre-R1562 blank block — tagged, because a
/// painted thing with no tag cannot be asked about and the corner's extent is
/// what tells a client where the two bands begin, but carrying nothing to press
/// and no a11y node, since a control that does nothing is noise in an AT tree
/// rather than information.
///
/// [`CornerAction::SelectAll`] paints the tri-state select-all mark inside an
/// inner container tagged `"<click_tag>#c"`, so the press routes through the
/// R51.42 `'#'`-split to the selection coordinator. The outer / inner split is
/// [`header_cell`]'s exactly: the outer tag is the presentational one the a11y
/// walker resolves the `columnheader`'s bounds from, the inner one is the
/// control — which is also the HTML shape a select-all has (`<th>` around an
/// `<input type=checkbox>`), so the AT tree comes out right by construction.
fn header_corner(
    tag: &str,
    click_tag: &str,
    width: u32,
    corner: CornerAction,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let children = match corner {
        CornerAction::Inert => Vec::new(),
        CornerAction::SelectAll(extent) => vec![Scene::Container(
            ContainerNode::new(corner_mark(extent, theme, style))
                .with_tag(format!("{click_tag}#{}", GridSendKey::Corner.encode()))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Center)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(width, style.header_height)),
                ),
        )],
    };
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(GridTag::header_corner(tag))
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHigh),
            ))
            .with_layout(LayoutStyle::new().with_size(Size::px(width, style.header_height))),
    )
}

/// R1562 §5.27 — the corner control's mark for each extent.
///
/// Nothing / `\u{2212}` / `\u{2713}` — the three marks an HTML checkbox draws
/// for unchecked / `indeterminate` / checked, and the two glyphs this framework
/// already paints (the R668 checkbox's check, the window-control minus), so no
/// font acquires a new obligation. An empty box is the *absence* of a mark
/// rather than a third glyph, exactly as [`crate::checkbox`] paints it.
fn corner_mark(extent: SelectionExtent, theme: &Theme, style: &TableStyle) -> Vec<Scene> {
    let glyph = match extent {
        SelectionExtent::Empty => return Vec::new(),
        SelectionExtent::Partial => crate::glyph::SELECT_ALL_PARTIAL,
        SelectionExtent::All => crate::glyph::SELECT_ALL_COMPLETE,
    };
    vec![Scene::Text(
        TextNode::styled(
            glyph.to_string(),
            Rect::default(),
            section_label_style(style, theme.resolve(ColorRole::Accent)),
        )
        .with_role(TextRole::Presentational),
    )]
}

/// R1548 §5.27 — the inputs [`row_header_pane`] needs beyond the window and the
/// palette, bundled to keep it under the argument budget (the [`ColumnLayout`] /
/// [`RowPane`] precedent).
#[derive(Clone, Copy)]
struct RowHeaderBand<'a> {
    /// The band's width in logical pixels — [`TableStyle::row_header_width`].
    width: u32,
    /// Total scrollable height, the same `content_height` the body pane sizes
    /// against, so the two scroll in step.
    total_h: u32,
    /// The windowed rows, in view order: each visual position's **data** row
    /// (the R778 sort permutation) paired with that row's role answers and
    /// whether it is selected (R1562).
    ///
    /// One list rather than a row-resolver beside a list of answers, because
    /// two statements of "which row is at this visual position" can only ever
    /// disagree — the shape R1524 removed from the cell axis.
    sections: &'a [(usize, SectionRoles, SelectionExtent)],
    /// R1562 — what a press on the corner does.
    corner: CornerAction,
    /// R1562 — the anchor every pressable part of the band routes to.
    click_tag: &'a str,
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
            // R1544 — an open editor **replaces** this cell's display subtree,
            // which is what an editor is: Qt's view hides the item and puts
            // the editor widget in its rect. The column's editor delegate is
            // consulted only here, so a grid that is not editing never asks
            // for one (Qt calls `createEditor` at open time, not per paint).
            let painted = if let Some(slot) = pane.editing.iter().find(|e| e.col == col) {
                let edit_render = CellEditRender {
                    cell: &render,
                    edit: &slot.cell.edit,
                    field_tag: slot.field_tag,
                    field: slot.field,
                    pending: slot.cell.editor.pending(),
                    focused: slot.cell.focused,
                    parked: slot.cell.editor.parked_text(),
                };
                slot.paint
                    .map_or_else(|| cell_editor(&edit_render), |paint| paint(&edit_render))
            } else {
                pane.painters
                    .get(j)
                    .copied()
                    .flatten()
                    .map_or_else(|| text_cell_painter(&render), |paint| paint(&render))
            };
            // R1563 — the selection ink for a cell the row strip does not
            // cover. It wraps whatever the column's delegate produced rather
            // than being that painter's job, which is both Qt's order
            // (`QStyledItemDelegate::paint` draws `PE_PanelItemViewItem`
            // first, then the content) and the only shape that works for a
            // delegate this framework did not write.
            if pane.selected_cells.get(j).copied().unwrap_or(false) {
                return Scene::Container(
                    ContainerNode::new(vec![painted])
                        .with_style(BoxStyle::filled(row_fill(
                            pane.theme,
                            RadioState::Idle,
                            true,
                            data_id,
                        )))
                        .with_layout(
                            LayoutStyle::new()
                                .flex(FlexDirection::Row)
                                .with_size(Size::px(render.width, style.row_height)),
                        ),
                );
            }
            painted
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
    // R1547 — the eager surface answers both section roles too. It builds every
    // column (there is no viewport to window against), so the span is the whole
    // header; `header_sections` is still the one place the accessors are asked,
    // so the two surfaces cannot diverge on what a section answers.
    let mut eager_columns = HeaderAxis {
        label: |col: usize| {
            data.headers
                .get(col)
                .copied()
                .unwrap_or_default()
                .to_string()
        },
        decoration: |col: usize| data.header_decoration.and_then(|answer| answer(col)),
    };
    let sections = ask_sections(&mut eager_columns, 0..cols);
    let row_selection = |row: usize| row_selected.get(row).copied().unwrap_or(false);
    let header = header_row(
        tag,
        tag,
        &sections,
        sort,
        ColumnLayout {
            widths: &widths,
            resizable: false,
            // The eager table has no viewport to window against — it builds
            // every row, so it builds every column.
            pad: ColumnPad::NONE,
            col_base: 0,
            container_tag: &hrow_tag,
            selection: &row_selection,
        },
        theme,
        style,
    );
    // R1548 — the vertical axis, asked once per painted row with that row's
    // DATA index, through the same `SectionRoles::ask` the column band used.
    // The eager surface paints every row, so its "window" is every row.
    let mut row_sections: Vec<Scene> = Vec::with_capacity(data.rows.len());
    let mut children: Vec<Scene> = Vec::with_capacity(data.rows.len() + 1);
    children.push(header);
    for (visual, cells_text) in data.rows.iter().enumerate() {
        // Data-row id for this visual position (identity fallback).
        let data_id = data.row_ids.get(visual).copied().unwrap_or(visual);
        let state = row_states.get(data_id).copied().unwrap_or(RadioState::Idle);
        let selected = row_selected.get(data_id).copied().unwrap_or(false);
        if let Some(mut band) = data.row_headers {
            let roles = SectionRoles::ask(&mut band.sections, data_id);
            row_sections.push(row_header_cell(
                tag,
                data_id,
                &RowSection {
                    roles: &roles,
                    selected: whole_or_nothing(selected),
                    click_tag: tag,
                },
                style.row_header_width,
                theme,
                style,
            ));
        }
        // Zebra parity is **visual** so the stripe pattern stays stable
        // across re-sorts; selection / state are **data-indexed**.
        let fill = row_fill(theme, state, selected, visual);
        let fg = row_fg(theme, state);
        let row_tag = GridTag::data_row(tag, data_id);
        let decorations: Vec<Option<Decoration>> = data.decoration.map_or_else(
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
                // R1563 — the eager surface selects whole rows, so the strip's
                // own fill covers every selected cell and there is nothing
                // left for cell ink to add.
                selected_cells: &[],
                // R1536 — the eager surface answers the decoration role too,
                // through `TableData::decoration`. Asked once per painted cell
                // of this row, the same rule the virtualized grid follows, so
                // the two surfaces cannot disagree about when the role is
                // consulted.
                decorations: &decorations,
                theme,
                // R1544 — the eager `view_table` exposes no editing surface
                // (`VirtualTableData` is the virtualized grid's carrier), for
                // the same reason it exposes no delegate: it is the R707
                // fixed-row table, and editing is a Model/View axis.
                editing: &[],
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
    eager_frame(
        tag,
        children,
        row_sections,
        data.row_headers
            .map_or(CornerAction::Inert, |band| band.corner),
        theme,
        style,
    )
}

/// R707 / R1548 §5.27 — the eager table's outer block: its `[header, rows…]`
/// column, optionally beside the vertical band, inside the tagged rounded frame.
///
/// Extracted at R1548 because the band made [`view_table`] the one function in
/// this file over the line budget, and because the choice it holds is one
/// decision — whether this surface has a second section axis — rather than part
/// of building the rows.
///
/// `row_sections` **empty** is the axis declined: the block then holds `children`
/// directly, with no extra node in it, so a table whose model answers no
/// vertical axis paints a scene byte-identical to the pre-R1548 one.
fn eager_frame(
    tag: &str,
    children: Vec<Scene>,
    row_sections: Vec<Scene>,
    corner: CornerAction,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let column = |items| {
        Scene::Container(
            ContainerNode::new(items).with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
        )
    };
    // Through the same `row_header_column` the virtualized grid uses, so the
    // corner and the band width are one statement across both surfaces.
    let (direction, content) = if row_sections.is_empty() {
        (FlexDirection::Column, children)
    } else {
        let band = row_header_column(
            tag,
            BandCorner {
                width: style.row_header_width,
                corner,
                click_tag: tag,
            },
            column(row_sections),
            theme,
            style,
        );
        (FlexDirection::Row, vec![band, column(children)])
    };
    Scene::Container(
        ContainerNode::new(content)
            .with_tag(tag.to_string())
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow))
                    .with_corner_radius(style.corner_radius),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(direction)
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
    /// same way. The labels come from [`HeaderAxis::label`], asked per section.
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
    /// R1544 §5.27 — the open editor, or `None` when the grid is not editing
    /// (which is every pre-R1544 caller, and every read-only grid).
    ///
    /// The binding wires it from the shared
    /// [`GridEditState`](pinion_core::widgets::grid_edit::GridEditState) it
    /// also routes input through, so the cell that paints an editor and the
    /// cell the keystrokes reach are one fact rather than two that agree.
    pub editing: Option<GridEditing<'a>>,
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
    pub decoration: Option<&'a Decoration>,
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

/// R1544 §5.27 — how one column's **editor** is painted (Qt
/// `QStyledItemDelegate::createEditor` + `setEditorData`), wired per column
/// through [`GridEditing::editor`].
///
/// Qt needs two calls because it hands back a live `QWidget` that must then be
/// populated: `createEditor` constructs, `setEditorData` seeds. A view function
/// rebuilds the editor's subtree from state on every frame, so "construct" and
/// "seed with the current value" are not two moments — there is one, and it
/// takes the seed ([`CellEditRender::edit`]) as an argument. That is not a
/// shortcut around Qt's pair; it is what those two calls collapse to once the
/// editor is a value rather than an object with a lifetime.
pub type CellEditorPainter<'a> = &'a dyn Fn(&CellEditRender<'_>) -> Scene;

/// R1544 §5.27 — what a [`CellEditorPainter`] is given: the display context
/// the cell would have had, plus the editing context.
///
/// It **embeds** the [`CellRender`] rather than restating its nine fields, so
/// an editor keeps the exact geometry, palette and tag rules the display
/// painter follows and a round that adds a field to one adds it to both.
pub struct CellEditRender<'a> {
    /// Everything the cell's display painter would have been given —
    /// including [`CellRender::text`], which is still the **display** role.
    /// An editor that wants to show the original beside the in-flight value
    /// has both.
    pub cell: &'a CellRender<'a>,
    /// The model's `Qt::EditRole` answer for this cell: the seed the editor
    /// opened with, and the [`CellKind`](pinion_core::CellKind) that selects
    /// the keystroke gate and
    /// the commit parser.
    pub edit: &'a CellEdit,
    /// The inline field's `use_text_edit_state` key
    /// ([`GridEditState::field_tag`](pinion_core::widgets::grid_edit::GridEditState::field_tag)).
    /// An editor that hosts text must paint **this** field, because it is the
    /// buffer [`GridEditState::commit_with`](pinion_core::widgets::grid_edit::GridEditState::commit_with)
    /// reads back.
    pub field_tag: &'static str,
    /// The inline field widget's statechart snapshot and caret byte, as the
    /// binding's `read_state` derived them.
    ///
    /// Threaded through rather than synthesized because the editor is a real
    /// [`TextField`](pinion_core::widgets::text_field::TextField) the binding
    /// owns — focus, IME preedit and clipboard all route to that widget, so a
    /// grid that invented a plausible-looking state here would paint a caret
    /// the input path does not agree with.
    pub field: (TextFieldState, u32),
    /// R1555 — the **in-flight** value of a latch-buffered form
    /// ([`EditorForm::Toggle`] / [`EditorForm::Selector`]), from
    /// [`OpenEditor::pending`](pinion_core::widgets::grid_edit::OpenEditor::pending).
    ///
    /// `None` for the text-buffered forms, whose in-flight value is the inline
    /// field's buffer and is therefore already on screen through
    /// [`Self::field_tag`]. Carried separately from [`Self::edit`] because that
    /// is the **seed** — what the editor opened with — and a toggle that has
    /// been flipped but not committed must paint the flip, not the seed.
    pub pending: Option<&'a CellValue>,
    /// R1571 — whether this editor holds the keyboard, and so the shared inline
    /// field named by [`Self::field_tag`].
    ///
    /// The framework has one keyboard focus where Qt has one focusable
    /// `QWidget` per editor, so with N editors open only one of them can be
    /// typed into. A painter must branch on this: the focused editor paints the
    /// live field (caret, selection, IME preedit), and the rest paint
    /// [`Self::parked`].
    pub focused: bool,
    /// R1571 — the in-flight text of a **text-buffered** editor that does not
    /// hold the field ([`OpenEditor::parked_text`](pinion_core::widgets::grid_edit::OpenEditor::parked_text)).
    ///
    /// `None` for the focused editor (its text is in the field) and for the
    /// latch-buffered forms (which have no text at all).
    pub parked: Option<&'a str>,
}

impl CellEditRender<'_> {
    /// R1555 — the datum an editor should **paint**: the in-flight value when
    /// the latch holds one, else the seed.
    ///
    /// One accessor rather than each form deciding, because the fallback is not
    /// a form's choice: a latch-buffered form always has a pending value (its
    /// `begin` put one there), and a text-buffered form never does, so the
    /// `unwrap_or` is the same answer in both cases and stating it twice is what
    /// would let one form paint a stale seed after a gesture.
    #[must_use]
    pub fn in_flight(&self) -> &CellValue {
        self.pending.unwrap_or_else(|| self.edit.value())
    }
}

/// R1544 §5.27 — the open editors, and everything needed to paint them.
///
/// One bundle on [`VirtualTableData`] rather than parallel fields, because they
/// are only meaningful together: `None` **is** "this grid hosts no editors at
/// all", which also means the per-column editor delegate is consulted only
/// while editing — matching Qt, where `createEditor` is called when an edit
/// starts and never during an ordinary paint.
///
/// R1571 — [`Self::open`] became a **slice**, because Qt's
/// `openPersistentEditor` keeps N editors open at once. The binding narrows it
/// to the painted rows through
/// [`GridEditState::open_cells`](pinion_core::widgets::grid_edit::GridEditState::open_cells),
/// so an editor outside the window costs this pass nothing — where Qt's
/// `updateEditorGeometries()` repositions every persistent editor on every
/// scroll whether or not its row is on screen.
#[derive(Clone, Copy)]
pub struct GridEditing<'a> {
    /// R1571 — every open editor whose cell may be painted, with the model's
    /// edit role and its focus already resolved
    /// ([`GridEditState::open_cells`](pinion_core::widgets::grid_edit::GridEditState::open_cells)).
    ///
    /// An **empty** slice is the same statement as `editing: None` and costs
    /// the same: nothing in this grid is being edited right now.
    pub open: &'a [OpenCell],
    /// The inline field's `use_text_edit_state` key.
    pub field_tag: &'static str,
    /// The inline field widget's statechart snapshot and caret byte.
    pub field: (TextFieldState, u32),
    /// Per-**column** editor delegates (Qt
    /// `QAbstractItemView::setItemDelegateForColumn`, editing half):
    /// `editor(col)` returns the [`CellEditorPainter`] for that column, or
    /// `None` for the built-in [`text_cell_editor`].
    ///
    /// Per column and not per cell for the reason [`VirtualTableData::delegate`]
    /// is: which *kind* of editor a column opens is a property of the column.
    /// What varies per cell — whether the cell is editable at all, and what
    /// the editor is seeded with — is the model's role
    /// (`GridModel::edit`), which is asked per cell.
    pub editor: Option<&'a dyn Fn(usize) -> Option<CellEditorPainter<'a>>>,
}

impl core::fmt::Debug for GridEditing<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GridEditing")
            .field("open", &self.open)
            .field("field_tag", &self.field_tag)
            .field("field", &self.field)
            .field("editor", &self.editor.map(|_| "<fn>"))
            .finish()
    }
}

/// R1544 §5.27 — the built-in cell editor: the inline
/// [`TextField`](pinion_core::widgets::text_field::TextField) Qt's
/// `QItemEditorFactory` produces for every text / numeric `QVariant::Type`.
///
/// Sized to the cell it replaces, so opening an editor does not reflow the
/// row, and tagged with [`CellRender::tag`] on its container by the same rule
/// every painter follows — an editing cell stays addressable by pointer and by
/// RPC exactly as a displayed one is.
///
/// Public for the reason [`text_cell_painter`] is: a column whose editor only
/// *extends* the default should compose with it rather than re-derive the
/// cell's padding, size and tag placement.
///
/// The keystroke gate and the commit parser are **not** here — they belong to
/// [`edit_field_keymap`](pinion_core::edit_field_keymap) and
/// [`CellKind::parse`](pinion_core::CellKind::parse), which the binding's
/// input path calls with [`CellEditRender::edit`]'s kind. A painter that also
/// filtered keystrokes would be a second gate that could disagree with the
/// one the events actually pass through.
#[must_use]
pub fn text_cell_editor(c: &CellEditRender<'_>) -> Scene {
    let cell = c.cell;
    let field_style = crate::text_field::TextFieldStyle {
        // The editor fills the cell minus its horizontal padding, so the text
        // it shows starts where the label it replaced did.
        field_w: cell.width.saturating_sub(cell.style.cell_pad_x * 2),
        field_h: cell.height.saturating_sub(EDITOR_INSET_PX * 2),
        ..crate::text_field::TextFieldStyle::m3_filled()
    };
    let field = editor_field_node(c, &field_style);
    Scene::Container(
        ContainerNode::new(vec![field])
            .with_tag(cell.tag.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(cell.width, cell.height))
                    .with_padding(Rect::new(
                        cell.style.cell_pad_x,
                        0,
                        cell.style.cell_pad_x,
                        0,
                    )),
            ),
    )
}

/// Vertical inset of the built-in editor inside its cell, in logical px, so
/// the field's own frame reads as sitting *in* the row rather than as
/// replacing it.
const EDITOR_INSET_PX: u32 = 3;

/// Width of a [`EditorForm::Stepper`]'s arrow column, in logical px — both
/// arrows stack inside it, Qt `QSpinBox`'s `SC_SpinBoxUp` / `SC_SpinBoxDown`
/// sub-controls.
const STEP_COLUMN_W: u32 = 16;

/// Side of a [`EditorForm::Swatch`]'s colour chip, in logical px.
const SWATCH_CHIP_PX: u32 = 18;

// R1555 — the step arrows are **local**, not lifted into [`crate::glyph`].
// That module's own discipline decides it: `U+25B2` / `U+25BC` recurring here
// and in `SORT_ASCENDING` / `SORT_DESCENDING` is a glyph coincidence, not a
// shared gesture, and its doc records the same call for the datepicker's
// month-nav arrows — "deliberately un-lifted for the same semantics reason".
// A third same-gesture consumer (a standalone spin-button paint helper) is what
// would lift them.

/// Increment affordance of a stepper editor — `U+25B2` BLACK UP-POINTING
/// TRIANGLE.
const STEP_UP_GLYPH: &str = "\u{25B2}";

/// Decrement affordance of a stepper editor — `U+25BC` BLACK DOWN-POINTING
/// TRIANGLE.
const STEP_DOWN_GLYPH: &str = "\u{25BC}";

/// R1555 §5.27 — **the built-in editor factory**: which editor a cell opens,
/// chosen by its datum's kind. Qt `QItemEditorFactory`.
///
/// The other half of Qt's editing decomposition from R1544's per-column
/// delegate. A column delegate ([`GridEditing::editor`]) *overrides* this, which
/// is exactly how `setItemDelegateForColumn` relates to the factory in Qt; with
/// no delegate the cell's own datum decides, through
/// [`CellKind::editor_form`](pinion_core::CellKind::editor_form).
///
/// # What it replaces
///
/// [`text_cell_editor`] was the built-in for **every** kind, and for two of the
/// six it is an editor that cannot work: `Bool` and `Choice` refuse every
/// keystroke and parse to nothing, so the seam opened a text field that could
/// not be typed into and whose commit could never produce a value.
///
/// # Where the forms are past Qt's default factory
///
/// See [`EditorForm`] — Qt's bool creator is a
/// two-item combo box, its double creator silently rounds to two decimals, its
/// factory cannot produce a populated combo for an enumerated cell at all, and
/// it has no colour creator, so a colour cell in a plain `QTableView` is not
/// editable.
///
/// Each form is public on its own so a delegate that only *extends* one
/// composes with it instead of re-deriving the cell's padding, size and tag
/// placement — the reason [`text_cell_painter`] is public.
#[must_use]
pub fn cell_editor(c: &CellEditRender<'_>) -> Scene {
    match c.edit.form() {
        EditorForm::Field => text_cell_editor(c),
        EditorForm::Stepper => stepper_cell_editor(c),
        EditorForm::Toggle => toggle_cell_editor(c),
        EditorForm::Selector => selector_cell_editor(c),
        EditorForm::Swatch => swatch_cell_editor(c),
    }
}

/// The editor's outer box: the cell's geometry and padding, and — the contract
/// every cell painter follows — the cell's own hit-test tag.
fn editor_shell(cell: &CellRender<'_>, children: Vec<Scene>) -> Scene {
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(cell.tag.to_string())
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(cell.width, cell.height))
                    .with_padding(Rect::new(
                        cell.style.cell_pad_x,
                        0,
                        cell.style.cell_pad_x,
                        0,
                    )),
            ),
    )
}

/// R1571 §5.27 — the text half of an editor: the **shared inline field** when
/// this editor holds the keyboard, and a static box holding its parked text
/// when it does not.
///
/// This framework has one keyboard focus where Qt has one focusable `QWidget`
/// per editor, so with N editors open exactly one of them owns the field's
/// buffer, its caret, its selection and its IME preedit. Painting
/// [`view_field`](crate::text_field::view_field) for an unfocused editor would
/// draw the *focused* editor's text under this cell's tag — one buffer read
/// through two addresses — which is the defect
/// [`EditBuffer::Parked`](pinion_core::widgets::grid_edit::EditBuffer::Parked)
/// exists to make unrepresentable.
///
/// The picture matches Qt's: an unfocused `QLineEdit` shows its own text with
/// no caret. What differs is the cost, and that is the point — an editor here
/// is state, so an unfocused one is a text node rather than a live widget.
///
/// The three text-buffered forms share it ([`text_cell_editor`],
/// [`stepper_cell_editor`], [`swatch_cell_editor`]) so none of them can be the
/// one that forgets to branch.
fn editor_field_node(c: &CellEditRender<'_>, style: &crate::text_field::TextFieldStyle) -> Scene {
    let cell = c.cell;
    // The branch is on [`Self::parked`] — the BUFFER — rather than on
    // [`Self::focused`], and the difference is not cosmetic. `parked.is_none()`
    // *is* "this editor owns the shared buffer"; `focused` is "this editor has
    // the keyboard". `OpenEditors`' fourth invariant makes them equivalent, and
    // a counterfactual at R1571 showed what branching on the second one costs
    // when they come apart: the field renders under this cell's tag holding
    // text that belongs to no editor at all.
    let Some(parked) = c.parked else {
        return crate::text_field::view_field(
            c.field_tag,
            c.field.0,
            c.field.1,
            cell.theme,
            style,
            "",
        );
    };
    let label = Scene::Text(TextNode::styled(
        parked,
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.font_size_px)
            .with_fg(cell.theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_style(
                BoxStyle::filled(cell.theme.resolve(ColorRole::SurfaceContainerHighest))
                    .with_corner_radius(style.field_corner),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(style.field_w, style.field_h))
                    .with_padding(Rect::new(style.field_pad, 0, style.field_pad, 0)),
            ),
    )
}

/// The width left for an editor's content inside [`editor_shell`], after the
/// cell's horizontal padding and `reserved` px of affordances.
fn editor_content_w(cell: &CellRender<'_>, reserved: u32) -> u32 {
    cell.width
        .saturating_sub(cell.style.cell_pad_x * 2)
        .saturating_sub(reserved)
}

/// R1555 §5.27 — the [`EditorForm::Swatch`] editor: a colour chip beside a hex
/// field.
///
/// Qt's default factory has **no** `QColor` creator, so `createEditor` answers
/// `nullptr`, `QStyledItemDelegate` passes it through, and
/// `QAbstractItemView::edit` then silently does nothing — a colour cell in a
/// plain `QTableView` is simply not editable.
///
/// The hex half is the in-flight buffer ([`EditorForm::buffer_is_text`] is true
/// for this form), so the commit path is the same
/// [`CellKind::parse`](pinion_core::CellKind::parse) every text-buffered form
/// uses. A full picker is a popup the binding owns; what the factory ships is
/// the always-visible chip and an editable hex.
#[must_use]
pub fn swatch_cell_editor(c: &CellEditRender<'_>) -> Scene {
    let cell = c.cell;
    // The chip previews the colour the hex buffer currently spells, read from
    // the same `TextEditState` `view_field` below paints — so the preview cannot
    // lag the text the user is typing.
    let live = pinion_core::widgets::text_edit::use_text_edit_state(c.field_tag).text();
    let parsed = Color::from_hex(live.trim());
    let seed = match c.edit.value() {
        CellValue::Color(color) => *color,
        // A swatch editor is only opened for a `Color` datum; a non-colour seed
        // here would be a factory that dispatched on one kind and was handed
        // another. Painting the palette's outline rather than inventing a colour
        // keeps that case visible instead of plausible.
        _ => cell.theme.resolve(ColorRole::Outline),
    };
    let chip = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_style(
                BoxStyle::filled(parsed.unwrap_or(seed)).with_border(Border::new(
                    // A malformed buffer is not hidden behind a stale swatch:
                    // the chip keeps the last valid colour and says so.
                    if parsed.is_some() {
                        cell.theme.resolve(ColorRole::Outline)
                    } else {
                        cell.theme.resolve(ColorRole::Error)
                    },
                    2,
                )),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(SWATCH_CHIP_PX, SWATCH_CHIP_PX))),
    );
    let field_style = crate::text_field::TextFieldStyle {
        field_w: editor_content_w(cell, SWATCH_CHIP_PX + cell.style.cell_pad_x),
        field_h: cell.height.saturating_sub(EDITOR_INSET_PX * 2),
        ..crate::text_field::TextFieldStyle::m3_filled()
    };
    let field = editor_field_node(c, &field_style);
    editor_shell(
        cell,
        vec![chip, crate::spacer::spacer(cell.style.cell_pad_x, 1), field],
    )
}

/// R1555 §5.27 — the [`EditorForm::Stepper`] editor: the inline field with two
/// step affordances beside it. Qt `QSpinBox` / `QDoubleSpinBox`.
///
/// The arrows are the **only** editor sub-parts with their own addresses
/// ([`GridSendKey::EditorStep`]) — see that variant's doc for why the other
/// forms need none. A press on one routes to the grid's send wire exactly as a
/// cell click does, so a binding steps through
/// [`GridEditState::step`](pinion_core::widgets::grid_edit::GridEditState::step)
/// and the value an arrow produces is the value a keystroke would have.
#[must_use]
pub fn stepper_cell_editor(c: &CellEditRender<'_>) -> Scene {
    let cell = c.cell;
    let field_style = crate::text_field::TextFieldStyle {
        field_w: editor_content_w(cell, STEP_COLUMN_W),
        field_h: cell.height.saturating_sub(EDITOR_INSET_PX * 2),
        ..crate::text_field::TextFieldStyle::m3_filled()
    };
    let field = editor_field_node(c, &field_style);
    let arrows = Scene::Container(
        ContainerNode::new(vec![
            step_arrow(c, true, cell.height),
            step_arrow(c, false, cell.height),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(STEP_COLUMN_W, cell.height)),
        ),
    );
    editor_shell(cell, vec![field, arrows])
}

/// One step affordance, tagged through the [`GridSendKey`] encode SSOT so the
/// press the paint invites is the press the decoder understands.
fn step_arrow(c: &CellEditRender<'_>, up: bool, cell_h: u32) -> Scene {
    let cell = c.cell;
    let key = GridSendKey::EditorStep {
        row: cell.index.row,
        col: cell.index.col,
        up,
    };
    let glyph = if up { STEP_UP_GLYPH } else { STEP_DOWN_GLYPH };
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            glyph,
            Rect::default(),
            TextStyle::new()
                .with_size_px(STEP_GLYPH_PX)
                .with_fg(cell.theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(format!("{}#{}", cell.root, key.encode()))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(STEP_COLUMN_W, cell_h / 2)),
        ),
    )
}

/// Step-arrow glyph size in logical px — half a cell's height holds two of
/// these plus the row's own breathing room.
const STEP_GLYPH_PX: u32 = 9;

/// R1555 §5.27 — the [`EditorForm::Toggle`] editor: an inline checkbox.
///
/// Qt's default factory hands a two-item `QComboBox` reading "False" / "True"
/// for a bool, which is why a Qt application that wants a checkbox writes a
/// delegate. The box is the cell's own hit target (it fills the cell through
/// the shared editor shell), so a click or <kbd>Space</kbd> reaching the cell is the
/// toggle gesture and needs no sub-address.
#[must_use]
pub fn toggle_cell_editor(c: &CellEditRender<'_>) -> Scene {
    let cell = c.cell;
    let checked = matches!(c.in_flight(), CellValue::Bool(true));
    let style = crate::checkbox::CheckboxStyle {
        box_size: cell.height.saturating_sub(EDITOR_INSET_PX * 2).min(20),
        ..crate::checkbox::CheckboxStyle::m3_filled()
    };
    let box_visual = crate::checkbox::view_checkbox_box(
        checked,
        pinion_core::widgets::checkbox::CheckboxState::Idle,
        cell.theme,
        &style,
    );
    let label = Scene::Text(TextNode::styled(
        if checked { "On" } else { "Off" },
        Rect::default(),
        TextStyle::new()
            .with_size_px(cell.style.label_size_px)
            .with_fg(cell.fg),
    ));
    editor_shell(
        cell,
        vec![
            box_visual,
            crate::spacer::spacer(cell.style.cell_pad_x, 1),
            label,
        ],
    )
}

/// R1555 §5.27 — the [`EditorForm::Selector`] editor: the closed selector, Qt
/// `QComboBox`.
///
/// Qt's factory is keyed by `QVariant` type and an enumerated value **is an
/// int** to `QVariant`, so no registration there can produce a combo populated
/// with this cell's options. [`CellValue::Choice`] carries its own domain, so
/// the shipped form can.
///
/// The list itself is a popup, which is an overlay the binding owns — the same
/// division every popup in this tree has. What the factory ships is the closed
/// state: the selected option and the chevron that says it opens.
#[must_use]
pub fn selector_cell_editor(c: &CellEditRender<'_>) -> Scene {
    let cell = c.cell;
    let selected_label = c.in_flight().display();
    let label = Scene::Text(TextNode::styled(
        &selected_label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(cell.style.label_size_px)
            .with_fg(cell.fg),
    ));
    let chevron = Scene::Text(TextNode::styled(
        crate::glyph::DISCLOSURE_EXPANDED,
        Rect::default(),
        TextStyle::new()
            .with_size_px(STEP_GLYPH_PX)
            .with_fg(cell.theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    let content_w = editor_content_w(cell, 0);
    Scene::Container(
        ContainerNode::new(vec![Scene::Container(
            ContainerNode::new(vec![label, chevron]).with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::SpaceBetween)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(content_w, cell.height)),
            ),
        )])
        .with_tag(cell.tag.to_string())
        .with_style(
            BoxStyle::filled(cell.theme.resolve(ColorRole::SurfaceContainerHigh))
                .with_border(Border::new(cell.theme.resolve(ColorRole::Outline), 1)),
        )
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(cell.width, cell.height))
                .with_padding(Rect::new(
                    cell.style.cell_pad_x,
                    0,
                    cell.style.cell_pad_x,
                    0,
                )),
        ),
    )
}

/// R1544 §5.27 — a [`GridEditing`] with its column's editor delegate already
/// **resolved**, as a pane's rows receive it.
///
/// The internal peer of `GridRender::painters`: the public [`GridEditing`]
/// carries the delegate *picker* (`Fn(col) -> Option<CellEditorPainter>`)
/// because that is the shape a binding wires, and this carries the picker's
/// answer because that is the shape a row needs. Resolving between them also
/// keeps the picker's lifetime out of [`RowPane`] — a `&dyn Fn` whose own
/// return type mentions the lifetime is invariant in it, which would force
/// every field of a row pane to outlive the whole grid render.
#[derive(Clone, Copy)]
struct CellEditorSlot<'a> {
    /// The absolute column whose cell is being edited.
    col: usize,
    /// R1571 — the open editor and the model's `Qt::EditRole` answer for its
    /// cell, as [`GridEditState::open_cells`](pinion_core::widgets::grid_edit::GridEditState::open_cells)
    /// resolved them.
    cell: &'a OpenCell,
    /// The inline field's `use_text_edit_state` key.
    field_tag: &'static str,
    /// The inline field widget's statechart snapshot and caret byte.
    field: (TextFieldState, u32),
    /// The column's editor painter, or `None` for the [`cell_editor`] factory.
    paint: Option<CellEditorPainter<'a>>,
}

/// R1535 §5.27 — what a `Qt::DecorationRole` answer can be: the mark the
/// built-in painter draws **beside** the display text.
///
/// # Why it is not named for the cell (R1547)
///
/// It was `CellDecoration` while the cell axis was the only axis with a role
/// dimension. A role is not axis-specific: Qt reaches a cell's mark with
/// `data(index, Qt::DecorationRole)` and a column header's with
/// `headerData(section, Qt::Horizontal, Qt::DecorationRole)` — one role, one
/// `QVariant`, two addresses. Two types here would be two contracts that must
/// agree about what a mark *is*, and the pair R1536 established (ink **and**
/// what the ink means) is exactly the kind of agreement that decays when it is
/// stated twice. So [`GridModel::decoration`] and
/// [`HeaderAxis::decoration`] answer with this, and one painter draws it.
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
pub enum Decoration {
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

impl Decoration {
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
/// [`Qt::DecorationRole`](Decoration) mark when it has one. Qt's
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
        children.push(decoration_node(decoration, c.style.decoration_px, &tag));
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
/// [`view_virtual_table`] asks its consumer for. Qt's `QModelIndex`.
///
/// R1544 moved the definition to
/// [`pinion_core::model_index`] — the editing latch
/// is state, state substrates live in `pinion-core`, and a module cannot name
/// a type defined above it. Re-exported here because this is where every
/// consumer names it, and because the grid's own contract
/// ([`GridModel`], [`CellRender`]) is stated in terms of it.
pub use pinion_core::CellIndex;

/// R1548 §5.27 — **one section axis** of a grid's model: the roles that are
/// asked of a *section* rather than of a cell.
///
/// Qt spells both axes with one virtual, `headerData(int section,
/// Qt::Orientation orientation, int role)`, and pays for it the way it pays for
/// `QVariant`: the orientation is a **runtime argument**, so which roles a
/// model answers may silently differ per axis. The failure has a shape everyone
/// who has written a `QAbstractTableModel` has seen — override `headerData`,
/// handle `Qt::Horizontal`, fall off the end returning `QVariant()`, and
/// `QTableView` paints a vertical header of blank sections that still occupy
/// their width. Nothing reports it: not the model, not the view, not the
/// accessibility tree. The blank strip is indistinguishable from a table whose
/// rows genuinely have no names.
///
/// Here the orientation is promoted from an argument to **the type of the field
/// the axis is stored in** ([`GridModel::columns`] / [`GridModel::rows`]), so:
///
/// - a grid states which axes it answers, and [`no_row_header`] is a written
///   decision rather than a fallthrough;
/// - the two axes answer the **same role set** by construction — a role added
///   here is added to both or to neither, where Qt's orientation branch lets
///   one axis grow a role the other never learns about;
/// - one painter draws either axis's answer, which is the property R1547
///   established for the mark's *type* now holding for the section as a whole.
///
/// The roles are separate typed accessors rather than one `data(section, role)`
/// for the reason [`GridModel`] gives: a role's answer type is then exact, and a
/// model that cannot answer one is unrepresentable rather than returning an
/// invalid variant.
// `Clone` / `Copy` are conditional on the accessors', so the eager surface's
// `&dyn Fn` axis rides inside a `Copy` [`TableData`] while the virtualized
// grid's captured closures stay move-only.
#[derive(Clone, Copy)]
pub struct HeaderAxis<L, D> {
    /// `Qt::DisplayRole` — invoked once per **painted section** with that
    /// section's absolute index, returning its label. A section with no label
    /// returns an empty `String`.
    ///
    /// Windowed: a section outside the painted band is never asked, so the cost
    /// of a header scales with the window rather than with the extent.
    pub label: L,
    /// `Qt::DecorationRole` — invoked once per **painted section**, returning
    /// the mark drawn ahead of that section's label, or `None`.
    ///
    /// Asked per section rather than per cell because that is the axis the
    /// answer varies on: a column-type glyph, a funnel on a filtered column, a
    /// lock on a pinned row — each is a property of the *section*, so asking
    /// per cell would repeat one question once per crossing section and discard
    /// every answer but the first.
    pub decoration: D,
}

/// R1562 §5.27 §5.40 — what a press on the **corner** — the cell where the two
/// section axes meet — does. Qt's `QTableView::setCornerButtonEnabled`.
///
/// Qt's is a `bool` over a private `QTableCornerButton`, and its documented
/// behaviour is one-way: pressing it "selects all cells in the view", with no
/// state to show and no second press that takes the selection back. Here the
/// two arms are the two decisions, and the acting one **carries what it will
/// show** — the tri-state of an HTML header checkbox, which is the shape every
/// modern table's select-all has and which Qt's button cannot express because it
/// has no value at all.
/// R1562 — re-exported where the corner names it, by the rule
/// [`CellIndex`] is re-exported here: this is where every consumer of the band
/// declaration meets the type, and the grid's own contract is stated in terms
/// of it. Defined in `pinion-core` beside
/// [`VirtualSelect`](pinion_core::widgets::virtual_select::VirtualSelect),
/// because the extent is a fact about a **selection model** that a control
/// happens to show.
pub use pinion_core::widgets::virtual_select::SelectionExtent as CornerExtent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerAction {
    /// `setCornerButtonEnabled(false)`: a blank block that closes the two
    /// bands' corner and does nothing. The pre-R1562 corner, and a **written**
    /// decision in the sense [`no_row_header`] is one.
    Inert,
    /// `setCornerButtonEnabled(true)`, with the extent the control shows:
    /// pressing it selects every row, or clears when every row is already
    /// selected ([`VirtualSelect::toggle_all`](pinion_core::widgets::virtual_select::VirtualSelect::toggle_all)).
    SelectAll(SelectionExtent),
}

/// R1562 §5.27 — the **vertical header band**: its section axis, plus the
/// corner where that band meets the horizontal one.
///
/// A type rather than a third field on [`HeaderAxis`], because a corner is not
/// a role: the horizontal axis has no corner of its own, and a field that means
/// nothing on one of the two axes is exactly the drift [`HeaderAxis`] exists to
/// prevent one level up. It is also where Qt puts it — `cornerButtonEnabled` is
/// a `QTableView` property that can only matter when the vertical header is
/// there to make a corner.
///
/// Naming the band as a whole is what lets the corner reach both paint surfaces
/// (the virtualized [`view_virtual_table`] and the eager [`view_table`]) from
/// the one place they already share, `row_header_column`.
#[derive(Clone, Copy)]
pub struct RowHeaderAxis<L, D> {
    /// The roles each **section** of the band answers (Qt `headerData(section,
    /// Qt::Vertical, role)`).
    pub sections: HeaderAxis<L, D>,
    /// What a press on the corner does.
    pub corner: CornerAction,
}

impl<L, D> RowHeaderAxis<L, D> {
    /// A band whose corner does nothing — Qt's `setCornerButtonEnabled(false)`,
    /// and every band painted before R1562.
    pub fn inert(sections: HeaderAxis<L, D>) -> Self {
        Self {
            sections,
            corner: CornerAction::Inert,
        }
    }

    /// A band whose corner is the tri-state select-all control, showing
    /// `extent`.
    pub fn select_all(sections: HeaderAxis<L, D>, extent: SelectionExtent) -> Self {
        Self {
            sections,
            corner: CornerAction::SelectAll(extent),
        }
    }
}

/// The type [`HeaderAxis::labelled`] and [`no_row_header`] name for "this
/// axis answers no `Qt::DecorationRole`" — a plain `fn` pointer so the
/// constructors have a nameable return type and monomorphize to a constant
/// `None`.
type NoSectionDecoration = fn(usize) -> Option<Decoration>;

/// The type [`no_row_header`] names for "this axis answers no
/// `Qt::DisplayRole`".
type NoSectionLabel = fn(usize) -> String;

impl HeaderAxis<NoSectionLabel, NoSectionDecoration> {
    /// R1548 — Qt's own default `headerData` for a section that has no name of
    /// its own: the 1-based section number.
    ///
    /// `QAbstractItemModel::headerData` returns `section + 1` for an
    /// un-overridden model, which is why an untouched `QTableView` shows
    /// `1, 2, 3…` down its left edge. Provided as a **named** adapter, so a
    /// grid that wants row numbers says so, rather than inheriting them from a
    /// base class it forgot to override.
    ///
    /// The number is the section's absolute index, so under a sort permutation
    /// it numbers the *model's* rows and follows them as they move — the same
    /// answer `QSortFilterProxyModel` produces by mapping the section back to
    /// the source model before asking.
    #[must_use]
    pub fn row_numbers() -> Self {
        Self {
            label: section_number,
            decoration: no_section_decoration,
        }
    }
}

impl<L: FnMut(usize) -> String> HeaderAxis<L, NoSectionDecoration> {
    /// R1548 — an axis that answers `Qt::DisplayRole` and nothing else, which
    /// is every header this framework painted before R1547.
    pub fn labelled(label: L) -> Self {
        Self {
            label,
            decoration: no_section_decoration,
        }
    }
}

/// R1548 §5.27 — the [`GridModel::rows`] of a grid with **no vertical header**:
/// a `QTableView` whose `verticalHeader()` is hidden, or a `QListView`, which
/// has no such axis at all.
///
/// A named function rather than a bare `None` because `None` does not typecheck
/// here — the axis's two accessor types would be unconstrained — and because
/// naming it is the point. This is the statement Qt cannot make: a Qt model that
/// does not answer an orientation is byte-identical, at every observation point,
/// to one that answers it with blanks, and the resulting strip of empty sections
/// is reported by nothing. Here it is a written decision, greppable, and the
/// view asks the axis **zero** times a frame.
#[must_use]
pub fn no_row_header() -> Option<RowHeaderAxis<NoSectionLabel, NoSectionDecoration>> {
    None
}

/// The [`HeaderAxis::label`] accessor of [`HeaderAxis::row_numbers`]: Qt's
/// default `headerData`, `section + 1`.
fn section_number(section: usize) -> String {
    (section + 1).to_string()
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
/// [`HeaderAxis::label`] instead of folding it into `cell`.
///
/// Five of Qt's roles are answered — `DisplayRole` ([`Self::cell`]),
/// `DisplayRole` on a section axis ([`HeaderAxis::label`]), `DecorationRole`
/// ([`Self::decoration`]), `DecorationRole` on a section axis (R1547,
/// [`HeaderAxis::decoration`]) and, since R1544, `EditRole` ([`Self::edit`]).
/// `ToolTipRole` is not: it needs a per-cell hover path.
///
/// # The two axes (R1547, R1548)
///
/// Qt's role enum is shared by `data(index, role)` and `headerData(section,
/// orientation, role)`, so a role is a question that can be asked of a cell or
/// of a section. Until R1547 only the cell axis here could be asked anything
/// but its name: a column could say what it was **called** and nothing else.
/// R1548 then gave the *section* question its second axis — Qt's
/// `Qt::Vertical`, the row headers — and did it by making an axis a **type**
/// ([`HeaderAxis`]) rather than a second pair of loose accessors, so the two
/// axes answer one role set instead of two that must be kept in agreement.
///
/// The cell axis and a section axis are not obliged to answer the same set — a
/// section has no `EditRole`, because a header is not edited in place — but a
/// role either answers is answered with the same type ([`Decoration`]) and
/// drawn by the same painter, so they cannot drift into disagreeing about what
/// a mark is.
pub struct GridModel<C, CL, CD, RL, RD, D, E> {
    /// Invoked once per **painted cell** with that cell's [`CellIndex`],
    /// returning its text (Qt `data(QModelIndex)`, Flutter `cellBuilder`).
    pub cell: C,
    /// R1530 / R1548 — the **horizontal** section axis: the column headers (Qt
    /// `headerData(section, Qt::Horizontal, …)`).
    pub columns: HeaderAxis<CL, CD>,
    /// R1548 — the **vertical** section axis: the row headers (Qt
    /// `headerData(section, Qt::Vertical, …)`, painted by
    /// `QTableView::verticalHeader()`).
    ///
    /// Asked once per **painted row**, with the row's absolute *data* index —
    /// not its position on screen — so a mark stays with its row across a sort
    /// or a filter, and [`HeaderAxis::row_numbers`] numbers the model rather
    /// than the viewport.
    ///
    /// `None` ([`no_row_header`]) is a grid with no vertical header, which is
    /// every caller before R1548: the band is not painted and the axis is asked
    /// zero times a frame.
    ///
    /// The presence of the axis is stated **here**, on the model, and nowhere
    /// else — its width is a [`TableStyle::row_header_width`] dimension, beside
    /// every other pixel. The pair could have been split the way Qt splits it
    /// (`headerData` answers; `verticalHeader()->hide()` decides), but a
    /// separately-stated "paint a band" flag would make a band of blank sections
    /// over an unanswered axis representable — which is the exact Qt failure
    /// this type exists to remove. Painted if and only if answered.
    pub rows: Option<RowHeaderAxis<RL, RD>>,
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
    /// R1544 — invoked with a [`CellIndex`], returning that cell's
    /// `Qt::EditRole` answer, or `None` when the cell **cannot be edited**
    /// (Qt: `flags(index)` without `Qt::ItemIsEditable`).
    ///
    /// The one role that is not asked once per painted cell. Qt does ask it
    /// per paint — `QStyledItemDelegate::initStyleOption` reads `EditRole` as
    /// a fallback for a missing `DisplayRole` — but here the display role is
    /// mandatory, so the only things that need this answer are the *four*
    /// moments editing has: opening an editor on a cell, seeding it,
    /// advancing to the next editable cell, and telling assistive technology
    /// whether a cell is read-only. The first three are events; the fourth is
    /// per cell but on the a11y walk, not the paint walk.
    ///
    /// A read-only grid passes [`no_edit`] and pays nothing.
    pub edit: E,
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
pub fn no_decoration(_: CellIndex) -> Option<Decoration> {
    None
}

/// R1547 §5.27 — the [`HeaderAxis::decoration`] accessor for an axis where no
/// **section** carries a mark: `headerData(section, orientation,
/// Qt::DecorationRole)` answered with an invalid `QVariant` on every section.
///
/// The section-axis peer of [`no_decoration`], and separate from it because the
/// two accessors take different addresses — a [`CellIndex`] and a section index
/// — which is the distinction Qt draws with two entry points and pinion draws
/// with two types. A single `|_| None` would unify them only by erasing that.
///
/// R1548 renamed it from `no_header_decoration`: one function now serves both
/// section axes, which is the first evidence that grouping them into
/// [`HeaderAxis`] was the right cut — a *column*-named answer for the row axis
/// would have been the second contract [`HeaderAxis`] exists to prevent.
#[must_use]
pub fn no_section_decoration(_: usize) -> Option<Decoration> {
    None
}

/// R1544 §5.27 — the [`GridModel::edit`] accessor for a **read-only** grid:
/// Qt's `flags()` without `Qt::ItemIsEditable` on every index.
///
/// The peer of [`no_decoration`], and a named function for the same two
/// reasons: "this grid is read-only" becomes greppable, and the read-only case
/// reads as a decision rather than as a closure that happens to return
/// nothing. It is also the answer that makes the a11y walk mark every cell
/// `read_only` — a display-only grid says so to assistive technology instead
/// of staying silent about it.
#[must_use]
pub fn no_edit(_: CellIndex) -> Option<CellEdit> {
    None
}

/// R1530 §5.27 — the [`HeaderAxis::label`] accessor for a grid whose labels
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
/// - `selection` — the R1563 [`GridSelection`] question. A `Fn(usize) -> bool`
///   satisfies it (a row predicate is a selection of whole records), which is
///   what every row-select caller passes. A `true` row's
///   strip is washed with the accent tint (the same `row_fill` selection
///   path the eager [`view_table`] uses). This is the data-indexed
///   generalization of a single selected index — single-select passes
///   `|id| selected == Some(id)`, R782 multi-select passes set membership,
///   and a display-only grid passes `|_| false`. One virtualized-grid paint
///   path serves all three (no parallel `_multi` body to diverge from the
///   windowed-sizer geometry). It is invoked only for the windowed rows.
/// - `model` — the [`GridModel`]: `cell` (R1524, once per painted cell),
///   `columns` (R1530 / R1548, once per painted column header), `rows` (R1548,
///   once per painted row header — [`no_row_header`] for a grid with no
///   vertical header) and `decoration` (R1535, once per painted cell —
///   [`no_decoration`] when no column carries a mark). This is the Model/View
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
    selection: &impl GridSelection,
    model: GridModel<
        impl FnMut(CellIndex) -> String,
        impl FnMut(usize) -> String,
        impl FnMut(usize) -> Option<Decoration>,
        impl FnMut(usize) -> String,
        impl FnMut(usize) -> Option<Decoration>,
        impl FnMut(CellIndex) -> Option<Decoration>,
        impl FnMut(CellIndex) -> Option<CellEdit>,
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
    let GridModel {
        cell,
        columns,
        mut rows,
        decoration,
        // R1544 — the `EditRole` accessor is not a paint-time role: the open
        // editor's seed reaches the painter already resolved, on
        // `GridEditing::edit`. It rides the model bundle because it is a model
        // role (the editing verbs and the a11y walk ask it), and a paint pass
        // that also asked would invoke it once per painted cell for an answer
        // only four events need.
        edit: _,
    } = model;
    // The cell + column roles are consumed by exactly one branch (an
    // `if`/`else`), so each helper takes them by value without a re-move.
    let content = if frozen_cols == 0 {
        render.render_unsplit(scroll, selection, cell, columns, decoration)
    } else {
        render.render_frozen(scroll, frozen_cols, selection, cell, columns, decoration)
    };
    // R1548 — the vertical header band, pinned left of everything the panes
    // built. Composed here rather than inside the two assemblies so both of
    // them — and any future third — gain the axis from one place; a grid with
    // no band skips this entirely and paints the pre-R1548 scene.
    let content = match rows.as_mut() {
        None => content,
        Some(rows) => {
            let width = style.row_header_width;
            // Asked over the same window the band paints, with each visual
            // position resolved to its DATA row, so a mark stays with its row
            // across a sort — the discipline `ask_sections` enforces on the
            // column axis and `data_row` follows on this one. R1562 asks the
            // selection predicate here too, over the same rows, so the band's
            // fill and the strip's fill are one answer read twice rather than
            // two derivations that must agree.
            let sections = window
                .indices()
                .map(|view_pos| {
                    let row = render.source_of(view_pos);
                    (
                        row,
                        SectionRoles::ask(&mut rows.sections, row),
                        selection.row(row),
                    )
                })
                .collect::<Vec<_>>();
            let pane = row_header_pane(
                tag,
                scroll.body,
                &window,
                RowHeaderBand {
                    width,
                    total_h,
                    sections: &sections,
                    corner: rows.corner,
                    click_tag: tag,
                },
                theme,
                style,
            );
            Scene::Container(
                ContainerNode::new(vec![pane, content]).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_flex_grow(1.0),
                ),
            )
        }
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
    fn row_inputs(&self, view_pos: usize, selection: &dyn GridSelection) -> (usize, Color, Color) {
        let source = self.source_of(view_pos);
        let selected = selection.row(source) == SelectionExtent::All;
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
    fn header_band<L, D>(&self, columns: &mut HeaderAxis<L, D>, layout: ColumnLayout<'_>) -> Scene
    where
        L: FnMut(usize) -> String,
        D: FnMut(usize) -> Option<Decoration>,
    {
        let span = layout.col_base..layout.col_base + layout.widths.len();
        let sections = ask_sections(columns, span);
        header_row(
            self.tag,
            self.click_tag,
            &sections,
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

    /// R1544 §5.27 — the open editor narrowed to data row `row`: `Some` only
    /// when the editing cell is in this row.
    ///
    /// Asked once per painted **row** rather than once per painted cell, which
    /// is the axis the row question lives on — the same per-unit discipline
    /// [`Self::painters`] applies to the column axis. A grid that is not
    /// editing answers `None` here and every cell takes the display path with
    /// one comparison against a `None`, not a per-cell index equality.
    fn editing_in_row(&self, row: usize) -> Vec<CellEditorSlot<'d>> {
        let Some(editing) = self.data.editing else {
            return Vec::new();
        };
        editing
            .open
            .iter()
            .filter(|cell| cell.editor.index.row == row)
            .map(|cell| CellEditorSlot {
                col: cell.editor.index.col,
                cell,
                field_tag: editing.field_tag,
                field: editing.field,
                // Qt calls `createEditor` when the edit opens, not on every
                // paint; resolving here means the column's editor delegate is
                // asked once per painted row of an editing row — which is
                // once, because a row is painted once.
                paint: editing.editor.and_then(|pick| pick(cell.editor.index.col)),
            })
            .collect()
    }

    fn render_unsplit(
        &self,
        scroll: GridScroll<'_>,
        selection: &dyn GridSelection,
        mut cell: impl FnMut(CellIndex) -> String,
        mut columns: HeaderAxis<
            impl FnMut(usize) -> String,
            impl FnMut(usize) -> Option<Decoration>,
        >,
        mut decoration: impl FnMut(CellIndex) -> Option<Decoration>,
    ) -> Scene {
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
            &mut columns,
            ColumnLayout {
                widths: &self.widths[span.clone()],
                resizable: self.data.resizable,
                pad,
                col_base: cols.first,
                container_tag: &hrow_tag,
                selection,
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
                let (source, fill, fg) = self.row_inputs(view_pos, selection);
                let (cells_text, decos) =
                    pane_cells(&mut cell, &mut decoration, source, span.clone());
                let selected_cells = cell_ink(selection, source, span.clone());
                let cell_refs: Vec<&str> = cells_text.iter().map(String::as_str).collect();
                let row_tag = GridTag::data_row(self.tag, source);
                let editing = self.editing_in_row(source);
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
                        selected_cells: &selected_cells,
                        theme: self.theme,
                        editing: &editing,
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
    /// R859 §5.27 — the frozen split's **two** header bands: the pinned one
    /// over columns `0..frozen_cols`, and the scrolling one over the windowed
    /// slice already resolved by the caller.
    ///
    /// Extracted from `render_frozen` because they are one decision — how the
    /// split divides the header axis — and because the two differ in exactly
    /// three things (which columns, whether a resize grabber is reserved, and
    /// which container tag), which reads as a pair here and as forty lines of
    /// near-repetition inline.
    ///
    /// R1530 — each band asks for **its own** sections, in absolute
    /// coordinates, so neither pane can be handed labels it will not paint.
    /// The frozen columns do not horizontally scroll, so a resize grabber
    /// (which grows the horizontal extent) is moot for them.
    fn split_header_bands<L, D>(
        &self,
        columns: &mut HeaderAxis<L, D>,
        frozen_cols: usize,
        scrolled_window: &[u32],
        scrolled_base: usize,
        scroll_pad: ColumnPad,
        selection: &dyn GridSelection,
    ) -> (Scene, Scene)
    where
        L: FnMut(usize) -> String,
        D: FnMut(usize) -> Option<Decoration>,
    {
        let fhrow_tag = GridTag::frozen_header_row(self.tag);
        let frozen = self.header_band(
            columns,
            ColumnLayout {
                widths: &self.widths[..frozen_cols],
                resizable: false,
                pad: ColumnPad::NONE,
                col_base: 0,
                container_tag: &fhrow_tag,
                selection,
            },
        );
        let hrow_tag = GridTag::header_row(self.tag);
        let scrolling = self.header_band(
            columns,
            ColumnLayout {
                widths: scrolled_window,
                resizable: self.data.resizable,
                pad: scroll_pad,
                col_base: scrolled_base,
                container_tag: &hrow_tag,
                selection,
            },
        );
        (frozen, scrolling)
    }

    fn render_frozen(
        &self,
        scroll: GridScroll<'_>,
        frozen_cols: usize,
        selection: &dyn GridSelection,
        mut cell: impl FnMut(CellIndex) -> String,
        mut columns: HeaderAxis<
            impl FnMut(usize) -> String,
            impl FnMut(usize) -> Option<Decoration>,
        >,
        mut decoration: impl FnMut(CellIndex) -> Option<Decoration>,
    ) -> Scene {
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
            let (source, fill, fg) = self.row_inputs(view_pos, selection);
            // R1524 — the pinned pane asks for the pinned columns only.
            let (cells_text, decos) =
                pane_cells(&mut cell, &mut decoration, source, 0..frozen_cols);
            let cell_refs: Vec<&str> = cells_text.iter().map(String::as_str).collect();
            let selected_cells = cell_ink(selection, source, 0..frozen_cols);
            // R859 — distinct `_frow{id}` container tag so the split panes
            // never emit a duplicate strip tag (per-cell `_{col}` tags stay
            // unique by absolute column).
            let frow_tag = GridTag::frozen_data_row(self.tag, source);
            let editing = self.editing_in_row(source);
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
                    selected_cells: &selected_cells,
                    theme: self.theme,
                    editing: &editing,
                },
                self.style,
            )
        });
        let scroll_slots = uniform_slots(self.window, scroll_w, row_pitch, |view_pos| {
            let (source, fill, fg) = self.row_inputs(view_pos, selection);
            // R1524 — the scrolling pane asks for its column window, in
            // absolute coordinates (`abs`), so the consumer never learns that
            // this pane's own column space starts at `frozen_cols`.
            let (cells_text, decos) = pane_cells(&mut cell, &mut decoration, source, abs.clone());
            let cell_refs: Vec<&str> = cells_text.iter().map(String::as_str).collect();
            let selected_cells = cell_ink(selection, source, abs.clone());
            let row_tag = GridTag::data_row(self.tag, source);
            let editing = self.editing_in_row(source);
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
                    selected_cells: &selected_cells,
                    theme: self.theme,
                    editing: &editing,
                },
                self.style,
            )
        });

        let (frozen_header, scroll_header) = self.split_header_bands(
            &mut columns,
            frozen_cols,
            &scrolled[rel.clone()],
            abs.start,
            scroll_pad,
            selection,
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

    /// R1562 — the EAGER surface's band, which had no paint test in this crate
    /// at all: its sections carry the same press address the virtualized band's
    /// do, and its corner is whatever `row_headers` declared.
    ///
    /// Written because the round made `CornerAction::SelectAll` representable on
    /// a surface no shipped consumer declares it on — a state with no coverage
    /// is a state that drifts, and `eager_frame` reads the corner off
    /// `TableData::row_headers` through its own `map_or`.
    #[test]
    fn r1562_the_eager_band_presses_and_carries_its_corner() {
        let (headers, rows) = data();
        let label = |row: usize| format!("R{row}");
        let deco: fn(usize) -> Option<Decoration> = |_| None;
        let eager = |corner| {
            let label = &label as &dyn Fn(usize) -> String;
            let deco = &deco as &dyn Fn(usize) -> Option<Decoration>;
            Owner::new().run(|| {
                view_table(
                    "table",
                    TableData {
                        headers: &headers,
                        rows: &rows,
                        row_ids: &[],
                        decoration: None,
                        header_decoration: None,
                        row_headers: Some(RowHeaderAxis {
                            sections: HeaderAxis {
                                label,
                                decoration: deco,
                            },
                            corner,
                        }),
                    },
                    TableSelection {
                        rows: &[false, true, false],
                        cells: None,
                    },
                    &all_idle(),
                    None,
                    &light(),
                    &TableStyle::m3(),
                )
            })
        };
        let inert = eager(CornerAction::Inert);
        for row in 0..rows.len() {
            let key = format!("table#{}", GridSendKey::RowHeader { row }.encode());
            assert!(
                inert.contains_tag(&key),
                "eager section {row} is pressable at {key}",
            );
        }
        assert!(
            !inert.contains_tag("table#c"),
            "an inert corner has no address on this surface either",
        );
        // The selected row's section is washed here too — one derivation, both
        // surfaces, so the eager band cannot be the one that stays silent.
        let fill = |scene: &Scene, row: usize| {
            find_tagged(scene, &GridTag::row_header("table", row))
                .expect("the section is painted")
                .style
                .fill
        };
        assert_ne!(
            fill(&inert, 1),
            fill(&inert, 0),
            "row 1 is the selected one"
        );
        let live = eager(CornerAction::SelectAll(CornerExtent::Partial));
        assert!(
            live.contains_tag("table#c"),
            "a declared corner is pressable on the eager surface",
        );
        let node = find_tagged(&live, "table#c").expect("the corner");
        let mut marks = Vec::new();
        for child in &node.children {
            collect_text(child, &mut marks);
        }
        assert_eq!(
            marks,
            vec![crate::glyph::SELECT_ALL_PARTIAL],
            "and it paints the extent it was given, by the same painter",
        );
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
                    header_decoration: None,
                    row_headers: None,
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
                    header_decoration: None,
                    row_headers: None,
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
                    header_decoration: None,
                    row_headers: None,
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
                    header_decoration: None,
                    row_headers: None,
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
                    editing: None,
                },
                &theme,
                &style,
                &|_| false,
                GridModel {
                    cell: vt_cell,
                    columns: HeaderAxis::labelled(vt_header),
                    rows: no_row_header(),
                    decoration: no_decoration,
                    edit: no_edit,
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

    /// R1536 — the accessible name the §5.40 derivation gives the node at
    /// `tag`, over the paint scene the shell hands it.
    ///
    /// R1547 — it **calls** `pinion_a11y::enrich_names_from_scene` instead of
    /// restating its precedence. The R1536 version was a hand-rolled mirror
    /// (`aria_label`, else the first *direct-child* `Text`), and a mirror of a
    /// derivation is a second implementation free to disagree with it — which
    /// it did: production walks descendants depth-first, so it names a header
    /// cell (whose label sits inside the clickable inner container) while the
    /// mirror answered `None`. Every assertion here is now against the function
    /// that runs in the shell, which is the whole point of deriving names from
    /// the paint in the first place.
    fn cell_access_name(scene: &Scene, tag: &str) -> Option<String> {
        let mut nodes = vec![pinion_a11y::AccessNode::new(
            tag,
            pinion_a11y::AriaRole::GridCell,
        )];
        pinion_a11y::enrich_names_from_scene(&mut nodes, scene);
        nodes.into_iter().next().and_then(|n| n.name)
    }

    /// R1536 — a decorative test mark keyed to `n`, so an assertion can name
    /// which cell's answer it is looking at. `meaning` empty = the `alt=""`
    /// arm; the tests that exercise the meaningful arm spell it out.
    fn test_swatch(n: u8) -> Decoration {
        Decoration::Swatch {
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
                    editing: None,
                },
                &theme,
                &style,
                &|_| false,
                GridModel {
                    cell: vt_cell,
                    columns: HeaderAxis::labelled(vt_header),
                    rows: no_row_header(),
                    decoration,
                    edit: no_edit,
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
                columns: HeaderAxis::labelled(wide_header),
                rows: no_row_header(),
                decoration: deco,
                edit: no_edit,
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
                columns: HeaderAxis::labelled(wide_header),
                rows: no_row_header(),
                decoration: |c: CellIndex| {
                    Some(test_swatch(u8::try_from(c.col % 256).unwrap_or(0)))
                },
                edit: no_edit,
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
                    editing: None,
                },
                &theme,
                &style,
                &|_| false,
                GridModel {
                    cell: |c: CellIndex| {
                        if c.col == 1 {
                            text.to_string()
                        } else {
                            vt_cell(c)
                        }
                    },
                    columns: HeaderAxis::labelled(vt_header),
                    rows: no_row_header(),
                    decoration: |c: CellIndex| {
                        (c.col == 1).then(|| Decoration::Swatch {
                            color: Color::rgb(0, 0, 0),
                            meaning: owned.clone(),
                        })
                    },
                    edit: no_edit,
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
            Decoration::Icon {
                source: "s".into(),
                meaning: "Folder".into()
            }
            .meaning(),
            "Folder",
        );
        assert_eq!(
            Decoration::Swatch {
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
            (c.col == 1).then(|| Decoration::Swatch {
                color: Color::rgb(u8::try_from(c.row).unwrap_or(0), 0, 0),
                meaning: "Marked".to_string(),
            })
        };
        let render = |deco: Option<&dyn Fn(CellIndex) -> Option<Decoration>>| {
            Owner::new().run(|| {
                view_table(
                    "table",
                    TableData {
                        headers: &headers,
                        rows: &rows,
                        row_ids: &[],
                        decoration: deco,
                        header_decoration: None,
                        row_headers: None,
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

    // ── R1547 the section axis answers a role ───────────────────────

    /// R1547 — the mark on column `col`'s **header**, found by its address
    /// (`GridTag::header_decoration`), like its cell peer.
    fn header_swatch(scene: &Scene, root: &str, col: usize) -> Option<Color> {
        find_tagged(scene, &GridTag::header_decoration(root, col)).map(|c| c.style.fill)
    }

    /// R1547 — render the virtual table with `col`'s header answering the
    /// section-axis decoration role with `meaning`, counting the asks.
    ///
    /// `wide` widens the table past the viewport so the column axis actually
    /// windows; the windowing tests need that and the rest do not care.
    fn run_vtable_header_mark(col: usize, meaning: &str, wide: bool, asks: &Cell<usize>) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, 200);
        let pitch = i32::try_from(TableStyle::m3().row_height).unwrap();
        state.set_max(0, i32::try_from(VT_N).unwrap() * pitch);
        let h_state = vt_hscroll(0);
        let theme = light();
        let style = TableStyle::m3();
        let cols = if wide { 40 } else { VT_HEADERS.len() };
        let meaning = meaning.to_string();
        let header_decoration = |c: usize| {
            asks.set(asks.get() + 1);
            (c == col).then(|| Decoration::Swatch {
                color: Color::rgb(9, 0, 0),
                meaning: meaning.clone(),
            })
        };
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll {
                    body: &state,
                    horizontal: &h_state,
                },
                VirtualTableData {
                    column_count: cols,
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
                    editing: None,
                },
                &theme,
                &style,
                &|_| false,
                GridModel {
                    cell: vt_cell,
                    columns: HeaderAxis {
                        label: vt_header,
                        decoration: header_decoration,
                    },
                    rows: no_row_header(),
                    decoration: no_decoration,
                    edit: no_edit,
                },
            )
        })
    }

    /// The seam: the marked column's header carries the model's mark, and no
    /// other section's does.
    ///
    /// The negative half is load-bearing — without it a painter that marked
    /// every header with the answer it got for one would pass.
    #[test]
    fn r1547_a_marked_section_carries_the_models_swatch() {
        let asks = Cell::new(0);
        let scene = run_vtable_header_mark(1, "", false, &asks);
        assert_eq!(
            header_swatch(&scene, "vtbl", 1),
            Some(Color::rgb(9, 0, 0)),
            "the marked header carries the colour the section role answered",
        );
        assert_eq!(
            header_swatch(&scene, "vtbl", 0),
            None,
            "and a section whose role answered `None` carries no mark",
        );
        assert_eq!(
            header_swatch(&scene, "vtbl", 2),
            None,
            "on either side of it",
        );
    }

    /// The mark is not a cell's. R1547 gave the section axis its own address
    /// space precisely so `_deco0_1` (cell (0, 1)) and `_hdeco1` (the header of
    /// column 1) cannot be confused; asserted here on a painted scene rather
    /// than only on the formatter.
    #[test]
    fn r1547_a_section_mark_is_not_a_cell_mark() {
        let asks = Cell::new(0);
        let scene = run_vtable_header_mark(1, "", false, &asks);
        assert!(
            header_swatch(&scene, "vtbl", 1).is_some(),
            "premise: column 1's header is marked",
        );
        assert_eq!(
            cell_swatch(&scene, "vtbl", 0, 1),
            None,
            "and NO cell of that column is — the two axes answer separately, \
             so a header mark leaking into row 0 would be a real defect",
        );
    }

    /// **Windowed like the label.** Over 40 columns showing a handful, the
    /// section role is asked once per painted header — not once per column.
    ///
    /// Equality against the header cells actually in the tree, like the R1530
    /// label peer: a `<=` bound would pass on an implementation that asked for
    /// nothing.
    #[test]
    fn r1547_the_section_role_is_asked_per_painted_section() {
        let asks = Cell::new(0);
        let scene = run_vtable_header_mark(1, "", true, &asks);
        let painted = header_cols(&scene).len();
        assert!(
            painted > 0 && painted < 40,
            "premise: the column axis windowed, {painted} of 40 painted",
        );
        assert_eq!(
            asks.get(),
            painted,
            "the section's mark is asked for exactly the sections painted",
        );
    }

    /// A model that marks nothing paints the pre-R1547 header **exactly** —
    /// asserted as a node count, because a mark carries no text and a text
    /// comparison could not see one.
    #[test]
    fn r1547_a_model_that_marks_nothing_changes_nothing() {
        let asks = Cell::new(0);
        // `usize::MAX` is a section index no window reaches, so the answer is
        // `None` everywhere while the accessor is still a real closure.
        let marked_none = run_vtable_header_mark(usize::MAX, "", false, &asks);
        let plain = run_vtable_h(200, 0, 0);
        assert_eq!(
            node_count(&marked_none),
            node_count(&plain),
            "an unmarked header band emits the pre-R1547 node exactly",
        );
    }

    /// §5.40 — a **meaningful** section mark joins the `columnheader`'s
    /// accessible name, ahead of the label.
    ///
    /// This is the Qt divergence the round is for: `QAccessibleTableHeaderCell`
    /// names a section from `headerData(..., Qt::DisplayRole)` alone, so a Qt
    /// header whose distinguishing information is its glyph announces only the
    /// column's name.
    #[test]
    fn r1547_a_meaningful_section_mark_is_announced() {
        let asks = Cell::new(0);
        let scene = run_vtable_header_mark(1, "Primary key", false, &asks);
        assert_eq!(
            cell_access_name(&scene, &GridTag::col_header("vtbl", 1)).as_deref(),
            Some("Primary key Name"),
            "the meaning precedes the label in the composed header name",
        );
    }

    /// And a **decorative** one is silent: `meaning: \"\"` leaves the header
    /// named by its label alone, so a legend swatch cannot make a screen reader
    /// say the column's name twice.
    ///
    /// The pair is what makes either assertion mean anything — a derivation
    /// that always composed, or never did, would pass one of them.
    #[test]
    fn r1547_a_decorative_section_mark_is_silent() {
        let asks = Cell::new(0);
        let scene = run_vtable_header_mark(1, "", false, &asks);
        assert!(
            header_swatch(&scene, "vtbl", 1).is_some(),
            "premise: the header IS marked — decorative means unannounced, \
             not absent",
        );
        assert_eq!(
            cell_access_name(&scene, &GridTag::col_header("vtbl", 1)).as_deref(),
            Some("Name"),
            "named by its label alone",
        );
    }

    /// The name comes from the **paint**, and nothing else supplies one.
    ///
    /// R1547 removed the labels from the a11y builders so this is the only
    /// source; the guard is that an unmarked header still derives its name from
    /// the painted label rather than from an `aria_label` the painter left set.
    #[test]
    fn r1547_an_unmarked_section_is_still_named_by_its_painted_label() {
        let asks = Cell::new(0);
        let scene = run_vtable_header_mark(1, "Primary key", false, &asks);
        let unmarked = find_tagged(&scene, &GridTag::col_header("vtbl", 0)).expect("header 0");
        assert!(
            unmarked.aria_label.is_none(),
            "an unmarked header carries NO override — the derivation must \
             reach its painted text, which is what names it",
        );
        assert_eq!(
            cell_access_name(&scene, &GridTag::col_header("vtbl", 0)).as_deref(),
            Some("Index"),
            "so it is named by the label the model answered with",
        );
    }

    // ── R1548 §5.27 — a row is asked for its header ─────────────────

    /// R1548 — how many rows the vertical band shows at `VT_MEASURED_H`.
    ///
    /// Stated as the arithmetic rather than a literal, so the fixture and the
    /// assertions cannot drift apart when the pitch or the overscan changes.
    fn vt_row_window() -> VisibleWindow {
        compute_visible_range(0, VT_MEASURED_H, VT_N, TableStyle::m3().row_height, 2)
    }

    /// R1548 — the measured body height every row-header fixture uses.
    const VT_MEASURED_H: u32 = 200;

    /// R1548 — render the virtual table with a **vertical header axis**,
    /// counting how many times each of its two roles was asked.
    ///
    /// `marked` is the data row whose header carries a mark; `meaning` is what
    /// that mark says (empty = the decorative arm). `order` is the R778 sort
    /// permutation, so one fixture serves the unpermuted and the sorted case —
    /// which is the whole question on this axis, because a row header names a
    /// row's identity and not its place on screen.
    fn run_vtable_row_headers(
        marked: usize,
        meaning: &str,
        order: Option<&[usize]>,
        asks: &(Cell<usize>, Cell<usize>),
    ) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, VT_MEASURED_H);
        let pitch = i32::try_from(TableStyle::m3().row_height).unwrap();
        state.set_max(0, i32::try_from(VT_N).unwrap() * pitch);
        let h_state = vt_hscroll(0);
        let theme = light();
        let style = TableStyle::m3();
        let meaning = meaning.to_string();
        let label = |row: usize| {
            asks.0.set(asks.0.get() + 1);
            format!("R{row}")
        };
        let decoration = |row: usize| {
            asks.1.set(asks.1.get() + 1);
            (row == marked).then(|| Decoration::Swatch {
                color: Color::rgb(0, 9, 0),
                meaning: meaning.clone(),
            })
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
                    order,
                    col_widths: None,
                    resizable: false,
                    frozen_cols: 0,
                    row_style: None,
                    delegate: None,
                    editing: None,
                },
                &theme,
                &style,
                &|_| false,
                GridModel {
                    cell: vt_cell,
                    columns: HeaderAxis::labelled(vt_header),
                    rows: Some(RowHeaderAxis::inert(HeaderAxis { label, decoration })),
                    decoration: no_decoration,
                    edit: no_edit,
                },
            )
        })
    }

    /// R1562 — the band with a stated corner and a stated selection, the two
    /// inputs the sections and the corner are derived from.
    fn run_vtable_band(corner: CornerAction, selected: &dyn Fn(usize) -> bool) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, VT_MEASURED_H);
        let pitch = i32::try_from(TableStyle::m3().row_height).unwrap();
        state.set_max(0, i32::try_from(VT_N).unwrap() * pitch);
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
                    editing: None,
                },
                &theme,
                &style,
                &selected,
                GridModel {
                    cell: vt_cell,
                    columns: HeaderAxis::labelled(vt_header),
                    rows: Some(RowHeaderAxis {
                        sections: HeaderAxis::row_numbers(),
                        corner,
                    }),
                    decoration: no_decoration,
                    edit: no_edit,
                },
            )
        })
    }

    /// R1562 — every windowed section carries the press address that routes to
    /// the selection coordinator, and the address encodes the **data** row.
    #[test]
    fn r1562_every_section_carries_its_press_address() {
        let scene = run_vtable_band(CornerAction::Inert, &|_| false);
        for row in vt_row_window().indices() {
            let key = format!("vtbl#{}", GridSendKey::RowHeader { row }.encode());
            assert!(
                find_tagged(&scene, &key).is_some(),
                "section {row} is pressable at {key}",
            );
        }
        // Windowed: a row that is not painted has no address, so the band
        // cannot be pressed into a row it never drew.
        let key = format!("vtbl#{}", GridSendKey::RowHeader { row: VT_N - 1 }.encode());
        assert!(find_tagged(&scene, &key).is_none());
    }

    /// R1562 — the section's fill is DERIVED from the row predicate: the same
    /// answer the strip beside it is filled from. Qt makes this a view flag
    /// (`QHeaderView::highlightSections`) that defaults to **false** and can
    /// therefore disagree with the rows.
    #[test]
    fn r1562_a_selected_rows_section_is_washed_with_it() {
        let window = vt_row_window();
        let chosen = window.first + 1;
        let scene = run_vtable_band(CornerAction::Inert, &|row| row == chosen);
        let fill = |row: usize| {
            find_tagged(&scene, &GridTag::row_header("vtbl", row))
                .expect("the section is painted")
                .style
                .fill
        };
        let other = window.first;
        assert_ne!(
            fill(chosen),
            fill(other),
            "the selected row's section is not filled like an unselected one",
        );
        assert_eq!(
            fill(chosen),
            row_fill(&light(), RadioState::Idle, true, chosen),
            "and it is filled by the SAME derivation the row strip uses",
        );
    }

    /// R1562 — an `Inert` corner is the pre-R1562 blank block: no mark, and
    /// nothing to press. The negative control for the tri-state below.
    #[test]
    fn r1562_an_inert_corner_has_nothing_to_press() {
        let scene = run_vtable_band(CornerAction::Inert, &|_| false);
        assert!(
            find_tagged(&scene, &GridTag::header_corner("vtbl")).is_some(),
            "the corner still closes the two bands",
        );
        assert!(
            find_tagged(&scene, "vtbl#c").is_none(),
            "but `setCornerButtonEnabled(false)` gives it no address",
        );
    }

    /// R1562 — the corner's three marks. Empty draws none, which is how an
    /// unchecked checkbox is painted; the other two are distinct glyphs, so
    /// "partial" and "all" cannot be read off the same pixels.
    #[test]
    fn r1562_the_corner_paints_the_extent_it_was_given() {
        let mark = |extent| {
            let scene = run_vtable_band(CornerAction::SelectAll(extent), &|_| false);
            let node = find_tagged(&scene, "vtbl#c").expect("the corner is pressable");
            let mut out = Vec::new();
            for child in &node.children {
                collect_text(child, &mut out);
            }
            out
        };
        assert!(mark(CornerExtent::Empty).is_empty(), "no mark is unchecked");
        assert_eq!(
            mark(CornerExtent::Partial),
            vec![crate::glyph::SELECT_ALL_PARTIAL]
        );
        assert_eq!(
            mark(CornerExtent::All),
            vec![crate::glyph::SELECT_ALL_COMPLETE]
        );
    }

    /// The seam: the band exists, one section per **painted** row, and the
    /// corner where the two axes meet is there to align them.
    #[test]
    fn r1548_a_row_header_is_painted_for_every_windowed_row() {
        let asks = (Cell::new(0), Cell::new(0));
        let scene = run_vtable_row_headers(usize::MAX, "", None, &asks);
        let window = vt_row_window();
        assert!(window.count > 0 && window.count < VT_N, "a real window");
        for row in window.indices() {
            assert!(
                find_tagged(&scene, &GridTag::row_header("vtbl", row)).is_some(),
                "row {row} is painted, so its header section is painted",
            );
        }
        assert!(
            find_tagged(&scene, &GridTag::row_header("vtbl", VT_N - 1)).is_none(),
            "and a row outside the window has no header section — the band is \
             windowed by the row window, exactly as the strip beside it is",
        );
        assert!(
            find_tagged(&scene, &GridTag::header_corner("vtbl")).is_some(),
            "the corner cell aligns the two bands",
        );
    }

    /// The cost claim: the vertical axis is asked once per painted row, not
    /// once per row in the table — and **both** its roles are, because a role
    /// that quietly stopped being windowed would be the regression.
    #[test]
    fn r1548_the_vertical_axis_is_asked_once_per_painted_row() {
        let asks = (Cell::new(0), Cell::new(0));
        run_vtable_row_headers(usize::MAX, "", None, &asks);
        let painted = vt_row_window().count;
        assert_eq!(
            asks.0.get(),
            painted,
            "one `Qt::DisplayRole` ask per painted row (table is {VT_N} rows)",
        );
        assert_eq!(
            asks.1.get(),
            painted,
            "and one `Qt::DecorationRole` ask — an equality, not a bound: \
             'asks for what it paints' is what the contract says",
        );
    }

    /// **The statement Qt cannot make.** An axis the model does not answer is
    /// not painted blank — it is not painted, and not asked.
    ///
    /// In Qt the two cases are indistinguishable at every observation point: a
    /// model that falls through its `orientation` switch returns an invalid
    /// `QVariant`, `QHeaderView` paints sections that still occupy their width,
    /// and nothing reports it.
    #[test]
    fn r1548_an_unanswered_axis_paints_no_band_at_all() {
        let scene = run_vtable(VT_MEASURED_H, 0);
        assert!(
            find_tagged(&scene, &GridTag::header_corner("vtbl")).is_none(),
            "no corner",
        );
        for row in vt_row_window().indices() {
            assert!(
                find_tagged(&scene, &GridTag::row_header("vtbl", row)).is_none(),
                "and no section for painted row {row} — `no_row_header()` is a \
                 statement, not a band of empty cells",
            );
        }
    }

    /// The role reaches assistive technology on **this** axis: a mark that
    /// carries meaning joins the section's name, ahead of its label.
    #[test]
    fn r1548_a_meaningful_row_mark_is_announced() {
        let asks = (Cell::new(0), Cell::new(0));
        let marked = vt_row_window().first + 1;
        let scene = run_vtable_row_headers(marked, "Pinned", None, &asks);
        assert_eq!(
            cell_access_name(&scene, &GridTag::row_header("vtbl", marked)).as_deref(),
            Some(format!("Pinned R{marked}").as_str()),
            "the mark's meaning precedes the row's label — Qt answers a header \
             cell's name from the display role alone, so this fact would be \
             unhearable there",
        );
    }

    /// And the negative half: a decorative mark (`alt=""`) leaves the name to
    /// the painted label, so the AT tree is what an unmarked row's would be.
    #[test]
    fn r1548_a_decorative_row_mark_is_silent() {
        let asks = (Cell::new(0), Cell::new(0));
        let marked = vt_row_window().first + 1;
        let scene = run_vtable_row_headers(marked, "", None, &asks);
        let cell = find_tagged(&scene, &GridTag::row_header("vtbl", marked)).expect("section");
        assert!(cell.aria_label.is_none(), "no override");
        assert_eq!(
            cell_access_name(&scene, &GridTag::row_header("vtbl", marked)).as_deref(),
            Some(format!("R{marked}").as_str()),
            "so the derivation names it from the label that was drawn",
        );
    }

    /// A section's mark is addressable on this axis too, and its address
    /// cannot be read as the other axis's.
    #[test]
    fn r1548_a_row_mark_has_its_own_address() {
        let asks = (Cell::new(0), Cell::new(0));
        let marked = vt_row_window().first + 1;
        let scene = run_vtable_row_headers(marked, "Pinned", None, &asks);
        assert!(
            find_tagged(&scene, &GridTag::row_header_decoration("vtbl", marked)).is_some(),
            "the mark is tagged, so it can be asked about",
        );
        assert!(
            find_tagged(&scene, &GridTag::header_decoration("vtbl", marked)).is_none(),
            "and the COLUMN axis's mark address is not it",
        );
    }

    /// **The identity claim.** The vertical axis is asked with the row's data
    /// index, not its position on screen, so a sort carries each header with
    /// its row instead of renumbering the viewport.
    #[test]
    fn r1548_a_row_header_names_its_row_not_its_position() {
        let asks = (Cell::new(0), Cell::new(0));
        // Reverse the first rows: visual position 0 is now data row 3.
        let order: Vec<usize> = (0..VT_N).rev().collect();
        let scene = run_vtable_row_headers(usize::MAX, "", Some(&order), &asks);
        let top = VT_N - 1;
        assert_eq!(
            cell_access_name(&scene, &GridTag::row_header("vtbl", top)).as_deref(),
            Some(format!("R{top}").as_str()),
            "the topmost band section is data row {top}'s, named as {top} — \
             not as the first row of the view",
        );
        assert!(
            find_tagged(&scene, &GridTag::data_row("vtbl", top)).is_some(),
            "and it is the row whose strip sits beside it",
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
                    editing: None,
                },
                &theme,
                &style,
                &|_| false,
                GridModel {
                    cell: vt_cell,
                    columns: HeaderAxis::labelled(vt_header),
                    rows: no_row_header(),
                    decoration: no_decoration,
                    edit: no_edit,
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
                    editing: None,
                },
                &theme,
                &style,
                &|_| false,
                GridModel {
                    cell: vt_cell,
                    columns: HeaderAxis::labelled(vt_header),
                    rows: no_row_header(),
                    decoration: no_decoration,
                    edit: no_edit,
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
                    editing: None,
                },
                &light(),
                &TableStyle::m3(),
                &|_| false,
                GridModel {
                    cell: vt_marker_cell,
                    columns: HeaderAxis::labelled(vt_header),
                    rows: no_row_header(),
                    decoration: no_decoration,
                    edit: no_edit,
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
                    editing: None,
                },
                &light(),
                &TableStyle::m3(),
                &|_| false,
                GridModel {
                    cell: vt_marker_cell,
                    columns: HeaderAxis::labelled(vt_header),
                    rows: no_row_header(),
                    decoration: no_decoration,
                    edit: no_edit,
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
                    editing: None,
                },
                &theme,
                &style,
                &|_| false,
                GridModel {
                    cell: vt_cell,
                    columns: HeaderAxis::labelled(vt_header),
                    rows: no_row_header(),
                    decoration: no_decoration,
                    edit: no_edit,
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
                columns: HeaderAxis::labelled(wide_header),
                rows: no_row_header(),
                decoration: no_decoration,
                edit: no_edit,
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
            impl FnMut(usize) -> Option<Decoration>,
            impl FnMut(usize) -> String,
            impl FnMut(usize) -> Option<Decoration>,
            impl FnMut(CellIndex) -> Option<Decoration>,
            impl FnMut(CellIndex) -> Option<CellEdit>,
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
                    editing: None,
                },
                &light(),
                &TableStyle::m3(),
                &|_| false,
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
                columns: HeaderAxis::labelled(wide_header),
                rows: no_row_header(),
                decoration: no_decoration,
                edit: no_edit,
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
                columns: HeaderAxis::labelled(|col: usize| {
                    log.borrow_mut().push(col);
                    wide_header(col)
                }),
                rows: no_row_header(),
                decoration: no_decoration,
                edit: no_edit,
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
                columns: HeaderAxis::labelled(|_: usize| String::new()),
                rows: no_row_header(),
                decoration: no_decoration,
                edit: no_edit,
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
