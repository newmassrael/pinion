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
//! shared [`clamp_nav`](super::virtual_select::clamp_nav) SSOT — clamp,
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

use super::virtual_select::clamp_nav;
use crate::reactive::batch;
use crate::Signal;
use serde::de::DeserializeOwned;
use serde::Serialize;

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

/// The visible-row index of `index`'s **parent**: the nearest earlier
/// row one level shallower. In a depth-first preorder flattening the
/// parent is always the closest preceding row at `depth − 1`, so Arrow
/// Left "ascend to parent" needs no separate tree walk — it reads the
/// same [`flat_visible`] sequence navigation already holds. `None` at
/// depth 0 (a root row has no parent).
#[must_use]
pub fn parent_row(rows: &[VisibleRow], index: usize) -> Option<usize> {
    let parent_depth = rows[index].depth.checked_sub(1)?;
    rows[..index].iter().rposition(|row| row.depth == parent_depth)
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
///   [`clamp_nav`](super::virtual_select::clamp_nav) SSOT (clamp, not
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
            let Some(i) = cursor else { return TreeKey::Consumed };
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
            let Some(i) = cursor else { return TreeKey::Consumed };
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
            let Some(i) = cursor else { return TreeKey::Consumed };
            if rows[i].has_children {
                TreeKey::Toggle(rows[i].id.clone())
            } else {
                TreeKey::Consumed // leaf
            }
        }
        _ => TreeKey::Unhandled,
    }
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

#[cfg(test)]
mod tests {
    //! R811 §5.50 §5.27 — the WAI-ARIA APG 6.13 Tree keyboard contract
    //! over the pure [`resolve_tree_key`] resolver (no reactive scope, so
    //! the keyboard semantics are unit-testable without a framework
    //! cache). Ported from `hello-tree-view`'s R809 nav tests when the
    //! resolver was lifted here at R811; the example now exercises only
    //! its `FileNode` [`TreeNode`] glue.

    use super::{
        find_node_mut, flat_visible, parent_row, resolve_tree_key, TreeKey, TreeNode, VisibleRow,
    };

    /// Page jump used by the tree consumers (mirrors `hello-tree-view`'s
    /// `NAV_PAGE`); larger than the sample listing so the page arm
    /// clamps to an end.
    const NAV_PAGE: usize = 7;

    /// A minimal [`TreeNode`] for the resolver tests, standing in for the
    /// real consumer node types (`FileNode` / `TreeItem`).
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
            ["src", "src/main.rs", "src/lib.rs", "src/widgets", "tests", "docs"],
            "flat visible order = depth-first preorder over expanded branches",
        );
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["src", "main.rs", "lib.rs", "widgets", "tests", "docs"]);
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
        assert_eq!((docs.depth, docs.position_in_set, docs.size_of_set), (0, 3, 3));
        assert!(docs.has_children && !docs.expanded, "docs is a collapsed branch");
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
        assert_eq!(key(Some("src"), "ArrowDown"), TreeKey::Focus("src/main.rs".into()));
    }

    #[test]
    fn arrow_up_moves_to_previous_visible_row() {
        assert_eq!(key(Some("src/lib.rs"), "ArrowUp"), TreeKey::Focus("src/main.rs".into()));
    }

    #[test]
    fn arrow_down_clamps_at_last_row_no_wrap() {
        assert_eq!(key(Some("docs"), "ArrowDown"), TreeKey::Focus("docs".into()));
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
        assert_eq!(key(Some("tests"), "ArrowRight"), TreeKey::Expand("tests".into()));
    }

    #[test]
    fn arrow_right_on_expanded_branch_descends_to_first_child() {
        assert_eq!(key(Some("src"), "ArrowRight"), TreeKey::Focus("src/main.rs".into()));
    }

    #[test]
    fn arrow_right_on_leaf_is_consumed_noop() {
        assert_eq!(key(Some("src/main.rs"), "ArrowRight"), TreeKey::Consumed);
    }

    #[test]
    fn arrow_left_on_expanded_branch_collapses() {
        assert_eq!(key(Some("src"), "ArrowLeft"), TreeKey::Collapse("src".into()));
    }

    #[test]
    fn arrow_left_on_collapsed_branch_ascends_to_parent() {
        assert_eq!(key(Some("src/widgets"), "ArrowLeft"), TreeKey::Focus("src".into()));
    }

    #[test]
    fn arrow_left_on_leaf_ascends_to_parent() {
        assert_eq!(key(Some("src/main.rs"), "ArrowLeft"), TreeKey::Focus("src".into()));
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
        assert!(find_node_mut(&mut nodes, "absent").is_none(), "missing id -> None");
    }

    // The `Signal<Vec<N>>` flag-store helpers (`set_expanded_in` /
    // `toggle_expanded` / the `apply_tree_key` bridge) are integration-
    // tested through their consumers rather than here: instantiating
    // `Signal<Vec<TestNode>>` inside pinion-core's own tests monomorphizes
    // serde's deserialization buffer (a >16KB array in generated code) and
    // trips `clippy::large_stack_arrays` at the crate root, where no scoped
    // `#[allow]` reaches. End-to-end coverage lives in `hello-tree-view`'s
    // `r809_tree_keyboard_waiaria` demo (every `apply_tree_key` arm) and
    // `hello-virtual-tree`'s `toggle_collapses_then_expands_a_branch` test;
    // the decision logic each composes (`resolve_tree_key` + the pure
    // `find_node_mut` above) is unit-tested here directly.
}
