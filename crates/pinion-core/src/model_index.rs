//! R1524 §5.27 — the address of **one cell** in a Model/View grid: the
//! toolkit's model index.
//!
//! # Why it lives here (R1544)
//!
//! [`CellIndex`] was introduced in `pinion_widget_paint::table`, beside the
//! grid that asks with it. That was the only consumer until R1544 gave the
//! grid an **editing** axis, whose latch — which cell has an open editor — is
//! state, and state substrates live in this crate (`grid_sort`, `row_search`,
//! `scroll`, `column_widths`). A state module cannot name a type defined above
//! it, so the address had to come down.
//!
//! The move is not merely mechanical bookkeeping: it corrects a layering
//! inversion this crate already avoided on the other half of the vocabulary.
//! [`CellValue`](crate::cell_value::CellValue) — the model's *datum* — has always been
//! here; the model's *address* was above the paint layer. The toolkit keeps
//! model index and dynamic value in the same module (`QtCore`) for exactly this
//! reason: they are the two halves of one contract, and the view is what sits
//! above them, not what defines them.
//!
//! `pinion_widget_paint::table` re-exports the type, so every path that named
//! it there still resolves.

/// R1524 §5.27 — the address of **one cell**: the unit
/// `view_virtual_table` asks its consumer for.
///
/// This is the Model/View address every framework with a virtualized grid
/// carries — the toolkit's model index (the argument to
/// `data`), another retained-mode toolkit's `TableVicinity` (the argument to
/// `TableView.builder`'s `cellBuilder`) — and it is a *struct* for the same
/// reason both of those are: the two coordinates are the same type, so a
/// positional `(row, col)` pair silently accepts them swapped. Naming them
/// also lets `col` state the property that a windowed grid makes load-bearing
/// and that R1523's frozen pane had to convert between internally — it is
/// **absolute**.
///
/// R1544 — derives serde because the grid's editing latch holds one inside a
/// `Signal`, whose R36 §5.31 hot-reload bound is `Serialize +
/// DeserializeOwned`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct CellIndex {
    /// The **data**-row index — already resolved through the sort
    /// permutation, so a consumer indexes its dataset directly and never
    /// sees a visual position (the R730 data-indexed convention the
    /// selection predicate follows).
    pub row: usize,
    /// The **absolute** table-column index: `0` is the grid's first column
    /// whatever the column window or the frozen split. A consumer therefore
    /// answers for the column it was asked about with no knowledge of either.
    pub col: usize,
}

impl CellIndex {
    /// R1544 — the index at `(row, col)`.
    ///
    /// A constructor beside the public fields because the editing verbs name
    /// indices inline (`state.begin(CellIndex::new(r, c), ..)`), where the
    /// struct-literal form is three tokens longer at every call and the
    /// field names it spells are the ones directly above it.
    #[must_use]
    pub const fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

/// R1544 §5.27 — how large the model is on both axes: the toolkit's
/// `rowCount()` / `columnCount()` pair.
///
/// A struct for the reason [`CellIndex`] is one — the two extents are the same
/// type, so a positional `(rows, cols)` argument silently accepts them
/// swapped, and a swapped extent does not fail loudly: it produces a cursor
/// that walks a plausible-looking wrong rectangle.
///
/// It exists because the editing cursor walks the model, not the painted
/// window: the toolkit's `EditNextItem` moves to the next index in the *model*, whether
/// or not that index is currently rendered. A grid that windowed 5 of 200
/// columns and advanced within the window would stop at the window edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GridExtent {
    /// Total data-row count (the toolkit `rowCount()`).
    pub rows: usize,
    /// Total column count (the toolkit `columnCount()`).
    pub cols: usize,
}

impl GridExtent {
    /// The extent of a `rows` x `cols` model.
    #[must_use]
    pub const fn new(rows: usize, cols: usize) -> Self {
        Self { rows, cols }
    }

    /// Whether `index` addresses a cell inside this extent.
    #[must_use]
    pub const fn contains(&self, index: CellIndex) -> bool {
        index.row < self.rows && index.col < self.cols
    }

    /// The number of cells — `rows * cols`, saturating.
    ///
    /// The bound the editing cursor's wrap-around walk uses, so a model with
    /// no editable cell terminates after exactly one lap instead of spinning.
    #[must_use]
    pub const fn cell_count(&self) -> usize {
        self.rows.saturating_mul(self.cols)
    }
}
