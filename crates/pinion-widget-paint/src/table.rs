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
//!   [`InputRouter`](pinion_runtime::InputRouter) `'#'`-split routes a
//!   click on cell `(r, c)` to the table's `"<r>_<c>:<EventName>"` send).

use std::rc::Rc;

use pinion_core::composite_tag::GridSendKey;
use pinion_core::scene::{ContainerNode, Rect, ScrollAxis, ScrollNode, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::virtual_list::{compute_visible_range, content_height};
use pinion_core::Scene;

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
}

/// R786 §5.27 — visible divider line width inside the resize grabber, in
/// logical pixels (an M3 hairline). The grabber is a [`TableStyle::resize_handle_w`]
/// transparent hit zone with this thin [`ColorRole::Outline`] line hugging its
/// right edge — the painted column boundary the user grabs.
const RESIZE_DIVIDER_W: u32 = 1;

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
        }
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
#[derive(Clone, Copy, Debug)]
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

/// One cell: a left-aligned label inside a fixed-size box. `tag` is the
/// composite hit-test tag (`"<root>#<row>_<col>"`) for a data cell, or a
/// presentational header tag (`"<root>_ch<col>"`). The keyboard-focus
/// ring is the shell's job (R694 `paint_focus_ring` over the
/// active-descendant cell), so no focus state is threaded here.
fn cell(
    tag: &str,
    text: &str,
    fg: Color,
    size_px: u32,
    style: &TableStyle,
    width: u32,
    height: u32,
) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(
            TextNode::styled(
                text.to_string(),
                Rect::default(),
                TextStyle::new().with_size_px(size_px).with_fg(fg),
            )
            .with_role(TextRole::Presentational),
        )])
        .with_tag(tag.to_string())
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Start)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(width, height))
                .with_padding(Rect::new(style.cell_pad_x, 0, style.cell_pad_x, 0)),
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
        .map(|c| col_widths.and_then(|w| w.get(c)).copied().unwrap_or(style.col_width))
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
            .with_layout(LayoutStyle::new().with_size(Size::px(RESIZE_DIVIDER_W, style.header_height))),
    );
    Scene::Container(
        ContainerNode::new(vec![divider])
            .with_tag(format!("{tag}_ch{col}#resize"))
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

/// R730 §5.50 — sort-direction indicator glyphs shown on the active sort
/// column's header (U+25B2 BLACK UP-POINTING TRIANGLE / U+25BC down).
/// Named consts + escapes per the non-ASCII-source rule.
const ASC_GLYPH: &str = "\u{25B2}";
const DESC_GLYPH: &str = "\u{25BC}";

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
    let ColCell { col, width, resizable } = cell;
    let fg = theme.resolve(ColorRole::OnSurface);
    // R786 — reserve the trailing grabber width from the clickable / label area
    // so the cell's total width stays `width` (the data cells' width). A
    // non-resizable header is byte-identical to the R785 layout (full `width`).
    let content_w = if resizable { width.saturating_sub(style.resize_handle_w) } else { width };
    let label_node = Scene::Text(
        TextNode::styled(
            label.to_string(),
            Rect::default(),
            TextStyle::new().with_size_px(style.header_size_px).with_fg(fg),
        )
        .with_role(TextRole::Presentational),
    );
    let mut inner_children = vec![label_node];
    if let Some((c, ascending)) = sort {
        if c == col {
            let glyph = if ascending { ASC_GLYPH } else { DESC_GLYPH };
            inner_children.push(Scene::Text(
                TextNode::styled(
                    glyph.to_string(),
                    Rect::default(),
                    TextStyle::new().with_size_px(style.header_size_px).with_fg(fg),
                )
                .with_role(TextRole::Presentational),
            ));
        }
    }
    let inner = Scene::Container(
        ContainerNode::new(inner_children)
            // R777.1 — the clickable header sub-key via the GridSendKey SSOT.
            // R778 — the click routes to `click_tag` (the sort anchor),
            // which may differ from the presentational `tag` (the a11y /
            // paint-root anchor) when sort is a separate coordinator.
            .with_tag(format!("{click_tag}#{}", GridSendKey::Header { col }.encode()))
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
        cell_children.push(resize_handle(tag, col, theme.resolve(ColorRole::Outline), style));
    }
    Scene::Container(
        ContainerNode::new(cell_children)
            .with_tag(format!("{tag}_ch{col}"))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    )
}

/// R786 §5.27 — the header's per-column layout: the resolved widths plus
/// whether each column carries a resize grabber. Bundled so [`header_row`] stays
/// under the argument budget (the [`ColCell`] / [`VirtualTableData`] precedent).
#[derive(Clone, Copy)]
struct ColumnLayout<'a> {
    widths: &'a [u32],
    resizable: bool,
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
        .map(|(col, label)| {
            let width = layout.widths.get(col).copied().unwrap_or(style.col_width);
            header_cell(
                tag,
                click_tag,
                ColCell { col, width, resizable: layout.resizable },
                label,
                sort,
                theme,
                style,
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(cells)
            // Tagged `"<tag>_hrow"` so the binding's `access_node` walker
            // can attach the header `row` node (and resolve its bounds);
            // not a composite `'#'` tag, so it is inert to hit routing.
            .with_tag(format!("{tag}_hrow"))
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)))
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
    widths: &[u32],
    style: &TableStyle,
) -> Scene {
    let cols = widths.len();
    // R730 §5.40 — cells / strip are tagged by **data-row id** (not the
    // visual position), so a click on a sorted row routes to the right
    // data row's `"<data_id>_<col>"` send wire and the table's
    // data-indexed selection stays correct without a remap. When unsorted
    // (`data_id == visual`) the tags are identical to the R707 scheme.
    let cells: Vec<Scene> = (0..cols)
        .map(|col| {
            let text = cells_text.get(col).copied().unwrap_or("");
            cell(
                // R777.1 — the cell sub-key via the GridSendKey SSOT.
                &format!("{tag}#{}", GridSendKey::Cell { row: data_id, col }.encode()),
                text,
                fg,
                style.label_size_px,
                style,
                widths.get(col).copied().unwrap_or(style.col_width),
                style.row_height,
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(cells)
            .with_tag(format!("{tag}_row{data_id}"))
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
/// - `row_selected` — per-row selection bitmap, indexed by **data** row
///   (parallel to `row_states`). A `true` row strip is washed with the
///   accent tint; a row outside the slice defaults to unselected. One
///   path serves both single-select (one bit set) and R735 multi-select.
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
    row_selected: &[bool],
    row_states: &[RadioState],
    sort: Option<(usize, bool)>,
    theme: &Theme,
    style: &TableStyle,
) -> Scene {
    let cols = data.headers.len();
    // The eager table is one combined coordinator (header + cells route to
    // the same `TableExternal`), so the clickable header anchor is `tag`.
    // Eager tables are uniform-width (no per-column model), so every column
    // resolves to `style.col_width`.
    let widths = resolve_widths(cols, None, style);
    // The eager table is uniform-width and not user-resizable (no width model);
    // the header keeps its full-width R707 layout.
    let header = header_row(
        tag,
        tag,
        data.headers,
        sort,
        ColumnLayout { widths: &widths, resizable: false },
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
        children.push(data_row(tag, data_id, cells_text, fill, fg, &widths, style));
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
#[derive(Clone, Copy, Debug)]
pub struct VirtualTableData<'a> {
    /// Column header labels; `headers.len()` is the column count.
    pub headers: &'a [&'a str],
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
    /// so the grabber's capture drives the shared [`ColumnWidths`] model;
    /// dragging a border widens the column, growing the R784 horizontal scroll.
    /// `false` keeps the full-width R785 header (no grabber, no width reserved).
    pub resizable: bool,
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
/// [`data_row`]. The shared windowed-sizer + flex-`ScrollNode` shape is
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
/// - `data` — the [`VirtualTableData`] (column headers + total row count +
///   overscan).
/// - `theme` / `style` — palette + [`TableStyle`] dimensions.
/// - `is_selected` — a predicate over the **data-row** index: a `true` row's
///   strip is washed with the accent tint (the same `row_fill` selection
///   path the eager [`view_table`] uses). This is the data-indexed
///   generalization of a single selected index — single-select passes
///   `|id| selected == Some(id)`, R782 multi-select passes set membership,
///   and a display-only grid passes `|_| false`. One virtualized-grid paint
///   path serves all three (no parallel `_multi` body to diverge from the
///   windowed-sizer geometry). It is invoked only for the windowed rows.
/// - `build_cells` — invoked once per windowed data-row index; returns the
///   row's cell texts (a cell beyond the returned length renders blank).
#[must_use]
pub fn view_virtual_table(
    tag: &str,
    scroll: GridScroll<'_>,
    data: VirtualTableData<'_>,
    theme: &Theme,
    style: &TableStyle,
    is_selected: impl Fn(usize) -> bool,
    mut build_cells: impl FnMut(usize) -> Vec<String>,
) -> Scene {
    let cols = data.headers.len();
    // R785 — resolve the per-column widths once (uniform `style.col_width`
    // fallback when no width model is wired). Content width is their sum, so
    // widening a column grows the horizontal scroll extent (R784).
    let widths = resolve_widths(cols, data.col_widths, style);
    let total_w: u32 = widths.iter().copied().sum();
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

    // The uniform-pitch slot geometry (`top = view_pos · row_height`, framed
    // `total_w × row_height`) is the same windowed-sizer shape the list
    // bodies use — built via the shared `uniform_slots` (R775.1) so the
    // grid and the lists cannot disagree on slot placement. Only the row
    // *content* diverges (a multi-cell `data_row`).
    let slots = uniform_slots(&window, total_w, style.row_height, |view_pos| {
        // R778 — visual position → source data row (identity when unsorted).
        let source = data.order.map_or(view_pos, |o| o.get(view_pos).copied().unwrap_or(view_pos));
        let cells_text = build_cells(source);
        let cell_refs: Vec<&str> = cells_text.iter().map(String::as_str).collect();
        // Rows are idle (no per-row hover/press at the grid level); a
        // selected row gets the accent tint — selection is **data-indexed**
        // (`is_selected(source)`), so it survives a re-sort, and several
        // rows may be selected at once in multi-select. Zebra parity is
        // **visual** (`view_pos`), so the stripe pattern stays stable across
        // re-sorts (the eager `view_table` convention); cells / strip tag by
        // **source** id so a click on a sorted row routes to the right data
        // row.
        let fill = row_fill(theme, RadioState::Idle, is_selected(source), view_pos);
        let fg = row_fg(theme, RadioState::Idle);
        data_row(tag, source, &cell_refs, fill, fg, &widths, style)
    });
    let body = assemble_windowed_flex(scroll.body, total_w, total_h, slots);
    // R778 — clickable headers route to the sort anchor (`sort_tag`) when a
    // sort coordinator is wired, else stay on `tag` (R777 default).
    let header = header_row(
        tag,
        data.sort_tag.unwrap_or(tag),
        data.headers,
        data.sort,
        ColumnLayout { widths: &widths, resizable: data.resizable },
        theme,
        style,
    );

    // R784 — the scrolled column: the frozen header above the inner
    // vertical body scroll. This whole `total_w`-wide column is what the
    // outer horizontal scroll slides; the header is *between* the two
    // scrolls, so it moves horizontally with the body but never
    // vertically. No surface fill / padding here — the frame below owns
    // those so the rounded block stays put while the content scrolls.
    let column = Scene::Container(
        ContainerNode::new(vec![header, body])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    );
    // R784 — the outer horizontal scroll. `from_state` derives its tag /
    // offset from `h_scroll`; `with_axis(Horizontal)` makes the layout
    // pass unbound the width so the `total_w` column overflows when it
    // exceeds the viewport, and `flex_grow` claims the frame's interior.
    let h_scrolled = Scene::Scroll(
        ScrollNode::from_state(Rc::clone(scroll.horizontal), Rect::default(), column)
            .with_axis(ScrollAxis::Horizontal)
            .with_layout(LayoutStyle::new().with_flex_grow(1.0)),
    );

    Scene::Container(
        ContainerNode::new(vec![h_scrolled])
            .with_tag(tag.to_string())
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow))
                    .with_corner_radius(style.corner_radius),
            )
            // flex_grow so the grid fills its parent (the AutoSizer
            // contract); the outer horizontal ScrollNode then claims the
            // framed interior and the body scroll claims the height left
            // after the fixed header band.
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_flex_grow(1.0)
                    .with_padding(Rect::new(
                        style.block_pad,
                        style.block_pad,
                        style.block_pad,
                        style.block_pad,
                    )),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

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
                TableData { headers: &headers, rows: &rows, row_ids: &[] },
                &[],
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
                TableData { headers: &headers, rows: &rows, row_ids: &[] },
                &[],
                &all_idle(),
                None,
                &theme,
                &TableStyle::m3(),
            )
        });
        let mut t = Vec::new();
        collect_text(&unsorted, &mut t);
        assert!(!t.iter().any(|s| s == ASC_GLYPH || s == DESC_GLYPH), "no glyph when unsorted");

        let sorted = Owner::new().run(|| {
            view_table(
                "table",
                TableData { headers: &headers, rows: &rows, row_ids: &[] },
                &[],
                &all_idle(),
                Some((1, true)),
                &theme,
                &TableStyle::m3(),
            )
        });
        let mut t2 = Vec::new();
        collect_text(&sorted, &mut t2);
        assert_eq!(t2.iter().filter(|s| *s == ASC_GLYPH).count(), 1, "one ascending glyph");
        assert!(!t2.iter().any(|s| s == DESC_GLYPH), "no descending glyph for ascending sort");
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
                TableData { headers: &headers, rows: &reordered, row_ids: &[2, 0, 1] },
                &[],
                &all_idle(),
                Some((0, true)),
                &light(),
                &TableStyle::m3(),
            )
        });
        let Scene::Container(root) = &scene else { panic!("root container") };
        // children[0] = header band; children[1..] = data rows in visual order.
        let strip_tag = |i: usize| {
            let Scene::Container(c) = &root.children[i] else { panic!("row strip") };
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

    fn run_vtable(measured_h: u32, offset_y: i32) -> Scene {
        run_vtable_h(measured_h, offset_y, 0)
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
        let h_state = Rc::new(ScrollState::with_tag("vtbl_h"));
        // Three 120-wide columns = 360 content; let the outer scroll have
        // room to move so a non-zero offset_x actually applies.
        h_state.set_max(1000, 0);
        h_state.scroll_to(offset_x, 0);
        let theme = light();
        let style = TableStyle::m3();
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll { body: &state, horizontal: &h_state },
                VirtualTableData {
                    headers: &VT_HEADERS,
                    item_count: VT_N,
                    overscan: 2,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: None,
                    resizable: false,
                },
                &theme,
                &style,
                |_| false,
                |id| vec![format!("{id}"), format!("Row {id}"), format!("v{id}")],
            )
        })
    }

    /// Count `vtbl_row<id>` data-row strips anywhere in the scene.
    fn count_vt_rows(scene: &Scene) -> usize {
        fn walk(scene: &Scene, n: &mut usize) {
            match scene {
                Scene::Container(c) => {
                    if c.tag.as_deref().is_some_and(|t| t.starts_with("vtbl_row")) {
                        *n += 1;
                    }
                    for child in &c.children {
                        walk(child, n);
                    }
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), n),
                _ => {}
            }
        }
        let mut n = 0;
        walk(scene, &mut n);
        n
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
        assert!(tall > short, "taller viewport => more body rows: {tall} vs {short}");
        assert!(tall < 40, "still a window: {tall}");
    }

    #[test]
    fn r784_virtual_table_header_frozen_above_body_inside_h_scroll() {
        // R784 — the grid is `frame > h-scroll > column[header, body]`.
        // The header sits ABOVE the inner vertical body scroll but INSIDE
        // the outer horizontal scroll, so it is pinned vertically yet
        // tracks the body's horizontal offset.
        let scene = run_vtable(360, 0);
        let Scene::Container(frame) = &scene else { panic!("root frame is a Container") };
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
        assert!(header_first, "frozen header band is the column's first child");
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
        assert_eq!(h.axis, ScrollAxis::Horizontal, "outer scroll axis is horizontal");
        assert!(
            (h.layout.flex_grow - 1.0).abs() < f32::EPSILON,
            "outer horizontal ScrollNode flex-grows to fill the framed interior",
        );
        assert!(h.state.is_some(), "outer scroll carries its ScrollState Rc");
        assert_eq!(h.tag.as_deref(), Some("vtbl_h"), "derived from the h_scroll tag");
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
        let h_state = Rc::new(ScrollState::with_tag("vtbl_h"));
        let widths = [200u32, 50, 300];
        let scene = Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll { body: &state, horizontal: &h_state },
                VirtualTableData {
                    headers: &VT_HEADERS,
                    item_count: 5,
                    overscan: 1,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: Some(&widths),
                    resizable: false,
                },
                &light(),
                &TableStyle::m3(),
                |_| false,
                |id| vec![format!("{id}"), "x".to_string(), "y".to_string()],
            )
        });
        assert_eq!(cell_layout_width(&scene, "vtbl#0_0"), Some(200), "col 0 cell width");
        assert_eq!(cell_layout_width(&scene, "vtbl#0_1"), Some(50), "col 1 cell width");
        assert_eq!(cell_layout_width(&scene, "vtbl#0_2"), Some(300), "col 2 cell width");
    }

    #[test]
    fn r785_no_col_widths_falls_back_to_uniform() {
        // R785 — `col_widths: None` keeps the uniform `style.col_width` (the
        // pre-R785 behaviour every existing grid relies on).
        let scene = run_vtable(360, 0);
        let uniform = TableStyle::m3().col_width;
        assert_eq!(cell_layout_width(&scene, "vtbl#0_0"), Some(uniform), "uniform fallback");
        assert_eq!(cell_layout_width(&scene, "vtbl#0_1"), Some(uniform), "uniform fallback");
    }

    fn run_vtable_resizable(widths: &[u32], resizable: bool) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, 360);
        let h_state = Rc::new(ScrollState::with_tag("vtbl_h"));
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll { body: &state, horizontal: &h_state },
                VirtualTableData {
                    headers: &VT_HEADERS,
                    item_count: 5,
                    overscan: 1,
                    sort: None,
                    sort_tag: None,
                    order: None,
                    col_widths: Some(widths),
                    resizable,
                },
                &light(),
                &TableStyle::m3(),
                |_| false,
                |id| vec![format!("{id}"), "x".to_string(), "y".to_string()],
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
        assert_eq!(cell_layout_width(&scene, "vtbl#h0"), Some(200 - handle_w), "col 0 content");
        assert_eq!(cell_layout_width(&scene, "vtbl#h1"), Some(50 - handle_w), "col 1 content");
        // Data cells keep the full per-column width (no grabber).
        assert_eq!(cell_layout_width(&scene, "vtbl#0_0"), Some(200), "data cell full width");
        assert_eq!(cell_layout_width(&scene, "vtbl#0_2"), Some(300), "data cell full width");
    }

    #[test]
    fn r786_non_resizable_header_has_no_grabber() {
        // R786 — `resizable: false` is byte-identical to the R785 header: no
        // grabber tag, and the content area is the full column width.
        let scene = run_vtable_resizable(&[200, 50, 300], false);
        assert!(!scene.contains_tag("vtbl_ch0#resize"), "no grabber when not resizable");
        assert_eq!(cell_layout_width(&scene, "vtbl#h0"), Some(200), "full-width content area");
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
        state.set_max(0, i32::try_from(order.len()).unwrap() * i32::try_from(pitch).unwrap());
        let h_state = Rc::new(ScrollState::with_tag("vtbl_h"));
        let theme = light();
        let style = TableStyle::m3();
        Owner::new().run(|| {
            view_virtual_table(
                "vtbl",
                GridScroll { body: &state, horizontal: &h_state },
                VirtualTableData {
                    headers: &VT_HEADERS,
                    item_count: order.len(),
                    overscan: 2,
                    sort,
                    sort_tag: Some("vsort"),
                    order: Some(order),
                    col_widths: None,
                    resizable: false,
                },
                &theme,
                &style,
                |_| false,
                |id| vec![format!("{id}"), format!("Row {id}"), format!("v{id}")],
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
        assert_eq!(text.iter().filter(|s| *s == DESC_GLYPH).count(), 1, "one descending glyph");
        assert!(!text.iter().any(|s| s == ASC_GLYPH), "no ascending glyph for a descending sort");
    }

    #[test]
    fn r778_display_grid_keeps_headers_on_the_grid_anchor() {
        // sort_tag = None (the R777 default) keeps headers on the paint root,
        // where the selection coordinator harmlessly ignores `h<col>`.
        let scene = run_vtable(360, 0);
        assert!(scene.contains_tag("vtbl#h0"), "display grid header stays on the grid anchor");
    }
}
