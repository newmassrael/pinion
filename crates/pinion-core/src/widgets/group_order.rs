//! R843 §5.27 §5.40 — **group-by proxy with collapsible group headers** for a
//! Model/View list.
//!
//! R747 lands the sort/filter [`view-order proxy`](crate::widgets::view_order)
//! and R821 the [`tree filter proxy`](crate::widgets::tree_filter), completing
//! the *flat* and *hierarchical* proxy axes. This module adds the fourth axis
//! every data grid and asset browser needs: a **group-by proxy** that
//! partitions an ordered list of source rows into named groups, each fronted by
//! a **collapsible header** — Qt `QTreeView` with a grouping model, WPF
//! `CollectionView.GroupDescriptions`, a DCC asset browser's "group by type",
//! a code editor's outline "group by symbol kind".
//!
//! ## The flattening ([`group_rows`])
//!
//! The proxy's one SSOT is the **flattening** that turns an `order`
//! permutation + a per-row group key into a single windowable row sequence of
//! [`GroupRow`]s: a `Header` at the start of each group's run, followed by the
//! group's `Data` rows *unless the group is collapsed* (a collapsed group
//! contributes only its header). Groups appear in **first-appearance order**
//! over `order` — the position of a group is the position of its first member —
//! and within a group the members keep their `order` order. This is a stable
//! *partition*, not a run-length pass: every member of a group is gathered
//! under that group's single header even when `order` interleaves groups, so
//! "group by type" gathers all the meshes together regardless of source order.
//!
//! The result is an ordinary `Vec<GroupRow>` the windowing
//! ([`compute_visible_range`](crate::widgets::virtual_list::compute_visible_range))
//! sizes and slices exactly as it does a flat list — group headers and data
//! rows are uniform-pitch rows in one sequence, so a 10 000-row grouped list
//! pays for only the visible window plus its overscan, headers included.
//!
//! ## Composition (orthogonal axes, data-indexed)
//!
//! [`group_rows`] takes the `order` slice, so grouping composes **on top of**
//! any upstream order: pass the identity `0..count` (the [`GroupOrderState`]
//! default) for source order, or a
//! [`ViewOrderState::order`](crate::widgets::view_order::ViewOrderState::order)
//! permutation to group a sorted/filtered view (sort *within* each group).
//! That composition is additive — the free function never assumes identity.
//!
//! A data row carries its **source** index, so selection stays orthogonal: the
//! R746 [`VirtualSelectExternal`](crate::widgets::virtual_select) holds a source
//! index, and [`source_at`](GroupOrderState::source_at) resolves a visual
//! position to that source (or `None` for a header position) — collapsing a
//! group hides its rows without disturbing the selection, which re-appears
//! selected when the group expands (selection ⊥ grouping, both data-indexed,
//! the R747 witness extended).
//!
//! ## One reactive source of truth ([`GroupOrderState`] + [`use_group_order`])
//!
//! The per-group collapse set is held **once** in a reactive
//! [`GroupOrderState`] — the exact `ScrollState` / `ViewOrderState` pattern
//! ([[round-direction-auto-select-no-ask]] notwithstanding, this is the
//! `use_scroll_state` sharing): the [`GroupOrderExternal`] *mutates* the shared
//! collapse set on `invoke` / a clicked header, and the view + a11y tree *read*
//! the same `Rc` through [`use_group_order`], so the flattened rows the view
//! paints are the rows the proxy's `query` reports — by construction. Reading
//! [`rows`](GroupOrderState::rows) inside a view-fn auto-subscribes to the
//! collapse `Signal`, so a collapse/expand repaints exactly as a scroll change
//! does, and the flattening is memoized on the collapse snapshot (recomputed
//! only when a group toggles, never per frame).
//!
//! ## Scope (honest boundaries)
//!
//! - **Dense group ids.** A group id is an index into the consumer's label
//!   table (`0..group_count`), the [`ViewOrderState`] category convention. The
//!   free function [`group_rows`] is general (any `usize` key, first-appearance
//!   order); [`GroupOrderState`] binds the dense-id form its label table needs.
//! - **Identity base order.** [`GroupOrderState`] groups source order; wiring a
//!   sort/filter [`ViewOrderState`] in front (group a *sorted* view) is the
//!   additive composition above, exercised at the free-function level here.
//! - **No group aggregates.** Per-group sum/min/max footers are a later
//!   additive axis (the first consumer groups, it does not aggregate).
//! - **No `Model` trait.** Like every pinion proxy the source is the
//!   consumer's per-row group key, not a retained trait object — the second
//!   proxy consumer proved pinion has no unifying `Model` (R780/R821).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use crate::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError, at_index,
    query_proxy_external_impl,
};
use crate::reactive::{Owner, Signal};
use crate::scene::Scene;
use crate::widgets::scroll::ScrollState;

/// R843 §5.27 §5.40 — one row of the grouped-list flattening: a group header
/// or a data row belonging to the most recent header's group.
///
/// `Copy` (it carries only indices/counts/a flag), so the windowing and
/// introspection paths read rows out of the memoized `Rc<Vec<GroupRow>>` by
/// value without cloning a `String`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GroupRow {
    /// The header at the start of a group's run.
    Header {
        /// The group id (an index into the consumer's label table).
        group: usize,
        /// The number of data rows in this group (over the base order;
        /// independent of the collapse state).
        member_count: usize,
        /// Whether this group is currently collapsed (its data rows are
        /// omitted from the flattening, only this header is present).
        collapsed: bool,
    },
    /// A data row belonging to the most recent [`Header`](Self::Header)'s
    /// group, carrying its **source** data index (the row's identity, the
    /// selection key).
    Data {
        /// Source data index — the selection key, stable across collapse /
        /// regroup (selection ⊥ grouping).
        source: usize,
    },
}

impl GroupRow {
    /// The introspect kind string: `"header"` or `"data"`. The §5.12 wire
    /// discriminator a client reads via `query("kind_at.<pos>")`.
    #[must_use]
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Header { .. } => "header",
            Self::Data { .. } => "data",
        }
    }

    /// The source data index of a [`Data`](Self::Data) row, or `None` for a
    /// header — the visual-position → source map selection composes over.
    #[must_use]
    pub fn source(&self) -> Option<usize> {
        match self {
            Self::Data { source } => Some(*source),
            Self::Header { .. } => None,
        }
    }

    /// R848 §5.35 — this row's composite paint / a11y tag under the two
    /// namespaces: a group header routes to `"{header_prefix}#{group}"` (the
    /// collapse coordinator), a data row to `"{data_prefix}#{source}"` (the
    /// selection coordinator). The SSOT for the **`GroupRow` → tag** mapping the
    /// consumers that hold a `GroupRow` share — the a11y builders tag their nodes
    /// with it and a binding's `access_focus_target` rings the cursor row with
    /// it, so the focus ring and the AT tree address a row identically. (The
    /// paint producers build the same tag from a raw `group` / `source` via the
    /// trivial `format!("{prefix}#{id}")` composite-encode idiom — R803 settled
    /// that encode is an idiom, not a missed abstraction; only a holder of a
    /// `GroupRow` routes through here.)
    #[must_use]
    pub fn composite_tag(&self, header_prefix: &str, data_prefix: &str) -> String {
        match *self {
            Self::Header { group, .. } => format!("{header_prefix}#{group}"),
            Self::Data { source } => format!("{data_prefix}#{source}"),
        }
    }
}

/// R843 §5.27 §5.40 — flatten an `order` permutation into a grouped
/// visible-row sequence (the proxy SSOT).
///
/// Partitions `order` into groups by `group_of` (the group key of a *source*
/// index) in **first-appearance order**, then emits, per group, a
/// [`GroupRow::Header`] followed by the group's [`GroupRow::Data`] rows unless
/// `collapsed` reports that group collapsed (then only the header).
///
/// # Parameters
///
/// - `order` — the base visual→source permutation. Identity `0..count` groups
///   source order; a sort/filter proxy's `order` groups a sorted/filtered view
///   (the members keep `order` order within each group).
/// - `group_of` — the group key (id) of source row `i`. Any `usize`; groups
///   are ordered by first appearance, so the key space may be sparse.
/// - `collapsed` — whether group `g` is collapsed (its data rows are omitted).
///
/// Each source index in `order` is visited once (O(n)); the partition map is a
/// `HashMap` keyed on the group id, but the *output* order is the deterministic
/// first-appearance `group_seq`, never the map's iteration order.
#[must_use]
pub fn group_rows(
    order: &[usize],
    group_of: impl Fn(usize) -> usize,
    collapsed: impl Fn(usize) -> bool,
) -> Vec<GroupRow> {
    // First pass: partition into groups, recording first-appearance order in
    // `group_seq` (the output order) and members per group in `members`.
    let mut group_seq: Vec<usize> = Vec::new();
    let mut slot_of: HashMap<usize, usize> = HashMap::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for &source in order {
        let g = group_of(source);
        let slot = *slot_of.entry(g).or_insert_with(|| {
            group_seq.push(g);
            members.push(Vec::new());
            members.len() - 1
        });
        members[slot].push(source);
    }

    // Second pass: header + (data rows unless collapsed), in first-appearance
    // group order.
    let mut rows: Vec<GroupRow> = Vec::with_capacity(order.len() + group_seq.len());
    for (slot, &group) in group_seq.iter().enumerate() {
        let group_members = &members[slot];
        let is_collapsed = collapsed(group);
        rows.push(GroupRow::Header {
            group,
            member_count: group_members.len(),
            collapsed: is_collapsed,
        });
        if !is_collapsed {
            rows.extend(
                group_members
                    .iter()
                    .map(|&source| GroupRow::Data { source }),
            );
        }
    }
    rows
}

/// A single-entry memo of the [`group_rows`] flattening keyed on the upstream
/// order + the collapse snapshot. The grouped-list peer of
/// [`OrderMemo`](crate::widgets::order_memo); distinct from it (which memoizes
/// a `Vec<usize>` permutation keyed on a `PartialEq` `(sort, filter)` config),
/// because this memoizes a `Vec<GroupRow>` keyed on a *held* `Rc` (compared by
/// identity) — a genuinely different value + key class (R831 distinct ≠
/// redundant), so it lives here rather than widening the shared config memo.
struct GroupRowsMemo {
    /// `(upstream-order Rc, collapse snapshot)`. The flattening changes only
    /// when the upstream `order` is recomputed (a sort/filter proxy mints a
    /// fresh `Rc`, R843.1) or a group toggles. The order `Rc` is **held** here
    /// (not a bare pointer), so while it is the key its allocation cannot be
    /// freed-and-reused: [`Rc::ptr_eq`] is a true identity test, sound *by
    /// construction* over a mutable upstream (R847 — the earlier bare-`as_ptr`
    /// key was sound only by leaning on the upstream's allocate-before-drop
    /// discipline, an uncited cross-module invariant; holding the `Rc` removes
    /// that dependence). This is the reactive-source tracking the `OrderMemo`
    /// doc notes a config-keyed memo cannot do for a live source.
    key: Option<(Rc<Vec<usize>>, BTreeSet<usize>)>,
    rows: Rc<Vec<GroupRow>>,
}

/// R843.1 §5.27 — where a [`GroupOrderState`] reads its base visual→source
/// order: the identity `0..count` (group source order) or an **upstream
/// proxy's** live `order` permutation (group a sorted/filtered view — the
/// canonical filter → sort → group proxy chain).
///
/// The identity arm holds the permutation in a once-built `Rc` so its pointer
/// is stable across reads (the memo hits); the external arm calls the
/// consumer's closure — typically
/// [`ViewOrderState::order`](crate::widgets::view_order::ViewOrderState::order),
/// whose `Rc` is itself stable until the `(sort, filter)` changes. Either way
/// [`rows`](GroupOrderState::rows) keys its memo on the returned `Rc`'s pointer.
enum OrderSource {
    /// Group source order: a once-built identity permutation `0..count`.
    Identity(Rc<Vec<usize>>),
    /// Group an upstream proxy's live order (read fresh, reactively).
    External(Box<dyn Fn() -> Rc<Vec<usize>>>),
}

impl OrderSource {
    /// Resolve the current base order. Reading the external closure
    /// auto-subscribes to the upstream proxy's `Signal`s (so a sort/filter
    /// change upstream repaints the grouped view), exactly as reading the
    /// collapse `Signal` does for the group's own state.
    fn order(&self) -> Rc<Vec<usize>> {
        match self {
            Self::Identity(order) => Rc::clone(order),
            Self::External(f) => f(),
        }
    }
}

/// R843 §5.27 §5.40 — the reactive **single source of truth** for a grouped
/// list's collapse state + flattening.
///
/// Holds the source group keys + the label table (materialized once), the
/// active collapse set as a reactive [`Signal`], and the derived
/// [`GroupRow`] flattening (memoized on the collapse snapshot). Created once
/// via [`use_group_order`], it is shared by the [`GroupOrderExternal`] (which
/// mutates the collapse set) and the view / a11y tree (which read the rows)
/// through the same `Rc`. Reading [`rows`](Self::rows) inside a view-fn
/// auto-subscribes, so a collapse/expand repaints exactly like a scroll change.
pub struct GroupOrderState {
    tag: Option<&'static str>,
    /// Group id of source row `i` (an index into [`group_labels`](Self::group_labels)).
    groups: Vec<usize>,
    /// Visible label per group id.
    group_labels: Vec<String>,
    /// Member count per group id (precomputed; collapse-independent).
    member_counts: Vec<usize>,
    /// Collapsed group ids. Empty = every group expanded.
    collapsed: Signal<BTreeSet<usize>>,
    /// R848 — the roving keyboard cursor: a visual position into [`rows`], or
    /// `None` before the first key. Orthogonal to the selection (cursor ⊥
    /// selection); read inside a view-fn it subscribes, so a cursor move
    /// repaints and the binding's focus ring tracks it.
    cursor: Signal<Option<usize>>,
    /// Where the base visual→source order comes from: identity (group source
    /// order) or an upstream sort/filter proxy (group a sorted/filtered view).
    order_source: OrderSource,
    rows: RefCell<GroupRowsMemo>,
}

impl GroupOrderState {
    /// Construct over a dataset whose source row `i` belongs to group
    /// `groups[i]`, with `group_labels[g]` the visible label of group `g`.
    /// Starts with every group expanded.
    ///
    /// # Panics
    ///
    /// Panics if any `groups[i]` is not a valid index into `group_labels` —
    /// group ids are dense indices into the label table (see the module note).
    #[must_use]
    pub fn new(groups: Vec<usize>, group_labels: Vec<String>) -> Self {
        assert!(
            groups.iter().all(|&g| g < group_labels.len()),
            "every group id must index the label table",
        );
        let mut member_counts = vec![0usize; group_labels.len()];
        for &g in &groups {
            member_counts[g] += 1;
        }
        let identity: Rc<Vec<usize>> = Rc::new((0..groups.len()).collect());
        Self {
            tag: None,
            groups,
            group_labels,
            member_counts,
            collapsed: Signal::new(BTreeSet::new()),
            cursor: Signal::new(None),
            order_source: OrderSource::Identity(identity),
            rows: RefCell::new(GroupRowsMemo {
                key: None,
                rows: Rc::new(Vec::new()),
            }),
        }
    }

    /// Group an **upstream proxy's** live `order` instead of source order: the
    /// builder that turns a standalone grouped list into the third stage of a
    /// filter → sort → group proxy chain. `source` is read fresh on every
    /// [`rows`](Self::rows) recompute (typically `move || view_order.order()`
    /// for an upstream
    /// [`ViewOrderState`](crate::widgets::view_order::ViewOrderState)), so the
    /// grouped view reflects the current sort/filter and repaints when it
    /// changes (reading the closure auto-subscribes upstream). Groups appear in
    /// first-appearance order *over that order*, so sorting reorders the groups
    /// as well as the members within them, and filtering shrinks members (a
    /// group whose every member is filtered out simply has no header).
    ///
    /// The selection (held by source index) is orthogonal to all three stages —
    /// it survives filter, sort and collapse alike.
    ///
    /// # Contract
    ///
    /// `source` must yield source indices `< count()` (it is a permutation /
    /// filtered subset of `0..count`, which every shipped proxy's `order()`
    /// guarantees). [`rows`](Self::rows) indexes the group table by these, so an
    /// out-of-range index fails fast (a `debug_assert` in debug, the slice index
    /// in release).
    #[must_use]
    pub fn with_order_source(mut self, source: impl Fn() -> Rc<Vec<usize>> + 'static) -> Self {
        self.order_source = OrderSource::External(Box::new(source));
        self
    }

    /// As [`new`](Self::new) but records the [`use_group_order`] cache key, for
    /// symmetry with
    /// [`ViewOrderState::with_tag`](crate::widgets::view_order::ViewOrderState::with_tag).
    #[must_use]
    pub fn with_tag(key: &'static str, groups: Vec<usize>, group_labels: Vec<String>) -> Self {
        Self {
            tag: Some(key),
            ..Self::new(groups, group_labels)
        }
    }

    /// The [`use_group_order`] cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// Source row count (collapse-independent).
    #[must_use]
    pub fn count(&self) -> usize {
        self.groups.len()
    }

    /// Number of groups (the label table length).
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.group_labels.len()
    }

    /// The label of group `g`, or `None` for an out-of-range id.
    #[must_use]
    pub fn group_label(&self, group: usize) -> Option<&str> {
        self.group_labels.get(group).map(String::as_str)
    }

    /// The **dataset-total** data-row count of group `g`, or `0` for an
    /// out-of-range id. Collapse- *and* order-source-independent: it counts
    /// every source row in group `g`, not the rows surviving an upstream
    /// filter. Named `total_` to disambiguate from the *currently visible*
    /// (filter-respecting) [`GroupRow::Header::member_count`] the flattening
    /// reports — two genuinely different quantities, one per call site.
    #[must_use]
    pub fn total_member_count(&self, group: usize) -> usize {
        self.member_counts.get(group).copied().unwrap_or(0)
    }

    /// The [`GroupRow`] flattening for the current base order + collapse state,
    /// recomputed only when the upstream order is re-minted or a group toggles
    /// (memoized). Subscribes to the collapse `Signal` and (for an external
    /// order source) the upstream proxy's `Signal`s, so a view that calls this
    /// repaints on a sort/filter/collapse change. Cheap `Rc` clone on a cache
    /// hit.
    ///
    /// # Panics
    ///
    /// Debug-only: panics if the order source yields a source index `>= count()`
    /// (a malformed [`with_order_source`](Self::with_order_source) closure — the
    /// shipped proxies always yield in-range source indices). In release this is
    /// the natural slice index, so a malformed closure still fails fast.
    #[must_use]
    pub fn rows(&self) -> Rc<Vec<GroupRow>> {
        let order = self.order_source.order();
        let collapsed = self.collapsed.get();
        debug_assert!(
            order.iter().all(|&s| s < self.groups.len()),
            "order source yielded a source index out of range (>= count)",
        );
        // Key on the *held* upstream-order `Rc` (identity via `Rc::ptr_eq`): the
        // order is re-minted iff the sort/filter recomputed, so a different `Rc`
        // means recompute even when `collapsed` is unchanged. Holding the `Rc`
        // (not a bare pointer) keeps its allocation alive while keyed, so the
        // identity test is sound by construction over a mutable upstream (R847).
        let mut memo = self.rows.borrow_mut();
        let hit = memo
            .key
            .as_ref()
            .is_some_and(|(o, set)| Rc::ptr_eq(o, &order) && *set == collapsed);
        if !hit {
            let rows = group_rows(&order, |s| self.groups[s], |g| collapsed.contains(&g));
            memo.rows = Rc::new(rows);
            memo.key = Some((order, collapsed));
        }
        Rc::clone(&memo.rows)
    }

    /// Number of visible rows (headers + the data rows of expanded groups).
    #[must_use]
    pub fn visible_len(&self) -> usize {
        self.rows().len()
    }

    /// The [`GroupRow`] at visual position `view_pos`, or `None` when out of
    /// range.
    #[must_use]
    pub fn row_at(&self, view_pos: usize) -> Option<GroupRow> {
        self.rows().get(view_pos).copied()
    }

    /// Source data index painted at visual position `view_pos` — `Some` for a
    /// data row, `None` for a header or an out-of-range position. The
    /// visual→source map selection composes over.
    #[must_use]
    pub fn source_at(&self, view_pos: usize) -> Option<usize> {
        self.row_at(view_pos).and_then(|r| r.source())
    }

    /// Whether group `g` is collapsed. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn is_collapsed(&self, group: usize) -> bool {
        self.collapsed.get().contains(&group)
    }

    /// Collapse / expand group `g`. A no-op for an out-of-range id or a
    /// redundant set (no `Signal` write, so no repaint).
    pub fn set_collapsed(&self, group: usize, collapsed: bool) {
        if group >= self.group_count() {
            return;
        }
        let mut set = self.collapsed.get();
        let changed = if collapsed {
            set.insert(group)
        } else {
            set.remove(&group)
        };
        if changed {
            self.collapsed.set(set);
        }
    }

    /// Flip group `g`'s collapse state (the clicked-header / `invoke
    /// "toggle_group"` path). Returns the resulting collapsed flag; an
    /// out-of-range id is a no-op returning `false`.
    pub fn toggle_group(&self, group: usize) -> bool {
        if group >= self.group_count() {
            return false;
        }
        let mut set = self.collapsed.get();
        let now_collapsed = if set.contains(&group) {
            set.remove(&group);
            false
        } else {
            set.insert(group);
            true
        };
        self.collapsed.set(set);
        now_collapsed
    }

    /// Collapse every group (only headers remain visible). A no-op when already
    /// all collapsed.
    pub fn collapse_all(&self) {
        let all: BTreeSet<usize> = (0..self.group_count()).collect();
        if self.collapsed.get() != all {
            self.collapsed.set(all);
        }
    }

    /// Expand every group. A no-op when already all expanded.
    pub fn expand_all(&self) {
        if !self.collapsed.get().is_empty() {
            self.collapsed.set(BTreeSet::new());
        }
    }

    /// R848 — the roving keyboard cursor: the visual position (into [`rows`])
    /// the keyboard navigation currently addresses, or `None` before the first
    /// key. Distinct from the **selection** (a data row's source index held by
    /// the [`VirtualSelectExternal`](crate::widgets::virtual_select)): the
    /// cursor can rest on a group header (which has no selection) while a data
    /// row stays selected. Read inside a view-fn it subscribes, so a cursor
    /// move repaints (the binding's `access_focus_target` rings the cursor row).
    #[must_use]
    pub fn cursor(&self) -> Option<usize> {
        self.cursor.get()
    }

    /// Move the roving keyboard cursor to visual position `pos`, or clear it
    /// with `None`. Equality-skipped (no repaint on a redundant write). An
    /// out-of-range `pos` is stored as given — a later collapse can shrink the
    /// flattening under a held cursor; [`group_nav`] clamps the cursor it reads
    /// and the focus-ring mapping resolves an out-of-range cursor to no active
    /// descendant. The AI-first wire mirror is the `GroupOrderExternal`'s
    /// `cursor` query / intervene.
    pub fn set_cursor(&self, pos: Option<usize>) {
        self.cursor.set(pos);
    }
}

/// R843 §5.27 §5.40 — resolve the shared [`GroupOrderState`] for `key`,
/// building it once via `data` (the source group keys + label table). Mirrors
/// [`use_view_order`](crate::widgets::view_order::use_view_order): the
/// `External` and the view both call this with the same `key` and receive the
/// same `Rc`, so the flattening is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` /
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
#[must_use]
pub fn use_group_order(
    key: &'static str,
    data: impl FnOnce() -> (Vec<usize>, Vec<String>),
) -> Rc<GroupOrderState> {
    Owner::current()
        .expect("use_group_order requires an active Owner scope")
        .cache(key, || {
            let (groups, labels) = data();
            GroupOrderState::with_tag(key, groups, labels)
        })
}

/// R872 §5.27 §5.40 — [`use_group_order`]'s **upstream-order** sibling: resolve
/// the shared [`GroupOrderState`] for `key`, building it once over an
/// **external** base order ([`with_order_source`](GroupOrderState::with_order_source))
/// instead of the identity. `source` is the live base visual→source permutation
/// — typically `move || upstream.order()` for an upstream sort / filter proxy —
/// read fresh on every [`rows`](GroupOrderState::rows) recompute (so the grouped
/// view reflects the current upstream and repaints when it changes).
///
/// The lifted SSOT for the *cache + `with_tag` + `with_order_source`* boilerplate
/// the filter → sort → group proxy chain repeats: `hello-grouped-sort` (group a
/// `ViewOrderState` order) and `hello-grouped-grid-sort` (group a `GridSortState`
/// order) are its two consumers — the R758 Rule-of-Three the identity
/// [`use_group_order`] already modelled. (`hello-property-grid` was a third
/// consumer through R920; R921 migrated it off this 2-level group-by proxy onto
/// the arbitrary-depth `tree_nav` Tree substrate, because the Inspector's struct
/// properties need a third hierarchy level the flat group-by cannot express.)
///
/// Resolve any upstream proxy **before** calling this (capture its `Rc` in
/// `source`), never inside `source` / `data`, so the two `Owner::cache` calls do
/// not nest ([`owner-cache-no-nested-factory`] R666).
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` /
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
///
/// [`owner-cache-no-nested-factory`]: crate::reactive::Owner::cache
#[must_use]
pub fn use_group_order_with_source(
    key: &'static str,
    data: impl FnOnce() -> (Vec<usize>, Vec<String>),
    source: impl Fn() -> Rc<Vec<usize>> + 'static,
) -> Rc<GroupOrderState> {
    Owner::current()
        .expect("use_group_order_with_source requires an active Owner scope")
        .cache(key, || {
            let (groups, labels) = data();
            GroupOrderState::with_tag(key, groups, labels).with_order_source(source)
        })
}

/// R848 §5.27 — read the roving keyboard cursor's visual position off a
/// [`GroupOrderExternal`]'s introspect handle (the `cursor` query slot), the
/// grouped peer of
/// [`read_selected`](crate::widgets::virtual_select::read_selected). `None` when
/// unset or non-Int. A binding reaches it through the standard
/// `find_external_with_tag(group_tag) → introspect → read_cursor` idiom, the
/// same shape as reading the selection off the primary coordinator.
#[must_use]
pub fn read_cursor(intro: &dyn ExternalIntrospect) -> Option<usize> {
    match intro.query("cursor") {
        Some(IntrospectValue::Int(v)) => usize::try_from(v).ok(),
        _ => None,
    }
}

/// R848 §5.27 — the outcome of one grouped keyboard-navigation key, the
/// pure-policy result [`group_nav`] returns. Two arms because a grouped
/// collapsible collection has exactly two keyboard effects: move the roving
/// cursor (the WAI-ARIA tree/grid Arrow/Home/End roving, selection following
/// onto a data row) or toggle a group's collapse (Right/Left/Enter on a
/// header, the cursor staying put).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupNavOutcome {
    /// Move the roving cursor to this visual position. When the row there is a
    /// data row, the controller selects its source (selection-follows-focus).
    MoveTo(usize),
    /// Collapse / expand this group; the cursor stays on its header.
    Toggle(usize),
}

/// R848 §5.27 — the **keyboard-navigation policy** for a grouped collapsible
/// collection (the [`GroupRow`] flatten): map a key + the current cursor to a
/// [`GroupNavOutcome`], or `None` for a key this widget does not handle (the
/// caller falls through to the shell's unrecognised-key swallow).
///
/// The model is WAI-ARIA's tree / grouped-grid keyboard interaction over the
/// flattened visible rows:
///
/// - **`ArrowDown` / `ArrowUp` / `Home` / `End` / `PageDown` / `PageUp`** —
///   move the cursor over every visible row (headers *and* data), linear clamp
///   (no wrap), reusing the list [`clamp_nav`](crate::widgets::virtual_select::clamp_nav)
///   policy. From no cursor the first key lands on row 0.
/// - **`ArrowRight`** — on a collapsed group header expands it (cursor stays);
///   on an expanded header steps into its first child (the next row).
/// - **`ArrowLeft`** — on an expanded group header collapses it (cursor stays);
///   on a data row jumps up to its group header (the nearest header above).
/// - **`Enter` / `Space`** — on a header toggles it; on a data row re-affirms
///   the selection (a `MoveTo` to the cursor's own position).
///
/// Pure and total over `rows` (an empty flatten yields `None` for every key),
/// so it unit-tests without a scene. The cursor it reads is clamped into range
/// first, so a stale cursor (a collapse shrank the flatten under a held
/// position) still navigates sanely.
#[must_use]
pub fn group_nav(
    rows: &[GroupRow],
    cursor: Option<usize>,
    key: &str,
    page: usize,
) -> Option<GroupNavOutcome> {
    let last = rows.len().checked_sub(1)?;
    // Clamp a possibly-stale cursor into range before reading the row under it.
    let cursor = cursor.map(|c| c.min(last));
    let current = cursor.map(|c| (c, rows[c]));
    match key {
        "ArrowDown" | "ArrowUp" | "Home" | "End" | "PageDown" | "PageUp" => {
            crate::widgets::virtual_select::clamp_nav(cursor, key, rows.len(), page)
                .map(GroupNavOutcome::MoveTo)
        }
        "ArrowRight" => match current {
            // Collapsed header → expand it (cursor stays on the header).
            Some((
                _,
                GroupRow::Header {
                    group,
                    collapsed: true,
                    ..
                },
            )) => Some(GroupNavOutcome::Toggle(group)),
            // Expanded header → step into its first child (the next row).
            Some((
                pos,
                GroupRow::Header {
                    collapsed: false, ..
                },
            )) => Some(GroupNavOutcome::MoveTo((pos + 1).min(last))),
            // Data row (or no cursor) → no horizontal axis at row scope.
            _ => None,
        },
        "ArrowLeft" => match current {
            // Expanded header → collapse it (cursor stays on the header).
            Some((
                _,
                GroupRow::Header {
                    group,
                    collapsed: false,
                    ..
                },
            )) => Some(GroupNavOutcome::Toggle(group)),
            // Data row → jump up to its group header (the nearest header above).
            Some((pos, GroupRow::Data { .. })) => (0..pos)
                .rev()
                .find(|&i| matches!(rows[i], GroupRow::Header { .. }))
                .map(GroupNavOutcome::MoveTo),
            // Collapsed header (or no cursor) → nothing to the left.
            _ => None,
        },
        "Enter" | " " | "Space" => match current {
            Some((_, GroupRow::Header { group, .. })) => Some(GroupNavOutcome::Toggle(group)),
            Some((pos, GroupRow::Data { .. })) => Some(GroupNavOutcome::MoveTo(pos)),
            None => None,
        },
        _ => None,
    }
}

/// R848 §5.27 — drive keyboard navigation over a grouped collapsible
/// collection: the shared `WidgetCore::apply_key` body behind
/// `hello-grouped-list` (a `tree`) and `hello-grouped-grid` (a `grid`). The
/// wiring is byte-identical between them (only the tag, scroll state and row
/// pitch differ), and a divergence — collapsing or revealing differently in one
/// than the other — would be a bug, not a style choice, so it lifts here rather
/// than living twice (the R758 self-grep mandate, the [`nav_select_key`]
/// precedent).
///
/// `groups` is the shared [`GroupOrderState`] (the cursor + collapse SSOT,
/// reached by the binding via `use_group_order`); `tag` is the focusable tab
/// stop **and** the [`VirtualSelectExternal`] anchor (selection-follows-focus
/// routes there). Keys only act when `focused == Some(tag)` (single tab stop).
///
/// On a handled key it applies the [`group_nav`] policy:
/// [`MoveTo`](GroupNavOutcome::MoveTo) writes the cursor and, when the landed
/// row is a data row, selects its source through the coordinator's AI-first
/// `invoke("select", …)` funnel (the same wire a `scene/invoke` drives — keyboard
/// and RPC selection are one funnel), then scrolls the cursor into view;
/// [`Toggle`](GroupNavOutcome::Toggle) flips the group's collapse and keeps the
/// header revealed. Returns `true` exactly when the key was handled.
///
/// [`nav_select_key`]: crate::widgets::virtual_select::nav_select_key
pub fn group_nav_key(
    scene: &mut Scene,
    scroll: &ScrollState,
    groups: &GroupOrderState,
    tag: &str,
    focused: Option<&str>,
    key: &str,
    row_pitch: u32,
) -> bool {
    if focused != Some(tag) {
        return false;
    }
    let page = crate::widgets::virtual_list::page_rows(scroll, row_pitch);
    let rows = groups.rows();
    let Some(outcome) = group_nav(&rows, groups.cursor(), key, page) else {
        return false;
    };
    match outcome {
        GroupNavOutcome::MoveTo(pos) => {
            groups.set_cursor(Some(pos));
            // Selection-follows-focus: landing on a data row selects its source
            // through the coordinator's `invoke` funnel.
            if let Some(source) = rows.get(pos).and_then(GroupRow::source) {
                if let (Some(node), Ok(s)) =
                    (scene.find_external_with_tag_mut(tag), i64::try_from(source))
                {
                    if let Some(intro) = node.handle.introspect_mut() {
                        let _ = intro.invoke("select", IntrospectValue::Int(s));
                    }
                }
            }
            crate::widgets::virtual_list::reveal_row(scroll, pos, row_pitch);
        }
        GroupNavOutcome::Toggle(group) => {
            groups.toggle_group(group);
            // The cursor stays on the toggled header; keep it in view.
            if let Some(pos) = groups.cursor() {
                crate::widgets::virtual_list::reveal_row(scroll, pos, row_pitch);
            }
        }
    }
    true
}

/// R843 §5.27 §5.40 — the group-by **proxy coordinator** External: a thin
/// adapter that surfaces the shared [`GroupOrderState`] to the §5.12
/// `scene/query` / `scene/intervene` / `scene/invoke` paths and the R51.42
/// composite pointer channel (a clicked group header).
///
/// Like [`ViewSortFilterExternal`](crate::widgets::view_order::ViewSortFilterExternal)
/// it owns no interaction statechart and emits **no** §5.20 intent — collapse
/// is a display reconfiguration observed through `query` (`visible_len` /
/// `kind_at` / `collapsed_at` / `collapsed.<group>`), not a selection. All
/// state lives in the shared [`GroupOrderState`] — the external holds only the
/// `Rc`, so the view paints the same flattening this external reports.
#[derive(Clone)]
pub struct GroupOrderExternal {
    state: Rc<GroupOrderState>,
}

impl core::fmt::Debug for GroupOrderExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GroupOrderExternal")
            .field("group_count", &self.state.group_count())
            .field("visible_len", &self.state.visible_len())
            .finish_non_exhaustive()
    }
}

impl GroupOrderExternal {
    /// Wrap the shared [`GroupOrderState`] (from [`use_group_order`]).
    #[must_use]
    pub fn new(state: Rc<GroupOrderState>) -> Self {
        Self { state }
    }

    /// The shared state handle (the view reaches the same `Rc` via
    /// [`use_group_order`]).
    #[must_use]
    pub fn state(&self) -> &Rc<GroupOrderState> {
        &self.state
    }

    /// `visible_len` as an `IntrospectValue::Int` — the uniform return for the
    /// mutating `invoke` paths.
    fn visible_len_value(&self) -> IntrospectValue {
        IntrospectValue::Int(i64::try_from(self.state.visible_len()).unwrap_or(i64::MAX))
    }

    /// Drive the composite pointer channel: a clicked group header, tagged
    /// `{anchor}#{group}`, has its `{group}` sub-tag routed here by the R51.42
    /// router as the `send` payload `"{group}:{EventName}[:{mods}]"`. The first
    /// segment (the group id) toggles that group on the activation edge
    /// (`PointerUp` / `KeyboardActivate`); every other arc event is a no-op.
    fn handle_send(&self, payload: &str) {
        let Some((group, event_name, _modifiers)) =
            crate::composite_tag::parse_send_payload::<usize>(payload)
        else {
            return;
        };
        if crate::input::is_activation_event(event_name) {
            self.state.toggle_group(group);
        }
    }
}

// The shared display-config-proxy `External` skeleton (R847 SSOT): no §5.20
// intent — the collapse `Signal` write already repaints every subscribed view,
// so the widget is never independently dirty.
query_proxy_external_impl!(GroupOrderExternal);

impl ExternalIntrospect for GroupOrderExternal {
    fn schema(&self) -> IntrospectSchema {
        // `visible_len`     — headers + expanded-group data rows (query only).
        // `count`           — source row count (query only).
        // `group_count`     — number of groups (query only).
        // `kind_at`         — `kind_at.<pos>` "header"/"data" (query only).
        // `source_at`       — `source_at.<pos>` data-row source, Null on a
        //                     header (query only).
        // `group_at`        — `group_at.<pos>` header's group id, Null on data.
        // `label_at`        — `label_at.<pos>` header's group label, Null on data.
        // `member_count_at` — `member_count_at.<pos>` header's count, Null on data.
        // `collapsed_at`    — `collapsed_at.<pos>` header's collapse flag, Null on data.
        // `collapsed`       — `collapsed.<group>` per-group flag (query + intervene).
        // `cursor`          — the roving keyboard cursor visual position
        //                     (query + intervene; Null when unset). R848.
        // `toggle_group`/`collapse_all`/`expand_all`/`send` — invoke channels.
        IntrospectSchema::new(&[
            ("visible_len", "int"),
            ("count", "int"),
            ("group_count", "int"),
            ("kind_at", "string"),
            ("source_at", "int"),
            ("group_at", "int"),
            ("label_at", "string"),
            ("member_count_at", "int"),
            ("collapsed_at", "bool"),
            ("collapsed", "bool"),
            ("cursor", "int"),
            ("toggle_group", "int"),
            ("collapse_all", "int"),
            ("expand_all", "int"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let rows = self.state.rows();
        let row_get = |i: usize| rows.get(i).copied();
        if let Some(rest) = path.strip_prefix("kind_at.") {
            return Some(at_index(rest, row_get, |r| {
                IntrospectValue::Text(r.kind_str().into())
            }));
        }
        if let Some(rest) = path.strip_prefix("source_at.") {
            return Some(at_index(rest, row_get, |r| {
                r.source()
                    .and_then(|s| i64::try_from(s).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int)
            }));
        }
        if let Some(rest) = path.strip_prefix("group_at.") {
            return Some(at_index(rest, row_get, |r| match r {
                GroupRow::Header { group, .. } => {
                    i64::try_from(group).map_or(IntrospectValue::Null, IntrospectValue::Int)
                }
                GroupRow::Data { .. } => IntrospectValue::Null,
            }));
        }
        if let Some(rest) = path.strip_prefix("label_at.") {
            return Some(at_index(rest, row_get, |r| match r {
                GroupRow::Header { group, .. } => self
                    .state
                    .group_label(group)
                    .map_or(IntrospectValue::Null, |l| IntrospectValue::Text(l.into())),
                GroupRow::Data { .. } => IntrospectValue::Null,
            }));
        }
        if let Some(rest) = path.strip_prefix("member_count_at.") {
            return Some(at_index(rest, row_get, |r| match r {
                GroupRow::Header { member_count, .. } => {
                    i64::try_from(member_count).map_or(IntrospectValue::Null, IntrospectValue::Int)
                }
                GroupRow::Data { .. } => IntrospectValue::Null,
            }));
        }
        if let Some(rest) = path.strip_prefix("collapsed_at.") {
            return Some(at_index(rest, row_get, |r| match r {
                GroupRow::Header { collapsed, .. } => IntrospectValue::Bool(collapsed),
                GroupRow::Data { .. } => IntrospectValue::Null,
            }));
        }
        if let Some(rest) = path.strip_prefix("collapsed.") {
            // A per-group collapse flag; an out-of-range group is
            // present-but-empty (Null), the §5.12 convention.
            return Some(
                rest.parse::<usize>()
                    .ok()
                    .filter(|&g| g < self.state.group_count())
                    .map_or(IntrospectValue::Null, |g| {
                        IntrospectValue::Bool(self.state.is_collapsed(g))
                    }),
            );
        }
        match path {
            "visible_len" => Some(self.visible_len_value()),
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.state.count()).unwrap_or(i64::MAX),
            )),
            "group_count" => Some(IntrospectValue::Int(
                i64::try_from(self.state.group_count()).unwrap_or(i64::MAX),
            )),
            // The roving keyboard cursor's visual position, Null when unset.
            "cursor" => Some(
                self.state
                    .cursor()
                    .and_then(|c| i64::try_from(c).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        if let Some(rest) = path.strip_prefix("collapsed.") {
            // Admin / restore: set a group's collapse flag directly. A numeric
            // group id is accepted; an out-of-range one is a graceful no-op
            // (`set_collapsed` guards it) — symmetric with the `query`'s
            // present-but-empty `Null` and the `toggle_group` invoke no-op
            // ([[wire-form-read-write-symmetry]]). Only a *non-numeric* suffix
            // is a malformed (unknown) path.
            let Some(group) = rest.parse::<usize>().ok() else {
                return Err(InterveneError::UnknownPath);
            };
            return match value {
                IntrospectValue::Bool(b) => {
                    self.state.set_collapsed(group, b);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            };
        }
        if path == "cursor" {
            // The roving keyboard cursor is read/write (admin / RPC drive): an
            // Int sets the visual position (out-of-range stored as given, the
            // `set_cursor` contract), Null clears it. A negative Int or a
            // non-Int/Null value is a type mismatch.
            return match value {
                IntrospectValue::Int(i) => match usize::try_from(i) {
                    Ok(pos) => {
                        self.state.set_cursor(Some(pos));
                        Ok(())
                    }
                    Err(_) => Err(InterveneError::TypeMismatch),
                },
                IntrospectValue::Null => {
                    self.state.set_cursor(None);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            };
        }
        match path {
            "visible_len" | "count" | "group_count" | "kind_at" | "source_at" | "group_at"
            | "label_at" | "member_count_at" | "collapsed_at" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first toggle — returns the resulting visible_len in one
            // round-trip; an out-of-range / non-Int group is a no-op / mismatch.
            "toggle_group" => match args {
                IntrospectValue::Int(i) => {
                    if let Ok(group) = usize::try_from(i) {
                        self.state.toggle_group(group);
                    }
                    Ok(self.visible_len_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "collapse_all" => {
                self.state.collapse_all();
                Ok(self.visible_len_value())
            }
            "expand_all" => {
                self.state.expand_all();
                Ok(self.visible_len_value())
            }
            // R51.42 §5.35 composite pointer channel: a clicked group header
            // routes the pointer arc here as `<group>:<EventName>`.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    self.handle_send(payload);
                    Ok(self.visible_len_value())
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

    // A 12-row dataset over 3 groups, source order interleaving the groups
    // (group = i % 3), so the partition must *gather* each group's members.
    fn groups() -> Vec<usize> {
        (0..12).map(|i| i % 3).collect()
    }
    fn labels() -> Vec<String> {
        vec!["Alpha".into(), "Bravo".into(), "Charlie".into()]
    }

    #[test]
    fn group_rows_partitions_in_first_appearance_order_and_gathers_members() {
        let g = groups();
        let rows = group_rows(&(0..12).collect::<Vec<_>>(), |s| g[s], |_| false);
        // Groups appear 0,1,2 (sources 0,1,2 are the first of each); each
        // gathers its interleaved members in source order.
        assert_eq!(
            rows,
            vec![
                GroupRow::Header {
                    group: 0,
                    member_count: 4,
                    collapsed: false
                },
                GroupRow::Data { source: 0 },
                GroupRow::Data { source: 3 },
                GroupRow::Data { source: 6 },
                GroupRow::Data { source: 9 },
                GroupRow::Header {
                    group: 1,
                    member_count: 4,
                    collapsed: false
                },
                GroupRow::Data { source: 1 },
                GroupRow::Data { source: 4 },
                GroupRow::Data { source: 7 },
                GroupRow::Data { source: 10 },
                GroupRow::Header {
                    group: 2,
                    member_count: 4,
                    collapsed: false
                },
                GroupRow::Data { source: 2 },
                GroupRow::Data { source: 5 },
                GroupRow::Data { source: 8 },
                GroupRow::Data { source: 11 },
            ],
        );
    }

    #[test]
    fn collapsed_group_contributes_only_its_header() {
        let g = groups();
        // Collapse group 1: its header stays, its 4 data rows vanish.
        let rows = group_rows(&(0..12).collect::<Vec<_>>(), |s| g[s], |grp| grp == 1);
        let kinds: Vec<&str> = rows.iter().map(GroupRow::kind_str).collect();
        assert_eq!(
            kinds,
            [
                "header", "data", "data", "data", "data", "header", "header", "data", "data",
                "data", "data"
            ],
        );
        // The collapsed flag is reported on the header even while collapsed.
        assert_eq!(
            rows[5],
            GroupRow::Header {
                group: 1,
                member_count: 4,
                collapsed: true
            }
        );
    }

    #[test]
    fn group_rows_composes_with_a_non_identity_order() {
        // Group a *sorted* view: the order descends source index, so within
        // each group the members are gathered in that (descending) order, and
        // groups appear by the first descending source of each group.
        let g = groups();
        let order: Vec<usize> = (0..12).rev().collect(); // 11,10,...,0
        let rows = group_rows(&order, |s| g[s], |_| false);
        // Source 11 (group 2) appears first → group 2 leads; its members in
        // order order are 11,8,5,2.
        assert_eq!(
            rows[0],
            GroupRow::Header {
                group: 2,
                member_count: 4,
                collapsed: false
            }
        );
        assert_eq!(rows[1], GroupRow::Data { source: 11 });
        assert_eq!(rows[2], GroupRow::Data { source: 8 });
    }

    #[test]
    fn empty_order_is_empty() {
        let rows = group_rows(&[], |_| 0usize, |_| false);
        assert!(rows.is_empty());
    }

    fn state() -> Rc<GroupOrderState> {
        Rc::new(GroupOrderState::new(groups(), labels()))
    }

    #[test]
    fn state_boots_all_expanded() {
        let s = state();
        assert_eq!(s.count(), 12);
        assert_eq!(s.group_count(), 3);
        assert_eq!(s.total_member_count(0), 4);
        assert_eq!(
            s.total_member_count(99),
            0,
            "out-of-range group has no members"
        );
        assert_eq!(s.group_label(1), Some("Bravo"));
        assert_eq!(s.group_label(99), None);
        // 3 headers + 12 data rows.
        assert_eq!(s.visible_len(), 15);
        assert!(!s.is_collapsed(0));
        // First row is group 0's header; second is its first member (source 0).
        assert_eq!(
            s.row_at(0),
            Some(GroupRow::Header {
                group: 0,
                member_count: 4,
                collapsed: false
            })
        );
        assert_eq!(s.source_at(0), None, "a header has no source");
        assert_eq!(s.source_at(1), Some(0));
    }

    #[test]
    fn toggle_group_shrinks_and_restores_visible_len_and_memoizes() {
        let s = state();
        let first = s.rows();
        assert!(Rc::ptr_eq(&first, &s.rows()), "rows memoized across reads");
        assert!(
            s.toggle_group(1),
            "toggle collapses group 1 (returns now-collapsed = true)"
        );
        assert!(s.is_collapsed(1));
        // Group 1's 4 data rows vanish; 15 - 4 = 11.
        assert_eq!(s.visible_len(), 11);
        // A new collapse snapshot recomputes (different Rc).
        assert!(!Rc::ptr_eq(&first, &s.rows()));
        assert!(
            !s.toggle_group(1),
            "toggle expands group 1 again (returns now-collapsed = false)"
        );
        assert_eq!(s.visible_len(), 15);
    }

    #[test]
    fn collapse_all_and_expand_all() {
        let s = state();
        s.collapse_all();
        assert_eq!(s.visible_len(), 3, "only the 3 headers remain");
        for g in 0..3 {
            assert!(s.is_collapsed(g));
        }
        s.expand_all();
        assert_eq!(s.visible_len(), 15);
        assert!(!s.is_collapsed(0));
    }

    #[test]
    fn set_collapsed_guards_out_of_range_and_is_idempotent() {
        let s = state();
        s.set_collapsed(99, true); // out of range — no-op
        assert_eq!(s.visible_len(), 15);
        s.set_collapsed(0, true);
        assert_eq!(s.visible_len(), 11);
        s.set_collapsed(0, true); // redundant — still collapsed
        assert_eq!(s.visible_len(), 11);
        s.set_collapsed(0, false);
        assert_eq!(s.visible_len(), 15);
    }

    fn ext() -> GroupOrderExternal {
        GroupOrderExternal::new(state())
    }

    #[test]
    fn query_surfaces_structure_and_per_position_axes() {
        let e = ext();
        assert_eq!(e.query("visible_len"), Some(IntrospectValue::Int(15)));
        assert_eq!(e.query("count"), Some(IntrospectValue::Int(12)));
        assert_eq!(e.query("group_count"), Some(IntrospectValue::Int(3)));
        // pos 0 = group 0 header.
        assert_eq!(
            e.query("kind_at.0"),
            Some(IntrospectValue::Text("header".into()))
        );
        assert_eq!(e.query("group_at.0"), Some(IntrospectValue::Int(0)));
        assert_eq!(
            e.query("label_at.0"),
            Some(IntrospectValue::Text("Alpha".into()))
        );
        assert_eq!(e.query("member_count_at.0"), Some(IntrospectValue::Int(4)));
        assert_eq!(
            e.query("collapsed_at.0"),
            Some(IntrospectValue::Bool(false))
        );
        assert_eq!(
            e.query("source_at.0"),
            Some(IntrospectValue::Null),
            "a header has no source"
        );
        // pos 1 = group 0's first data row (source 0).
        assert_eq!(
            e.query("kind_at.1"),
            Some(IntrospectValue::Text("data".into()))
        );
        assert_eq!(e.query("source_at.1"), Some(IntrospectValue::Int(0)));
        assert_eq!(
            e.query("group_at.1"),
            Some(IntrospectValue::Null),
            "a data row reports no group id"
        );
        assert_eq!(e.query("label_at.1"), Some(IntrospectValue::Null));
        // Per-group collapse flag.
        assert_eq!(e.query("collapsed.0"), Some(IntrospectValue::Bool(false)));
        assert_eq!(
            e.query("collapsed.99"),
            Some(IntrospectValue::Null),
            "out-of-range group → Null"
        );
        // Out-of-range position is present-but-empty; undeclared path is absent.
        assert_eq!(e.query("kind_at.999"), Some(IntrospectValue::Null));
        assert_eq!(e.query("nope"), None);
    }

    #[test]
    fn invoke_toggle_collapse_expand_return_visible_len() {
        let mut e = ext();
        assert_eq!(
            e.invoke("toggle_group", IntrospectValue::Int(0)),
            Ok(IntrospectValue::Int(11))
        );
        assert!(e.state().is_collapsed(0));
        assert_eq!(
            e.invoke("collapse_all", IntrospectValue::Null),
            Ok(IntrospectValue::Int(3))
        );
        assert_eq!(
            e.invoke("expand_all", IntrospectValue::Null),
            Ok(IntrospectValue::Int(15))
        );
        // An out-of-range group toggle is a harmless no-op.
        assert_eq!(
            e.invoke("toggle_group", IntrospectValue::Int(99)),
            Ok(IntrospectValue::Int(15))
        );
        assert_eq!(
            e.invoke("toggle_group", IntrospectValue::Null),
            Err(InvokeError::TypeMismatch)
        );
        assert_eq!(
            e.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn invoke_send_toggles_on_activation_edge_only() {
        let mut e = ext();
        // Hover (PointerEnter) over group 1's header does not toggle.
        e.invoke("send", IntrospectValue::Text("1:PointerEnter".into()))
            .expect("enter is a no-op");
        assert!(
            !e.state().is_collapsed(1),
            "hover did not collapse the group"
        );
        // Release (PointerUp) toggles group 1.
        assert_eq!(
            e.invoke("send", IntrospectValue::Text("1:PointerUp".into())),
            Ok(IntrospectValue::Int(11)),
            "release collapses group 1 → visible_len 11",
        );
        assert!(e.state().is_collapsed(1));
    }

    #[test]
    fn intervene_sets_collapse_and_guards_readonly() {
        let mut e = ext();
        e.intervene("collapsed.2", IntrospectValue::Bool(true))
            .expect("collapse group 2");
        assert!(e.state().is_collapsed(2));
        assert_eq!(e.state().visible_len(), 11);
        e.intervene("collapsed.2", IntrospectValue::Bool(false))
            .expect("expand group 2");
        assert!(!e.state().is_collapsed(2));
        assert_eq!(
            e.intervene("collapsed.2", IntrospectValue::Int(1)),
            Err(InterveneError::TypeMismatch),
        );
        // R847 wire-symmetry: an out-of-range *numeric* group is a graceful
        // no-op (mirrors query -> Null and toggle_group invoke no-op), not an
        // UnknownPath — only a non-numeric suffix is a malformed path.
        e.intervene("collapsed.99", IntrospectValue::Bool(true))
            .expect("out-of-range group is a graceful no-op");
        assert_eq!(
            e.state().visible_len(),
            15,
            "out-of-range collapse changed nothing"
        );
        assert_eq!(
            e.intervene("collapsed.notanumber", IntrospectValue::Bool(true)),
            Err(InterveneError::UnknownPath),
            "a non-numeric group suffix is a malformed path",
        );
        assert_eq!(
            e.intervene("visible_len", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            e.intervene("source_at", IntrospectValue::Null),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            e.intervene("nope", IntrospectValue::Null),
            Err(InterveneError::UnknownPath)
        );
    }

    #[test]
    fn external_shares_the_same_state() {
        let s = state();
        let mut e = GroupOrderExternal::new(Rc::clone(&s));
        e.invoke("toggle_group", IntrospectValue::Int(0)).unwrap();
        assert!(
            s.is_collapsed(0),
            "external + view share one source of truth"
        );
    }

    // ── R844: group over an upstream proxy's order (filter → sort → group) ──

    #[test]
    fn with_order_source_groups_over_the_supplied_order() {
        // Group the 3-group data over a REVERSED order: groups appear by the
        // first (highest) source of each group, members within in that order.
        let order = Rc::new((0..12).rev().collect::<Vec<usize>>()); // 11,10,..,0
        let s =
            GroupOrderState::new(groups(), labels()).with_order_source(move || Rc::clone(&order));
        let rows = s.rows();
        // Source 11 is group 2 (11 % 3) → Charlie leads; members 11,8,5,2.
        assert_eq!(
            rows[0],
            GroupRow::Header {
                group: 2,
                member_count: 4,
                collapsed: false
            }
        );
        assert_eq!(rows[1], GroupRow::Data { source: 11 });
        assert_eq!(rows[2], GroupRow::Data { source: 8 });
        assert_eq!(
            s.visible_len(),
            15,
            "same rows, regrouped: 3 headers + 12 data"
        );
    }

    #[test]
    fn external_order_memo_recomputes_when_the_upstream_order_is_reminted() {
        // A live upstream order: the same Rc → memo hit; a re-minted Rc (what a
        // sort/filter change produces) → recompute, even though collapse is
        // unchanged. This is the soundness the pointer-keyed memo buys over a
        // mutable upstream (the OrderMemo doc's config-keyed-memo caveat).
        let cell: Rc<RefCell<Rc<Vec<usize>>>> = Rc::new(RefCell::new(Rc::new((0..12).collect())));
        let c2 = Rc::clone(&cell);
        let s = GroupOrderState::new(groups(), labels())
            .with_order_source(move || Rc::clone(&c2.borrow()));
        let first = s.rows();
        assert!(
            Rc::ptr_eq(&first, &s.rows()),
            "unchanged upstream order Rc → memo hit"
        );
        *cell.borrow_mut() = Rc::new((0..12).rev().collect());
        let second = s.rows();
        assert!(
            !Rc::ptr_eq(&first, &second),
            "re-minted upstream order → recompute"
        );
        assert_eq!(
            second[1],
            GroupRow::Data { source: 11 },
            "regrouped over the new order"
        );
    }

    #[test]
    fn external_filtered_order_drops_groups_with_no_surviving_members() {
        // An upstream filter keeping only group-1 sources (1,4,7,10): the
        // grouped view has exactly one header (Bravo) and its 4 members; the
        // emptied groups have no header at all.
        let filtered = Rc::new(vec![1usize, 4, 7, 10]);
        let s = GroupOrderState::new(groups(), labels())
            .with_order_source(move || Rc::clone(&filtered));
        let rows = s.rows();
        assert_eq!(
            rows[0],
            GroupRow::Header {
                group: 1,
                member_count: 4,
                collapsed: false
            }
        );
        assert_eq!(
            s.visible_len(),
            5,
            "one header + 4 members; emptied groups vanish"
        );
        // The dataset-total member_count is order-source-independent.
        assert_eq!(
            s.total_member_count(0),
            4,
            "member_count is the dataset total, not the filtered count"
        );
    }

    // ── R848: roving keyboard cursor + grouped navigation policy ──

    /// The 15-row flatten of the 12-row / 3-group dataset, nothing collapsed:
    /// pos 0 = group-0 header, pos 1–4 its data; pos 5 = group-1 header, 6–9
    /// its data; pos 10 = group-2 header, 11–14 its data.
    fn flat() -> Vec<GroupRow> {
        group_rows(&(0..12).collect::<Vec<_>>(), |s| s % 3, |_| false)
    }

    #[test]
    fn nav_arrows_home_end_rove_over_every_visible_row() {
        let rows = flat();
        // From no cursor the first key lands on row 0 (a header).
        assert_eq!(
            group_nav(&rows, None, "ArrowDown", 5),
            Some(GroupNavOutcome::MoveTo(0))
        );
        // Step down through header → data; clamp at the ends.
        assert_eq!(
            group_nav(&rows, Some(0), "ArrowDown", 5),
            Some(GroupNavOutcome::MoveTo(1))
        );
        assert_eq!(
            group_nav(&rows, Some(1), "ArrowUp", 5),
            Some(GroupNavOutcome::MoveTo(0))
        );
        assert_eq!(
            group_nav(&rows, Some(0), "ArrowUp", 5),
            Some(GroupNavOutcome::MoveTo(0)),
            "clamp at top"
        );
        assert_eq!(
            group_nav(&rows, Some(14), "ArrowDown", 5),
            Some(GroupNavOutcome::MoveTo(14)),
            "clamp at bottom"
        );
        assert_eq!(
            group_nav(&rows, Some(7), "Home", 5),
            Some(GroupNavOutcome::MoveTo(0))
        );
        assert_eq!(
            group_nav(&rows, Some(0), "End", 5),
            Some(GroupNavOutcome::MoveTo(14))
        );
        // PageDown advances by the page size, clamped.
        assert_eq!(
            group_nav(&rows, Some(1), "PageDown", 5),
            Some(GroupNavOutcome::MoveTo(6))
        );
        assert_eq!(
            group_nav(&rows, Some(12), "PageDown", 5),
            Some(GroupNavOutcome::MoveTo(14))
        );
    }

    #[test]
    fn nav_right_expands_or_enters_a_header_left_collapses_or_climbs() {
        let rows = flat();
        // Right on an expanded header steps into its first child (next row).
        assert_eq!(
            group_nav(&rows, Some(0), "ArrowRight", 5),
            Some(GroupNavOutcome::MoveTo(1))
        );
        // Left on an expanded header collapses it (cursor stays).
        assert_eq!(
            group_nav(&rows, Some(0), "ArrowLeft", 5),
            Some(GroupNavOutcome::Toggle(0))
        );
        // Left on a data row climbs to its group header.
        assert_eq!(
            group_nav(&rows, Some(3), "ArrowLeft", 5),
            Some(GroupNavOutcome::MoveTo(0)),
            "data climbs to header"
        );
        assert_eq!(
            group_nav(&rows, Some(8), "ArrowLeft", 5),
            Some(GroupNavOutcome::MoveTo(5)),
            "group-1 data climbs to group-1 header"
        );
        // Right on a data row has no row-scope horizontal axis.
        assert_eq!(group_nav(&rows, Some(3), "ArrowRight", 5), None);
    }

    #[test]
    fn nav_right_expands_a_collapsed_header_left_on_it_is_inert() {
        // Group 0 collapsed: pos 0 = collapsed header, pos 1 = group-1 header, …
        let rows = group_rows(&(0..12).collect::<Vec<_>>(), |s| s % 3, |g| g == 0);
        assert!(matches!(
            rows[0],
            GroupRow::Header {
                group: 0,
                collapsed: true,
                ..
            }
        ));
        assert_eq!(
            group_nav(&rows, Some(0), "ArrowRight", 5),
            Some(GroupNavOutcome::Toggle(0)),
            "right expands a collapsed header"
        );
        assert_eq!(
            group_nav(&rows, Some(0), "ArrowLeft", 5),
            None,
            "left on a collapsed header is inert"
        );
    }

    #[test]
    fn nav_enter_space_toggle_headers_and_reaffirm_data() {
        let rows = flat();
        assert_eq!(
            group_nav(&rows, Some(0), "Enter", 5),
            Some(GroupNavOutcome::Toggle(0))
        );
        assert_eq!(
            group_nav(&rows, Some(5), " ", 5),
            Some(GroupNavOutcome::Toggle(1)),
            "Space toggles a header"
        );
        assert_eq!(
            group_nav(&rows, Some(2), "Enter", 5),
            Some(GroupNavOutcome::MoveTo(2)),
            "Enter on data re-affirms"
        );
    }

    #[test]
    fn nav_clamps_a_stale_cursor_and_ignores_unknown_keys_and_empty_rows() {
        let rows = flat();
        // A cursor left past the end by an external collapse still navigates
        // sanely: clamp into range (14), then up → 13.
        assert_eq!(
            group_nav(&rows, Some(99), "ArrowUp", 5),
            Some(GroupNavOutcome::MoveTo(13))
        );
        // An unhandled key falls through.
        assert_eq!(group_nav(&rows, Some(0), "Tab", 5), None);
        // Empty flatten yields None for every key.
        assert_eq!(group_nav(&[], None, "ArrowDown", 5), None);
        assert_eq!(group_nav(&[], Some(0), "Enter", 5), None);
    }

    #[test]
    fn cursor_defaults_none_and_round_trips() {
        let s = state();
        assert_eq!(s.cursor(), None);
        s.set_cursor(Some(3));
        assert_eq!(s.cursor(), Some(3));
        s.set_cursor(None);
        assert_eq!(s.cursor(), None);
    }

    #[test]
    fn external_cursor_query_and_intervene_round_trip() {
        let mut e = ext();
        assert_eq!(
            e.query("cursor"),
            Some(IntrospectValue::Null),
            "cursor unset at boot"
        );
        e.intervene("cursor", IntrospectValue::Int(4))
            .expect("set cursor");
        assert_eq!(e.query("cursor"), Some(IntrospectValue::Int(4)));
        assert_eq!(
            e.state().cursor(),
            Some(4),
            "external + view share the cursor"
        );
        e.intervene("cursor", IntrospectValue::Null)
            .expect("clear cursor");
        assert_eq!(e.query("cursor"), Some(IntrospectValue::Null));
        // A negative Int or a non-Int/Null value is a type mismatch, not a clear.
        assert_eq!(
            e.intervene("cursor", IntrospectValue::Int(-1)),
            Err(InterveneError::TypeMismatch)
        );
        assert_eq!(
            e.intervene("cursor", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn composite_tag_maps_header_and_data_rows() {
        let rows = flat();
        // The SSOT row → tag mapping the a11y builders + access_focus_target share.
        assert_eq!(
            rows[0].composite_tag("grp", "row"),
            "grp#0",
            "header → {{header}}#{{group}}"
        );
        assert_eq!(
            rows[1].composite_tag("grp", "row"),
            "row#0",
            "data → {{data}}#{{source}}"
        );
        assert_eq!(
            rows[5].composite_tag("grp", "row"),
            "grp#1",
            "group-1 header"
        );
    }

    #[test]
    fn read_cursor_reads_the_external_cursor_slot() {
        let mut e = ext();
        assert_eq!(read_cursor(&e), None, "unset cursor reads None");
        e.intervene("cursor", IntrospectValue::Int(5))
            .expect("set cursor");
        assert_eq!(
            read_cursor(&e),
            Some(5),
            "read_cursor mirrors the cursor query slot"
        );
    }
}
