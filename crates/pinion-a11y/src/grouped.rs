//! R847 §5.50 §5.27 — WAI-ARIA `AccessNode` builders for a **grouped** Model/View
//! collection (the R843 [`GroupRow`] flatten), in two presentations: a grouped
//! **list** ([`grouped_tree_access_nodes`], a one-level `tree`) and a grouped
//! **grid** ([`grouped_grid_access_nodes`], a `grid` with spanning group-header
//! rows).
//!
//! Lifted at the second consumer of each shape (R847 audit): `hello-grouped-list`
//! and `hello-grouped-sort` both build the grouped-list `tree` topology, while
//! `hello-grouped-grid` and `hello-grouped-grid-sort` both build the
//! grouped-grid `grid` topology. By the project's a11y doctrine — *a divergence between two
//! consumers' AT topology is a bug, not a style choice; lift at the second
//! consumer* ([`tree_access_nodes`](crate::tree_access_nodes) /
//! [`grid_table_nodes`](crate::grid_table_nodes) / `windowed_*_nodes` all did) —
//! these belong here as one source of truth. The grid pair had **already**
//! diverged by hand (one carried `aria-sort` + a different header-tag scheme,
//! the other did not); expressing the per-column tag/label/`aria-sort` as a
//! [`GridColumn`] slice (the [`grid_table_nodes`] convention) turns that
//! divergence into *data* the consumer supplies, while the builder owns the
//! load-bearing topology decision.
//!
//! ## What the builder owns vs the consumer owns
//!
//! The builder owns the **AT decision** every grouped collection must agree on:
//! the roles (`tree`/`treeitem`; `grid`/`row`/`columnheader`/`gridcell`), the
//! `aria-level` (header = 1, data = 2 in the list), `aria-expanded` on group
//! headers, `aria-selected` on the selected data row, the
//! `aria-posinset`/`aria-setsize`-over-the-window convention, and the header
//! name format `"<group> (<count>)"`. The consumer owns only what genuinely
//! differs: the composite **tag namespaces** (group headers vs data rows route
//! to different coordinators) and the display **labels**, supplied as a small
//! spec + label closures.

use crate::grid::GridColumn;
use crate::node::AccessNode;
use crate::role::AriaRole;
use pinion_core::widgets::group_order::GroupRow;
use pinion_core::widgets::virtual_list::VisibleWindow;

/// The addressing config of a grouped **list** (`tree`) a11y tree. The labels
/// are supplied as closures to [`grouped_tree_access_nodes`]; this carries the
/// non-closure config (tags + the selected source).
pub struct GroupedTreeSpec<'a> {
    /// The `role=tree` container tag (the focusable element / tab stop).
    pub tree_tag: &'a str,
    /// Optional accessible name for the tree container.
    pub name: Option<&'a str>,
    /// Composite-tag namespace of group headers — header `g` is
    /// `"{header_prefix}#{g}"` (routes to the group-collapse coordinator).
    pub header_prefix: &'a str,
    /// Composite-tag namespace of data rows — source `s` is
    /// `"{data_prefix}#{s}"` (routes to the selection coordinator).
    pub data_prefix: &'a str,
    /// The selected **source** index, or `None`. The matching data row carries
    /// `aria-selected`.
    pub selected_source: Option<usize>,
}

/// R847 §5.50 — build the grouped-**list** `tree` a11y nodes over the visible
/// window of a [`GroupRow`] flatten.
///
/// Emits `[tree, ...windowed treeitems]`: a `role=tree` root referencing every
/// windowed row, then per visible row a `role=treeitem` — a **group header** at
/// `aria-level = 1` with `aria-expanded` and name `"<group> (<count>)"`, or a
/// **data row** at `aria-level = 2` with `aria-selected` and name `data_label`.
/// `aria-posinset` is the visual position (1-based) and `aria-setsize` the
/// flattened row count, the windowed-list convention.
///
/// `group_label(group)` yields the group's display name (the builder appends
/// the `" (<count>)"`); `data_label(source)` yields the data row's name. A
/// standalone sibling node (e.g. a sort-control button) is the consumer's to
/// prepend/append around this result.
#[must_use]
pub fn grouped_tree_access_nodes(
    spec: &GroupedTreeSpec,
    rows: &[GroupRow],
    window: VisibleWindow,
    group_label: impl Fn(usize) -> String,
    data_label: impl Fn(usize) -> String,
) -> Vec<AccessNode> {
    let total = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    let row_tag = |row: &GroupRow| match *row {
        GroupRow::Header { group, .. } => format!("{}#{group}", spec.header_prefix),
        GroupRow::Data { source } => format!("{}#{source}", spec.data_prefix),
    };

    let mut nodes: Vec<AccessNode> = Vec::with_capacity(window.count + 1);
    let mut tree = AccessNode::new(spec.tree_tag, AriaRole::Tree).with_size_of_set(total);
    if let Some(name) = spec.name {
        tree = tree.with_name(name);
    }
    for view_pos in window.indices() {
        if let Some(row) = rows.get(view_pos) {
            tree = tree.with_child(row_tag(row));
        }
    }
    nodes.push(tree);

    for view_pos in window.indices() {
        let Some(row) = rows.get(view_pos) else { continue };
        let posinset = u32::try_from(view_pos + 1).unwrap_or(u32::MAX);
        let node = match *row {
            GroupRow::Header { group, member_count, collapsed } => {
                AccessNode::new(row_tag(row), AriaRole::TreeItem)
                    .with_name(format!("{} ({member_count})", group_label(group)))
                    .with_level(1)
                    .with_position_in_set(posinset)
                    .with_size_of_set(total)
                    .with_expanded(!collapsed)
            }
            GroupRow::Data { source } => AccessNode::new(row_tag(row), AriaRole::TreeItem)
                .with_name(data_label(source))
                .with_level(2)
                .with_position_in_set(posinset)
                .with_size_of_set(total)
                .with_selected(spec.selected_source == Some(source)),
        };
        nodes.push(node);
    }
    nodes
}

/// The addressing config of a grouped **grid** a11y tree. Columns are a
/// [`GridColumn`] slice (tag + label + optional `aria-sort`) — the same
/// caller-owned-columns convention as [`grid_table_nodes`], so a *sortable*
/// grid and a static one differ only in the slice they pass, not in code.
pub struct GroupedGridSpec<'a> {
    /// The `role=grid` container tag.
    pub grid_tag: &'a str,
    /// Optional accessible name for the grid.
    pub name: Option<&'a str>,
    /// The column-header `role=row` tag.
    pub header_row_tag: &'a str,
    /// The columns (tag + label + active `aria-sort`), left to right.
    pub columns: &'a [GridColumn],
    /// Composite-tag namespace of group headers (`"{group_prefix}#{g}"`).
    pub group_prefix: &'a str,
    /// Composite-tag namespace of data rows (`"{data_prefix}#{s}"`).
    pub data_prefix: &'a str,
    /// The selected **source** index, or `None`.
    pub selected_source: Option<usize>,
}

/// R847 §5.50 — build the grouped-**grid** `grid` a11y nodes over the visible
/// window of a [`GroupRow`] flatten.
///
/// Emits `[grid, header_row, ...columnheaders, ...windowed rows + gridcells]`:
/// a `role=grid` root referencing the header row + every windowed row; a
/// `role=row` header carrying one `role=columnheader` per [`GridColumn`] (with
/// `aria-sort` when the column's [`GridColumn::sort`] is `Some`); then per
/// visible row a spanning **group-header** `role=row` (`aria-expanded`, name
/// `"<group> (<count>)"`) or a **data** `role=row` (`aria-selected`) whose
/// `role=gridcell` children are named `"<column>: <value>"`.
///
/// `group_label(group)` yields the group's display name; `cell_tag(source, col)`
/// the per-cell composite tag (a `pointer_transparent` paint node carries its
/// bound); `cell_value(source, col)` the cell's value (the builder prefixes the
/// column label).
#[must_use]
pub fn grouped_grid_access_nodes(
    spec: &GroupedGridSpec,
    rows: &[GroupRow],
    window: VisibleWindow,
    group_label: impl Fn(usize) -> String,
    cell_tag: impl Fn(usize, usize) -> String,
    cell_value: impl Fn(usize, usize) -> String,
) -> Vec<AccessNode> {
    let total = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    let row_tag = |row: &GroupRow| match *row {
        GroupRow::Header { group, .. } => format!("{}#{group}", spec.group_prefix),
        GroupRow::Data { source } => format!("{}#{source}", spec.data_prefix),
    };

    let mut nodes: Vec<AccessNode> = Vec::with_capacity(window.count * (spec.columns.len() + 1) + 2);

    // Grid root: header row then every windowed row.
    let mut grid = AccessNode::new(spec.grid_tag, AriaRole::Grid);
    if let Some(name) = spec.name {
        grid = grid.with_name(name);
    }
    grid = grid.with_child(spec.header_row_tag);
    for view_pos in window.indices() {
        if let Some(row) = rows.get(view_pos) {
            grid = grid.with_child(row_tag(row));
        }
    }
    nodes.push(grid);

    // Header row + its column headers (aria-sort from the GridColumn slice).
    let mut header_row = AccessNode::new(spec.header_row_tag, AriaRole::Row);
    for column in spec.columns {
        header_row = header_row.with_child(column.tag.as_str());
    }
    nodes.push(header_row);
    for column in spec.columns {
        let mut col = AccessNode::new(column.tag.as_str(), AriaRole::ColumnHeader).with_name(&column.label);
        if let Some(dir) = column.sort {
            col = col.with_sort(dir);
        }
        nodes.push(col);
    }

    // Windowed rows: spanning group headers + data rows with gridcells.
    for view_pos in window.indices() {
        let Some(row) = rows.get(view_pos) else { continue };
        let posinset = u32::try_from(view_pos + 1).unwrap_or(u32::MAX);
        match *row {
            GroupRow::Header { group, member_count, collapsed } => {
                nodes.push(
                    AccessNode::new(row_tag(row), AriaRole::Row)
                        .with_name(format!("{} ({member_count})", group_label(group)))
                        .with_position_in_set(posinset)
                        .with_size_of_set(total)
                        .with_expanded(!collapsed),
                );
            }
            GroupRow::Data { source } => {
                let mut data_row = AccessNode::new(row_tag(row), AriaRole::Row)
                    .with_position_in_set(posinset)
                    .with_size_of_set(total)
                    .with_selected(spec.selected_source == Some(source));
                for c in 0..spec.columns.len() {
                    data_row = data_row.with_child(cell_tag(source, c));
                }
                nodes.push(data_row);
                for (c, column) in spec.columns.iter().enumerate() {
                    nodes.push(
                        AccessNode::new(cell_tag(source, c), AriaRole::GridCell)
                            .with_name(format!("{}: {}", column.label, cell_value(source, c))),
                    );
                }
            }
        }
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::{
        grouped_grid_access_nodes, grouped_tree_access_nodes, GroupedGridSpec, GroupedTreeSpec,
    };
    use crate::grid::GridColumn;
    use crate::role::{AriaRole, SortDirection};
    use pinion_core::widgets::group_order::{group_rows, GroupRow};
    use pinion_core::widgets::virtual_list::compute_visible_range;

    // A small 2-group flatten: group 0 = sources 0,2; group 1 = source 1.
    fn sample() -> Vec<GroupRow> {
        // order 0,1,2 ; group_of = i % 2 ; nothing collapsed.
        group_rows(&[0, 1, 2], |s| s % 2, |_| false)
    }

    fn full_window(rows: &[GroupRow]) -> pinion_core::widgets::virtual_list::VisibleWindow {
        // pitch 1, viewport tall enough for every row.
        compute_visible_range(0, u32::try_from(rows.len()).unwrap() + 1, rows.len(), 1, 0)
    }

    #[test]
    fn grouped_tree_emits_tree_expandable_headers_and_selected_data() {
        let rows = sample();
        let win = full_window(&rows);
        let spec = GroupedTreeSpec {
            tree_tag: "list",
            name: Some("Grouped"),
            header_prefix: "grp",
            data_prefix: "row",
            selected_source: Some(2),
        };
        let nodes = grouped_tree_access_nodes(
            &spec,
            &rows,
            win,
            |g| format!("G{g}"),
            |s| format!("item-{s}"),
        );
        // Root tree.
        assert_eq!(nodes[0].role, AriaRole::Tree);
        assert_eq!(nodes[0].size_of_set, Some(u32::try_from(rows.len()).unwrap()));
        // Group 0 header: treeitem, level 1, expanded, name "G0 (2)".
        let h0 = nodes.iter().find(|n| n.tag == "grp#0").expect("group 0 header");
        assert_eq!(h0.role, AriaRole::TreeItem);
        assert_eq!(h0.level, Some(1));
        assert_eq!(h0.expanded, Some(true));
        assert_eq!(h0.name.as_deref(), Some("G0 (2)"));
        // Data row source 2: treeitem level 2, selected, name "item-2".
        let d2 = nodes.iter().find(|n| n.tag == "row#2").expect("data row 2");
        assert_eq!(d2.role, AriaRole::TreeItem);
        assert_eq!(d2.level, Some(2));
        assert_eq!(d2.selected, Some(true));
        assert_eq!(d2.name.as_deref(), Some("item-2"));
        // A non-selected data row carries selected = Some(false).
        let d0 = nodes.iter().find(|n| n.tag == "row#0").expect("data row 0");
        assert_eq!(d0.selected, Some(false));
    }

    #[test]
    fn grouped_grid_emits_grid_columnheaders_group_rows_and_gridcells() {
        let rows = sample();
        let win = full_window(&rows);
        let columns = [
            GridColumn { tag: "c0".into(), label: "Name".into(), sort: Some(SortDirection::Ascending) },
            GridColumn { tag: "c1".into(), label: "Size".into(), sort: None },
        ];
        let spec = GroupedGridSpec {
            grid_tag: "grid",
            name: Some("Grouped grid"),
            header_row_tag: "head",
            columns: &columns,
            group_prefix: "grp",
            data_prefix: "row",
            selected_source: Some(0),
        };
        let nodes = grouped_grid_access_nodes(
            &spec,
            &rows,
            win,
            |g| format!("G{g}"),
            |s, c| format!("cell_{s}_{c}"),
            |s, c| format!("v{s}{c}"),
        );
        assert_eq!(nodes[0].role, AriaRole::Grid);
        // Two column headers; the active one carries aria-sort, the other none.
        let c0 = nodes.iter().find(|n| n.tag == "c0").expect("column 0");
        assert_eq!(c0.role, AriaRole::ColumnHeader);
        assert_eq!(c0.sort, Some(SortDirection::Ascending));
        let c1 = nodes.iter().find(|n| n.tag == "c1").expect("column 1");
        assert_eq!(c1.sort, None);
        // Group 0 header is an expandable Row.
        let h0 = nodes.iter().find(|n| n.tag == "grp#0").expect("group 0 header");
        assert_eq!(h0.role, AriaRole::Row);
        assert_eq!(h0.expanded, Some(true));
        // Data row source 0: Row, selected, with two gridcells named "Col: value".
        let d0 = nodes.iter().find(|n| n.tag == "row#0").expect("data row 0");
        assert_eq!(d0.role, AriaRole::Row);
        assert_eq!(d0.selected, Some(true));
        let cell00 = nodes.iter().find(|n| n.tag == "cell_0_0").expect("cell 0,0");
        assert_eq!(cell00.role, AriaRole::GridCell);
        assert_eq!(cell00.name.as_deref(), Some("Name: v00"));
        let cell01 = nodes.iter().find(|n| n.tag == "cell_0_1").expect("cell 0,1");
        assert_eq!(cell01.name.as_deref(), Some("Size: v01"));
    }
}
