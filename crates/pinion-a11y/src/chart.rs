//! R1634 §5.40 — a chart projected as a **table**, so its data is reachable one
//! datum at a time instead of as one string.
//!
//! # What a chart was to a screen reader
//!
//! One node. Every chart in this tree published a single [`AccessNode`] whose
//! whole content was the string its inspect readout happened to build, so "the
//! third series in April" was not addressable, not navigable, and not
//! announced. The strongest thing this framework draws was, to a reader who
//! does not read pixels, a sentence.
//!
//! # The references have nothing here, measured
//!
//! The toolkit's three charting modules at 6.11 — its 2D charts, the newer
//! graphs module that replaces them, and the 3D data visualisation one —
//! contain **no accessibility integration at all** between them: not one call
//! into the platform accessibility layer the rest of that toolkit is wired
//! through. A chart view there is a graphics view, which reaches the platform
//! as one scroll-area object — exactly the state described above. The DCC and the engine have no charting at all. So
//! unlike every other axis this project measures itself on, there is no floor to
//! clear: the shape had to be chosen and argued rather than bettered.
//!
//! # Why a table, and not a graphics role
//!
//! WAI-ARIA has no chart role. Its graphics module offers `graphics-document` /
//! `graphics-object` / `graphics-symbol`, and the platform bridges largely drop
//! them — a role no assistive technology maps is a role that announces nothing.
//! What a chart's data *is*, on the other hand, is a table: points down, series
//! across. `table` / `row` / `columnheader` / `rowheader` / `cell` are already in
//! this crate's vocabulary, already lowered by
//! [`AccessTreeBuilder`](crate::AccessTreeBuilder), and already understood by
//! every screen reader there is, with navigation commands users already know.
//!
//! # The orientation, and why this way round
//!
//! **Rows are data points; columns are series.** A chart usually has many
//! points and few series, so this keeps the *column* count small — which is what
//! a table reader moves across — and lets the common gesture, reading down a
//! column, be reading one series in order. It is also the projection the
//! established web charting libraries expose for the same reason.
//!
//! # Where this is past what the picture can do
//!
//! **Every column and every row is named, including the ones the canvas could
//! not fit.** R1633 thins axis labels when there are more of them than room;
//! that is a fact about pixels, and a reader who is not reading pixels is not
//! subject to it. The row header here comes from the category list rather than
//! from the painted label, so a thinned axis still announces all thirty
//! endpoints while drawing eight. The two are consistent rather than in
//! conflict: `labels_omitted` says what the *picture* left off, and the table
//! says what the *data* has.
//!
//! # How many nodes
//!
//! One per drawn mark, and the bound is the chart's own window — the same bound
//! the picture has. When a window is active, [`ChartTable::set_size`] carries
//! the full extent so `aria-setsize` states what the window is a window *onto*,
//! which is the model [`virtual_list`](crate::virtual_list) established: the
//! full extent by declaration, the rendered part by the present nodes. No
//! arbitrary node ceiling is imposed, because the tree's rule is that the AT
//! tree exposes what the paint exposes, and a chart that draws ten thousand
//! marks has drawn them.

use crate::node::AccessNode;
use crate::role::AriaRole;

/// One datum of one series, at one point (R1634).
#[derive(Clone, Debug, PartialEq)]
pub struct ChartCell {
    /// The paint tag of the mark this datum was drawn as — a bar, a point, a
    /// box — so the shell resolves the cell's bounds to the thing on screen and
    /// a magnifier or a touch explorer lands on it.
    ///
    /// `None` for a series whose samples are drawn as one path rather than as
    /// individual marks: the datum is still announced, and there is simply no
    /// rectangle that is only it. Stated rather than faked, because a made-up
    /// rectangle would point a touch explorer at the wrong place.
    pub tag: Option<String>,
    /// What the datum reads as — the value in the units the axis carries, as
    /// the chart itself formats it, so the announcement and the tooltip cannot
    /// disagree about precision.
    pub value: String,
}

/// One series, as a **column** of the projected table (R1634).
#[derive(Clone, Debug, PartialEq)]
pub struct ChartColumn {
    /// The column header's tag. Usually the series' own painted tag, which is
    /// how a reader's cursor reaches the line or the swatch it names.
    pub tag: String,
    /// The series name, announced as the column header.
    pub name: String,
}

/// One data point, as a **row** of the projected table (R1634).
#[derive(Clone, Debug, PartialEq)]
pub struct ChartRow {
    /// The row header's tag.
    pub tag: String,
    /// What the point is called on the x axis — a category name, a formatted
    /// number, a timestamp.
    ///
    /// From the **data**, not from the painted label: R1633 draws fewer labels
    /// than there are points when they will not fit, and a reader who is not
    /// reading pixels is not subject to that.
    pub name: String,
    /// This point's datum in each column, in column order. A shorter list
    /// leaves the remaining columns without a cell, which is how a series that
    /// simply has no value there is told from one that has zero.
    pub cells: Vec<ChartCell>,
}

/// A chart's data as a table (R1634).
#[derive(Clone, Debug, PartialEq)]
pub struct ChartTable {
    /// The chart root's tag — the node an AT lands on when it reaches the
    /// chart.
    pub tag: String,
    /// What the chart is called.
    pub name: String,
    /// What the x axis is called, announced as the corner header so a reader
    /// hears what the row names *are* before hearing thirty of them.
    pub axis_name: String,
    /// The series, left to right.
    pub columns: Vec<ChartColumn>,
    /// The points, top to bottom, in the order the axis draws them.
    pub rows: Vec<ChartRow>,
    /// How many points the chart has in total when [`Self::rows`] is a window
    /// onto them, else `None`.
    ///
    /// `aria-setsize` / `aria-rowcount` then state the whole extent while the
    /// present rows are the part that exists — the
    /// [`virtual_list`](crate::virtual_list) model, and the reason a windowed
    /// chart does not announce itself as having only the visible points.
    pub set_size: Option<usize>,
}

impl ChartTable {
    /// How many rows the table claims, which is the full extent when the rows
    /// are a window onto it.
    #[must_use]
    fn claimed_rows(&self) -> usize {
        self.set_size.unwrap_or(self.rows.len())
    }
}

/// Build the `table` container, its header row, and one row per data point
/// (R1634).
///
/// The returned vector is `[table, header_row, header_cells…, row, row_cells…,
/// …]` — the container first, then each row followed by its own cells, which is
/// the flat convention `lower_access_node` resolves into a tree.
///
/// Row and column counts include the headers, per WAI-ARIA 1.2 §6.6.3: a table
/// of three points and two series is four rows by three columns, because the
/// header row and the row-header column are part of the grid a reader
/// navigates.
#[must_use]
pub fn chart_table_nodes(table: &ChartTable) -> Vec<AccessNode> {
    let columns = u32::try_from(table.columns.len() + 1).unwrap_or(u32::MAX);
    let rows = u32::try_from(table.claimed_rows() + 1).unwrap_or(u32::MAX);
    let header_tag = format!("{}.a11y.header", table.tag);

    let mut children = vec![header_tag.clone()];
    children.extend(table.rows.iter().map(|row| row.tag.clone()));

    let mut out = vec![
        children.into_iter().fold(
            AccessNode::new(&table.tag, AriaRole::Table)
                .with_name(&table.name)
                .with_row_count(rows)
                .with_column_count(columns),
            AccessNode::with_child,
        ),
    ];
    out.push(header_row(table, &header_tag, rows, columns));
    out.extend(header_cells(table));
    for (index, row) in table.rows.iter().enumerate() {
        out.push(data_row(table, row, index, rows, columns));
        out.extend(data_cells(table, row, index));
    }
    out
}

/// The header row: the corner cell naming the axis, then one column header per
/// series.
fn header_row(table: &ChartTable, tag: &str, rows: u32, columns: u32) -> AccessNode {
    let mut cells = vec![format!("{}.a11y.axis", table.tag)];
    cells.extend(table.columns.iter().map(|column| column.tag.clone()));
    cells.into_iter().fold(
        AccessNode::new(tag, AriaRole::Row)
            .with_row(0)
            .with_row_count(rows)
            .with_column_count(columns),
        AccessNode::with_child,
    )
}

/// The corner cell and the per-series column headers.
fn header_cells(table: &ChartTable) -> Vec<AccessNode> {
    let mut out = vec![
        AccessNode::new(format!("{}.a11y.axis", table.tag), AriaRole::ColumnHeader)
            .with_name(&table.axis_name)
            .with_row(0)
            .with_column(0),
    ];
    out.extend(table.columns.iter().enumerate().map(|(index, column)| {
        AccessNode::new(&column.tag, AriaRole::ColumnHeader)
            .with_name(&column.name)
            .with_row(0)
            .with_column(index + 1)
    }));
    out
}

/// One data row: its header cell plus its per-series cells, claimed as children.
fn data_row(
    table: &ChartTable,
    row: &ChartRow,
    index: usize,
    rows: u32,
    columns: u32,
) -> AccessNode {
    let mut cells = vec![format!("{}.a11y.rowhdr", row.tag)];
    cells.extend(
        row.cells
            .iter()
            .enumerate()
            .map(|(column, _)| cell_tag(row, column)),
    );
    cells.into_iter().fold(
        AccessNode::new(&row.tag, AriaRole::Row)
            .with_row(index + 1)
            .with_row_count(rows)
            .with_column_count(columns)
            .with_set_position(index, table.claimed_rows()),
        AccessNode::with_child,
    )
}

/// A row's header cell and its data cells.
fn data_cells(table: &ChartTable, row: &ChartRow, index: usize) -> Vec<AccessNode> {
    let mut out = vec![
        AccessNode::new(format!("{}.a11y.rowhdr", row.tag), AriaRole::RowHeader)
            .with_name(&row.name)
            .with_row(index + 1)
            .with_column(0),
    ];
    out.extend(row.cells.iter().enumerate().map(|(column, cell)| {
        // The name carries the column with the value — "Revenue: 182" — because
        // a cell announced alone is a number with no subject, and a reader
        // moving across a row hears which series each one belongs to. The same
        // rule `grid_table_nodes` states for its `GridCell`.
        let name = table.columns.get(column).map_or_else(
            || cell.value.clone(),
            |series| format!("{}: {}", series.name, cell.value),
        );
        let node = AccessNode::new(cell_tag(row, column), AriaRole::Cell)
            .with_name(name)
            .with_row(index + 1)
            .with_column(column + 1);
        // A datum drawn as its own mark points at that mark; one drawn inside a
        // shared path has no rectangle of its own, and the union tag is left
        // empty rather than pointing somewhere it is not.
        match &cell.tag {
            Some(tag) => node.with_bounds_union_tag(tag),
            None => node,
        }
    }));
    out
}

/// A cell's own tag.
///
/// Derived from the row's rather than taken from the mark's paint tag, because a
/// datum with no mark of its own still needs an address — and because two
/// series may draw one datum with one node, where two cells must still be two
/// nodes. The mark tag is carried as the bounds union instead, which is what
/// makes the cell point at the thing on screen without being named by it.
fn cell_tag(row: &ChartRow, column: usize) -> String {
    format!("{}.a11y.c{column}", row.tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> ChartTable {
        ChartTable {
            tag: "chart".into(),
            name: "Revenue by month".into(),
            axis_name: "Month".into(),
            columns: vec![ChartColumn {
                tag: "chart.series.0".into(),
                name: "Revenue".into(),
            }],
            rows: vec![
                ChartRow {
                    tag: "chart.a11y.r0".into(),
                    name: "Jan".into(),
                    cells: vec![ChartCell {
                        tag: Some("chart.bar.0".into()),
                        value: "182".into(),
                    }],
                },
                ChartRow {
                    tag: "chart.a11y.r1".into(),
                    name: "Feb".into(),
                    cells: vec![ChartCell {
                        tag: Some("chart.bar.1".into()),
                        value: "164".into(),
                    }],
                },
            ],
            set_size: None,
        }
    }

    fn find<'a>(nodes: &'a [AccessNode], tag: &str) -> &'a AccessNode {
        nodes
            .iter()
            .find(|n| n.tag == tag)
            .unwrap_or_else(|| panic!("no node tagged {tag}: {:?}", tags(nodes)))
    }

    fn tags(nodes: &[AccessNode]) -> Vec<&str> {
        nodes.iter().map(|n| n.tag.as_str()).collect()
    }

    /// ★ The projection is a real table: a container claiming a header row and
    /// one row per point, each row claiming a header cell and one cell per
    /// series, with the counts WAI-ARIA asks for.
    #[test]
    fn r1634_a_chart_is_a_table_of_points_by_series() {
        let nodes = chart_table_nodes(&table());
        let root = find(&nodes, "chart");
        assert_eq!(root.role, AriaRole::Table);
        assert_eq!(root.name.as_deref(), Some("Revenue by month"));
        assert_eq!(
            (root.row_count, root.column_count),
            (Some(3), Some(2)),
            "two points and one series, plus the headers of each axis"
        );
        assert_eq!(
            root.children,
            vec!["chart.a11y.header", "chart.a11y.r0", "chart.a11y.r1"],
            "the container claims the header row and every data row"
        );

        let jan = find(&nodes, "chart.a11y.r0");
        assert_eq!(jan.role, AriaRole::Row);
        assert_eq!(
            jan.row_index,
            Some(2),
            "aria-rowindex is 1-based and row 1 is the header"
        );
        assert_eq!((jan.position_in_set, jan.size_of_set), (Some(1), Some(2)));
        assert_eq!(
            jan.children,
            vec!["chart.a11y.r0.a11y.rowhdr", "chart.a11y.r0.a11y.c0"]
        );
    }

    /// ★ A cell is named by its SERIES and its value, and points at the mark it
    /// was drawn as.
    ///
    /// A number announced with no subject is the failure this is written
    /// against: a reader moving across a row of four series must hear which one
    /// each figure belongs to.
    #[test]
    fn r1634_a_cell_names_its_series_and_points_at_its_mark() {
        let nodes = chart_table_nodes(&table());
        let cell = find(&nodes, "chart.a11y.r0.a11y.c0");
        assert_eq!(cell.role, AriaRole::Cell);
        assert_eq!(cell.name.as_deref(), Some("Revenue: 182"));
        assert_eq!(
            cell.bounds_union_tags,
            vec!["chart.bar.0"],
            "★ and it resolves its bounds to the BAR, so a touch explorer lands \
             on the thing that was drawn"
        );
        assert_eq!((cell.row_index, cell.column_index), (Some(2), Some(2)));
    }

    /// ★ Every row is named from the DATA, so an axis whose labels the picture
    /// could not fit still announces all of them.
    ///
    /// R1633 thins labels when there are more than there is room for. That is a
    /// property of pixels; this is the assertion that a reader who is not
    /// reading pixels is not subject to it.
    #[test]
    fn r1634_a_thinned_axis_still_names_every_row() {
        let mut spec = table();
        spec.rows = (0..30)
            .map(|i| ChartRow {
                tag: format!("chart.a11y.r{i}"),
                name: format!("/endpoint-{i}"),
                cells: vec![ChartCell {
                    tag: Some(format!("chart.bar.{i}")),
                    value: i.to_string(),
                }],
            })
            .collect();
        let nodes = chart_table_nodes(&spec);

        let named: Vec<String> = (0..30)
            .map(|i| {
                find(&nodes, &format!("chart.a11y.r{i}.a11y.rowhdr"))
                    .name
                    .clone()
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(named.len(), 30, "★ all thirty, however few were painted");
        assert_eq!(named[29], "/endpoint-29");
        assert_eq!(find(&nodes, "chart").row_count, Some(31));
    }

    /// ★ A windowed chart states the whole extent while presenting the part it
    /// drew — the virtualized-list model, applied to a chart's own window.
    #[test]
    fn r1634_a_window_states_the_extent_it_is_a_window_onto() {
        let mut spec = table();
        spec.set_size = Some(52);
        let nodes = chart_table_nodes(&spec);

        assert_eq!(
            find(&nodes, "chart").row_count,
            Some(53),
            "the table claims every point, not the two on screen"
        );
        let jan = find(&nodes, "chart.a11y.r0");
        assert_eq!(
            jan.size_of_set,
            Some(52),
            "★ and a row says which of the WHOLE set it is"
        );
        assert_eq!(
            nodes.iter().filter(|n| n.role == AriaRole::Row).count(),
            3,
            "while only the drawn rows — and the header — are present"
        );
    }

    /// A datum drawn inside a shared path has no rectangle of its own, and says
    /// so by carrying none rather than by borrowing one.
    #[test]
    fn r1634_a_datum_with_no_mark_of_its_own_claims_no_bounds() {
        let mut spec = table();
        spec.rows[0].cells[0].tag = None;
        let nodes = chart_table_nodes(&spec);
        let cell = find(&nodes, "chart.a11y.r0.a11y.c0");
        assert!(cell.bounds_union_tags.is_empty());
        assert_eq!(
            cell.name.as_deref(),
            Some("Revenue: 182"),
            "the datum is still announced — only its rectangle is absent"
        );
    }
}
