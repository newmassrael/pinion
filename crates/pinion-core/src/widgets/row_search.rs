//! R1004 §5.27 §5.40 — **search-and-jump cursor over a Model/View dataset**.
//!
//! The Model/View-at-scale campaign's filter axis (R783/R997 [`GridFilter`])
//! *removes* the rows a predicate rejects; **search** does the opposite — it
//! keeps every row visible and instead carries a **cursor** that walks the
//! rows a predicate *accepts*, scrolling each match into view. This is the
//! "Find Packet" of a analyser capture, the search-and-jump of a dlt-viewer
//! log, the `Ctrl+G` "go to next match" of a code editor's results pane: the
//! data set never shrinks, you navigate matches in place.
//!
//! The decisive reuse: a search query **is** an R997
//! [`GridFilter`] conjunction — the
//! same multi-facet predicate the grid's filter axis carries and the R998
//! row-coloring engine matches against. Search is the **third** consumer of
//! the `GridFilter` match vocabulary (filter removes, coloring tints, search
//! navigates), so an AI client searches with `invoke "set_query" "0~Delta"`
//! reading `0~Delta` (Name column contains "Delta") exactly as a filter or a
//! coloring rule would. The wire form is shared; the *action* on a match is
//! the consumer's policy.
//!
//! ## What is genuinely new here (the cursor, not the predicate)
//!
//! Neither the filter proxy nor the coloring engine has a **cursor over a
//! match set**: a current position among the matching rows, with
//! next/previous (wrapping) and a direct jump. That cursor — mirroring the
//! text editor's [`find_next`](crate::widgets::text_edit) / `find_prev` /
//! `find_current_index` but indexed over **rows** instead of text spans — is
//! the new substrate this module adds. The match *set* itself reuses the
//! shared single-entry [`OrderMemo`](crate::widgets::order_memo) (sound here
//! for the same reason it is for the sort proxies: [`RowSearchState`] owns an
//! immutable materialized source, so the match list can only change when the
//! query does).
//!
//! ## State (pure) vs. reveal (the adapter's job)
//!
//! [`RowSearchState`] holds only the query, the cursor, and the memoized
//! match list — it is pure and fully unit-testable with no scroll dependency.
//! Scrolling a match **into view** is the adapter's responsibility, exactly
//! as the keyboard-nav glue (`nav_select_key`) reveals a selection while the
//! [`VirtualSelectExternal`](crate::widgets::virtual_select) does not: the
//! [`RowSearchExternal`] optionally carries a [`ScrollState`] handle (via
//! [`with_reveal`](RowSearchExternal::with_reveal)) and calls
//! [`reveal_row`](crate::widgets::virtual_list::reveal_row) after every cursor
//! move. The reveal target is the match's **source** index, which equals its
//! view position when search runs over the un-reordered source (the first
//! consumer's shape); composing search with a sort/filter proxy that permutes
//! rows is an additive follow-up that maps source→view before revealing.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `query("match_count")` is the number of matching rows; `query("current")`
//! is the cursor's position among them and `query("current_row")` the source
//! row it points at; `query("match.<i>")` is the i-th matching source row —
//! the whole match set and the cursor are introspectable without a single
//! pixel (see `tools/demos/r1004_grid_search.py`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    ReadRefusal, SchemaArg, SchemaField, query_proxy_external_impl,
};
use crate::reactive::{Owner, Signal};
use crate::widgets::grid_sort::{GridFilter, grid_filter_from_str, grid_filter_str};
use crate::widgets::order_memo::{OrderMemo, source_at_value};
use crate::widgets::scroll::ScrollState;

/// R1004 §5.27 §5.40 — the matching **source row indices** for a `query` over
/// a `count`-row dataset, in ascending source order.
///
/// A row matches when `query` (a [`GridFilter`] conjunction) accepts its cells
/// — `cell(row, col)` yields the text of the row's column `col`, the text
/// policy [`GridFilter::matches`] uses. A `None` query has **no** matches (an
/// inactive search has an empty match set and no cursor — distinct from a
/// filter, where `None` means "match everything"). The free-function peer of
/// [`RowSearchState`], so a consumer can compute a match set directly and the
/// state can memoize the same computation.
#[must_use]
pub fn search_matches<'a>(
    count: usize,
    query: Option<&GridFilter>,
    cell: impl Fn(usize, usize) -> &'a str,
) -> Vec<usize> {
    match query {
        None => Vec::new(),
        Some(f) => (0..count).filter(|&r| f.matches(|c| cell(r, c))).collect(),
    }
}

/// R1004 §5.27 §5.40 — the reactive **single source of truth** for a search
/// over a Model/View dataset: the active query, the cursor among its matches,
/// and the memoized match list.
///
/// Holds the source cells (materialized once — the immutable source that makes
/// the value-keyed `OrderMemo` sound), the active `query` as a reactive
/// [`Signal`], a `cursor` [`Signal`] (an index into the match list), and the
/// derived match list (memoized on the query). It holds **no** scroll — the
/// reveal-into-view lives on the external, not here (see the module note).
/// Created once via [`use_row_search`] and shared — the same `Rc` — by the
/// [`RowSearchExternal`] (which mutates the query / cursor) and the view (which
/// reads them). Reading [`query`](Self::query) /
/// [`current_row`](Self::current_row) inside a view-fn auto-subscribes, so a
/// search or a cursor move repaints exactly like a scroll-offset change.
pub struct RowSearchState {
    tag: Option<&'static str>,
    /// Row-major source cells — `cells[row][col]`. Materialized once; a row
    /// shorter than `col_count` reads `""` for the missing cells (the same
    /// blank-cell tolerance the paint layer and [`GridSortState`](crate::widgets::grid_sort::GridSortState) apply).
    cells: Vec<Vec<String>>,
    col_count: usize,
    /// Active search query (a [`GridFilter`] predicate; `None` = no search).
    query: Signal<Option<GridFilter>>,
    /// Cursor: an index into the current match list, or `None` when there is
    /// no active match (no query, or a query that matches nothing).
    cursor: Signal<Option<usize>>,
    /// Memoized matching source-row list, recomputed only when the query
    /// changes — the shared `OrderMemo` (R780 lift), keyed on the query.
    matches: RefCell<OrderMemo<Option<GridFilter>>>,
}

impl RowSearchState {
    /// Construct over a `col_count`-wide grid whose source row `r` has cells
    /// `cells[r]`. Starts with no query and no cursor.
    #[must_use]
    pub fn new(col_count: usize, cells: Vec<Vec<String>>) -> Self {
        Self {
            tag: None,
            cells,
            col_count,
            query: Signal::new(None),
            cursor: Signal::new(None),
            matches: RefCell::new(OrderMemo::new()),
        }
    }

    /// As [`new`](Self::new) but records the [`use_row_search`] cache key, for
    /// symmetry with [`ScrollState::with_tag`](crate::widgets::scroll::ScrollState::with_tag).
    #[must_use]
    pub fn with_tag(key: &'static str, col_count: usize, cells: Vec<Vec<String>>) -> Self {
        Self {
            tag: Some(key),
            ..Self::new(col_count, cells)
        }
    }

    /// The [`use_row_search`] cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// Source row count (query-independent).
    #[must_use]
    pub fn count(&self) -> usize {
        self.cells.len()
    }

    /// Column count (the searchable column bound).
    #[must_use]
    pub fn col_count(&self) -> usize {
        self.col_count
    }

    /// The cell text at `(row, col)`, or `""` out of range.
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> &str {
        self.cells
            .get(row)
            .and_then(|r| r.get(col))
            .map_or("", String::as_str)
    }

    /// Active search query, or `None`. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn query(&self) -> Option<GridFilter> {
        self.query.get()
    }

    /// The matching source rows for the current query, recomputed only when
    /// the query changes (memoized). Subscribes to the `query` `Signal`, so a
    /// view that calls this repaints on a query change. Cheap `Rc` clone on a
    /// cache hit.
    #[must_use]
    pub fn matches(&self) -> Rc<Vec<usize>> {
        let query = self.query.get();
        self.matches.borrow_mut().get(query.clone(), || {
            search_matches(self.cells.len(), query.as_ref(), |r, c| self.cell(r, c))
        })
    }

    /// Number of matching rows. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn match_count(&self) -> usize {
        self.matches().len()
    }

    /// The cursor's zero-based position among the matches, or `None` when no
    /// match is active. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.cursor.get()
    }

    /// The **source row** the cursor points at, or `None` when no match is
    /// active. Subscribes (cursor + match list) when read inside a view-fn.
    #[must_use]
    pub fn current_row(&self) -> Option<usize> {
        let i = self.cursor.get()?;
        self.matches().get(i).copied()
    }

    /// The source row at match position `i`, or `None` when out of range.
    #[must_use]
    pub fn match_at(&self, i: usize) -> Option<usize> {
        self.matches().get(i).copied()
    }

    /// Set the search query and land the cursor on the **first** match (or
    /// `None` when nothing matches) — the textbook "type a search, jump to the
    /// first hit" behavior. Returns the resulting [`match_count`](Self::match_count).
    ///
    /// The two `Signal` writes need no [`batch`](crate::reactive::batch): a
    /// pull-based view-fn marks dirty on either write and repaints once
    /// regardless, so coalescing the dispatch would buy nothing. `matches()`
    /// is read *after* the query write to derive the new cursor; `Signal::set`
    /// writes the cell eagerly even inside a batch, so that read sees the new
    /// query either way (the memo recomputes once against it).
    pub fn set_query(&self, query: Option<GridFilter>) -> usize {
        self.query.set(query);
        let n = self.match_count();
        self.cursor.set((n > 0).then_some(0));
        n
    }

    /// Move the cursor to the next match, wrapping to the first; lands on the
    /// first match when none is active. Returns the new
    /// [`current_row`](Self::current_row), or `None` when there are no matches.
    /// The textbook find-next: repeated calls walk every match and loop.
    pub fn next_match(&self) -> Option<usize> {
        let n = self.matches().len();
        if n == 0 {
            return None;
        }
        let next = self.cursor.get().map_or(0, |i| (i + 1) % n);
        self.cursor.set(Some(next));
        self.current_row()
    }

    /// Mirror of [`next_match`](Self::next_match): move the cursor to the
    /// previous match, wrapping to the last; lands on the last match when none
    /// is active.
    pub fn prev_match(&self) -> Option<usize> {
        let n = self.matches().len();
        if n == 0 {
            return None;
        }
        let prev = self.cursor.get().map_or(n - 1, |i| (i + n - 1) % n);
        self.cursor.set(Some(prev));
        self.current_row()
    }

    /// Jump the cursor directly to match position `i`. Returns the new
    /// [`current_row`](Self::current_row), or `None` (cursor unchanged) when
    /// `i` is out of range — an out-of-range jump is a no-op, never a panic.
    pub fn jump_to(&self, i: usize) -> Option<usize> {
        if i < self.matches().len() {
            self.cursor.set(Some(i));
            self.current_row()
        } else {
            None
        }
    }
}

/// R1004 §5.27 §5.40 — resolve the shared [`RowSearchState`] for `key`,
/// building it once via `data` (the source `(col_count, cells)`). Mirrors
/// [`use_grid_sort`](crate::widgets::grid_sort::use_grid_sort): the `External`
/// and the view both call this with the same `key` and receive the same `Rc`,
/// so the cursor is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
#[must_use]
pub fn use_row_search(
    key: &'static str,
    data: impl FnOnce() -> (usize, Vec<Vec<String>>),
) -> Rc<RowSearchState> {
    Owner::current()
        .expect("use_row_search requires an active Owner scope")
        .cache(key, || {
            let (col_count, cells) = data();
            RowSearchState::with_tag(key, col_count, cells)
        })
}

/// R1004 §5.27 §5.40 — the search **coordinator** External: a thin adapter that
/// surfaces the shared [`RowSearchState`] to the §5.12 `scene/query` /
/// `scene/intervene` / `scene/invoke` paths.
///
/// Like [`ViewSortFilterExternal`](crate::widgets::view_order::ViewSortFilterExternal)
/// it owns no interaction statechart and emits **no** §5.20 intent — the query
/// and cursor `Signal` writes already repaint every subscribed view, so the
/// widget is never independently dirty. All state lives in the shared
/// [`RowSearchState`]; the external optionally carries a [`ScrollState`] handle
/// so a cursor move scrolls the match into view (the "jump"), exactly as the
/// keyboard-nav glue reveals a selection.
#[derive(Clone)]
pub struct RowSearchExternal {
    state: Rc<RowSearchState>,
    /// When attached (via [`with_reveal`](Self::with_reveal)) every cursor move
    /// scrolls the current match into view through this `(scroll, row_pitch)`.
    reveal: Option<(Rc<ScrollState>, u32)>,
}

impl core::fmt::Debug for RowSearchExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RowSearchExternal")
            .field("query", &grid_filter_str(self.state.query().as_ref()))
            .field("match_count", &self.state.match_count())
            .field("current", &self.state.current_index())
            .finish_non_exhaustive()
    }
}

impl RowSearchExternal {
    /// Wrap the shared [`RowSearchState`] (from [`use_row_search`]). Cursor
    /// moves do not scroll until a [`ScrollState`] is attached with
    /// [`with_reveal`](Self::with_reveal).
    #[must_use]
    pub fn new(state: Rc<RowSearchState>) -> Self {
        Self {
            state,
            reveal: None,
        }
    }

    /// Attach the body [`ScrollState`] (and its uniform `row_pitch`) so every
    /// cursor move reveals the current match into view — the "jump" half of
    /// search-and-jump. The reveal target is the match's source index (= its
    /// view position when search runs over the un-reordered source).
    #[must_use]
    pub fn with_reveal(mut self, scroll: Rc<ScrollState>, row_pitch: u32) -> Self {
        self.reveal = Some((scroll, row_pitch));
        self
    }

    /// The shared state handle (the view reaches the same `Rc` via
    /// [`use_row_search`]).
    #[must_use]
    pub fn state(&self) -> &Rc<RowSearchState> {
        &self.state
    }

    /// Scroll the current match into view when a [`ScrollState`] is attached
    /// and a match is active — the keyboard-nav reveal idiom
    /// ([`reveal_row`](crate::widgets::virtual_list::reveal_row)) applied to
    /// the search cursor.
    fn reveal_current(&self) {
        if let (Some((scroll, pitch)), Some(row)) = (&self.reveal, self.state.current_row()) {
            crate::widgets::virtual_list::reveal_row(scroll, row, *pitch);
        }
    }

    /// Parse a wire query (the [`grid_filter_str`] form) and clamp it to the
    /// dataset's column count, so a malformed or phantom-column query collapses
    /// to `None` (no search) rather than searching a column that does not
    /// exist — the [`GridFilter::clamped_to_col_count`] safety the filter axis
    /// applies on restore.
    fn parse_query(&self, s: &str) -> Option<GridFilter> {
        grid_filter_from_str(s).and_then(|f| f.clamped_to_col_count(self.state.col_count()))
    }

    /// `match_count` as an `IntrospectValue::Int` — the uniform return for the
    /// query-setting `invoke` paths.
    fn match_count_value(&self) -> IntrospectValue {
        IntrospectValue::Int(i64::try_from(self.state.match_count()).unwrap_or(i64::MAX))
    }

    /// `current_row` as an `IntrospectValue` — the source row the cursor points
    /// at, or `Null` when no match is active. The uniform return for the
    /// cursor-moving `invoke` paths.
    fn current_row_value(&self) -> IntrospectValue {
        self.state
            .current_row()
            .and_then(|r| i64::try_from(r).ok())
            .map_or(IntrospectValue::Null, IntrospectValue::Int)
    }
}

// The shared display-config-proxy `External` skeleton (R847 SSOT): no §5.20
// intent — the query / cursor `Signal` writes already repaint every subscribed
// view, so the widget is never independently dirty.
query_proxy_external_impl!(RowSearchExternal);

impl ExternalIntrospect for RowSearchExternal {
    fn schema(&self) -> IntrospectSchema {
        // `query`       — the GridFilter wire form (query + intervene).
        // `match_count` — matching row count (query only).
        // `current`     — cursor position among matches, or Null (query only).
        // `current_row` — source row at the cursor, or Null (query only).
        // `count`       — source row count (query only).
        // `cols`        — column count (query only); the bound `cell.<col>` cites.
        // `match`       — `match.<i>` i-th matching source row (query only).
        // `cell`        — R1525 `cell.<row>.<col>` source cell text (query only).
        // `set_query`/`next`/`prev`/`jump`/`clear` — invoke channels.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("query", "string"),
                    SchemaField::new("match_count", "int"),
                    SchemaField::new("current", "int"),
                    SchemaField::new("current_row", "int"),
                    SchemaField::new("count", "int"),
                    SchemaField::new("cols", "int"),
                    SchemaField::parametric(
                        "match.<i>",
                        "int",
                        const { &[SchemaArg::index("i", "match_count")] },
                    ),
                    SchemaField::parametric(
                        "cell.<row>.<col>",
                        "string",
                        const {
                            &[
                                SchemaArg::index("row", "count"),
                                SchemaArg::index("col", "cols"),
                            ]
                        },
                    ),
                    SchemaField::action("set_query", "string"),
                    SchemaField::action("next", "string"),
                    SchemaField::action("prev", "string"),
                    SchemaField::action("jump", "int"),
                    SchemaField::action("clear", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        // `match.<i>` resolves the i-th matching source row; an out-of-range
        // position reports Null (present-but-empty), never absence.
        if let Some(rest) = path.strip_prefix("match.") {
            return Ok(source_at_value(rest, |i| self.state.match_at(i)));
        }
        // R1525 — `cell.<row>.<col>`: the SOURCE cell text this proxy searches.
        // The wire form `TableExternal` answers for the eager table and
        // `GridSortExternal` now answers for the sort/filter proxy; a search
        // cursor without it can report WHICH row matched but not what that row
        // says, so an agent cannot check that the row it was sent to actually
        // contains the query.
        if let Some(rest) = path.strip_prefix("cell.") {
            let (row_str, col_str) = rest.split_once('.').ok_or(ReadRefusal::QueryTypeMismatch)?;
            let row: usize = row_str
                .parse()
                .map_err(|_| ReadRefusal::QueryTypeMismatch)?;
            let col: usize = col_str
                .parse()
                .map_err(|_| ReadRefusal::QueryTypeMismatch)?;
            if row >= self.state.count() || col >= self.state.col_count() {
                return Err(ReadRefusal::no_such_member(format!(
                    "cell {row}.{col} is outside 0..{}.0..{}",
                    self.state.count(),
                    self.state.col_count()
                )));
            }
            return Ok(IntrospectValue::Text(self.state.cell(row, col).to_string()));
        }
        match path {
            "query" => Ok(IntrospectValue::Text(grid_filter_str(
                self.state.query().as_ref(),
            ))),
            "match_count" => Ok(self.match_count_value()),
            "current" => Ok(self
                .state
                .current_index()
                .and_then(|i| i64::try_from(i).ok())
                .map_or(IntrospectValue::Null, IntrospectValue::Int)),
            "current_row" => Ok(self.current_row_value()),
            "count" => Ok(IntrospectValue::Int(
                i64::try_from(self.state.count()).unwrap_or(i64::MAX),
            )),
            "cols" => Ok(IntrospectValue::Int(
                i64::try_from(self.state.col_count()).unwrap_or(i64::MAX),
            )),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Admin / restore: set the query from its wire form ("none" clears).
            "query" => match value {
                IntrospectValue::Text(ref s) => {
                    let q = self.parse_query(s);
                    self.state.set_query(q);
                    self.reveal_current();
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "match_count" | "current" | "current_row" | "count" | "match" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first search: set the query, land on the first match, scroll it
            // into view; returns the resulting match_count in one round-trip.
            "set_query" => match args {
                IntrospectValue::Text(ref s) => {
                    let q = self.parse_query(s);
                    self.state.set_query(q);
                    self.reveal_current();
                    Ok(self.match_count_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Walk the cursor; each returns the new current_row (Null when there
            // are no matches) and scrolls it into view.
            "next" => {
                self.state.next_match();
                self.reveal_current();
                Ok(self.current_row_value())
            }
            "prev" => {
                self.state.prev_match();
                self.reveal_current();
                Ok(self.current_row_value())
            }
            // Jump the cursor to an explicit match position (out-of-range = no-op).
            "jump" => match args {
                IntrospectValue::Int(i) => {
                    if let Ok(i) = usize::try_from(i) {
                        self.state.jump_to(i);
                        self.reveal_current();
                    }
                    Ok(self.current_row_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Clear the search (no query, no cursor); returns match_count = 0.
            "clear" => {
                self.state.set_query(None);
                Ok(self.match_count_value())
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::grid_sort::{ColumnFacet, FilterOp};

    // A 12-row dataset: col 0 = Name "<Cat><nn>", col 1 = numeric score,
    // col 2 = cyclic status. Category/status cycle so a query keeps a sparse,
    // interleaved subset (so a cursor walk visibly skips non-matching rows).
    const CATS: [&str; 3] = ["Alpha", "Bravo", "Charlie"];
    const STATUS: [&str; 3] = ["Idle", "Active", "Done"];

    /// R1525 — the source cells this proxy searches are on the wire.
    ///
    /// A search cursor could report WHICH row matched but not what that row
    /// says, so an agent could not check that the row it was sent to actually
    /// contains the query. Same `cell.<row>.<col>` wire form as the eager
    /// `TableExternal` and the sort proxy.
    #[test]
    fn r1525_query_surfaces_the_source_cells() {
        let mut e = ext();
        e.invoke("set_query", IntrospectValue::Text("2=Done".into()))
            .unwrap();
        let row = match e.query("current_row") {
            Ok(IntrospectValue::Int(r)) => usize::try_from(r).unwrap(),
            other => panic!("cursor row expected, got {other:?}"),
        };
        assert_eq!(
            e.query(&format!("cell.{row}.2")),
            Ok(IntrospectValue::Text("Done".into())),
            "the cell the search matched on is readable at the row it reports",
        );
        assert_eq!(e.query("cols"), Ok(IntrospectValue::Int(3)));
        assert!(
            matches!(e.query("cell.12.0"), Err(ReadRefusal::NoSuchMember(_))),
            "row beyond the dataset is absent"
        );
        assert!(
            matches!(e.query("cell.0.3"), Err(ReadRefusal::NoSuchMember(_))),
            "column beyond the grid is absent"
        );
    }

    fn cells() -> Vec<Vec<String>> {
        (0..12)
            .map(|i| {
                vec![
                    format!("{}{i:02}", CATS[i % 3]),
                    ((i * 7) % 100).to_string(),
                    STATUS[i % 3].to_string(),
                ]
            })
            .collect()
    }

    fn state() -> Rc<RowSearchState> {
        Rc::new(RowSearchState::new(3, cells()))
    }

    fn ext() -> RowSearchExternal {
        RowSearchExternal::new(state())
    }

    // Status=Done rows are i % 3 == 2: 2, 5, 8, 11.
    const DONE_ROWS: [usize; 4] = [2, 5, 8, 11];

    #[test]
    fn search_matches_none_is_empty_some_keeps_predicate() {
        let c = cells();
        let cell = |r: usize, col: usize| c[r][col].as_str();
        assert_eq!(
            search_matches(12, None, cell),
            Vec::<usize>::new(),
            "no query → no matches"
        );
        let q = GridFilter::eq(2, "Done");
        assert_eq!(search_matches(12, Some(&q), cell), DONE_ROWS.to_vec());
    }

    #[test]
    fn new_state_is_inactive() {
        let s = state();
        assert_eq!(s.count(), 12);
        assert_eq!(s.col_count(), 3);
        assert_eq!(s.query(), None);
        assert_eq!(
            s.match_count(),
            0,
            "no query → no matches (unlike a filter's match-all)"
        );
        assert_eq!(s.current_index(), None);
        assert_eq!(s.current_row(), None);
    }

    #[test]
    fn set_query_lands_on_first_match_and_counts() {
        let s = state();
        assert_eq!(s.set_query(Some(GridFilter::eq(2, "Done"))), 4);
        assert_eq!(s.match_count(), 4);
        assert_eq!(
            s.current_index(),
            Some(0),
            "cursor lands on the first match"
        );
        assert_eq!(s.current_row(), Some(2), "first Done row is source 2");
        // A query that matches nothing yields no cursor.
        assert_eq!(s.set_query(Some(GridFilter::eq(2, "Nope"))), 0);
        assert_eq!(s.current_index(), None);
        assert_eq!(s.current_row(), None);
    }

    #[test]
    fn matches_are_memoized_until_the_query_changes() {
        let s = state();
        s.set_query(Some(GridFilter::eq(2, "Done")));
        let first = s.matches();
        assert!(
            Rc::ptr_eq(&first, &s.matches()),
            "match list memoized across reads"
        );
        s.set_query(Some(GridFilter::eq(2, "Active")));
        assert!(!Rc::ptr_eq(&first, &s.matches()), "a new query recomputes");
    }

    #[test]
    fn next_and_prev_walk_and_wrap() {
        let s = state();
        s.set_query(Some(GridFilter::eq(2, "Done"))); // matches 2, 5, 8, 11; cursor at 0
        assert_eq!(s.next_match(), Some(5));
        assert_eq!(s.next_match(), Some(8));
        assert_eq!(s.next_match(), Some(11));
        assert_eq!(s.next_match(), Some(2), "wraps to the first");
        assert_eq!(s.prev_match(), Some(11), "wraps back to the last");
        assert_eq!(s.prev_match(), Some(8));
        assert_eq!(s.current_index(), Some(2));
    }

    #[test]
    fn next_prev_are_noops_without_matches() {
        let s = state();
        assert_eq!(s.next_match(), None);
        assert_eq!(s.prev_match(), None);
        assert_eq!(s.current_index(), None);
    }

    #[test]
    fn jump_to_sets_cursor_and_guards_range() {
        let s = state();
        s.set_query(Some(GridFilter::eq(2, "Done")));
        assert_eq!(s.jump_to(2), Some(8), "match position 2 is source 8");
        assert_eq!(s.current_index(), Some(2));
        assert_eq!(s.jump_to(99), None, "out-of-range jump is a no-op");
        assert_eq!(
            s.current_index(),
            Some(2),
            "cursor unchanged after a bad jump"
        );
    }

    #[test]
    fn numeric_facet_query_reuses_grid_filter_ops() {
        // Reuse of the R997 multi-facet predicate: score >= 50 AND status=Active.
        let s = state();
        let q = GridFilter::all(vec![
            ColumnFacet::new(1, FilterOp::Ge, "50"),
            ColumnFacet::new(2, FilterOp::Eq, "Active"),
        ]);
        let n = s.set_query(Some(q));
        // Active rows are 1,4,7,10 with scores 7,28,49,70 → only 70 (row 10) is ≥ 50.
        assert_eq!(n, 1);
        assert_eq!(s.current_row(), Some(10));
    }

    #[test]
    fn query_surfaces_match_set_and_cursor() {
        let mut e = ext();
        e.invoke("set_query", IntrospectValue::Text("2=Done".into()))
            .unwrap();
        assert_eq!(e.query("query"), Ok(IntrospectValue::Text("2=Done".into())));
        assert_eq!(e.query("match_count"), Ok(IntrospectValue::Int(4)));
        assert_eq!(e.query("current"), Ok(IntrospectValue::Int(0)));
        assert_eq!(e.query("current_row"), Ok(IntrospectValue::Int(2)));
        assert_eq!(e.query("count"), Ok(IntrospectValue::Int(12)));
        assert_eq!(e.query("match.0"), Ok(IntrospectValue::Int(2)));
        assert_eq!(e.query("match.3"), Ok(IntrospectValue::Int(11)));
        assert_eq!(
            e.query("match.9"),
            Ok(IntrospectValue::Null),
            "out-of-range match position is present-but-empty, not absent",
        );
        assert_eq!(
            e.query("nope"),
            Err(ReadRefusal::UnknownPath),
            "undeclared path is genuinely absent"
        );
    }

    #[test]
    fn invoke_set_query_next_prev_jump_clear_return_outcome() {
        let mut e = ext();
        assert_eq!(
            e.invoke("set_query", IntrospectValue::Text("2=Done".into())),
            Ok(IntrospectValue::Int(4)),
            "set_query returns match_count",
        );
        assert_eq!(
            e.invoke("next", IntrospectValue::Null),
            Ok(IntrospectValue::Int(5))
        );
        assert_eq!(
            e.invoke("prev", IntrospectValue::Null),
            Ok(IntrospectValue::Int(2))
        );
        assert_eq!(
            e.invoke("jump", IntrospectValue::Int(3)),
            Ok(IntrospectValue::Int(11))
        );
        assert_eq!(
            e.invoke("clear", IntrospectValue::Null),
            Ok(IntrospectValue::Int(0)),
            "clear returns match_count 0",
        );
        assert_eq!(e.query("query"), Ok(IntrospectValue::Text("none".into())));
        assert_eq!(e.query("current"), Ok(IntrospectValue::Null));
        assert_eq!(
            e.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn intervene_sets_query_and_guards_readonly() {
        let mut e = ext();
        e.intervene("query", IntrospectValue::Text("2=Active".into()))
            .expect("query set");
        assert_eq!(e.state().query(), Some(GridFilter::eq(2, "Active")));
        // "none" clears.
        e.intervene("query", IntrospectValue::Text("none".into()))
            .expect("query clear");
        assert_eq!(e.state().query(), None);
        assert_eq!(
            e.intervene("match_count", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            e.intervene("query", IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch),
        );
        assert_eq!(
            e.intervene("nope", IntrospectValue::Null),
            Err(InterveneError::UnknownPath),
        );
    }

    #[test]
    fn phantom_column_query_is_dropped_on_parse() {
        let mut e = ext();
        // Column 9 does not exist in a 3-wide grid → clamped away → no search.
        e.invoke("set_query", IntrospectValue::Text("9=Done".into()))
            .unwrap();
        assert_eq!(
            e.state().query(),
            None,
            "phantom-column query collapses to no search"
        );
        assert_eq!(e.query("match_count"), Ok(IntrospectValue::Int(0)));
    }

    #[test]
    fn with_reveal_scrolls_the_current_match_into_view() {
        // The "jump" half: a cursor move scrolls the match into view through
        // the attached ScrollState (the keyboard-nav reveal idiom).
        let s = state();
        let scroll = Rc::new(ScrollState::new());
        scroll.set_max(0, 12 * 32);
        scroll.set_measured_viewport(360, 64); // ~2 rows tall
        let mut e = RowSearchExternal::new(Rc::clone(&s)).with_reveal(Rc::clone(&scroll), 32);

        // Search for Active rows (1, 4, 7, 10); cursor lands on row 1 (near top).
        e.invoke("set_query", IntrospectValue::Text("2=Active".into()))
            .unwrap();
        // Jump to the last match (source row 10) — deep enough to require a scroll.
        e.invoke("jump", IntrospectValue::Int(3)).unwrap();
        assert_eq!(s.current_row(), Some(10));
        let deep = scroll.offset_y();
        assert!(deep > 0, "deep match revealed: offset {deep}");
        // Jump back to the first match (source row 1) — scrolls up, aligning the
        // row's top to the viewport (offset = 1 * pitch = 32).
        e.invoke("jump", IntrospectValue::Int(0)).unwrap();
        assert!(scroll.offset_y() < deep, "jumping back scrolls up");
        assert_eq!(
            scroll.offset_y(),
            32,
            "first match (source row 1) aligns to its row top"
        );
    }
}
