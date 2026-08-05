//! R1563 §5.27 §5.40 — **a selection with two axes**: a set of *cells*, held as
//! the bands it is made of.
//!
//! R1561 made a selection a set of runs over the row axis. R1562 then made the
//! vertical header band press select the row through it, and named the mirror
//! it could not build: a **column** header cannot select its column, because
//! there was no column axis to select on. A selection was a set of rows, so
//! "column 3" was not a statement the model could hold at any price.
//!
//! Qt's model is two-dimensional throughout — `QItemSelectionModel` selects
//! `QModelIndex`es, and `QItemSelectionRange` is a rectangle from a top-left to
//! a bottom-right index — so the *capability* is Qt's floor. The *shape* is
//! chosen here, and the choice is forced by one fact a rectangle list cannot
//! accommodate: this framework's selection is **canonical** (R1561), and a set
//! of cells has no unique minimal decomposition into rectangles. A cross — row
//! 0 entirely, plus column 0 entirely — is two rectangles two different ways,
//! both minimal, and a representation that can spell one selection two ways
//! cannot report whether an interaction changed anything.
//!
//! # The normal form
//!
//! So the type is not a rectangle list. A selection is a function from a row to
//! the set of columns selected in it, and this holds that function **grouped by
//! its value**: one [`SelectionBand`] per distinct [`ColumnSpan`], carrying the
//! rows that have it. That is unique by construction — each row appears in
//! exactly one band, because its column set is one value — so two
//! `CellSelection`s are equal exactly when they select the same cells, and the
//! R1561 invariant survives the extra dimension rather than being traded for
//! it.
//!
//! The bands are ordered by their lowest row, which is a total order because
//! the bands partition the rows they touch.
//!
//! # Why the axes are not symmetric
//!
//! [`ColumnSpan::All`] carries no column count: it means *this whole record*,
//! whatever the schema turns out to hold. The row axis has no such value — a
//! full-column selection names its rows as runs.
//!
//! That asymmetry is the model's, not an omission. The row axis is the one this
//! framework **windows**: rows stream in by the million and a selection stated
//! over them must not silently claim rows that arrive later (select the visible
//! thousand, let five hundred more load, and they are *not* selected — which is
//! what Qt does too, and what a user expects). Columns are the schema: adding a
//! metadata column to a table must not turn a selected record into a partly
//! selected one.
//!
//! **Past Qt 6.11.** Qt cannot state the column half at all. A full row there is
//! `QItemSelectionRange(index(r, 0), index(r, columnCount() - 1))`, bound to the
//! column count at the moment of selection, so inserting a column leaves the
//! range covering every column *but the new one* — the row is silently no longer
//! fully selected, and `QItemSelectionModel::selectedRows()`, which is
//! documented to return rows "where all columns are selected", stops returning
//! it. Here that row is [`ColumnSpan::All`] and stays whole.
//!
//! The one operation that gives the property up is **subtraction**: removing a
//! column from "all columns" is a statement about a known set of columns, so
//! [`ColumnSpan::without`] takes the model's current column count and resolves
//! `All` into the runs it stood for. That is the moment a selection becomes
//! count-dependent, and it is a named parameter at the call site rather than a
//! silent rewrite.

use super::index_runs::IndexRuns;
use super::virtual_select::SelectionExtent;

/// R954 / R1563 §5.38 §5.40 — what a press selects: Qt
/// `QAbstractItemView::setSelectionBehavior`.
///
/// A property of the view, fixed when the coordinator is built, and orthogonal
/// to cardinality (*how many* things a selection may hold — the eager
/// [`Table`](super::table::Table)'s `with_multiselect`, the windowed
/// [`VirtualSelect`](super::virtual_select::VirtualSelect)'s
/// [`SelectionMode`](super::virtual_select::SelectionMode)).
///
/// R954 defined it on the eager `Table`; R1563 moved it here when the windowed
/// coordinator acquired the same axis, and added Qt's **third** arm, which
/// neither had. One enum rather than one per coordinator, because two
/// vocabularies for one question can disagree about what `SelectItems` means —
/// and Qt, which is the floor here, spells it once on `QAbstractItemView`.
///
/// Not every coordinator offers every arm: the eager `Table`'s only setter is
/// `with_select_items`, so it cannot hold [`SelectColumns`](Self::SelectColumns)
/// at all. A shared vocabulary with per-coordinator constructors is how that is
/// said — rather than by each coordinator owning a narrower copy of the enum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectionBehavior {
    /// `SelectRows` — a press selects (washes) the whole **record**, whatever
    /// cell it landed on. The R707 single-row / R735 multi-row model
    /// (`hello-table`), and every selection this framework held before the
    /// column axis existed, which is why it is the default here where Qt's is
    /// `SelectItems`: the pre-existing behaviour must not move under a round
    /// that adds an axis.
    #[default]
    SelectRows,
    /// `SelectItems` — a press selects that single **cell**, and `Shift` grows
    /// a rectangle from the pinned anchor (the spreadsheet model;
    /// `hello-cell-select` on the eager coordinator, `hello-column-select` on
    /// the windowed one).
    SelectItems,
    /// R1563 — `SelectColumns`: a press selects the whole **column** it landed
    /// in. Qt's third arm, which this framework did not have on either
    /// coordinator.
    SelectColumns,
}

/// R1563 §5.27 — the **question** a grid painter asks about its selection, so
/// the paint never holds a second copy of it.
///
/// R1536 turned the a11y builders from taking a `&BTreeSet<usize>` into taking
/// the membership question; this is that seam for the paint layer, widened to
/// two axes. A painter asks only about what it is drawing — the ~20 rows and
/// ~6 columns of the window — so the cost of showing a selection is the
/// window's size whatever the model's is.
///
/// The blanket impl over `Fn(usize) -> bool` is what makes every pre-R1563
/// caller keep compiling *and* keep its exact behaviour: a row predicate is a
/// selection whose rows are whole records. Its
/// [`column`](Self::column) is [`SelectionExtent::Empty`] because a row
/// predicate carries no row count and so cannot say whether a column is fully
/// covered — which is also the right *behaviour* for those grids, whose
/// horizontal band is the sort control and shows no selection at all (Qt's
/// `QHeaderView::highlightSections` defaults to `false` for the same reason).
pub trait GridSelection {
    /// Whether the cell at `(row, col)` is selected.
    fn cell(&self, row: usize, col: usize) -> bool;

    /// How much of `row` is selected — the tri-state the vertical band shows.
    fn row(&self, row: usize) -> SelectionExtent;

    /// How much of `col` is selected — the tri-state the horizontal band shows.
    fn column(&self, _col: usize) -> SelectionExtent {
        SelectionExtent::Empty
    }
}

impl<F: Fn(usize) -> bool + ?Sized> GridSelection for F {
    fn cell(&self, row: usize, _col: usize) -> bool {
        self(row)
    }

    fn row(&self, row: usize) -> SelectionExtent {
        if self(row) {
            SelectionExtent::All
        } else {
            SelectionExtent::Empty
        }
    }
}

/// R1563 — a [`CellSelection`] bound to the model it is a selection *of*, which
/// is what makes it answerable as a [`GridSelection`].
///
/// The two counts are not decoration: [`ColumnSpan::All`] has no width of its
/// own and a column's extent is measured against the model's height, so a
/// selection alone cannot answer either band's question. Binding them here
/// rather than storing them in [`CellSelection`] keeps the selection a
/// statement about cells and not a snapshot of a model's shape.
#[derive(Clone, Copy, Debug)]
pub struct SelectionOf<'a> {
    /// The selected cells.
    pub cells: &'a CellSelection,
    /// The model's height, for [`GridSelection::column`].
    pub rows: usize,
    /// The model's width, for [`GridSelection::row`].
    pub columns: usize,
}

impl GridSelection for SelectionOf<'_> {
    fn cell(&self, row: usize, col: usize) -> bool {
        self.cells.contains(row, col)
    }

    fn row(&self, row: usize) -> SelectionExtent {
        self.cells.row_extent(row, self.columns)
    }

    fn column(&self, col: usize) -> SelectionExtent {
        self.cells.column_extent(col, self.rows)
    }
}

/// R1563 — the column axis of a [`SelectionBand`]: every column, or named ones.
///
/// Two arms rather than one because [`All`](Self::All) is a statement that
/// outlives the schema — see the [module documentation](self) for why the row
/// axis has no peer of it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "ColumnSpanWire", into = "ColumnSpanWire")]
pub enum ColumnSpan {
    /// Every column of the row, however many there are now or later. The value
    /// every selection made before R1563 carries, and the one a row-select grid
    /// only ever produces.
    All,
    /// The named columns. Empty is representable but never stored: a band with
    /// no columns selects nothing, so [`CellSelection`] drops it.
    Runs(IndexRuns),
}

impl ColumnSpan {
    /// The span covering exactly one column.
    #[must_use]
    pub fn column(col: usize) -> Self {
        Self::Runs(IndexRuns::run(col, col))
    }

    /// Whether this is [`All`](Self::All) — the count-independent arm.
    #[must_use]
    pub const fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// Whether the span names no column at all. [`All`](Self::All) is never
    /// empty, including over a model with no columns: it is a statement about
    /// the record, and a record with no fields still selects as a record.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::All => false,
            Self::Runs(runs) => runs.is_empty(),
        }
    }

    /// Whether `col` is in the span.
    #[must_use]
    pub fn contains(&self, col: usize) -> bool {
        match self {
            Self::All => true,
            Self::Runs(runs) => runs.contains(col),
        }
    }

    /// How many columns the span covers in a model `column_count` wide.
    ///
    /// The count is the parameter because [`All`](Self::All) has no size of its
    /// own — that is the whole point of it.
    #[must_use]
    pub fn count(&self, column_count: usize) -> usize {
        match self {
            Self::All => column_count,
            Self::Runs(runs) => runs.clamped_below(column_count).len(),
        }
    }

    /// Whether the span covers every column of a model `column_count` wide.
    ///
    /// `Runs` can answer `true` — a span that happens to name every column
    /// covers every column *today*. It is still not [`All`](Self::All), and the
    /// difference is what happens when a column is added.
    #[must_use]
    pub fn covers_all(&self, column_count: usize) -> bool {
        self.count(column_count) == column_count
    }

    /// The span as explicit runs over a model `column_count` wide — the
    /// resolution that makes [`All`](Self::All) count-dependent.
    #[must_use]
    pub fn resolved(&self, column_count: usize) -> IndexRuns {
        match self {
            Self::All => {
                if column_count == 0 {
                    IndexRuns::new()
                } else {
                    IndexRuns::run(0, column_count - 1)
                }
            }
            Self::Runs(runs) => runs.clamped_below(column_count),
        }
    }

    /// Every column in either span. Needs no count: `All` absorbs anything.
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::All, _) | (_, Self::All) => Self::All,
            (Self::Runs(a), Self::Runs(b)) => Self::Runs(a.union(b)),
        }
    }

    /// The columns of `self` that are not in `other`, over a model
    /// `column_count` wide.
    ///
    /// The count is required and cannot be defaulted: "all columns except
    /// column 3" is not a value [`All`](Self::All) can hold, so subtracting
    /// from it resolves it first. See the [module documentation](self) — this
    /// is the one operation that spends the count-independence, and it says so
    /// in its signature.
    #[must_use]
    pub fn without(&self, other: &Self, column_count: usize) -> Self {
        match other {
            Self::All => Self::Runs(IndexRuns::new()),
            Self::Runs(b) => Self::Runs(self.resolved(column_count).difference(b)),
        }
    }
}

/// The wire and snapshot form of a [`ColumnSpan`]: the word `"all"`, or the
/// `[[first, last], …]` array [`IndexRuns`] already serializes as.
///
/// A word rather than a boolean-tagged object because the two arms are two
/// *values* of one axis, and a client reading `"all"` beside `[[0, 2]]` needs no
/// schema to see that. An unknown word is a **refusal** ([`TryFrom`]), not an
/// empty span: R1561 made the same call for a malformed run array, and for the
/// same reason — a payload that says something the model cannot read must not
/// arrive as a selection that says nothing.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum ColumnSpanWire {
    Word(String),
    Runs(Vec<(usize, usize)>),
}

impl TryFrom<ColumnSpanWire> for ColumnSpan {
    type Error = &'static str;

    fn try_from(wire: ColumnSpanWire) -> Result<Self, Self::Error> {
        match wire {
            ColumnSpanWire::Word(word) if word == "all" => Ok(Self::All),
            ColumnSpanWire::Word(_) => Err("a column span is \"all\" or an array of runs"),
            ColumnSpanWire::Runs(pairs) => Ok(Self::Runs(IndexRuns::from(pairs))),
        }
    }
}

impl From<ColumnSpan> for ColumnSpanWire {
    fn from(span: ColumnSpan) -> Self {
        match span {
            ColumnSpan::All => Self::Word("all".to_string()),
            ColumnSpan::Runs(runs) => Self::Runs(runs.into()),
        }
    }
}

/// R1563 — the rows that share one [`ColumnSpan`], and that span.
///
/// Not "a rectangle": the rows are a run *set*, so rows 0, 4 and 9 selected in
/// the same two columns are one band. That is what makes the normal form
/// canonical — a band is the pre-image of one column set, and there is exactly
/// one of those per distinct set.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SelectionBand {
    /// The rows in this band. Never empty in a stored band.
    pub rows: IndexRuns,
    /// The columns selected in every one of those rows. Never empty in a stored
    /// band.
    pub columns: ColumnSpan,
}

/// R1563 §5.27 §5.40 — a canonical set of cells: one [`SelectionBand`] per
/// distinct column span.
///
/// See the [module documentation](self) for the normal form and why it is not a
/// rectangle list.
///
/// The methods divide by what they cost, and the division is the point — a
/// selection over a million rows and two hundred columns is a handful of bands:
///
/// - **O(1)**: [`is_empty`](Self::is_empty), [`band_count`](Self::band_count),
///   [`bands`](Self::bands), [`rows_all_columns`](Self::rows_all_columns).
/// - **O(bands · log runs)**: [`contains`](Self::contains),
///   [`row_span`](Self::row_span), [`row_extent`](Self::row_extent).
/// - **O(bands · runs)**: [`add`](Self::add), [`remove`](Self::remove),
///   [`cell_count`](Self::cell_count), [`column_extent`](Self::column_extent),
///   equality.
///
/// Nothing here is O(cells).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "Vec<SelectionBand>", into = "Vec<SelectionBand>")]
pub struct CellSelection {
    /// One per distinct column span, each with a non-empty row set and a
    /// non-empty span, pairwise row-disjoint, ordered by lowest row.
    bands: Vec<SelectionBand>,
}

/// The empty row set, so [`CellSelection::rows_all_columns`] can hand back a
/// reference when there is no `All` band rather than forcing every caller to
/// own a clone of nothing.
static NO_ROWS: IndexRuns = IndexRuns::new();

impl CellSelection {
    /// The empty selection.
    #[must_use]
    pub const fn new() -> Self {
        Self { bands: Vec::new() }
    }

    /// The bands, ordered by lowest row.
    #[must_use]
    pub fn bands(&self) -> &[SelectionBand] {
        &self.bands
    }

    /// How many bands the selection is made of — the size of the
    /// representation, the number a scale claim is stated in (R1561's
    /// [`run_count`](IndexRuns::run_count), one dimension up).
    #[must_use]
    pub fn band_count(&self) -> usize {
        self.bands.len()
    }

    /// Whether nothing is selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bands.is_empty()
    }

    /// Whether the cell at `(row, col)` is selected.
    #[must_use]
    pub fn contains(&self, row: usize, col: usize) -> bool {
        self.row_span(row).is_some_and(|span| span.contains(col))
    }

    /// The columns selected in `row`, or `None` when the row has none.
    #[must_use]
    pub fn row_span(&self, row: usize) -> Option<&ColumnSpan> {
        self.bands
            .iter()
            .find(|band| band.rows.contains(row))
            .map(|band| &band.columns)
    }

    /// The rows selected in **every** column, count-independently — the
    /// [`ColumnSpan::All`] band.
    ///
    /// This is what a row-select grid's selection *is*, so it is the O(1)
    /// accessor and the one
    /// [`VirtualSelect::selection`](super::virtual_select::VirtualSelect::selection)
    /// answers with. A row whose columns are named individually is not in it
    /// even if they happen to be all of them — see
    /// [`rows_covering_all_columns`](Self::rows_covering_all_columns), which is
    /// Qt's `selectedRows()` and needs the count.
    #[must_use]
    pub fn rows_all_columns(&self) -> &IndexRuns {
        self.bands
            .iter()
            .find(|band| band.columns.is_all())
            .map_or(&NO_ROWS, |band| &band.rows)
    }

    /// Qt `QItemSelectionModel::selectedRows()` — the rows in which every one of
    /// a model's `column_count` columns is selected, however the span spells it.
    #[must_use]
    pub fn rows_covering_all_columns(&self, column_count: usize) -> IndexRuns {
        self.bands
            .iter()
            .filter(|band| band.columns.covers_all(column_count))
            .fold(IndexRuns::new(), |acc, band| acc.union(&band.rows))
    }

    /// Every row with at least one selected cell.
    #[must_use]
    pub fn touched_rows(&self) -> IndexRuns {
        self.bands
            .iter()
            .fold(IndexRuns::new(), |acc, band| acc.union(&band.rows))
    }

    /// How much of `row` is selected, in a model `column_count` wide.
    ///
    /// **Past Qt 6.11**: a Qt header section shows selection as a bool
    /// (`QHeaderView::highlightSections`), so a row with two of two hundred
    /// columns selected is indistinguishable from a fully selected one. The
    /// tri-state is what the band actually knows.
    #[must_use]
    pub fn row_extent(&self, row: usize, column_count: usize) -> SelectionExtent {
        self.row_span(row).map_or(SelectionExtent::Empty, |span| {
            SelectionExtent::of(span.count(column_count), column_count)
        })
    }

    /// How much of `col` is selected, in a model `row_count` tall — the
    /// transpose of [`row_extent`](Self::row_extent), and what the horizontal
    /// band shows.
    #[must_use]
    pub fn column_extent(&self, col: usize, row_count: usize) -> SelectionExtent {
        let selected = self
            .bands
            .iter()
            .filter(|band| band.columns.contains(col))
            .map(|band| band.rows.clamped_below(row_count).len())
            .sum();
        SelectionExtent::of(selected, row_count)
    }

    /// Qt `QItemSelectionModel::selectedColumns()` — the columns selected in
    /// every one of a model's `row_count` rows.
    #[must_use]
    pub fn columns_covering_all_rows(&self, row_count: usize, column_count: usize) -> IndexRuns {
        (0..column_count)
            .filter(|&col| self.column_extent(col, row_count) == SelectionExtent::All)
            .collect()
    }

    /// How many **cells** are selected, in a model `column_count` wide.
    #[must_use]
    pub fn cell_count(&self, column_count: usize) -> usize {
        self.bands
            .iter()
            .map(|band| band.rows.len() * band.columns.count(column_count))
            .sum()
    }

    /// The lowest row with a selected cell, or `None`.
    #[must_use]
    pub fn first_row(&self) -> Option<usize> {
        self.bands.first().and_then(|band| band.rows.first())
    }

    /// The highest row with a selected cell, or `None`.
    #[must_use]
    pub fn last_row(&self) -> Option<usize> {
        self.bands.iter().filter_map(|band| band.rows.last()).max()
    }

    /// Deselect everything. Returns whether anything changed.
    pub fn clear(&mut self) -> bool {
        let had = !self.bands.is_empty();
        self.bands.clear();
        had
    }

    /// Select `columns` in every row of `rows`, keeping everything already
    /// selected. Returns whether the selection changed.
    ///
    /// Each existing band is split against the incoming rows — the part that
    /// overlaps takes the union of the two spans, the part that does not keeps
    /// its own — and the rows no band held take `columns`. Every split is a run
    /// operation ([`IndexRuns::intersection`] / [`difference`](IndexRuns::difference)),
    /// so selecting a million rows costs the same as selecting one.
    pub fn add(&mut self, rows: &IndexRuns, columns: &ColumnSpan) -> bool {
        if rows.is_empty() || columns.is_empty() {
            return false;
        }
        let mut next: Vec<SelectionBand> = Vec::with_capacity(self.bands.len() + 2);
        let mut fresh = rows.clone();
        for band in &self.bands {
            let hit = band.rows.intersection(rows);
            let miss = band.rows.difference(rows);
            push_band(&mut next, miss, band.columns.clone());
            if !hit.is_empty() {
                push_band(&mut next, hit, band.columns.union(columns));
            }
            fresh = fresh.difference(&band.rows);
        }
        push_band(&mut next, fresh, columns.clone());
        self.commit(next)
    }

    /// Deselect `columns` in every row of `rows`. Returns whether the selection
    /// changed.
    ///
    /// `column_count` is required because taking a column away from a
    /// [`ColumnSpan::All`] band resolves it — see
    /// [`ColumnSpan::without`].
    pub fn remove(&mut self, rows: &IndexRuns, columns: &ColumnSpan, column_count: usize) -> bool {
        if rows.is_empty() || columns.is_empty() || self.bands.is_empty() {
            return false;
        }
        let mut next: Vec<SelectionBand> = Vec::with_capacity(self.bands.len() + 1);
        for band in &self.bands {
            let hit = band.rows.intersection(rows);
            let miss = band.rows.difference(rows);
            push_band(&mut next, miss, band.columns.clone());
            if !hit.is_empty() {
                push_band(&mut next, hit, band.columns.without(columns, column_count));
            }
        }
        self.commit(next)
    }

    /// Replace the whole selection with `columns` in `rows`. Returns whether
    /// the selection changed.
    pub fn replace(&mut self, rows: &IndexRuns, columns: &ColumnSpan) -> bool {
        let mut next = Vec::with_capacity(1);
        push_band(&mut next, rows.clone(), columns.clone());
        self.commit(next)
    }

    /// The selection with every row at or beyond `row_count`, and every column
    /// at or beyond `column_count`, dropped — the model's validity clamp, so a
    /// restored snapshot or a malformed payload can never name a cell that does
    /// not exist.
    ///
    /// [`ColumnSpan::All`] survives the column clamp untouched: it names the
    /// row's columns, whatever they are, so there is nothing in it to be out of
    /// range.
    #[must_use]
    pub fn clamped(&self, row_count: usize, column_count: usize) -> Self {
        let mut next = Vec::with_capacity(self.bands.len());
        for band in &self.bands {
            let columns = match &band.columns {
                ColumnSpan::All => ColumnSpan::All,
                ColumnSpan::Runs(runs) => ColumnSpan::Runs(runs.clamped_below(column_count)),
            };
            push_band(&mut next, band.rows.clamped_below(row_count), columns);
        }
        let mut out = Self::new();
        out.commit(next);
        out
    }

    /// Install `next` as the bands, canonicalised. Returns whether the value
    /// changed.
    ///
    /// The one write path — every mutator funnels through it, so the invariant
    /// is restored in a single place rather than once per operation (R1561's
    /// rule, and the reason `changed` can be a value comparison at all).
    fn commit(&mut self, mut next: Vec<SelectionBand>) -> bool {
        next.retain(|band| !band.rows.is_empty() && !band.columns.is_empty());
        next.sort_by_key(|band| band.rows.first().unwrap_or(usize::MAX));
        if next == self.bands {
            return false;
        }
        self.bands = next;
        true
    }
}

/// Merge `rows` into the band with an equal span, or append a new one.
///
/// Grouping by span is what makes the form canonical, and it happens here
/// because this is the only place a band enters the vector.
fn push_band(bands: &mut Vec<SelectionBand>, rows: IndexRuns, columns: ColumnSpan) {
    if rows.is_empty() || columns.is_empty() {
        return;
    }
    if let Some(band) = bands.iter_mut().find(|band| band.columns == columns) {
        band.rows = band.rows.union(&rows);
    } else {
        bands.push(SelectionBand { rows, columns });
    }
}

/// The wire and snapshot form: the bands, canonicalised on the way in.
///
/// A payload may spell its bands in any order, with duplicate spans, empty rows
/// or overlapping row sets — a hand-written `intervene`, an older snapshot, a
/// client that appended as the user clicked — and the value that results is the
/// one the mutators build. Overlap resolves by **union of the spans**, which is
/// the reading that makes a payload's two statements about one row additive
/// rather than order-dependent.
impl From<Vec<SelectionBand>> for CellSelection {
    fn from(bands: Vec<SelectionBand>) -> Self {
        let mut out = Self::new();
        for band in bands {
            out.add(&band.rows, &band.columns);
        }
        out
    }
}

impl From<CellSelection> for Vec<SelectionBand> {
    fn from(value: CellSelection) -> Self {
        value.bands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(pairs: &[(usize, usize)]) -> IndexRuns {
        IndexRuns::from(pairs.to_vec())
    }

    #[test]
    fn r1563_a_row_select_grid_produces_one_all_band() {
        let mut sel = CellSelection::new();
        assert!(sel.replace(&runs(&[(0, 999_999)]), &ColumnSpan::All));
        assert_eq!(sel.band_count(), 1);
        assert_eq!(sel.rows_all_columns(), &runs(&[(0, 999_999)]));
        // O(bands): a million rows over two hundred columns is one band.
        assert_eq!(sel.cell_count(200), 200_000_000);
    }

    #[test]
    fn r1563_the_normal_form_is_canonical_across_orders() {
        // The same cells reached two ways must be the same value, or `changed`
        // reporting is a coin flip.
        let mut a = CellSelection::new();
        a.add(&runs(&[(0, 0)]), &ColumnSpan::column(1));
        a.add(&runs(&[(1, 1)]), &ColumnSpan::column(1));
        let mut b = CellSelection::new();
        b.add(&runs(&[(1, 1)]), &ColumnSpan::column(1));
        b.add(&runs(&[(0, 0)]), &ColumnSpan::column(1));
        assert_eq!(a, b);
        assert_eq!(a.band_count(), 1, "one span, so one band");
    }

    #[test]
    fn r1563_a_cross_is_one_value_though_it_is_two_rectangles() {
        // Row 0 entirely plus column 0 entirely over a 4x3 model. As rectangles
        // this decomposes two ways, both minimal; the row-grouped form has one
        // spelling.
        let mut a = CellSelection::new();
        a.add(&runs(&[(0, 0)]), &ColumnSpan::All);
        a.add(&runs(&[(0, 3)]), &ColumnSpan::column(0));
        let mut b = CellSelection::new();
        b.add(&runs(&[(0, 3)]), &ColumnSpan::column(0));
        b.add(&runs(&[(0, 0)]), &ColumnSpan::All);
        assert_eq!(a, b);
        assert_eq!(a.band_count(), 2);
        assert!(a.contains(0, 2));
        assert!(a.contains(3, 0));
        assert!(!a.contains(3, 2));
    }

    #[test]
    fn r1563_all_columns_outlives_a_column_being_added() {
        // The past-Qt property: a full row stays full when the schema grows,
        // where a Qt range built against the old count does not.
        let mut whole = CellSelection::new();
        whole.replace(&runs(&[(2, 2)]), &ColumnSpan::All);
        let mut named = CellSelection::new();
        named.replace(&runs(&[(2, 2)]), &ColumnSpan::Runs(runs(&[(0, 2)])));
        assert_eq!(whole.row_extent(2, 3), SelectionExtent::All);
        assert_eq!(named.row_extent(2, 3), SelectionExtent::All);
        // A fourth column arrives.
        assert_eq!(whole.row_extent(2, 4), SelectionExtent::All);
        assert_eq!(named.row_extent(2, 4), SelectionExtent::Partial);
    }

    #[test]
    fn r1563_removing_a_column_resolves_all_against_the_count() {
        let mut sel = CellSelection::new();
        sel.replace(&runs(&[(0, 0)]), &ColumnSpan::All);
        assert!(sel.remove(&runs(&[(0, 0)]), &ColumnSpan::column(1), 3));
        assert_eq!(
            sel.row_span(0),
            Some(&ColumnSpan::Runs(runs(&[(0, 0), (2, 2)])))
        );
        assert!(sel.contains(0, 2));
        assert!(!sel.contains(0, 1));
    }

    #[test]
    fn r1563_a_band_emptied_of_columns_is_dropped_not_kept_empty() {
        let mut sel = CellSelection::new();
        sel.replace(&runs(&[(0, 1)]), &ColumnSpan::column(0));
        assert!(sel.remove(&runs(&[(0, 1)]), &ColumnSpan::column(0), 4));
        assert!(sel.is_empty(), "no columns left means no selection");
        assert_eq!(sel.band_count(), 0);
    }

    #[test]
    fn r1563_extents_are_the_tri_state_qt_cannot_show() {
        let mut sel = CellSelection::new();
        sel.replace(&runs(&[(0, 0)]), &ColumnSpan::column(1));
        assert_eq!(sel.row_extent(0, 4), SelectionExtent::Partial);
        assert_eq!(sel.row_extent(1, 4), SelectionExtent::Empty);
        assert_eq!(sel.column_extent(1, 4), SelectionExtent::Partial);
        sel.add(&runs(&[(1, 3)]), &ColumnSpan::column(1));
        assert_eq!(sel.column_extent(1, 4), SelectionExtent::All);
        assert_eq!(sel.columns_covering_all_rows(4, 3), runs(&[(1, 1)]));
    }

    #[test]
    fn r1563_the_wire_round_trips_and_refuses_an_unknown_word() {
        let mut sel = CellSelection::new();
        sel.add(&runs(&[(0, 0)]), &ColumnSpan::All);
        sel.add(&runs(&[(5, 9)]), &ColumnSpan::column(2));
        let json = serde_json::to_string(&sel).unwrap();
        assert!(json.contains("\"all\""), "{json}");
        assert_eq!(serde_json::from_str::<CellSelection>(&json).unwrap(), sel);
        // An unknown word is a refusal, not an empty span.
        assert!(serde_json::from_str::<ColumnSpan>("\"every\"").is_err());
        assert_eq!(
            serde_json::from_str::<ColumnSpan>("\"all\"").unwrap(),
            ColumnSpan::All
        );
    }

    #[test]
    fn r1563_the_wire_canonicalises_an_overlapping_payload() {
        // Two bands naming row 0 — the shape a client that appended as the user
        // clicked produces. The rows must not end up in two bands.
        let decoded: CellSelection = serde_json::from_str(
            r#"[{"rows":[[0,0]],"columns":[[1,1]]},{"rows":[[0,0]],"columns":[[3,3]]}]"#,
        )
        .unwrap();
        assert_eq!(decoded.band_count(), 1);
        assert_eq!(
            decoded.row_span(0),
            Some(&ColumnSpan::Runs(runs(&[(1, 1), (3, 3)])))
        );
    }

    #[test]
    fn r1563_the_clamp_drops_cells_outside_the_model() {
        let mut sel = CellSelection::new();
        sel.add(&runs(&[(0, 20)]), &ColumnSpan::Runs(runs(&[(0, 9)])));
        sel.add(&runs(&[(30, 30)]), &ColumnSpan::All);
        let clamped = sel.clamped(25, 4);
        assert_eq!(clamped.band_count(), 1, "row 30 is gone");
        assert_eq!(
            clamped.row_span(0),
            Some(&ColumnSpan::Runs(runs(&[(0, 3)])))
        );
        assert!(!clamped.contains(0, 4));
    }
}
