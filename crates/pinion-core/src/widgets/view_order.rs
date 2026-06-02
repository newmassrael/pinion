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
//! - **O(n log n) per recompute, cached.** The proxy recomputes [`order`]
//!   only when the sort or filter changes (not per frame); a stable sort
//!   keeps equal-key rows in source order so re-sorting is deterministic.
//! - **No `Model` trait.** The source is the consumer's per-row key + a
//!   category accessor, not a retained trait object — `compute_order` is a
//!   free function over closures (the second proxy consumer proves the
//!   shape; premature here).

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};

/// R747 §5.40 — cycle a sort key the way a clicked sort header does:
/// unsorted → ascending → descending → unsorted.
///
/// The free-function peer of [`Table::cycle_sort`](crate::widgets::table);
/// shared so the proxy External and any binding agree on the cycle.
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

/// R747 §5.27 §5.40 — the sort/filter **proxy coordinator** External.
///
/// A plain value/config holder (no interaction statechart) like
/// [`SpinButtonExternal`](crate::widgets::spin_button): operability is "set
/// the sort / filter", driven by the AI-first `invoke` paths and the R51.42
/// composite pointer channel (a clicked sort header). It holds the source
/// keys + categories, the current `(sort, filter)`, and the derived
/// [`order`](Self::order) permutation (recomputed only on a config change).
///
/// Unlike a *selection* coordinator it emits **no** §5.20 intent — sort and
/// filter are display reconfigurations observed through `query`
/// (`sort_dir` / `filter` / `view_len` / `source_at.<pos>`), exactly as the
/// R730 table surfaces sort through `query` and reserves the `"selected"`
/// intent for selection. (Category-correct contract: this is a *value*
/// holder, not a selection coordinator.)
#[derive(Debug, Clone)]
pub struct ViewSortFilterExternal {
    /// Per-source-row sort key (the display label). Length is the source
    /// count.
    keys: Vec<String>,
    /// Per-source-row filter category id. Same length as `keys`.
    categories: Vec<usize>,
    /// Active sort: `None` source order / `Some(true)` asc / `Some(false)`
    /// desc.
    sort: Option<bool>,
    /// Active filter category, or `None` for "show all".
    filter: Option<usize>,
    /// Cached visual → source permutation for the current `(sort, filter)`.
    /// Recomputed by [`recompute`](Self::recompute) on every config change.
    order: Vec<usize>,
}

impl ViewSortFilterExternal {
    /// Construct a proxy over a dataset whose source row `i` has sort key
    /// `keys[i]` and filter category `categories[i]`. Starts unsorted,
    /// unfiltered (the view shows the full dataset in source order).
    ///
    /// # Panics
    ///
    /// Panics if `keys` and `categories` differ in length — they are the
    /// two attribute columns of the same dataset and must agree.
    #[must_use]
    pub fn new(keys: Vec<String>, categories: Vec<usize>) -> Self {
        assert_eq!(
            keys.len(),
            categories.len(),
            "keys and categories describe the same rows",
        );
        let order = (0..keys.len()).collect();
        Self { keys, categories, sort: None, filter: None, order }
    }

    /// Source row count (filter-independent).
    #[must_use]
    pub fn count(&self) -> usize {
        self.keys.len()
    }

    /// Active sort state.
    #[must_use]
    pub fn sort(&self) -> Option<bool> {
        self.sort
    }

    /// Active filter category, or `None`.
    #[must_use]
    pub fn filter(&self) -> Option<usize> {
        self.filter
    }

    /// Number of rows in the current view (rows passing the filter).
    #[must_use]
    pub fn view_len(&self) -> usize {
        self.order.len()
    }

    /// The visual → source permutation for the current `(sort, filter)`.
    #[must_use]
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    /// Source data index painted at visual position `view_pos`, or `None`
    /// when out of range (≥ [`view_len`](Self::view_len)).
    #[must_use]
    pub fn source_at(&self, view_pos: usize) -> Option<usize> {
        self.order.get(view_pos).copied()
    }

    /// Recompute [`order`](Self::order) from the current `(sort, filter)`.
    /// The single place the permutation is derived — `key` returns `&str`
    /// so the stable sort never clones a key.
    fn recompute(&mut self) {
        let filter = self.filter;
        self.order = compute_order(
            self.keys.len(),
            self.sort,
            |i| self.keys[i].as_str(),
            |i| filter.is_none_or(|f| self.categories[i] == f),
        );
    }

    /// Cycle the sort (unsorted → asc → desc → unsorted) and recompute.
    /// The clicked-header + `invoke "cycle_sort"` path.
    pub fn cycle_sort(&mut self) {
        self.sort = cycle_sort(self.sort);
        self.recompute();
    }

    /// Set the sort directly (the admin / restore channel) and recompute.
    pub fn set_sort(&mut self, sort: Option<bool>) {
        if sort != self.sort {
            self.sort = sort;
            self.recompute();
        }
    }

    /// Set the filter category (`None` clears) and recompute. Returns the
    /// resulting [`view_len`](Self::view_len).
    pub fn set_filter(&mut self, filter: Option<usize>) -> usize {
        if filter != self.filter {
            self.filter = filter;
            self.recompute();
        }
        self.view_len()
    }

    /// Drive the composite pointer channel: a clicked sort header routes the
    /// pointer arc here; on the activation edge (`PointerUp` /
    /// `KeyboardActivate`) the sort cycles. Every other arc event is a
    /// harmless no-op (no hover/press feedback at the header level).
    fn handle_send(&mut self, payload: &str) {
        let Some((_region, event_name)) =
            crate::composite_tag::parse_send_payload::<String>(payload)
        else {
            return;
        };
        if matches!(event_name, "PointerUp" | "KeyboardActivate") {
            self.cycle_sort();
        }
    }

    /// `view_len` as an `IntrospectValue::Int` — the uniform return for the
    /// mutating `invoke` paths.
    fn view_len_value(&self) -> IntrospectValue {
        IntrospectValue::Int(i64::try_from(self.view_len()).unwrap_or(i64::MAX))
    }
}

impl External for ViewSortFilterExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    // A value/config holder emits no §5.20 intent (see the type doc); the
    // sort / filter value changes only through `invoke` / `intervene`, which
    // the framework already follows with a repaint, so the widget is never
    // independently dirty.
}

impl ExternalIntrospect for ViewSortFilterExternal {
    fn schema(&self) -> IntrospectSchema {
        // `sort_dir` — none/ascending/descending (query + intervene).
        // `filter`   — active category id, or Null (query + intervene).
        // `view_len` — rows passing the filter (query only).
        // `count`    — source row count (query only).
        // `source_at`— `source_at.<pos>` visual→source map (query only).
        // `cycle_sort`/`set_filter`/`send` — invoke channels.
        IntrospectSchema::new(&[
            ("sort_dir", "string"),
            ("filter", "int"),
            ("view_len", "int"),
            ("count", "int"),
            ("source_at", "int"),
            ("cycle_sort", "string"),
            ("set_filter", "int"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // `source_at.<pos>` resolves the visual→source map; an out-of-range
        // position reports Null (present-but-empty), never absence.
        if let Some(rest) = path.strip_prefix("source_at.") {
            let value = rest
                .parse::<usize>()
                .ok()
                .and_then(|p| self.source_at(p))
                .and_then(|src| i64::try_from(src).ok())
                .map_or(IntrospectValue::Null, IntrospectValue::Int);
            return Some(value);
        }
        match path {
            "sort_dir" => Some(IntrospectValue::Text(sort_dir_str(self.sort).into())),
            "filter" => Some(
                self.filter
                    .and_then(|f| i64::try_from(f).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            "view_len" => Some(self.view_len_value()),
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.count()).unwrap_or(i64::MAX),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Admin / restore: set the sort from its string form.
            "sort_dir" => match value {
                IntrospectValue::Text(ref s) => {
                    self.set_sort(sort_dir_from_str(s));
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // Admin / restore: set (Int) or clear (Null) the filter.
            "filter" => match value {
                IntrospectValue::Int(i) => {
                    self.set_filter(usize::try_from(i).ok());
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.set_filter(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "view_len" | "count" | "source_at" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first sort cycle — returns the resulting sort_dir string.
            "cycle_sort" => {
                self.cycle_sort();
                Ok(IntrospectValue::Text(sort_dir_str(self.sort).into()))
            }
            // AI-first filter — Int sets the category, Null clears; returns
            // the resulting view_len so the caller sees the outcome in one
            // round-trip.
            "set_filter" => match args {
                IntrospectValue::Int(i) => {
                    self.set_filter(usize::try_from(i).ok());
                    Ok(self.view_len_value())
                }
                IntrospectValue::Null => {
                    self.set_filter(None);
                    Ok(self.view_len_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R51.42 §5.35 composite pointer channel: a clicked sort header
            // routes the pointer arc here as `<region>:<EventName>`.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    self.handle_send(payload);
                    Ok(IntrospectValue::Text(sort_dir_str(self.sort).into()))
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
        // Within an (impossible here, keys distinct) key tie the source
        // index would stay ascending.
        let order = compute_order(12, Some(false), |i| k[i].as_str(), |_| true);
        assert_eq!(order, vec![11, 8, 5, 2, 10, 7, 4, 1, 9, 6, 3, 0]);
    }

    #[test]
    fn filter_keeps_only_matching_category_in_source_order() {
        let c = cats();
        // Filter category 1 (B rows): source indices 1,4,7,10, unsorted.
        let order = compute_order(12, None, |i| i, |i| c[i] == 1);
        assert_eq!(order, vec![1, 4, 7, 10]);
    }

    #[test]
    fn filter_then_sort_composes() {
        let k = keys();
        let c = cats();
        // Category 2 (C rows) descending: C rows are 2,5,8,11 with keys
        // C02,C05,C08,C11 → descending = 11,8,5,2.
        let order = compute_order(12, Some(false), |i| k[i].as_str(), |i| c[i] == 2);
        assert_eq!(order, vec![11, 8, 5, 2]);
    }

    fn ext() -> ViewSortFilterExternal {
        ViewSortFilterExternal::new(keys(), cats())
    }

    #[test]
    fn new_starts_unsorted_unfiltered_identity() {
        let e = ext();
        assert_eq!(e.count(), 12);
        assert_eq!(e.sort(), None);
        assert_eq!(e.filter(), None);
        assert_eq!(e.view_len(), 12);
        assert_eq!(e.order(), &(0..12).collect::<Vec<_>>()[..]);
        assert_eq!(e.source_at(0), Some(0));
        assert_eq!(e.source_at(12), None);
    }

    #[test]
    fn cycle_sort_reorders_and_recomputes() {
        let mut e = ext();
        e.cycle_sort(); // ascending
        assert_eq!(e.sort(), Some(true));
        assert_eq!(e.order(), &[0, 3, 6, 9, 1, 4, 7, 10, 2, 5, 8, 11][..]);
        e.cycle_sort(); // descending
        assert_eq!(e.sort(), Some(false));
        assert_eq!(e.source_at(0), Some(11), "descending paints C11 (source 11) first");
        e.cycle_sort(); // back to source order
        assert_eq!(e.sort(), None);
        assert_eq!(e.order(), &(0..12).collect::<Vec<_>>()[..]);
    }

    #[test]
    fn set_filter_shrinks_view_and_reports_len() {
        let mut e = ext();
        assert_eq!(e.set_filter(Some(0)), 4, "category 0 has 4 rows");
        assert_eq!(e.view_len(), 4);
        assert_eq!(e.order(), &[0, 3, 6, 9][..]);
        // Clearing restores the full view.
        assert_eq!(e.set_filter(None), 12);
        assert_eq!(e.view_len(), 12);
    }

    #[test]
    fn query_surfaces_sort_filter_view_len_and_source_map() {
        let mut e = ext();
        assert_eq!(e.query("sort_dir"), Some(IntrospectValue::Text("none".into())));
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
        e.cycle_sort();
        assert_eq!(
            e.query("sort_dir"),
            Some(IntrospectValue::Text("ascending".into())),
        );
        assert_eq!(e.query("source_at.0"), Some(IntrospectValue::Int(0)));
        assert_eq!(e.query("source_at.4"), Some(IntrospectValue::Int(1)));
    }

    #[test]
    fn intervene_sets_sort_and_filter_guards_readonly() {
        let mut e = ext();
        e.intervene("sort_dir", IntrospectValue::Text("descending".into()))
            .expect("sort_dir set");
        assert_eq!(e.sort(), Some(false));
        e.intervene("filter", IntrospectValue::Int(2)).expect("filter set");
        assert_eq!(e.filter(), Some(2));
        assert_eq!(e.view_len(), 4);
        e.intervene("filter", IntrospectValue::Null).expect("filter clear");
        assert_eq!(e.filter(), None);
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
        assert_eq!(e.sort(), Some(true), "hover did not change the sort");
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
}
