//! R1560 §5.40 §5.36 — the document's **table** structure, derived from the
//! paint scene.
//!
//! A paragraph that a table's addressing placed in a cell
//! ([`pinion_core::text_table::CellPlacement`]) becomes a WAI-ARIA `cell`
//! carrying `aria-rowindex`, `aria-colindex` and — when it covers more than
//! one slot — `aria-rowspan` / `aria-colspan`, inside a `row` inside a `table`
//! that declares `aria-rowcount` / `aria-colcount`. That is how a
//! screen-reader user learns "row 3, column 2 of 4, spanning 2 columns" — the
//! information a sighted reader takes from the ruled grid.
//!
//! ## Qt has none of it
//!
//! Not a smaller amount: none. The accessibility interface a `QTextEdit`
//! implements is `QAccessibleTextInterface`, whose vocabulary is character
//! offsets, selections, ranges and text attributes, and which has **no method
//! that reports block structure at all** — the wall R1551 hit for heading
//! levels and R1559 for lists. Qt does have `QAccessibleTableInterface`, but it
//! is implemented by `QTableView` / `QTreeView`, the item **views**; a
//! `QTextTable` inside a document reaches `QTextDocumentLayout`, which draws
//! its rules, and stops. So a Qt document's table is, to a screen reader, an
//! undifferentiated run of paragraphs: the cell boundaries, the columns and
//! above all the *merges* are invisible, and a merged cell silently shifts
//! every following cell's apparent position with nothing to say so.
//!
//! ## Why a pass over the painted tree
//!
//! The R1543 / R1548 / R1551 / R1559 shape. **One derivation**: the address is
//! computed once, in the view, and rides the painted node, so the grid area a
//! cell occupies and the position an assistive technology announces are the
//! same allocation — there is no second "and tell the AT it is column 2" call
//! to forget. **Every topology**: a pass reaches any composition that paints a
//! table cell, including ones that do not exist yet.

use std::collections::HashSet;

use pinion_core::Scene;
use pinion_core::text_table::HeaderScope;

use crate::{AccessNode, AriaRole, NodeIndex};

/// Emit a WAI-ARIA `table` node per painted table, a `row` per row and a
/// `cell` per cell, and return how many nodes the pass added or upgraded.
///
/// # What is announced
///
/// On each **cell** (tagged with the cell's own box, so a multi-block cell is
/// one object rather than one per paragraph):
///
/// * the **role** — `cell`, or `columnheader` / `rowheader` when the address
///   falls in a header band;
/// * `aria-rowindex` / `aria-colindex` — its 1-based address, which is what an
///   assistive technology reads as "row 3, column 2";
/// * `aria-rowspan` / `aria-colspan` — only when the cell covers more than one
///   slot, matching ARIA's own rule that the attributes' absence means one.
///
/// On each **row**: the `row` role, `aria-rowindex`, and its cells as children
/// in column order. On each **table**: the `table` role, `aria-rowcount` /
/// `aria-colcount`, and its rows as children in row order.
///
/// # A row no cell starts in
///
/// A row every column of which is covered by a `rowspan` reaching down from
/// above has no cell to name it, and so gets no `row` node. That is not a
/// hole: what is in such a row is the continuation of cells announced in an
/// earlier row, and `aria-rowspan` on those cells is the attribute that says
/// so. The row's *extent* is still announced, through the table's
/// `aria-rowcount` and the cells' absolute `aria-rowindex`, so nothing a
/// reader navigates by is missing.
///
/// # Merging rather than duplicating
///
/// As in [`attach_block_lists`](crate::attach_block_lists): a node that
/// already exists for the tag has its role and attributes filled in rather
/// than gaining a twin, because two nodes for one tag is a malformed tree. The
/// lookup is indexed rather than scanned, which is also this round's repayment
/// of R1559's own note — that pass merged by a linear `find` per item, and a
/// table can have far more cells than a list has items.
pub fn attach_block_tables(nodes: &mut Vec<AccessNode>, scene: &Scene) -> usize {
    /// One cell, collected in paint order.
    struct Cell {
        tag: String,
        row_tag: String,
        table_tag: String,
        row: u32,
        column: u32,
        row_span: u16,
        column_span: u16,
        header: HeaderScope,
        row_count: u32,
        column_count: u32,
    }

    let mut cells: Vec<Cell> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    scene.for_each_text_leaf(|node, _, _| {
        let Some(placement) = node.cell.as_deref() else {
            return;
        };
        // A multi-block cell paints one box and several paragraphs; the cell
        // is the box, so the first paragraph that names it is the one that
        // creates it.
        if !seen.insert(placement.cell_tag.clone()) {
            return;
        }
        cells.push(Cell {
            tag: placement.cell_tag.clone(),
            row_tag: placement.row_tag.clone(),
            table_tag: placement.table_tag.clone(),
            row: placement.row,
            column: placement.column,
            row_span: placement.row_span,
            column_span: placement.column_span,
            header: placement.header,
            row_count: placement.row_count,
            column_count: placement.column_count,
        });
    });
    if cells.is_empty() {
        return 0;
    }

    // Rows and tables, in the order their first cell was painted — which is
    // flow order, so row-major.
    let mut rows: Vec<(String, u32, Vec<String>)> = Vec::new();
    let mut tables: Vec<(String, u32, u32, Vec<String>)> = Vec::new();
    for cell in &cells {
        if let Some(row) = rows.iter_mut().find(|(tag, ..)| *tag == cell.row_tag) {
            row.2.push(cell.tag.clone());
        } else {
            rows.push((cell.row_tag.clone(), cell.row, vec![cell.tag.clone()]));
        }
        if let Some(table) = tables.iter_mut().find(|(tag, ..)| *tag == cell.table_tag) {
            if !table.3.contains(&cell.row_tag) {
                table.3.push(cell.row_tag.clone());
            }
        } else {
            tables.push((
                cell.table_tag.clone(),
                cell.row_count,
                cell.column_count,
                vec![cell.row_tag.clone()],
            ));
        }
    }

    let mut index = NodeIndex::new(nodes);
    let mut touched = 0usize;

    for cell in cells {
        let role = match cell.header {
            // The corner cell announces as a `columnheader`: reading order
            // runs along the header row, and what it labels is the column the
            // row headers are in. HTML's own model does the same with a
            // scope-less corner `<th>`.
            HeaderScope::Column | HeaderScope::Corner => AriaRole::ColumnHeader,
            HeaderScope::Row => AriaRole::RowHeader,
            HeaderScope::None => AriaRole::Cell,
        };
        let node = index.upsert(nodes, &cell.tag, role);
        // A heading inside a cell is the stronger claim and survives, exactly
        // as it does for a list item.
        if node.role != AriaRole::Heading {
            node.role = role;
        }
        node.row_index = Some(cell.row.saturating_add(1));
        node.column_index = Some(cell.column.saturating_add(1));
        // ARIA reads an absent span as one, so stating it would be noise that
        // an AT has to compare against the default anyway.
        node.row_span = (cell.row_span > 1).then(|| u32::from(cell.row_span));
        node.column_span = (cell.column_span > 1).then(|| u32::from(cell.column_span));
        touched += 1;
    }

    for (tag, row, children) in rows {
        let node = index.upsert(nodes, &tag, AriaRole::Row);
        node.role = AriaRole::Row;
        node.row_index = Some(row.saturating_add(1));
        node.children = children;
        touched += 1;
    }

    for (tag, row_count, column_count, children) in tables {
        let node = index.upsert(nodes, &tag, AriaRole::Table);
        node.role = AriaRole::Table;
        node.row_count = Some(row_count);
        node.column_count = Some(column_count);
        node.children = children;
        touched += 1;
    }
    touched
}

#[cfg(test)]
mod tests {
    use super::attach_block_tables;
    use crate::{AccessNode, AriaRole};
    use pinion_core::Scene;
    use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
    use pinion_core::text_table::{CellSpec, TableFormat, TablePart, place_cells};

    /// Build a painted table from `(text, colspan, rowspan)` triples, addressed
    /// the way `view_document` addresses one.
    fn painted(format: &TableFormat, cells: &[(&str, u16, u16)]) -> Scene {
        let specs: Vec<Option<CellSpec>> = cells
            .iter()
            .map(|(_, colspan, rowspan)| {
                Some(
                    CellSpec::new(format.clone())
                        .spanning_columns(*colspan)
                        .spanning_rows(*rowspan),
                )
            })
            .collect();
        let addressing = place_cells(&specs, |part| match part {
            TablePart::Table(k) => format!("doc_tbl{k}"),
            TablePart::Row(k, r) => format!("doc_tbl{k}r{r}"),
            TablePart::Cell(i) => format!("doc_cel{i}"),
        });
        let children = cells
            .iter()
            .enumerate()
            .map(|(i, (text, ..))| {
                let node = TextNode::new((*text).to_string(), Rect::new(0, 0, 40, 20))
                    .with_tag(format!("doc_blk{i}"));
                match addressing.placements.get(i).and_then(Option::as_ref) {
                    Some(placement) => Scene::Text(node.with_cell_placement(placement.clone())),
                    None => Scene::Text(node),
                }
            })
            .collect();
        Scene::Container(ContainerNode::new(children))
    }

    fn by_tag<'n>(nodes: &'n [AccessNode], tag: &str) -> &'n AccessNode {
        nodes.iter().find(|n| n.tag == tag).expect("a node")
    }

    /// A cell announces where it is, the row announces which row it is, and
    /// the table announces how big it is.
    #[test]
    fn a_cell_announces_its_address() {
        let scene = painted(
            &TableFormat::new(2),
            &[("a", 1, 1), ("b", 1, 1), ("c", 1, 1)],
        );
        let mut nodes = Vec::new();
        assert_eq!(
            attach_block_tables(&mut nodes, &scene),
            6,
            "three cells, two rows, one table",
        );
        let third = by_tag(&nodes, "doc_cel2");
        assert_eq!(third.role, AriaRole::Cell);
        assert_eq!(third.row_index, Some(2), "1-based, as ARIA counts");
        assert_eq!(third.column_index, Some(1));
        assert_eq!(third.row_span, None, "an unspanned cell states no span");
        assert_eq!(third.column_span, None);
        let row = by_tag(&nodes, "doc_tbl0r1");
        assert_eq!(row.role, AriaRole::Row);
        assert_eq!(row.row_index, Some(2));
        assert_eq!(row.children, ["doc_cel2"]);
        let table = by_tag(&nodes, "doc_tbl0");
        assert_eq!(table.role, AriaRole::Table);
        assert_eq!(table.row_count, Some(2));
        assert_eq!(table.column_count, Some(2));
        assert_eq!(table.children, ["doc_tbl0r0", "doc_tbl0r1"]);
    }

    /// The half a position alone cannot carry: a merged cell announces the
    /// extent it covers, so the cells after it are not heard in the wrong
    /// place.
    #[test]
    fn a_merged_cell_announces_the_extent_it_covers() {
        let scene = painted(
            &TableFormat::new(3),
            &[("wide", 2, 1), ("tall", 1, 2), ("next", 1, 1)],
        );
        let mut nodes = Vec::new();
        attach_block_tables(&mut nodes, &scene);
        let wide = by_tag(&nodes, "doc_cel0");
        assert_eq!(wide.column_span, Some(2));
        assert_eq!(wide.row_span, None);
        let tall = by_tag(&nodes, "doc_cel1");
        assert_eq!(tall.row_span, Some(2));
        assert_eq!((tall.row_index, tall.column_index), (Some(1), Some(3)));
        let next = by_tag(&nodes, "doc_cel2");
        assert_eq!(
            (next.row_index, next.column_index),
            (Some(2), Some(1)),
            "the third cell is on row 2 because the first two filled row 1",
        );
    }

    /// Header-ness reaches the AT as the role, on both axes, derived from the
    /// address rather than declared.
    #[test]
    fn a_header_cell_announces_its_axis() {
        let format = TableFormat::new(2)
            .with_header_rows(1)
            .with_header_columns(1);
        let scene = painted(
            &format,
            &[("", 1, 1), ("col", 1, 1), ("row", 1, 1), ("data", 1, 1)],
        );
        let mut nodes = Vec::new();
        attach_block_tables(&mut nodes, &scene);
        assert_eq!(
            by_tag(&nodes, "doc_cel0").role,
            AriaRole::ColumnHeader,
            "the corner cell labels the row-header column",
        );
        assert_eq!(by_tag(&nodes, "doc_cel1").role, AriaRole::ColumnHeader);
        assert_eq!(by_tag(&nodes, "doc_cel2").role, AriaRole::RowHeader);
        assert_eq!(by_tag(&nodes, "doc_cel3").role, AriaRole::Cell);
    }

    /// A multi-block cell is one announced object, not one per paragraph.
    #[test]
    fn a_multi_block_cell_is_one_node() {
        let format = TableFormat::new(2);
        let specs = vec![
            Some(CellSpec::new(format.clone())),
            Some(CellSpec::new(format.clone()).continued()),
            Some(CellSpec::new(format)),
        ];
        let addressing = place_cells(&specs, |part| match part {
            TablePart::Table(k) => format!("doc_tbl{k}"),
            TablePart::Row(k, r) => format!("doc_tbl{k}r{r}"),
            TablePart::Cell(i) => format!("doc_cel{i}"),
        });
        let children = (0..3)
            .map(|i| {
                Scene::Text(
                    TextNode::new(format!("p{i}"), Rect::new(0, 0, 40, 20))
                        .with_tag(format!("doc_blk{i}"))
                        .with_cell_placement(addressing.placements[i].clone().expect("addressed")),
                )
            })
            .collect();
        let scene = Scene::Container(ContainerNode::new(children));
        let mut nodes = Vec::new();
        assert_eq!(
            attach_block_tables(&mut nodes, &scene),
            4,
            "two cells, one row, one table",
        );
        assert_eq!(
            by_tag(&nodes, "doc_tbl0r0").children,
            ["doc_cel0", "doc_cel2"]
        );
    }

    /// The counterfactual: a paragraph outside a table announces nothing.
    #[test]
    fn a_paragraph_outside_a_table_announces_nothing() {
        let scene = Scene::Container(ContainerNode::new(vec![Scene::Text(
            TextNode::new("plain".to_string(), Rect::new(0, 0, 40, 20)).with_tag("doc_blk0"),
        )]));
        let mut nodes = Vec::new();
        assert_eq!(attach_block_tables(&mut nodes, &scene), 0);
        assert!(nodes.is_empty());
    }

    /// R1536's arm: a document long enough to have tables is a document inside
    /// a scroll, and the leaf walk descends into one.
    #[test]
    fn a_cell_inside_a_scroll_is_found() {
        let scene = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 100, 50),
            painted(&TableFormat::new(1), &[("deep", 1, 1)]),
        ));
        let mut nodes = Vec::new();
        assert_eq!(attach_block_tables(&mut nodes, &scene), 3);
        assert_eq!(by_tag(&nodes, "doc_cel0").role, AriaRole::Cell);
    }

    /// A binding that already describes the tag gets its attributes filled in
    /// rather than a second node for the same object — and a heading in a cell
    /// stays a heading.
    #[test]
    fn an_existing_node_is_upgraded_and_a_heading_keeps_its_role() {
        let scene = painted(&TableFormat::new(1), &[("one", 1, 1), ("two", 1, 1)]);
        let mut nodes = vec![
            AccessNode::new("doc_cel0", AriaRole::Generic).with_name("Explicit"),
            AccessNode::new("doc_cel1", AriaRole::Heading).with_level(2),
        ];
        attach_block_tables(&mut nodes, &scene);
        assert_eq!(nodes.len(), 5, "two cells, two rows, one table — no twins");
        let first = by_tag(&nodes, "doc_cel0");
        assert_eq!(first.role, AriaRole::Cell);
        assert_eq!(first.name.as_deref(), Some("Explicit"), "the name survives");
        let heading = by_tag(&nodes, "doc_cel1");
        assert_eq!(heading.role, AriaRole::Heading);
        assert_eq!(heading.row_index, Some(2), "and is still in row 2");
    }

    /// A row every column of which is covered from above has no cell to name
    /// it, so it gets no `row` node — while the extent it occupies is still
    /// announced, by the spanning cell's `aria-rowspan` and the table's
    /// `aria-rowcount`.
    #[test]
    fn a_fully_spanned_row_is_announced_through_the_span() {
        let scene = painted(&TableFormat::new(1), &[("tall", 1, 3)]);
        let mut nodes = Vec::new();
        attach_block_tables(&mut nodes, &scene);
        assert_eq!(nodes.len(), 3, "one cell, one row, one table");
        assert_eq!(by_tag(&nodes, "doc_cel0").row_span, Some(3));
        assert_eq!(by_tag(&nodes, "doc_tbl0").row_count, Some(3));
        assert_eq!(
            by_tag(&nodes, "doc_tbl0").children,
            ["doc_tbl0r0"],
            "the rows the cell reaches into have no cells of their own",
        );
    }
}
