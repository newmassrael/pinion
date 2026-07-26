//! R1446 — the seam between a typed **cell model** and a chart:
//! [`ModelMapper`], the `QtCharts` `Q*XYModelMapper` contract expressed over
//! [`CellValue`].
//!
//! # Why this exists
//!
//! Until this module every chart in the tree was fed a `Vec<Series>` built
//! in code — `pinion-chart` and pinion's Model/View layer had never met.
//! Meanwhile every editable grid in the tree (`hello-property-grid`,
//! `hello-data-grid`) holds its data as a flat, **row-major** `Vec<CellValue>`
//! with a column count. A mapper is the missing adapter: point it at that
//! block, say which field is x and which are y, and it produces the
//! [`Series`] a chart draws. What the user edits is then what the chart
//! plots, and *which* field is plotted becomes a runtime choice rather than a
//! recompile.
//!
//! # The one place this is deliberately not Qt
//!
//! Qt reads a model cell through `QVariant::toReal()`, which answers `0.0`
//! for anything that is not a number. A mistyped cell, or a y-axis pointed at
//! a label column, therefore plots as a point **on the axis** —
//! indistinguishable from a measured zero, and silent. That is the classic
//! model-mapper footgun.
//!
//! [`ModelMapper::map`] never invents a number. A cell it cannot read
//! contributes **no point** and is reported in [`Mapped::unreadable`], with
//! its table coordinates and the [`CellKind`] that was actually there — so a
//! consumer can say *"Month is text, not a measure"* instead of drawing a
//! flat line at zero. The chart stays a pure scene producer; the reporting is
//! data, per §2 #7.
//!
//! Note what "no point" means for a line: the polyline joins the surviving
//! neighbours, so an interior unreadable row reads as interpolation rather
//! than as a gap. Breaking a series at a hole needs a gap-aware `Series`,
//! which does not exist yet; the report is how a consumer surfaces the hole
//! today, and `examples/hello-model-chart` does exactly that.
//!
//! # Example
//!
//! ```
//! use pinion_chart::{CellTable, Field, ModelMapper};
//! use pinion_core::CellValue;
//!
//! // Two columns: a label and a measure. Three records.
//! let cells = vec![
//!     CellValue::Text("Jan".into()), CellValue::Float(12.0),
//!     CellValue::Text("Feb".into()), CellValue::Float(18.0),
//!     CellValue::Text("Mar".into()), CellValue::Float(15.0),
//! ];
//! let table = CellTable::new(&cells, 2);
//!
//! let mapped = ModelMapper::new(Field::Ordinal)
//!     .with_series(1, "revenue")
//!     .map(&table);
//!
//! assert_eq!(mapped.series[0].points.len(), 3);
//! assert!(mapped.unreadable.is_empty());
//!
//! // Point y at the LABEL column and nothing is fabricated:
//! let bad = ModelMapper::new(Field::Ordinal).with_series(0, "month").map(&table);
//! assert!(bad.series[0].points.is_empty());
//! assert_eq!(bad.unreadable.len(), 3);
//! ```

use pinion_core::{CellKind, CellValue};

use crate::series::{DataPoint, Series};

/// Which way the **fields** run through a row-major cell block.
///
/// Qt splits this into two classes (`QVXYModelMapper` / `QHXYModelMapper`);
/// one mapper with an orientation keeps a single code path, and makes the
/// orientation itself introspectable data rather than a type choice frozen at
/// the call site.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// Each **column** is a field and each **row** a record — the spreadsheet
    /// norm, and what every editable grid in the tree holds (Qt's
    /// `QVXYModelMapper`).
    #[default]
    Columns,
    /// Each **row** is a field and each **column** a record — a pivoted block,
    /// where a field's samples run across one row (Qt's `QHXYModelMapper`).
    Rows,
}

/// Where one mapped axis reads its numbers.
///
/// The naming is orientation-neutral on purpose: under
/// [`Orientation::Rows`] a "column index" would name the wrong thing, whereas
/// "the field at index *i*" and "the record's ordinal" hold either way.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Field {
    /// The field at this index along the field axis — a column under
    /// [`Orientation::Columns`], a row under [`Orientation::Rows`].
    At(usize),
    /// The record's own position (`0, 1, 2, …`) rather than any stored cell —
    /// a block of measurements with no explicit independent variable, which is
    /// the common shape for an evenly-sampled series.
    ///
    /// Only meaningful for x; a y read from an ordinal would plot the record
    /// index against itself, so [`ModelMapper::with_series`] takes a field
    /// index and never a [`Field`].
    Ordinal,
}

/// A borrowed, row-major block of typed cells — the shape pinion's editable
/// grids hold (`cells[row * ncols + col]`).
///
/// Borrowed rather than owned so a mapping is a pure read of whatever the
/// consumer already has in its `Signal`, with no copy and no second source of
/// truth to fall out of step.
#[derive(Copy, Clone, Debug)]
pub struct CellTable<'a> {
    cells: &'a [CellValue],
    ncols: usize,
}

impl<'a> CellTable<'a> {
    /// View `cells` as a row-major table `ncols` wide.
    ///
    /// A trailing **partial** row (`cells.len() % ncols != 0`) is not a
    /// record: [`Self::nrows`] floors, so a half-written row is invisible to a
    /// mapping rather than half-plotted.
    #[must_use]
    pub const fn new(cells: &'a [CellValue], ncols: usize) -> Self {
        Self { cells, ncols }
    }

    /// The column count this table was built with.
    #[must_use]
    pub const fn ncols(&self) -> usize {
        self.ncols
    }

    /// The number of complete rows. Zero when the table has no columns.
    #[must_use]
    pub const fn nrows(&self) -> usize {
        if self.ncols == 0 {
            0
        } else {
            self.cells.len() / self.ncols
        }
    }

    /// The cell at `(row, col)`, or `None` when either index is outside the
    /// table.
    #[must_use]
    pub fn get(&self, row: usize, col: usize) -> Option<&'a CellValue> {
        if col >= self.ncols || row >= self.nrows() {
            return None;
        }
        self.cells.get(row * self.ncols + col)
    }
}

/// The numeric reading of a cell, or `None` when the cell holds no number.
///
/// **[`CellValue::Int`] and [`CellValue::Float`] only.** A `Bool` is a flag,
/// not a measure — Qt would widen it to `0.0` / `1.0`, but a checkbox column
/// silently plotted as a two-level signal is a guess about intent, and a
/// consumer that wants that encoding can map it explicitly. `Text` is not
/// parsed either: a typed cell's kind is declared by its column, so a numeric
/// string in a text column means the column is text.
///
/// This is the crate's single definition of "reads as a number" — a field
/// picker asks it the same question the mapper does.
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "an i64 past 2^53 rounds to the nearest f64 — the same widening \
              every f64 chart value already carries, and finer than any axis"
)]
pub fn numeric(cell: &CellValue) -> Option<f64> {
    match *cell {
        CellValue::Int(i) => Some(i as f64),
        CellValue::Float(f) => Some(f),
        CellValue::Bool(_)
        | CellValue::Text(_)
        | CellValue::Choice { .. }
        | CellValue::Color(_) => None,
    }
}

/// A record ordinal as an x value.
#[allow(
    clippy::cast_precision_loss,
    reason = "a record ordinal is exact in f64 below 2^53; a table that long \
              cannot be held in memory as CellValues"
)]
const fn ordinal(record: usize) -> f64 {
    record as f64
}

/// A cell a mapping could not read as a number, in **table** coordinates
/// (`row` / `col` regardless of [`Orientation`], so a consumer can point at
/// the offending cell in the grid the user sees).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UnreadableCell {
    /// Row index in the cell block.
    pub row: usize,
    /// Column index in the cell block.
    pub col: usize,
    /// What the cell actually holds — the reason it is not a number.
    pub kind: CellKind,
}

/// The outcome of [`ModelMapper::map`]: the series a chart draws, plus every
/// cell the mapping could not read.
///
/// `unreadable` is ordered x-field first (read once for the whole record
/// range), then per mapped series in declaration order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mapped {
    /// One series per [`ModelMapper::with_series`] call, in declaration order,
    /// ready for [`LineChart`](crate::LineChart) / [`ScatterChart`](crate::ScatterChart).
    pub series: Vec<Series>,
    /// Every cell that held no number. Empty when the mapping was clean.
    pub unreadable: Vec<UnreadableCell>,
}

impl Mapped {
    /// How many unreadable cells fell in table column `col`.
    ///
    /// The count a consumer states when a picked field turns out not to be a
    /// measure ("Month: 8 cells are text").
    #[must_use]
    pub fn unreadable_in_col(&self, col: usize) -> usize {
        self.unreadable.iter().filter(|u| u.col == col).count()
    }
}

/// One mapped series: which field it reads and what the legend calls it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MappedField {
    index: usize,
    name: String,
}

/// Binds a [`CellTable`] to chart [`Series`] — the `QtCharts`
/// `Q*XYModelMapper` contract (`setXColumn` / `setYColumn` / `setFirstRow` /
/// `setRowCount`), with [`Mapped::unreadable`] in place of Qt's silent zero.
///
/// The mapper is plain data: build one, keep it in state, and re-run
/// [`map`](Self::map) each frame over the live cells. Re-pointing an axis is
/// then a state change, which is what makes "the user chooses what is
/// plotted" a runtime affordance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelMapper {
    orientation: Orientation,
    x: Field,
    y: Vec<MappedField>,
    first: usize,
    count: Option<usize>,
}

impl ModelMapper {
    /// A mapper over [`Orientation::Columns`] (records in rows) reading x from
    /// `x`, with no series yet and no record-range restriction.
    #[must_use]
    pub const fn new(x: Field) -> Self {
        Self {
            orientation: Orientation::Columns,
            x,
            y: Vec::new(),
            first: 0,
            count: None,
        }
    }

    /// Read fields along rows instead of columns (Qt's `QHXYModelMapper`).
    #[must_use]
    pub const fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Point the x axis at another field.
    #[must_use]
    pub const fn with_x(mut self, x: Field) -> Self {
        self.x = x;
        self
    }

    /// Map one more series: the field at `index`, labelled `name` in the
    /// legend.
    ///
    /// The name comes from the caller because a `CellValue` block is values
    /// only — column headers are the grid binding's metadata, not part of the
    /// model.
    #[must_use]
    pub fn with_series(mut self, index: usize, name: impl Into<String>) -> Self {
        self.y.push(MappedField {
            index,
            name: name.into(),
        });
        self
    }

    /// Restrict the mapping to `count` records starting at `first` (Qt's
    /// `setFirstRow` / `setRowCount`) — the "plot only the newest N samples"
    /// window. Both ends are clamped to the table.
    #[must_use]
    pub const fn with_record_range(mut self, first: usize, count: usize) -> Self {
        self.first = first;
        self.count = Some(count);
        self
    }

    /// The field the x axis reads.
    #[must_use]
    pub const fn x(&self) -> Field {
        self.x
    }

    /// The field indices this mapper draws as series, in declaration order.
    #[must_use]
    pub fn series_fields(&self) -> Vec<usize> {
        self.y.iter().map(|f| f.index).collect()
    }

    /// The number of records `table` offers this mapper — rows under
    /// [`Orientation::Columns`], columns under [`Orientation::Rows`].
    #[must_use]
    pub const fn record_count(&self, table: &CellTable<'_>) -> usize {
        match self.orientation {
            Orientation::Columns => table.nrows(),
            Orientation::Rows => table.ncols(),
        }
    }

    /// Project `table` into series.
    ///
    /// A record contributes a point only when **both** its x and its y read as
    /// numbers; anything else is reported in [`Mapped::unreadable`] and no
    /// value is invented. A field index outside the table yields an empty
    /// series and reports nothing — there are no cells there to be unreadable,
    /// so validate an index against [`CellTable::ncols`] / [`CellTable::nrows`]
    /// (which is what a field picker derived from the table does by
    /// construction).
    #[must_use]
    pub fn map(&self, table: &CellTable<'_>) -> Mapped {
        let records = self.record_count(table);
        let first = self.first.min(records);
        let last = self
            .count
            .map_or(records, |c| first.saturating_add(c).min(records));

        let mut unreadable = Vec::new();

        // The x field is read ONCE for the whole range rather than inside the
        // series loop: re-reading it per series would report the same
        // unreadable x cell once per mapped series, so a two-series chart over
        // a text x column would claim twice as many bad cells as it has.
        let xs: Vec<Option<f64>> = (first..last)
            .map(|record| match self.x {
                Field::Ordinal => Some(ordinal(record)),
                Field::At(field) => self.read(table, record, field, &mut unreadable),
            })
            .collect();

        let mut series = Vec::with_capacity(self.y.len());
        for field in &self.y {
            let mut points = Vec::new();
            for (offset, record) in (first..last).enumerate() {
                // y is read (and so reported) even when x already failed: that
                // a cell holds no number is a fact about the cell, not about
                // its neighbour.
                let y = self.read(table, record, field.index, &mut unreadable);
                if let (Some(x), Some(y)) = (xs[offset], y) {
                    points.push(DataPoint::new(x, y));
                }
            }
            series.push(Series::new(field.name.clone(), points));
        }

        Mapped { series, unreadable }
    }

    /// The table coordinates of field `field` within `record`.
    const fn cell_at(&self, record: usize, field: usize) -> (usize, usize) {
        match self.orientation {
            Orientation::Columns => (record, field),
            Orientation::Rows => (field, record),
        }
    }

    /// Read one cell as a number, recording it when it is not one. An **absent**
    /// cell (index outside the table) reports nothing — see [`Self::map`].
    fn read(
        &self,
        table: &CellTable<'_>,
        record: usize,
        field: usize,
        out: &mut Vec<UnreadableCell>,
    ) -> Option<f64> {
        let (row, col) = self.cell_at(record, field);
        let cell = table.get(row, col)?;
        let value = numeric(cell);
        if value.is_none() {
            out.push(UnreadableCell {
                row,
                col,
                kind: cell.kind(),
            });
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mapped value passes through no arithmetic, so it should land exactly;
    /// the crate's epsilon idiom states that without asking `clippy::float_cmp`
    /// to look away.
    #[track_caller]
    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[track_caller]
    fn assert_at(point: &DataPoint, x: f64, y: f64) {
        assert_close(point.x, x);
        assert_close(point.y, y);
    }

    /// Four columns: label (Text), revenue (Float), units (Int), active (Bool).
    fn sample_cells() -> Vec<CellValue> {
        let mut cells = Vec::new();
        for (i, (label, revenue, units)) in
            [("Jan", 12.0, 3_i64), ("Feb", 18.0, 5), ("Mar", 15.0, 4)]
                .into_iter()
                .enumerate()
        {
            cells.push(CellValue::Text(label.to_owned()));
            cells.push(CellValue::Float(revenue));
            cells.push(CellValue::Int(units));
            cells.push(CellValue::Bool(i % 2 == 0));
        }
        cells
    }

    #[test]
    fn a_column_of_floats_maps_to_a_series_against_the_ordinal() {
        let cells = sample_cells();
        let mapped = ModelMapper::new(Field::Ordinal)
            .with_series(1, "revenue")
            .map(&CellTable::new(&cells, 4));

        assert!(mapped.unreadable.is_empty());
        assert_eq!(mapped.series.len(), 1);
        assert_eq!(mapped.series[0].name, "revenue");
        let pts = &mapped.series[0].points;
        assert_eq!(pts.len(), 3);
        assert_at(&pts[0], 0.0, 12.0);
        assert_at(&pts[2], 2.0, 15.0);
    }

    #[test]
    fn an_int_column_reads_as_a_number_and_can_be_the_x_axis() {
        let cells = sample_cells();
        let mapped = ModelMapper::new(Field::At(2))
            .with_series(1, "revenue")
            .map(&CellTable::new(&cells, 4));

        assert!(mapped.unreadable.is_empty());
        let pts = &mapped.series[0].points;
        assert_at(&pts[0], 3.0, 12.0);
        assert_at(&pts[1], 5.0, 18.0);
    }

    #[test]
    fn two_mapped_fields_become_two_series_in_declaration_order() {
        let cells = sample_cells();
        let mapped = ModelMapper::new(Field::Ordinal)
            .with_series(2, "units")
            .with_series(1, "revenue")
            .map(&CellTable::new(&cells, 4));

        assert_eq!(mapped.series.len(), 2);
        assert_eq!(mapped.series[0].name, "units");
        assert_eq!(mapped.series[1].name, "revenue");
        assert_close(mapped.series[0].points[1].y, 5.0);
        assert_close(mapped.series[1].points[1].y, 18.0);
    }

    /// The headline difference from Qt: a y axis pointed at a label column
    /// plots NOTHING and says why, where `QVariant::toReal()` would draw a
    /// flat line on zero that reads as measured data.
    #[test]
    fn a_text_column_is_reported_never_plotted_as_zero() {
        let cells = sample_cells();
        let mapped = ModelMapper::new(Field::Ordinal)
            .with_series(0, "month")
            .map(&CellTable::new(&cells, 4));

        assert!(
            mapped.series[0].points.is_empty(),
            "no point is invented for a cell that holds no number"
        );
        assert_eq!(mapped.unreadable.len(), 3);
        assert_eq!(mapped.unreadable_in_col(0), 3);
        assert_eq!(
            mapped.unreadable[0],
            UnreadableCell {
                row: 0,
                col: 0,
                kind: CellKind::Text,
            }
        );
    }

    #[test]
    fn a_bool_is_a_flag_not_a_two_level_signal() {
        let cells = sample_cells();
        let mapped = ModelMapper::new(Field::Ordinal)
            .with_series(3, "active")
            .map(&CellTable::new(&cells, 4));

        assert!(mapped.series[0].points.is_empty());
        assert_eq!(mapped.unreadable_in_col(3), 3);
        assert_eq!(mapped.unreadable[0].kind, CellKind::Bool);
    }

    #[test]
    fn choice_and_colour_cells_are_not_numbers_either() {
        let cells = vec![
            CellValue::Choice {
                selected: 1,
                options: vec!["a".to_owned(), "b".to_owned()],
            },
            CellValue::Color(pinion_core::style::Color::rgb(1, 2, 3)),
        ];
        let mapped = ModelMapper::new(Field::Ordinal)
            .with_series(0, "choice")
            .with_series(1, "colour")
            .map(&CellTable::new(&cells, 2));

        assert!(mapped.series.iter().all(|s| s.points.is_empty()));
        assert_eq!(mapped.unreadable_in_col(0), 1);
        assert_eq!(mapped.unreadable_in_col(1), 1);
    }

    /// The de-duplication `map` reads x once for: without it a two-series
    /// chart over a text x column would report each bad x cell twice and a
    /// consumer's "3 of 3 cells" readout would say 6.
    #[test]
    fn an_unreadable_x_is_reported_once_not_once_per_series() {
        let cells = sample_cells();
        let mapped = ModelMapper::new(Field::At(0))
            .with_series(1, "revenue")
            .with_series(2, "units")
            .map(&CellTable::new(&cells, 4));

        assert_eq!(
            mapped.unreadable_in_col(0),
            3,
            "3 bad x cells, not 3 per series"
        );
        assert!(
            mapped.series.iter().all(|s| s.points.is_empty()),
            "an unreadable x drops the record from every series"
        );
    }

    #[test]
    fn a_record_range_windows_the_mapping() {
        let cells = sample_cells();
        let table = CellTable::new(&cells, 4);
        let mapped = ModelMapper::new(Field::Ordinal)
            .with_series(1, "revenue")
            .with_record_range(1, 2)
            .map(&table);

        let pts = &mapped.series[0].points;
        assert_eq!(pts.len(), 2);
        // The ordinal stays the record's TABLE position, not its offset in
        // the window — a windowed series keeps sitting where its records are.
        assert_at(&pts[0], 1.0, 18.0);
    }

    #[test]
    fn a_record_range_past_the_end_is_clamped_not_panicking() {
        let cells = sample_cells();
        let table = CellTable::new(&cells, 4);
        let mapped = ModelMapper::new(Field::Ordinal)
            .with_series(1, "revenue")
            .with_record_range(2, 99)
            .map(&table);
        assert_eq!(mapped.series[0].points.len(), 1);

        let past = ModelMapper::new(Field::Ordinal)
            .with_series(1, "revenue")
            .with_record_range(99, 4)
            .map(&table);
        assert!(past.series[0].points.is_empty());
    }

    #[test]
    fn a_field_outside_the_table_is_an_empty_series_with_nothing_to_report() {
        let cells = sample_cells();
        let mapped = ModelMapper::new(Field::Ordinal)
            .with_series(9, "nowhere")
            .map(&CellTable::new(&cells, 4));

        assert!(mapped.series[0].points.is_empty());
        assert!(
            mapped.unreadable.is_empty(),
            "an absent cell is a bad mapping, not an unreadable cell"
        );
    }

    #[test]
    fn a_pivoted_block_maps_the_same_series_by_rows() {
        // The transpose of two records x two fields: field 0 = x, field 1 = y.
        // Columns orientation: [x0 y0 / x1 y1] with ncols = 2.
        let by_cols = vec![
            CellValue::Float(10.0),
            CellValue::Float(1.0),
            CellValue::Float(20.0),
            CellValue::Float(2.0),
        ];
        // Rows orientation: row 0 is the x field, row 1 the y field.
        let by_rows = vec![
            CellValue::Float(10.0),
            CellValue::Float(20.0),
            CellValue::Float(1.0),
            CellValue::Float(2.0),
        ];

        let cols = ModelMapper::new(Field::At(0))
            .with_series(1, "y")
            .map(&CellTable::new(&by_cols, 2));
        let rows = ModelMapper::new(Field::At(0))
            .with_orientation(Orientation::Rows)
            .with_series(1, "y")
            .map(&CellTable::new(&by_rows, 2));

        assert_eq!(cols.series, rows.series);
        assert_eq!(rows.series[0].points.len(), 2);
        assert_at(&rows.series[0].points[1], 20.0, 2.0);
    }

    #[test]
    fn a_pivoted_block_reports_in_table_coordinates() {
        let cells = vec![
            CellValue::Float(10.0),
            CellValue::Float(20.0),
            CellValue::Text("n/a".to_owned()),
            CellValue::Float(2.0),
        ];
        let mapped = ModelMapper::new(Field::At(0))
            .with_orientation(Orientation::Rows)
            .with_series(1, "y")
            .map(&CellTable::new(&cells, 2));

        // The bad cell is row 1, col 0 in the TABLE, even though it is
        // record 0 of field 1 in the mapping.
        assert_eq!(
            mapped.unreadable,
            vec![UnreadableCell {
                row: 1,
                col: 0,
                kind: CellKind::Text,
            }]
        );
        assert_eq!(mapped.series[0].points.len(), 1);
    }

    #[test]
    fn a_partial_trailing_row_is_not_a_record() {
        let mut cells = sample_cells();
        cells.push(CellValue::Text("Apr".to_owned())); // half a 4th row
        let table = CellTable::new(&cells, 4);
        assert_eq!(table.nrows(), 3);
        let mapped = ModelMapper::new(Field::Ordinal)
            .with_series(1, "revenue")
            .map(&table);
        assert_eq!(mapped.series[0].points.len(), 3);
    }

    #[test]
    fn a_table_with_no_columns_is_empty_not_a_division_by_zero() {
        let cells = sample_cells();
        let table = CellTable::new(&cells, 0);
        assert_eq!(table.nrows(), 0);
        assert!(table.get(0, 0).is_none());
        let mapped = ModelMapper::new(Field::Ordinal)
            .with_series(0, "none")
            .map(&table);
        assert!(mapped.series[0].points.is_empty());
        assert!(mapped.unreadable.is_empty());
    }

    /// The live claim: the mapper reads the model, so editing a cell changes
    /// the plotted series with no rebuild of anything else.
    #[test]
    fn the_series_follows_an_edited_cell() {
        let mapper = ModelMapper::new(Field::Ordinal).with_series(1, "revenue");
        let mut cells = sample_cells();
        let before = mapper.map(&CellTable::new(&cells, 4));

        let (row, col) = (1_usize, 1_usize);
        cells[row * 4 + col] = CellValue::Float(99.0);
        let after = mapper.map(&CellTable::new(&cells, 4));

        assert_close(before.series[0].points[1].y, 18.0);
        assert_close(after.series[0].points[1].y, 99.0);
        assert_ne!(before.series, after.series);
    }

    #[test]
    fn the_mapper_reports_what_it_is_pointed_at() {
        let mapper = ModelMapper::new(Field::At(2))
            .with_series(1, "revenue")
            .with_series(3, "active");
        assert_eq!(mapper.x(), Field::At(2));
        assert_eq!(mapper.series_fields(), vec![1, 3]);

        let cells = sample_cells();
        assert_eq!(mapper.record_count(&CellTable::new(&cells, 4)), 3);
        assert_eq!(
            mapper
                .with_orientation(Orientation::Rows)
                .record_count(&CellTable::new(&cells, 4)),
            4
        );
    }

    #[test]
    fn numeric_is_the_one_definition_of_reads_as_a_number() {
        assert_close(
            numeric(&CellValue::Int(-7)).expect("an int is a number"),
            -7.0,
        );
        assert_close(
            numeric(&CellValue::Float(1.5)).expect("a float is a number"),
            1.5,
        );
        assert!(numeric(&CellValue::Bool(true)).is_none());
        assert!(
            numeric(&CellValue::Text("3.5".to_owned())).is_none(),
            "a numeric STRING in a text column is still text"
        );
    }
}
