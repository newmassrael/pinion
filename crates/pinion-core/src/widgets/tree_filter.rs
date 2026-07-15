//! R821 §5.27 §5.40 §5.50 — **recursive filter view-order proxy for a
//! Model/View tree**.
//!
//! The tree completes the proxy family the Model/View substrate windows
//! over: R747 lands the 1-D list sort/filter proxy
//! ([`view_order`](crate::widgets::view_order)), R778/R783 the data-grid
//! column sort/filter proxy ([`grid_sort`](crate::widgets::grid_sort)), and
//! this module the **hierarchical** filter proxy a tree needs — a clicked /
//! typed search box filters the whole tree, revealing only the paths that
//! lead to a match, **without materializing the off-window rows**. The view
//! windows over the filtered visible-row sequence
//! ([`TreeFilterState::visible_rows`]); the proxy's
//! `query("visible_at.<pos>")` reports the id the view paints at each
//! filtered position — by construction, the same source of truth.
//!
//! This is the canonical proxy-model layer Qt's `QSortFilterProxyModel`
//! (with `setRecursiveFilteringEnabled(true)`) provides for a tree, the
//! hierarchical analogue of WPF `CollectionView` / `TanStack` Table's
//! filtered row model. The *recursion* — path-to-match + auto-expand +
//! sibling renumbering — is the [`flat_visible_filtered`](crate::widgets::tree_nav::flat_visible_filtered) SSOT in
//! [`tree_nav`](crate::widgets::tree_nav); this module is the reactive
//! coordinator + RPC adapter around it, mirroring
//! [`ViewOrderState`](crate::widgets::view_order::ViewOrderState).
//!
//! ## Why a peer of the sort proxies, not a reuse
//!
//! The two sort proxies memoize a visual→source **permutation**
//! (`Vec<usize>`) and address rows by source index; a tree filter changes
//! the visible **set and structure**, so it derives a filtered
//! `Vec<VisibleRow>` flattening and addresses rows by **id** (a sorted flat
//! list has the same rows in a new order; a filtered tree has *different*
//! rows at *different* depths). They genuinely diverge — peers, not one
//! merged type.
//!
//! Crucially they also diverge in **how the derived view is memoized**, and
//! the sort proxies' [`OrderMemo`](crate::widgets::order_memo) is the *wrong*
//! tool here. The sort proxies **own** an immutable materialized source (their
//! key column / cell grid is fixed at construction), so a memo keyed on the
//! `(sort, filter)` config alone is sound — the source can never invalidate
//! it. A tree filter **borrows** the consumer's *mutable* retained tree (via
//! `rows_fn`), so a query-keyed memo would go stale on any tree edit. This
//! proxy therefore derives its rows through a reactive [`Computed`]
//! that tracks the query **and** the tree, recomputing when either changes —
//! the correct memoization for a live source (the scene graph the Phase-D
//! editor mutates).
//!
//! ## One reactive source of truth ([`TreeFilterState`] + [`use_tree_filter`])
//!
//! The active `query` string and the derived filtered rows are held **once**
//! in a reactive [`TreeFilterState`] — the `ScrollState` /
//! [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) pattern
//! this crate shares an interactive axis with. The [`TreeFilterExternal`] is
//! a thin adapter that *mutates* the query on `invoke "set_filter"` /
//! `intervene "query"`; the view and the a11y tree *read* the same state
//! through `use_tree_filter(key)` (the same `Rc`), so reading
//! [`query`](TreeFilterState::query) / [`visible_rows`](TreeFilterState::visible_rows)
//! inside a view-fn auto-subscribes and a filter change repaints exactly
//! like a scroll-offset change.
//!
//! ## The source tree is the caller's ([`Box<dyn Fn>`] recompute)
//!
//! Unlike the sort proxies (which materialize their key column / cell grid),
//! this proxy is **node-type agnostic**: it holds a
//! `Box<dyn Fn(&str) -> Vec<VisibleRow>>` recompute closure that captures the
//! consumer's retained tree `Signal` and applies its own matcher — a
//! `QSortFilterProxyModel` holds a pointer to an abstract `sourceModel()`,
//! not a concrete type. The caller bakes two policies into the closure: the
//! **matcher** (a case-insensitive name search, a kind filter, a glob — like
//! [`compute_order`](crate::widgets::view_order::compute_order)'s `pass`
//! closure) and the **empty-query** fallback (an empty query means "no
//! filter" → the normal expand-respecting
//! [`flat_visible`](crate::widgets::tree_nav::flat_visible), not the
//! fully-expanded accept-all flattening). The proxy owns only the query
//! Signal + the memo, so `pinion-core` carries no node-type generics — the
//! coordinator is one concrete type regardless of the consumer's node type,
//! exactly as `QSortFilterProxyModel` is one class over any `sourceModel()`.

use std::rc::Rc;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaArg, SchemaField,
    ThreadOwnership,
};
use crate::reactive::{Computed, Owner, Signal};
use crate::widgets::tree_nav::VisibleRow;

/// The recompute closure shape: a filtered visible-row flattening for a query
/// string. Captures the consumer's source tree + matcher (see the module
/// note); the proxy calls it only when the query changes (memoized).
pub type TreeRowsFn = Box<dyn Fn(&str) -> Vec<VisibleRow>>;

/// R821 §5.27 §5.50 — the reactive **single source of truth** for a tree's
/// filter view: the active query string + the derived filtered visible-row
/// flattening (memoized on the query).
///
/// The [`ScrollState`](crate::widgets::scroll::ScrollState) of the tree-filter
/// axis: created once via [`use_tree_filter`], it is shared by the
/// [`TreeFilterExternal`] (which mutates the query) and the view / a11y tree
/// (which read [`visible_rows`](Self::visible_rows)) through the same `Rc`.
/// Reading [`query`](Self::query) / [`visible_rows`](Self::visible_rows)
/// inside a view-fn auto-subscribes, so a query change repaints exactly like
/// a scroll-offset change.
pub struct TreeFilterState {
    tag: Option<&'static str>,
    query: Signal<String>,
    /// The filtered flattening as a **reactive derived value**: a
    /// [`Computed`] that auto-tracks *both* the `query` [`Signal`] and the
    /// source-tree `Signal` read inside the consumer's `rows_fn`, so it
    /// recomputes when **either** changes (the textbook reactive memo —
    /// Solid `createMemo`, Leptos `Memo`).
    ///
    /// This is the decisive divergence from the sort proxies'
    /// [`OrderMemo`](crate::widgets::order_memo): they **own** an immutable
    /// materialized source (the key column / cell grid is fixed at
    /// construction), so keying a memo on the `(sort, filter)` config alone is
    /// sound. A tree filter instead **borrows** the consumer's *mutable*
    /// retained tree through `rows_fn`, so a memo keyed on the query alone
    /// would return stale rows the moment the tree is edited (a node added,
    /// removed, or relabelled) without the query changing — exactly the live
    /// scene-graph the Phase-D editor mutates. The `Computed` dependency graph
    /// keeps the filtered view correct (and the repaint subscription complete)
    /// across tree edits, which a value-keyed memo cannot.
    visible: Computed<Rc<Vec<VisibleRow>>>,
}

impl TreeFilterState {
    /// Construct over a recompute closure (see [`TreeRowsFn`]). Starts with an
    /// empty query — the closure's "no filter" fallback decides what an empty
    /// query shows (normally the full expand-respecting tree). The `rows_fn`
    /// is wrapped in a [`Computed`] so every source it reads (the query +
    /// the consumer's tree `Signal`) is tracked.
    #[must_use]
    pub fn new(rows_fn: TreeRowsFn) -> Self {
        let query = Signal::new(String::new());
        let q = query.clone();
        let visible = Computed::new(move || Rc::new(rows_fn(&q.get())));
        Self {
            tag: None,
            query,
            visible,
        }
    }

    /// As [`new`](Self::new) but records the `use_tree_filter` cache key, for
    /// symmetry with [`ScrollState::with_tag`](crate::widgets::scroll::ScrollState::with_tag).
    #[must_use]
    pub fn with_tag(key: &'static str, rows_fn: TreeRowsFn) -> Self {
        Self {
            tag: Some(key),
            ..Self::new(rows_fn)
        }
    }

    /// The `use_tree_filter` cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// The active filter query. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn query(&self) -> String {
        self.query.get()
    }

    /// Whether a filter is active (a non-empty query).
    #[must_use]
    pub fn is_filtering(&self) -> bool {
        !self.query.get().is_empty()
    }

    /// The filtered visible-row flattening for the current query, recomputed
    /// only when the query **or the source tree** changes (the [`Computed`]
    /// memo). Reading it inside a view-fn auto-subscribes to both, so the view
    /// repaints on a filter change *and* on a live tree edit. Cheap `Rc` clone
    /// on a cache hit.
    #[must_use]
    pub fn visible_rows(&self) -> Rc<Vec<VisibleRow>> {
        self.visible.get()
    }

    /// Number of rows in the current filtered view.
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.visible_rows().len()
    }

    /// The node id painted at filtered visual position `pos`, or `None` when
    /// out of range — the tree analogue of the sort proxies'
    /// `source_at(view_pos)` (an id, not a source index, because the filtered
    /// tree is a different row set, not a permutation).
    #[must_use]
    pub fn visible_id_at(&self, pos: usize) -> Option<String> {
        self.visible_rows().get(pos).map(|row| row.id.clone())
    }

    /// The label painted at filtered visual position `pos`, or `None` when out
    /// of range.
    #[must_use]
    pub fn visible_label_at(&self, pos: usize) -> Option<String> {
        self.visible_rows().get(pos).map(|row| row.label.clone())
    }

    /// Set the filter query directly (the clicked / typed / `invoke` path). A
    /// `Signal` write repaints every view that read [`query`](Self::query) /
    /// [`visible_rows`](Self::visible_rows). Returns the resulting
    /// [`visible_count`](Self::visible_count) so the AI-first `set_filter` path
    /// reports the new result size in one round-trip (mirrors
    /// [`ViewOrderState::set_filter`](crate::widgets::view_order::ViewOrderState::set_filter)).
    pub fn set_query(&self, query: impl Into<String>) -> usize {
        self.query.set(query.into());
        self.visible_count()
    }
}

/// R821 §5.27 §5.50 — resolve the shared [`TreeFilterState`] for `key`,
/// building it once via `rows_fn` (the recompute closure capturing the
/// consumer's source tree). Mirrors
/// [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) /
/// [`use_view_order`](crate::widgets::view_order::use_view_order): the
/// `External` and the view both call this with the same `key` and receive the
/// same `Rc`, so the filtered rows are one source of truth.
///
/// Resolve the source tree's `Signal` *before* calling this (pass it captured
/// in `rows_fn`), never inside the factory — the factory runs while
/// [`Owner::cache`](crate::reactive::Owner::cache) holds its slot borrow, so a
/// nested `use_*` cache call inside it panics ([[owner-cache-no-nested-factory]]).
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
#[must_use]
pub fn use_tree_filter(
    key: &'static str,
    rows_fn: impl FnOnce() -> TreeRowsFn,
) -> Rc<TreeFilterState> {
    Owner::current()
        .expect("use_tree_filter requires an active Owner scope")
        .cache(key, || TreeFilterState::with_tag(key, rows_fn()))
}

/// R821 §5.27 §5.50 — the filter **proxy coordinator** External: a thin
/// adapter that surfaces the shared [`TreeFilterState`] to the §5.12
/// `scene/query` / `scene/intervene` / `scene/invoke` paths (the grid/list
/// peer of
/// [`ViewSortFilterExternal`](crate::widgets::view_order::ViewSortFilterExternal)).
///
/// Like [`SpinButtonExternal`](crate::widgets::spin_button) it owns no
/// interaction statechart and emits **no** §5.20 intent — the filter is a
/// display reconfiguration observed through `query` (`query` / `visible_count`
/// / `visible_at.<pos>` / `label_at.<pos>`), and the query `Signal` write
/// already repaints every subscribed view. All state lives in the shared
/// [`TreeFilterState`] — the external holds only the `Rc`, so the view paints
/// the same filtered rows this external reports.
#[derive(Clone)]
pub struct TreeFilterExternal {
    state: Rc<TreeFilterState>,
}

impl core::fmt::Debug for TreeFilterExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TreeFilterExternal")
            .field("query", &self.state.query())
            .field("visible_count", &self.state.visible_count())
            .finish_non_exhaustive()
    }
}

impl TreeFilterExternal {
    /// Wrap the shared [`TreeFilterState`] (from [`use_tree_filter`]).
    #[must_use]
    pub fn new(state: Rc<TreeFilterState>) -> Self {
        Self { state }
    }

    /// The shared state handle (the view reaches the same `Rc` via
    /// [`use_tree_filter`]).
    #[must_use]
    pub fn state(&self) -> &Rc<TreeFilterState> {
        &self.state
    }

    /// `visible_count` as an `IntrospectValue::Int` — the uniform return for
    /// the mutating `set_filter` path.
    fn visible_count_value(&self) -> IntrospectValue {
        IntrospectValue::Int(i64::try_from(self.state.visible_count()).unwrap_or(i64::MAX))
    }
}

impl External for TreeFilterExternal {
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

    // A query/config holder emits no §5.20 intent (see
    // `ViewSortFilterExternal`): the query `Signal` write already repaints
    // every subscribed view, so the widget is never independently dirty.
}

impl ExternalIntrospect for TreeFilterExternal {
    fn schema(&self) -> IntrospectSchema {
        // `query`        — active filter string (query + intervene).
        // `visible_count`— rows in the filtered view (query only).
        // `visible_at`   — `visible_at.<pos>` filtered position → node id.
        // `label_at`     — `label_at.<pos>` filtered position → node label.
        // `set_filter`   — invoke channel (Text sets, Null clears).
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("query", "string"),
                    SchemaField::new("visible_count", "int"),
                    SchemaField::parametric(
                        "visible_at.<pos>",
                        "string",
                        const { &[SchemaArg::index("pos", "visible_count")] },
                    ),
                    SchemaField::parametric(
                        "label_at.<pos>",
                        "string",
                        const { &[SchemaArg::index("pos", "visible_count")] },
                    ),
                    SchemaField::new("set_filter", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // `visible_at.<pos>` / `label_at.<pos>` resolve the filtered visual
        // position; an out-of-range position reports Null (present-but-empty),
        // never absence (the caller has matched the prefix).
        if let Some(rest) = path.strip_prefix("visible_at.") {
            return Some(id_value(rest, |p| self.state.visible_id_at(p)));
        }
        if let Some(rest) = path.strip_prefix("label_at.") {
            return Some(id_value(rest, |p| self.state.visible_label_at(p)));
        }
        match path {
            "query" => Some(IntrospectValue::Text(self.state.query())),
            "visible_count" => Some(self.visible_count_value()),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Admin / restore: set (Text) or clear (Null) the filter query.
            "query" => match value {
                IntrospectValue::Text(s) => {
                    self.state.set_query(s);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.state.set_query(String::new());
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "visible_count" | "visible_at" | "label_at" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first filter: a Text payload sets the query, Null clears.
            // Returns the resulting visible_count in one round-trip, so an AI
            // client learns the new result size without a follow-up query.
            "set_filter" => match args {
                IntrospectValue::Text(s) => {
                    self.state.set_query(s);
                    Ok(self.visible_count_value())
                }
                IntrospectValue::Null => {
                    self.state.set_query(String::new());
                    Ok(self.visible_count_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// Resolve a filtered visual position (`rest`, the part after the matched
/// `visible_at.` / `label_at.` prefix) to its string projection via `lookup`.
/// An out-of-range or unparseable position reports [`IntrospectValue::Null`]
/// (present-but-empty), never absence — the tree analogue of
/// [`source_at_value`](crate::widgets::order_memo) for a `String`-valued
/// projection (the sort proxies' `source_at` is `Int`-valued, hence not shared).
fn id_value(rest: &str, lookup: impl Fn(usize) -> Option<String>) -> IntrospectValue {
    // The tree-filter binding of the shared [`at_index`] projection:
    // look up a `String` at the position, project it as `Text`.
    crate::external::at_index(rest, lookup, IntrospectValue::Text)
}

#[cfg(test)]
mod tests {
    //! R821 §5.27 §5.50 coverage for the tree-filter proxy, restored to
    //! pinion-core at R822. The earlier note exiled these to the
    //! `hello-tree-filter` example on the theory that constructing the proxy
    //! in-crate tripped `clippy::large_stack_arrays` via a serde `Signal`
    //! buffer; that diagnosis was wrong (the lint fired on libtest's own
    //! `[&TestDescAndFn; N]` runner table once the crate passed 2048 tests),
    //! so a `cfg(test)`-scoped crate allow in `lib.rs` lets the proxy
    //! coordinator be tested next to its definition.

    use super::{TreeFilterExternal, TreeFilterState, TreeRowsFn, use_tree_filter};
    use crate::external::{ExternalIntrospect, InterveneError, IntrospectValue, InvokeError};
    use crate::reactive::{Owner, Signal};
    use crate::widgets::tree_nav::{TreeNode, flat_visible, flat_visible_filtered};
    use std::rc::Rc;

    #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct Node {
        id: String,
        label: String,
        expanded: bool,
        children: Vec<Node>,
    }

    impl Node {
        fn leaf(id: &str, label: &str) -> Self {
            Self {
                id: id.to_owned(),
                label: label.to_owned(),
                expanded: false,
                children: Vec::new(),
            }
        }
        fn branch(id: &str, label: &str, children: Vec<Node>) -> Self {
            Self {
                id: id.to_owned(),
                label: label.to_owned(),
                expanded: true,
                children,
            }
        }
    }

    impl TreeNode for Node {
        fn id(&self) -> &str {
            &self.id
        }
        fn label(&self) -> &str {
            &self.label
        }
        fn expanded(&self) -> bool {
            self.expanded
        }
        fn children(&self) -> &[Self] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut [Self] {
            &mut self.children
        }
        fn set_expanded(&mut self, expanded: bool) {
            self.expanded = expanded;
        }
    }

    /// Two expanded groups (6 rows unfiltered); "Ap" matches only `g0`'s two
    /// leaves, "rry" only `g1`'s `Cherry`.
    fn fixture() -> Vec<Node> {
        vec![
            Node::branch(
                "g0",
                "Group0",
                vec![Node::leaf("a0", "Apple"), Node::leaf("a1", "Apricot")],
            ),
            Node::branch(
                "g1",
                "Group1",
                vec![Node::leaf("b0", "Banana"), Node::leaf("b1", "Cherry")],
            ),
        ]
    }

    /// The consumer's recompute closure: an empty query shows the full
    /// expand-respecting tree; otherwise a case-insensitive label substring
    /// drives [`flat_visible_filtered`](crate::widgets::tree_nav::flat_visible_filtered). Captures the source-tree `Signal` so
    /// the `Computed` memo tracks live tree edits.
    fn rows_fn_over(nodes: Signal<Vec<Node>>) -> TreeRowsFn {
        Box::new(move |q: &str| {
            let tree = nodes.get();
            if q.is_empty() {
                flat_visible(&tree)
            } else {
                let needle = q.to_lowercase();
                flat_visible_filtered(&tree, move |n: &Node| {
                    n.label().to_lowercase().contains(&needle)
                })
            }
        })
    }

    #[test]
    fn state_boots_unfiltered_and_set_query_reports_the_new_count() {
        Owner::new().run(|| {
            let state = Rc::new(TreeFilterState::new(rows_fn_over(Signal::new(fixture()))));
            assert_eq!(state.query(), "");
            assert!(!state.is_filtering());
            assert_eq!(
                state.visible_count(),
                6,
                "empty query = full expand-respecting tree"
            );
            // set_query returns the resulting filtered count in one round-trip.
            assert_eq!(
                state.set_query("Ap"),
                3,
                "g0 + its two Ap* leaves; g1 pruned"
            );
            assert!(state.is_filtering());
            assert_eq!(state.visible_id_at(0).as_deref(), Some("g0"));
            assert_eq!(state.visible_label_at(1).as_deref(), Some("Apple"));
            assert_eq!(
                state.visible_id_at(99),
                None,
                "out-of-range filtered position"
            );
            assert_eq!(
                state.set_query(""),
                6,
                "clearing the query restores the full view"
            );
        });
    }

    #[test]
    fn external_query_invoke_intervene_and_guards() {
        Owner::new().run(|| {
            let state = Rc::new(TreeFilterState::new(rows_fn_over(Signal::new(fixture()))));
            let mut ext = TreeFilterExternal::new(Rc::clone(&state));
            // Boot: empty query, full view.
            assert_eq!(
                ext.query("query"),
                Some(IntrospectValue::Text(String::new()))
            );
            assert_eq!(ext.query("visible_count"), Some(IntrospectValue::Int(6)));
            // invoke set_filter returns the resulting visible_count in one trip.
            assert_eq!(
                ext.invoke("set_filter", IntrospectValue::Text("rry".into())),
                Ok(IntrospectValue::Int(2)),
                "g1 + Cherry",
            );
            assert_eq!(
                ext.query("query"),
                Some(IntrospectValue::Text("rry".into()))
            );
            assert_eq!(
                ext.query("visible_at.0"),
                Some(IntrospectValue::Text("g1".into()))
            );
            assert_eq!(
                ext.query("label_at.1"),
                Some(IntrospectValue::Text("Cherry".into()))
            );
            assert_eq!(
                ext.query("visible_at.999"),
                Some(IntrospectValue::Null),
                "out-of-range filtered position is present-but-empty",
            );
            assert_eq!(ext.query("nope"), None, "undeclared path is absent");
            // intervene sets the query (admin / restore); read-only + type guards.
            ext.intervene("query", IntrospectValue::Text("Ap".into()))
                .unwrap();
            assert_eq!(state.query(), "Ap");
            assert_eq!(
                ext.intervene("visible_count", IntrospectValue::Int(1)),
                Err(InterveneError::ReadOnly),
            );
            assert_eq!(
                ext.intervene("query", IntrospectValue::Int(1)),
                Err(InterveneError::TypeMismatch),
            );
            assert_eq!(
                ext.invoke("bogus", IntrospectValue::Null),
                Err(InvokeError::UnknownPath)
            );
            // Null clears via invoke → back to the full view.
            assert_eq!(
                ext.invoke("set_filter", IntrospectValue::Null),
                Ok(IntrospectValue::Int(6))
            );
            assert_eq!(state.query(), "");
        });
    }

    #[test]
    fn filter_re_derives_when_the_source_tree_mutates() {
        // The decisive soundness witness for the R821.1 reactive `Computed`
        // memo: a tree edit under an unchanged active filter must update the
        // filtered view. A memo keyed on the query alone (the sort-proxy
        // pattern, sound only for an owned immutable source) would return
        // stale rows here; the `Computed` tracks the source-tree `Signal`.
        Owner::new().run(|| {
            let nodes = Signal::new(fixture());
            let state = Rc::new(TreeFilterState::new(rows_fn_over(nodes.clone())));
            assert_eq!(state.set_query("Ap"), 3);
            // Append a new matching leaf to g0 WITHOUT touching the query.
            let mut tree = nodes.get();
            tree[0].children.push(Node::leaf("a2", "Applesauce"));
            nodes.set(tree);
            assert_eq!(
                state.visible_count(),
                4,
                "a live tree edit updates the filtered view (Computed tracks the tree, not just the query)",
            );
        });
    }

    #[test]
    fn use_tree_filter_caches_one_shared_state_per_key() {
        Owner::new().run(|| {
            let make = || {
                use_tree_filter("r822-tree-filter-test", || {
                    rows_fn_over(Signal::new(fixture()))
                })
            };
            let a = make();
            let b = make();
            assert!(
                Rc::ptr_eq(&a, &b),
                "the same key resolves to one shared Rc (factory runs once)"
            );
        });
    }
}
