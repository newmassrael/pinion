//! AccessKit tree builder for a **WAI-ARIA virtualized `grid`** (R775
//! §5.27, lifted R777): one labelled [`AriaRole::Grid`] container with
//! `aria-setsize = <full row count>` claiming the frozen header row + the
//! windowed data-row tags as children; a header [`AriaRole::Row`] of
//! [`AriaRole::ColumnHeader`] cells; and one data [`AriaRole::Row`] per
//! rendered index carrying its absolute 1-based `aria-posinset` + the same
//! `aria-setsize`, each holding one [`AriaRole::GridCell`] per column.
//!
//! This is the data-grid analogue of
//! [`windowed_list_nodes`](crate::windowed_list_nodes): the *full* extent
//! is conveyed by `aria-setsize`, the *rendered* window by the present
//! `row` / `gridcell` nodes. It is the shared shape behind the
//! display-only `hello-virtual-table` (R775) and the selectable /
//! keyboard-navigable `hello-grid-nav` (R777); a divergence between them
//! would be an a11y bug, not a style choice (the R743.1 / R745
//! "divergence-is-a-bug" rule). The interactive grid is the **second**
//! windowed-grid consumer that triggers the lift, per R758's a11y-axis
//! self-grep mandate (R775 deferred it as 1-consumer bespoke).
//!
//! The tag scheme mirrors
//! [`view_virtual_table`](../../../pinion_widget_paint/table/fn.view_virtual_table.html)
//! exactly so the a11y tree and the painted tree resolve the same bounds:
//! grid `{tag}`, frozen header row `{tag}_hrow`, column header
//! `{tag}_ch{col}`, data row `{tag}_row{id}`, grid cell `{tag}#{id}_{col}`.

use crate::node::AccessNode;
use crate::role::{AriaRole, SortDirection};
use pinion_core::composite_tag::{GridSendKey, GridTag};
use pinion_core::widgets::virtual_list::VisibleWindow;

/// Build the virtualized `grid` container + frozen header row + one data
/// `row` (with its `gridcell`s) per windowed index.
///
/// - `grid_tag` — the paint-root tag; the sub-tags are derived from it
///   (`{grid_tag}_hrow`, `{grid_tag}_ch{col}`, `{grid_tag}_row{id}`,
///   `{grid_tag}#{id}_{col}`).
/// - `grid_name` — the grid's accessible name (no paint-scene equivalent,
///   so it is an explicit override).
/// - `headers` — column header labels; `headers.len()` is the column count.
/// - `set_size` — the *full* data-row count (not the rendered window
///   count) — the value `aria-setsize` conveys on the grid and every row.
/// - `window` — the [`VisibleWindow`] the binding's view fn windowed
///   against (same `compute_visible_range` source), so the a11y tree and
///   the painted tree never disagree on which rows exist.
///
/// The returned vector is `[grid, header_row, columnheader…, (data_row,
/// gridcell…)…]` — the container first, mirroring the flat convention
/// `lower_access_node` resolves into a tree. Display-only: no row carries
/// `aria-selected` (see [`windowed_grid_nodes_selected`] for the selectable
/// peer).
#[must_use]
pub fn windowed_grid_nodes(
    grid_tag: &str,
    grid_name: &str,
    headers: &[&str],
    set_size: u32,
    window: &VisibleWindow,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::all(headers),
        set_size,
        window,
        GridSelection::Display,
        false,
    )
}

/// R863 §5.40 §5.27 §5.45 — build a **frozen-split** virtualized `grid`: the
/// same container + header + windowed-row topology as [`windowed_grid_nodes`],
/// but each header / data `Row` additionally lists its **frozen-pane** strip
/// (`{tag}_fhrow` / `{tag}_frow{id}`) as a
/// [bounds-union fragment](AccessNode::bounds_union_tags) so the shell resolves
/// the Row's AT bounds as the union of both panes.
///
/// The R859 frozen-left-column grid paints each logical row across two scene
/// fragments — the scrolling pane's `{tag}_row{id}` strip and the frozen pane's
/// `{tag}_frow{id}` strip — but the a11y `Row` container resolves only the
/// former, so the frozen (identity / name) columns fall outside its bounds. The
/// individual `gridcell`s already resolve correctly per-pane (each `{tag}#{id}_{col}`
/// is painted in its own pane); only the row **container** needs the span. This
/// is the first [`AccessNode::bounds_union_tags`] consumer (the tree-grid `row`
/// is the second, the divergence-is-a-bug trigger that lifted the union into the
/// substrate). Display-only — a frozen + selectable grid would add the
/// `aria-selected` axis additively at its second consumer
/// (`[[abstraction-needs-second-consumer]]`).
#[must_use]
pub fn windowed_grid_nodes_frozen(
    grid_tag: &str,
    grid_name: &str,
    headers: &[&str],
    set_size: u32,
    window: &VisibleWindow,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::all(headers),
        set_size,
        window,
        GridSelection::Display,
        true,
    )
}

/// Whether the grid exposes an `aria-selected` axis on its data rows, and
/// (if so) how membership is decided. Distinguishes the three public
/// builders without nested `Option`s.
#[derive(Clone, Copy)]
enum GridSelection<'a> {
    /// Display-only — data rows carry no `aria-selected`.
    Display,
    /// Single-select — `aria-selected = (id == _)` on each data row.
    Single(Option<usize>),
    /// Multi-select — `aria-selected = set.contains(id)` on each data row,
    /// and the grid container is `aria-multiselectable`.
    Multi(&'a std::collections::BTreeSet<usize>),
}

/// The cell paint tag for data row `id`, column `col` — the
/// `GridSendKey` SSOT so the a11y `gridcell` identity matches the painted
/// cell (and the decoder) exactly.
fn cell_tag(grid_tag: &str, id: usize, col: usize) -> String {
    format!("{grid_tag}#{}", GridSendKey::Cell { row: id, col }.encode())
}

/// R1523 §5.40 §5.27 — the **column axis** of a grid whose columns may be
/// windowed: the labels actually in the tree, where they start, and how many
/// columns the grid has in total.
///
/// The column-axis counterpart of the `(window, set_size)` pair the row axis
/// already threads through these builders, and it exists for the same reason:
/// once an axis holds a slice, the slice has to say what it is a slice of, or
/// the AT reads a 200-column table as five columns wide.
///
/// Private — every public builder either takes the whole header slice
/// ([`Self::all`], the pre-R1523 shape) or a header slice plus a column
/// [`VisibleWindow`] it resolves here, so no caller assembles this by hand.
#[derive(Clone, Copy)]
struct GridColumns<'a> {
    /// Labels of the columns present in the tree.
    labels: &'a [&'a str],
    /// Absolute table-column index of `labels[0]`.
    first: usize,
    /// Total number of columns the grid is drawn from (`aria-colcount`).
    total: usize,
}

impl<'a> GridColumns<'a> {
    /// Every column is in the tree — the un-windowed column axis.
    fn all(labels: &'a [&'a str]) -> Self {
        Self {
            labels,
            first: 0,
            total: labels.len(),
        }
    }

    /// R1530 — `labels` are **already** the window: the caller asked its model
    /// for the sections it is describing and no others, so this only records
    /// where the window starts and how wide the table is. Until R1530 this took
    /// every label and sliced here, which required the caller to hold them all.
    ///
    /// `total` is stated rather than derived (`labels.len()` is the window's
    /// width now, not the table's) — the same reason the paint side carries
    /// `VirtualTableData::column_count`.
    fn window(labels: &'a [&'a str], first: usize, total: usize) -> Self {
        Self {
            labels,
            first,
            total,
        }
    }

    /// `(absolute column index, label)` for each column in the tree.
    fn enumerate(&self) -> impl Iterator<Item = (usize, &'a str)> + '_ {
        self.labels
            .iter()
            .enumerate()
            .map(move |(i, label)| (self.first + i, *label))
    }

    /// Absolute indices of the columns in the tree.
    fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        let first = self.first;
        (0..self.labels.len()).map(move |i| first + i)
    }

    /// `aria-colcount` as the saturating `u32` the node carries.
    fn colcount(&self) -> u32 {
        u32::try_from(self.total).unwrap_or(u32::MAX)
    }
}

/// Shared builder for both grid topologies. `selection` distinguishes the
/// two: `None` → display-only (no `aria-selected` axis); `Some(sel)` →
/// single-select (each *data* row gets `aria-selected = (id == sel)`,
/// applied here at construction where `id` is in scope — no tag re-parse).
fn grid_nodes(
    grid_tag: &str,
    grid_name: &str,
    cols: GridColumns<'_>,
    set_size: u32,
    window: &VisibleWindow,
    selection: GridSelection,
    frozen: bool,
) -> Vec<AccessNode> {
    let ncols = cols.labels.len();
    // grid + header row + ncols columnheaders + per windowed row (1 row +
    // ncols cells).
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(window.count * (ncols + 1) + ncols + 2);

    let mut grid = AccessNode::new(grid_tag, AriaRole::Grid)
        .with_name(grid_name)
        .with_size_of_set(set_size)
        // R1523 — the column extent, which `ncols` above is NOT when the
        // column axis is windowed.
        .with_column_count(cols.colcount());
    // Multi-select: selection is an arbitrary row **set**, so the grid
    // container is `aria-multiselectable` (Display / Single are not).
    if matches!(selection, GridSelection::Multi(_)) {
        grid.multiselectable = true;
    }
    grid = grid.with_child(GridTag::header_row(grid_tag));
    for id in window.indices() {
        grid = grid.with_child(GridTag::data_row(grid_tag, id));
    }
    nodes.push(grid);

    // Frozen header row + its columnheader cells. R863 §5.45 — in a
    // frozen-split grid (R859) the header band is painted as `{tag}_hrow`
    // (scrolling pane) + `{tag}_fhrow` (frozen pane); the a11y `Row`
    // resolves only the former, so the frozen columns fall outside its
    // bounds. Union the frozen-pane band so the header Row spans both panes.
    let mut hrow = AccessNode::new(GridTag::header_row(grid_tag), AriaRole::Row);
    if frozen {
        hrow = hrow.with_bounds_union_tag(GridTag::frozen_header_row(grid_tag));
    }
    for col in cols.indices() {
        hrow = hrow.with_child(GridTag::col_header(grid_tag, col));
    }
    nodes.push(hrow);
    for (col, label) in cols.enumerate() {
        nodes.push(
            AccessNode::new(GridTag::col_header(grid_tag, col), AriaRole::ColumnHeader)
                .with_name(label)
                .with_column(col),
        );
    }

    // Windowed data rows + their gridcells.
    for id in window.indices() {
        let posinset = u32::try_from(id + 1).unwrap_or(u32::MAX);
        let mut row = AccessNode::new(GridTag::data_row(grid_tag, id), AriaRole::Row)
            .with_position_in_set(posinset)
            .with_size_of_set(set_size);
        // R863 §5.45 — the frozen pane paints this row's frozen columns as
        // `{tag}_frow{id}`; union it so the data Row's AT bounds cover the
        // frozen (e.g. name) columns, not just the scrolling pane's strip.
        if frozen {
            row = row.with_bounds_union_tag(GridTag::frozen_data_row(grid_tag, id));
        }
        for col in cols.indices() {
            row = row.with_child(cell_tag(grid_tag, id, col));
        }
        // aria-selected on the ROW (WAI-ARIA `SelectRows`): single-select
        // tests `id == selected`, multi-select tests set membership.
        // Display-only omits the axis entirely.
        match selection {
            GridSelection::Display => {}
            GridSelection::Single(selected) => row = row.with_selected(selected == Some(id)),
            GridSelection::Multi(set) => row = row.with_selected(set.contains(&id)),
        }
        nodes.push(row);
        for col in cols.indices() {
            nodes.push(
                AccessNode::new(cell_tag(grid_tag, id, col), AriaRole::GridCell)
                    // R1523 — a windowed cell states its absolute column, so it
                    // stays locatable with its predecessors absent from the tree.
                    .with_column(col),
            );
        }
    }
    nodes
}

/// R1523 §5.40 §5.27 §5.45 — build a `grid` windowed on **both** axes: the
/// [`windowed_grid_nodes`] topology, plus only the columns `cols` selects.
///
/// The row axis has conveyed "which of how many" since R775 through
/// `aria-setsize` / `aria-posinset`. This is the same contract on the column
/// axis: the grid carries `aria-colcount` = `column_count` (the **full**
/// extent) and every `columnheader` / `gridcell` carries its absolute
/// `aria-colindex`, so an AT can place a cell whose 136 predecessors are not in
/// the tree. Windowing an axis without its extent pair would leave the grid
/// *less* readable than before it scaled — a 200-column table announced as five
/// columns wide.
///
/// - `labels` — the labels of the columns `cols` selects, in column order, and
///   **only** those. R1530: until then this took every column's label and
///   sliced the window out here, which meant an AT pass over a 200-column grid
///   materialized 200 strings to read five — the same defect the paint path
///   carried, and the reason the two now share one per-section accessor
///   (`GridModel::header`) instead of one whole-table slice.
/// - `column_count` — the **full** column extent (`aria-colcount`). Passed
///   rather than derived, because `labels` is now a window and its length is
///   not the table's width. This is the column-axis peer of `set_size`.
/// - `rows` — the row window (the same `compute_visible_range` the view painted
///   against).
/// - `cols` — the column window, from
///   [`visible_columns`](pinion_core::widgets::column_widths::visible_columns)
///   over the same widths and the same measured horizontal viewport the view
///   painted against.
///
/// Display-only, like [`windowed_grid_nodes`]: a two-axis windowed grid with a
/// selection axis is additive at its second consumer
/// (`[[abstraction-needs-second-consumer]]`).
#[must_use]
pub fn windowed_grid_nodes_wide(
    grid_tag: &str,
    grid_name: &str,
    labels: &[&str],
    column_count: usize,
    set_size: u32,
    rows: &VisibleWindow,
    cols: &VisibleWindow,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::window(labels, cols.first, column_count),
        set_size,
        rows,
        GridSelection::Display,
        false,
    )
}

/// Build a **single-select** virtualized `grid`: the same container +
/// header + windowed-row topology as [`windowed_grid_nodes`], but every
/// rendered data `row` additionally carries `aria-selected = (id ==
/// selected)`.
///
/// This is the decorated peer the display-only [`windowed_grid_nodes`]
/// serves alongside — a virtualized grid whose selection is held by
/// **data-row index** (a `VirtualSelectExternal`-style coordinator), the
/// grid analogue of [`windowed_list_nodes_selected`](crate::windowed_list_nodes_selected).
/// `aria-selected` lives on the `row` (WAI-ARIA `SelectRows`: the row, not
/// individual cells, carries the selection state — the grid default). A
/// selected row scrolled out of the window simply has no node this frame;
/// the selection survives in the coordinator and re-paints when the row
/// scrolls back.
///
/// `selected` is the absolute data-row index of the selected row, or
/// `None`. Single-select — no `aria-multiselectable` (the multi-select index
/// model is the additive [`windowed_grid_nodes_multiselected`] peer).
#[must_use]
pub fn windowed_grid_nodes_selected(
    grid_tag: &str,
    grid_name: &str,
    headers: &[&str],
    set_size: u32,
    window: &VisibleWindow,
    selected: Option<usize>,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::all(headers),
        set_size,
        window,
        GridSelection::Single(selected),
        false,
    )
}

/// Build a **multi-select** virtualized `grid` (R782 §5.40): the same
/// container + frozen header + windowed-row topology as
/// [`windowed_grid_nodes`], but the grid container additionally carries
/// `aria-multiselectable="true"` and every rendered data `row` carries
/// `aria-selected = selection.contains(id)` — so several visible rows can be
/// `aria-selected` at once (WAI-ARIA `SelectRows`: the selection axis lives
/// on the `row`, not individual cells).
///
/// This is the grid analogue of
/// [`windowed_list_nodes_multiselected`](crate::windowed_list_nodes_multiselected),
/// and the decorated peer the single-select [`windowed_grid_nodes_selected`]
/// serves alongside — a virtualized grid whose selection is held as an
/// arbitrary index **set** by a
/// [`VirtualSelectExternal`](pinion_core::widgets::virtual_select::VirtualSelectExternal)
/// in `Multi` mode. It is the **second** windowed-grid multi-select consumer
/// (after the eager `hello-table-multi`), the R758-cadence trigger that
/// lifts the windowed builder out of the example (R780/R781 carry (a)). A
/// selected row scrolled out of the window simply has no node this frame;
/// the set survives in the coordinator and re-paints when the row scrolls
/// back.
///
/// `selection` is the set of selected absolute data-row indices (built over
/// the window by the caller, exactly as the list peer does).
#[must_use]
pub fn windowed_grid_nodes_multiselected(
    grid_tag: &str,
    grid_name: &str,
    headers: &[&str],
    set_size: u32,
    window: &VisibleWindow,
    selection: &std::collections::BTreeSet<usize>,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::all(headers),
        set_size,
        window,
        GridSelection::Multi(selection),
        false,
    )
}

/// Build a **sorted / filtered** virtualized `grid` (R783 §5.40, lifted from
/// the inline `hello-grid-sort` + `hello-grid-filter` builders): the same
/// container + frozen-header + windowed-row topology as
/// [`windowed_grid_nodes`], but built over a **view-order permutation** rather
/// than the identity mapping the [`windowed_grid_nodes`] family assumes.
///
/// This is the permuted-grid peer R776 carved out as bespoke (a sort / filter
/// reorders + shrinks the rows, so `posinset` is the **visual** position and
/// rows are tagged / selected by **source** id — the identity `posinset = id +
/// 1` no longer holds). The second consumer (`hello-grid-filter` alongside
/// `hello-grid-sort`) triggers the lift, per R758's a11y-axis self-grep
/// mandate: the two were byte-identical, and a divergence between them would
/// be an a11y bug, not a style choice (R743.1 / R745 divergence-is-a-bug).
///
/// - `order` — the visual→source permutation (`order[visual] = source id`);
///   `order.len()` is the current view length (what `aria-setsize` conveys —
///   the *filtered* count, not the full dataset).
/// - `sort` — the active `(col, ascending)` sort key; the matching
///   `columnheader` carries `aria-sort` (none on the others). `None` = no
///   `aria-sort` glyph.
/// - `selected` — the selected **source** data-row index, or `None`; each
///   windowed row carries `aria-selected = (source == selected)`.
/// - `window` — the [`VisibleWindow`] over the *view* positions (same
///   `compute_visible_range` over `order.len()` the view fn uses), so the a11y
///   tree and the painted tree window identically.
#[must_use]
pub fn windowed_grid_nodes_sorted(
    grid_tag: &str,
    grid_name: &str,
    headers: &[&str],
    order: &[usize],
    sort: Option<(usize, bool)>,
    selected: Option<usize>,
    window: &VisibleWindow,
) -> Vec<AccessNode> {
    let ncols = headers.len();
    let total = u32::try_from(order.len()).unwrap_or(u32::MAX);
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(window.count * (ncols + 1) + ncols + 2);

    // Grid container: header row + the windowed data rows (by source id). The
    // setsize is the *view* length (the filtered/sorted row count).
    let mut grid = AccessNode::new(grid_tag, AriaRole::Grid)
        .with_name(grid_name)
        .with_size_of_set(total)
        // R1523 — the column extent, declared on every grid so a windowed and
        // an un-windowed column axis read identically to an AT.
        .with_column_count(u32::try_from(ncols).unwrap_or(u32::MAX));
    grid = grid.with_child(GridTag::header_row(grid_tag));
    for view_pos in window.indices() {
        if let Some(&source) = order.get(view_pos) {
            grid = grid.with_child(GridTag::data_row(grid_tag, source));
        }
    }
    nodes.push(grid);

    // Frozen header row + its columnheaders; the active sort column carries
    // `aria-sort`.
    let mut hrow = AccessNode::new(GridTag::header_row(grid_tag), AriaRole::Row);
    for col in 0..ncols {
        hrow = hrow.with_child(GridTag::col_header(grid_tag, col));
    }
    nodes.push(hrow);
    for (col, label) in headers.iter().enumerate() {
        let mut ch = AccessNode::new(GridTag::col_header(grid_tag, col), AriaRole::ColumnHeader)
            .with_name(*label)
            .with_column(col);
        // R886.1 — active-column decision + bool→direction through the
        // two SSOTs (`col_sort_dir` / `SortDirection::from_ascending`);
        // this builder was a surviving hand-rolled copy the R886 lift's
        // self-grep missed (crate modules count too).
        if let Some(asc) = pinion_core::widgets::grid_sort::col_sort_dir(sort, col) {
            ch = ch.with_sort(SortDirection::from_ascending(asc));
        }
        nodes.push(ch);
    }

    // Windowed data rows: posinset = **visual** position (within the view),
    // tag + selection by **source** id.
    for view_pos in window.indices() {
        let Some(&source) = order.get(view_pos) else {
            continue;
        };
        let posinset = u32::try_from(view_pos + 1).unwrap_or(u32::MAX);
        let mut row = AccessNode::new(GridTag::data_row(grid_tag, source), AriaRole::Row)
            .with_position_in_set(posinset)
            .with_size_of_set(total)
            .with_selected(selected == Some(source));
        for col in 0..ncols {
            row = row.with_child(cell_tag(grid_tag, source, col));
        }
        nodes.push(row);
        for col in 0..ncols {
            nodes.push(
                AccessNode::new(cell_tag(grid_tag, source, col), AriaRole::GridCell)
                    .with_column(col),
            );
        }
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADERS: [&str; 3] = ["Index", "Name", "Status"];

    fn window(first: usize, count: usize) -> VisibleWindow {
        VisibleWindow { first, count }
    }

    // ── R1523 column-axis windowing ─────────────────────────────────

    /// The 200-column table's width. Only a handful of its labels are ever
    /// built — see [`wide_labels`].
    const WIDE_NCOLS: usize = 200;

    /// R1530 — the labels of the columns `cols` selects, and only those: what a
    /// binding produces by asking its model per section. Before R1530 this
    /// fixture built all 200 and the builder sliced, which is the shape the
    /// round removed.
    fn wide_labels(cols: &VisibleWindow) -> Vec<String> {
        cols.indices().map(|c| format!("C{c:03}")).collect()
    }

    /// The two-axis contract: the tree holds the windowed columns, and says how
    /// many columns they were drawn from.
    #[test]
    fn r1523_wide_grid_windows_columns_and_declares_the_full_extent() {
        let cols = window(50, 5);
        let labels = wide_labels(&cols);
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let nodes = windowed_grid_nodes_wide(
            "vcol",
            "Wide grid",
            &refs,
            WIDE_NCOLS,
            10_000,
            &window(0, 2),
            &cols,
        );
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(
            nodes[0].column_count,
            Some(200),
            "aria-colcount is the FULL column extent, not the windowed count",
        );
        let columnheaders: Vec<&AccessNode> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .collect();
        assert_eq!(columnheaders.len(), 5, "only the windowed columns exist");
        assert_eq!(
            columnheaders[0].name.as_deref(),
            Some("C050"),
            "the window starts at the label of column 50, not column 0",
        );
        // 2 windowed rows x 5 windowed columns.
        let cells: Vec<&AccessNode> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::GridCell)
            .collect();
        assert_eq!(cells.len(), 2 * 5);
    }

    /// `aria-colindex` is one-based and **absolute**, so a windowed cell is
    /// locatable although the 50 columns before it are not in the tree.
    #[test]
    fn r1523_colindex_is_one_based_and_absolute() {
        let cols = window(50, 3);
        let labels = wide_labels(&cols);
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let nodes =
            windowed_grid_nodes_wide("vcol", "G", &refs, WIDE_NCOLS, 10_000, &window(0, 1), &cols);
        let cols: Vec<u32> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .filter_map(|n| n.column_index)
            .collect();
        assert_eq!(cols, vec![51, 52, 53], "one-based absolute column indices");
        let cell_cols: Vec<u32> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::GridCell)
            .filter_map(|n| n.column_index)
            .collect();
        assert_eq!(cell_cols, vec![51, 52, 53], "cells carry the same axis");
        // The cell TAGS keep the zero-based absolute column, unchanged — the
        // wire vocabulary and the ARIA vocabulary are different axes and this
        // is the one place both are visible.
        assert!(
            nodes.iter().any(|n| n.tag == cell_tag("vcol", 0, 50)),
            "cell tags stay zero-based absolute (the paint / send-wire form)",
        );
    }

    /// The row axis is untouched: its extent still reaches the AT the way it
    /// has since R775.
    #[test]
    fn r1523_row_axis_extent_is_unchanged_by_column_windowing() {
        let cols = window(50, 3);
        let labels = wide_labels(&cols);
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let nodes = windowed_grid_nodes_wide(
            "vcol",
            "G",
            &refs,
            WIDE_NCOLS,
            10_000,
            &window(100, 2),
            &cols,
        );
        assert_eq!(nodes[0].size_of_set, Some(10_000));
        let first_data = nodes
            .iter()
            .find(|n| n.role == AriaRole::Row && n.tag != "vcol_hrow")
            .expect("a windowed data row");
        assert_eq!(first_data.position_in_set, Some(101));
        assert_eq!(first_data.size_of_set, Some(10_000));
    }

    /// R1530 — the builder describes exactly the labels it is handed, and the
    /// extent it is told, with no arithmetic between them.
    ///
    /// R1523 clamped a window that ran past the header list, because the two
    /// arrived separately (two reads of the same reactive state, so a frame
    /// that saw them disagree was reachable) and a bad slice would panic the
    /// a11y walker. There is no slice left to get wrong: the labels ARE the
    /// window. The out-of-range case is therefore not clamped here but simply
    /// absent — a caller past the end asks its model for nothing and hands over
    /// nothing — and the `aria-colcount` still contextualises whatever is left.
    #[test]
    fn r1530_builder_describes_the_labels_it_is_given() {
        let one = ["Value"];
        let nodes =
            windowed_grid_nodes_wide("vcol", "G", &one, 3, 10, &window(0, 1), &window(2, 1));
        let columnheaders: Vec<&AccessNode> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .collect();
        assert_eq!(columnheaders.len(), 1, "one label in, one columnheader out");
        assert_eq!(
            columnheaders[0].column_index,
            Some(3),
            "placed at the section the window says, not at the label's offset",
        );
        assert_eq!(
            nodes[0].column_count,
            Some(3),
            "the extent is stated, not counted from the labels"
        );
        // Nothing in range: no columns, no panic, extent intact.
        let empty: [&str; 0] = [];
        let none =
            windowed_grid_nodes_wide("vcol", "G", &empty, 3, 10, &window(0, 1), &window(99, 0));
        assert_eq!(
            none.iter()
                .filter(|n| n.role == AriaRole::ColumnHeader)
                .count(),
            0,
        );
        assert_eq!(none[0].column_count, Some(3));
    }

    /// Every grid declares its column count, windowed or not, so the two cases
    /// read identically to an AT (and a consumer cannot tell them apart by the
    /// absence of an attribute).
    #[test]
    fn r1523_every_grid_declares_its_column_count() {
        let display = windowed_grid_nodes("vtbl", "G", &HEADERS, 10, &window(0, 1));
        assert_eq!(display[0].column_count, Some(3));
        let selected =
            windowed_grid_nodes_selected("vtbl", "G", &HEADERS, 10, &window(0, 1), Some(0));
        assert_eq!(selected[0].column_count, Some(3));
        let sorted = windowed_grid_nodes_sorted(
            "vtbl",
            "G",
            &HEADERS,
            &[0, 1, 2],
            None,
            None,
            &window(0, 1),
        );
        assert_eq!(sorted[0].column_count, Some(3));
        let frozen = windowed_grid_nodes_frozen("vtbl", "G", &HEADERS, 10, &window(0, 1));
        assert_eq!(frozen[0].column_count, Some(3));
    }

    #[test]
    fn emits_grid_header_and_windowed_rows() {
        let nodes = windowed_grid_nodes("vtbl", "Data grid", &HEADERS, 10_000, &window(0, 2));
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(nodes[0].name.as_deref(), Some("Data grid"));
        assert_eq!(
            nodes[0].size_of_set,
            Some(10_000),
            "grid setsize = FULL dataset"
        );
        // grid claims the header row + the 2 windowed data rows.
        assert_eq!(nodes[0].children.len(), 3);
        // header row + 3 columnheaders.
        assert_eq!(nodes[1].role, AriaRole::Row);
        assert_eq!(nodes[1].tag, "vtbl_hrow");
        let columnheaders = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .count();
        assert_eq!(columnheaders, 3, "one columnheader per column");
        // 2 windowed data rows, each with 3 gridcells.
        let data_rows = nodes
            .iter()
            .filter(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow")
            .count();
        assert_eq!(data_rows, 2);
        let cells = nodes
            .iter()
            .filter(|n| n.role == AriaRole::GridCell)
            .count();
        assert_eq!(cells, 2 * 3, "NCOLS gridcells per windowed row");
    }

    #[test]
    fn posinset_is_one_based_and_tracks_the_window() {
        let nodes = windowed_grid_nodes("vtbl", "G", &HEADERS, 10_000, &window(100, 2));
        let first_data = nodes
            .iter()
            .find(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow")
            .expect("a data row");
        assert_eq!(first_data.tag, "vtbl_row100");
        assert_eq!(first_data.position_in_set, Some(101));
    }

    #[test]
    fn display_only_omits_aria_selected() {
        let nodes = windowed_grid_nodes("vtbl", "G", &HEADERS, 10_000, &window(0, 2));
        for n in nodes
            .iter()
            .filter(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow")
        {
            assert_eq!(
                n.selected, None,
                "display-only data rows omit aria-selected"
            );
        }
    }

    // ── R863 frozen-split bounds union ───────────────────────────────

    #[test]
    fn unsplit_grid_rows_carry_no_bounds_union() {
        // The shared (non-frozen) builders never list a frozen-pane fragment —
        // a single-pane row resolves bounds from its own tag alone.
        let nodes = windowed_grid_nodes("vtbl", "G", &HEADERS, 10_000, &window(0, 2));
        for n in nodes.iter().filter(|n| n.role == AriaRole::Row) {
            assert!(
                n.bounds_union_tags.is_empty(),
                "unsplit Row {} has no union",
                n.tag
            );
        }
    }

    #[test]
    fn frozen_grid_rows_union_the_frozen_pane_strip() {
        // The frozen builder makes the header Row + each data Row span both
        // panes: the scrolling-pane strip (own tag) ∪ the frozen-pane strip.
        let nodes =
            windowed_grid_nodes_frozen("gfz", "Frozen grid", &HEADERS, 10_000, &window(0, 2));
        let hrow = nodes.iter().find(|n| n.tag == "gfz_hrow").unwrap();
        assert_eq!(
            hrow.bounds_union_tags,
            vec!["gfz_fhrow"],
            "header Row unions the frozen header band",
        );
        let row0 = nodes.iter().find(|n| n.tag == "gfz_row0").unwrap();
        assert_eq!(
            row0.bounds_union_tags,
            vec!["gfz_frow0"],
            "data Row unions its frozen strip"
        );
        let row1 = nodes.iter().find(|n| n.tag == "gfz_row1").unwrap();
        assert_eq!(row1.bounds_union_tags, vec!["gfz_frow1"]);
    }

    #[test]
    fn frozen_is_a_superset_of_the_display_topology() {
        // The frozen builder must build the IDENTICAL container + header +
        // row + cell topology as the display-only builder — only each Row's
        // bounds_union_tags fragment is added (the gridcells already resolve
        // per-pane, so they are byte-identical).
        let plain = windowed_grid_nodes("gfz", "G", &HEADERS, 10_000, &window(0, 2));
        let frozen = windowed_grid_nodes_frozen("gfz", "G", &HEADERS, 10_000, &window(0, 2));
        assert_eq!(plain.len(), frozen.len());
        for (p, f) in plain.iter().zip(&frozen) {
            assert_eq!(p.tag, f.tag);
            assert_eq!(p.role, f.role);
            assert_eq!(p.children, f.children, "{} children unchanged", p.tag);
            assert_eq!(p.position_in_set, f.position_in_set);
        }
        // Only the Row nodes gain a union fragment; gridcells / columnheaders
        // stay single-fragment.
        for n in frozen.iter().filter(|n| n.role != AriaRole::Row) {
            assert!(
                n.bounds_union_tags.is_empty(),
                "{} is single-fragment",
                n.tag
            );
        }
    }

    #[test]
    fn selected_marks_only_the_selected_row() {
        // Window 100..102, row 101 selected.
        let nodes =
            windowed_grid_nodes_selected("vtbl", "G", &HEADERS, 10_000, &window(100, 2), Some(101));
        let row100 = nodes.iter().find(|n| n.tag == "vtbl_row100").unwrap();
        let row101 = nodes.iter().find(|n| n.tag == "vtbl_row101").unwrap();
        assert_eq!(row100.selected, Some(false));
        assert_eq!(row101.selected, Some(true), "selected row is aria-selected");
        // The header row never gets a selected axis.
        let hrow = nodes.iter().find(|n| n.tag == "vtbl_hrow").unwrap();
        assert_eq!(hrow.selected, None, "header row carries no aria-selected");
        // Single-select: no multiselectable on the grid container.
        assert!(!nodes[0].multiselectable);
    }

    #[test]
    fn selected_is_a_superset_of_the_display_topology() {
        let plain = windowed_grid_nodes("vtbl", "G", &HEADERS, 10_000, &window(0, 2));
        let decorated =
            windowed_grid_nodes_selected("vtbl", "G", &HEADERS, 10_000, &window(0, 2), None);
        assert_eq!(plain.len(), decorated.len());
        for (p, d) in plain.iter().zip(&decorated) {
            assert_eq!(p.tag, d.tag);
            assert_eq!(p.role, d.role);
            assert_eq!(p.position_in_set, d.position_in_set);
            assert_eq!(p.children, d.children);
        }
        // Only the per-data-row aria-selected axis is added.
        let d_row = decorated.iter().find(|n| n.tag == "vtbl_row0").unwrap();
        assert_eq!(
            d_row.selected,
            Some(false),
            "decorated row sets aria-selected"
        );
    }

    #[test]
    fn selected_outside_window_marks_no_visible_row() {
        let nodes =
            windowed_grid_nodes_selected("vtbl", "G", &HEADERS, 10_000, &window(0, 2), Some(9_999));
        for n in nodes
            .iter()
            .filter(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow")
        {
            assert_eq!(n.selected, Some(false));
        }
    }

    // ── R782 multi-select decorated variant ─────────────────────────

    #[test]
    fn multiselect_marks_every_member_and_the_container() {
        // Window 100..104; rows 101 and 103 selected → both rows
        // aria-selected, the grid container aria-multiselectable.
        let selection: std::collections::BTreeSet<usize> = [101, 103].into_iter().collect();
        let nodes = windowed_grid_nodes_multiselected(
            "vtbl",
            "G",
            &HEADERS,
            10_000,
            &window(100, 4),
            &selection,
        );
        assert!(
            nodes[0].multiselectable,
            "multi-select grid is aria-multiselectable"
        );
        let row101 = nodes.iter().find(|n| n.tag == "vtbl_row101").unwrap();
        let row103 = nodes.iter().find(|n| n.tag == "vtbl_row103").unwrap();
        let row102 = nodes.iter().find(|n| n.tag == "vtbl_row102").unwrap();
        assert_eq!(row101.selected, Some(true), "member row is aria-selected");
        assert_eq!(
            row103.selected,
            Some(true),
            "a second member is aria-selected at once"
        );
        assert_eq!(
            row102.selected,
            Some(false),
            "a non-member between them is not"
        );
        // The header row never gets a selected axis.
        let hrow = nodes.iter().find(|n| n.tag == "vtbl_hrow").unwrap();
        assert_eq!(hrow.selected, None, "header row carries no aria-selected");
    }

    #[test]
    fn multiselect_is_a_superset_of_the_display_topology() {
        // The multi builder must build the IDENTICAL container + header +
        // row + cell topology as the display-only builder — only the
        // container `aria-multiselectable` + per-row `aria-selected` are
        // added.
        let empty = std::collections::BTreeSet::new();
        let plain = windowed_grid_nodes("vtbl", "G", &HEADERS, 10_000, &window(0, 2));
        let decorated =
            windowed_grid_nodes_multiselected("vtbl", "G", &HEADERS, 10_000, &window(0, 2), &empty);
        assert_eq!(plain.len(), decorated.len());
        for (p, d) in plain.iter().zip(&decorated) {
            assert_eq!(p.tag, d.tag);
            assert_eq!(p.role, d.role);
            assert_eq!(p.position_in_set, d.position_in_set);
            assert_eq!(p.children, d.children);
        }
        // Display-only is not multiselectable; the multi container is.
        assert!(!plain[0].multiselectable);
        assert!(decorated[0].multiselectable);
        let d_row = decorated.iter().find(|n| n.tag == "vtbl_row0").unwrap();
        assert_eq!(
            d_row.selected,
            Some(false),
            "decorated row sets aria-selected"
        );
    }

    // ── R783 permuted (sorted / filtered) variant ───────────────────

    #[test]
    fn sorted_builds_rows_by_source_with_visual_posinset() {
        // A reversed 4-row view order over a 4-row dataset, col-1 ascending,
        // source row 2 selected. posinset tracks VISUAL position; tags +
        // selection track SOURCE id.
        let order = [3usize, 2, 1, 0];
        let nodes = windowed_grid_nodes_sorted(
            "vtbl",
            "Sorted grid",
            &HEADERS,
            &order,
            Some((1, true)),
            Some(2),
            &window(0, 4),
        );
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(nodes[0].size_of_set, Some(4), "setsize is the VIEW length");
        // Active column header carries aria-sort=ascending; others none.
        let ch1 = nodes.iter().find(|n| n.tag == "vtbl_ch1").unwrap();
        assert_eq!(ch1.sort, Some(SortDirection::Ascending));
        let ch0 = nodes.iter().find(|n| n.tag == "vtbl_ch0").unwrap();
        assert_eq!(ch0.sort, None, "inactive column has no aria-sort");
        // Visual position 1 is source 2 (order[1]) → posinset 2, selected.
        let src2 = nodes.iter().find(|n| n.tag == "vtbl_row2").unwrap();
        assert_eq!(
            src2.position_in_set,
            Some(2),
            "posinset = visual position + 1"
        );
        assert_eq!(
            src2.selected,
            Some(true),
            "the selected SOURCE row is aria-selected"
        );
        // Source 3 sits at visual 0 → posinset 1, not selected.
        let src3 = nodes.iter().find(|n| n.tag == "vtbl_row3").unwrap();
        assert_eq!(src3.position_in_set, Some(1));
        assert_eq!(src3.selected, Some(false));
    }

    #[test]
    fn sorted_setsize_is_the_filtered_view_length() {
        // A filtered view: only 2 of a larger dataset survive; the order is
        // those 2 source ids. setsize conveys the VIEW length (2), not the
        // dataset size, and only the surviving sources get rows.
        let order = [7usize, 4];
        let nodes = windowed_grid_nodes_sorted(
            "vtbl",
            "Filtered grid",
            &HEADERS,
            &order,
            None,
            None,
            &window(0, 2),
        );
        assert_eq!(
            nodes[0].size_of_set,
            Some(2),
            "setsize = filtered view length"
        );
        let data_rows: Vec<&str> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow")
            .map(|n| n.tag.as_str())
            .collect();
        assert_eq!(
            data_rows,
            vec!["vtbl_row7", "vtbl_row4"],
            "only surviving sources, in view order"
        );
        // No active sort → no aria-sort on any header.
        assert!(
            nodes
                .iter()
                .filter(|n| n.role == AriaRole::ColumnHeader)
                .all(|n| n.sort.is_none())
        );
    }

    #[test]
    fn multiselect_empty_window_yields_grid_and_header_only() {
        let selection: std::collections::BTreeSet<usize> = [0, 5].into_iter().collect();
        let nodes = windowed_grid_nodes_multiselected(
            "vtbl",
            "G",
            &HEADERS,
            10_000,
            &VisibleWindow::EMPTY,
            &selection,
        );
        assert!(nodes[0].multiselectable, "the container axis still applies");
        let data_rows = nodes
            .iter()
            .filter(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow")
            .count();
        assert_eq!(data_rows, 0, "an empty window has no data-row nodes");
    }
}
