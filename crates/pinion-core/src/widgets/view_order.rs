//! R747 §5.27 §5.40 — **sort / filter view-order proxy for a Model/View list**.
//!
//! R744–R746 land virtualization (window an N-row dataset) and selection
//! held by data index. This module adds the third Model/View axis: a
//! **view-order proxy** that maps *visual* row positions to *source* data
//! indices under a sort and a filter, without materializing the dataset.
//!
//! This is the canonical proxy-model layer every data grid has between its
//! source rows and its view: Qt `QSortFilterProxyModel`, WPF
//! `CollectionView`, `TanStack` Table's sorted/filtered row model. The view
//! windows over *view positions* `0..view_len`; each position resolves to a
//! source index through [`compute_order`]; the row builder paints the source
//! row; selection (the R746 [`VirtualSelectExternal`](crate::widgets::virtual_select))
//! still holds a **source** index, so re-sorting moves a selected row's
//! visual position while keeping it selected — selection ⊥ ordering, both
//! data-indexed.
//!
//! ## One reactive source of truth ([`ViewOrderState`] + [`use_view_order`])
//!
//! The `(sort, filter)` and the derived `order` permutation are held **once**
//! in a reactive [`ViewOrderState`] — the exact `ScrollState` /
//! [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) pattern this
//! crate already uses to share an interactive axis between an `External` and a
//! view. The [`ViewSortFilterExternal`] is a thin adapter that *mutates* the
//! shared state on `invoke` / a clicked header; the view and the a11y tree
//! *read* the same state through `use_view_order(key)` (the same `Rc`), so the
//! `order` is computed **once** (memoized on `(sort, filter)`) and the rows
//! the view paints are the same rows the proxy's `query("source_at.<n>")`
//! reports — by construction, not by two code paths happening to agree.
//! Reading `sort()` / `filter()` / `order()` inside a view auto-subscribes to
//! the underlying `Signal`s, so a sort/filter change repaints exactly as a
//! scroll-offset change does — no `read_state` projection of the order is
//! needed (the order is not state to copy; R746.1: a reactive `Signal`
//! already drives the repaint).
//!
//! ## Sort representation (`Option<bool>`, the R730 table convention)
//!
//! The sort key is `Option<bool>`: `None` = source order, `Some(true)` =
//! ascending, `Some(false)` = descending — exactly
//! [`Table::sort_state`](crate::widgets::table)'s `Option<(col, bool)>`
//! minus the column (a 1-D list has a single key). A dedicated `SortDir`
//! enum is deliberately **not** declared: `pinion_a11y::SortDirection`
//! already exists for the `aria-sort` mapping, and `pinion-core` cannot
//! depend on `pinion-a11y` (wrong direction), so a core enum would be a
//! parallel re-declaration of it. The binding maps this `Option<bool>` to
//! `aria-sort` at the layer that has both crates.
//!
//! ## Filter representation
//!
//! `Option<usize>` — a single category id, or `None` for "show all". The
//! filter is a membership predicate over source rows; a row passes when its
//! category equals the active filter (or always, when `None`). Multi-facet
//! / predicate filters are a later additive axis (one filter dimension is
//! the first consumer's need).
//!
//! ## Scope (honest boundaries)
//!
//! - **Single sort key, single filter facet.** Multi-column sort and
//!   compound filters are additive when a consumer needs them.
//! - **O(n log n) per recompute, cached.** The proxy recomputes the order
//!   only when the sort or filter changes (not per frame); a stable sort
//!   keeps equal-key rows in source order so re-sorting is deterministic.
//! - **No `Model` trait.** The source is the consumer's per-row key + a
//!   category column, not a retained trait object — `compute_order` is a
//!   free function over closures (the second proxy consumer proves the
//!   shape; premature here).

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use crate::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError, SchemaArg,
    SchemaField, query_proxy_external_impl,
};
use crate::reactive::{Owner, Signal, batch};
use crate::undo::{UndoCommand, UndoStack};
use crate::widgets::order_memo::{OrderMemo, source_at_value};

/// R747 §5.40 — cycle a sort key the way a clicked sort header does:
/// unsorted → ascending → descending → unsorted.
///
/// The free-function peer of [`Table::cycle_sort`](crate::widgets::table);
/// shared so the proxy and any binding agree on the cycle.
#[must_use]
pub fn cycle_sort(sort: Option<bool>) -> Option<bool> {
    match sort {
        None => Some(true),
        Some(true) => Some(false),
        Some(false) => None,
    }
}

/// R747 §5.40 — the `aria-sort` / introspect string for a sort state:
/// `"none"` / `"ascending"` / `"descending"`.
#[must_use]
pub fn sort_dir_str(sort: Option<bool>) -> &'static str {
    match sort {
        None => "none",
        Some(true) => "ascending",
        Some(false) => "descending",
    }
}

/// R747 §5.40 — parse a sort state from its [`sort_dir_str`] form. Any
/// unrecognized string is `None` (unsorted) — the safe default for a
/// malformed wire payload.
#[must_use]
pub fn sort_dir_from_str(s: &str) -> Option<bool> {
    match s {
        "ascending" => Some(true),
        "descending" => Some(false),
        _ => None,
    }
}

/// R747 §5.27 §5.40 — build the **visual → source** permutation for a 1-D
/// list under `sort` and `filter`.
///
/// Returns `order` where `order[view_pos]` is the source data index painted
/// at visual position `view_pos`. The length is the number of rows that
/// pass the filter (≤ `count`) — windowing then runs over `order.len()`, so
/// a filtered view shrinks naturally.
///
/// # Parameters
///
/// - `count` — total source row count.
/// - `sort` — `None` source order / `Some(true)` ascending / `Some(false)`
///   descending (see the module note).
/// - `key` — sort key of source row `i` (any [`Ord`] type; typically the
///   row's display label as `&str`).
/// - `pass` — filter membership: `true` keeps source row `i`.
///
/// The sort is **stable with an explicit source-index tie-break**, so equal
/// keys keep ascending source order in both directions — re-sorting is
/// deterministic (mirrors the R730 table `order()` contract).
#[must_use]
pub fn compute_order<K: Ord>(
    count: usize,
    sort: Option<bool>,
    key: impl Fn(usize) -> K,
    pass: impl Fn(usize) -> bool,
) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).filter(|&i| pass(i)).collect();
    match sort {
        None => {}
        Some(true) => order.sort_by(|&a, &b| key(a).cmp(&key(b)).then(a.cmp(&b))),
        Some(false) => order.sort_by(|&a, &b| key(b).cmp(&key(a)).then(a.cmp(&b))),
    }
    order
}

/// R747 §5.27 §5.40 — the reactive **single source of truth** for a list's
/// sort/filter view order.
///
/// Holds the source sort keys + categories (materialized once), the active
/// `(sort, filter)` as reactive [`Signal`]s, and the derived `order`
/// permutation (memoized). This is the `ScrollState` of the view-order axis:
/// created once via [`use_view_order`], it is shared by the
/// [`ViewSortFilterExternal`] (which mutates it) and the view / a11y tree
/// (which read it) through the same `Rc`. Reading [`sort`](Self::sort) /
/// [`filter`](Self::filter) / [`order`](Self::order) inside a view-fn
/// auto-subscribes, so a config change repaints exactly like a scroll-offset
/// change.
pub struct ViewOrderState {
    tag: Option<&'static str>,
    keys: Vec<String>,
    categories: Vec<usize>,
    sort: Signal<Option<bool>>,
    filter: Signal<Option<usize>>,
    /// Memoized visual→source permutation, recomputed only when the
    /// `(sort, filter)` key changes — the shared [`OrderMemo`] (R780 lift).
    order: RefCell<OrderMemo<ViewConfig>>,
}

impl ViewOrderState {
    /// Construct over a dataset whose source row `i` has sort key `keys[i]`
    /// and filter category `categories[i]`. Starts unsorted, unfiltered.
    ///
    /// # Panics
    ///
    /// Panics if `keys` and `categories` differ in length — they are the two
    /// attribute columns of the same dataset and must agree.
    #[must_use]
    pub fn new(keys: Vec<String>, categories: Vec<usize>) -> Self {
        assert_eq!(
            keys.len(),
            categories.len(),
            "keys and categories describe the same rows",
        );
        Self {
            tag: None,
            keys,
            categories,
            sort: Signal::new(None),
            filter: Signal::new(None),
            order: RefCell::new(OrderMemo::new()),
        }
    }

    /// As [`new`](Self::new) but records the `use_view_order` cache key, for
    /// symmetry with [`ScrollState::with_tag`](crate::widgets::scroll::ScrollState::with_tag).
    #[must_use]
    pub fn with_tag(key: &'static str, keys: Vec<String>, categories: Vec<usize>) -> Self {
        Self {
            tag: Some(key),
            ..Self::new(keys, categories)
        }
    }

    /// The `use_view_order` cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// Source row count (filter-independent).
    #[must_use]
    pub fn count(&self) -> usize {
        self.keys.len()
    }

    /// Active sort state. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn sort(&self) -> Option<bool> {
        self.sort.get()
    }

    /// Active filter category (or `None`). Subscribes when read inside a
    /// view-fn.
    #[must_use]
    pub fn filter(&self) -> Option<usize> {
        self.filter.get()
    }

    /// The visual → source permutation for the current `(sort, filter)`,
    /// recomputed only when the config changes (memoized). Subscribes to the
    /// `sort` + `filter` `Signal`s, so a view that calls this repaints on a
    /// config change. Cheap `Rc` clone on a cache hit.
    #[must_use]
    pub fn order(&self) -> Rc<Vec<usize>> {
        let sort = self.sort.get();
        let filter = self.filter.get();
        self.order.borrow_mut().get((sort, filter), || {
            compute_order(
                self.keys.len(),
                sort,
                |i| self.keys[i].as_str(),
                |i| filter.is_none_or(|f| self.categories[i] == f),
            )
        })
    }

    /// Number of rows in the current view (rows passing the filter).
    #[must_use]
    pub fn view_len(&self) -> usize {
        self.order().len()
    }

    /// Source data index painted at visual position `view_pos`, or `None`
    /// when out of range.
    #[must_use]
    pub fn source_at(&self, view_pos: usize) -> Option<usize> {
        self.order().get(view_pos).copied()
    }

    /// Set the sort directly (admin / restore). A `Signal` write repaints
    /// every view that read [`sort`](Self::sort) / [`order`](Self::order).
    pub fn set_sort(&self, sort: Option<bool>) {
        self.sort.set(sort);
    }

    /// Cycle the sort (unsorted → asc → desc → unsorted) — the clicked-header
    /// + `invoke "cycle_sort"` path.
    pub fn cycle_sort(&self) {
        self.sort.set(cycle_sort(self.sort.get()));
    }

    /// Set the filter category (`None` clears). Returns the resulting
    /// [`view_len`](Self::view_len).
    pub fn set_filter(&self, filter: Option<usize>) -> usize {
        self.filter.set(filter);
        self.view_len()
    }

    /// Set the whole `(sort, filter)` config atomically. Wrapped in
    /// [`batch`] so a view subscribed to **both** `sort` and `filter`
    /// re-runs **once**, not once per axis — the
    /// [`ScrollState`](crate::widgets::scroll::ScrollState) multi-axis
    /// convention ([[signal-batch-atomic-multi-axis-update]]). The single
    /// source of truth for a dual-axis write, shared by the undoable
    /// [`SortFilterEdit`] and the direct (no-undo) external path.
    pub fn set_config(&self, (sort, filter): ViewConfig) {
        batch(|| {
            self.sort.set(sort);
            self.filter.set(filter);
        });
    }
}

/// R747 §5.27 §5.40 — resolve the shared [`ViewOrderState`] for `key`,
/// building it once via `data` (the source keys + categories). Mirrors
/// [`use_scroll_state`](crate::widgets::scroll::use_scroll_state): the
/// `External` and the view both call this with the same `key` and receive the
/// same `Rc`, so the order is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
#[must_use]
pub fn use_view_order(
    key: &'static str,
    data: impl FnOnce() -> (Vec<String>, Vec<usize>),
) -> Rc<ViewOrderState> {
    Owner::current()
        .expect("use_view_order requires an active Owner scope")
        .cache(key, || {
            let (keys, categories) = data();
            ViewOrderState::with_tag(key, keys, categories)
        })
}

/// The undoable `(sort, filter)` configuration of a [`ViewOrderState`].
pub type ViewConfig = (Option<bool>, Option<usize>);

/// R748 §5.52 — a reversible `(sort, filter)` change of a shared
/// [`ViewOrderState`]: the **second** [`UndoCommand`] consumer and the
/// witness that the trait earns its erasure. Unlike
/// [`SignalEdit`](crate::undo::SignalEdit) it is a *compound* edit (it moves
/// two signals at once) targeting `ViewOrderState`'s public setters rather
/// than a single `Signal`. `redo` / `undo` restore the whole captured
/// config, so a sort cycle and a filter change recorded onto one
/// [`UndoStack`] unwind independently on a single Ctrl+Z timeline.
pub struct SortFilterEdit {
    state: Rc<ViewOrderState>,
    before: ViewConfig,
    after: ViewConfig,
    label: Cow<'static, str>,
}

impl SortFilterEdit {
    /// Capture the current `(sort, filter)` as the `before` snapshot; the
    /// edit moves the state to `after` when recorded onto an [`UndoStack`].
    #[must_use]
    pub fn capture(
        state: &Rc<ViewOrderState>,
        after: ViewConfig,
        label: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            state: Rc::clone(state),
            before: (state.sort(), state.filter()),
            after,
            label: label.into(),
        }
    }

    fn apply(&self, config: ViewConfig) {
        self.state.set_config(config);
    }
}

impl UndoCommand for SortFilterEdit {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        self.apply(self.after);
    }

    fn undo(&self) {
        self.apply(self.before);
    }
}

/// R747 §5.27 §5.40 — the sort/filter **proxy coordinator** External: a thin
/// adapter that surfaces the shared [`ViewOrderState`] to the §5.12
/// `scene/query` / `scene/intervene` / `scene/invoke` paths and the R51.42
/// composite pointer channel (a clicked sort header).
///
/// Like [`SpinButtonExternal`](crate::widgets::spin_button) it owns no
/// interaction statechart and emits **no** §5.20 intent — sort and filter are
/// display reconfigurations observed through `query` (`sort_dir` / `filter` /
/// `view_len` / `source_at.<pos>`), exactly as the R730 table surfaces sort
/// through `query` and reserves the `"selected"` intent for selection
/// (category-correct contract: a *value* holder, not a selection
/// coordinator). All state lives in the shared [`ViewOrderState`] — the
/// external holds only the `Rc`, so the view paints the same `order` this
/// external reports.
#[derive(Clone)]
pub struct ViewSortFilterExternal {
    state: Rc<ViewOrderState>,
    /// When attached (via [`with_undo`](Self::with_undo)) every sort/filter
    /// mutation is recorded as a reversible [`SortFilterEdit`] instead of
    /// mutating the state directly (R748 §5.52).
    undo: Option<Rc<UndoStack>>,
}

impl core::fmt::Debug for ViewSortFilterExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ViewSortFilterExternal")
            .field("sort", &sort_dir_str(self.state.sort()))
            .field("filter", &self.state.filter())
            .field("view_len", &self.state.view_len())
            .finish_non_exhaustive()
    }
}

impl ViewSortFilterExternal {
    /// Wrap the shared [`ViewOrderState`] (from [`use_view_order`]). Sort and
    /// filter mutate the state directly (the R747 behavior); attach an
    /// [`UndoStack`] with [`with_undo`](Self::with_undo) to make them
    /// reversible.
    #[must_use]
    pub fn new(state: Rc<ViewOrderState>) -> Self {
        Self { state, undo: None }
    }

    /// Route every sort/filter mutation through `stack` as a reversible
    /// [`SortFilterEdit`] (R748 §5.52), so `invoke "undo"` / `invoke "redo"`
    /// on the stack step the view config back and forth.
    #[must_use]
    pub fn with_undo(mut self, stack: Rc<UndoStack>) -> Self {
        self.undo = Some(stack);
        self
    }

    /// Apply a `(sort, filter)` config, recording it as a reversible
    /// [`SortFilterEdit`] when an undo stack is attached, else mutating the
    /// shared state directly.
    fn apply_config(&self, after: ViewConfig, label: impl Into<Cow<'static, str>>) {
        if let Some(stack) = &self.undo {
            stack.record(SortFilterEdit::capture(&self.state, after, label));
        } else {
            self.state.set_config(after);
        }
    }

    /// Cycle the sort (recorded when an undo stack is attached).
    fn apply_cycle_sort(&self) {
        let after = (cycle_sort(self.state.sort()), self.state.filter());
        self.apply_config(after, format!("Sort {}", sort_dir_str(after.0)));
    }

    /// Set the sort directly (admin / restore; recorded when attached).
    fn apply_set_sort(&self, sort: Option<bool>) {
        let after = (sort, self.state.filter());
        self.apply_config(after, format!("Sort {}", sort_dir_str(sort)));
    }

    /// Set / clear the filter (recorded when attached).
    fn apply_set_filter(&self, filter: Option<usize>) {
        let after = (self.state.sort(), filter);
        let label = match filter {
            Some(c) => format!("Filter {c}"),
            None => "Show all".to_string(),
        };
        self.apply_config(after, label);
    }

    /// The shared state handle (the view reaches the same `Rc` via
    /// [`use_view_order`]).
    #[must_use]
    pub fn state(&self) -> &Rc<ViewOrderState> {
        &self.state
    }

    /// Drive the composite pointer channel: a clicked sort header routes the
    /// pointer arc here; on the activation edge (`PointerUp` /
    /// `KeyboardActivate`) the sort cycles. Every other arc event is a
    /// harmless no-op.
    fn handle_send(&self, payload: &str) {
        let Some((_region, event_name, _modifiers)) =
            crate::composite_tag::parse_send_payload::<String>(payload)
        else {
            return;
        };
        if crate::input::is_activation_event(event_name) {
            self.apply_cycle_sort();
        }
    }

    /// `view_len` as an `IntrospectValue::Int` — the uniform return for the
    /// mutating `invoke` paths.
    fn view_len_value(&self) -> IntrospectValue {
        IntrospectValue::Int(i64::try_from(self.state.view_len()).unwrap_or(i64::MAX))
    }
}

// The shared display-config-proxy `External` skeleton (R847 SSOT): no §5.20
// intent — the sort / filter `Signal` writes already repaint every subscribed
// view, so the widget is never independently dirty.
query_proxy_external_impl!(ViewSortFilterExternal);

impl ExternalIntrospect for ViewSortFilterExternal {
    fn schema(&self) -> IntrospectSchema {
        // `sort_dir` — none/ascending/descending (query + intervene).
        // `filter`   — active category id, or Null (query + intervene).
        // `view_len` — rows passing the filter (query only).
        // `count`    — source row count (query only).
        // `source_at`— `source_at.<pos>` visual→source map (query only).
        // `cycle_sort`/`set_filter`/`send` — invoke channels.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("sort_dir", "string"),
                    SchemaField::new("filter", "int"),
                    SchemaField::new("view_len", "int"),
                    SchemaField::new("count", "int"),
                    SchemaField::parametric(
                        "source_at.<pos>",
                        "int",
                        const { &[SchemaArg::index("pos", "view_len")] },
                    ),
                    SchemaField::action("cycle_sort", "string"),
                    SchemaField::action("set_filter", "int"),
                    SchemaField::action("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // `source_at.<pos>` resolves the visual→source map; an out-of-range
        // position reports Null (present-but-empty), never absence.
        if let Some(rest) = path.strip_prefix("source_at.") {
            return Some(source_at_value(rest, |p| self.state.source_at(p)));
        }
        match path {
            "sort_dir" => Some(IntrospectValue::Text(
                sort_dir_str(self.state.sort()).into(),
            )),
            "filter" => Some(
                self.state
                    .filter()
                    .and_then(|f| i64::try_from(f).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            "view_len" => Some(self.view_len_value()),
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.state.count()).unwrap_or(i64::MAX),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Admin / restore: set the sort from its string form.
            "sort_dir" => match value {
                IntrospectValue::Text(ref s) => {
                    self.apply_set_sort(sort_dir_from_str(s));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // Admin / restore: set (Int) or clear (Null) the filter.
            "filter" => match value {
                IntrospectValue::Int(i) => {
                    self.apply_set_filter(usize::try_from(i).ok());
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.apply_set_filter(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "view_len" | "count" | "source_at" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first sort cycle — returns the resulting sort_dir string.
            "cycle_sort" => {
                self.apply_cycle_sort();
                Ok(IntrospectValue::Text(
                    sort_dir_str(self.state.sort()).into(),
                ))
            }
            // AI-first filter — Int sets the category, Null clears; returns
            // the resulting view_len in one round-trip.
            "set_filter" => match args {
                IntrospectValue::Int(i) => {
                    self.apply_set_filter(usize::try_from(i).ok());
                    Ok(self.view_len_value())
                }
                IntrospectValue::Null => {
                    self.apply_set_filter(None);
                    Ok(self.view_len_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R51.42 §5.35 composite pointer channel: a clicked sort header
            // routes the pointer arc here as `<region>:<EventName>`.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    self.handle_send(payload);
                    Ok(IntrospectValue::Text(
                        sort_dir_str(self.state.sort()).into(),
                    ))
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

    // A 12-row dataset whose source order interleaves 3 categories, so a
    // sort visibly regroups it. Key = "<cat-letter><nn>" (e.g. "A00"),
    // category = i % 3.
    fn keys() -> Vec<String> {
        (0..12)
            .map(|i| format!("{}{i:02}", ["A", "B", "C"][i % 3]))
            .collect()
    }
    fn cats() -> Vec<usize> {
        (0..12).map(|i| i % 3).collect()
    }

    #[test]
    fn cycle_and_dir_strings_round_trip() {
        assert_eq!(cycle_sort(None), Some(true));
        assert_eq!(cycle_sort(Some(true)), Some(false));
        assert_eq!(cycle_sort(Some(false)), None);
        for s in [None, Some(true), Some(false)] {
            assert_eq!(sort_dir_from_str(sort_dir_str(s)), s);
        }
        assert_eq!(sort_dir_from_str("garbage"), None);
    }

    #[test]
    fn unsorted_unfiltered_is_identity() {
        let k = keys();
        let order = compute_order(12, None, |i| k[i].as_str(), |_| true);
        assert_eq!(order, (0..12).collect::<Vec<_>>());
    }

    #[test]
    fn ascending_groups_by_key_stable_on_index() {
        let k = keys();
        // Ascending by "A00,A03,A06,A09,B01,...": all A rows (0,3,6,9) then
        // B (1,4,7,10) then C (2,5,8,11), each ascending by source index.
        let order = compute_order(12, Some(true), |i| k[i].as_str(), |_| true);
        assert_eq!(order, vec![0, 3, 6, 9, 1, 4, 7, 10, 2, 5, 8, 11]);
    }

    #[test]
    fn descending_reverses_keys_but_keeps_index_tiebreak_ascending() {
        let k = keys();
        // Descending by key (high → low): C11,C08,C05,C02 then B10..B01
        // then A09..A00 — i.e. source indices 11,8,5,2,10,7,4,1,9,6,3,0.
        let order = compute_order(12, Some(false), |i| k[i].as_str(), |_| true);
        assert_eq!(order, vec![11, 8, 5, 2, 10, 7, 4, 1, 9, 6, 3, 0]);
    }

    #[test]
    fn filter_keeps_only_matching_category_in_source_order() {
        let c = cats();
        let order = compute_order(12, None, |i| i, |i| c[i] == 1);
        assert_eq!(order, vec![1, 4, 7, 10]);
    }

    #[test]
    fn filter_then_sort_composes() {
        let k = keys();
        let c = cats();
        // Category 2 (C rows) descending: keys C02,C05,C08,C11 → 11,8,5,2.
        let order = compute_order(12, Some(false), |i| k[i].as_str(), |i| c[i] == 2);
        assert_eq!(order, vec![11, 8, 5, 2]);
    }

    fn state() -> Rc<ViewOrderState> {
        Rc::new(ViewOrderState::new(keys(), cats()))
    }

    fn ext() -> ViewSortFilterExternal {
        ViewSortFilterExternal::new(state())
    }

    #[test]
    fn state_starts_unsorted_unfiltered_identity() {
        let s = state();
        assert_eq!(s.count(), 12);
        assert_eq!(s.sort(), None);
        assert_eq!(s.filter(), None);
        assert_eq!(s.view_len(), 12);
        assert_eq!(&*s.order(), &(0..12).collect::<Vec<_>>());
        assert_eq!(s.source_at(0), Some(0));
        assert_eq!(s.source_at(12), None);
    }

    #[test]
    fn cycle_sort_reorders_and_memoizes() {
        let s = state();
        let first = s.order();
        // Cache hit returns the same Rc while the config is unchanged.
        assert!(
            Rc::ptr_eq(&first, &s.order()),
            "order memoized across reads"
        );
        s.cycle_sort(); // ascending
        assert_eq!(s.sort(), Some(true));
        assert_eq!(&*s.order(), &[0, 3, 6, 9, 1, 4, 7, 10, 2, 5, 8, 11]);
        s.cycle_sort(); // descending
        assert_eq!(s.sort(), Some(false));
        assert_eq!(
            s.source_at(0),
            Some(11),
            "descending paints C11 (source 11) first"
        );
        s.cycle_sort(); // back to source order
        assert_eq!(s.sort(), None);
        assert_eq!(&*s.order(), &(0..12).collect::<Vec<_>>());
    }

    #[test]
    fn set_filter_shrinks_view_and_reports_len() {
        let s = state();
        assert_eq!(s.set_filter(Some(0)), 4, "category 0 has 4 rows");
        assert_eq!(s.view_len(), 4);
        assert_eq!(&*s.order(), &[0, 3, 6, 9]);
        assert_eq!(s.set_filter(None), 12);
        assert_eq!(s.view_len(), 12);
    }

    #[test]
    fn external_shares_the_same_state() {
        // The external mutates the shared state; a separately-held handle to
        // the SAME Rc observes the change (the use_view_order sharing).
        let s = state();
        let mut e = ViewSortFilterExternal::new(Rc::clone(&s));
        e.invoke("cycle_sort", IntrospectValue::Null).unwrap();
        assert_eq!(
            s.sort(),
            Some(true),
            "external + view share one source of truth"
        );
    }

    #[test]
    fn query_surfaces_sort_filter_view_len_and_source_map() {
        let e = ext();
        assert_eq!(
            e.query("sort_dir"),
            Some(IntrospectValue::Text("none".into()))
        );
        assert_eq!(e.query("filter"), Some(IntrospectValue::Null));
        assert_eq!(e.query("view_len"), Some(IntrospectValue::Int(12)));
        assert_eq!(e.query("count"), Some(IntrospectValue::Int(12)));
        assert_eq!(e.query("source_at.0"), Some(IntrospectValue::Int(0)));
        assert_eq!(
            e.query("source_at.99"),
            Some(IntrospectValue::Null),
            "out-of-range view position is present-but-empty, not absent",
        );
        assert_eq!(e.query("nope"), None, "undeclared path is genuinely absent");
    }

    #[test]
    fn intervene_sets_sort_and_filter_guards_readonly() {
        let mut e = ext();
        e.intervene("sort_dir", IntrospectValue::Text("descending".into()))
            .expect("sort_dir set");
        assert_eq!(e.state().sort(), Some(false));
        e.intervene("filter", IntrospectValue::Int(2))
            .expect("filter set");
        assert_eq!(e.state().filter(), Some(2));
        assert_eq!(e.state().view_len(), 4);
        e.intervene("filter", IntrospectValue::Null)
            .expect("filter clear");
        assert_eq!(e.state().filter(), None);
        assert_eq!(
            e.intervene("view_len", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            e.intervene("sort_dir", IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch),
        );
        assert_eq!(
            e.intervene("nope", IntrospectValue::Null),
            Err(InterveneError::UnknownPath),
        );
    }

    #[test]
    fn invoke_cycle_sort_set_filter_send_return_outcome() {
        let mut e = ext();
        assert_eq!(
            e.invoke("cycle_sort", IntrospectValue::Null),
            Ok(IntrospectValue::Text("ascending".into())),
        );
        assert_eq!(
            e.invoke("set_filter", IntrospectValue::Int(0)),
            Ok(IntrospectValue::Int(4)),
        );
        assert_eq!(
            e.invoke("set_filter", IntrospectValue::Null),
            Ok(IntrospectValue::Int(12)),
        );
        // Composite send: only the activation edge cycles.
        e.invoke("send", IntrospectValue::Text("cycle:PointerEnter".into()))
            .expect("enter is a no-op");
        assert_eq!(
            e.state().sort(),
            Some(true),
            "hover did not change the sort"
        );
        assert_eq!(
            e.invoke("send", IntrospectValue::Text("cycle:PointerUp".into())),
            Ok(IntrospectValue::Text("descending".into())),
            "release cycles ascending → descending",
        );
        assert_eq!(
            e.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath),
        );
        assert_eq!(
            e.invoke("cycle_sort", IntrospectValue::Null),
            Ok(IntrospectValue::Text("none".into())),
        );
    }

    #[test]
    fn with_undo_records_sort_and_filter_reversibly() {
        Owner::new().run(|| {
            let st = state();
            let stack = Rc::new(UndoStack::new());
            let mut e = ViewSortFilterExternal::new(Rc::clone(&st)).with_undo(Rc::clone(&stack));

            // Each mutation is recorded as one reversible edit.
            assert_eq!(
                e.invoke("cycle_sort", IntrospectValue::Null),
                Ok(IntrospectValue::Text("ascending".into())),
            );
            e.invoke("set_filter", IntrospectValue::Int(1))
                .expect("filter set");
            assert_eq!(st.sort(), Some(true));
            assert_eq!(st.filter(), Some(1));
            assert_eq!(stack.len(), 2, "two recorded edits");

            // Undo unwinds in reverse: filter first, then sort.
            assert!(stack.undo());
            assert_eq!(st.filter(), None, "undo reverts the filter");
            assert_eq!(st.sort(), Some(true), "sort still applied");
            assert!(stack.undo());
            assert_eq!(st.sort(), None, "undo reverts the sort");

            // Redo replays the sort forward.
            assert!(stack.redo());
            assert_eq!(st.sort(), Some(true), "redo re-applies the sort");
        });
    }
}
