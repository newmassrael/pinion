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

use pinion_core::composite_tag::{GridSendKey, GridTag};
use pinion_core::scene::{ContainerNode, Rect, ScrollAxis, ScrollNode, TextNode, TextRole};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::virtual_list::{compute_visible_range, content_height, VisibleWindow};
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
    let y = style.block_pad
        + style.header_height
        + u32::try_from(r0).unwrap_or(0) * style.row_height;
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
                TextStyle::new().with_size_px(style.header_size_px).with_fg(fg),
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
            .with_tag(GridTag::col_header(tag, col))
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
            // Tagged `"<tag>_hrow"` (or `"<tag>_fhrow"` for the R859
            // frozen pane) so the binding's `access_node` walker can
            // attach the header `row` node (and resolve its bounds); not a
            // composite `'#'` tag, so it is inert to hit routing.
            .with_tag(layout.container_tag.to_string())
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
            let text = cells_text.get(j).copied().unwrap_or("");
            cell(
                // R777.1 — the cell sub-key via the GridSendKey SSOT.
                &format!("{tag}#{}", GridSendKey::Cell { row: data_id, col }.encode()),
                text,
                fg,
                style.label_size_px,
                style,
                pane.widths.get(j).copied().unwrap_or(style.col_width),
                style.row_height,
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(cells)
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
        ColumnLayout { widths: &widths, resizable: false, col_base: 0, container_tag: &hrow_tag },
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
        children.push(data_row(
            tag,
            data_id,
            cells_text,
            fill,
            fg,
            RowPane { container_tag: &row_tag, col_base: 0, widths: &widths },
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
}

impl core::fmt::Debug for VirtualTableData<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VirtualTableData")
            .field("headers", &self.headers)
            .field("item_count", &self.item_count)
            .field("overscan", &self.overscan)
            .field("sort", &self.sort)
            .field("sort_tag", &self.sort_tag)
            .field("order", &self.order)
            .field("col_widths", &self.col_widths)
            .field("resizable", &self.resizable)
            .field("frozen_cols", &self.frozen_cols)
            .field("row_style", &self.row_style.map(|_| "<fn>"))
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
    build_cells: impl FnMut(usize) -> Vec<String>,
) -> Scene {
    let cols = data.headers.len();
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
    // `build_cells` / `is_selected` are consumed by exactly one branch (an
    // `if`/`else`), so each helper takes them by value without a re-move.
    let content = if frozen_cols == 0 {
        render.render_unsplit(scroll, &is_selected, build_cells)
    } else {
        render.render_frozen(scroll, frozen_cols, &is_selected, build_cells)
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
/// ([`uniform_slots`] slot-tops + the `total_h` sizer) and the nested
/// single-axis scroll wiring ([`assemble_windowed_flex`] vertical body inside
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
struct GridRender<'a> {
    tag: &'a str,
    click_tag: &'a str,
    data: &'a VirtualTableData<'a>,
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
/// "divergence-is-a-bug"): the read-only unsplit grid ([`render_unsplit`]),
/// the frozen grid's scrolling pane ([`frozen_split_panes`]), and — R896 —
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
/// R859) and the **tree-grid** ([`view_virtual_treegrid`], R860): both feed
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
    let frozen_body = assemble_windowed_flex(scroll.body, frozen.width, total_h, frozen.slots, true);
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
    let scroll_body = assemble_windowed_flex(scroll.body, scrolled.width, total_h, scrolled.slots, false);
    let scroll_pane = h_scrolled_column(scroll.horizontal, scrolled.header, scroll_body);
    Scene::Container(
        ContainerNode::new(vec![frozen_pane, scroll_pane])
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_flex_grow(1.0)),
    )
}

impl GridRender<'_> {
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
    fn row_inputs(&self, view_pos: usize, is_selected: &impl Fn(usize) -> bool) -> (usize, Color, Color) {
        let source = self.source_of(view_pos);
        // R998 — precedence: selection highlight > matched coloring rule >
        // zebra stripe. A selected row keeps the accent fill so the selection
        // stays visible over any rule tint; otherwise a row-style rule (per
        // SOURCE row, so it survives a re-sort) overrides the zebra default.
        if !is_selected(source) {
            if let Some((bg, fg)) = self.data.row_style.and_then(|resolve| resolve(source)) {
                return (source, bg, fg);
            }
        }
        let fill = row_fill(self.theme, RadioState::Idle, is_selected(source), view_pos);
        let fg = row_fg(self.theme, RadioState::Idle);
        (source, fill, fg)
    }

    /// R784 single-scroll grid (the only mode before R859): one
    /// `total_w`-wide `[header, body]` column inside an outer horizontal
    /// scroll. Byte-identical to the pre-R859 assembly.
    fn render_unsplit(
        &self,
        scroll: GridScroll<'_>,
        is_selected: &impl Fn(usize) -> bool,
        mut build_cells: impl FnMut(usize) -> Vec<String>,
    ) -> Scene {
        let total_w: u32 = self.widths.iter().copied().sum();
        let hrow_tag = GridTag::header_row(self.tag);
        // R784 — the frozen header above the inner vertical body scroll, the
        // whole `total_w`-wide column slid by the outer horizontal scroll. No
        // surface fill here — the frame in `view_virtual_table` owns it so the
        // rounded block stays put while the content scrolls.
        let header = header_row(
            self.tag,
            self.click_tag,
            self.data.headers,
            self.data.sort,
            ColumnLayout {
                widths: self.widths,
                resizable: self.data.resizable,
                col_base: 0,
                container_tag: &hrow_tag,
            },
            self.theme,
            self.style,
        );
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
            header,
            |view_pos| {
                let (source, fill, fg) = self.row_inputs(view_pos, is_selected);
                let cells_text = build_cells(source);
                let cell_refs: Vec<&str> = cells_text.iter().map(String::as_str).collect();
                let row_tag = GridTag::data_row(self.tag, source);
                data_row(
                    self.tag,
                    source,
                    &cell_refs,
                    fill,
                    fg,
                    RowPane { container_tag: &row_tag, col_base: 0, widths: self.widths },
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
    /// `build_cells` is invoked once per visible row **per pane** (the
    /// small overscanned window only): two `uniform_slots` passes keep the
    /// slot-top geometry on its R775.1 SSOT instead of hand-rolling a loop.
    fn render_frozen(
        &self,
        scroll: GridScroll<'_>,
        frozen_cols: usize,
        is_selected: &impl Fn(usize) -> bool,
        mut build_cells: impl FnMut(usize) -> Vec<String>,
    ) -> Scene {
        let frozen_w: u32 = self.widths[..frozen_cols].iter().copied().sum();
        let scroll_w: u32 = self.widths[frozen_cols..].iter().copied().sum();
        let row_pitch = self.style.row_height;

        let frozen_slots = uniform_slots(self.window, frozen_w, row_pitch, |view_pos| {
            let (source, fill, fg) = self.row_inputs(view_pos, is_selected);
            let cells_text = build_cells(source);
            let cell_refs: Vec<&str> = cells_text.iter().map(String::as_str).collect();
            let split = frozen_cols.min(cell_refs.len());
            // R859 — distinct `_frow{id}` container tag so the split panes
            // never emit a duplicate strip tag (per-cell `_{col}` tags stay
            // unique by absolute column).
            let frow_tag = GridTag::frozen_data_row(self.tag, source);
            data_row(
                self.tag,
                source,
                &cell_refs[..split],
                fill,
                fg,
                RowPane { container_tag: &frow_tag, col_base: 0, widths: &self.widths[..frozen_cols] },
                self.style,
            )
        });
        let scroll_slots = uniform_slots(self.window, scroll_w, row_pitch, |view_pos| {
            let (source, fill, fg) = self.row_inputs(view_pos, is_selected);
            let cells_text = build_cells(source);
            let cell_refs: Vec<&str> = cells_text.iter().map(String::as_str).collect();
            let split = frozen_cols.min(cell_refs.len());
            let row_tag = GridTag::data_row(self.tag, source);
            data_row(
                self.tag,
                source,
                &cell_refs[split..],
                fill,
                fg,
                RowPane {
                    container_tag: &row_tag,
                    col_base: frozen_cols,
                    widths: &self.widths[frozen_cols..],
                },
                self.style,
            )
        });

        // Frozen pane header (left, columns `0..frozen_cols`). Frozen
        // columns do not horizontally scroll, so a resize grabber (which
        // grows the horizontal extent) is moot — `resizable: false`.
        let fhrow_tag = GridTag::frozen_header_row(self.tag);
        let frozen_header = header_row(
            self.tag,
            self.click_tag,
            &self.data.headers[..frozen_cols],
            self.data.sort,
            ColumnLayout {
                widths: &self.widths[..frozen_cols],
                resizable: false,
                col_base: 0,
                container_tag: &fhrow_tag,
            },
            self.theme,
            self.style,
        );
        // Scrolling pane header (right, columns `frozen_cols..`).
        let hrow_tag = GridTag::header_row(self.tag);
        let scroll_header = header_row(
            self.tag,
            self.click_tag,
            &self.data.headers[frozen_cols..],
            self.data.sort,
            ColumnLayout {
                widths: &self.widths[frozen_cols..],
                resizable: self.data.resizable,
                col_base: frozen_cols,
                container_tag: &hrow_tag,
            },
            self.theme,
            self.style,
        );
        // R859 — the shared frozen-split assembler (also the tree-grid's,
        // R860): frozen pane (follower V-body) beside the scrolling pane
        // (primary V-body in the outer horizontal scroll), vertical lockstep.
        frozen_split_panes(
            scroll,
            self.total_h,
            SplitPane { header: frozen_header, width: frozen_w, slots: frozen_slots },
            SplitPane { header: scroll_header, width: scroll_w, slots: scroll_slots },
        )
    }
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
                TableSelection { rows: &[], cells: None },
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
                TableSelection { rows: &[], cells: None },
                &all_idle(),
                None,
                &theme,
                &TableStyle::m3(),
            )
        });
        let mut t = Vec::new();
        collect_text(&unsorted, &mut t);
        assert!(!t.iter().any(|s| s == crate::glyph::SORT_ASCENDING || s == crate::glyph::SORT_DESCENDING), "no glyph when unsorted");

        let sorted = Owner::new().run(|| {
            view_table(
                "table",
                TableData { headers: &headers, rows: &rows, row_ids: &[] },
                TableSelection { rows: &[], cells: None },
                &all_idle(),
                Some((1, true)),
                &theme,
                &TableStyle::m3(),
            )
        });
        let mut t2 = Vec::new();
        collect_text(&sorted, &mut t2);
        assert_eq!(t2.iter().filter(|s| *s == crate::glyph::SORT_ASCENDING).count(), 1, "one ascending glyph");
        assert!(!t2.iter().any(|s| s == crate::glyph::SORT_DESCENDING), "no descending glyph for ascending sort");
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
                TableSelection { rows: &[], cells: None },
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
                    frozen_cols: 0,
                    row_style: None,                },
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

    /// R859 — render the virtual table with `frozen_cols` leading columns
    /// pinned (and a non-zero horizontal offset so the split is exercised).
    fn run_vtable_frozen(measured_h: u32, offset_x: i32, frozen_cols: usize) -> Scene {
        let state = Rc::new(ScrollState::new());
        state.set_measured_viewport(360, measured_h);
        let pitch = i32::try_from(TableStyle::m3().row_height).unwrap();
        state.set_max(0, i32::try_from(VT_N).unwrap() * pitch);
        let h_state = Rc::new(ScrollState::with_tag("vtbl_h"));
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
                    frozen_cols,
                    row_style: None,                },
                &theme,
                &style,
                |_| false,
                |id| vec![format!("{id}"), format!("Row {id}"), format!("v{id}")],
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
        assert!(scene.contains_tag("vtbl_fhrow"), "frozen header band present");
        assert!(scene.contains_tag("vtbl_hrow"), "scrolled header band present");
        // Frozen header owns column 0; scrolled header owns columns 1 + 2.
        assert!(scene.contains_tag("vtbl_ch0"), "frozen pane carries col 0 header");
        assert!(scene.contains_tag("vtbl_ch1"), "scrolled pane carries col 1 header");
        assert!(scene.contains_tag("vtbl_ch2"), "scrolled pane carries col 2 header");
    }

    #[test]
    fn r859_frozen_cells_keep_absolute_column_send_tags() {
        // The split must not renumber columns: the frozen pane's cell keeps
        // col 0, the scrolled pane's keep cols 1 + 2 (absolute), so a click
        // routes to the right data column across the freeze boundary.
        let scene = run_vtable_frozen(360, 200, 1);
        for col in 0..3 {
            let tag = format!("vtbl#{}", GridSendKey::Cell { row: 0, col }.encode());
            assert!(scene.contains_tag(&tag), "cell {tag} present across the split");
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
        assert_eq!(followers, vec![true, false], "frozen pane follows, scrolled pane leads");
    }

    #[test]
    fn r859_frozen_cols_zero_stays_unsplit_r784_grid() {
        // The default (every pre-R859 caller) renders no frozen pane.
        let scene = run_vtable_frozen(360, 0, 0);
        assert!(scene.contains_tag("vtbl_hrow"), "single header band");
        assert!(!scene.contains_tag("vtbl_fhrow"), "no frozen header band when unsplit");
        let mut followers = Vec::new();
        collect_vertical_followers(&scene, &mut followers);
        assert_eq!(followers, vec![false], "one primary vertical scroll, no follower");
    }

    #[test]
    fn r859_frozen_cols_clamped_to_keep_one_scrollable_column() {
        // frozen_cols >= cols is clamped to cols - 1 (= 2 here): columns 0
        // and 1 freeze, column 2 stays scrollable. Must not panic on the
        // over-range request, and the scrolled pane keeps column 2.
        let scene = run_vtable_frozen(360, 200, 99);
        assert!(scene.contains_tag("vtbl_fhrow"), "frozen pane exists (clamped, not unsplit)");
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
                    frozen_cols: 0,
                    row_style: None,                },
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
                    frozen_cols: 0,
                    row_style: None,                },
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
                    frozen_cols: 0,
                    row_style: None,                },
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
        assert_eq!(text.iter().filter(|s| *s == crate::glyph::SORT_DESCENDING).count(), 1, "one descending glyph");
        assert!(!text.iter().any(|s| s == crate::glyph::SORT_ASCENDING), "no ascending glyph for a descending sort");
    }

    #[test]
    fn r778_display_grid_keeps_headers_on_the_grid_anchor() {
        // sort_tag = None (the R777 default) keeps headers on the paint root,
        // where the selection coordinator harmlessly ignores `h<col>`.
        let scene = run_vtable(360, 0);
        assert!(scene.contains_tag("vtbl#h0"), "display grid header stays on the grid anchor");
    }
}
