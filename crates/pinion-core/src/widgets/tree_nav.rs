//! R811 §5.50 §5.27 — backend-agnostic WAI-ARIA APG 6.13 Tree keyboard
//! navigation substrate, lifted from `hello-tree-view` (R809/R809.1) at
//! its second interactive-tree consumer: the `DevTools` inspector tree in
//! `hello-dock-panels`.
//!
//! The model is the standard one for tree keyboard navigation: flatten
//! the *visible* tree into a depth-first preorder row sequence
//! ([`flat_visible`]), then resolve each key as index + depth arithmetic
//! over that one flattening ([`resolve_tree_key`]). The keyboard cursor
//! only ever sits on a visible row, so no key needs a separate tree
//! search — every axis reads the same flattened sequence the paint walk
//! and the AT tree also consume (the R809.1 single-traversal invariant,
//! now shared across consumers rather than re-derived per example).
//!
//! Vertical motion (Up / Down / Home / End / Page) delegates to the
//! shared [`clamp_nav`] SSOT — clamp,
//! not wrap, because a tree has ends.
//!
//! The resolver is **pure**: it returns a [`TreeKey`] outcome the
//! consumer applies to its own expand-state store, so the substrate is
//! independent of how each consumer retains its tree. `hello-tree-view`
//! owns a `Vec<FileNode>` whose nodes carry `expanded` directly; the
//! `DevTools` inspector projects a fresh tree from the live scene every
//! frame and overlays an external collapse set. Both drive the same
//! resolver because both expose their tree through the [`TreeNode`]
//! trait.

use std::rc::Rc;

use super::scroll::ScrollState;
use super::virtual_list::scroll_offset_to_reveal;
use super::virtual_select::clamp_nav;
use crate::Signal;
use crate::external::{
    IntrospectSchema, IntrospectValue, QueryOnlyIntrospect, QuerySource, SchemaArg, SchemaField,
    at_index,
};
use crate::reactive::batch;
use crate::widget_core::ExtraExternal;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A node in a navigable tree. The substrate reads only the four facets
/// WAI-ARIA tree navigation needs — a stable id, the visible label,
/// the expand state, and the child nodes — so any retained or derived
/// tree can drive [`flat_visible`] / [`resolve_tree_key`] by
/// implementing it. Both shipped consumers paint through
/// `pinion_widget_paint::tree_view::TreeItem` (which implements this);
/// `hello-tree-view`'s `FileNode` implements it too.
pub trait TreeNode {
    /// Stable node id — the navigation cursor key and the composite-tag
    /// suffix the row click router parses.
    fn id(&self) -> &str;
    /// Visible label — the type-ahead search key and the AT accessible
    /// name.
    fn label(&self) -> &str;
    /// Whether this branch is currently expanded (its children are part
    /// of the visible row sequence).
    fn expanded(&self) -> bool;
    /// Child nodes; empty for a leaf.
    fn children(&self) -> &[Self]
    where
        Self: Sized;
    /// Mutable child access, so the R820 flag-storage helpers
    /// ([`find_node_mut`] / [`set_expanded_in`] / [`toggle_expanded`]) can
    /// descend and mutate a retained `Vec<Self>` tree in place. A node
    /// whose tree is *derived* (the inspector projects a fresh tree each
    /// frame and never calls these) may return an empty slice.
    fn children_mut(&mut self) -> &mut [Self]
    where
        Self: Sized;
    /// Set this node's expanded flag — the write side of [`expanded`] the
    /// R820 [`set_expanded_in`] / [`toggle_expanded`] mutators go through.
    ///
    /// [`expanded`]: TreeNode::expanded
    fn set_expanded(&mut self, expanded: bool);
}

/// R935 §5.51 — the **structural-mutation** peer of [`TreeNode`]. Where
/// [`TreeNode`] exposes children as slices (read + flag mutation — which every
/// tree, including a *derived* one the inspector rebuilds each frame, can
/// provide), this exposes the child list as an owned `&mut Vec<Self>` so the
/// subtree editors below ([`remove_subtree`] / [`insert_subtree`]) can
/// add and remove nodes. Only a **retained**, `Vec`-backed tree implements it;
/// a derived projection does not (and never reparents).
pub trait MutableTreeNode: TreeNode + Sized {
    /// The owned child list, for structural insert / remove.
    fn children_vec_mut(&mut self) -> &mut Vec<Self>;
}

/// R935 §5.51 — remove the subtree rooted at `id` (depth-first), returning it
/// (`None` when `id` is absent). The structural inverse of [`insert_subtree`];
/// together they are the move/delete/undo backbone every retained tree shares
/// (lifted from `hello-tree-grid` when the `hello-tree-reparent` drag editor
/// became the second consumer — a `Vec::remove`/`insert` at the wrong depth
/// silently corrupts the tree, so it is a divergence-is-a-bug duplication).
pub fn remove_subtree<N: MutableTreeNode>(nodes: &mut Vec<N>, id: &str) -> Option<N> {
    if let Some(pos) = nodes.iter().position(|n| n.id() == id) {
        return Some(nodes.remove(pos));
    }
    for n in nodes.iter_mut() {
        if let Some(found) = remove_subtree(n.children_vec_mut(), id) {
            return Some(found);
        }
    }
    None
}

/// R935 §5.51 — insert `node` under `parent` (`None` = root) at `index`
/// (clamped to the sibling count). The structural inverse of
/// [`remove_subtree`]. A `parent` id that is absent is a broken caller
/// invariant (the caller resolved it from the live tree): it fires a
/// `debug_assert` and falls back to appending at root in release so nothing is
/// lost.
pub fn insert_subtree<N: MutableTreeNode>(
    nodes: &mut Vec<N>,
    parent: Option<&str>,
    index: usize,
    node: N,
) {
    match parent {
        None => {
            let i = index.min(nodes.len());
            nodes.insert(i, node);
        }
        Some(pid) => {
            if let Some(p) = find_node_mut(nodes, pid) {
                let children = p.children_vec_mut();
                let i = index.min(children.len());
                children.insert(i, node);
            } else {
                debug_assert!(
                    false,
                    "insert_subtree: parent '{pid}' not found in the tree"
                );
                nodes.push(node);
            }
        }
    }
}

/// One row of the depth-first flattening of the *visible* tree: the
/// canonical representation tree keyboard navigation works against.
/// Carries every field its consumers need — `id` (nav cursor +
/// composite tag), `label` (type-ahead key + AT name), `depth` /
/// `position_in_set` / `size_of_set` (the WAI-ARIA hierarchical axes),
/// `has_children` / `expanded` (expand / collapse / descend + the
/// `aria-expanded` AT state) — so navigation, type-ahead and the AT
/// tree all read one traversal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleRow {
    /// Stable node id — the nav cursor key + composite-tag suffix.
    pub id: String,
    /// Visible label — the type-ahead search key + AT accessible name.
    pub label: String,
    /// Zero-based tree depth (root rows = 0). WAI-ARIA `aria-level` is
    /// `depth + 1`; the Arrow Left parent search compares depths.
    pub depth: u32,
    /// One-based index among visible siblings (WAI-ARIA `aria-posinset`).
    pub position_in_set: u32,
    /// Visible sibling count (WAI-ARIA `aria-setsize`).
    pub size_of_set: u32,
    /// Whether this node is a branch (has children) — drives expand /
    /// collapse / descend and the `aria-expanded` AT state.
    pub has_children: bool,
    /// Whether a branch is currently expanded.
    pub expanded: bool,
}

/// Recursive helper: append each sibling in `siblings` (all at `depth`),
/// then descend into the expanded ones, recording each row's WAI-ARIA
/// `posinset` / `setsize` from its sibling position. Hoisted out of
/// [`flat_visible`] so `clippy::items_after_statements` stays clean.
fn walk_visible<N: TreeNode>(siblings: &[N], depth: u32, out: &mut Vec<VisibleRow>) {
    let size_of_set = u32::try_from(siblings.len()).unwrap_or(u32::MAX);
    for (idx, node) in siblings.iter().enumerate() {
        let children = node.children();
        out.push(VisibleRow {
            id: node.id().to_owned(),
            label: node.label().to_owned(),
            depth,
            position_in_set: u32::try_from(idx + 1).unwrap_or(u32::MAX),
            size_of_set,
            has_children: !children.is_empty(),
            expanded: node.expanded(),
        });
        if node.expanded() {
            walk_visible(children, depth + 1, out);
        }
    }
}

/// The single depth-first walk of the visible row sequence (exactly what
/// a tree paint walk emits): the SSOT consumed by keyboard navigation
/// ([`resolve_tree_key`]), type-ahead and the AT tree. A collapsed
/// branch's descendants are omitted, so the flattening is precisely the
/// set of rows the user can see.
#[must_use]
pub fn flat_visible<N: TreeNode>(nodes: &[N]) -> Vec<VisibleRow> {
    let mut out: Vec<VisibleRow> = Vec::new();
    walk_visible(nodes, 0, &mut out);
    out
}

/// R821 §5.27 §5.50 — the **recursive sort/filter proxy** flattening of a
/// tree: the tree peer of the 1-D list's
/// [`compute_order`](super::view_order::compute_order) filter and the data
/// grid's [`GridFilter`](super::grid_sort::GridFilter) facet, completing the
/// proxy family (list / grid / tree) the Model/View substrate windows over.
///
/// The model is the toolkit's sort filter proxy model with
/// `setRecursiveFilteringEnabled(true)`: a node is kept **iff it matches
/// `accepts`, or any of its descendants does** — the *path-to-match* policy
/// every find-as-you-type tree converges on (a code editor's search tree,
/// `IntelliJ` Speed Search, a file manager's filter box). Three consequences,
/// all canonical:
///
/// - **Ancestors of a match are revealed** regardless of their retained
///   [`expanded`](TreeNode::expanded) flag — a match can never stay hidden
///   inside a collapsed branch — so this walk *ignores* expand state where
///   [`flat_visible`] honours it.
/// - **Non-matching siblings vanish**, leaving only the branches that lead
///   to a match; [`position_in_set`](VisibleRow::position_in_set) /
///   [`size_of_set`](VisibleRow::size_of_set) renumber over the **surviving**
///   siblings, so WAI-ARIA "N of M" announces the filtered group.
/// - Each emitted row's [`has_children`](VisibleRow::has_children) /
///   [`expanded`](VisibleRow::expanded) reflect the **filtered** structure: a
///   branch keeps the children that survive (`has_children = true`,
///   auto-`expanded`); a node that matches but whose every child is pruned
///   becomes a leaf in the filtered view (`has_children = false`).
///
/// The output is an ordinary [`VisibleRow`] sequence — same depth-first
/// preorder, same WAI-ARIA axes as [`flat_visible`] — so it drops straight
/// into `view_virtual_tree`, `tree_access_nodes`, and [`resolve_tree_key`]
/// with **no** change: the filtered tree is just another visible-row sequence
/// the windowing, AT and keyboard axes all read.
///
/// `accepts` is the per-node match predicate (typically a case-insensitive
/// substring over [`label`](TreeNode::label)); the *matcher policy* — name
/// search, kind filter, glob — is the caller's, exactly as
/// [`compute_order`](super::view_order::compute_order) takes the consumer's
/// `pass` closure. The SSOT this owns is the **recursion** (path-to-match +
/// auto-expand + sibling renumbering), the error-prone part every tree
/// consumer would otherwise re-derive. Each node is visited once (O(n)); the
/// proxy coordinator ([`TreeFilterState`](super::tree_filter::TreeFilterState))
/// memoizes the result on the query so it recomputes only when the filter
/// changes, never per frame.
#[must_use]
pub fn flat_visible_filtered<N: TreeNode>(
    nodes: &[N],
    accepts: impl Fn(&N) -> bool,
) -> Vec<VisibleRow> {
    let mut out: Vec<VisibleRow> = Vec::new();
    walk_filtered(nodes, 0, &accepts, &mut out);
    out
}

/// Recursive helper for [`flat_visible_filtered`]: build each surviving
/// sibling's subtree depth-first (so every node is visited exactly once),
/// then renumber the survivors' `posinset` / `setsize` over the filtered
/// sibling group. Hoisted out so `clippy::items_after_statements` stays
/// clean (mirrors [`walk_visible`]).
fn walk_filtered<N: TreeNode, F: Fn(&N) -> bool>(
    siblings: &[N],
    depth: u32,
    accepts: &F,
    out: &mut Vec<VisibleRow>,
) {
    // One subtree per surviving sibling, built bottom-up: recurse into the
    // children first (the single O(1)-per-node visit), then a node survives
    // iff it matches or that recursion yielded any visible child rows.
    let mut survivors: Vec<Vec<VisibleRow>> = Vec::new();
    for node in siblings {
        let mut child_rows: Vec<VisibleRow> = Vec::new();
        walk_filtered(node.children(), depth + 1, accepts, &mut child_rows);
        let has_visible_children = !child_rows.is_empty();
        if !accepts(node) && !has_visible_children {
            continue; // no match anywhere in this subtree → pruned
        }
        let mut rows = Vec::with_capacity(child_rows.len() + 1);
        rows.push(VisibleRow {
            id: node.id().to_owned(),
            label: node.label().to_owned(),
            depth,
            // Renumbered over the survivor group once it is known, below.
            position_in_set: 0,
            size_of_set: 0,
            has_children: has_visible_children,
            // A filtered branch is shown fully expanded along the match path.
            expanded: has_visible_children,
        });
        rows.append(&mut child_rows);
        survivors.push(rows);
    }
    let size_of_set = u32::try_from(survivors.len()).unwrap_or(u32::MAX);
    for (idx, mut rows) in survivors.into_iter().enumerate() {
        rows[0].position_in_set = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        rows[0].size_of_set = size_of_set;
        out.append(&mut rows);
    }
}

/// The visible-row index of `index`'s **parent**: the nearest earlier
/// row one level shallower. In a depth-first preorder flattening the
/// parent is always the closest preceding row at `depth − 1`, so Arrow
/// Left "ascend to parent" needs no separate tree walk — it reads the
/// same [`flat_visible`] sequence navigation already holds. `None` at
/// depth 0 (a root row has no parent).
#[must_use]
pub fn parent_row(rows: &[VisibleRow], index: usize) -> Option<usize> {
    let parent_depth = rows[index].depth.checked_sub(1)?;
    rows[..index]
        .iter()
        .rposition(|row| row.depth == parent_depth)
}

/// The outcome of resolving one key against the visible rows, decoupled
/// from any reactive state mutation so the WAI-ARIA Tree keyboard
/// contract is unit-testable as a pure function. The consumer applies
/// the outcome to its own cursor + expand-state store.
#[derive(Debug, PartialEq, Eq)]
pub enum TreeKey {
    /// Move the keyboard cursor to this visible row id.
    Focus(String),
    /// Expand this collapsed branch.
    Expand(String),
    /// Collapse this expanded branch.
    Collapse(String),
    /// Toggle this branch's expanded flag (Space / Enter).
    Toggle(String),
    /// A recognised navigation key with no state change (clamp at an
    /// end, Arrow Right on a leaf, Arrow Left at a parent-less root).
    /// Consumed so it does not fall through to type-ahead.
    Consumed,
    /// Not a navigation key — the caller tries type-ahead next.
    Unhandled,
}

/// Pure WAI-ARIA APG 6.13 Tree keyboard resolver over the
/// [`flat_visible`] `rows`. `current` is the focused row id; `page` is
/// the Page Up/Down jump size. The keyboard cursor only ever sits on a
/// visible row, so every axis is index + depth arithmetic over the one
/// flattened sequence — no separate tree searches:
///
/// - vertical (Up / Down / Home / End / Page) → the shared
///   [`clamp_nav`] SSOT (clamp, not
///   wrap — a tree has ends);
/// - Arrow Right → a collapsed branch expands; an expanded branch
///   descends to its first child (= the very next row in preorder); a
///   leaf is a no-op;
/// - Arrow Left → an expanded branch collapses; otherwise it ascends to
///   the [`parent_row`] (a no-op at a parent-less root);
/// - Space / Enter → toggle a branch.
#[must_use]
pub fn resolve_tree_key(
    rows: &[VisibleRow],
    current: Option<&str>,
    key: &str,
    page: usize,
) -> TreeKey {
    let cursor = current.and_then(|id| rows.iter().position(|row| row.id == id));
    match key {
        "ArrowUp" | "ArrowDown" | "Home" | "End" | "PageUp" | "PageDown" => {
            match clamp_nav(cursor, key, rows.len(), page) {
                Some(target) => TreeKey::Focus(rows[target].id.clone()),
                None => TreeKey::Consumed,
            }
        }
        "ArrowRight" => {
            let Some(i) = cursor else {
                return TreeKey::Consumed;
            };
            let row = &rows[i];
            if !row.has_children {
                TreeKey::Consumed // leaf
            } else if !row.expanded {
                TreeKey::Expand(row.id.clone()) // collapsed branch → expand
            } else {
                // Expanded branch → its first child is the next row in
                // the preorder flattening.
                match rows.get(i + 1) {
                    Some(child) => TreeKey::Focus(child.id.clone()),
                    None => TreeKey::Consumed,
                }
            }
        }
        "ArrowLeft" => {
            let Some(i) = cursor else {
                return TreeKey::Consumed;
            };
            let row = &rows[i];
            if row.has_children && row.expanded {
                TreeKey::Collapse(row.id.clone()) // expanded branch → collapse
            } else {
                // Collapsed branch or leaf → ascend to the parent.
                match parent_row(rows, i) {
                    Some(p) => TreeKey::Focus(rows[p].id.clone()),
                    None => TreeKey::Consumed, // root row, no parent
                }
            }
        }
        "Space" | "Enter" => {
            let Some(i) = cursor else {
                return TreeKey::Consumed;
            };
            if rows[i].has_children {
                TreeKey::Toggle(rows[i].id.clone())
            } else {
                TreeKey::Consumed // leaf
            }
        }
        _ => TreeKey::Unhandled,
    }
}

/// R945.1 §5.27 §5.40 — the keyboard cursor (or selection) id, but only when it
/// still names a currently-visible row. A tree whose cursor can be hidden by a
/// collapse it did not initiate — the lazy outliner's click-collapse, or the
/// inspector's pointer selection landing under a since-collapsed ancestor —
/// must read the cursor through this gate before advertising it as
/// `aria-activedescendant` or painting its focus highlight: a cursor naming an
/// off-screen row would dangle the active descendant at a node the AT can no
/// longer reach. The shell sets the active-descendant **child** unconditionally
/// (it self-corrects only the parent focus tag), so the gate must live with the
/// binding. Returns `None` once the cursor row leaves the visible set, so the
/// next navigation key restarts from an end (the resolver treats an off-list
/// cursor as no current row). The `Signal` read that produces `cursor` stays
/// caller-side — this is the pure SSOT every overlay-cursor tree shares.
#[must_use]
pub fn effective_cursor<'a>(rows: &[VisibleRow], cursor: Option<&'a str>) -> Option<&'a str> {
    let id = cursor?;
    rows.iter().any(|row| row.id == id).then_some(id)
}

/// R820 §5.27 §5.50 — recursive find-by-id for a mutable node in a
/// retained `Vec<N>` tree. The depth-first search the flag-storage
/// mutators ([`set_expanded_in`] / [`toggle_expanded`]) descend; pure
/// (no reactive scope), so it is unit-testable on a plain `Vec`. `None`
/// when no node carries `id`.
///
/// Lifted at R820 when `hello-virtual-tree` became the second retained
/// flag-on-node consumer (with `hello-tree-view`). The `DevTools`
/// inspector is a third tree consumer but retains a *collapse-set
/// overlay*, not flag-on-node, so it drives the pure [`resolve_tree_key`]
/// directly and never calls these helpers ([[ssot-lift-grep-repo-wide-cross-enum]]:
/// storage glue is caller-choice; only the flag-on-node shape is shared).
#[must_use]
pub fn find_node_mut<'a, N: TreeNode>(nodes: &'a mut [N], id: &str) -> Option<&'a mut N> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let Some(found) = find_node_mut(node.children_mut(), id) {
            return Some(found);
        }
    }
    None
}

/// R935.1 §5.27 — the **immutable peer** of [`find_node_mut`]: depth-first
/// find of the node carrying `id`, returning a shared reference (`None` for an
/// unknown id). Lifted from `hello-property-grid` when `hello-tree-reparent`
/// became the second consumer of this exact recursive lookup — a find that
/// descends the wrong child is a correctness bug, so the two must share one
/// copy (the immutable half of the structural-edit family below).
#[must_use]
pub fn find_node<'a, N: TreeNode>(nodes: &'a [N], id: &str) -> Option<&'a N> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let Some(found) = find_node(node.children(), id) {
            return Some(found);
        }
    }
    None
}

/// R820 §5.27 §5.50 — set branch `id`'s expanded flag to `expanded` in a
/// reactive `Signal<Vec<N>>` flag store. A leaf or a redundant set is a
/// no-op (no `Signal::set`, so no repaint). Wrapped in [`batch`] so the
/// single write coalesces.
pub fn set_expanded_in<N>(nodes_signal: &Signal<Vec<N>>, id: &str, expanded: bool)
where
    N: TreeNode + Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    batch(|| {
        let mut nodes = nodes_signal.get();
        if let Some(node) = find_node_mut(&mut nodes, id) {
            if node.children().is_empty() || node.expanded() == expanded {
                return;
            }
            node.set_expanded(expanded);
            nodes_signal.set(nodes);
        }
    });
}

/// R820 §5.27 §5.50 — flip branch `id`'s expanded flag in a reactive
/// `Signal<Vec<N>>` flag store (the click / Space-Enter toggle path).
/// Leaves are a no-op. [`batch`]ed like [`set_expanded_in`].
pub fn toggle_expanded<N>(nodes_signal: &Signal<Vec<N>>, id: &str)
where
    N: TreeNode + Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    batch(|| {
        let mut nodes = nodes_signal.get();
        if let Some(node) = find_node_mut(&mut nodes, id) {
            if node.children().is_empty() {
                return;
            }
            let next = !node.expanded();
            node.set_expanded(next);
            nodes_signal.set(nodes);
        }
    });
}

/// R820 §5.27 §5.50 — apply one key to a retained flag-on-node tree held
/// in a `Signal<Vec<N>>` + a `Signal<Option<String>>` focus cursor: the
/// [`resolve_tree_key`] → flag-store bridge shared by the retained-tree
/// consumers (`hello-tree-view`, `hello-virtual-tree`). Returns `true`
/// when the key was a recognised navigation key (the caller falls through
/// to type-ahead on `false`, which stays caller-side per the module-doc
/// purity boundary — the cursor's search buffer is application state).
///
/// Storage-specific by design (the pure [`resolve_tree_key`] stays the
/// SSOT every consumer shares; this convenience only serves the
/// flag-on-node `Signal` shape — the inspector's collapse-set overlay
/// applies the same `TreeKey` outcome to its own store).
#[must_use]
pub fn apply_tree_key<N>(
    nodes_signal: &Signal<Vec<N>>,
    focused: &Signal<Option<String>>,
    key: &str,
    page: usize,
) -> bool
where
    N: TreeNode + Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    let nodes = nodes_signal.get();
    let rows = flat_visible(&nodes);
    let current = focused.get();
    match resolve_tree_key(&rows, current.as_deref(), key, page) {
        TreeKey::Focus(id) => {
            focused.set(Some(id));
            true
        }
        TreeKey::Expand(id) => {
            set_expanded_in(nodes_signal, &id, true);
            true
        }
        TreeKey::Collapse(id) => {
            set_expanded_in(nodes_signal, &id, false);
            true
        }
        TreeKey::Toggle(id) => {
            toggle_expanded(nodes_signal, &id);
            true
        }
        TreeKey::Consumed => true,
        TreeKey::Unhandled => false,
    }
}

/// R866 §5.27 §5.45 — scroll a **windowed** tree's keyboard cursor row into the
/// rendered window (the keyboard ⊥ virtualization step), so navigating (or
/// type-ahead jumping) to a row outside the rendered window scrolls there and
/// materializes it. No-op when nothing is focused or the cursor row is already
/// visible (`scroll_offset_to_reveal` returns the unchanged offset).
///
/// Lifted at R866 from the byte-identical `reveal_cursor` copies in
/// `hello-virtual-tree` (R820) and `hello-tree-grid` (R864): the second
/// windowed-tree-keyboard consumer is the trigger that lifts the glue, exactly
/// as [`apply_tree_key`] / `tree_typeahead_jump`
/// were lifted at their second consumers (the project's own
/// `[[abstraction-needs-second-consumer]]` rule; R864's "no new substrate"
/// rationalization was the smell). It composes the existing
/// [`scroll_offset_to_reveal`] pixel math with the [`flat_visible`] cursor
/// lookup — both already SSOTs — so it adds no new geometry logic.
///
/// `rows` is the visible flattening, `cursor` the focused row id, `scroll` the
/// vertical body scroll, `pitch` the uniform row height.
pub fn reveal_row_cursor(
    rows: &[VisibleRow],
    cursor: Option<&str>,
    scroll: &ScrollState,
    pitch: u32,
) {
    let Some(id) = cursor else { return };
    let Some(index) = rows.iter().position(|row| row.id == id) else {
        return;
    };
    let (_, measured_h) = scroll.measured_viewport();
    let offset = scroll_offset_to_reveal(index, scroll.offset_y(), measured_h, pitch);
    scroll.scroll_to(0, offset);
}

/// Resolve a visible position (`rest`, the part after a matched
/// `<axis>_at.` prefix) to a per-row projection. An out-of-range or
/// unparseable position reports [`IntrospectValue::Null`]
/// (present-but-empty) — the same convention
/// [`TreeFilterExternal`](super::tree_filter::TreeFilterExternal) uses for
/// its `visible_at.<pos>`.
fn row_at(
    rest: &str,
    rows: &[VisibleRow],
    project: impl Fn(&VisibleRow) -> IntrospectValue,
) -> IntrospectValue {
    // The tree-view binding of the shared [`at_index`] projection: index the
    // visible-row slice, project the row to its per-axis value.
    at_index(rest, |i| rows.get(i), project)
}

/// R823 §5.50 §5.12 — the **read-only introspection source** for a tree
/// widget's interactive state: the visible-row flattening + the keyboard
/// cursor, surfaced to the §5.12 `scene/query` path so an AI client reads
/// the tree's structure and navigation state *as data* rather than
/// scraping the paint scene ([[ai-first-rpc-introspection-obligation]]).
///
/// It is the base-tree peer of
/// [`TreeFilterExternal`](super::tree_filter::TreeFilterExternal) (which
/// surfaces the *filtered* view): where the row-click router
/// (`TreeRowClickExternal`) exposes only the transient `pressed`/`hovered`
/// pointer state, this exposes the structural + WAI-ARIA axes —
/// `row_count`, the `cursor` id + its `cursor_index`, and per visible
/// position `id_at` / `label_at` / `level_at` (aria-level) / `expanded_at`
/// (aria-expanded: a `Bool` for a branch, `Null` for a leaf).
///
/// **Read-only by design**: navigation and expand/collapse already mutate
/// through the click router and `scene/key`, so this node owns no mutation
/// path — it is the third consumer of the
/// [`QueryOnlyIntrospect`] lift
/// (after `ModalState` / `SnackbarTimer`). It is **node-type agnostic**
/// like [`TreeFilterState`](super::tree_filter::TreeFilterState): it holds
/// two closures that read the consumer's retained tree + cursor `Signal`s
/// fresh on each query, so `pinion-core` carries no node generics and the
/// consumer keeps its own state storage (R811 storage-is-caller-choice).
pub struct TreeViewIntrospect {
    rows: Box<dyn Fn() -> Vec<VisibleRow>>,
    cursor: Box<dyn Fn() -> Option<String>>,
}

impl TreeViewIntrospect {
    /// Construct over the consumer's state readers: `rows` yields the
    /// current visible-row flattening (typically `move || flat_visible(&nodes.get())`)
    /// and `cursor` the focused row id (typically `move || focused.get()`).
    /// Both are read fresh on every query, so the introspection always
    /// reports live state.
    #[must_use]
    pub fn new(
        rows: impl Fn() -> Vec<VisibleRow> + 'static,
        cursor: impl Fn() -> Option<String> + 'static,
    ) -> Self {
        Self {
            rows: Box::new(rows),
            cursor: Box::new(cursor),
        }
    }
}

impl core::fmt::Debug for TreeViewIntrospect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TreeViewIntrospect")
            .field("row_count", &(self.rows)().len())
            .field("cursor", &(self.cursor)())
            .finish_non_exhaustive()
    }
}

impl QuerySource for TreeViewIntrospect {
    fn introspect_schema(&self) -> IntrospectSchema {
        // `row_count`   — visible row count (int).
        // `cursor`      — keyboard cursor row id (string), Null when unset.
        // `cursor_index`— cursor's index among visible rows (int), Null when
        //                 unset or off the visible set.
        // `id_at`       — `id_at.<pos>` visible position → node id.
        // `label_at`    — `label_at.<pos>` visible position → label.
        // `level_at`    — `level_at.<pos>` → WAI-ARIA aria-level (depth + 1).
        // `expanded_at` — `expanded_at.<pos>` → aria-expanded (Bool for a
        //                 branch, Null for a leaf).
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("row_count", "int"),
                    SchemaField::new("cursor", "string"),
                    SchemaField::new("cursor_index", "int"),
                    // R1353 §2 #2 — these four are PARAMETRIC and now say so.
                    // They read a VISIBLE position, so the bound is `row_count`
                    // (the flattened visible rows), not the tree's node count: a
                    // client enumerating them walks what is on screen.
                    SchemaField::parametric(
                        "id_at.<pos>",
                        "string",
                        const { &[SchemaArg::index("pos", "row_count")] },
                    ),
                    SchemaField::parametric(
                        "label_at.<pos>",
                        "string",
                        const { &[SchemaArg::index("pos", "row_count")] },
                    ),
                    SchemaField::parametric(
                        "level_at.<pos>",
                        "int",
                        const { &[SchemaArg::index("pos", "row_count")] },
                    ),
                    SchemaField::parametric(
                        "expanded_at.<pos>",
                        "bool",
                        const { &[SchemaArg::index("pos", "row_count")] },
                    ),
                ]
            },
        )
    }

    fn introspect_query(&self, path: &str) -> Option<IntrospectValue> {
        let rows = (self.rows)();
        if let Some(rest) = path.strip_prefix("id_at.") {
            return Some(row_at(rest, &rows, |r| IntrospectValue::Text(r.id.clone())));
        }
        if let Some(rest) = path.strip_prefix("label_at.") {
            return Some(row_at(rest, &rows, |r| {
                IntrospectValue::Text(r.label.clone())
            }));
        }
        if let Some(rest) = path.strip_prefix("level_at.") {
            return Some(row_at(rest, &rows, |r| {
                IntrospectValue::Int(i64::from(r.depth) + 1)
            }));
        }
        if let Some(rest) = path.strip_prefix("expanded_at.") {
            return Some(row_at(rest, &rows, |r| {
                // aria-expanded is undefined (Null) for a leaf, Bool for a branch.
                if r.has_children {
                    IntrospectValue::Bool(r.expanded)
                } else {
                    IntrospectValue::Null
                }
            }));
        }
        match path {
            "row_count" => Some(IntrospectValue::Int(
                i64::try_from(rows.len()).unwrap_or(i64::MAX),
            )),
            "cursor" => Some((self.cursor)().map_or(IntrospectValue::Null, IntrospectValue::Text)),
            "cursor_index" => {
                let index = (self.cursor)().and_then(|id| rows.iter().position(|r| r.id == id));
                Some(index.map_or(IntrospectValue::Null, |i| {
                    IntrospectValue::Int(i64::try_from(i).unwrap_or(i64::MAX))
                }))
            }
            _ => None,
        }
    }
}

/// R823 §5.50 §5.12 — register a [`TreeViewIntrospect`] as a query-only
/// [`ExtraExternal`] at `tag`, mirroring
/// [`modal_introspection_extra`](super::modal::modal_introspection_extra) /
/// [`snackbar_introspection_extra`](super::snackbar::snackbar_introspection_extra).
/// `rows` / `cursor` are the consumer's live-state readers (see
/// [`TreeViewIntrospect::new`]). The node paints nothing — it surfaces the
/// tree's state to `scene/query` only.
#[must_use]
pub fn tree_view_introspection_extra(
    tag: &'static str,
    rows: impl Fn() -> Vec<VisibleRow> + 'static,
    cursor: impl Fn() -> Option<String> + 'static,
) -> ExtraExternal {
    ExtraExternal::new(
        tag,
        Box::new(QueryOnlyIntrospect::new(Rc::new(TreeViewIntrospect::new(
            rows, cursor,
        )))),
    )
}

#[cfg(test)]
mod tests {
    //! R811 §5.50 §5.27 — the WAI-ARIA APG 6.13 Tree keyboard contract
    //! over the pure [`resolve_tree_key`] resolver (no reactive scope, so
    //! the keyboard semantics are unit-testable without a framework
    //! cache). Ported from `hello-tree-view`'s R809 nav tests when the
    //! resolver was lifted here at R811; the example now exercises only
    //! its `FileNode` [`TreeNode`] glue.

    use super::{
        MutableTreeNode, TreeKey, TreeNode, VisibleRow, apply_tree_key, effective_cursor,
        find_node, find_node_mut, flat_visible, flat_visible_filtered, insert_subtree, parent_row,
        remove_subtree, resolve_tree_key, set_expanded_in, toggle_expanded,
    };
    use crate::Signal;
    use crate::reactive::Owner;

    /// Page jump used by the tree consumers (mirrors `hello-tree-view`'s
    /// `NAV_PAGE`); larger than the sample listing so the page arm
    /// clamps to an end.
    const NAV_PAGE: usize = 7;

    /// A minimal [`TreeNode`] for the resolver tests, standing in for the
    /// real consumer node types (`FileNode` / `TreeItem`). Derives the
    /// `Signal` value bounds (`Clone + PartialEq + Serialize +
    /// DeserializeOwned`) so the retained-tree helpers
    /// ([`set_expanded_in`] / [`toggle_expanded`] / [`apply_tree_key`])
    /// are exercised here over a `Signal<Vec<TestNode>>` directly.
    #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct TestNode {
        id: String,
        label: String,
        expanded: bool,
        children: Vec<TestNode>,
    }

    impl TestNode {
        fn leaf(id: &str, label: &str) -> Self {
            Self {
                id: id.to_owned(),
                label: label.to_owned(),
                expanded: false,
                children: Vec::new(),
            }
        }

        fn branch(id: &str, label: &str, expanded: bool, children: Vec<TestNode>) -> Self {
            Self {
                id: id.to_owned(),
                label: label.to_owned(),
                expanded,
                children,
            }
        }
    }

    impl TreeNode for TestNode {
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

    impl MutableTreeNode for TestNode {
        fn children_vec_mut(&mut self) -> &mut Vec<Self> {
            &mut self.children
        }
    }

    /// A deterministic sample tree:
    /// ```text
    /// src        (branch, expanded)
    ///   main.rs  (leaf)
    ///   lib.rs   (leaf)
    ///   widgets  (branch, collapsed)   <- mod.rs hidden
    /// tests      (branch, collapsed)   <- children hidden
    /// docs       (branch, collapsed)   <- children hidden
    /// ```
    /// Visible row order: src, src/main.rs, src/lib.rs, src/widgets,
    /// tests, docs (6 rows).
    fn sample() -> Vec<TestNode> {
        vec![
            TestNode::branch(
                "src",
                "src",
                true,
                vec![
                    TestNode::leaf("src/main.rs", "main.rs"),
                    TestNode::leaf("src/lib.rs", "lib.rs"),
                    TestNode::branch(
                        "src/widgets",
                        "widgets",
                        false,
                        vec![TestNode::leaf("src/widgets/mod.rs", "mod.rs")],
                    ),
                ],
            ),
            TestNode::branch(
                "tests",
                "tests",
                false,
                vec![TestNode::leaf("tests/it.rs", "it.rs")],
            ),
            TestNode::branch(
                "docs",
                "docs",
                false,
                vec![TestNode::leaf("docs/README.md", "README.md")],
            ),
        ]
    }

    fn key(current: Option<&str>, k: &str) -> TreeKey {
        let rows = flat_visible(&sample());
        resolve_tree_key(&rows, current, k, NAV_PAGE)
    }

    #[test]
    fn flat_visible_is_paint_order_with_labels() {
        let rows = flat_visible(&sample());
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "src",
                "src/main.rs",
                "src/lib.rs",
                "src/widgets",
                "tests",
                "docs"
            ],
            "flat visible order = depth-first preorder over expanded branches",
        );
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            labels,
            ["src", "main.rs", "lib.rs", "widgets", "tests", "docs"]
        );
    }

    #[test]
    fn flat_visible_carries_waiaria_axes() {
        let rows = flat_visible(&sample());
        let by_id = |id: &str| rows.iter().find(|r| r.id == id).expect("row present");
        let src = by_id("src");
        assert_eq!((src.depth, src.position_in_set, src.size_of_set), (0, 1, 3));
        assert!(src.has_children && src.expanded);
        let lib = by_id("src/lib.rs");
        assert_eq!((lib.depth, lib.position_in_set, lib.size_of_set), (1, 2, 3));
        assert!(!lib.has_children, "a leaf reports no children");
        let docs = by_id("docs");
        assert_eq!(
            (docs.depth, docs.position_in_set, docs.size_of_set),
            (0, 3, 3)
        );
        assert!(
            docs.has_children && !docs.expanded,
            "docs is a collapsed branch"
        );
    }

    #[test]
    fn parent_row_is_nearest_shallower_predecessor() {
        let rows = flat_visible(&sample());
        let idx = |id: &str| rows.iter().position(|r| r.id == id).expect("row present");
        // src/lib.rs (depth 1) → src (depth 0).
        assert_eq!(parent_row(&rows, idx("src/lib.rs")), Some(idx("src")));
        // src (depth 0, a root) → no parent.
        assert_eq!(parent_row(&rows, idx("src")), None);
    }

    #[test]
    fn arrow_down_moves_to_next_visible_row() {
        assert_eq!(
            key(Some("src"), "ArrowDown"),
            TreeKey::Focus("src/main.rs".into())
        );
    }

    #[test]
    fn arrow_up_moves_to_previous_visible_row() {
        assert_eq!(
            key(Some("src/lib.rs"), "ArrowUp"),
            TreeKey::Focus("src/main.rs".into())
        );
    }

    #[test]
    fn arrow_down_clamps_at_last_row_no_wrap() {
        assert_eq!(
            key(Some("docs"), "ArrowDown"),
            TreeKey::Focus("docs".into())
        );
    }

    #[test]
    fn arrow_up_clamps_at_first_row_no_wrap() {
        assert_eq!(key(Some("src"), "ArrowUp"), TreeKey::Focus("src".into()));
    }

    #[test]
    fn home_and_end_jump_to_first_and_last() {
        assert_eq!(key(Some("tests"), "Home"), TreeKey::Focus("src".into()));
        assert_eq!(key(Some("src"), "End"), TreeKey::Focus("docs".into()));
    }

    #[test]
    fn page_down_and_up_jump_clamped() {
        assert_eq!(key(Some("src"), "PageDown"), TreeKey::Focus("docs".into()));
        assert_eq!(key(Some("docs"), "PageUp"), TreeKey::Focus("src".into()));
    }

    #[test]
    fn arrow_right_on_collapsed_branch_expands() {
        assert_eq!(
            key(Some("tests"), "ArrowRight"),
            TreeKey::Expand("tests".into())
        );
    }

    #[test]
    fn arrow_right_on_expanded_branch_descends_to_first_child() {
        assert_eq!(
            key(Some("src"), "ArrowRight"),
            TreeKey::Focus("src/main.rs".into())
        );
    }

    #[test]
    fn arrow_right_on_leaf_is_consumed_noop() {
        assert_eq!(key(Some("src/main.rs"), "ArrowRight"), TreeKey::Consumed);
    }

    #[test]
    fn arrow_left_on_expanded_branch_collapses() {
        assert_eq!(
            key(Some("src"), "ArrowLeft"),
            TreeKey::Collapse("src".into())
        );
    }

    #[test]
    fn arrow_left_on_collapsed_branch_ascends_to_parent() {
        assert_eq!(
            key(Some("src/widgets"), "ArrowLeft"),
            TreeKey::Focus("src".into())
        );
    }

    #[test]
    fn arrow_left_on_leaf_ascends_to_parent() {
        assert_eq!(
            key(Some("src/main.rs"), "ArrowLeft"),
            TreeKey::Focus("src".into())
        );
    }

    #[test]
    fn arrow_left_on_root_level_collapsed_branch_is_consumed() {
        assert_eq!(key(Some("tests"), "ArrowLeft"), TreeKey::Consumed);
    }

    #[test]
    fn space_and_enter_toggle_a_branch() {
        assert_eq!(key(Some("tests"), "Space"), TreeKey::Toggle("tests".into()));
        assert_eq!(key(Some("src"), "Enter"), TreeKey::Toggle("src".into()));
    }

    #[test]
    fn space_on_leaf_is_consumed_noop() {
        assert_eq!(key(Some("src/main.rs"), "Space"), TreeKey::Consumed);
    }

    #[test]
    fn printable_char_is_unhandled_for_typeahead_fallthrough() {
        assert_eq!(key(Some("src"), "m"), TreeKey::Unhandled);
        assert_eq!(key(Some("src"), "\u{d55c}"), TreeKey::Unhandled);
    }

    #[test]
    fn unknown_named_key_is_unhandled() {
        assert_eq!(key(Some("src"), "F1"), TreeKey::Unhandled);
    }

    #[test]
    fn no_cursor_vertical_key_starts_from_an_end() {
        // A `None` cursor (nothing focused yet) on a vertical key resolves
        // through `clamp_nav`'s start-from-an-end behaviour rather than
        // panicking — the same contract `hello-tree-view` relied on.
        let rows: Vec<VisibleRow> = flat_visible(&sample());
        assert_eq!(
            resolve_tree_key(&rows, None, "ArrowDown", NAV_PAGE),
            TreeKey::Focus("src".into()),
        );
    }

    #[test]
    fn find_node_mut_descends_to_a_nested_branch_and_mutates() {
        let mut nodes = sample();
        let found = find_node_mut(&mut nodes, "src/widgets").expect("nested branch found");
        assert_eq!(found.id(), "src/widgets");
        // The write side of the R820 trait extension persists through the tree.
        found.set_expanded(true);
        assert!(
            find_node_mut(&mut nodes, "src/widgets").unwrap().expanded(),
            "set_expanded mutation persists"
        );
        assert!(
            find_node_mut(&mut nodes, "absent").is_none(),
            "missing id -> None"
        );
    }

    // ── `flat_visible_filtered` recursive path-to-match proxy (R821) ──
    //
    // Restored to pinion-core at R822: the test target's
    // `clippy::large_stack_arrays` ceiling was libtest's own
    // `[&TestDescAndFn; N]` runner table crossing 16384 bytes at the 2049th
    // test, not a serde `Signal` buffer (the R820/R821 diagnosis was wrong);
    // a `cfg(test)`-scoped crate allow in `lib.rs` lifts it, so the SSOT
    // decision is unit-tested next to its definition again.

    /// `(id, depth, posinset, setsize, has_children, expanded)` per row.
    fn shape(rows: &[VisibleRow]) -> Vec<(&str, u32, u32, u32, bool, bool)> {
        rows.iter()
            .map(|r| {
                (
                    r.id.as_str(),
                    r.depth,
                    r.position_in_set,
                    r.size_of_set,
                    r.has_children,
                    r.expanded,
                )
            })
            .collect()
    }

    fn label_contains(needle: &str) -> impl Fn(&TestNode) -> bool + '_ {
        move |n: &TestNode| n.label().to_lowercase().contains(&needle.to_lowercase())
    }

    #[test]
    fn filter_reveals_path_to_match_through_a_collapsed_branch() {
        // "mod" matches only `src/widgets/mod.rs`, which lives in the
        // *collapsed* `widgets` branch. Path-to-match reveals the ancestors
        // regardless of expand state, prunes every non-path sibling, and
        // renumbers posinset/setsize over the surviving (single-child) groups.
        let rows = flat_visible_filtered(&sample(), label_contains("mod"));
        assert_eq!(
            shape(&rows),
            [
                ("src", 0, 1, 1, true, true),
                ("src/widgets", 1, 1, 1, true, true),
                ("src/widgets/mod.rs", 2, 1, 1, false, false),
            ],
        );
    }

    #[test]
    fn filter_matching_branch_with_no_matching_children_is_a_leaf() {
        // "widgets" matches the branch by name; its `mod.rs` child does not,
        // so the branch shows as a filtered leaf (has_children = false) and
        // the `tests` / `docs` siblings are pruned.
        let rows = flat_visible_filtered(&sample(), label_contains("widgets"));
        assert_eq!(
            shape(&rows),
            [
                ("src", 0, 1, 1, true, true),
                ("src/widgets", 1, 1, 1, false, false)
            ]
        );
    }

    #[test]
    fn filter_no_match_is_empty_and_query_is_case_insensitive() {
        assert!(flat_visible_filtered(&sample(), label_contains("zzz")).is_empty());
        // A lowercase query finds the same path as the original-case label.
        let upper = flat_visible_filtered(&sample(), label_contains("MAIN"));
        assert_eq!(
            shape(&upper),
            [
                ("src", 0, 1, 1, true, true),
                ("src/main.rs", 1, 1, 1, false, false)
            ],
        );
    }

    // ── retained-tree flag-store helpers over `Signal<Vec<TestNode>>` ──
    //
    // Restored to pinion-core at R822 (see the note above): `set_expanded_in`
    // / `toggle_expanded` / the `apply_tree_key` bridge mutate a reactive
    // `Signal<Vec<N>>`, so they are exercised here against a real signal under
    // an `Owner` scope, the same shape `hello-tree-view` / `hello-virtual-tree`
    // drive.

    #[test]
    fn toggle_expanded_flips_a_branch_and_is_a_noop_on_leaves() {
        Owner::new().run(|| {
            let nodes = Signal::new(sample());
            // `tests` boots collapsed → toggling reveals its child row.
            toggle_expanded(&nodes, "tests");
            let ids: Vec<String> = flat_visible(&nodes.get())
                .iter()
                .map(|r| r.id.clone())
                .collect();
            assert!(
                ids.iter().any(|id| id == "tests/it.rs"),
                "toggled branch reveals its child"
            );
            // Toggling back hides it again.
            toggle_expanded(&nodes, "tests");
            let after = flat_visible(&nodes.get());
            assert!(
                after.iter().all(|r| r.id != "tests/it.rs"),
                "second toggle re-collapses"
            );
            // A leaf toggle is a no-op: no value change, so no revision bump.
            let rev = nodes.revision();
            toggle_expanded(&nodes, "src/main.rs");
            assert_eq!(
                nodes.revision(),
                rev,
                "toggling a leaf does not write the signal"
            );
        });
    }

    #[test]
    fn set_expanded_in_collapses_a_branch_and_skips_redundant_writes() {
        Owner::new().run(|| {
            let nodes = Signal::new(sample());
            // `src` boots expanded → collapsing hides every descendant.
            set_expanded_in(&nodes, "src", false);
            let ids: Vec<String> = flat_visible(&nodes.get())
                .iter()
                .map(|r| r.id.clone())
                .collect();
            assert_eq!(
                ids,
                ["src", "tests", "docs"],
                "collapsed branch hides its subtree"
            );
            // Re-collapsing an already-collapsed branch is a no-op.
            let rev = nodes.revision();
            set_expanded_in(&nodes, "src", false);
            assert_eq!(
                nodes.revision(),
                rev,
                "redundant set does not write the signal"
            );
        });
    }

    #[test]
    fn apply_tree_key_bridges_resolver_outcomes_to_the_flag_store() {
        Owner::new().run(|| {
            let nodes = Signal::new(sample());
            let focused = Signal::new(Some(String::from("src")));
            // A navigation key moves the focus cursor and reports handled.
            assert!(apply_tree_key(&nodes, &focused, "ArrowDown", NAV_PAGE));
            assert_eq!(focused.get().as_deref(), Some("src/main.rs"));
            // Arrow Right on a collapsed branch expands it in the flag store.
            focused.set(Some(String::from("tests")));
            assert!(apply_tree_key(&nodes, &focused, "ArrowRight", NAV_PAGE));
            assert!(
                find_node_mut(&mut nodes.get(), "tests").unwrap().expanded(),
                "ArrowRight expanded the collapsed branch via set_expanded_in",
            );
            // A printable key is unhandled → the caller falls through to type-ahead.
            assert!(!apply_tree_key(&nodes, &focused, "m", NAV_PAGE));
        });
    }

    // ── TreeViewIntrospect: read-only tree-state RPC introspection (R823) ──

    #[test]
    fn tree_view_introspect_exposes_structure_and_cursor() {
        use super::TreeViewIntrospect;
        use crate::external::{IntrospectValue, QuerySource};
        // sample() visible order: src, src/main.rs, src/lib.rs, src/widgets,
        // tests, docs (6 rows); cursor parked on src/lib.rs.
        let src = TreeViewIntrospect::new(
            || flat_visible(&sample()),
            || Some(String::from("src/lib.rs")),
        );
        assert_eq!(
            src.introspect_query("row_count"),
            Some(IntrospectValue::Int(6))
        );
        assert_eq!(
            src.introspect_query("cursor"),
            Some(IntrospectValue::Text("src/lib.rs".into()))
        );
        assert_eq!(
            src.introspect_query("cursor_index"),
            Some(IntrospectValue::Int(2))
        );
        // Per-position id / label.
        assert_eq!(
            src.introspect_query("id_at.0"),
            Some(IntrospectValue::Text("src".into()))
        );
        assert_eq!(
            src.introspect_query("label_at.1"),
            Some(IntrospectValue::Text("main.rs".into()))
        );
        // aria-level = depth + 1; aria-expanded = Bool for a branch.
        assert_eq!(
            src.introspect_query("level_at.0"),
            Some(IntrospectValue::Int(1))
        );
        assert_eq!(
            src.introspect_query("expanded_at.0"),
            Some(IntrospectValue::Bool(true))
        );
        // src/widgets is a collapsed branch one level deeper.
        assert_eq!(
            src.introspect_query("level_at.3"),
            Some(IntrospectValue::Int(2))
        );
        assert_eq!(
            src.introspect_query("expanded_at.3"),
            Some(IntrospectValue::Bool(false))
        );
        // A leaf's aria-expanded is undefined (Null).
        assert_eq!(
            src.introspect_query("expanded_at.1"),
            Some(IntrospectValue::Null)
        );
        // Out-of-range position is present-but-empty; undeclared path is absent.
        assert_eq!(
            src.introspect_query("id_at.99"),
            Some(IntrospectValue::Null)
        );
        assert_eq!(src.introspect_query("nope"), None);
    }

    #[test]
    fn effective_cursor_gates_to_a_visible_row() {
        // R945.1 — the shared overlay-cursor gate: a cursor naming a visible row
        // passes through; one naming no visible row (hidden under a collapse, or
        // never present) returns None so the binding never advertises a
        // dangling aria-activedescendant.
        let rows = flat_visible(&sample()); // src, src/main.rs, src/lib.rs, src/widgets, tests, docs
        assert_eq!(
            effective_cursor(&rows, Some("src/lib.rs")),
            Some("src/lib.rs"),
            "visible passes"
        );
        assert_eq!(
            effective_cursor(&rows, Some("ghost")),
            None,
            "off-list cursor gates to None"
        );
        assert_eq!(effective_cursor(&rows, None), None, "no cursor → None");
    }

    #[test]
    fn tree_view_introspect_handles_unset_and_off_set_cursor() {
        use super::TreeViewIntrospect;
        use crate::external::{IntrospectValue, QuerySource};
        let none = TreeViewIntrospect::new(|| flat_visible(&sample()), || None);
        assert_eq!(none.introspect_query("cursor"), Some(IntrospectValue::Null));
        assert_eq!(
            none.introspect_query("cursor_index"),
            Some(IntrospectValue::Null)
        );
        // A cursor on a collapsed-away id is reported but has no visible index.
        let hidden = TreeViewIntrospect::new(
            || flat_visible(&sample()),
            || Some(String::from("tests/it.rs")),
        );
        assert_eq!(
            hidden.introspect_query("cursor"),
            Some(IntrospectValue::Text("tests/it.rs".into()))
        );
        assert_eq!(
            hidden.introspect_query("cursor_index"),
            Some(IntrospectValue::Null)
        );
    }

    #[test]
    fn tree_view_introspect_schema_lists_every_query_axis() {
        use super::TreeViewIntrospect;
        use crate::external::QuerySource;
        let src = TreeViewIntrospect::new(Vec::new, || None);
        let names: Vec<&str> = src
            .introspect_schema()
            .fields
            .iter()
            .map(|f| f.path)
            .collect();
        // (R1353) The four `_at` axes declare their wire TEMPLATE, so this pins
        // what an agent actually reads out of `$schema` — `id_at.<pos>`, not the
        // stem `id_at`, which is not a readable path at all. `stem()` recovers
        // the prefix the `query` impl strips.
        assert_eq!(
            names,
            [
                "row_count",
                "cursor",
                "cursor_index",
                "id_at.<pos>",
                "label_at.<pos>",
                "level_at.<pos>",
                "expanded_at.<pos>"
            ],
        );
        let prefixes: Vec<&str> = src
            .introspect_schema()
            .fields
            .iter()
            .map(crate::external::SchemaField::literal_prefix)
            .collect();
        // `literal_prefix` is what each `query` arm strips, verbatim — the
        // separator included, because the separator is the author's choice.
        assert_eq!(
            prefixes,
            [
                "row_count",
                "cursor",
                "cursor_index",
                "id_at.",
                "label_at.",
                "level_at.",
                "expanded_at."
            ],
        );
    }

    // R866 §5.27 §5.45 — the lifted windowed-cursor scroll-into-view helper.

    fn flat_rows(n: usize) -> Vec<VisibleRow> {
        (0..n)
            .map(|i| VisibleRow {
                id: format!("r{i}"),
                label: format!("Row {i}"),
                depth: 0,
                position_in_set: u32::try_from(i + 1).unwrap_or(u32::MAX),
                size_of_set: u32::try_from(n).unwrap_or(u32::MAX),
                has_children: false,
                expanded: false,
            })
            .collect()
    }

    #[test]
    fn reveal_row_cursor_scrolls_an_off_window_cursor_into_view() {
        use super::reveal_row_cursor;
        use crate::widgets::scroll::ScrollState;
        let rows = flat_rows(100);
        let scroll = ScrollState::new();
        scroll.set_measured_viewport(200, 10 * 24); // 10-row window, pitch 24
        scroll.set_max(0, 100 * 24);
        // A deep cursor is scrolled into the window.
        reveal_row_cursor(&rows, Some("r80"), &scroll, 24);
        assert!(scroll.offset_y() > 0, "deep cursor scrolled into view");
    }

    #[test]
    fn reveal_row_cursor_is_a_noop_without_a_resolvable_cursor() {
        use super::reveal_row_cursor;
        use crate::widgets::scroll::ScrollState;
        let rows = flat_rows(100);
        let scroll = ScrollState::new();
        scroll.set_measured_viewport(200, 240);
        scroll.set_max(0, 100 * 24);
        reveal_row_cursor(&rows, None, &scroll, 24);
        assert_eq!(scroll.offset_y(), 0, "no cursor -> no scroll");
        reveal_row_cursor(&rows, Some("nonexistent"), &scroll, 24);
        assert_eq!(scroll.offset_y(), 0, "unknown cursor -> no scroll");
    }

    // ── R935 structural mutation: remove_subtree / insert_subtree ──────────

    /// The id sequence of the whole (not just visible) tree, depth-first —
    /// expand everything so structure is fully observable.
    fn all_ids(nodes: &[TestNode]) -> Vec<String> {
        fn walk(nodes: &[TestNode], out: &mut Vec<String>) {
            for n in nodes {
                out.push(n.id.clone());
                walk(&n.children, out);
            }
        }
        let mut out = Vec::new();
        walk(nodes, &mut out);
        out
    }

    #[test]
    fn find_node_locates_at_any_depth_immutably() {
        let tree = sample();
        assert_eq!(find_node(&tree, "src").map(TreeNode::id), Some("src"));
        assert_eq!(
            find_node(&tree, "src/widgets/mod.rs").map(TreeNode::id),
            Some("src/widgets/mod.rs")
        );
        assert!(find_node(&tree, "no-such-id").is_none());
    }

    #[test]
    fn remove_subtree_takes_a_nested_branch_with_its_children() {
        let mut tree = sample();
        let taken = remove_subtree(&mut tree, "src/widgets").expect("widgets removed");
        assert_eq!(taken.id, "src/widgets");
        assert_eq!(
            taken.children.len(),
            1,
            "the subtree's child travels with it"
        );
        assert!(
            !all_ids(&tree).contains(&"src/widgets".to_owned()),
            "gone from the tree"
        );
        assert!(
            !all_ids(&tree).contains(&"src/widgets/mod.rs".to_owned()),
            "and its descendant"
        );
        assert!(
            remove_subtree(&mut tree, "no-such-id").is_none(),
            "absent id → None"
        );
    }

    #[test]
    fn insert_subtree_places_under_parent_at_clamped_index() {
        let mut tree = sample();
        let node = TestNode::leaf("src/new.rs", "new.rs");
        insert_subtree(&mut tree, Some("src"), 1, node);
        let src = find_node_mut(&mut tree, "src").expect("src present");
        assert_eq!(
            src.children[1].id, "src/new.rs",
            "inserted at index 1 under src"
        );
        // An over-range index clamps to the end rather than panicking.
        insert_subtree(
            &mut tree,
            Some("src"),
            999,
            TestNode::leaf("src/end.rs", "end.rs"),
        );
        let src = find_node_mut(&mut tree, "src").expect("src present");
        assert_eq!(
            src.children.last().unwrap().id,
            "src/end.rs",
            "over-range index clamps to end"
        );
        // parent = None inserts at root.
        insert_subtree(&mut tree, None, 0, TestNode::leaf("root.rs", "root.rs"));
        assert_eq!(tree[0].id, "root.rs", "None parent inserts at root");
    }

    #[test]
    fn remove_then_insert_round_trips_a_subtree_to_a_new_parent() {
        // The reparent backbone: lift a subtree out and graft it elsewhere.
        let mut tree = sample();
        let moved = remove_subtree(&mut tree, "src/widgets").expect("removed");
        insert_subtree(&mut tree, Some("docs"), 0, moved);
        let docs = find_node_mut(&mut tree, "docs").expect("docs present");
        assert_eq!(
            docs.children[0].id, "src/widgets",
            "subtree reparented under docs"
        );
        assert_eq!(
            docs.children[0].children[0].id, "src/widgets/mod.rs",
            "with its child"
        );
        // src no longer holds it.
        let src = find_node_mut(&mut tree, "src").expect("src present");
        assert!(
            src.children.iter().all(|c| c.id != "src/widgets"),
            "removed from old parent"
        );
    }
}
