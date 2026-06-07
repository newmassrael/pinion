//! R812 §5.40 §5.50 §5.27 — WAI-ARIA `tree` + `treeitem` `AccessNode`
//! builder.
//!
//! Lifted from `hello-tree-view`'s hand-rolled `access_rows` at its second
//! tree consumer: the `hello-dock-panels` `DevTools` inspector tree, which
//! (together with `hello-multi-window`'s inspector) previously emitted zero
//! tree AT nodes despite painting the very same
//! [`view_tree`](pinion_widget_paint::tree_view) row sequence. The tree was
//! the only interactive widget family without a lifted a11y node builder —
//! [`navigation_link_nodes`](crate::navigation_link_nodes),
//! [`toggle_button_group_nodes`](crate::toggle_button_group_nodes),
//! [`radiogroup_radio_nodes`](crate::radiogroup_radio_nodes),
//! [`toolbar_button_nodes`](crate::toolbar_button_nodes),
//! [`windowed_list_nodes`](crate::windowed_list_nodes) and
//! [`windowed_grid_nodes`](crate::windowed_grid_nodes) all already had one.
//! A divergence between two tree consumers' AT topology would be an a11y
//! bug, not a style choice, so the canonical shape is lifted here as one
//! source of truth (the R758 / R759 a11y-axis "divergence-is-a-bug" rule,
//! applied at the second consumer).
//!
//! ## Shape
//!
//! `[tree_root, treeitem_0, treeitem_1, ...]` — the `role=tree` container
//! first (referencing every visible row as a child so the AT topology
//! mirrors the painted topology), then one `role=treeitem` per visible
//! row. The rows come straight from the
//! [`flat_visible`](pinion_core::widgets::tree_nav::flat_visible) SSOT, so
//! the AT announces exactly the row set the user sees and the keyboard
//! cursor navigates — the same flattening the paint walk consumes
//! (the R811.1 single-traversal invariant, now shared across consumers).
//! The root-first flat-list order mirrors the convention
//! [`lower_access_node`](crate::tree::AccessTreeBuilder) resolves into an
//! AT subtree (identical to [`navigation_link_nodes`](crate::navigation_link_nodes)).
//!
//! ## WAI-ARIA axes
//!
//! Each row carries the hierarchical axes WAI-ARIA 1.2 requires authors to
//! supply on a custom `role=treeitem` (AT does **not** infer them from DOM
//! nesting): `aria-level` (6.6.8, one-based = `depth + 1`),
//! `aria-posinset` (6.6.9), `aria-setsize` (6.6.10), and `aria-expanded`
//! (6.6.3 — branches only; leaves omit it). A single-select tree adds
//! `aria-selected` on the cursor / selected row; a tree with no selection
//! model passes `None` and no row carries the attribute.

use crate::node::AccessNode;
use crate::role::AriaRole;
use pinion_core::widgets::tree_nav::VisibleRow;

/// Compose a row's composite `AccessNode` tag — the frozen R51.42
/// `{row_prefix}#{id}` separator the paint substrate stamps
/// (`pinion_widget_paint::tree_view::composite_row_tag`) and the hit-test
/// router parses, so the AT `NodeId` hashes through the same key as the
/// click target. Encode is the accepted trivial-join idiom per the R803
/// composite-tag decision (only the decode side warrants a named parser),
/// so it is inlined here rather than reaching across into
/// `pinion-widget-paint`.
fn tree_row_tag(row_prefix: &str, id: &str) -> String {
    format!("{row_prefix}#{id}")
}

/// Build the `tree` root + one `treeitem` per visible row.
///
/// # Arguments
///
/// - `tree_tag` — the `role=tree` root node's tag: the focusable element
///   (the tab stop a binding lists in `focusable_tags`). AccessKit's
///   `TreeUpdate::focus` lands here while `aria-activedescendant` names the
///   cursor row.
/// - `row_prefix` — the per-row composite-tag namespace; each row becomes
///   `{row_prefix}#{id}`. Usually equal to `tree_tag` (the painted tree
///   container is itself the tab stop, e.g. the dock-panels inspector);
///   they differ when a binding makes a separate element the tab stop
///   (e.g. `hello-tree-view`'s invisible-root `External` `"tree_root"`
///   vs the `view_tree` container `"file_tree"`).
/// - `name` — optional accessible name for the tree container.
/// - `rows` — the [`flat_visible`](pinion_core::widgets::tree_nav::flat_visible)
///   flattening of the visible tree (the SSOT every tree axis reads).
/// - `selected_id` — the row id carrying `aria-selected` (the single-select
///   cursor / selection-follows-focus row), or `None` for a tree with no
///   selection model.
///
/// # Returns
///
/// `[tree_root, ...treeitems]` — root first. Bounds are filled in by the
/// shell after layout (a11y builders never resolve pixel rects).
#[must_use]
pub fn tree_access_nodes(
    tree_tag: &str,
    row_prefix: &str,
    name: Option<&str>,
    rows: &[VisibleRow],
    selected_id: Option<&str>,
) -> Vec<AccessNode> {
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(rows.len() + 1);
    let mut root = AccessNode::new(tree_tag, AriaRole::Tree);
    if let Some(name) = name {
        root = root.with_name(name);
    }
    for row in rows {
        root = root.with_child(tree_row_tag(row_prefix, &row.id));
    }
    nodes.push(root);
    for row in rows {
        let mut node = AccessNode::new(tree_row_tag(row_prefix, &row.id), AriaRole::TreeItem)
            .with_name(row.label.as_str())
            // WAI-ARIA 1.2 6.6.8 — one-based depth (root rows → 1).
            .with_level(row.depth + 1)
            .with_position_in_set(row.position_in_set)
            .with_size_of_set(row.size_of_set);
        // 6.6.3 — disclosure state on branches only; leaves omit it.
        if row.has_children {
            node = node.with_expanded(row.expanded);
        }
        // Single-select tree: only the cursor / selected row is selected.
        if selected_id == Some(row.id.as_str()) {
            node = node.with_selected(true);
        }
        nodes.push(node);
    }
    nodes
}

#[cfg(test)]
mod tests {
    //! R812 §5.40 §5.50 — the WAI-ARIA `tree` / `treeitem` lowering
    //! contract over [`tree_access_nodes`]. Ported from
    //! `hello-tree-view`'s R674 / R809.1 `access_rows` tests when the
    //! builder was lifted here at R812; the example now keeps only a
    //! binding-side composite-format lockstep pin.

    use super::tree_access_nodes;
    use crate::node::AccessNode;
    use crate::role::AriaRole;
    use pinion_core::widgets::tree_nav::VisibleRow;

    /// A deterministic visible-row flattening, mirroring the
    /// `hello-tree-view` R674 sample tree:
    /// ```text
    /// src         (branch, expanded)        level 1, 1 of 2
    ///   main.rs   (leaf)                     level 2, 1 of 2
    ///   widgets   (branch, expanded)         level 2, 2 of 2
    ///     mod.rs  (leaf)                      level 3, 1 of 1
    /// docs        (branch, collapsed)        level 1, 2 of 2
    /// ```
    fn sample_rows() -> Vec<VisibleRow> {
        vec![
            VisibleRow {
                id: "src".into(),
                label: "src".into(),
                depth: 0,
                position_in_set: 1,
                size_of_set: 2,
                has_children: true,
                expanded: true,
            },
            VisibleRow {
                id: "src/main.rs".into(),
                label: "main.rs".into(),
                depth: 1,
                position_in_set: 1,
                size_of_set: 2,
                has_children: false,
                expanded: false,
            },
            VisibleRow {
                id: "src/widgets".into(),
                label: "widgets".into(),
                depth: 1,
                position_in_set: 2,
                size_of_set: 2,
                has_children: true,
                expanded: true,
            },
            VisibleRow {
                id: "src/widgets/mod.rs".into(),
                label: "mod.rs".into(),
                depth: 2,
                position_in_set: 1,
                size_of_set: 1,
                has_children: false,
                expanded: false,
            },
            VisibleRow {
                id: "docs".into(),
                label: "docs".into(),
                depth: 0,
                position_in_set: 2,
                size_of_set: 2,
                has_children: true,
                expanded: false,
            },
        ]
    }

    fn nodes() -> Vec<AccessNode> {
        tree_access_nodes("tree", "tree", None, &sample_rows(), None)
    }

    #[test]
    fn emits_tree_root_then_one_treeitem_per_row() {
        let out = nodes();
        assert_eq!(out.len(), sample_rows().len() + 1, "root + one node per row");
        assert_eq!(out[0].role, AriaRole::Tree, "first node is the tree root");
        for row in &out[1..] {
            assert_eq!(row.role, AriaRole::TreeItem, "every child is a treeitem");
        }
    }

    #[test]
    fn root_references_every_row_in_paint_order() {
        let out = nodes();
        assert_eq!(
            out[0].children,
            vec![
                "tree#src",
                "tree#src/main.rs",
                "tree#src/widgets",
                "tree#src/widgets/mod.rs",
                "tree#docs",
            ],
            "root lists every visible row as a child, in depth-first preorder",
        );
    }

    #[test]
    fn row_tags_are_composite_namespaced() {
        let out = nodes();
        let tags: Vec<&str> = out[1..].iter().map(|n| n.tag.as_str()).collect();
        assert_eq!(
            tags,
            ["tree#src", "tree#src/main.rs", "tree#src/widgets", "tree#src/widgets/mod.rs", "tree#docs"],
        );
    }

    #[test]
    fn level_is_depth_plus_one_one_based() {
        // WAI-ARIA 1.2 6.6.8 — root rows are level 1, not 0.
        let out = nodes();
        let by_tag = |tag: &str| out.iter().find(|n| n.tag == tag).expect("row present");
        assert_eq!(by_tag("tree#src").level, Some(1));
        assert_eq!(by_tag("tree#docs").level, Some(1));
        assert_eq!(by_tag("tree#src/main.rs").level, Some(2));
        assert_eq!(by_tag("tree#src/widgets").level, Some(2));
        assert_eq!(by_tag("tree#src/widgets/mod.rs").level, Some(3));
    }

    #[test]
    fn posinset_and_setsize_mirror_the_visible_row() {
        // WAI-ARIA 1.2 6.6.9 / 6.6.10 — one-based sibling index + count.
        let out = nodes();
        let by_tag = |tag: &str| out.iter().find(|n| n.tag == tag).expect("row present");
        let src = by_tag("tree#src");
        assert_eq!((src.position_in_set, src.size_of_set), (Some(1), Some(2)));
        let widgets = by_tag("tree#src/widgets");
        assert_eq!((widgets.position_in_set, widgets.size_of_set), (Some(2), Some(2)));
        let mod_rs = by_tag("tree#src/widgets/mod.rs");
        assert_eq!((mod_rs.position_in_set, mod_rs.size_of_set), (Some(1), Some(1)));
    }

    #[test]
    fn name_mirrors_label() {
        let out = nodes();
        let by_tag = |tag: &str| out.iter().find(|n| n.tag == tag).expect("row present");
        assert_eq!(by_tag("tree#src").name.as_deref(), Some("src"));
        assert_eq!(by_tag("tree#src/main.rs").name.as_deref(), Some("main.rs"));
        assert_eq!(by_tag("tree#src/widgets/mod.rs").name.as_deref(), Some("mod.rs"));
    }

    #[test]
    fn branches_carry_aria_expanded_leaves_omit_it() {
        // WAI-ARIA 1.2 6.6.3 — expandable rows expose aria-expanded; a
        // leaf omits it. The flag rides the same flat_visible row the
        // keyboard toggles, so AT state can never disagree with the glyph.
        let out = nodes();
        let by_tag = |tag: &str| out.iter().find(|n| n.tag == tag).expect("row present");
        assert_eq!(by_tag("tree#src").expanded, Some(true));
        assert_eq!(by_tag("tree#src/widgets").expanded, Some(true));
        assert_eq!(by_tag("tree#docs").expanded, Some(false), "collapsed branch");
        assert_eq!(by_tag("tree#src/main.rs").expanded, None, "leaf omits aria-expanded");
        assert_eq!(by_tag("tree#src/widgets/mod.rs").expanded, None);
    }

    #[test]
    fn selected_row_carries_aria_selected_others_omit_it() {
        // Single-select tree: only the named cursor row is aria-selected.
        let out = tree_access_nodes("tree", "tree", None, &sample_rows(), Some("src/widgets"));
        let by_tag = |tag: &str| out.iter().find(|n| n.tag == tag).expect("row present");
        assert_eq!(by_tag("tree#src/widgets").selected, Some(true), "cursor row selected");
        assert_eq!(by_tag("tree#src").selected, None, "non-cursor row omits aria-selected");
        assert_eq!(by_tag("tree#docs").selected, None);
    }

    #[test]
    fn no_selection_leaves_every_row_unselected() {
        for node in &nodes()[1..] {
            assert_eq!(node.selected, None, "None selected_id → no aria-selected anywhere");
        }
    }

    #[test]
    fn collapsed_branch_hides_descendants_from_at_tree() {
        // The flattening already drops a collapsed branch's children, so
        // the AT announces only what the user sees.
        let out = nodes();
        let tags: Vec<&str> = out.iter().map(|n| n.tag.as_str()).collect();
        assert!(tags.contains(&"tree#docs"), "the collapsed branch itself stays visible");
        assert!(!tags.iter().any(|t| t.starts_with("tree#docs/")), "its descendants are hidden");
    }

    #[test]
    fn distinct_root_tag_and_row_prefix() {
        // hello-tree-view shape: a separate focusable root External
        // ("tree_root") vs the painted tree container's row prefix
        // ("file_tree"). The Tree node carries the root tag; rows carry
        // the prefix.
        let out = tree_access_nodes("tree_root", "file_tree", None, &sample_rows(), None);
        assert_eq!(out[0].tag, "tree_root", "Tree root node uses the focusable tag");
        assert_eq!(out[1].tag, "file_tree#src", "rows use the composite prefix");
        assert_eq!(out[0].children[0], "file_tree#src", "root references the prefixed row tags");
    }

    #[test]
    fn optional_name_lowers_when_present() {
        let named = tree_access_nodes("tree", "tree", Some("Scene inspector"), &sample_rows(), None);
        assert_eq!(named[0].name.as_deref(), Some("Scene inspector"));
        assert_eq!(nodes()[0].name, None, "no name when None");
    }

    #[test]
    fn empty_rows_emit_tree_root_only() {
        let out = tree_access_nodes("tree", "tree", None, &[], None);
        assert_eq!(out.len(), 1, "empty tree → just the role=tree root");
        assert_eq!(out[0].role, AriaRole::Tree);
        assert!(out[0].children.is_empty(), "root references no rows");
    }
}
