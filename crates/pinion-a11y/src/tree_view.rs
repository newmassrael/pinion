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
use pinion_core::composite_tag::GridTag;
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

/// R863 §5.40 §5.27 §5.50 — WAI-ARIA `treegrid` + `row` + `rowheader` /
/// `gridcell` `AccessNode` builder: the columned analogue of
/// [`tree_access_nodes`] for [`view_virtual_treegrid`](pinion_widget_paint::tree_view::view_virtual_treegrid).
///
/// A `tree` / `treeitem` topology cannot expose the metadata columns — a
/// `treeitem` has no `gridcell` children in WAI-ARIA — so a hierarchical
/// outliner *with columns* (the self-hosted editor's scene outliner) lowers to
/// a `treegrid`: rows carry the tree disclosure axes (`aria-level` /
/// `aria-expanded` / `aria-posinset` / `aria-setsize`) **and** hold one
/// `gridcell` per column. The leading **name** column is the row's label, so it
/// lowers to a `rowheader`; the metadata columns lower to plain `gridcell`s.
///
/// Each `row` is painted across two frozen-split panes — the `{grid_tag}_drow{id}`
/// metadata strip (scrolling pane) and the `{grid_tag}#{id}` name cell (frozen
/// pane) — so the `row` node lists the name cell as a
/// [bounds-union fragment](AccessNode::bounds_union_tags) and the shell resolves
/// its bounds across both panes (the second `bounds_union_tags` consumer, after
/// the frozen data-grid). The header `row` spans the panes the same way
/// (`{grid_tag}_hrow` ∪ `{grid_tag}_fhrow`).
///
/// # Arguments
///
/// - `treegrid_tag` — the `role=treegrid` container node's tag: the focusable
///   element (`AccessKit::TreeUpdate::focus` lands here). Usually a separate
///   invisible root anchor (`hello-tree-grid`'s `"tgrid_root"`), distinct from
///   `grid_tag`.
/// - `grid_tag` — the paint-root tag every cell / header / strip tag derives
///   from: name cells `{grid_tag}#{id}`, metadata cells
///   `{grid_tag}_dcell{id}_{col}`, metadata-row strips `{grid_tag}_drow{id}`,
///   name header `{grid_tag}_chtree`, metadata headers `{grid_tag}_ch{col}`,
///   header bands `{grid_tag}_hrow` / `{grid_tag}_fhrow` — the same scheme
///   `view_virtual_treegrid` paints (the cross-crate [`GridTag`] SSOT).
/// - `name` — optional accessible name for the treegrid container.
/// - `tree_header` — the name column's header label.
/// - `data_headers` — the metadata column header labels; `data_headers.len()`
///   is the metadata column count.
/// - `rows` — the [`flat_visible`](pinion_core::widgets::tree_nav::flat_visible)
///   flattening of the visible tree (the SSOT the paint walk also consumes).
/// - `selected_id` — the row id carrying `aria-selected`, or `None`.
///
/// # Returns
///
/// `[treegrid, header_row, name_columnheader, metadata_columnheader…, (row,
/// rowheader, gridcell…)…]` — the container first, mirroring the flat
/// convention [`lower_access_node`](crate::tree::AccessTreeBuilder) resolves
/// into a tree. Gridcell names are left `None` so the shell's
/// [`enrich_names_from_scene`](crate::enrich_names_from_scene) derives them from
/// the painted cell text (scene-as-data: the value lives in one place).
#[must_use]
pub fn treegrid_nodes(
    treegrid_tag: &str,
    grid_tag: &str,
    name: Option<&str>,
    tree_header: &str,
    data_headers: &[&str],
    rows: &[VisibleRow],
    selected_id: Option<&str>,
) -> Vec<AccessNode> {
    let ncols = data_headers.len();
    // container + header row + (ncols + 1) columnheaders + per row (row +
    // rowheader + ncols gridcells).
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(rows.len() * (ncols + 2) + ncols + 3);

    // TreeGrid container: references the header row + every visible data row.
    let mut grid = AccessNode::new(treegrid_tag, AriaRole::TreeGrid);
    if let Some(name) = name {
        grid = grid.with_name(name);
    }
    grid = grid.with_child(GridTag::header_row(grid_tag));
    for row in rows {
        grid = grid.with_child(GridTag::metadata_row(grid_tag, &row.id));
    }
    nodes.push(grid);

    // Header `row`: the metadata header band (`{grid_tag}_hrow`) ∪ the frozen
    // name header band (`{grid_tag}_fhrow`), with the name columnheader first.
    let mut hrow = AccessNode::new(GridTag::header_row(grid_tag), AriaRole::Row)
        .with_bounds_union_tag(GridTag::frozen_header_row(grid_tag))
        .with_child(GridTag::tree_col_header(grid_tag));
    for col in 0..ncols {
        hrow = hrow.with_child(GridTag::col_header(grid_tag, col));
    }
    nodes.push(hrow);
    nodes.push(
        AccessNode::new(GridTag::tree_col_header(grid_tag), AriaRole::ColumnHeader)
            .with_name(tree_header),
    );
    for (col, label) in data_headers.iter().enumerate() {
        nodes.push(
            AccessNode::new(GridTag::col_header(grid_tag, col), AriaRole::ColumnHeader)
                .with_name(*label),
        );
    }

    // Windowed data rows: each `row` carries the tree disclosure axes + a
    // bounds span over both panes, holding a `rowheader` (name) + one
    // `gridcell` per metadata column.
    for row in rows {
        let row_tag = GridTag::metadata_row(grid_tag, &row.id);
        let name_cell = tree_row_tag(grid_tag, &row.id);
        let mut row_node = AccessNode::new(&row_tag, AriaRole::Row)
            // R863 — span the frozen name cell (the other pane fragment).
            .with_bounds_union_tag(&name_cell)
            // WAI-ARIA 1.2 6.6.8 / 6.6.9 / 6.6.10 — one-based depth + sibling
            // index + count (a treegrid row carries the tree axes itself).
            .with_level(row.depth + 1)
            .with_position_in_set(row.position_in_set)
            .with_size_of_set(row.size_of_set)
            .with_child(&name_cell);
        for col in 0..ncols {
            row_node = row_node.with_child(GridTag::metadata_cell(grid_tag, &row.id, col));
        }
        // 6.6.3 — disclosure state on branches only; leaves omit it.
        if row.has_children {
            row_node = row_node.with_expanded(row.expanded);
        }
        if selected_id == Some(row.id.as_str()) {
            row_node = row_node.with_selected(true);
        }
        nodes.push(row_node);
        // The name cell is the row's label → `rowheader` (the row analogue of
        // a columnheader); the explicit name mirrors the painted label.
        nodes.push(AccessNode::new(&name_cell, AriaRole::RowHeader).with_name(row.label.as_str()));
        // Metadata `gridcell`s — names left to `enrich_names_from_scene` (the
        // painted cell text is the single source of truth).
        for col in 0..ncols {
            nodes.push(AccessNode::new(
                GridTag::metadata_cell(grid_tag, &row.id, col),
                AriaRole::GridCell,
            ));
        }
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

    // ── R863 §5.40 §5.27 — treegrid (columned tree) builder ──────────

    use super::treegrid_nodes;

    const DATA_HEADERS: [&str; 3] = ["Type", "Visible", "Layer"];

    /// `[treegrid, header_row, name_columnheader, 3 metadata columnheaders,
    /// (row, rowheader, 3 gridcells) × rows]`.
    fn tg_nodes() -> Vec<AccessNode> {
        treegrid_nodes("tg_root", "tg", Some("Scene outliner"), "Name", &DATA_HEADERS, &sample_rows(), None)
    }

    #[test]
    fn treegrid_emits_container_header_and_one_row_block_per_visible_row() {
        let out = tg_nodes();
        // 1 treegrid + 1 header row + (1 name + 3 metadata) columnheaders +
        // 5 rows × (1 row + 1 rowheader + 3 gridcells).
        assert_eq!(out.len(), 1 + 1 + 4 + sample_rows().len() * 5);
        assert_eq!(out[0].role, AriaRole::TreeGrid, "container is a treegrid, not a tree");
        assert_eq!(out[0].name.as_deref(), Some("Scene outliner"));
        // The container references the header row + every visible data-row strip.
        assert_eq!(out[0].children[0], "tg_hrow");
        assert_eq!(out[0].children[1], "tg_drowsrc");
        assert_eq!(out[0].children.len(), 1 + sample_rows().len());
    }

    #[test]
    fn treegrid_rows_span_both_panes_via_bounds_union() {
        // The defining R863 fix: each row is painted across the scrolling
        // metadata strip (own tag) + the frozen name cell (union fragment), so
        // the AT row bounds cover the name column, not just the metadata.
        let out = tg_nodes();
        let hrow = out.iter().find(|n| n.tag == "tg_hrow").unwrap();
        assert_eq!(hrow.bounds_union_tags, vec!["tg_fhrow"], "header row spans both header bands");
        let row = out.iter().find(|n| n.tag == "tg_drowsrc").unwrap();
        assert_eq!(row.role, AriaRole::Row);
        assert_eq!(row.bounds_union_tags, vec!["tg#src"], "data row spans the frozen name cell");
    }

    #[test]
    fn treegrid_name_cell_is_rowheader_metadata_are_gridcells() {
        let out = tg_nodes();
        // The name cell (`tg#src`) is the row's label → rowheader.
        let name = out.iter().find(|n| n.tag == "tg#src").unwrap();
        assert_eq!(name.role, AriaRole::RowHeader);
        assert_eq!(name.name.as_deref(), Some("src"), "rowheader name mirrors the label");
        // The metadata cells are gridcells, named by enrichment (None here).
        let cell0 = out.iter().find(|n| n.tag == "tg_dcellsrc_0").unwrap();
        assert_eq!(cell0.role, AriaRole::GridCell);
        assert!(cell0.name.is_none(), "gridcell name comes from the painted text, not the builder");
        // The row references the rowheader first, then one gridcell per column.
        let row = out.iter().find(|n| n.tag == "tg_drowsrc").unwrap();
        assert_eq!(row.children, vec!["tg#src", "tg_dcellsrc_0", "tg_dcellsrc_1", "tg_dcellsrc_2"]);
    }

    #[test]
    fn treegrid_rows_carry_the_tree_disclosure_axes() {
        // A treegrid row IS the tree node: it carries level / posinset /
        // setsize, and aria-expanded on branches only (leaves omit it).
        let out = tg_nodes();
        let by_tag = |tag: &str| out.iter().find(|n| n.tag == tag).expect("row present");
        let src = by_tag("tg_drowsrc");
        assert_eq!(src.level, Some(1), "root row is level 1");
        assert_eq!((src.position_in_set, src.size_of_set), (Some(1), Some(2)));
        assert_eq!(src.expanded, Some(true), "expanded branch");
        let mod_rs = by_tag("tg_drowsrc/widgets/mod.rs");
        assert_eq!(mod_rs.level, Some(3));
        assert_eq!(mod_rs.expanded, None, "a leaf omits aria-expanded");
        let docs = by_tag("tg_drowdocs");
        assert_eq!(docs.expanded, Some(false), "collapsed branch");
    }

    #[test]
    fn treegrid_columnheaders_cover_name_then_metadata() {
        let out = tg_nodes();
        let headers: Vec<(&str, &str)> = out
            .iter()
            .filter(|n| n.role == AriaRole::ColumnHeader)
            .map(|n| (n.tag.as_str(), n.name.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(
            headers,
            vec![
                ("tg_chtree", "Name"),
                ("tg_ch0", "Type"),
                ("tg_ch1", "Visible"),
                ("tg_ch2", "Layer"),
            ],
            "name column header first, then metadata headers in order",
        );
        // The header row references them in the same order.
        let hrow = out.iter().find(|n| n.tag == "tg_hrow").unwrap();
        assert_eq!(hrow.children, vec!["tg_chtree", "tg_ch0", "tg_ch1", "tg_ch2"]);
    }

    #[test]
    fn treegrid_selected_row_carries_aria_selected_others_omit_it() {
        let out =
            treegrid_nodes("tg_root", "tg", None, "Name", &DATA_HEADERS, &sample_rows(), Some("src/widgets"));
        let by_tag = |tag: &str| out.iter().find(|n| n.tag == tag).expect("row present");
        assert_eq!(by_tag("tg_drowsrc/widgets").selected, Some(true), "selected row");
        assert_eq!(by_tag("tg_drowsrc").selected, None, "non-selected row omits the axis");
    }

    #[test]
    fn treegrid_empty_rows_emit_container_and_headers_only() {
        let out = treegrid_nodes("tg_root", "tg", None, "Name", &DATA_HEADERS, &[], None);
        // container + header row + (1 name + 3 metadata) columnheaders.
        assert_eq!(out.len(), 1 + 1 + 4);
        assert_eq!(out[0].role, AriaRole::TreeGrid);
        assert_eq!(out[0].children, vec!["tg_hrow"], "no data rows referenced");
        assert!(out.iter().all(|n| n.role != AriaRole::Row || n.tag == "tg_hrow"));
    }
}
