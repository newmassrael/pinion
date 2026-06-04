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
use crate::role::AriaRole;
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
    let ncols = headers.len();
    // grid + header row + ncols columnheaders + per windowed row (1 row +
    // ncols cells).
    let mut nodes: Vec<AccessNode> =
        Vec::with_capacity(window.count * (ncols + 1) + ncols + 2);

    let mut grid = AccessNode::new(grid_tag, AriaRole::Grid)
        .with_name(grid_name)
        .with_size_of_set(set_size);
    grid = grid.with_child(format!("{grid_tag}_hrow"));
    for id in window.indices() {
        grid = grid.with_child(format!("{grid_tag}_row{id}"));
    }
    nodes.push(grid);

    // Frozen header row + its columnheader cells.
    let mut hrow = AccessNode::new(format!("{grid_tag}_hrow"), AriaRole::Row);
    for col in 0..ncols {
        hrow = hrow.with_child(format!("{grid_tag}_ch{col}"));
    }
    nodes.push(hrow);
    for (col, label) in headers.iter().enumerate() {
        nodes.push(
            AccessNode::new(format!("{grid_tag}_ch{col}"), AriaRole::ColumnHeader)
                .with_name(*label),
        );
    }

    // Windowed data rows + their gridcells.
    for id in window.indices() {
        let posinset = u32::try_from(id + 1).unwrap_or(u32::MAX);
        let mut row = AccessNode::new(format!("{grid_tag}_row{id}"), AriaRole::Row)
            .with_position_in_set(posinset)
            .with_size_of_set(set_size);
        for col in 0..ncols {
            row = row.with_child(format!("{grid_tag}#{id}_{col}"));
        }
        nodes.push(row);
        for col in 0..ncols {
            nodes.push(AccessNode::new(
                format!("{grid_tag}#{id}_{col}"),
                AriaRole::GridCell,
            ));
        }
    }
    nodes
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
/// `None`. Single-select — no `aria-multiselectable` (a multi-select index
/// model is a later additive axis).
#[must_use]
pub fn windowed_grid_nodes_selected(
    grid_tag: &str,
    grid_name: &str,
    headers: &[&str],
    set_size: u32,
    window: &VisibleWindow,
    selected: Option<usize>,
) -> Vec<AccessNode> {
    let mut nodes = windowed_grid_nodes(grid_tag, grid_name, headers, set_size, window);
    // Decorate each *data* row (tag `{grid_tag}_row{id}`) with
    // `aria-selected`. The header row is `{grid_tag}_hrow` (no `_row`
    // prefix), and gridcells are `{grid_tag}#…`, so the `_row` prefix
    // selects exactly the data rows.
    let row_prefix = format!("{grid_tag}_row");
    for node in &mut nodes {
        if node.role != AriaRole::Row {
            continue;
        }
        if let Some(id) = node.tag.strip_prefix(&row_prefix).and_then(|s| s.parse::<usize>().ok())
        {
            node.selected = Some(selected == Some(id));
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

    #[test]
    fn emits_grid_header_and_windowed_rows() {
        let nodes = windowed_grid_nodes("vtbl", "Data grid", &HEADERS, 10_000, &window(0, 2));
        assert_eq!(nodes[0].role, AriaRole::Grid);
        assert_eq!(nodes[0].name.as_deref(), Some("Data grid"));
        assert_eq!(nodes[0].size_of_set, Some(10_000), "grid setsize = FULL dataset");
        // grid claims the header row + the 2 windowed data rows.
        assert_eq!(nodes[0].children.len(), 3);
        // header row + 3 columnheaders.
        assert_eq!(nodes[1].role, AriaRole::Row);
        assert_eq!(nodes[1].tag, "vtbl_hrow");
        let columnheaders = nodes.iter().filter(|n| n.role == AriaRole::ColumnHeader).count();
        assert_eq!(columnheaders, 3, "one columnheader per column");
        // 2 windowed data rows, each with 3 gridcells.
        let data_rows = nodes
            .iter()
            .filter(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow")
            .count();
        assert_eq!(data_rows, 2);
        let cells = nodes.iter().filter(|n| n.role == AriaRole::GridCell).count();
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
        for n in nodes.iter().filter(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow") {
            assert_eq!(n.selected, None, "display-only data rows omit aria-selected");
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
        assert_eq!(d_row.selected, Some(false), "decorated row sets aria-selected");
    }

    #[test]
    fn selected_outside_window_marks_no_visible_row() {
        let nodes =
            windowed_grid_nodes_selected("vtbl", "G", &HEADERS, 10_000, &window(0, 2), Some(9_999));
        for n in nodes.iter().filter(|n| n.role == AriaRole::Row && n.tag != "vtbl_hrow") {
            assert_eq!(n.selected, Some(false));
        }
    }
}
