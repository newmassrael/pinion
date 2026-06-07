//! AccessKit builder for a **WAI-ARIA `grid` (data table)** (R707 §5.40,
//! lifted R816): a labelled [`AriaRole::Grid`] container holding a header
//! [`AriaRole::Row`] of [`AriaRole::ColumnHeader`] cells (optionally
//! `aria-sort`) followed by data [`AriaRole::Row`]s of
//! [`AriaRole::GridCell`]s, the grid optionally `aria-multiselectable`.
//!
//! This is the shared a11y shape behind two consumers — `hello-table` (a
//! sortable single-select catalog) and `hello-table-multi` (a multi-select
//! catalog). Both built the identical grid → header row of column headers →
//! data rows of grid cells topology by hand, differing only in per-cell data,
//! the grid name, the `aria-multiselectable` flag, the optional `aria-sort`
//! header axis, and the row ordering. A divergence between them would be an
//! a11y bug, not a style choice — so the skeleton is lifted here as one
//! source of truth (the R758 a11y-axis "divergence-is-a-bug" rule).
//!
//! ## Owned cells (vs the flat-list `&str` builders)
//!
//! Unlike [`listbox_option_nodes`](crate::listbox_option_nodes) /
//! [`tablist_tab_nodes`](crate::tablist_tab_nodes), whose cells borrow
//! `&str` (their labels are static or left to scene enrichment), a grid
//! cell's accessible name is *computed per cell* (`"Header: value"`) and
//! the structure is two levels deep (rows own cells). Borrowing would force
//! the caller into nested `Vec<Vec<String>>` pre-materialisation, so the
//! builder structs own their `String`s — the caller maps cleanly without
//! lifetime gymnastics. (a11y emit is not a hot path; it already allocates
//! the `AccessNode` `String`s.)
//!
//! `aria-posinset` / `aria-setsize` for the data rows are derived from each
//! row's index in the passed slice, so the caller passes rows in **visual**
//! (display / sorted) order and the row numbering matches what the user
//! sees.

use crate::node::{AccessNode, AccessState};
use crate::role::{AriaRole, SortDirection};
use pinion_core::widgets::radio::RadioState;

/// A column header within a [`grid_table_nodes`] grid.
#[derive(Clone, Debug)]
pub struct GridColumn {
    /// The column header's tag.
    pub tag: String,
    /// Accessible name (the column title).
    pub label: String,
    /// `aria-sort` direction when this is the active sort column, else
    /// `None` (the attribute is omitted).
    pub sort: Option<SortDirection>,
}

/// A data cell within a [`GridRow`].
#[derive(Clone, Debug)]
pub struct GridCell {
    /// The cell's `External` tag (typically a `{primary}#{row}_{col}`
    /// composite).
    pub tag: String,
    /// Accessible name — usually `"Header: value"` so an AT user hears the
    /// column context with the value.
    pub name: String,
    /// `true` when this cell is the roving active descendant (the grid owns
    /// focus and this is the active cell).
    pub focused: bool,
}

/// A data row within a [`grid_table_nodes`] grid.
#[derive(Clone, Debug)]
pub struct GridRow {
    /// The row's tag (typically data-indexed so selection survives a sort).
    pub tag: String,
    /// `true` when this row is selected — lowered to `aria-selected` (many
    /// rows may be selected in a multi-select grid).
    pub selected: bool,
    /// Interaction posture shared by the row's cells (drives `disabled` /
    /// `hovered` / `pressed`).
    pub state: RadioState,
    /// The row's data cells, left-to-right.
    pub cells: Vec<GridCell>,
}

/// Build the `grid` + header `row` + `columnheader`s + data `row`s +
/// `gridcell`s.
///
/// The grid carries an explicit `name`, `aria-multiselectable` when
/// `multiselectable`, and references the header row plus every data row as
/// children. The header row references each column header; a column carries
/// `aria-sort` when [`GridColumn::sort`] is `Some`. Each data row carries
/// its `aria-selected` value, the `aria-posinset` / `aria-setsize` derived
/// from its index in `rows`, and references its cells; each cell carries
/// its name + the row's interaction state (`focused` on the active cell).
///
/// The returned vector is `[grid, header_row, col_0, ..., row_0, cell_0_0,
/// ..., row_1, ...]` — containers before their children, the flat-list
/// convention `lower_access_node` resolves into an AT subtree.
#[must_use]
pub fn grid_table_nodes(
    grid_tag: &str,
    grid_name: &str,
    multiselectable: bool,
    header_row_tag: &str,
    columns: &[GridColumn],
    rows: &[GridRow],
) -> Vec<AccessNode> {
    // grid + header row + N columnheaders + per row (1 + ncols).
    let cell_total: usize = rows.iter().map(|r| r.cells.len()).sum();
    let mut nodes: Vec<AccessNode> =
        Vec::with_capacity(2 + columns.len() + rows.len() + cell_total);

    // Grid root: children are the header row then every data row (in the
    // passed order = visual order).
    let mut grid = AccessNode::new(grid_tag, AriaRole::Grid).with_name(grid_name);
    if multiselectable {
        grid = grid.with_multiselectable();
    }
    grid = grid.with_child(header_row_tag);
    for row in rows {
        grid = grid.with_child(row.tag.as_str());
    }
    nodes.push(grid);

    // Header row + its columnheader cells.
    let mut header_row = AccessNode::new(header_row_tag, AriaRole::Row);
    for column in columns {
        header_row = header_row.with_child(column.tag.as_str());
    }
    nodes.push(header_row);
    for column in columns {
        let mut ch = AccessNode::new(column.tag.as_str(), AriaRole::ColumnHeader)
            .with_name(column.label.as_str());
        if let Some(dir) = column.sort {
            ch = ch.with_sort(dir);
        }
        nodes.push(ch);
    }

    // Data rows + their gridcells.
    let set_size = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    for (i, row) in rows.iter().enumerate() {
        let mut row_node = AccessNode::new(row.tag.as_str(), AriaRole::Row)
            .with_selected(row.selected)
            .with_position_in_set(u32::try_from(i + 1).unwrap_or(u32::MAX))
            .with_size_of_set(set_size);
        for cell in &row.cells {
            row_node = row_node.with_child(cell.tag.as_str());
        }
        nodes.push(row_node);
        for cell in &row.cells {
            nodes.push(
                AccessNode::new(cell.tag.as_str(), AriaRole::GridCell)
                    .with_name(cell.name.as_str())
                    .with_state(AccessState {
                        focused: cell.focused,
                        ..AccessState::from_interaction(row.state, None)
                    }),
            );
        }
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> Vec<GridColumn> {
        vec![
            GridColumn { tag: "g_ch0".into(), label: "Name".into(), sort: Some(SortDirection::Ascending) },
            GridColumn { tag: "g_ch1".into(), label: "Tier".into(), sort: None },
        ]
    }

    fn rows() -> Vec<GridRow> {
        vec![
            GridRow {
                tag: "g_row0".into(),
                selected: true,
                state: RadioState::Idle,
                cells: vec![
                    GridCell { tag: "g#0_0".into(), name: "Name: Button".into(), focused: true },
                    GridCell { tag: "g#0_1".into(), name: "Tier: 1".into(), focused: false },
                ],
            },
            GridRow {
                tag: "g_row1".into(),
                selected: false,
                state: RadioState::Hover,
                cells: vec![
                    GridCell { tag: "g#1_0".into(), name: "Name: Slider".into(), focused: false },
                    GridCell { tag: "g#1_1".into(), name: "Tier: 2".into(), focused: false },
                ],
            },
        ]
    }

    fn nodes() -> Vec<AccessNode> {
        grid_table_nodes("g", "Catalog", false, "g_hrow", &columns(), &rows())
    }

    fn by_tag<'a>(nodes: &'a [AccessNode], tag: &str) -> &'a AccessNode {
        nodes.iter().find(|n| n.tag == tag).expect("node present")
    }

    #[test]
    fn emits_grid_header_columns_rows_cells() {
        let n = nodes();
        // grid + header row + 2 columnheaders + 2 rows + 4 cells = 11.
        assert_eq!(n.len(), 1 + 1 + 2 + 2 + 4);
        assert_eq!(n[0].role, AriaRole::Grid);
        assert_eq!(n[0].name.as_deref(), Some("Catalog"));
        assert_eq!(by_tag(&n, "g_hrow").role, AriaRole::Row);
        assert_eq!(by_tag(&n, "g_ch0").role, AriaRole::ColumnHeader);
        assert_eq!(by_tag(&n, "g_row0").role, AriaRole::Row);
        assert_eq!(by_tag(&n, "g#0_0").role, AriaRole::GridCell);
    }

    #[test]
    fn grid_references_header_row_then_data_rows() {
        let n = nodes();
        assert_eq!(n[0].children, vec!["g_hrow", "g_row0", "g_row1"]);
        // The header row references its columnheaders; data rows reference cells.
        assert_eq!(by_tag(&n, "g_hrow").children, vec!["g_ch0", "g_ch1"]);
        assert_eq!(by_tag(&n, "g_row0").children, vec!["g#0_0", "g#0_1"]);
    }

    #[test]
    fn multiselectable_flag_lowers_on_the_grid_root() {
        let single = nodes();
        assert!(!single[0].multiselectable);
        let multi = grid_table_nodes("g", "Catalog", true, "g_hrow", &columns(), &rows());
        assert!(multi[0].multiselectable);
    }

    #[test]
    fn active_sort_column_carries_aria_sort_others_omit() {
        let n = nodes();
        assert_eq!(by_tag(&n, "g_ch0").sort, Some(SortDirection::Ascending));
        assert_eq!(by_tag(&n, "g_ch1").sort, None);
    }

    #[test]
    fn rows_carry_selected_and_posinset_over_the_slice() {
        let n = nodes();
        let r0 = by_tag(&n, "g_row0");
        assert_eq!(r0.selected, Some(true));
        assert_eq!((r0.position_in_set, r0.size_of_set), (Some(1), Some(2)));
        let r1 = by_tag(&n, "g_row1");
        assert_eq!(r1.selected, Some(false));
        assert_eq!((r1.position_in_set, r1.size_of_set), (Some(2), Some(2)));
    }

    #[test]
    fn cells_carry_name_and_focused_state() {
        let n = nodes();
        assert_eq!(by_tag(&n, "g#0_0").name.as_deref(), Some("Name: Button"));
        assert!(by_tag(&n, "g#0_0").state.focused, "the active cell is focused");
        assert!(!by_tag(&n, "g#0_1").state.focused);
        // Row interaction posture flows to its cells (row 1 = Hover).
        assert!(by_tag(&n, "g#1_0").state.hovered);
        // Grid cells never carry aria-checked (membership = aria-selected on rows).
        assert!(n.iter().all(|node| node.state.checked.is_none()));
    }

    #[test]
    fn empty_rows_emit_grid_and_header_only() {
        let n = grid_table_nodes("g", "Catalog", false, "g_hrow", &columns(), &[]);
        // grid + header row + 2 columnheaders = 4.
        assert_eq!(n.len(), 4);
        assert_eq!(n[0].children, vec!["g_hrow"], "grid references only the header row");
    }
}
