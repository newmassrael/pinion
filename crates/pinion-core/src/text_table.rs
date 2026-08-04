//! R1560 §5.36 §5.40 — the **table** format and the cell addressing derived
//! from it (Qt `QTextTable` / `QTextTableFormat`; the HTML table model's slot
//! allocation and CSS Grid placement).
//!
//! # What a table is, and why its addresses cannot be hand-written
//!
//! The R1559 argument one dimension up. Everything else a table has can be
//! written by hand: a border is a stroke, a padding is an inset, a column width
//! is a length. What cannot be written by hand is a cell's **address**, because
//! `(row, column)` is not a property of the cell — it is a property of the
//! cell's *place in the flow*, once every earlier cell's spans have taken the
//! slots they take. Put one `colspan = 2` cell at the front and every later
//! cell in the table moves; give a cell a `rowspan` and it reaches down into a
//! row that has not been written yet, pushing that row's cells to the right.
//!
//! So a table is the block structure whose geometry is a function of the
//! sequence, and that is what this module derives. The author states which
//! blocks are cells and how far each one reaches; where each one lands is
//! [`place_cells`]'s answer.
//!
//! # Against Qt 6.11
//!
//! Qt has the concept and reaches it from the other end. A `QTextTable` is a
//! `QTextFrame` owned by a `QTextDocument`, built by `insertRows` /
//! `insertColumns` into a **rectangular grid the caller maintains**, and spans
//! exist only as `mergeCells(row, column, numRows, numCols)` applied afterwards
//! to a rectangle the caller has to work out. Five things here go past it:
//!
//! * **The address is derived, not maintained.** In Qt the author owns the
//!   grid and the merges; here the author owns neither, so a table cannot be
//!   internally inconsistent — there is no second copy of the geometry to
//!   disagree with the first.
//! * **A span that does not fit is clamped and NAMED.** `mergeCells` returns
//!   `void` and silently does nothing when the rectangle is not a merge
//!   candidate, so a Qt caller learns about a bad merge by looking at the
//!   screen. [`CellPlacement`] carries both the declared span and the one it
//!   got ([`CellPlacement::clamped`]), so the difference is data.
//! * **A table may be ragged.** Qt's grid is always full: `insertRows` creates
//!   every cell. Here the last row can simply stop, and the slots nobody filled
//!   are reported ([`TableRun::slack`]) rather than being an unrepresentable
//!   state.
//! * **Header COLUMNS.** Qt has `QTextTableFormat::headerRowCount` and no
//!   column equivalent, and its purpose is pagination — repeating the header
//!   when the table breaks across pages. [`TableFormat::header_columns`] is the
//!   missing half, and header-ness is *derived from the address*
//!   ([`CellPlacement::header`]) rather than declared per cell, which is the
//!   same argument as the numbering: a cell that moves into the header row
//!   becomes a header.
//! * **The addressing is data.** `QTextTableCell::row()` is an in-process C++
//!   call; the derivation here rides the painted scene and is published by
//!   `scene/text_tables`, so an agent can read a document's table structure
//!   without pixels and without being in-process (§2 #7).
//!
//! # What this module does NOT decide
//!
//! Where a cell is *painted* is the composing view's business
//! (`pinion_widget_paint::document`), which lowers each address onto the CSS
//! Grid placement the layout engine understands
//! ([`crate::style::GridPlacement`]). This module owns the vocabulary and the
//! allocation.

use crate::style::{Color, GridTrack};

/// Which axis a cell is a header for — the `scope` of an HTML `<th>`.
///
/// Derived from the cell's address against [`TableFormat::header_rows`] /
/// [`TableFormat::header_columns`], never declared per cell, so a cell that
/// moves into a header band becomes a header and one that moves out stops
/// being one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeaderScope {
    /// An ordinary data cell — HTML `<td>`, WAI-ARIA `cell`.
    #[default]
    None,
    /// In a header row: this cell labels its **column** — HTML
    /// `<th scope="col">`, WAI-ARIA `columnheader`.
    Column,
    /// In a header column: this cell labels its **row** — HTML
    /// `<th scope="row">`, WAI-ARIA `rowheader`.
    Row,
    /// In both bands at once — the corner cell above the row labels and left
    /// of the column labels.
    ///
    /// It is announced as a `columnheader`, which is what HTML's own table
    /// model does with a corner `<th>` that states no scope: reading order
    /// runs along the header row, and the corner cell is the label of the
    /// column the row headers are in.
    Corner,
}

impl HeaderScope {
    /// Whether this cell is a header of either kind.
    #[must_use]
    pub const fn is_header(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// A table's declared format — Qt `QTextTableFormat`, plus the column-header
/// band Qt does not have.
///
/// Carried on every [`CellSpec`] rather than stated once, for
/// [`crate::text_list::ListFormat`]'s reason: the blocks are a flat sequence
/// and a table is a *run* discovered within it, so there is no earlier place to
/// put the declaration. A run ends when the format changes, which is what makes
/// two tables separated by nothing still two tables.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TableFormat {
    /// How many columns the table has — Qt `QTextTable::columns()`.
    ///
    /// Declared rather than derived from the widest row, because it is what
    /// makes the flow *wrap*: a cell that reaches the last column puts the
    /// next one on a new row. HTML derives this from the widest row precisely
    /// because its rows are explicit (`<tr>`); a flat block sequence has no
    /// row delimiter, so the width is the declaration and the rows are the
    /// derivation. Zero is read as one — a table with no columns has nowhere
    /// to put a cell.
    pub columns: u16,
    /// Per-column track sizing — Qt
    /// `QTextTableFormat::setColumnWidthConstraints`, CSS
    /// `grid-template-columns`.
    ///
    /// Shorter than [`Self::columns`] is padded with [`GridTrack::Auto`] and
    /// longer is truncated ([`Self::tracks`]), so the two can never describe
    /// different tables. Qt keeps the same vector and simply ignores the
    /// mismatch.
    pub column_widths: Vec<GridTrack>,
    /// How many leading rows are header rows — Qt
    /// `QTextTableFormat::headerRowCount`.
    pub header_rows: u16,
    /// How many leading columns are header columns — the axis Qt has no
    /// property for.
    pub header_columns: u16,
    /// Space between a cell's border and its content, in px — Qt
    /// `QTextTableFormat::cellPadding`.
    pub cell_padding_px: u32,
    /// Space between neighbouring cells, in px — Qt
    /// `QTextTableFormat::cellSpacing`, CSS `grid-gap`.
    pub cell_spacing_px: u32,
    /// The width of the rule drawn around each cell, in px — Qt
    /// `QTextFrameFormat::border` as it applies to a table.
    pub border_px: u32,
    /// The colour of that rule — Qt `QTextFrameFormat::setBorderBrush`.
    ///
    /// On the format rather than taken from the composing view's palette
    /// because it is a property of the *table*, the way Qt has it: two tables
    /// in one document can rule differently, and a view that owned the colour
    /// could not let them.
    pub border_color: Color,
}

impl TableFormat {
    /// A table `columns` wide with every default: auto columns, no header
    /// bands, no padding, no spacing, a hairline border.
    #[must_use]
    pub fn new(columns: u16) -> Self {
        Self {
            columns,
            column_widths: Vec::new(),
            header_rows: 0,
            header_columns: 0,
            cell_padding_px: 4,
            cell_spacing_px: 0,
            border_px: 1,
            // A mid grey: legible against both a light and a dark page without
            // the format having to know which it is on. A view with a palette
            // overrides it (`with_border`).
            border_color: Color::rgb(0x9E, 0x9E, 0x9E),
        }
    }

    /// Builder: per-column track sizing.
    #[must_use]
    pub fn with_column_widths(mut self, widths: Vec<GridTrack>) -> Self {
        self.column_widths = widths;
        self
    }

    /// Builder: the leading `rows` are header rows (Qt `headerRowCount`).
    #[must_use]
    pub const fn with_header_rows(mut self, rows: u16) -> Self {
        self.header_rows = rows;
        self
    }

    /// Builder: the leading `columns` are header columns.
    #[must_use]
    pub const fn with_header_columns(mut self, columns: u16) -> Self {
        self.header_columns = columns;
        self
    }

    /// Builder: cell padding and cell spacing, in px.
    #[must_use]
    pub const fn with_metrics(mut self, padding_px: u32, spacing_px: u32) -> Self {
        self.cell_padding_px = padding_px;
        self.cell_spacing_px = spacing_px;
        self
    }

    /// Builder: the rule drawn around each cell — Qt
    /// `QTextFrameFormat::setBorder` + `setBorderBrush`.
    #[must_use]
    pub const fn with_border(mut self, width_px: u32, color: Color) -> Self {
        self.border_px = width_px;
        self.border_color = color;
        self
    }

    /// The effective column count — [`Self::columns`], never zero.
    #[must_use]
    pub const fn column_count(&self) -> u16 {
        if self.columns == 0 { 1 } else { self.columns }
    }

    /// The column tracks, resolved to exactly [`Self::column_count`] entries.
    ///
    /// Padding a short list rather than rejecting it is deliberate: a table
    /// that says "the first column is 120px" and leaves the rest alone is the
    /// common case, and requiring the author to spell `Auto` for every other
    /// column would be a second place for the column count to be stated.
    #[must_use]
    pub fn tracks(&self) -> Vec<GridTrack> {
        let count = usize::from(self.column_count());
        let mut tracks = self.column_widths.clone();
        tracks.truncate(count);
        tracks.resize(count, GridTrack::Auto);
        tracks
    }
}

/// A block's declared table membership — the authored half: under what format,
/// how far this cell reaches, and whether it opens a cell or continues the
/// previous one.
///
/// An author never states an address. Grouping consecutive members into tables
/// and allocating their slots is [`place_cells`]'s job.
#[derive(Debug, Clone, PartialEq)]
pub struct CellSpec {
    /// The format the enclosing table declares.
    pub format: TableFormat,
    /// How many rows this cell reaches down — HTML `rowspan`, the `numRows`
    /// of Qt's `mergeCells`. `0` is read as `1`.
    pub row_span: u16,
    /// How many columns this cell reaches across — HTML `colspan`. `0` is read
    /// as `1`.
    pub column_span: u16,
    /// This block continues the **previous** block's cell rather than opening
    /// one of its own — Qt's cell is a frame holding any number of blocks, and
    /// this is the flat-sequence spelling of that.
    ///
    /// It is a declaration and not a derivation because nothing about a
    /// paragraph says whether it belongs with the one before it. Continuing
    /// when there is no previous cell in this table opens a cell instead:
    /// a continuation of nothing is a beginning.
    pub continues: bool,
}

impl CellSpec {
    /// A one-slot cell of a table with `format`.
    #[must_use]
    pub fn new(format: TableFormat) -> Self {
        Self {
            format,
            row_span: 1,
            column_span: 1,
            continues: false,
        }
    }

    /// Builder: reach across `columns` columns (HTML `colspan`).
    #[must_use]
    pub const fn spanning_columns(mut self, columns: u16) -> Self {
        self.column_span = columns;
        self
    }

    /// Builder: reach down `rows` rows (HTML `rowspan`).
    #[must_use]
    pub const fn spanning_rows(mut self, rows: u16) -> Self {
        self.row_span = rows;
        self
    }

    /// Builder: this block continues the previous cell (see [`Self::continues`]).
    #[must_use]
    pub const fn continued(mut self) -> Self {
        self.continues = true;
        self
    }
}

/// Which part of a table a paint tag names — the argument
/// [`place_cells`]'s tag function is asked with.
///
/// One function rather than three because the three tags are one naming
/// scheme, and a caller that could supply two of them and forget the third
/// would produce a table whose parts do not share a prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TablePart {
    /// The `k`-th table the walk discovered, in document order.
    Table(usize),
    /// Row `row` (0-based) of the `k`-th table.
    Row(usize, u32),
    /// The cell opened by the block at index `i`.
    Cell(usize),
}

/// Where one cell landed — the derived half, and the only place a cell's
/// address exists.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CellPlacement {
    /// The paint tag of the table this cell belongs to.
    pub table_tag: String,
    /// The paint tag of the cell's own box. Every block of a multi-block cell
    /// carries the same one, which is what makes them one cell.
    pub cell_tag: String,
    /// The paint tag of the row this cell starts in.
    pub row_tag: String,
    /// 0-based row — Qt `QTextTableCell::row()`.
    pub row: u32,
    /// 0-based column — Qt `QTextTableCell::column()`.
    pub column: u32,
    /// Rows this cell covers after clamping — Qt `QTextTableCell::rowSpan()`.
    pub row_span: u16,
    /// Columns this cell covers after clamping — Qt
    /// `QTextTableCell::columnSpan()`.
    pub column_span: u16,
    /// The column span the author asked for, kept beside the one they got.
    ///
    /// Qt's `mergeCells` discards this distinction: an impossible merge is a
    /// silent no-op, so the request and the result are indistinguishable
    /// afterwards.
    pub declared_column_span: u16,
    /// The row span the author asked for. Never clamped today — a table grows
    /// downwards, so there is no bound to hit — and carried anyway so the pair
    /// is symmetric and a future bound has somewhere to be reported.
    pub declared_row_span: u16,
    /// How many rows the finished table has (Qt `QTextTable::rows()`), stamped
    /// once the table ends.
    pub row_count: u32,
    /// How many columns the finished table has (Qt `QTextTable::columns()`).
    pub column_count: u32,
    /// Which axis, if any, this cell is a header for — derived from the
    /// address.
    pub header: HeaderScope,
    /// 0-based ordinal of this cell among the table's cells, in flow order.
    pub index: u32,
    /// This block **opens** the cell. A continuation block is `false`, which is
    /// how a view knows to put it in the box the previous block already made.
    pub opens_cell: bool,
    /// The table's declared format, kept beside the addressing it produced for
    /// [`crate::style::BlockFormat`]'s reason: an address cannot be read back
    /// as the declaration that made it.
    pub format: TableFormat,
}

impl CellPlacement {
    /// The declared column span did not fit in the row and was reduced.
    ///
    /// The one thing Qt cannot report, because `mergeCells` answers `void`.
    #[must_use]
    pub const fn clamped(&self) -> bool {
        self.declared_column_span > self.column_span
    }
}

/// One slot of a table's grid that no cell covers.
///
/// A state Qt's table model cannot be in: `QTextTable` fills its grid on
/// construction, so every slot has a cell whether the author wanted one or
/// not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GridSlot {
    /// 0-based row.
    pub row: u32,
    /// 0-based column.
    pub column: u32,
}

/// One table the addressing discovered — the object Qt calls a `QTextTable`.
///
/// Reported beside the per-cell placements because a table has facts that are
/// not any one cell's: how many rows it turned out to have, and which of its
/// slots nobody filled.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TableRun {
    /// The table's paint tag.
    pub tag: String,
    /// Discovery order among the document's tables.
    pub index: usize,
    /// Rows the allocation needed — Qt `QTextTable::rows()`.
    pub rows: u32,
    /// Columns the format declared — Qt `QTextTable::columns()`.
    pub columns: u32,
    /// How many cells the table holds (a multi-block cell counts once).
    pub cells: u32,
    /// The slots no cell covers, in row-major order.
    pub slack: Vec<GridSlot>,
    /// The table's declared format.
    pub format: TableFormat,
}

/// The result of [`place_cells`]: one placement per input block (or `None` for
/// a block that is not in a table), and one summary per table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableAddressing {
    /// Parallel to the input: where each block's cell landed.
    pub placements: Vec<Option<CellPlacement>>,
    /// Every table the walk discovered, in document order.
    pub runs: Vec<TableRun>,
}

impl TableAddressing {
    /// The run with paint tag `tag`, if the walk discovered one.
    #[must_use]
    pub fn run(&self, tag: &str) -> Option<&TableRun> {
        self.runs.iter().find(|run| run.tag == tag)
    }
}

/// One table being filled while the walk is inside it.
struct Open {
    index: usize,
    tag: String,
    format: TableFormat,
    /// `[row][column]` occupancy, grown a row at a time. A `rowspan` marks
    /// rows that no cell has been *written* to yet, which is exactly why the
    /// allocation cannot be a per-row loop.
    occupied: Vec<Vec<bool>>,
    /// Where the next cell starts looking. Never moves backwards, which is
    /// what makes the allocation linear and is HTML's own rule.
    cursor: (u32, u32),
    cells: u32,
    /// Output indices of every block placed in this table, so the counts can
    /// be stamped once the table ends.
    blocks: Vec<usize>,
    /// The output index of the block that opened the current cell, for a
    /// continuation to copy.
    last_opened: Option<usize>,
}

/// Derive every cell's address from the declared membership and spans.
///
/// Consecutive blocks whose [`CellSpec`] shares one [`TableFormat`] are one
/// table; a block with no spec, or one whose format differs, ends it. That is
/// [`crate::text_list::number_blocks`]'s rule, and it holds for the same
/// reason: the blocks are a flat sequence, so a structure inside it is a *run*,
/// and a run ends where its declaration stops being restated.
///
/// `tag` names the parts (see [`TablePart`]); it is called once per part, and
/// the strings it returns are what the paint, the assistive-technology tree and
/// the wire all address.
///
/// # The allocation
///
/// HTML's own "forming a table" algorithm, which CSS Grid restates as
/// auto-placement: a cursor scans row-major for the first slot no earlier cell
/// covers, the cell takes a rectangle from there, and the cursor moves to the
/// far side of it. A column span wider than the row is clamped — CSS's rule
/// (a span that overflows the explicit grid is clamped) rather than HTML's
/// (which widens the table), because the column count is this table's *declared*
/// width and silently widening it would make the declaration a lie.
#[must_use]
pub fn place_cells(
    specs: &[Option<CellSpec>],
    tag: impl Fn(TablePart) -> String,
) -> TableAddressing {
    let mut out: Vec<Option<CellPlacement>> = vec![None; specs.len()];
    let mut current: Option<Open> = None;
    let mut closed: Vec<Open> = Vec::new();
    let mut next_table = 0usize;

    for (i, spec) in specs.iter().enumerate() {
        let Some(spec) = spec else {
            if let Some(open) = current.take() {
                closed.push(open);
            }
            continue;
        };
        // A table whose format changed is a different table — the same rule
        // `number_blocks` applies to a list, and the only way two adjacent
        // tables can be told apart in a flat sequence.
        if current
            .as_ref()
            .is_some_and(|open| open.format != spec.format)
            && let Some(open) = current.take()
        {
            closed.push(open);
        }
        let open = current.get_or_insert_with(|| {
            let index = next_table;
            next_table += 1;
            Open {
                index,
                tag: tag(TablePart::Table(index)),
                format: spec.format.clone(),
                occupied: Vec::new(),
                cursor: (0, 0),
                cells: 0,
                blocks: Vec::new(),
                last_opened: None,
            }
        });

        // A continuation joins the cell the previous block opened, so it takes
        // that cell's whole address rather than allocating one.
        if spec.continues
            && let Some(previous) = open.last_opened
            && let Some(placement) = out[previous].clone()
        {
            out[i] = Some(CellPlacement {
                opens_cell: false,
                ..placement
            });
            open.blocks.push(i);
            continue;
        }

        let columns = open.format.column_count();
        let (row, column) = next_free_slot(&mut open.occupied, columns, open.cursor);
        let declared_column_span = spec.column_span.max(1);
        let declared_row_span = spec.row_span.max(1);
        // Clamped to the FREE run, not merely to the row's width: a cell that
        // reached past a slot an earlier cell's `rowspan` already holds would
        // put two cells in one slot. HTML calls that a table model error and
        // renders the overlap anyway; CSS Grid allows overlap outright for
        // explicitly placed items. Neither is available here, because the
        // author did not compute this address — the allocation did, so an
        // overlap would be this function's mistake rather than a declaration
        // to be honoured. Clamping is the only total answer, and it makes
        // "no two cells share a slot" a property of the derivation.
        let column_span = declared_column_span
            .min(free_run(&open.occupied, columns, row, column))
            .max(1);
        occupy(
            &mut open.occupied,
            columns,
            row,
            column,
            declared_row_span,
            column_span,
        );
        open.cursor = (row, column + u32::from(column_span));

        let header = header_scope(&open.format, row, column);
        out[i] = Some(CellPlacement {
            table_tag: open.tag.clone(),
            cell_tag: tag(TablePart::Cell(i)),
            row_tag: tag(TablePart::Row(open.index, row)),
            row,
            column,
            row_span: declared_row_span,
            column_span,
            declared_column_span,
            declared_row_span,
            // Provisional: stamped with the finished table's extent below,
            // because a table does not know how big it is until it ends.
            row_count: 0,
            column_count: u32::from(columns),
            header,
            index: open.cells,
            opens_cell: true,
            format: open.format.clone(),
        });
        open.cells += 1;
        open.last_opened = Some(i);
        open.blocks.push(i);
    }
    if let Some(open) = current.take() {
        closed.push(open);
    }

    let mut runs: Vec<TableRun> = closed
        .into_iter()
        .map(|open| finish_table(open, &mut out))
        .collect();
    runs.sort_by_key(|run| run.index);

    TableAddressing {
        placements: out,
        runs,
    }
}

/// Close one table: stamp its finished extent onto every block it holds, and
/// summarise it.
///
/// Separate from the allocation because it answers a question the allocation
/// cannot: how big the table turned out to be. Every cell was placed before
/// the last row existed, so the extent is written back rather than known while
/// the walk is inside the table — the shape `number_blocks` uses for a list's
/// `count`.
fn finish_table(open: Open, out: &mut [Option<CellPlacement>]) -> TableRun {
    let columns = u32::from(open.format.column_count());
    let rows = open
        .blocks
        .iter()
        .filter_map(|i| out[*i].as_ref())
        .map(|p| p.row + u32::from(p.row_span))
        .max()
        .unwrap_or(0);
    for i in &open.blocks {
        if let Some(placement) = out[*i].as_mut() {
            placement.row_count = rows;
        }
    }
    let mut unfilled = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            let covered = open
                .occupied
                .get(usize::try_from(row).unwrap_or(usize::MAX))
                .and_then(|cells| cells.get(usize::try_from(column).unwrap_or(usize::MAX)))
                .copied()
                .unwrap_or(false);
            if !covered {
                unfilled.push(GridSlot { row, column });
            }
        }
    }
    TableRun {
        tag: open.tag,
        index: open.index,
        rows,
        columns,
        cells: open.cells,
        slack: unfilled,
        format: open.format,
    }
}

/// Which header band, if either, the cell at `(row, column)` is in.
fn header_scope(format: &TableFormat, row: u32, column: u32) -> HeaderScope {
    let in_header_row = row < u32::from(format.header_rows);
    let in_header_column = column < u32::from(format.header_columns);
    match (in_header_row, in_header_column) {
        (true, true) => HeaderScope::Corner,
        (true, false) => HeaderScope::Column,
        (false, true) => HeaderScope::Row,
        (false, false) => HeaderScope::None,
    }
}

/// Grow `occupied` until row `row` exists, then answer it.
fn row_mut(occupied: &mut Vec<Vec<bool>>, columns: u16, row: u32) -> &mut Vec<bool> {
    let row = usize::try_from(row).unwrap_or(usize::MAX);
    while occupied.len() <= row {
        occupied.push(vec![false; usize::from(columns)]);
    }
    &mut occupied[row]
}

/// The first slot at or after `cursor` that no earlier cell covers, scanning
/// row-major and wrapping at `columns`.
fn next_free_slot(occupied: &mut Vec<Vec<bool>>, columns: u16, cursor: (u32, u32)) -> (u32, u32) {
    let (mut row, mut column) = cursor;
    loop {
        if column >= u32::from(columns) {
            column = 0;
            row = row.saturating_add(1);
            continue;
        }
        let cells = row_mut(occupied, columns, row);
        let index = usize::try_from(column).unwrap_or(usize::MAX);
        if cells.get(index).copied().unwrap_or(false) {
            column += 1;
            continue;
        }
        return (row, column);
    }
}

/// How many consecutive slots from `(row, column)` no earlier cell covers.
///
/// At least one, because the caller only asks about a slot
/// [`next_free_slot`] has just answered with.
fn free_run(occupied: &[Vec<bool>], columns: u16, row: u32, column: u32) -> u16 {
    let cells = occupied.get(usize::try_from(row).unwrap_or(usize::MAX));
    let mut free = 0u16;
    let mut probe = column;
    while probe < u32::from(columns) {
        let covered = cells
            .and_then(|cells| cells.get(usize::try_from(probe).unwrap_or(usize::MAX)))
            .copied()
            .unwrap_or(false);
        if covered {
            break;
        }
        free = free.saturating_add(1);
        probe += 1;
    }
    free.max(1)
}

/// Mark the `row_span` x `column_span` rectangle at `(row, column)` covered.
fn occupy(
    occupied: &mut Vec<Vec<bool>>,
    columns: u16,
    row: u32,
    column: u32,
    row_span: u16,
    column_span: u16,
) {
    for r in 0..u32::from(row_span) {
        let cells = row_mut(occupied, columns, row.saturating_add(r));
        for c in 0..u32::from(column_span) {
            let index = usize::try_from(column.saturating_add(c)).unwrap_or(usize::MAX);
            if let Some(slot) = cells.get_mut(index) {
                *slot = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CellSpec, GridSlot, HeaderScope, TableFormat, TablePart, place_cells};
    use crate::style::GridTrack;

    fn tags(part: TablePart) -> String {
        match part {
            TablePart::Table(k) => format!("t{k}"),
            TablePart::Row(k, r) => format!("t{k}r{r}"),
            TablePart::Cell(i) => format!("c{i}"),
        }
    }

    fn cells(specs: &[Option<CellSpec>]) -> super::TableAddressing {
        place_cells(specs, tags)
    }

    fn plain(format: &TableFormat, n: usize) -> Vec<Option<CellSpec>> {
        (0..n)
            .map(|_| Some(CellSpec::new(format.clone())))
            .collect()
    }

    fn addresses(a: &super::TableAddressing) -> Vec<(u32, u32)> {
        a.placements
            .iter()
            .filter_map(|p| p.as_ref().map(|p| (p.row, p.column)))
            .collect()
    }

    /// The base case: cells fill row-major and wrap at the declared width.
    #[test]
    fn cells_wrap_at_the_declared_column_count() {
        let a = cells(&plain(&TableFormat::new(3), 5));
        assert_eq!(addresses(&a), [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1)]);
        let run = a.run("t0").expect("one table");
        assert_eq!((run.rows, run.columns, run.cells), (2, 3, 5));
    }

    /// The defining property: inserting one wide cell re-addresses every cell
    /// after it. Nothing an author wrote changed.
    #[test]
    fn a_wider_cell_re_addresses_everything_after_it() {
        let format = TableFormat::new(3);
        let mut specs = plain(&format, 5);
        assert_eq!(
            addresses(&cells(&specs)),
            [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1)]
        );
        specs.insert(0, Some(CellSpec::new(format).spanning_columns(2)));
        assert_eq!(
            addresses(&cells(&specs)),
            [(0, 0), (0, 2), (1, 0), (1, 1), (1, 2), (2, 0)],
            "one insertion, five addresses changed",
        );
    }

    /// A row span reaches into rows that have not been written yet, so the
    /// cells of the next row start to its right — the case a per-row loop
    /// cannot express and Qt makes the caller `mergeCells` by hand.
    #[test]
    fn a_row_span_pushes_the_next_rows_cells_aside() {
        let format = TableFormat::new(3);
        let mut specs = vec![Some(CellSpec::new(format.clone()).spanning_rows(2))];
        specs.extend(plain(&format, 4));
        let a = cells(&specs);
        assert_eq!(
            addresses(&a),
            [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2)],
            "row 1 starts at column 1 because the tall cell holds column 0",
        );
        let run = a.run("t0").expect("one table");
        assert_eq!(
            run.rows, 2,
            "the span decides the height, not the cell count"
        );
        assert!(run.slack.is_empty(), "and the grid is full");
    }

    /// A span that does not fit is clamped AND the request survives beside the
    /// result — the distinction Qt's `void mergeCells` throws away.
    #[test]
    fn an_oversized_span_is_clamped_and_named() {
        let format = TableFormat::new(3);
        let a = cells(&[
            Some(CellSpec::new(format.clone())),
            Some(CellSpec::new(format).spanning_columns(5)),
        ]);
        let wide = a.placements[1].as_ref().expect("placed");
        assert_eq!((wide.row, wide.column), (0, 1));
        assert_eq!(wide.column_span, 2, "clamped to what was left of the row");
        assert_eq!(
            wide.declared_column_span, 5,
            "and the ask is still readable"
        );
        assert!(wide.clamped());
        assert!(!a.placements[0].as_ref().expect("placed").clamped());
    }

    /// A span is clamped by what an earlier cell already holds, not only by
    /// the row's width — so the allocation cannot put two cells in one slot.
    /// HTML calls that a table model error and renders it; the derivation here
    /// cannot produce one.
    #[test]
    fn a_span_stops_at_a_slot_an_earlier_cell_holds() {
        let format = TableFormat::new(3);
        let a = cells(&[
            Some(CellSpec::new(format.clone()).spanning_columns(2)),
            Some(CellSpec::new(format.clone()).spanning_rows(2)),
            Some(CellSpec::new(format).spanning_columns(9)),
        ]);
        let tall = a.placements[1].as_ref().expect("placed");
        assert_eq!((tall.row, tall.column, tall.row_span), (0, 2, 2));
        let over = a.placements[2].as_ref().expect("placed");
        assert_eq!((over.row, over.column), (1, 0));
        assert_eq!(
            over.column_span, 2,
            "row 1 has three columns, but the tall cell holds the third",
        );
        assert_eq!(over.declared_column_span, 9);
        assert!(over.clamped());
        // The property the clamp exists for, stated directly.
        let mut seen: Vec<(u32, u32)> = Vec::new();
        for placement in a.placements.iter().flatten() {
            for r in 0..u32::from(placement.row_span) {
                for c in 0..u32::from(placement.column_span) {
                    let slot = (placement.row + r, placement.column + c);
                    assert!(!seen.contains(&slot), "{slot:?} claimed twice");
                    seen.push(slot);
                }
            }
        }
    }

    /// A ragged table is representable, and the slots nobody filled are
    /// reported — a state `QTextTable` cannot be in at all.
    #[test]
    fn an_unfinished_row_reports_its_slack() {
        let a = cells(&plain(&TableFormat::new(4), 6));
        let run = a.run("t0").expect("one table");
        assert_eq!(run.rows, 2);
        assert_eq!(
            run.slack,
            [
                GridSlot { row: 1, column: 2 },
                GridSlot { row: 1, column: 3 }
            ]
        );
    }

    /// Header-ness is derived from the address, so a cell that moves into the
    /// header band becomes a header without anything being re-declared.
    #[test]
    fn a_header_is_derived_from_where_the_cell_landed() {
        let format = TableFormat::new(3)
            .with_header_rows(1)
            .with_header_columns(1);
        let a = cells(&plain(&format, 6));
        let scopes: Vec<HeaderScope> = a
            .placements
            .iter()
            .filter_map(|p| p.as_ref().map(|p| p.header))
            .collect();
        assert_eq!(
            scopes,
            [
                HeaderScope::Corner,
                HeaderScope::Column,
                HeaderScope::Column,
                HeaderScope::Row,
                HeaderScope::None,
                HeaderScope::None,
            ]
        );
        assert!(HeaderScope::Corner.is_header());
        assert!(!HeaderScope::None.is_header());
    }

    /// A continuation block joins the cell the previous block opened: same
    /// address, same tag, and it does not consume a slot.
    #[test]
    fn a_continuation_joins_the_previous_cell() {
        let format = TableFormat::new(2);
        let a = cells(&[
            Some(CellSpec::new(format.clone())),
            Some(CellSpec::new(format.clone()).continued()),
            Some(CellSpec::new(format)),
        ]);
        let first = a.placements[0].as_ref().expect("placed");
        let second = a.placements[1].as_ref().expect("placed");
        let third = a.placements[2].as_ref().expect("placed");
        assert_eq!((second.row, second.column), (0, 0));
        assert_eq!(second.cell_tag, first.cell_tag, "one cell, two blocks");
        assert!(first.opens_cell);
        assert!(!second.opens_cell);
        assert_eq!(
            (third.row, third.column),
            (0, 1),
            "the slot after the first"
        );
        assert_eq!(a.run("t0").expect("one table").cells, 2);
    }

    /// Continuing when there is no cell to continue opens one instead — a
    /// continuation of nothing is a beginning.
    #[test]
    fn a_continuation_with_no_previous_cell_opens_one() {
        let format = TableFormat::new(2);
        let a = cells(&[Some(CellSpec::new(format).continued())]);
        let only = a.placements[0].as_ref().expect("placed");
        assert!(only.opens_cell);
        assert_eq!((only.row, only.column), (0, 0));
    }

    /// Two runs are two tables: a block outside one ends it, and a format
    /// change ends it even with nothing in between.
    #[test]
    fn a_break_or_a_format_change_starts_a_second_table() {
        let two = TableFormat::new(2);
        let three = TableFormat::new(3);
        let a = cells(&[
            Some(CellSpec::new(two.clone())),
            None,
            Some(CellSpec::new(two.clone())),
            Some(CellSpec::new(three)),
        ]);
        assert_eq!(a.runs.len(), 3);
        assert_eq!(a.runs[0].tag, "t0");
        assert_eq!(a.placements[2].as_ref().expect("placed").table_tag, "t1");
        assert_eq!(a.placements[3].as_ref().expect("placed").table_tag, "t2");
        assert_eq!(a.runs[2].columns, 3);
        assert_eq!(
            a.placements[2].as_ref().expect("placed").column_count,
            2,
            "each table reports its own width",
        );
    }

    /// Every block of a table learns the finished table's extent, including
    /// the ones placed before the last row existed.
    #[test]
    fn the_extent_is_stamped_once_the_table_ends() {
        let a = cells(&plain(&TableFormat::new(2), 5));
        for placement in a.placements.iter().flatten() {
            assert_eq!(placement.row_count, 3);
            assert_eq!(placement.column_count, 2);
        }
    }

    /// The tracks are resolved against the column count, in both directions,
    /// so the two can never describe different tables.
    #[test]
    fn the_tracks_are_resolved_to_the_column_count() {
        let short = TableFormat::new(3).with_column_widths(vec![GridTrack::Px(120)]);
        assert_eq!(
            short.tracks(),
            [GridTrack::Px(120), GridTrack::Auto, GridTrack::Auto]
        );
        let long = TableFormat::new(1).with_column_widths(vec![
            GridTrack::Fr(1.0),
            GridTrack::Fr(2.0),
            GridTrack::Auto,
        ]);
        assert_eq!(long.tracks(), [GridTrack::Fr(1.0)]);
        assert_eq!(
            TableFormat::new(0).column_count(),
            1,
            "a table has a column"
        );
    }

    /// The negative control: no membership, no addressing, no runs.
    #[test]
    fn blocks_outside_a_table_are_not_addressed() {
        let a = cells(&[None, None]);
        assert!(a.placements.iter().all(Option::is_none));
        assert!(a.runs.is_empty());
    }
}
