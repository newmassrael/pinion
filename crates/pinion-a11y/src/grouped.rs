//! R847 §5.50 §5.27 — WAI-ARIA `AccessNode` builders for a **grouped** Model/View
//! collection (the R843 [`GroupRow`] flatten), in two presentations: a grouped
//! **list** ([`grouped_tree_access_nodes`], a one-level `tree`) and a grouped
//! **grid** ([`grouped_grid_access_nodes`], a two-level `treegrid` with spanning
//! group-header rows).
//!
//! Lifted at the second consumer of each shape (R847 audit): `hello-grouped-list`
//! and `hello-grouped-sort` both build the grouped-list `tree` topology, while
//! `hello-grouped-grid` and `hello-grouped-grid-sort` both build the
//! grouped-grid `treegrid` topology. By the project's a11y doctrine — *a divergence between two
//! consumers' AT topology is a bug, not a style choice; lift at the second
//! consumer* ([`tree_access_nodes`](crate::tree_access_nodes) /
//! [`grid_table_nodes`](crate::grid_table_nodes) / `windowed_*_nodes` all did) —
//! these belong here as one source of truth. The grid pair had **already**
//! diverged by hand (one carried `aria-sort` + a different header-tag scheme,
//! the other did not); expressing the per-column tag/label/`aria-sort` as a
//! [`GridColumn`] slice (the [`grid_table_nodes`](crate::grid_table_nodes) convention) turns that
//! divergence into *data* the consumer supplies, while the builder owns the
//! load-bearing topology decision.
//!
//! ## Why a `treegrid` (R874)
//!
//! A grouped grid is *hierarchical with columns*: expandable group-header rows
//! at `aria-level = 1` over data rows at `aria-level = 2`, each data row holding
//! one `gridcell` per column. That is precisely WAI-ARIA's
//! [`treegrid`](crate::AriaRole::TreeGrid), not a flat `grid` — a `grid` has no
//! disclosure axis. The grouped grid was first built as a flat `grid` only
//! because pinion had no `treegrid` role (the R845 note); R863 added the role
//! (its first consumer was the columned outliner [`treegrid_nodes`](crate::treegrid_nodes)),
//! so R874 lands the grouped grid as the role's second consumer — the
//! documented future-axis reaching its genuine user, not a flat-grid regression.
//!
//! ## What the builder owns vs the consumer owns
//!
//! The builder owns the **AT decision** every grouped collection must agree on:
//! the roles (`tree`/`treeitem`; `treegrid`/`row`/`columnheader`/`gridcell`), the
//! `aria-level` (header = 1, data = 2 in both presentations), `aria-expanded` on
//! group headers, whether the collection exposes an `aria-selected` axis at all
//! ([`GroupedGridSelection`] — a focus-only inspector carries none), the
//! `aria-posinset`/`aria-setsize`-over-the-window convention, and the header
//! name format `"<group> (<count>)"`. The consumer owns only what genuinely
//! differs: the composite **tag namespaces** (group headers vs data rows route
//! to different coordinators) and the display **labels**, supplied as a small
//! spec + label closures.

use crate::focus::AccessFocus;
use crate::grid::GridColumn;
use crate::node::AccessNode;
use crate::role::AriaRole;
use pinion_core::widgets::group_order::{GroupOrderState, GroupRow};
use pinion_core::widgets::virtual_list::VisibleWindow;

/// R850 §5.50 — the focus-ring target for a grouped collapsible collection's
/// roving keyboard cursor: rings the cursor's row (the `aria-activedescendant`)
/// when the container owns shell focus, else the container itself. The one SSOT
/// both grouped bindings (`hello-grouped-list` tree, `hello-grouped-grid` grid)
/// delegate their `access_focus_target` to — the access-focus peer of
/// [`group_nav_key`](pinion_core::widgets::group_order::group_nav_key) (R848
/// lifted the nav controller but left this ring-policy wrapper duplicated across
/// the two presentations; R850 closes that gap so the policy cannot diverge).
///
/// `tag` is the focusable container **and** the data-row composite prefix;
/// `group_prefix` is the group-header composite prefix. The `cursor` (a visual
/// position into the live `groups.rows()`) maps to its row's composite tag via
/// [`GroupRow::composite_tag`]; an unset / out-of-range cursor rings the
/// container.
#[must_use]
pub fn grouped_focus_target(
    groups: &GroupOrderState,
    tag: &str,
    group_prefix: &str,
    cursor: Option<usize>,
    focused: Option<&str>,
) -> Option<AccessFocus> {
    if focused != Some(tag) {
        return focused.map(AccessFocus::atomic);
    }
    let cursor_tag = cursor
        .and_then(|pos| groups.row_at(pos))
        .map(|row| row.composite_tag(group_prefix, tag));
    Some(cursor_tag.map_or_else(
        || AccessFocus::atomic(tag),
        |t| AccessFocus::composite(tag, t),
    ))
}

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
    /// R848 — the roving keyboard cursor's **visual position** (into `rows`),
    /// or `None`. The windowed row at that position carries the `focused`
    /// state (`aria-activedescendant`); orthogonal to `selected_source` (the
    /// cursor can rest on a group header). A grouped collection with no
    /// keyboard navigation passes `None`.
    pub focused_view_pos: Option<usize>,
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
    let row_tag = |row: &GroupRow| row.composite_tag(spec.header_prefix, spec.data_prefix);

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
        let Some(row) = rows.get(view_pos) else {
            continue;
        };
        let posinset = u32::try_from(view_pos + 1).unwrap_or(u32::MAX);
        let node = match *row {
            GroupRow::Header {
                group,
                member_count,
                collapsed,
            } => AccessNode::new(row_tag(row), AriaRole::TreeItem)
                .with_name(format!("{} ({member_count})", group_label(group)))
                .with_level(1)
                .with_position_in_set(posinset)
                .with_size_of_set(total)
                .with_expanded(!collapsed),
            GroupRow::Data { source } => AccessNode::new(row_tag(row), AriaRole::TreeItem)
                .with_name(data_label(source))
                .with_level(2)
                .with_position_in_set(posinset)
                .with_size_of_set(total)
                .with_selected(spec.selected_source == Some(source)),
        };
        // The roving cursor's row carries the active-descendant `focused` state.
        nodes.push(node.with_focused(spec.focused_view_pos == Some(view_pos)));
    }
    nodes
}

/// R874 §5.40 — whether a grouped grid exposes an `aria-selected` axis on its
/// data rows, and (if so) which source is selected.
///
/// Mirrors the [`virtual_grid`](crate::virtual_grid) `GridSelection` split:
/// the distinction between "no selection model" and "selection model, nothing
/// selected" is a *role* fact (`aria-selected` only belongs on rows in a
/// container that supports selection), expressed as a named enum rather than a
/// nested `Option`. A focus-only inspector (the property grid, whose roving
/// cursor is keyboard focus surfaced as `aria-activedescendant`) must **not**
/// stamp `aria-selected` — doing so would mis-announce it as a selection widget
/// (R873/R874). The grouped grid has no multi-select consumer, so unlike
/// `virtual_grid::GridSelection` there is no `Multi` variant (added when one
/// lands).
#[derive(Clone, Copy)]
pub enum GroupedGridSelection {
    /// Display / focus-only — data rows carry **no** `aria-selected`.
    Display,
    /// Single-select — each data row carries `aria-selected = (source == _)`;
    /// `None` selects nothing (every data row is `aria-selected = false`).
    Single(Option<usize>),
}

/// The addressing config of a grouped **grid** a11y tree (a `treegrid`, R874).
/// Columns are a [`GridColumn`] slice (tag + label + optional `aria-sort`) — the
/// same caller-owned-columns convention as [`grid_table_nodes`](crate::grid_table_nodes), so a *sortable*
/// grid and a static one differ only in the slice they pass, not in code.
pub struct GroupedGridSpec<'a> {
    /// The `role=treegrid` container tag.
    pub grid_tag: &'a str,
    /// Optional accessible name for the grid.
    pub name: Option<&'a str>,
    /// The column-header `role=row` tag.
    pub header_row_tag: &'a str,
    /// The columns (tag + label + active `aria-sort`), left to right.
    pub columns: &'a [GridColumn],
    /// Whether the grid exposes an `aria-selected` axis, and which source is
    /// selected ([`GroupedGridSelection`]).
    pub selection: GroupedGridSelection,
    /// R848 — the roving keyboard cursor's **visual position** (into `rows`),
    /// or `None`. The windowed `role=row` at that position (group header or
    /// data row) carries the `focused` state (`aria-activedescendant`). A
    /// grid with no keyboard navigation passes `None`.
    pub focused_view_pos: Option<usize>,
    /// R895 — the focused **column** within the focused data row, for a
    /// **cell-focus** treegrid (an editable grid's 2-D cell cursor). When the
    /// focused row ([`focused_view_pos`](Self::focused_view_pos)) is a data row
    /// and this is `Some(col)`, that row's `gridcell` at `col` carries the
    /// `focused` `aria-activedescendant` **instead of** the row; `None` keeps
    /// **row-level** focus (a selection-navigated read-only grid). A group
    /// header is always row-focused (it has no cells), regardless of this.
    pub focused_cell: Option<usize>,
}

/// R847 §5.50 — build the grouped-**grid** `treegrid` a11y nodes over the
/// visible window of a [`GroupRow`] flatten (R874: a `treegrid`, the
/// hierarchical-with-columns role, not a flat `grid`).
///
/// Emits `[treegrid, header_row, ...columnheaders, ...windowed rows + gridcells]`:
/// a `role=treegrid` root referencing the header row + every windowed row; a
/// `role=row` header carrying one `role=columnheader` per [`GridColumn`] (with
/// `aria-sort` when the column's [`GridColumn::sort`] is `Some`); then per
/// visible row a spanning **group-header** `role=row` at `aria-level = 1`
/// (`aria-expanded`, name `"<group> (<count>)"`) or a **data** `role=row` at
/// `aria-level = 2` whose `role=gridcell` children are named by value.
///
/// Whether data rows carry `aria-selected` is the [`GroupedGridSpec::selection`]
/// decision: [`GroupedGridSelection::Display`] omits the axis entirely (a
/// focus-only inspector), [`GroupedGridSelection::Single`] marks the selected
/// source. Cell names are the bare `cell_value` — the column label lives once on
/// the `columnheader`, so prefixing it onto every cell would duplicate what AT
/// already announces from the cell's column association (R874).
///
/// `group_label(group)` yields the group's display name; `cell_tag(source, col)`
/// the per-cell composite tag (a `pointer_transparent` paint node carries its
/// bound); `cell_value(source, col)` the cell's value; `row_tag(&GroupRow)` the
/// header / data **row** tag — the consumer owns it (R895), so a grid routing
/// rows through one coordinator (`{prefix}#{id}` via
/// [`GroupRow::composite_tag`]) and a single-`External` grid with a bespoke
/// scheme (`data_grid#g<g>` / `dg_row<src>`) build from the SAME topology,
/// differing only in the closure they pass — the same caller-owned-tag
/// convention `cell_tag` already follows.
#[must_use]
pub fn grouped_grid_access_nodes(
    spec: &GroupedGridSpec,
    rows: &[GroupRow],
    window: VisibleWindow,
    group_label: impl Fn(usize) -> String,
    cell_tag: impl Fn(usize, usize) -> String,
    cell_value: impl Fn(usize, usize) -> String,
    row_tag: impl Fn(&GroupRow) -> String,
) -> Vec<AccessNode> {
    let total = u32::try_from(rows.len()).unwrap_or(u32::MAX);

    let mut nodes: Vec<AccessNode> =
        Vec::with_capacity(window.count * (spec.columns.len() + 1) + 2);

    // TreeGrid root: header row then every windowed row.
    let mut grid = AccessNode::new(spec.grid_tag, AriaRole::TreeGrid);
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
        let mut col =
            AccessNode::new(column.tag.as_str(), AriaRole::ColumnHeader).with_name(&column.label);
        if let Some(dir) = column.sort {
            col = col.with_sort(dir);
        }
        nodes.push(col);
    }

    // Windowed rows: spanning group headers + data rows with gridcells.
    for view_pos in window.indices() {
        let Some(row) = rows.get(view_pos) else {
            continue;
        };
        let posinset = u32::try_from(view_pos + 1).unwrap_or(u32::MAX);
        // The roving cursor's row (header or data) carries the active-descendant
        // `focused` state; the gridcells under a data row do not.
        let focused = spec.focused_view_pos == Some(view_pos);
        match *row {
            GroupRow::Header {
                group,
                member_count,
                collapsed,
            } => {
                // Group header = level-1 expandable branch row (WAI-ARIA 6.6.8).
                nodes.push(
                    AccessNode::new(row_tag(row), AriaRole::Row)
                        .with_name(format!("{} ({member_count})", group_label(group)))
                        .with_level(1)
                        .with_position_in_set(posinset)
                        .with_size_of_set(total)
                        .with_expanded(!collapsed)
                        .with_focused(focused),
                );
            }
            GroupRow::Data { source } => {
                // R895 — cell-focus vs row-focus: on the focused data row, a
                // `Some(col)` `focused_cell` moves the activedescendant onto
                // that gridcell (an editable grid's 2-D cell cursor) and off
                // the row; `None` keeps the row focused (a row-navigated grid).
                let cell_focus = if focused { spec.focused_cell } else { None };
                // Data row = level-2 leaf row carrying one gridcell per column.
                let mut data_row = AccessNode::new(row_tag(row), AriaRole::Row)
                    .with_level(2)
                    .with_position_in_set(posinset)
                    .with_size_of_set(total)
                    .with_focused(focused && cell_focus.is_none());
                // `aria-selected` only on a selection-capable grid (R874): a
                // focus-only inspector (`Display`) omits the axis entirely.
                if let GroupedGridSelection::Single(sel) = spec.selection {
                    data_row = data_row.with_selected(sel == Some(source));
                }
                for c in 0..spec.columns.len() {
                    data_row = data_row.with_child(cell_tag(source, c));
                }
                nodes.push(data_row);
                for c in 0..spec.columns.len() {
                    // Bare value: the column label lives on the columnheader, so
                    // AT announces "<column>: <value>" without prefixing (R874).
                    nodes.push(
                        AccessNode::new(cell_tag(source, c), AriaRole::GridCell)
                            .with_name(cell_value(source, c))
                            .with_focused(cell_focus == Some(c)),
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
        GroupedGridSelection, GroupedGridSpec, GroupedTreeSpec, grouped_focus_target,
        grouped_grid_access_nodes, grouped_tree_access_nodes,
    };
    use crate::grid::GridColumn;
    use crate::role::{AriaRole, SortDirection};
    use pinion_core::widgets::group_order::{GroupRow, group_rows};
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
        // Cursor on visual pos 1 (the first data row, source 0).
        let spec = GroupedTreeSpec {
            tree_tag: "list",
            name: Some("Grouped"),
            header_prefix: "grp",
            data_prefix: "row",
            selected_source: Some(2),
            focused_view_pos: Some(1),
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
        assert_eq!(
            nodes[0].size_of_set,
            Some(u32::try_from(rows.len()).unwrap())
        );
        // Group 0 header: treeitem, level 1, expanded, name "G0 (2)".
        let h0 = nodes
            .iter()
            .find(|n| n.tag == "grp#0")
            .expect("group 0 header");
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
        // The cursor (visual pos 1 = data row 0) carries the focused state;
        // a non-cursor row does not. Cursor ⊥ selection (focused on source 0,
        // selected on source 2).
        assert!(d0.state.focused, "the cursor row is the active descendant");
        assert!(!d2.state.focused, "a non-cursor row is not focused");
        assert!(!h0.state.focused, "the header is not the cursor here");
    }

    fn grid_columns() -> [GridColumn; 2] {
        [
            GridColumn {
                tag: "c0".into(),
                label: "Name".into(),
                sort: Some(SortDirection::Ascending),
            },
            GridColumn {
                tag: "c1".into(),
                label: "Size".into(),
                sort: None,
            },
        ]
    }

    #[test]
    fn grouped_grid_emits_treegrid_columnheaders_group_rows_and_gridcells() {
        let rows = sample();
        let win = full_window(&rows);
        let columns = grid_columns();
        // Cursor on visual pos 0 (the group-0 header).
        let spec = GroupedGridSpec {
            grid_tag: "grid",
            name: Some("Grouped grid"),
            header_row_tag: "head",
            columns: &columns,
            selection: GroupedGridSelection::Single(Some(0)),
            focused_view_pos: Some(0),
            focused_cell: None,
        };
        let nodes = grouped_grid_access_nodes(
            &spec,
            &rows,
            win,
            |g| format!("G{g}"),
            |s, c| format!("cell_{s}_{c}"),
            |s, c| format!("v{s}{c}"),
            |r| r.composite_tag("grp", "row"),
        );
        // R874 — the grouped grid is a `treegrid` (hierarchical + columns), not
        // a flat `grid`.
        assert_eq!(nodes[0].role, AriaRole::TreeGrid);
        // Two column headers; the active one carries aria-sort, the other none.
        let c0 = nodes.iter().find(|n| n.tag == "c0").expect("column 0");
        assert_eq!(c0.role, AriaRole::ColumnHeader);
        assert_eq!(c0.sort, Some(SortDirection::Ascending));
        let c1 = nodes.iter().find(|n| n.tag == "c1").expect("column 1");
        assert_eq!(c1.sort, None);
        // Group 0 header is an expandable level-1 Row, and the cursor (visual
        // pos 0) rests on it → focused (active descendant); a data row is not.
        let h0 = nodes
            .iter()
            .find(|n| n.tag == "grp#0")
            .expect("group 0 header");
        assert_eq!(h0.role, AriaRole::Row);
        assert_eq!(h0.level, Some(1), "group header is aria-level 1");
        assert_eq!(h0.expanded, Some(true));
        assert!(h0.state.focused, "the cursor rests on the group-0 header");
        // Data row source 0: level-2 Row, selected, with two gridcells named by
        // bare value (no column-label prefix — that lives on the columnheader).
        let d0 = nodes.iter().find(|n| n.tag == "row#0").expect("data row 0");
        assert_eq!(d0.role, AriaRole::Row);
        assert_eq!(d0.level, Some(2), "data row is aria-level 2");
        assert_eq!(d0.selected, Some(true));
        assert!(!d0.state.focused, "a data row is not the cursor here");
        let cell00 = nodes
            .iter()
            .find(|n| n.tag == "cell_0_0")
            .expect("cell 0,0");
        assert_eq!(cell00.role, AriaRole::GridCell);
        assert_eq!(cell00.name.as_deref(), Some("v00"));
        let cell01 = nodes
            .iter()
            .find(|n| n.tag == "cell_0_1")
            .expect("cell 0,1");
        assert_eq!(cell01.name.as_deref(), Some("v01"));
    }

    #[test]
    fn r895_focused_cell_moves_activedescendant_onto_the_gridcell() {
        // R895 — the cell-focus model (an editable grid's 2-D cursor):
        // `focused_cell = Some(col)` on the focused data row moves the
        // `aria-activedescendant` onto that gridcell, OFF the row; `None`
        // keeps row focus (the read-only grids). Also exercises the `row_tag`
        // closure with a bespoke (non-composite) scheme.
        let rows = sample(); // [Header g0, Data 0, Data 2, Header g1, Data 1]
        let win = full_window(&rows);
        let columns = grid_columns();
        let spec = GroupedGridSpec {
            grid_tag: "grid",
            name: None,
            header_row_tag: "head",
            columns: &columns,
            selection: GroupedGridSelection::Display,
            focused_view_pos: Some(1), // the first data row (source 0)
            focused_cell: Some(1),     // cell-focus on column 1
        };
        let nodes = grouped_grid_access_nodes(
            &spec,
            &rows,
            win,
            |g| format!("G{g}"),
            |s, c| format!("cell_{s}_{c}"),
            |s, c| format!("v{s}{c}"),
            // A bespoke single-coordinator scheme ('g'-prefixed headers, a
            // dotted data-row tag) the composite scheme cannot express.
            |r| match *r {
                GroupRow::Header { group, .. } => format!("grid#g{group}"),
                GroupRow::Data { source } => format!("drow.{source}"),
            },
        );
        // The bespoke row tags are honored (the consumer owns the tag).
        assert!(
            nodes.iter().any(|n| n.tag == "grid#g0"),
            "bespoke header tag"
        );
        let d0 = nodes
            .iter()
            .find(|n| n.tag == "drow.0")
            .expect("bespoke data-row tag");
        // Cell-focus: the row is NOT the active descendant; its column-1 cell is.
        assert!(
            !d0.state.focused,
            "cell-focus: the row is not the active descendant"
        );
        let cell01 = nodes
            .iter()
            .find(|n| n.tag == "cell_0_1")
            .expect("cell 0,1");
        assert!(
            cell01.state.focused,
            "the focused column's gridcell is the active descendant"
        );
        let cell00 = nodes
            .iter()
            .find(|n| n.tag == "cell_0_0")
            .expect("cell 0,0");
        assert!(
            !cell00.state.focused,
            "an unfocused column's gridcell is not"
        );
    }

    #[test]
    fn grouped_grid_display_selection_omits_aria_selected() {
        // R874 — a focus-only grid (`Display`) carries NO `aria-selected` axis:
        // its roving cursor is keyboard focus (`aria-activedescendant`), not
        // selection. Every data row's `selected` is `None` (absent), not
        // `Some(false)` — which would mis-announce a selection model.
        let rows = sample();
        let win = full_window(&rows);
        let columns = grid_columns();
        let spec = GroupedGridSpec {
            grid_tag: "grid",
            name: None,
            header_row_tag: "head",
            columns: &columns,
            selection: GroupedGridSelection::Display,
            focused_view_pos: Some(1),
            focused_cell: None,
        };
        let nodes = grouped_grid_access_nodes(
            &spec,
            &rows,
            win,
            |g| format!("G{g}"),
            |s, c| format!("cell_{s}_{c}"),
            |s, c| format!("v{s}{c}"),
            |r| r.composite_tag("grp", "row"),
        );
        for tag in ["row#0", "row#1", "row#2"] {
            let d = nodes
                .iter()
                .find(|n| n.tag == tag)
                .unwrap_or_else(|| panic!("{tag}"));
            assert_eq!(
                d.selected, None,
                "{tag} carries no aria-selected under Display"
            );
        }
        // The cursor row (visual pos 1 = data source 0) is still the active
        // descendant — focus is orthogonal to the absent selection axis.
        let cursor = nodes
            .iter()
            .find(|n| n.tag == "row#0")
            .expect("cursor data row");
        assert!(
            cursor.state.focused,
            "Display still conveys the roving cursor via focus"
        );
    }

    #[test]
    fn grouped_focus_target_rings_cursor_row_or_container() {
        use pinion_core::widgets::group_order::GroupOrderState;
        // 3 sources over 2 groups (0,1,0): rows = [Header0, Data0, Data2, Header1, Data1].
        let s = GroupOrderState::new(vec![0, 1, 0], vec!["A".into(), "B".into()]);
        // Unfocused → the focused tag passes through atomic (no active descendant).
        let other =
            grouped_focus_target(&s, "list", "grp", Some(0), Some("other")).expect("atomic");
        assert_eq!(other.focus_tag, "other");
        assert_eq!(other.active_descendant, None);
        // Focused, cursor on pos 0 (group-0 header) → composite(list, grp#0).
        let h = grouped_focus_target(&s, "list", "grp", Some(0), Some("list")).expect("composite");
        assert_eq!(h.focus_tag, "list");
        assert_eq!(h.active_descendant.as_deref(), Some("grp#0"));
        // Cursor on pos 1 (data source 0) → composite(list, list#0).
        let d = grouped_focus_target(&s, "list", "grp", Some(1), Some("list")).expect("composite");
        assert_eq!(d.active_descendant.as_deref(), Some("list#0"));
        // No cursor (or out-of-range) → ring the container, no active descendant.
        let none = grouped_focus_target(&s, "list", "grp", None, Some("list")).expect("container");
        assert_eq!(none.active_descendant, None);
        let stale =
            grouped_focus_target(&s, "list", "grp", Some(999), Some("list")).expect("container");
        assert_eq!(
            stale.active_descendant, None,
            "out-of-range cursor rings the container"
        );
    }
}
