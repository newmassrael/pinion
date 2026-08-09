//! `scene/text_tables` — the document's table structure, and the addressing it
//! produced (R1560 §5.12 §5.36 §2 #7).
//!
//! A table is the block structure whose geometry is a function of its
//! *sequence*: a cell's `(row, column)` is not something the cell has, it is
//! where the cell lands once every earlier cell's spans have taken their slots.
//! So the interesting question about a table is never "what does this paragraph
//! say" — `scene/text_blocks` answers that — but "what shape is this table, and
//! which slot did each cell get". This method answers that: one row per table,
//! each holding its cells in flow order with the address each one was given and
//! the box it was painted in.
//!
//! # Against the toolkit 6.11
//!
//! The toolkit has the concept and keeps every part of it in-process, and two
//! of the facts here it does not have at all:
//!
//! - **Enumeration.** There is no "what tables does this document have"
//!   accessor. text document exposes `rootFrame()` and frame iteration, and
//!   finding the tables means walking the frame tree `qobject_cast`-ing each
//!   child to text table yourself. Here the census IS the answer, and it is
//!   answerable from outside the process.
//! - **Addressing.** `row()` / `column()` / `rowSpan()` /
//!   `columnSpan()` are C++ calls on an object that only exists inside a
//!   text document. None is reachable from a driver, a test harness or an
//!   agent.
//! - **A refused merge has no trace.** `mergeCells` returns `void`
//!   and silently does nothing when the rectangle is not a merge candidate, so
//!   after the fact the request and the result are indistinguishable.
//!   [`TextCellWire::declared_column_span`] and [`TextCellWire::clamped`]
//!   publish both.
//! - **A ragged table is not representable in the toolkit.** `insertRows` fills the
//!   grid, so every slot has a cell whether the author wanted one or not.
//!   [`TextTableWire::slack`] reports the slots nobody filled.
//! - **Geometry.** A text table is a text frame, so its own rect is
//!   reachable through `frameBoundingRect`; a *cell*'s is not — it has to be
//!   reconstructed from `firstCursorPosition()` and `blockBoundingRect`, a
//!   second derivation free to disagree with the painter's. Every box here is
//!   the one the layout produced.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "tables": [
//!       { "tag": "essay_tbl0", "rows": 2, "columns": 3, "cell_count": 5,
//!         "header_rows": 1, "header_columns": 0,
//!         "column_widths": ["auto", "120px", "1fr"],
//!         "cell_padding_px": 4, "cell_spacing_px": 0, "border_px": 1,
//!         "x": 20, "y": 68, "width": 460, "height": 84,
//!         "slack": [ { "row": 1, "column": 2 } ],
//!         "cells": [
//!           { "tag": "essay_cel1", "row_tag": "essay_tbl0r0",
//!             "row": 0, "column": 0, "row_span": 1, "column_span": 2,
//!             "declared_row_span": 1, "declared_column_span": 2,
//!             "clamped": false, "header": "column", "index": 0,
//!             "blocks": ["essay_blk1"],
//!             "x": 20, "y": 68, "width": 300, "height": 28 }
//!         ] }
//!     ]
//!   }
//! }
//! ```
//!
//! Request — no parameters; the method reads the last painted scene, so the
//! tables it reports are the tables on screen.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/text_tables", "id": 1 }
//! ```
//!
//! A binding that paints no table answers with an empty array. That is a
//! legitimate state — most windows have no document in them — and not an
//! error.
//!
//! # Coordinates
//!
//! Window-absolute, with enclosing `Scene::Scroll` offsets folded in exactly as
//! `Scene::rect_for_tag_absolute` folds them. A box is `null` when the paint
//! has not laid it out yet — the honest answer for a frame that has been built
//! but not measured, rather than a zero rect a caller would read as "at the
//! origin, empty".

use std::collections::HashMap;

use pinion_core::Scene;
use pinion_core::scene::Rect;
use pinion_core::style::GridTrack;
use pinion_core::text_table::CellPlacement;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One slot of a table's grid that no cell covers.
#[derive(Debug, Clone, Serialize)]
pub struct GridSlotWire {
    /// 0-based row.
    pub row: u32,
    /// 0-based column.
    pub column: u32,
}

/// One painted cell: where the allocation put it, and where the paint drew it.
#[derive(Debug, Clone, Serialize)]
pub struct TextCellWire {
    /// The cell box's paint tag (`DocumentTag::cell`) — the same object
    /// `scene/access` announces, so the two surfaces join on it.
    pub tag: String,
    /// The paint tag of the row band this cell starts in.
    pub row_tag: String,
    /// 0-based row — the toolkit `row()`.
    pub row: u32,
    /// 0-based column — the toolkit `column()`.
    pub column: u32,
    /// Rows covered, after clamping — the toolkit `rowSpan()`.
    pub row_span: u16,
    /// Columns covered, after clamping — the toolkit `columnSpan()`.
    pub column_span: u16,
    /// The row span the author declared.
    pub declared_row_span: u16,
    /// The column span the author declared, which is what makes
    /// [`Self::clamped`] readable rather than merely assertable.
    pub declared_column_span: u16,
    /// The declared column span did not fit and was reduced — the fact the
    /// toolkit's `void mergeCells` throws away.
    pub clamped: bool,
    /// Which header band the address falls in: `none`, `column`, `row` or
    /// `corner`.
    pub header: String,
    /// 0-based ordinal among the table's cells, in flow order.
    pub index: u32,
    /// The paint tags of the paragraphs in this cell, in order — the join to
    /// `scene/text_blocks`, which is where their text and shaped lines live.
    ///
    /// The text itself is deliberately not repeated here: it is the author's
    /// content rather than this derivation's output, and a second copy is a
    /// second thing to disagree.
    pub blocks: Vec<String>,
    /// The cell box's window-absolute left edge, or `null` before layout.
    pub x: Option<i64>,
    /// The cell box's window-absolute top edge.
    pub y: Option<i64>,
    /// The cell box's width.
    pub width: Option<u32>,
    /// The cell box's height.
    pub height: Option<u32>,
}

/// One painted table: what it declared, what shape it turned out to be, and
/// every cell in it in flow order.
#[derive(Debug, Clone, Serialize)]
pub struct TextTableWire {
    /// The table container's paint tag (`DocumentTag::table`).
    pub tag: String,
    /// Rows the allocation needed — the toolkit `rows()`.
    pub rows: u32,
    /// Columns the format declared — the toolkit `columns()`.
    pub columns: u32,
    /// How many cells the table holds; a multi-block cell counts once.
    pub cell_count: u32,
    /// Leading header rows — the toolkit `headerRowCount`.
    pub header_rows: u16,
    /// Leading header columns — the axis the toolkit has no property for.
    pub header_columns: u16,
    /// The column tracks, in CSS `grid-template-columns` spelling
    /// (`auto` / `120px` / `30%` / `1fr` / `min-content` / `max-content`), one
    /// per column.
    ///
    /// Resolved rather than echoed: a format that named fewer widths than it
    /// has columns is padded before it reaches the layout, so what is
    /// published is what the tracks actually are.
    pub column_widths: Vec<String>,
    /// Padding inside each cell, px — the toolkit `cellPadding`.
    pub cell_padding_px: u32,
    /// Gap between cells, px — the toolkit `cellSpacing`.
    pub cell_spacing_px: u32,
    /// Rule width around each cell, px — the toolkit `border`.
    pub border_px: u32,
    /// The table container's window-absolute left edge, or `null` before
    /// layout.
    pub x: Option<i64>,
    /// The table container's window-absolute top edge.
    pub y: Option<i64>,
    /// The table container's width.
    pub width: Option<u32>,
    /// The table container's height.
    pub height: Option<u32>,
    /// The slots no cell covers, in row-major order — a state text table
    /// cannot be in.
    pub slack: Vec<GridSlotWire>,
    /// The cells, in flow order.
    pub cells: Vec<TextCellWire>,
}

/// Response payload for `scene/text_tables`.
#[derive(Debug, Clone, Serialize)]
pub struct TextTablesOutcome {
    /// Every painted table, in the order its first cell was painted.
    pub tables: Vec<TextTableWire>,
}

/// Build the `scene/text_tables` response from the last painted scene.
///
/// # Errors
///
/// A serialization failure, unreachable in practice for owned strings and
/// numbers; surfaced rather than unwrapped so an RPC handler never panics the
/// shell.
pub fn handle_scene_text_tables(last_paint_scene: Option<&Scene>) -> Result<Value, RpcError> {
    let tables = last_paint_scene.map(collect_tables).unwrap_or_default();
    serde_json::to_value(TextTablesOutcome { tables }).map_err(RpcError::internal_error)
}

/// Every painted table in `scene`, in the order their first cells were
/// painted.
///
/// The whole census reads ONE field — the [`CellPlacement`] the addressing left
/// on each paragraph's text node — so it cannot disagree with the grid areas on
/// screen or with the `aria-colindex` the a11y pass announces: all three read
/// the same derivation rather than each recomputing it from a document order
/// that only the view has.
#[must_use]
pub fn collect_tables(scene: &Scene) -> Vec<TextTableWire> {
    // Indexed, not scanned — twice over, and the second one is what mattered.
    //
    // Resolving each cell's box by tag walks the whole scene per cell, and
    // MEASURED on a 5,000-cell table that was 0.92s of a 0.92s census: the
    // whole cost. (The per-cell lookup into the rows already collected is
    // quadratic too — the shape R1559 recorded and this round lifted to
    // `NodeIndex` — but measuring first showed it was the smaller term by an
    // order of magnitude. Both are indexed here; only one of them was worth
    // guessing about.)
    let rects = scene.absolute_rects_by_tag();
    let mut tables: Vec<TextTableWire> = Vec::new();
    let mut table_at: HashMap<String, usize> = HashMap::new();
    let mut cell_at: HashMap<String, (usize, usize)> = HashMap::new();
    scene.for_each_text_leaf(|node, _, _| {
        let (Some(placement), Some(tag)) = (node.cell.as_deref(), node.tag.as_deref()) else {
            return;
        };
        let table = *table_at
            .entry(placement.table_tag.clone())
            .or_insert_with(|| {
                tables.push(table_row(placement, &rects));
                tables.len() - 1
            });
        let Some(row) = tables.get_mut(table) else {
            return;
        };
        // A continuation paragraph joins the cell its opener created, which is
        // how a multi-block cell is one row here and one node in the AT tree.
        if let Some((_, at)) = cell_at.get(&placement.cell_tag).copied()
            && let Some(cell) = row.cells.get_mut(at)
        {
            cell.blocks.push(tag.to_owned());
            return;
        }
        cell_at.insert(placement.cell_tag.clone(), (table, row.cells.len()));
        let table = row;
        let rect = rects.get(&placement.cell_tag).copied();
        table.cells.push(TextCellWire {
            tag: placement.cell_tag.clone(),
            row_tag: placement.row_tag.clone(),
            row: placement.row,
            column: placement.column,
            row_span: placement.row_span,
            column_span: placement.column_span,
            declared_row_span: placement.declared_row_span,
            declared_column_span: placement.declared_column_span,
            clamped: placement.clamped(),
            header: header_as_wire(placement),
            index: placement.index,
            blocks: vec![tag.to_owned()],
            x: rect.map(|r| i64::from(r.x)),
            y: rect.map(|r| i64::from(r.y)),
            width: rect.map(|r| r.w),
            height: rect.map(|r| r.h),
        });
    });
    for table in &mut tables {
        table.cell_count = u32::try_from(table.cells.len()).unwrap_or(u32::MAX);
        table.slack = slack_of(table);
    }
    tables
}

/// The declared half of a table's row, plus the box its container was laid out
/// in.
fn table_row(placement: &CellPlacement, rects: &HashMap<String, Rect>) -> TextTableWire {
    let format = &placement.format;
    let rect = rects.get(&placement.table_tag).copied();
    TextTableWire {
        tag: placement.table_tag.clone(),
        rows: placement.row_count,
        columns: placement.column_count,
        cell_count: 0,
        header_rows: format.header_rows,
        header_columns: format.header_columns,
        column_widths: format.tracks().into_iter().map(track_as_wire).collect(),
        cell_padding_px: format.cell_padding_px,
        cell_spacing_px: format.cell_spacing_px,
        border_px: format.border_px,
        x: rect.map(|r| i64::from(r.x)),
        y: rect.map(|r| i64::from(r.y)),
        width: rect.map(|r| r.w),
        height: rect.map(|r| r.h),
        slack: Vec::new(),
        cells: Vec::new(),
    }
}

/// The slots of `table`'s grid that none of its cells covers.
///
/// Recomputed from the published cells rather than carried from the
/// allocation, deliberately: it is then a statement about the rows this census
/// is reporting, so a caller can check it against them. Carrying it would make
/// the two independently wrong-able.
fn slack_of(table: &TextTableWire) -> Vec<GridSlotWire> {
    let mut covered = vec![false; usize::try_from(table.rows * table.columns).unwrap_or(0)];
    for cell in &table.cells {
        for r in 0..u32::from(cell.row_span) {
            for c in 0..u32::from(cell.column_span) {
                let row = cell.row + r;
                let column = cell.column + c;
                if row >= table.rows || column >= table.columns {
                    continue;
                }
                if let Some(slot) =
                    covered.get_mut(usize::try_from(row * table.columns + column).unwrap_or(0))
                {
                    *slot = true;
                }
            }
        }
    }
    covered
        .iter()
        .enumerate()
        .filter(|(_, taken)| !**taken)
        .filter_map(|(at, _)| {
            let at = u32::try_from(at).ok()?;
            Some(GridSlotWire {
                row: at / table.columns,
                column: at % table.columns,
            })
        })
        .collect()
}

/// A track in CSS `grid-template-columns` spelling.
///
/// CSS's own grammar rather than serde's enum encoding, because a wire value a
/// reader has to decode as `{"Px": 120}` is a Rust type leaking into a
/// protocol; `"120px"` is the string the same track would be written as in a
/// stylesheet.
fn track_as_wire(track: GridTrack) -> String {
    match track {
        GridTrack::Px(px) => format!("{px}px"),
        GridTrack::Percent(fraction) => format!("{}%", fraction * 100.0),
        GridTrack::Fr(units) => format!("{units}fr"),
        GridTrack::MinContent => "min-content".to_owned(),
        GridTrack::MaxContent => "max-content".to_owned(),
        _ => "auto".to_owned(),
    }
}

/// A cell's header scope as its ARIA-flavoured wire name.
fn header_as_wire(placement: &CellPlacement) -> String {
    serde_json::to_value(placement.header)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "none".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{collect_tables, handle_scene_text_tables, track_as_wire};
    use pinion_core::Scene;
    use pinion_core::scene::{ContainerNode, Rect, TextNode};
    use pinion_core::style::GridTrack;
    use pinion_core::text_table::{CellSpec, TableFormat, TablePart, place_cells};

    /// A painted table from `(text, colspan, rowspan, continues)` tuples, laid
    /// out on a 100px grid so the boxes are checkable.
    fn painted(format: &TableFormat, cells: &[(&str, u16, u16, bool)]) -> Scene {
        let specs: Vec<Option<CellSpec>> = cells
            .iter()
            .map(|(_, colspan, rowspan, continues)| {
                let spec = CellSpec::new(format.clone())
                    .spanning_columns(*colspan)
                    .spanning_rows(*rowspan);
                Some(if *continues { spec.continued() } else { spec })
            })
            .collect();
        let addressing = place_cells(&specs, |part| match part {
            TablePart::Table(k) => format!("doc_tbl{k}"),
            TablePart::Row(k, r) => format!("doc_tbl{k}r{r}"),
            TablePart::Cell(i) => format!("doc_cel{i}"),
        });
        let mut children = Vec::new();
        for (i, (text, ..)) in cells.iter().enumerate() {
            let placement = addressing.placements[i].clone().expect("addressed");
            let x = placement.column * 100;
            let y = placement.row * 40;
            if placement.opens_cell {
                let mut node = ContainerNode::new(Vec::new()).with_tag(placement.cell_tag.clone());
                node.rect = Rect::new(
                    x,
                    y,
                    100 * u32::from(placement.column_span),
                    40 * u32::from(placement.row_span),
                );
                children.push(Scene::Container(node));
            }
            children.push(Scene::Text(
                TextNode::new((*text).to_string(), Rect::new(x, y, 100, 20))
                    .with_tag(format!("doc_blk{i}"))
                    .with_cell_placement(placement),
            ));
        }
        let mut root = ContainerNode::new(children).with_tag("doc_tbl0");
        root.rect = Rect::new(0, 0, 300, 80);
        Scene::Container(root)
    }

    /// One row per table, one entry per cell, with the address and the painted
    /// box together — the join the toolkit cannot make from outside the
    /// process.
    #[test]
    fn a_table_publishes_its_shape_and_every_cell() {
        let scene = painted(
            &TableFormat::new(3),
            &[
                ("a", 1, 1, false),
                ("b", 1, 1, false),
                ("c", 1, 1, false),
                ("d", 1, 1, false),
            ],
        );
        let tables = collect_tables(&scene);
        assert_eq!(tables.len(), 1);
        let table = &tables[0];
        assert_eq!(table.tag, "doc_tbl0");
        assert_eq!((table.rows, table.columns, table.cell_count), (2, 3, 4));
        assert_eq!(table.column_widths, ["auto", "auto", "auto"]);
        assert_eq!((table.x, table.y), (Some(0), Some(0)));
        let third = &table.cells[2];
        assert_eq!(third.tag, "doc_cel2");
        assert_eq!((third.row, third.column), (0, 2));
        assert_eq!(third.blocks, ["doc_blk2"]);
        assert_eq!((third.x, third.y), (Some(200), Some(0)));
        assert_eq!(table.cells[3].row, 1);
    }

    /// A clamped span publishes both the ask and the result, and a ragged
    /// table publishes the slots nobody filled.
    #[test]
    fn a_clamp_and_the_slack_are_both_readable() {
        let scene = painted(
            &TableFormat::new(3),
            &[("wide", 9, 1, false), ("next", 1, 1, false)],
        );
        let table = &collect_tables(&scene)[0];
        let wide = &table.cells[0];
        assert_eq!(wide.column_span, 3);
        assert_eq!(wide.declared_column_span, 9);
        assert!(wide.clamped);
        assert!(!table.cells[1].clamped);
        assert_eq!(table.rows, 2);
        assert_eq!(
            table
                .slack
                .iter()
                .map(|s| (s.row, s.column))
                .collect::<Vec<_>>(),
            [(1, 1), (1, 2)],
            "the second row stops after one cell",
        );
    }

    /// A multi-block cell is one row with two block tags, not two rows.
    #[test]
    fn a_multi_block_cell_is_one_row_listing_both_paragraphs() {
        let scene = painted(
            &TableFormat::new(2),
            &[
                ("first", 1, 1, false),
                ("second", 1, 1, true),
                ("other", 1, 1, false),
            ],
        );
        let table = &collect_tables(&scene)[0];
        assert_eq!(table.cells.len(), 2);
        assert_eq!(table.cells[0].blocks, ["doc_blk0", "doc_blk1"]);
        assert_eq!(table.cell_count, 2);
    }

    /// The header band reaches the wire as its ARIA-flavoured name, derived
    /// from the address.
    #[test]
    fn the_header_band_is_named_on_the_wire() {
        let format = TableFormat::new(2)
            .with_header_rows(1)
            .with_header_columns(1);
        let scene = painted(
            &format,
            &[
                ("corner", 1, 1, false),
                ("col", 1, 1, false),
                ("row", 1, 1, false),
                ("data", 1, 1, false),
            ],
        );
        let table = &collect_tables(&scene)[0];
        let scopes: Vec<&str> = table.cells.iter().map(|c| c.header.as_str()).collect();
        assert_eq!(scopes, ["corner", "column", "row", "none"]);
        assert_eq!(table.header_rows, 1);
        assert_eq!(table.header_columns, 1);
    }

    /// Tracks reach the wire in CSS's spelling, not serde's.
    #[test]
    fn tracks_are_published_in_css_spelling() {
        assert_eq!(track_as_wire(GridTrack::Auto), "auto");
        assert_eq!(track_as_wire(GridTrack::Px(120)), "120px");
        assert_eq!(track_as_wire(GridTrack::Fr(1.0)), "1fr");
        assert_eq!(track_as_wire(GridTrack::Percent(0.25)), "25%");
        assert_eq!(track_as_wire(GridTrack::MinContent), "min-content");
        assert_eq!(track_as_wire(GridTrack::MaxContent), "max-content");
    }

    /// The negative controls: no scene and a scene with no table both answer
    /// an empty array rather than an error.
    #[test]
    fn a_window_with_no_table_answers_an_empty_census() {
        let value = handle_scene_text_tables(None).expect("serializes");
        assert_eq!(value["tables"].as_array().expect("an array").len(), 0);
        let plain = Scene::Container(ContainerNode::new(vec![Scene::Text(
            TextNode::new("plain".to_string(), Rect::new(0, 0, 40, 20)).with_tag("doc_blk0"),
        )]));
        assert!(collect_tables(&plain).is_empty());
    }
}
