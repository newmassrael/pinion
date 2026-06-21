//! R778 §5.27 §5.40 — **multi-column sort view-order proxy for a Model/View
//! data grid at scale**.
//!
//! The grid analog of [`view_order`](crate::widgets::view_order): R775 / R777
//! land a virtualized data grid (window an N-row × C-column dataset) with
//! selection held by data index. This module adds the sort axis a data grid
//! needs — a clicked column header sorts the whole grid — **without
//! materializing the rendered rows**. The view windows over *view positions*
//! `0..view_len`; each position resolves to a source row through the
//! [`order`](GridSortState::order) permutation; the row builder paints the
//! source row; the R777 [`VirtualSelectExternal`](crate::widgets::virtual_select)
//! still holds a **source** index, so re-sorting moves a selected row's
//! visual position while keeping it selected — selection ⊥ ordering, both
//! data-indexed (the same decisive Model/View separation the 1-D list draws,
//! mirroring Qt's `QSortFilterProxyModel` ⊥ `QItemSelectionModel`).
//!
//! ## Why a peer of [`ViewOrderState`](crate::widgets::view_order::ViewOrderState), not a reuse
//!
//! The 1-D list proxy sorts a **single key** column with a generic [`Ord`]
//! comparison ([`compute_order`](crate::widgets::view_order::compute_order)).
//! A data grid sorts **any** of its columns, and a column of numbers sorted
//! lexicographically (`"12" < "9"`) is visibly wrong — so the grid proxy is
//! multi-column and **numeric-aware**, sorting through the
//! [`cell_cmp`](crate::widgets::table::cell_cmp) /
//! [`grid_order_by`](crate::widgets::table::grid_order_by) SSOT the eager
//! [`Table`](crate::widgets::table::Table) already uses. The two proxies share
//! that comparator + the [`cycle_col_sort`](crate::widgets::table::cycle_col_sort)
//! transition, not their whole shape: the sort *representation* differs
//! (`Option<(col, bool)>` vs `Option<bool>`), so a forced merge would be a
//! wrong abstraction.
//!
//! ## One reactive source of truth ([`GridSortState`] + [`use_grid_sort`])
//!
//! The active `(col, dir)` and the derived `order` permutation are held
//! **once** in a reactive [`GridSortState`] — the `ScrollState` /
//! [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) pattern this
//! crate shares an interactive axis with. The [`GridSortExternal`] is a thin
//! adapter that *mutates* the shared state on `invoke` / a clicked header; the
//! view and the a11y tree *read* the same state through `use_grid_sort(key)`
//! (the same `Rc`), so the `order` is computed **once** (memoized on the sort
//! key) and the rows the view paints are the rows the proxy's
//! `query("source_at.<n>")` reports — by construction.
//!
//! ## Scope (honest boundaries)
//!
//! - **Sort + multi-facet filter.** The grid carries both a multi-column sort
//!   and a **conjunction filter** ([`GridFilter`] — an AND of per-column
//!   [`ColumnFacet`] predicates; R783 landed the single exact-match facet,
//!   R997 generalized it to multi-facet with comparison ops), composed
//!   filter-then-sort in [`order`](GridSortState::order).
//! - **Undoable sort; filter is not.** A sort change is reversible via
//!   [`GridSortEdit`] + [`with_undo`](GridSortExternal::with_undo) (R779); the
//!   filter is deliberately kept off the undo stack — the same optionally-
//!   undoable-mutation axis the 1-D proxy defers (R779.1).
//! - **Materialized cells.** The proxy holds the source cell grid (it must,
//!   to sort / filter by any column) — the dataset, not rendered rows;
//!   identical to [`ViewOrderState`](crate::widgets::view_order::ViewOrderState)
//!   materializing its key column.

use core::cmp::Ordering;
use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use crate::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    query_proxy_external_impl,
};
use crate::reactive::{Owner, Signal};
use crate::undo::{UndoCommand, UndoStack};
use crate::widgets::order_memo::{OrderMemo, source_at_value};
use crate::widgets::table::{cell_cmp, cycle_col_sort, grid_order_by};
use crate::widgets::view_order::{sort_dir_from_str, sort_dir_str};

/// R886 §5.40 — the active sort direction of column `col`, or `None` when
/// it is not the sort column. The one home of the "does THIS header show
/// the glyph / carry `aria-sort`" decision every column-header builder
/// needs; pre-R886 hello-grouped-grid-sort, hello-table and the R886
/// editable data grid each hand-rolled the same match (the 3-copy lift
/// trigger). Pairs with `SortDirection::from_ascending` on the a11y side.
#[must_use]
pub fn col_sort_dir(sort: Option<(usize, bool)>, col: usize) -> Option<bool> {
    match sort {
        Some((c, dir)) if c == col => Some(dir),
        _ => None,
    }
}

/// R778 §5.40 — render `(col, dir)` as a single wire string: `"none"` when
/// unsorted, else `"<col>:<dir>"` (e.g. `"1:ascending"`). The compound form
/// lets an admin / AI client read or restore the **whole** grid sort state in
/// one round-trip (the column matters for a grid, unlike the 1-D list's bare
/// direction string). The direction half reuses
/// [`sort_dir_str`](crate::widgets::view_order::sort_dir_str).
#[must_use]
pub fn grid_sort_str(sort: Option<(usize, bool)>) -> String {
    match sort {
        None => "none".to_string(),
        Some((col, dir)) => format!("{col}:{}", sort_dir_str(Some(dir))),
    }
}

/// R778 §5.40 — parse a `(col, dir)` from its [`grid_sort_str`] form. Any
/// string without a `"<col>:<dir>"` shape whose direction parses as
/// ascending / descending yields `None` (unsorted) — the safe default for a
/// malformed wire payload (mirrors
/// [`sort_dir_from_str`](crate::widgets::view_order::sort_dir_from_str)).
#[must_use]
pub fn grid_sort_from_str(s: &str) -> Option<(usize, bool)> {
    let (col, dir) = s.split_once(':')?;
    let col = col.parse::<usize>().ok()?;
    let dir = sort_dir_from_str(dir)?;
    Some((col, dir))
}

/// R997 §5.40 — the comparison a [`ColumnFacet`] applies between a row's cell
/// text and the facet value. The ordered variants (`Lt`..`Ge`) compare
/// **numeric-aware** through the [`cell_cmp`] SSOT (so `"9" < "12"`, the same
/// comparator the sort uses); `Contains` is a case-sensitive substring test;
/// `Eq` / `Ne` are exact text (in)equality (the R783 single facet was an
/// implicit `Eq`). The op is part of the **wire vocab**; how a *typed* consumer
/// interprets it against its own cell representation is that consumer's policy
/// (see [`CellValue::matches_facet`](crate::cell_value::CellValue::matches_facet)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FilterOp {
    /// Exact text equality (the R783 facet).
    Eq,
    /// Exact text inequality.
    Ne,
    /// Case-sensitive substring containment.
    Contains,
    /// Numeric-aware strictly-less-than ([`cell_cmp`]).
    Lt,
    /// Numeric-aware less-than-or-equal.
    Le,
    /// Numeric-aware strictly-greater-than.
    Gt,
    /// Numeric-aware greater-than-or-equal.
    Ge,
}

impl FilterOp {
    /// The wire token for this op (`"="`, `"!="`, `"~"`, `"<"`, `"<="`, `">"`,
    /// `">="`). Read / written by [`ColumnFacet::to_wire`] / [`ColumnFacet::from_wire`].
    #[must_use]
    pub fn wire_token(self) -> &'static str {
        match self {
            FilterOp::Eq => "=",
            FilterOp::Ne => "!=",
            FilterOp::Contains => "~",
            FilterOp::Lt => "<",
            FilterOp::Le => "<=",
            FilterOp::Gt => ">",
            FilterOp::Ge => ">=",
        }
    }

    /// Parse a leading op token off `s`, returning the op and the value slice
    /// after it. Two-char tokens (`"<="`, `">="`, `"!="`) are tested before
    /// their single-char prefixes so `">="` never decodes as `">"` + `"="`.
    fn parse_prefix(s: &str) -> Option<(FilterOp, &str)> {
        // Longest tokens first (the two-char ops share a first byte with `<`/`>`).
        const TOKENS: [(&str, FilterOp); 7] = [
            ("<=", FilterOp::Le),
            (">=", FilterOp::Ge),
            ("!=", FilterOp::Ne),
            ("~", FilterOp::Contains),
            ("=", FilterOp::Eq),
            ("<", FilterOp::Lt),
            (">", FilterOp::Gt),
        ];
        TOKENS
            .iter()
            .find_map(|&(tok, op)| s.strip_prefix(tok).map(|rest| (op, rest)))
    }

    /// Whether `cell` text satisfies `self <value>`. Ordered ops route the
    /// numeric-aware [`cell_cmp`] through the shared [`ordering_matches`](Self::ordering_matches)
    /// truth table; `Contains` is a substring test; `Eq` / `Ne` are exact TEXT
    /// (in)equality (so `"9"` ≠ `"9.0"`, the R783 "exact cell text" contract,
    /// unlike the numeric ordered ops). The text policy of the at-scale
    /// [`GridSortState`] (`String` cells); a typed grid applies its own.
    #[must_use]
    pub fn matches(self, cell: &str, value: &str) -> bool {
        match self {
            FilterOp::Eq => cell == value,
            FilterOp::Ne => cell != value,
            FilterOp::Contains => cell.contains(value),
            FilterOp::Lt | FilterOp::Le | FilterOp::Gt | FilterOp::Ge => {
                self.ordering_matches(cell_cmp(cell, value))
            }
        }
    }

    /// The **universal** op-vs-[`Ordering`] truth table for the ordered ops:
    /// given `ord` = the comparison of a cell against the facet value, whether
    /// `self` (one of `Lt`..`Ge`) holds. Only the *comparator* differs per
    /// consumer — the at-scale grid feeds text [`cell_cmp`], a typed grid feeds
    /// [`CellValue::sort_cmp`](crate::cell_value::CellValue::sort_cmp) — so this
    /// is the single home both route their `Ordering` into (a divergence in the
    /// `<` / `<=` / `>` / `>=` mapping would be a bug). `Contains` is textual,
    /// not ordering-decided, and is handled before reaching here.
    #[must_use]
    pub(crate) fn ordering_matches(self, ord: Ordering) -> bool {
        match self {
            FilterOp::Lt => ord.is_lt(),
            FilterOp::Le => ord.is_le(),
            FilterOp::Gt => ord.is_gt(),
            FilterOp::Ge => ord.is_ge(),
            FilterOp::Eq => ord.is_eq(),
            FilterOp::Ne => ord.is_ne(),
            FilterOp::Contains => false,
        }
    }
}

/// R997 §5.40 — one column predicate in a [`GridFilter`] conjunction: row
/// column [`col`](Self::col) must satisfy [`op`](Self::op) against
/// [`value`](Self::value). The R783 single facet was a bare
/// `{col, value}` = an implicit [`FilterOp::Eq`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ColumnFacet {
    /// Zero-based column the facet matches on.
    pub col: usize,
    /// The comparison applied between the cell text and [`value`](Self::value).
    pub op: FilterOp,
    /// The right-hand operand (an AI-readable string, parsed numeric-aware by
    /// the ordered ops).
    pub value: String,
}

impl ColumnFacet {
    /// A facet `col op value`.
    #[must_use]
    pub fn new(col: usize, op: FilterOp, value: impl Into<String>) -> Self {
        Self {
            col,
            op,
            value: value.into(),
        }
    }

    /// Render as the wire form `"<col><op><value>"` (e.g. `"1>=500"`,
    /// `"2=Active"`). The column digits lead, the op token follows, the rest is
    /// the value verbatim — so the value may itself contain op characters
    /// (`"1=a=b"` → col 1, `Eq`, `"a=b"`), only never the `&` facet separator.
    #[must_use]
    pub fn to_wire(&self) -> String {
        format!("{}{}{}", self.col, self.op.wire_token(), self.value)
    }

    /// Parse one facet from `"<col><op><value>"`; `None` when malformed (no
    /// leading column digits, or no recognized op token after them). The
    /// column is the leading ASCII-digit run; the op token is decoded by
    /// [`FilterOp::parse_prefix`]; the remainder is the value verbatim.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        // The op token starts at the first non-digit byte; a facet with no
        // leading digits (`split == 0`) or all digits (no op) is malformed.
        let split = s.find(|c: char| !c.is_ascii_digit())?;
        if split == 0 {
            return None;
        }
        let col = s[..split].parse::<usize>().ok()?;
        let (op, value) = FilterOp::parse_prefix(&s[split..])?;
        Some(Self {
            col,
            op,
            value: value.to_string(),
        })
    }
}

/// R997 §5.40 — a **conjunction (AND) of column facets** for a data grid at
/// scale: a row passes iff it satisfies *every* facet. Generalizes the R783
/// single exact-match facet (`GridFilter{col,value}` → one [`FilterOp::Eq`]
/// facet) into the multi-facet, multi-operator filter a Wireshark / dlt-class
/// viewer needs — substring on a name column AND a numeric `>=` on a score
/// column AND an exact category — the Model/View-at-scale campaign's
/// predicate-filter slice (Qt `QSortFilterProxyModel` per-column filters,
/// `TanStack` `columnFilters`). An empty facet set matches everything, so
/// callers hold the filter as `Option<GridFilter>` with `None` the canonical
/// unfiltered state.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GridFilter {
    /// The column predicates, combined with logical AND. Empty = matches
    /// every row (unfiltered).
    pub facets: Vec<ColumnFacet>,
}

impl GridFilter {
    /// A single exact-match facet — the R783 common case (`GridFilter::eq(2,
    /// "Active")` replaces the former `GridFilter{col:2, value:"Active".into()}`).
    #[must_use]
    pub fn eq(col: usize, value: impl Into<String>) -> Self {
        Self {
            facets: vec![ColumnFacet::new(col, FilterOp::Eq, value)],
        }
    }

    /// A single facet with an explicit op.
    #[must_use]
    pub fn single(facet: ColumnFacet) -> Self {
        Self {
            facets: vec![facet],
        }
    }

    /// A conjunction of facets (combined with logical AND). An empty vec =
    /// unfiltered.
    #[must_use]
    pub fn all(facets: Vec<ColumnFacet>) -> Self {
        Self { facets }
    }

    /// Whether a row passes every facet, where `cell` yields the text of the
    /// row's column `col`. The text policy used by the at-scale
    /// [`GridSortState`]; a typed grid evaluates the facets against its own
    /// cell representation (the documented "wire form is shared, comparison is
    /// the consumer's policy" ruling). An empty conjunction passes vacuously.
    #[must_use]
    pub fn matches<'a, F: Fn(usize) -> &'a str>(&self, cell: F) -> bool {
        self.facets
            .iter()
            .all(|f| f.op.matches(cell(f.col), &f.value))
    }

    /// Drop any facet whose column is out of range for a `col_count`-wide grid;
    /// `None` when none survive (an all-phantom or empty conjunction =
    /// unfiltered). The SSOT the at-scale [`GridSortState::set_filter`], a typed
    /// editable grid, and the R1004 search cursor
    /// ([`row_search`](crate::widgets::row_search)) use to clamp a restored /
    /// wire-parsed filter against their column count, so a malformed restore
    /// never filters on a column that does not exist (generalizes the R783
    /// single-phantom-column clamp — a divergence between these consumers would
    /// be a bug, so it lives here once).
    #[must_use]
    pub fn clamped_to_col_count(mut self, col_count: usize) -> Option<Self> {
        self.facets.retain(|f| f.col < col_count);
        (!self.facets.is_empty()).then_some(self)
    }
}

/// R783 §5.40 — render an [`Option<GridFilter>`] as a single wire string:
/// `"none"` when unfiltered, else the facets in [`ColumnFacet::to_wire`] form
/// joined by `"&"` (e.g. `"0~Alpha&1>=500&2=Active"`). The compound form lets
/// an admin / AI client read or restore the whole conjunction in one
/// round-trip; the peer of [`grid_filter_from_str`].
#[must_use]
pub fn grid_filter_str(filter: Option<&GridFilter>) -> String {
    match filter {
        Some(f) if !f.facets.is_empty() => f
            .facets
            .iter()
            .map(ColumnFacet::to_wire)
            .collect::<Vec<_>>()
            .join("&"),
        _ => "none".to_string(),
    }
}

/// R783 §5.40 — parse an [`Option<GridFilter>`] from its [`grid_filter_str`]
/// form. Facets are split on `"&"` and each parsed by [`ColumnFacet::from_wire`];
/// any malformed facet is dropped, and an empty result (no `&`-shaped facet, a
/// `"none"` payload, or a non-numeric column) yields `None` (unfiltered) — the
/// safe default for a malformed wire payload. A single `"<col>=<value>"`
/// round-trips to one [`FilterOp::Eq`] facet (R783 back-compat).
#[must_use]
pub fn grid_filter_from_str(s: &str) -> Option<GridFilter> {
    let facets: Vec<ColumnFacet> = s.split('&').filter_map(ColumnFacet::from_wire).collect();
    (!facets.is_empty()).then_some(GridFilter { facets })
}

/// R783 §5.40 — the [`OrderMemo`] cache key for a [`GridSortState`]: the
/// active `(sort, filter)` config the visual→source permutation is memoized
/// against, so the order recomputes only when the sort key *or* the filter
/// facet changes.
type GridOrderKey = (Option<(usize, bool)>, Option<GridFilter>);

/// R778 §5.27 §5.40 — the reactive **single source of truth** for a data
/// grid's column sort view order.
///
/// Holds the source cell grid (materialized once), the active `(col, dir)` as
/// a reactive [`Signal`], and the derived `order` permutation (memoized). The
/// `ScrollState` of the grid-sort axis: created once via [`use_grid_sort`], it
/// is shared by the [`GridSortExternal`] (which mutates it) and the view /
/// a11y tree (which read it) through the same `Rc`. Reading
/// [`sort`](Self::sort) / [`order`](Self::order) inside a view-fn
/// auto-subscribes, so a sort change repaints exactly like a scroll-offset
/// change.
pub struct GridSortState {
    tag: Option<&'static str>,
    /// Row-major source cells — `cells[row][col]`. The dataset the sort
    /// reorders (the proxy holds data, never rendered rows).
    cells: Vec<Vec<String>>,
    col_count: usize,
    sort: Signal<Option<(usize, bool)>>,
    /// R783 §5.40 — active multi-facet column filter (R997: an AND-conjunction
    /// of [`ColumnFacet`] predicates; `None` = unfiltered). Subscribed views
    /// repaint on a filter change exactly as on a sort change.
    filter: Signal<Option<GridFilter>>,
    /// Memoized visual→source permutation, recomputed only when the
    /// [`GridOrderKey`] (`(sort, filter)`) changes — the shared [`OrderMemo`]
    /// (R780 lift).
    order: RefCell<OrderMemo<GridOrderKey>>,
}

impl GridSortState {
    /// Construct over a `col_count`-wide grid whose source row `r` has cells
    /// `cells[r]`. Starts unsorted. A row shorter than `col_count` (ragged
    /// data) reads `""` for the missing cells — the same blank-cell tolerance
    /// the paint layer applies.
    #[must_use]
    pub fn new(col_count: usize, cells: Vec<Vec<String>>) -> Self {
        Self {
            tag: None,
            cells,
            col_count,
            sort: Signal::new(None),
            filter: Signal::new(None),
            order: RefCell::new(OrderMemo::new()),
        }
    }

    /// As [`new`](Self::new) but records the `use_grid_sort` cache key, for
    /// symmetry with [`ScrollState::with_tag`](crate::widgets::scroll::ScrollState::with_tag).
    #[must_use]
    pub fn with_tag(key: &'static str, col_count: usize, cells: Vec<Vec<String>>) -> Self {
        Self {
            tag: Some(key),
            ..Self::new(col_count, cells)
        }
    }

    /// The `use_grid_sort` cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// Source row count.
    #[must_use]
    pub fn count(&self) -> usize {
        self.cells.len()
    }

    /// Column count (the sortable column bound).
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

    /// Active sort key `(col, ascending)`, or `None` when unsorted.
    /// Subscribes when read inside a view-fn.
    #[must_use]
    pub fn sort(&self) -> Option<(usize, bool)> {
        self.sort.get()
    }

    /// R783 §5.40 — active column filter, or `None` when unfiltered.
    /// Subscribes when read inside a view-fn.
    #[must_use]
    pub fn filter(&self) -> Option<GridFilter> {
        self.filter.get()
    }

    /// Whether source row `row` passes the active filter (always `true` when
    /// unfiltered): its cells satisfy every facet in the conjunction, compared
    /// as text through the [`GridFilter::matches`] policy.
    #[must_use]
    fn passes_filter(&self, row: usize, filter: Option<&GridFilter>) -> bool {
        filter.is_none_or(|f| f.matches(|col| self.cell(row, col)))
    }

    /// The visual → source permutation for the current `(sort, filter)`,
    /// recomputed only when either changes (memoized). Subscribes to the
    /// `sort` + `filter` `Signal`s, so a view that calls this repaints on a
    /// sort *or* filter change. Cheap `Rc` clone on a cache hit. Ordered
    /// through the [`grid_order_by`] / [`cell_cmp`] SSOT (filter-then-sort: the
    /// filter shrinks the row set, the survivors sort by the numeric-aware
    /// comparison the eager [`Table::order`](crate::widgets::table::Table::order) uses).
    #[must_use]
    pub fn order(&self) -> Rc<Vec<usize>> {
        let sort = self.sort.get();
        let filter = self.filter.get();
        let filter_ref = filter.clone();
        self.order.borrow_mut().get((sort, filter), || {
            grid_order_by(
                self.cells.len(),
                sort,
                |col, a, b| cell_cmp(self.cell(a, col), self.cell(b, col)),
                |row| self.passes_filter(row, filter_ref.as_ref()),
            )
        })
    }

    /// Number of rows in the current view (rows passing the filter); equals
    /// [`count`](Self::count) when unfiltered, shrinks under an active filter.
    #[must_use]
    pub fn view_len(&self) -> usize {
        self.order().len()
    }

    /// Source data index painted at visual position `view_pos`, or `None` when
    /// out of range.
    #[must_use]
    pub fn source_at(&self, view_pos: usize) -> Option<usize> {
        self.order().get(view_pos).copied()
    }

    /// Set the sort directly (admin / restore). A `Signal` write repaints
    /// every view that read [`sort`](Self::sort) / [`order`](Self::order). An
    /// out-of-range column clamps to `None` (unsorted) so a malformed restore
    /// never points the glyph at a phantom column.
    pub fn set_sort(&self, sort: Option<(usize, bool)>) {
        let sort = sort.filter(|&(c, _)| c < self.col_count);
        self.sort.set(sort);
    }

    /// Cycle the sort on `col` the way a clicked column header does
    /// (unsorted → asc → desc → unsorted; a different column jumps to it
    /// ascending) via the [`cycle_col_sort`] SSOT. Out-of-range `col` is a
    /// silent no-op.
    pub fn cycle_sort(&self, col: usize) {
        self.sort
            .set(cycle_col_sort(self.sort.get(), col, self.col_count));
    }

    /// R783 §5.40 — set the column filter directly (`None` clears). A
    /// `Signal` write repaints every view that read [`filter`](Self::filter)
    /// / [`order`](Self::order). An out-of-range column clamps to `None`
    /// (unfiltered) so a malformed restore never filters on a phantom column.
    /// Returns the resulting [`view_len`](Self::view_len) (rows passing the
    /// filter) so the AI-first `set_filter` path reports the new view size in
    /// one round-trip (mirrors
    /// [`ViewOrderState::set_filter`](crate::widgets::view_order::ViewOrderState::set_filter)).
    /// Facets on a phantom (out-of-range) column are dropped, and a conjunction
    /// left empty clamps to `None` (unfiltered) — so a malformed restore never
    /// filters on a column that does not exist (generalizes the R783
    /// single-phantom-column clamp).
    pub fn set_filter(&self, filter: Option<GridFilter>) -> usize {
        let filter = filter.and_then(|f| f.clamped_to_col_count(self.col_count));
        self.filter.set(filter);
        self.view_len()
    }
}

/// R778 §5.27 §5.40 — resolve the shared [`GridSortState`] for `key`,
/// building it once via `data` (the column count + source cells). Mirrors
/// [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) /
/// [`use_view_order`](crate::widgets::view_order::use_view_order): the
/// `External` and the view both call this with the same `key` and receive the
/// same `Rc`, so the order is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
#[must_use]
pub fn use_grid_sort(
    key: &'static str,
    data: impl FnOnce() -> (usize, Vec<Vec<String>>),
) -> Rc<GridSortState> {
    Owner::current()
        .expect("use_grid_sort requires an active Owner scope")
        .cache(key, || {
            let (col_count, cells) = data();
            GridSortState::with_tag(key, col_count, cells)
        })
}

/// R779 §5.40 §5.52 — a reversible column-sort change of a shared
/// [`GridSortState`]: the **third** [`UndoCommand`] consumer.
///
/// The bespoke peer of [`SortFilterEdit`](crate::widgets::view_order::SortFilterEdit):
/// both are `{ Rc<holder> + before/after value + label }` whose `redo`/`undo`
/// restore the captured value through the holder's setter. They are kept as
/// **separate typed commands** (not folded into one generic closure-edit)
/// deliberately — the local convention, set by [`SignalEdit`](crate::undo::SignalEdit)
/// (single `Signal`) vs `SortFilterEdit` (compound `ViewOrderState`), is one
/// typed `UndoCommand` per holder (the `QUndoCommand`-per-command model): the
/// reversal body is two trivial lines, so a generic closure-edit would erase
/// the typed holder and add `Rc<dyn Fn>` indirection for no real saving. This
/// command restores the single `Option<(col, bool)>` sort key via
/// [`GridSortState::set_sort`].
pub struct GridSortEdit {
    state: Rc<GridSortState>,
    before: Option<(usize, bool)>,
    after: Option<(usize, bool)>,
    label: Cow<'static, str>,
}

impl GridSortEdit {
    /// Capture the current sort as the `before` snapshot; the edit moves the
    /// state to `after` when recorded onto an [`UndoStack`]
    /// ([`UndoStack::record`] applies it via [`redo`](UndoCommand::redo), so
    /// the stack is the single mutation path).
    #[must_use]
    pub fn capture(
        state: &Rc<GridSortState>,
        after: Option<(usize, bool)>,
        label: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            state: Rc::clone(state),
            before: state.sort(),
            after,
            label: label.into(),
        }
    }
}

impl UndoCommand for GridSortEdit {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        self.state.set_sort(self.after);
    }

    fn undo(&self) {
        self.state.set_sort(self.before);
    }
}

/// R778 §5.27 §5.40 — the **RPC / pointer adapter** over a shared
/// [`GridSortState`] (the grid peer of
/// [`ViewSortFilterExternal`](crate::widgets::view_order::ViewSortFilterExternal)).
///
/// All state lives in the shared [`GridSortState`] — the external holds only
/// the `Rc`, so the view paints the same `order` this external reports. A
/// clicked column header routes its pointer arc here as `"h<col>:<EventName>"`
/// (the [`GridSendKey::Header`](crate::composite_tag::GridSendKey) wire the
/// R777 selection coordinator deliberately ignores), and the activation edge
/// cycles that column's sort. R779: attach an [`UndoStack`] with
/// [`with_undo`](Self::with_undo) to make every sort change reversible.
#[derive(Clone)]
pub struct GridSortExternal {
    state: Rc<GridSortState>,
    /// When attached, every sort mutation is recorded as a reversible
    /// [`GridSortEdit`] instead of mutating the state directly (R779 §5.52).
    undo: Option<Rc<UndoStack>>,
}

impl core::fmt::Debug for GridSortExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GridSortExternal")
            .field("sort", &grid_sort_str(self.state.sort()))
            .field("filter", &grid_filter_str(self.state.filter().as_ref()))
            .field("count", &self.state.count())
            .finish_non_exhaustive()
    }
}

impl GridSortExternal {
    /// Wrap the shared [`GridSortState`] (from [`use_grid_sort`]). Sort changes
    /// mutate the state directly; attach an [`UndoStack`] with
    /// [`with_undo`](Self::with_undo) to make them reversible.
    #[must_use]
    pub fn new(state: Rc<GridSortState>) -> Self {
        Self { state, undo: None }
    }

    /// Route every sort mutation through `stack` as a reversible
    /// [`GridSortEdit`] (R779 §5.52), so `invoke "undo"` / `"redo"` on the
    /// stack step the column sort back and forth.
    #[must_use]
    pub fn with_undo(mut self, stack: Rc<UndoStack>) -> Self {
        self.undo = Some(stack);
        self
    }

    /// The shared state handle (the view reaches the same `Rc` via
    /// [`use_grid_sort`]).
    #[must_use]
    pub fn state(&self) -> &Rc<GridSortState> {
        &self.state
    }

    /// Apply a sort key, recording it as a reversible [`GridSortEdit`] when an
    /// undo stack is attached, else mutating the shared state directly. The
    /// single mutation path for every sort change (header click, `invoke`,
    /// `intervene`) so the undo timeline captures them all.
    fn apply_sort(&self, after: Option<(usize, bool)>) {
        if let Some(stack) = &self.undo {
            stack.record(GridSortEdit::capture(
                &self.state,
                after,
                format!("Sort {}", grid_sort_str(after)),
            ));
        } else {
            self.state.set_sort(after);
        }
    }

    /// Cycle the sort on `col` (recorded when an undo stack is attached) via
    /// the [`cycle_col_sort`] SSOT.
    fn apply_cycle_sort(&self, col: usize) {
        let after = cycle_col_sort(self.state.sort(), col, self.state.col_count());
        self.apply_sort(after);
    }

    /// Drive the composite pointer channel: a clicked column header routes the
    /// pointer arc here. Only a [`GridSendKey::Header`](crate::composite_tag::GridSendKey)
    /// key on the activation edge (`PointerUp` / `KeyboardActivate`) cycles
    /// that column's sort; a cell key or any non-activation arc is a harmless
    /// no-op (sort ⊥ selection — cells are the selection coordinator's axis).
    fn handle_send(&self, payload: &str) {
        // R880.1 — decode via the `split_send_payload` `:` grammar SSOT so a
        // modifier-held header release ("h1:PointerUp:s") still cycles the
        // sort (the hand-rolled split_once read "PointerUp:s" as the event
        // name and the activation test silently failed).
        let Some((key, event_name, _mods)) = crate::composite_tag::split_send_payload(payload)
        else {
            return;
        };
        if let Some(crate::composite_tag::GridSendKey::Header { col }) =
            crate::composite_tag::GridSendKey::parse(key)
        {
            if crate::input::is_activation_event(event_name) {
                self.apply_cycle_sort(col);
            }
        }
    }

    /// The active sort column as an `IntrospectValue` (`Int`, or `-1` when
    /// unsorted) — the uniform "no column" sentinel the table external uses
    /// for `selected_row`.
    fn sort_col_value(&self) -> IntrospectValue {
        let col = self.state.sort().and_then(|(c, _)| i64::try_from(c).ok());
        IntrospectValue::Int(col.unwrap_or(-1))
    }

    /// R783 §5.40 — apply a column filter directly (the single mutation path
    /// for the header-less filter axis). Unlike the sort, the filter is *not*
    /// recorded on the undo stack: undoable filter is the same optionally-
    /// undoable-mutation axis deferred for the 1-D proxy (R779.1 carry), kept
    /// out of this slice. Returns the resulting view length.
    fn apply_set_filter(&self, filter: Option<GridFilter>) -> usize {
        self.state.set_filter(filter)
    }
}

// The shared display-config-proxy `External` skeleton (R847 SSOT): no §5.20
// intent — the sort `Signal` write already repaints every subscribed view, so
// the widget is never independently dirty.
query_proxy_external_impl!(GridSortExternal);

impl ExternalIntrospect for GridSortExternal {
    fn schema(&self) -> IntrospectSchema {
        // `sort`      — compound "<col>:<dir>" / "none" (query + intervene).
        // `sort_col`  — active column id, or -1 (query only).
        // `sort_dir`  — none/ascending/descending (query only).
        // `filter`    — compound "<col><op><value>&..." / "none" (query + intervene).
        // `view_len`  — rows passing the filter (query only).
        // `count`     — source row count (query only).
        // `cols`      — column count (query only).
        // `source_at` — `source_at.<pos>` visual→source map (query only).
        // `cycle_sort`/`set_filter`/`send` — invoke channels.
        IntrospectSchema::new(&[
            ("sort", "string"),
            ("sort_col", "int"),
            ("sort_dir", "string"),
            ("filter", "string"),
            ("view_len", "int"),
            ("count", "int"),
            ("cols", "int"),
            ("source_at", "int"),
            ("cycle_sort", "int"),
            ("set_filter", "string"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // `source_at.<pos>` resolves the visual→source map; an out-of-range
        // position reports Null (present-but-empty), never absence.
        if let Some(rest) = path.strip_prefix("source_at.") {
            return Some(source_at_value(rest, |p| self.state.source_at(p)));
        }
        match path {
            "sort" => Some(IntrospectValue::Text(grid_sort_str(self.state.sort()))),
            "sort_col" => Some(self.sort_col_value()),
            "sort_dir" => Some(IntrospectValue::Text(
                sort_dir_str(self.state.sort().map(|(_, dir)| dir)).into(),
            )),
            "filter" => Some(IntrospectValue::Text(grid_filter_str(
                self.state.filter().as_ref(),
            ))),
            "view_len" => Some(IntrospectValue::Int(
                i64::try_from(self.state.view_len()).unwrap_or(i64::MAX),
            )),
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.state.count()).unwrap_or(i64::MAX),
            )),
            "cols" => Some(IntrospectValue::Int(
                i64::try_from(self.state.col_count()).unwrap_or(i64::MAX),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Admin / restore: set the whole sort from its compound string
            // (recorded onto the undo timeline when a stack is attached).
            "sort" => match value {
                IntrospectValue::Text(ref s) => {
                    self.apply_sort(grid_sort_from_str(s));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // Admin / restore: set the column filter from its compound string,
            // or clear it with Null (the header-less filter axis).
            "filter" => match value {
                IntrospectValue::Text(ref s) => {
                    self.apply_set_filter(grid_filter_from_str(s));
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.apply_set_filter(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "sort_col" | "sort_dir" | "view_len" | "count" | "cols" | "source_at" => {
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
            // AI-first sort cycle on a column — returns the resulting compound
            // sort string in one round-trip.
            "cycle_sort" => match args {
                IntrospectValue::Int(i) => {
                    let col = usize::try_from(i).map_err(|_| InvokeError::TypeMismatch)?;
                    self.apply_cycle_sort(col);
                    Ok(IntrospectValue::Text(grid_sort_str(self.state.sort())))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // AI-first column filter: a `"<col><op><value>&..."` conjunction
            // payload filters, Null clears. Returns the resulting view length
            // (rows passing the filter) in one round-trip, so an AI client
            // learns the new view size without a follow-up query.
            "set_filter" => {
                let view_len = match args {
                    IntrospectValue::Text(ref s) => self.apply_set_filter(grid_filter_from_str(s)),
                    IntrospectValue::Null => self.apply_set_filter(None),
                    _ => return Err(InvokeError::TypeMismatch),
                };
                Ok(IntrospectValue::Int(
                    i64::try_from(view_len).unwrap_or(i64::MAX),
                ))
            }
            // R51.42 §5.35 composite pointer channel: a clicked column header
            // routes the pointer arc here as `<key>:<EventName>`.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    self.handle_send(payload);
                    Ok(IntrospectValue::Text(grid_sort_str(self.state.sort())))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::PointerWireEvent;
    use crate::reactive::Owner;

    // A 4-row × 2-column grid: column 0 is text, column 1 is numeric (so the
    // numeric-vs-lexicographic distinction is observable: "9" < "12").
    fn cells() -> Vec<Vec<String>> {
        vec![
            vec!["Menu".to_string(), "12".to_string()],
            vec!["Tabs".to_string(), "9".to_string()],
            vec!["Table".to_string(), "707".to_string()],
            vec!["Grid".to_string(), "775".to_string()],
        ]
    }

    fn state() -> GridSortState {
        GridSortState::new(2, cells())
    }

    #[test]
    fn grid_sort_str_round_trips() {
        for s in [None, Some((0, true)), Some((1, false)), Some((2, true))] {
            assert_eq!(grid_sort_from_str(&grid_sort_str(s)), s);
        }
        assert_eq!(grid_sort_str(None), "none");
        assert_eq!(grid_sort_str(Some((1, true))), "1:ascending");
        assert_eq!(grid_sort_from_str("garbage"), None);
        assert_eq!(grid_sort_from_str("x:ascending"), None);
    }

    #[test]
    fn unsorted_order_is_identity() {
        let s = state();
        assert_eq!(*s.order(), vec![0, 1, 2, 3]);
        assert_eq!(s.sort(), None);
    }

    #[test]
    fn cycle_sort_text_column_lexicographic() {
        let s = state();
        s.cycle_sort(0); // ascending by name: Grid, Menu, Table, Tabs
        assert_eq!(s.sort(), Some((0, true)));
        // data ids: Grid=3, Menu=0, Table=2, Tabs=1
        assert_eq!(*s.order(), vec![3, 0, 2, 1]);
        s.cycle_sort(0); // descending
        assert_eq!(s.sort(), Some((0, false)));
        assert_eq!(*s.order(), vec![1, 2, 0, 3]);
        s.cycle_sort(0); // back to source order
        assert_eq!(s.sort(), None);
        assert_eq!(*s.order(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn cycle_sort_numeric_column_is_numeric_aware() {
        let s = state();
        s.cycle_sort(1); // ascending by number: 9 (id1), 12 (id0), 707 (id2), 775 (id3)
        // A lexicographic sort would put "12" before "9"; numeric must not.
        assert_eq!(*s.order(), vec![1, 0, 2, 3]);
    }

    #[test]
    fn clicking_a_different_column_jumps_to_it_ascending() {
        let s = state();
        s.cycle_sort(0);
        assert_eq!(s.sort(), Some((0, true)));
        s.cycle_sort(1); // different column -> ascending on it
        assert_eq!(s.sort(), Some((1, true)));
    }

    #[test]
    fn out_of_range_column_is_a_noop() {
        let s = state();
        s.cycle_sort(9); // col_count = 2
        assert_eq!(s.sort(), None);
    }

    #[test]
    fn set_sort_clamps_out_of_range_column() {
        let s = state();
        s.set_sort(Some((9, true)));
        assert_eq!(s.sort(), None, "phantom column clamps to unsorted");
        s.set_sort(Some((1, false)));
        assert_eq!(s.sort(), Some((1, false)));
    }

    #[test]
    fn source_at_tracks_the_permutation() {
        let s = state();
        s.cycle_sort(0); // order = [3, 0, 2, 1]
        assert_eq!(s.source_at(0), Some(3));
        assert_eq!(s.source_at(3), Some(1));
        assert_eq!(s.source_at(4), None);
    }

    #[test]
    fn external_send_header_cycles_sort() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            // h1:PointerUp -> sort column 1 ascending.
            let payload = format!("h1:{}", PointerWireEvent::Up.as_wire_name());
            ext.invoke("send", IntrospectValue::Text(payload)).unwrap();
            assert_eq!(st.sort(), Some((1, true)));
            // A cell key is ignored (selection coordinator's axis).
            ext.invoke(
                "send",
                IntrospectValue::Text(format!("0_0:{}", PointerWireEvent::Up.as_wire_name())),
            )
            .unwrap();
            assert_eq!(st.sort(), Some((1, true)), "cell send does not touch sort");
        });
    }

    #[test]
    fn external_invoke_cycle_sort_returns_compound_string() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            let out = ext.invoke("cycle_sort", IntrospectValue::Int(0)).unwrap();
            assert_eq!(out, IntrospectValue::Text("0:ascending".into()));
            assert_eq!(ext.query("sort_col"), Some(IntrospectValue::Int(0)));
            assert_eq!(
                ext.query("sort"),
                Some(IntrospectValue::Text("0:ascending".into()))
            );
        });
    }

    #[test]
    fn external_intervene_restores_whole_sort() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            ext.intervene("sort", IntrospectValue::Text("1:descending".into()))
                .unwrap();
            assert_eq!(st.sort(), Some((1, false)));
            ext.intervene("sort", IntrospectValue::Text("none".into()))
                .unwrap();
            assert_eq!(st.sort(), None);
            assert_eq!(
                ext.intervene("count", IntrospectValue::Int(0)),
                Err(InterveneError::ReadOnly)
            );
        });
    }

    #[test]
    fn grid_sort_edit_redo_undo_restore_the_sort() {
        // The bare command (3rd UndoCommand consumer): redo applies `after`,
        // undo restores the captured `before`.
        let st = Rc::new(state());
        st.set_sort(Some((0, true)));
        let edit = GridSortEdit::capture(&st, Some((1, false)), "Sort 1:descending");
        edit.redo();
        assert_eq!(st.sort(), Some((1, false)), "redo applies the after sort");
        edit.undo();
        assert_eq!(
            st.sort(),
            Some((0, true)),
            "undo restores the captured before sort"
        );
        assert_eq!(edit.label(), "Sort 1:descending");
    }

    #[test]
    fn r880_1_header_click_with_modifier_segment_still_sorts() {
        // The router emits "h0:PointerUp:<token>" when modifiers are held
        // (R781); the pre-R880.1 hand-rolled split read "PointerUp:s" as
        // the event name and the activation test silently failed.
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            ext.invoke("send", IntrospectValue::Text("h0:PointerUp:s".into()))
                .unwrap();
            assert_eq!(
                st.sort(),
                Some((0, true)),
                "Shift+click still cycles the sort"
            );
        });
    }

    #[test]
    fn external_with_undo_records_every_sort_path() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let stack = Rc::new(UndoStack::new());
            let mut ext = GridSortExternal::new(Rc::clone(&st)).with_undo(Rc::clone(&stack));

            // Header click cycles + records.
            ext.invoke(
                "send",
                IntrospectValue::Text(format!("h0:{}", PointerWireEvent::Up.as_wire_name())),
            )
            .unwrap();
            assert_eq!(st.sort(), Some((0, true)));
            assert!(
                stack.can_undo(),
                "the header-click sort is on the undo timeline"
            );

            // invoke + intervene also record onto the same timeline.
            ext.invoke("cycle_sort", IntrospectValue::Int(0)).unwrap(); // 0:desc
            ext.intervene("sort", IntrospectValue::Text("1:ascending".into()))
                .unwrap();
            assert_eq!(st.sort(), Some((1, true)));

            // Unwind the whole timeline: 1:asc -> 0:desc -> 0:asc -> none.
            stack.undo();
            assert_eq!(st.sort(), Some((0, false)), "undo the intervene");
            stack.undo();
            assert_eq!(st.sort(), Some((0, true)), "undo the cycle");
            stack.undo();
            assert_eq!(st.sort(), None, "undo the header click → unsorted");
            // Redo re-applies forward.
            stack.redo();
            assert_eq!(
                st.sort(),
                Some((0, true)),
                "redo re-applies the header click"
            );
        });
    }

    #[test]
    fn external_without_undo_mutates_directly() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(state());
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            ext.invoke("cycle_sort", IntrospectValue::Int(1)).unwrap();
            assert_eq!(
                st.sort(),
                Some((1, true)),
                "no undo stack → direct mutation (unchanged behaviour)"
            );
        });
    }

    // ── R783 §5.40 column filter ──────────────────────────────────────

    // A 6-row × 2-column grid: column 0 is a category facet with repeats
    // (so a filter keeps a *subset*), column 1 is numeric (so filter-then-sort
    // composition is observable on the survivors).
    fn cat_cells() -> Vec<Vec<String>> {
        vec![
            vec!["A".to_string(), "30".to_string()], // 0
            vec!["B".to_string(), "10".to_string()], // 1
            vec!["A".to_string(), "20".to_string()], // 2
            vec!["B".to_string(), "5".to_string()],  // 3
            vec!["A".to_string(), "9".to_string()],  // 4
            vec!["C".to_string(), "1".to_string()],  // 5
        ]
    }

    #[test]
    fn grid_filter_str_round_trips() {
        let cases = [None, Some(GridFilter::eq(0, "Active"))];
        for f in cases {
            assert_eq!(grid_filter_from_str(&grid_filter_str(f.as_ref())), f);
        }
        assert_eq!(grid_filter_str(None), "none");
        assert_eq!(grid_filter_str(Some(&GridFilter::eq(2, "Done"))), "2=Done");
        assert_eq!(grid_filter_from_str("none"), None, "no facet → unfiltered");
        assert_eq!(
            grid_filter_from_str("x=Done"),
            None,
            "non-numeric column → unfiltered"
        );
        // The value half is verbatim, even if it contains '=' (back-compat).
        assert_eq!(
            grid_filter_from_str("1=a=b"),
            Some(GridFilter::eq(1, "a=b"))
        );
    }

    #[test]
    fn filter_op_matches_text_and_numeric_aware() {
        assert!(FilterOp::Eq.matches("Active", "Active"));
        assert!(!FilterOp::Eq.matches("Active", "Idle"));
        assert!(FilterOp::Ne.matches("Active", "Idle"));
        assert!(FilterOp::Contains.matches("Alpha0042", "Alpha"));
        assert!(!FilterOp::Contains.matches("Bravo0001", "Alpha"));
        // Ordered ops are numeric-aware (a lexicographic test would say "9" > "12").
        assert!(FilterOp::Lt.matches("9", "12"), "numeric: 9 < 12");
        assert!(!FilterOp::Lt.matches("12", "9"));
        assert!(FilterOp::Le.matches("20", "20"));
        assert!(FilterOp::Gt.matches("30", "20"));
        assert!(FilterOp::Ge.matches("30", "20"));
        assert!(FilterOp::Ge.matches("20", "20"));
        assert!(!FilterOp::Ge.matches("9", "20"));
    }

    #[test]
    fn column_facet_wire_round_trips_every_op() {
        for op in [
            FilterOp::Eq,
            FilterOp::Ne,
            FilterOp::Contains,
            FilterOp::Lt,
            FilterOp::Le,
            FilterOp::Gt,
            FilterOp::Ge,
        ] {
            let facet = ColumnFacet::new(3, op, "500");
            assert_eq!(
                ColumnFacet::from_wire(&facet.to_wire()),
                Some(facet),
                "{op:?} round-trips"
            );
        }
        // A two-char op decodes before its single-char prefix.
        assert_eq!(
            ColumnFacet::from_wire("1>=5"),
            Some(ColumnFacet::new(1, FilterOp::Ge, "5"))
        );
        assert_eq!(
            ColumnFacet::from_wire("1>5"),
            Some(ColumnFacet::new(1, FilterOp::Gt, "5"))
        );
        assert_eq!(
            ColumnFacet::from_wire("1<=5"),
            Some(ColumnFacet::new(1, FilterOp::Le, "5"))
        );
        assert_eq!(
            ColumnFacet::from_wire("1!=x"),
            Some(ColumnFacet::new(1, FilterOp::Ne, "x"))
        );
        // Malformed: no leading column digits, all digits (no op), or empty.
        assert_eq!(ColumnFacet::from_wire("=x"), None);
        assert_eq!(ColumnFacet::from_wire("12"), None);
        assert_eq!(ColumnFacet::from_wire(""), None);
    }

    #[test]
    fn filter_keeps_only_matching_rows_in_source_order() {
        let s = GridSortState::new(2, cat_cells());
        assert_eq!(s.view_len(), 6, "unfiltered view is the full dataset");
        assert_eq!(s.set_filter(Some(GridFilter::eq(0, "A"))), 3);
        assert_eq!(
            *s.order(),
            vec![0, 2, 4],
            "only category-A rows, in source order"
        );
        assert_eq!(s.set_filter(None), 6, "clearing restores the full view");
        assert_eq!(*s.order(), vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn filter_then_sort_composes() {
        let s = GridSortState::new(2, cat_cells());
        s.set_filter(Some(GridFilter::eq(0, "A")));
        // Survivors are ids 0,2,4 with numeric col-1 values 30,20,9.
        s.cycle_sort(1); // ascending numeric over the FILTERED set
        assert_eq!(
            *s.order(),
            vec![4, 2, 0],
            "filter shrinks, then survivors sort: 9 < 20 < 30"
        );
        assert_eq!(s.view_len(), 3, "the sort does not change the view size");
    }

    #[test]
    fn multi_facet_conjunction_ands_the_predicates() {
        let s = GridSortState::new(2, cat_cells());
        // col0 == A  AND  col1 >= 20  → A-rows are 0,2,4 (vals 30,20,9); the
        // numeric `>= 20` drops id4 (9), keeping 0,2.
        let f = GridFilter::all(vec![
            ColumnFacet::new(0, FilterOp::Eq, "A"),
            ColumnFacet::new(1, FilterOp::Ge, "20"),
        ]);
        assert_eq!(s.set_filter(Some(f)), 2, "both facets must hold");
        assert_eq!(*s.order(), vec![0, 2]);
        // The sort composes over the conjunction survivors (20 < 30).
        s.cycle_sort(1);
        assert_eq!(*s.order(), vec![2, 0]);
    }

    #[test]
    fn multi_facet_wire_round_trips() {
        let f = GridFilter::all(vec![
            ColumnFacet::new(0, FilterOp::Contains, "Al"),
            ColumnFacet::new(1, FilterOp::Ge, "20"),
            ColumnFacet::new(2, FilterOp::Eq, "Active"),
        ]);
        let wire = grid_filter_str(Some(&f));
        assert_eq!(
            wire, "0~Al&1>=20&2=Active",
            "facets joined by '&' in the wire form"
        );
        assert_eq!(
            grid_filter_from_str(&wire),
            Some(f),
            "the conjunction round-trips"
        );
    }

    #[test]
    fn set_filter_drops_phantom_facets_keeps_valid() {
        let s = GridSortState::new(2, cat_cells());
        // One valid facet (col0=A) + one phantom (col9): the phantom is
        // dropped, col0=A still applies (the 3 A-rows survive).
        let mixed = GridFilter::all(vec![
            ColumnFacet::new(0, FilterOp::Eq, "A"),
            ColumnFacet::new(9, FilterOp::Eq, "x"),
        ]);
        assert_eq!(
            s.set_filter(Some(mixed)),
            3,
            "phantom facet dropped, col0=A kept"
        );
        assert_eq!(s.filter(), Some(GridFilter::eq(0, "A")));
        // An all-phantom conjunction clamps to unfiltered (the R783 single-
        // phantom-column clamp, generalized).
        assert_eq!(s.set_filter(Some(GridFilter::eq(9, "x"))), 6);
        assert_eq!(s.filter(), None);
    }

    #[test]
    fn external_filter_query_intervene_invoke() {
        let owner = Owner::new();
        owner.run(|| {
            let st = Rc::new(GridSortState::new(2, cat_cells()));
            let mut ext = GridSortExternal::new(Rc::clone(&st));
            // Boot: unfiltered, full view.
            assert_eq!(
                ext.query("filter"),
                Some(IntrospectValue::Text("none".into()))
            );
            assert_eq!(ext.query("view_len"), Some(IntrospectValue::Int(6)));
            // invoke set_filter returns the resulting view length in one trip.
            assert_eq!(
                ext.invoke("set_filter", IntrospectValue::Text("0=A".into())),
                Ok(IntrospectValue::Int(3)),
            );
            assert_eq!(
                ext.query("filter"),
                Some(IntrospectValue::Text("0=A".into()))
            );
            assert_eq!(ext.query("view_len"), Some(IntrospectValue::Int(3)));
            // A multi-facet conjunction over the wire (col0=A AND col1>=20).
            assert_eq!(
                ext.invoke("set_filter", IntrospectValue::Text("0=A&1>=20".into())),
                Ok(IntrospectValue::Int(2)),
            );
            assert_eq!(
                ext.query("filter"),
                Some(IntrospectValue::Text("0=A&1>=20".into()))
            );
            // Null clears via invoke.
            assert_eq!(
                ext.invoke("set_filter", IntrospectValue::Null),
                Ok(IntrospectValue::Int(6))
            );
            assert_eq!(st.filter(), None);
            // intervene sets + clears (admin / restore path).
            ext.intervene("filter", IntrospectValue::Text("0=B".into()))
                .unwrap();
            assert_eq!(st.filter(), Some(GridFilter::eq(0, "B")));
            ext.intervene("filter", IntrospectValue::Null).unwrap();
            assert_eq!(st.filter(), None);
            // view_len is read-only.
            assert!(ext.intervene("view_len", IntrospectValue::Int(1)).is_err());
        });
    }
}
