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
use pinion_core::widgets::cell_selection::GridSelection as CoreGridSelection;
use pinion_core::widgets::virtual_list::VisibleWindow;
use pinion_core::widgets::virtual_select::SelectionExtent;
use pinion_core::{CellIndex, EditorForm};

/// Build the virtualized `grid` container + frozen header row + one data
/// `row` (with its `gridcell`s) per windowed index.
///
/// - `grid_tag` — the paint-root tag; the sub-tags are derived from it
///   (`{grid_tag}_hrow`, `{grid_tag}_ch{col}`, `{grid_tag}_row{id}`,
///   `{grid_tag}#{id}_{col}`).
/// - `grid_name` — the grid's accessible name (no paint-scene equivalent,
///   so it is an explicit override).
/// - `columns` — how many columns the tree holds. R1547: a count, not the
///   labels. The `columnheader`s are named from the painted header band, so
///   nothing here needs to know what a column is called (see `GridColumns`).
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
    columns: usize,
    set_size: u32,
    window: &VisibleWindow,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::all(columns),
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
    columns: usize,
    set_size: u32,
    window: &VisibleWindow,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::all(columns),
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
    /// Multi-select — `aria-selected = is_selected(id)` on each data row, and
    /// the grid container is `aria-multiselectable`.
    ///
    /// R1561 — a **predicate**, not a set. It was a `&BTreeSet<usize>`, which
    /// obliged every caller to build one per frame purely so this could ask
    /// `contains` about the ~20 rows in the window; both bindings did exactly
    /// that, and the set was a third representation of a fact the model
    /// (`IndexRuns`) and the binding's own paint projection each already hold.
    /// The question this axis asks is per rendered row and is nothing but
    /// membership — the toolkit's `isSelected(index)` — so it takes
    /// the question rather than a container that can answer it, and a caller
    /// holding runs, a bitmap or a tree passes its own without converting.
    Multi(&'a dyn Fn(usize) -> bool),
    /// R1563 — a **two-axis** selection: `aria-selected` on the rendered
    /// `gridcell`s as well as on their `row`s, and on the `columnheader` of a
    /// fully selected column.
    ///
    /// The row arms above can only say that a whole record is selected, which
    /// is what they were built for. Under
    /// [`SelectionBehavior::SelectItems`](pinion_core::widgets::cell_selection::SelectionBehavior::SelectItems)
    /// the selected thing is a cell, and a row-level flag would announce a row
    /// as selected because one of its two hundred cells is — or, if it
    /// demanded all of them, announce nothing at all for the selection the user
    /// just made.
    ///
    /// **Past the toolkit 6.11 by being connected at all.** the toolkit has
    /// the accessor — `isSelected()` — but it reads `isSelected` on the *view's* model, and
    /// accessible table header cell has no selection state of any kind, so a
    /// fully selected column announces exactly as an unselected one.
    Cells(&'a dyn CoreGridSelection),
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
/// R1547 — it holds **counts, not labels**. Until R1547 it carried the label
/// of every column in the tree, for one purpose: stamping each `columnheader`'s
/// accessible name. That made the builder a *second* source for what a column
/// is called, and the paint the first — the shape R1536 removed from the cell
/// axis, where a hand-stamped name would have silently won over the derivation
/// and hidden whatever the grid actually drew. Now the name comes from the
/// painted header, so the AT tree and the pixels cannot disagree, and a
/// section's `DecorationRole` mark can reach the name at all (it is painted there, and
/// nothing here could see it).
///
/// Private — every public builder either states the tree holds every column
/// ([`Self::all`]) or states a window plus the table's width, so no caller
/// assembles this by hand.
#[derive(Clone, Copy)]
struct GridColumns {
    /// How many columns are present in the tree.
    count: usize,
    /// Absolute table-column index of the first of them.
    first: usize,
    /// Total number of columns the grid is drawn from (`aria-colcount`).
    total: usize,
}

impl GridColumns {
    /// Every column is in the tree — the un-windowed column axis.
    fn all(count: usize) -> Self {
        Self {
            count,
            first: 0,
            total: count,
        }
    }

    /// R1530 — `count` is **already** the window: the caller describes the
    /// sections it painted and no others. Until R1530 this took every label and
    /// sliced here, which required the caller to hold them all.
    ///
    /// `total` is stated rather than derived (`count` is the window's width, not
    /// the table's) — the same reason the paint side carries
    /// `VirtualTableData::column_count`.
    fn window(count: usize, first: usize, total: usize) -> Self {
        Self {
            count,
            first,
            total,
        }
    }

    /// Absolute indices of the columns in the tree.
    fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        let first = self.first;
        (0..self.count).map(move |i| first + i)
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
    cols: GridColumns,
    set_size: u32,
    window: &VisibleWindow,
    selection: GridSelection,
    frozen: bool,
) -> Vec<AccessNode> {
    let ncols = cols.count;
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
    if matches!(selection, GridSelection::Multi(_) | GridSelection::Cells(_)) {
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
    for col in cols.indices() {
        // R1547 §5.40 — NO `with_name`. The name is derived from the painted
        // header (`enrich_names_from_scene` skips any node that already has
        // one), which is what lets a section's `DecorationRole` mark join
        // it and what keeps the announced string identical to the drawn one.
        let mut header =
            AccessNode::new(GridTag::col_header(grid_tag, col), AriaRole::ColumnHeader)
                .with_column(col);
        // R1563 — a column selected in every row is `aria-selected`, the
        // column-axis peer of the row's flag. Only the `Cells` arm can answer
        // it: a row predicate carries no row extent, so it cannot know whether
        // a column is covered.
        if let GridSelection::Cells(sel) = selection {
            header = header.with_selected(sel.column(col) == SelectionExtent::All);
        }
        nodes.push(header);
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
            GridSelection::Multi(is_selected) => row = row.with_selected(is_selected(id)),
            // R1563 — a row is `aria-selected` when it is selected AS A RECORD.
            // A partly selected row is not: its selected cells carry the flag,
            // which is the only reading that does not announce two hundred
            // unselected cells as selected.
            GridSelection::Cells(sel) => {
                row = row.with_selected(sel.row(id) == SelectionExtent::All);
            }
        }
        nodes.push(row);
        for col in cols.indices() {
            let mut cell = AccessNode::new(cell_tag(grid_tag, id, col), AriaRole::GridCell)
                // R1523 — a windowed cell states its absolute column, so it
                // stays locatable with its predecessors absent from the tree.
                .with_column(col);
            // R1563 — the toolkit `isSelected()`. Set only on the
            // two-axis arm, so every row-select grid's tree is byte-identical:
            // there `aria-selected` on the row is the whole fact, and a
            // per-cell flag would restate it once per column.
            if let GridSelection::Cells(sel) = selection {
                cell = cell.with_selected(sel.cell(id, col));
            }
            nodes.push(cell);
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
/// - `columns` — how many columns `cols` selects, i.e. how many the tree holds.
///   R1530 made this a window rather than the whole table (an AT pass over a
///   200-column grid materialized 200 strings to read five); R1547 made it a
///   count rather than a slice of labels, because the `columnheader`s are named
///   from the painted header band and nothing here reads a label any more.
/// - `column_count` — the **full** column extent (`aria-colcount`). Passed
///   rather than derived, because `columns` is a window and is not the table's
///   width. This is the column-axis peer of `set_size`.
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
    columns: usize,
    column_count: usize,
    set_size: u32,
    rows: &VisibleWindow,
    cols: &VisibleWindow,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::window(columns, cols.first, column_count),
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
    columns: usize,
    set_size: u32,
    window: &VisibleWindow,
    selected: Option<usize>,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::all(columns),
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
/// R1561 — `is_selected` answers membership for one **absolute** data-row
/// index, and is asked once per **rendered** row. Was a `&BTreeSet<usize>` the
/// caller built over the window each frame, purely so this could ask
/// `contains` about the ~20 rows in it — a third representation of a fact the
/// model and the binding's own paint projection each already hold. The
/// question this axis asks is nothing but membership (the toolkit's
/// `isSelected`), so it takes the question rather than a
/// container that can answer it, and a caller holding runs, a bitmap or a tree
/// passes its own without converting.
#[must_use]
pub fn windowed_grid_nodes_multiselected(
    grid_tag: &str,
    grid_name: &str,
    columns: usize,
    set_size: u32,
    window: &VisibleWindow,
    is_selected: &dyn Fn(usize) -> bool,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::all(columns),
        set_size,
        window,
        GridSelection::Multi(is_selected),
        false,
    )
}

/// R1563 §5.40 §5.27 — build a virtualized `grid` whose selection has **two
/// axes**: `aria-selected` on the rendered `gridcell`s, on the `row`s selected
/// as whole records, and on the `columnheader` of a column selected in every
/// row.
///
/// The two-axis peer of [`windowed_grid_nodes_multiselected`], and it takes the
/// same kind of argument for the same reason — the R1536 rule that this axis
/// asks a *question*, so a caller holding runs, bands or a window bitmap passes
/// its own answer rather than materialising a container for the builder to
/// query.
///
/// - `columns` — how many columns the tree holds, and how many each rendered
///   row is asked about. The window, not the table's width.
/// - `selection` — the [`CoreGridSelection`] question, over **absolute** data
///   row and column indices.
#[must_use]
pub fn windowed_grid_nodes_cells(
    grid_tag: &str,
    grid_name: &str,
    columns: usize,
    set_size: u32,
    window: &VisibleWindow,
    selection: &dyn CoreGridSelection,
) -> Vec<AccessNode> {
    grid_nodes(
        grid_tag,
        grid_name,
        GridColumns::all(columns),
        set_size,
        window,
        GridSelection::Cells(selection),
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
///
/// # A second row emitter (recorded R1548)
///
/// This function builds its grid / header / row / cell nodes **itself** rather
/// than through `grid_nodes`, which every other builder in this module shares.
/// The two are structurally alike and differ in three things — the id is
/// `order[view_pos]` instead of `view_pos`, `posinset` is the visual position,
/// and the active column carries `aria-sort` — all three of which `grid_nodes`
/// could express. It is therefore a latent divergence: a fix applied to the row
/// topology in one lands in five builders and not in this one.
///
/// R1548 did **not** unify them, and the reason it could get away with that is
/// the reason it is recorded here rather than fixed: the vertical header axis
/// arrived as [`attach_row_headers`], a pass over already-built nodes, so it
/// reaches this topology without either implementation learning about it. A
/// flag would have had to be threaded into both. The next change that is *not*
/// expressible as a pass is the one that has to do the merge.
#[must_use]
pub fn windowed_grid_nodes_sorted(
    grid_tag: &str,
    grid_name: &str,
    columns: usize,
    order: &[usize],
    sort: Option<(usize, bool)>,
    selected: Option<usize>,
    window: &VisibleWindow,
) -> Vec<AccessNode> {
    let ncols = columns;
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
    for col in 0..ncols {
        // R1547 — named from the painted header, like every other
        // `columnheader`; see `GridColumns`.
        let mut ch = AccessNode::new(GridTag::col_header(grid_tag, col), AriaRole::ColumnHeader)
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

/// R1544 §5.40 §5.27 — mark every **non-editable** windowed `gridcell`
/// `aria-readonly`, from the same `GridModel::edit` role the grid opens its
/// editors from.
///
/// # Why a pass over the built nodes
///
/// Editability is a per-**cell** model answer, and the six builders above are
/// already at the argument ceiling; threading a predicate through all of them
/// would grow a family this module is trying not to grow. This composes with
/// every one of them instead, following the orphan-free rule
/// [`attach_child_button`](crate::node::attach_child_button) established here:
/// it addresses cells only through the [`GridSendKey`] encode SSOT (never by
/// decoding a tag back into coordinates), and a cell absent from `nodes` —
/// windowed out on either axis — is silently skipped rather than invented.
///
/// # Why the *non*-editable cells are the ones marked
///
/// WAI-ARIA's `aria-readonly` defaults to `false`, so an unmarked cell already reads as
/// editable. Marking the read-only ones is therefore the whole statement, and
/// it means a display-only grid that passes `|_| false` says so to assistive
/// technology — which no grid in this tree did before R1544, and which the
/// toolkit does not say at all: accessible table cell builds its state from
/// the *view's* selection and expansion, never from the model's `ItemIsEditable`, so a
/// toolkit screen-reader user cannot tell a fixed column from an editable one
/// until they try to type into it.
///
/// `rows` is the row window the tree was built from and `cols` the column
/// window — the same two the builder used, so this asks about exactly the
/// cells that exist.
pub fn mark_grid_editability(
    nodes: &mut [AccessNode],
    grid_tag: &str,
    rows: &VisibleWindow,
    cols: core::ops::Range<usize>,
    editable: impl Fn(CellIndex) -> bool,
) {
    for row in rows.indices() {
        for col in cols.clone() {
            if editable(CellIndex { row, col }) {
                continue;
            }
            let tag = cell_tag(grid_tag, row, col);
            if let Some(cell) = nodes.iter_mut().find(|n| n.tag == tag) {
                cell.state.read_only = true;
            }
        }
    }
}

/// R1555 §5.40 §5.27 — announce the **open editor** under the cell hosting it,
/// with the role its [`EditorForm`] actually has.
///
/// The a11y half of the toolkit's item editor factory. Called with the latch's
/// `(index, form)` when — and only when — an editor is painted, which is one statement on
/// the paint side ([`cell_editor`](../../pinion_widget_paint/table/fn.cell_editor.html)
/// dispatches on the same `form`), so the announced control and the drawn one
/// cannot be different kinds.
///
/// # Why a child node and not a role on the cell
///
/// A `gridcell` stays a `gridcell` while it hosts an editor — that is what keeps the row /
/// column geometry intact for AT table navigation. WAI-ARIA puts the input
/// *inside* the cell, and so does the toolkit: the editor is a real child
/// widget with its own accessible interface. The
/// [`attach_child_button`](crate::node::attach_child_button) precedent gives the same
/// orphan-free rule — a cell absent from `nodes` (windowed out on either axis)
/// emits **nothing**, neither the link nor the node, so an editor node with no
/// host cannot exist.
///
/// # Against the toolkit 6.11
///
/// The toolkit reaches the right role by accident of construction: because the
/// editor is a widget, accessible reports whatever that widget is. The
/// consequence is that the toolkit's *default* factory decides the announced
/// role too — a bool cell announces as a **combo box** there, because that is
/// what item editor factory hands back for `Bool`, and a colour cell announces
/// nothing at all because no editor is created. Here the role follows from the
/// datum, so the announcement is a property of the cell's type rather than of
/// which widget the factory happened to construct.
///
/// - `index` — the editing cell, from
///   [`GridEditState::editing`](../../pinion_core/widgets/grid_edit/struct.GridEditState.html#method.editing).
/// - `form` — its [`EditorForm`].
/// - `name` — the editor's accessible name. The cell's own name is its
///   *content*; an editor needs to say what is being edited, which only the
///   binding knows (the toolkit has the same gap and fills it from the column header).
pub fn attach_cell_editor(
    nodes: &mut Vec<AccessNode>,
    grid_tag: &str,
    index: CellIndex,
    form: EditorForm,
    name: impl Into<String>,
) {
    let cell = cell_tag(grid_tag, index.row, index.col);
    let Some(host) = nodes.iter_mut().find(|n| n.tag == cell) else {
        return;
    };
    let editor_tag = format!("{cell}#editor");
    host.children.push(editor_tag.clone());
    nodes.push(AccessNode::new(editor_tag, editor_role(form)).with_name(name));
}

/// R1555 — the WAI-ARIA role of each editor form.
///
/// Exhaustive over [`EditorForm`], so a sixth form cannot be added without
/// deciding what assistive technology should call it — the failure mode a
/// `_ => AriaRole::TextInput` fallback would hide.
#[must_use]
pub fn editor_role(form: EditorForm) -> AriaRole {
    match form {
        // A swatch's editable half is its hex field; the chip is painted
        // geometry beside it. `textbox` is what a user can act on.
        EditorForm::Field | EditorForm::Swatch => AriaRole::TextInput,
        EditorForm::Stepper => AriaRole::SpinButton,
        EditorForm::Toggle => AriaRole::CheckBox,
        EditorForm::Selector => AriaRole::ComboBox,
    }
}

/// R1548 §5.40 §5.27 — give a virtualized grid's tree its **vertical header
/// axis**: every windowed `row` gains a leading WAI-ARIA `rowheader` cell, and
/// a node for it is appended.
///
/// The a11y half of the toolkit's `headerData(section, Vertical, …)` /
/// `verticalHeader()`, paired with
/// [`GridModel::rows`](../../pinion_widget_paint/table/struct.GridModel.html)
/// on the paint side. Call it when — and only when — the grid's model answers
/// that axis, which is exactly when the band is painted: the two are one
/// statement there, so they cannot disagree here either.
///
/// # Why a pass over the built nodes
///
/// The R1544 [`mark_grid_editability`] precedent, for the reason it gives: the
/// builders above are at the argument ceiling, and threading a flag through all
/// six would grow a family this module is trying not to grow. Composing instead
/// means the axis reaches **every** topology at once — identity-order, sorted,
/// frozen, column-windowed, single- and multi-select — rather than the one
/// builder a new parameter would have been added to, which is how a sibling
/// surface silently ends up without an axis its neighbour has.
///
/// Orphan-free by the same rule: rows are addressed only through the
/// [`GridTag`] encode SSOT (never by decoding a tag back into an index), and a
/// row absent from `nodes` — windowed out — is skipped rather than invented.
///
/// # Why the cell carries no name here
///
/// R1547's rule, on the second axis: the `rowheader`'s accessible name is
/// derived from the painted band, so the announced string is the drawn one and
/// a section's `DecorationRole` mark can join it. The toolkit derives a header
/// cell's name from the model on a path independent of `paintSection`
/// (`text` reads `DisplayRole`), so a toolkit view
/// that elides or overrides its row header announces a string that is not on
/// screen — and one whose distinguishing information is a glyph announces
/// nothing.
///
/// - `rows` — the row window the tree was built from.
/// - `source` — visual position → **data** row, the R778 sort permutation the
///   paint asked its model with (`|v| v` for an unpermuted grid). One function
///   rather than a slice so the identity case allocates nothing.
pub fn attach_row_headers(
    nodes: &mut Vec<AccessNode>,
    grid_tag: &str,
    rows: &VisibleWindow,
    source: impl Fn(usize) -> usize,
) {
    let mut headers: Vec<AccessNode> = Vec::with_capacity(rows.count);
    for view_pos in rows.indices() {
        let id = source(view_pos);
        let row_tag = GridTag::data_row(grid_tag, id);
        let Some(row) = nodes.iter_mut().find(|n| n.tag == row_tag) else {
            continue;
        };
        let header_tag = GridTag::row_header(grid_tag, id);
        // Leading, because a `rowheader` labels the cells that follow it — the
        // reading order an AT walks, and the order the band is painted in.
        row.children.insert(0, header_tag.clone());
        headers.push(AccessNode::new(header_tag, AriaRole::RowHeader));
    }
    nodes.extend(headers);
}

/// R1562 §5.40 §5.27 — give a grid's tree the **corner control**: the
/// select-all where the two section axes meet (the toolkit's table corner
/// button).
///
/// Two nodes, in HTML's own shape — a `columnheader` (the `<th>` above the row
/// header band) holding a `checkbox` (the `<input type=checkbox>` inside it) —
/// because that is what the corner *is*, and because a bare `checkbox` under a
/// `row` would be a cell that is not a cell. The checkbox carries the tri-state
/// on WAI-ARIA's own axes: [`AccessState::checked`](crate::node::AccessState)
/// for the definite legs, `mixed` for `aria-checked="mixed"`.
///
/// # What the toolkit does not have here
///
/// table corner button is a private abstract button built inside
/// `qtableview.cpp` with **no text and no accessible name**, and table view
/// exposes only `setCornerButtonEnabled(bool)` / `isCornerButtonEnabled()` —
/// no accessor for the button — so there is no supported way to name it. A
/// screen-reader user meets an unnamed button whose state is not reported
/// because the button has no state: pressing it always runs `selectAll()`.
/// Here it is named, and it reports **and** takes back what it did.
///
/// A pass over the built nodes, by the [`attach_row_headers`] precedent and for
/// its reason: the corner then reaches every grid topology from one call rather
/// than the one builder a parameter would have been threaded into.
///
/// - `header_tag` — the header row the corner leads, the same
///   [`GridTag::header_row`] the builders used.
/// - `extent` — what the control shows, and therefore what its press will do.
pub fn attach_corner_button(
    nodes: &mut Vec<AccessNode>,
    grid_tag: &str,
    header_tag: &str,
    extent: SelectionExtent,
) {
    let Some(header_row) = nodes.iter_mut().find(|n| n.tag == header_tag) else {
        return;
    };
    let corner_tag = GridTag::header_corner(grid_tag);
    let toggle_tag = format!("{grid_tag}#{}", GridSendKey::Corner.encode());
    // Leading, because the corner sits left of the first column header — the
    // order the band is painted in and the order an AT reads.
    header_row.children.insert(0, corner_tag.clone());
    // R1692 — the corner is a `columnheader`, and a column header with no name
    // is a column a reader cannot identify. It has no text of its own to be
    // named from either: what it paints is the select-all mark, and that glyph
    // is declared presentational precisely so it is not read as a name. So the
    // name is authored here, once, for every grid topology — and it is what the
    // column HEADS rather than what the control inside it does, which the child
    // already says.
    let mut corner = AccessNode::new(corner_tag, AriaRole::ColumnHeader).with_name("Row selection");
    corner.children.push(toggle_tag.clone());
    let mut toggle = AccessNode::new(toggle_tag, AriaRole::CheckBox).with_name("Select all");
    // `checked` and `mixed` are separate axes, exactly as in the DOM: the mixed
    // leg is not a third value of `checked`, so an AT that understands only
    // `checked` still hears something true rather than a guess.
    toggle.state.checked = Some(extent == SelectionExtent::All);
    toggle.state.mixed = extent == SelectionExtent::Partial;
    nodes.push(corner);
    nodes.push(toggle);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R1547 — the test grid's WIDTH. It was the labels until R1547; the
    /// builders no longer read one, because the `columnheader`s are named from
    /// the painted header band.
    const NCOLS: usize = 3;

    fn window(first: usize, count: usize) -> VisibleWindow {
        VisibleWindow { first, count }
    }

    // ── R1523 column-axis windowing ─────────────────────────────────

    /// The 200-column table's width. Only a handful of its columns are ever in
    /// the tree.
    const WIDE_NCOLS: usize = 200;

    /// The two-axis contract: the tree holds the windowed columns, and says how
    /// many columns they were drawn from.
    #[test]
    fn r1523_wide_grid_windows_columns_and_declares_the_full_extent() {
        let cols = window(50, 5);
        let nodes = windowed_grid_nodes_wide(
            "vcol",
            "Wide grid",
            cols.count,
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
            columnheaders[0].column_index,
            Some(51),
            "the window starts at column 50 (one-based 51), not at column 0",
        );
        // R1547 — and it starts there without the builder being told what the
        // column is CALLED. The name is left for the paint derivation, so this
        // node is deliberately unnamed at construction; a name here would win
        // over the painted header and could differ from it.
        assert!(
            columnheaders.iter().all(|n| n.name.is_none()),
            "no columnheader is named by the builder — the painted header is",
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
        let nodes = windowed_grid_nodes_wide(
            "vcol",
            "G",
            cols.count,
            WIDE_NCOLS,
            10_000,
            &window(0, 1),
            &cols,
        );
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
        let nodes = windowed_grid_nodes_wide(
            "vcol",
            "G",
            cols.count,
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

    /// R1530 — the builder describes exactly the window it is handed, and the
    /// extent it is told, with no arithmetic between them.
    ///
    /// R1523 clamped a window that ran past the header list, because the two
    /// arrived separately (two reads of the same reactive state, so a frame
    /// that saw them disagree was reachable) and a bad slice would panic the
    /// a11y walker. There is no slice left to get wrong: the count IS the
    /// window. The out-of-range case is therefore not clamped here but simply
    /// absent — a caller past the end asks its model for nothing and hands over
    /// nothing — and the `aria-colcount` still contextualises whatever is left.
    #[test]
    fn r1530_builder_describes_the_window_it_is_given() {
        let nodes = windowed_grid_nodes_wide("vcol", "G", 1, 3, 10, &window(0, 1), &window(2, 1));
        let columnheaders: Vec<&AccessNode> = nodes
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .collect();
        assert_eq!(
            columnheaders.len(),
            1,
            "one column in, one columnheader out"
        );
        assert_eq!(
            columnheaders[0].column_index,
            Some(3),
            "placed at the section the window says, not at the label's offset",
        );
        assert_eq!(
            nodes[0].column_count,
            Some(3),
            "the extent is stated, not counted from the window"
        );
        // Nothing in range: no columns, no panic, extent intact.
        let none = windowed_grid_nodes_wide("vcol", "G", 0, 3, 10, &window(0, 1), &window(99, 0));
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
        let display = windowed_grid_nodes("vtbl", "G", NCOLS, 10, &window(0, 1));
        assert_eq!(display[0].column_count, Some(3));
        let selected = windowed_grid_nodes_selected("vtbl", "G", NCOLS, 10, &window(0, 1), Some(0));
        assert_eq!(selected[0].column_count, Some(3));
        let sorted =
            windowed_grid_nodes_sorted("vtbl", "G", NCOLS, &[0, 1, 2], None, None, &window(0, 1));
        assert_eq!(sorted[0].column_count, Some(3));
        let frozen = windowed_grid_nodes_frozen("vtbl", "G", NCOLS, 10, &window(0, 1));
        assert_eq!(frozen[0].column_count, Some(3));
    }

    #[test]
    fn emits_grid_header_and_windowed_rows() {
        let nodes = windowed_grid_nodes("vtbl", "Data grid", NCOLS, 10_000, &window(0, 2));
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
        let nodes = windowed_grid_nodes("vtbl", "G", NCOLS, 10_000, &window(100, 2));
        let first_data = nodes
            .iter()
            .find(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow")
            .expect("a data row");
        assert_eq!(first_data.tag, "vtbl_row100");
        assert_eq!(first_data.position_in_set, Some(101));
    }

    #[test]
    fn display_only_omits_aria_selected() {
        let nodes = windowed_grid_nodes("vtbl", "G", NCOLS, 10_000, &window(0, 2));
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
        let nodes = windowed_grid_nodes("vtbl", "G", NCOLS, 10_000, &window(0, 2));
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
        let nodes = windowed_grid_nodes_frozen("gfz", "Frozen grid", NCOLS, 10_000, &window(0, 2));
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
        let plain = windowed_grid_nodes("gfz", "G", NCOLS, 10_000, &window(0, 2));
        let frozen = windowed_grid_nodes_frozen("gfz", "G", NCOLS, 10_000, &window(0, 2));
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
            windowed_grid_nodes_selected("vtbl", "G", NCOLS, 10_000, &window(100, 2), Some(101));
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
        let plain = windowed_grid_nodes("vtbl", "G", NCOLS, 10_000, &window(0, 2));
        let decorated =
            windowed_grid_nodes_selected("vtbl", "G", NCOLS, 10_000, &window(0, 2), None);
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
            windowed_grid_nodes_selected("vtbl", "G", NCOLS, 10_000, &window(0, 2), Some(9_999));
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
        let nodes =
            windowed_grid_nodes_multiselected("vtbl", "G", NCOLS, 10_000, &window(100, 4), &|i| {
                selection.contains(&i)
            });
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
        let empty = std::collections::BTreeSet::<usize>::new();
        let plain = windowed_grid_nodes("vtbl", "G", NCOLS, 10_000, &window(0, 2));
        let decorated =
            windowed_grid_nodes_multiselected("vtbl", "G", NCOLS, 10_000, &window(0, 2), &|i| {
                empty.contains(&i)
            });
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
            NCOLS,
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
            NCOLS,
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
    fn r1544_editability_marks_only_the_non_editable_cells() {
        let window = VisibleWindow { first: 0, count: 2 };
        let mut nodes = windowed_grid_nodes("vtbl", "G", NCOLS, 10, &window);
        // Column 0 is the read-only identity column.
        mark_grid_editability(&mut nodes, "vtbl", &window, 0..NCOLS, |c| c.col != 0);
        let read_only = |row: usize, col: usize| {
            nodes
                .iter()
                .find(|n| n.tag == cell_tag("vtbl", row, col))
                .expect("cell present")
                .state
                .read_only
        };
        assert!(read_only(0, 0), "the fixed column says so to AT");
        assert!(read_only(1, 0));
        for col in 1..NCOLS {
            assert!(
                !read_only(0, col),
                "an editable cell stays silent — aria-readonly defaults to false"
            );
        }
        // Nothing but gridcells is touched: the grid container and the header
        // cells carry no editability claim.
        assert!(!nodes[0].state.read_only, "the grid container is untouched");
        assert!(
            nodes
                .iter()
                .filter(|n| n.role == AriaRole::ColumnHeader)
                .all(|n| !n.state.read_only),
            "a column header is not a cell"
        );
    }

    #[test]
    fn r1544_editability_skips_cells_outside_the_built_window() {
        // The orphan-free rule: asking about rows the tree does not contain
        // adds nothing rather than inventing nodes for them.
        let built = VisibleWindow { first: 0, count: 1 };
        let mut nodes = windowed_grid_nodes("vtbl", "G", NCOLS, 10, &built);
        let before = nodes.len();
        let wider = VisibleWindow { first: 0, count: 5 };
        mark_grid_editability(&mut nodes, "vtbl", &wider, 0..NCOLS + 4, |_| false);
        assert_eq!(nodes.len(), before, "no node is created by the pass");
        assert!(
            nodes
                .iter()
                .find(|n| n.tag == cell_tag("vtbl", 0, 0))
                .expect("present cell")
                .state
                .read_only,
            "the cells that DO exist are still marked"
        );
    }

    #[test]
    fn r1555_the_open_editor_is_announced_with_its_form_s_role() {
        // Every form's role is decided by the datum's kind, not by which
        // widget a factory happened to construct — which is why a toolkit bool
        // cell announces as a combo box and a toolkit colour cell announces
        // nothing.
        let expected = [
            (EditorForm::Field, AriaRole::TextInput),
            (EditorForm::Stepper, AriaRole::SpinButton),
            (EditorForm::Toggle, AriaRole::CheckBox),
            (EditorForm::Selector, AriaRole::ComboBox),
            (EditorForm::Swatch, AriaRole::TextInput),
        ];
        assert_eq!(
            expected.len(),
            EditorForm::ALL.len(),
            "the census drives the table, so a sixth form fails here rather \
             than announcing as whatever the fallback was"
        );
        for (form, role) in expected {
            assert_eq!(editor_role(form), role, "{form:?}");
            let window = VisibleWindow { first: 0, count: 2 };
            let mut nodes = windowed_grid_nodes("vtbl", "G", NCOLS, 10, &window);
            let before = nodes.len();
            let at = CellIndex { row: 1, col: 2 };
            attach_cell_editor(&mut nodes, "vtbl", at, form, "Score");
            assert_eq!(nodes.len(), before + 1, "exactly one editor node");
            let editor_tag = format!("{}#editor", cell_tag("vtbl", 1, 2));
            let editor = nodes
                .iter()
                .find(|n| n.tag == editor_tag)
                .expect("the editor node");
            assert_eq!(editor.role, role);
            assert_eq!(editor.name.as_deref(), Some("Score"));
            // The host stays a gridcell — that is what keeps row / column
            // geometry intact for AT table navigation — and claims the editor.
            let host = nodes
                .iter()
                .find(|n| n.tag == cell_tag("vtbl", 1, 2))
                .expect("host cell");
            assert_eq!(host.role, AriaRole::GridCell);
            assert!(host.children.contains(&editor_tag));
        }
    }

    #[test]
    fn r1555_an_editor_on_a_windowed_out_cell_emits_nothing() {
        // The `attach_child_button` orphan-free rule: no link, and no node, so
        // an editor node with no host cannot exist.
        let built = VisibleWindow { first: 0, count: 1 };
        let mut nodes = windowed_grid_nodes("vtbl", "G", NCOLS, 10, &built);
        let before = nodes.len();
        attach_cell_editor(
            &mut nodes,
            "vtbl",
            CellIndex { row: 40, col: 0 },
            EditorForm::Field,
            "Name",
        );
        assert_eq!(nodes.len(), before, "scrolled away: nothing is emitted");
        assert!(
            nodes
                .iter()
                .all(|n| !n.children.iter().any(|c| c.ends_with("#editor"))),
            "and no host claims an editor that does not exist"
        );
    }

    #[test]
    fn multiselect_empty_window_yields_grid_and_header_only() {
        let selection: std::collections::BTreeSet<usize> = [0, 5].into_iter().collect();
        let nodes = windowed_grid_nodes_multiselected(
            "vtbl",
            "G",
            NCOLS,
            10_000,
            &VisibleWindow::EMPTY,
            &|i| selection.contains(&i),
        );
        assert!(nodes[0].multiselectable, "the container axis still applies");
        let data_rows = nodes
            .iter()
            .filter(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow")
            .count();
        assert_eq!(data_rows, 0, "an empty window has no data-row nodes");
    }

    // ── R1548 §5.40 §5.27 — the vertical header axis ────────────────

    /// R1562 — the corner reaches the tree as HTML's own shape: a
    /// `columnheader` leading the header row, holding a **named** tri-state
    /// `checkbox`. Every extent maps to the `aria-checked` value table.
    #[test]
    fn r1562_the_corner_is_a_named_tri_state_control() {
        for (extent, checked, mixed) in [
            (SelectionExtent::Empty, Some(false), false),
            (SelectionExtent::Partial, Some(false), true),
            (SelectionExtent::All, Some(true), false),
        ] {
            let mut nodes = windowed_grid_nodes("g", "Grid", NCOLS, 100, &window(0, 3));
            let header = GridTag::header_row("g");
            attach_corner_button(&mut nodes, "g", &header, extent);
            let corner_tag = GridTag::header_corner("g");
            let toggle_tag = format!("g#{}", GridSendKey::Corner.encode());
            let row = nodes
                .iter()
                .find(|n| n.tag == header)
                .expect("the header row");
            assert_eq!(
                row.children.first().map(String::as_str),
                Some(corner_tag.as_str()),
                "the corner leads the header row, as it is painted",
            );
            let corner = nodes
                .iter()
                .find(|n| n.tag == corner_tag)
                .expect("the corner node");
            assert_eq!(corner.role, AriaRole::ColumnHeader);
            assert_eq!(corner.children, vec![toggle_tag.clone()]);
            // R1692 — and it says which column it is. The mark it paints is
            // declared presentational, so there is nothing to derive a name
            // from and an unnamed `columnheader` is what a reader gets.
            assert_eq!(corner.name.as_deref(), Some("Row selection"));
            let toggle = nodes
                .iter()
                .find(|n| n.tag == toggle_tag)
                .expect("the toggle node");
            assert_eq!(toggle.role, AriaRole::CheckBox);
            assert_eq!(
                toggle.name.as_deref(),
                Some("Select all"),
                "the toolkit's table corner button has no text and no accessor to give \
                 it one, so it announces as an unnamed button",
            );
            assert_eq!(toggle.state.checked, checked, "{extent:?}");
            assert_eq!(toggle.state.mixed, mixed, "{extent:?}");
        }
    }

    /// R1562 — a grid whose band was never attached gains no corner: the
    /// negative control that keeps the assertions above from passing on a
    /// builder that always emits one.
    #[test]
    fn r1562_a_grid_with_no_band_has_no_corner() {
        let nodes = windowed_grid_nodes("g", "Grid", NCOLS, 100, &window(0, 3));
        assert!(
            !nodes
                .iter()
                .any(|n| n.tag == GridTag::header_corner("g") || n.role == AriaRole::CheckBox),
            "no corner and no checkbox",
        );
    }

    /// The seam: every windowed row leads with a `rowheader`, and a node for it
    /// exists.
    #[test]
    fn r1548_every_windowed_row_leads_with_a_rowheader() {
        let rows = window(40, 3);
        let mut nodes = windowed_grid_nodes("vtbl", "G", NCOLS, 10_000, &rows);
        attach_row_headers(&mut nodes, "vtbl", &rows, |view_pos| view_pos);
        for id in rows.indices() {
            let header_tag = GridTag::row_header("vtbl", id);
            let row = nodes
                .iter()
                .find(|n| n.tag == GridTag::data_row("vtbl", id))
                .expect("the data row");
            assert_eq!(
                row.children.first().map(String::as_str),
                Some(header_tag.as_str()),
                "the rowheader is the row's FIRST child — it labels the cells \
                 that follow it, which is the order an AT walks",
            );
            let header = nodes
                .iter()
                .find(|n| n.tag == header_tag)
                .expect("the rowheader node");
            assert_eq!(header.role, AriaRole::RowHeader);
            assert!(
                header.name.is_none(),
                "and it carries NO name: R1547's rule on the second axis — the \
                 announced string is derived from the band that was painted",
            );
        }
    }

    /// **Why a pass and not a seventh builder.** The axis reaches a topology
    /// this function has never heard of — the permuted one, built by a
    /// different implementation — with no flag threaded anywhere.
    #[test]
    fn r1548_the_axis_composes_with_the_sorted_topology() {
        let order: Vec<usize> = (0..100).rev().collect();
        let rows = window(0, 3);
        let mut nodes = windowed_grid_nodes_sorted("g", "G", NCOLS, &order, None, None, &rows);
        attach_row_headers(&mut nodes, "g", &rows, |view_pos| order[view_pos]);
        // Visual position 0 is data row 99: the header is addressed by the
        // row's identity, exactly as its strip and its cells are.
        let row = nodes
            .iter()
            .find(|n| n.tag == GridTag::data_row("g", 99))
            .expect("the top row is data row 99");
        assert_eq!(
            row.children.first().map(String::as_str),
            Some(GridTag::row_header("g", 99).as_str()),
        );
        assert!(
            !nodes.iter().any(|n| n.tag == GridTag::row_header("g", 0)),
            "and data row 0, which is not in the window, has no header node",
        );
    }

    /// Orphan-free: a row the tree does not hold gets no header node, rather
    /// than a node pointing at nothing.
    #[test]
    fn r1548_a_row_outside_the_tree_is_skipped_not_invented() {
        let painted = window(10, 2);
        let mut nodes = windowed_grid_nodes("vtbl", "G", NCOLS, 10_000, &painted);
        // A window WIDER than the one the tree was built from.
        attach_row_headers(&mut nodes, "vtbl", &window(10, 5), |view_pos| view_pos);
        let headers = nodes
            .iter()
            .filter(|n| n.role == AriaRole::RowHeader)
            .count();
        assert_eq!(
            headers, painted.count,
            "one header per row that EXISTS, not per row that was asked about",
        );
    }

    /// The negative half: a grid whose model answers no vertical axis never
    /// calls this, and its tree is byte-identical to the pre-R1548 one.
    #[test]
    fn r1548_an_unheaded_grid_has_no_rowheader_at_all() {
        let rows = window(0, 4);
        let nodes = windowed_grid_nodes("vtbl", "G", NCOLS, 10_000, &rows);
        assert!(
            !nodes.iter().any(|n| n.role == AriaRole::RowHeader),
            "no rowheader node",
        );
        for id in rows.indices() {
            let row = nodes
                .iter()
                .find(|n| n.tag == GridTag::data_row("vtbl", id))
                .expect("the data row");
            assert_eq!(
                row.children.first().map(String::as_str),
                Some(cell_tag("vtbl", id, 0).as_str()),
                "and the row still leads with its first gridcell",
            );
        }
    }
}
