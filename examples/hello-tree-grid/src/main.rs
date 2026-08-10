//! `hello-tree-grid` — R860 §5.27 §5.50 **tree-grid (scene-outliner)**.
//!
//! A hierarchical outliner whose **frozen name column** (indent + expand
//! glyph + label) is pinned via the R859 frozen-split substrate while the
//! **metadata columns** (Type / Visible / Layer) scroll horizontally. Both
//! panes share the vertical body scroll through the R859 linked-scroll
//! [follower](pinion_core::scene::ScrollNode::as_follower), so the tree and
//! its metadata scroll in vertical lockstep. This is the self-hosted
//! editor's scene-outliner shape — the #1 Phase-B UI need.
//!
//! It is the first consumer of
//! [`pinion_widget_paint::tree_view::view_virtual_treegrid`], composing the
//! R819 tree virtualization ([`flat_visible`] windowing) with the R859
//! frozen-column data-grid.
//!
//! ## Interaction
//!
//! Click a folder row (the `{TREE_TAG}#{id}` composite-tag name cell, the
//! R674 [`TreeRowClickExternal`] path) to expand / collapse it; the visible
//! row count changes and the grid re-windows. Clicking also **selects** the row
//! (replace; highlighted across both panes). R864 — the outliner is
//! keyboard-navigable (single tab stop + `aria-activedescendant` roving cursor):
//! Arrow Up/Down move the cursor, Arrow Right expands / descends, Arrow Left
//! collapses / ascends, Enter/Space toggle, and type-ahead jumps by name — the
//! lifted `apply_tree_key` + `tree_typeahead_jump` substrate, with the cursor
//! scrolled into the body window (keyboard ⊥ virtualization). The metadata
//! columns are display cells the row cursor rides past, not separate tab
//! stops (an outliner navigates by row, not by cell).
//!
//! R902 — **multi-select** (the scene outliner's "select N assets → batch
//! rename / delete"): the selection is a **set** of rows by stable id (robust
//! across expand / collapse, where a flat index would shift), decoupled from
//! the keyboard cursor. Plain nav replaces the selection with the new cursor
//! (selection-follows-focus); `Shift`+nav extends the contiguous range from the
//! anchor; `Ctrl+Space` toggles the cursor row's membership; `Ctrl+A` selects
//! every visible row — the chord decode is the shared [`SelectionChord`] policy
//! (R880.1), so the keyboard matches the list/grid Model/View widgets. The
//! `query`/`intervene("selection")` + `invoke` ops on [`SELECT_TAG`] give the AI
//! the same multi-select through RPC (one funnel; §2 primary path). Modifier
//! pointer-clicks (`Ctrl`/`Shift`-click) are deferred — `TreeRowClickExternal`
//! is model-agnostic and carries only the bare id, so chord-clicks need it to
//! forward modifiers (a wire-symmetry change across the four tree consumers),
//! landing at the 2nd chord-click consumer.
//!
//! ## The witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` reports the windowed `{TREE_TAG}#{id}` name cells +
//! `{TREE_TAG}_drow{id}` metadata strips. `scene/scroll` on the horizontal
//! scroll (`tgrid_hscroll`) shifts the metadata columns left while the name
//! column stays put (the freeze); `scene/scroll` on the body (`tgrid_scroll`)
//! slides BOTH panes in lockstep. `scene/query` on the [`TREE_STATE_TAG`]
//! query-only introspection External reports the FULL visible-row count + per
//! row `id_at` / `level_at` / `expanded_at` (the virtualization sees only a
//! window, so the AI reads structure here, not from the painted nodes); R865 —
//! the [`CELLS_TAG`] peer reports `cell_at.<pos>.<col>` so the AI also reads the
//! off-window metadata (Type / Visible / Layer) the paint window cannot expose.
//! R902 — `scene/query` on [`SELECT_TAG`] reports the selection set
//! (`selection` JSON array of ids + `count` + `anchor`), the FULL set including
//! members hidden inside a collapsed branch; `scene/intervene("selection", […])`
//! restores it and `scene/invoke` drives `select` / `toggle` / `extend_to` /
//! `select_all` / `clear`. See `tools/demos/r860_tree_grid.py` +
//! `tools/demos/r902_tree_multi_select.py`.
//!
//! ## a11y (R863)
//!
//! The binding supplies a WAI-ARIA `treegrid` via the shared
//! [`treegrid_nodes`]: each `row` carries the tree disclosure axes
//! (`aria-level` / `aria-expanded` / `aria-posinset` / `aria-setsize`) **and**
//! holds a `rowheader` (the `{TREE_TAG}#{id}` name cell) + one `gridcell` per
//! metadata column, with the row's AT bounds spanning the frozen name pane +
//! the scrolling metadata pane (the R863 [`AccessNode::bounds_union_tags`]
//! substrate). This resolves the R860 carry — the metadata columns were
//! AT-invisible under the prior `tree` / `treeitem` topology (a `treeitem` has
//! no `gridcell` children in WAI-ARIA). R865 — the AT tree is **windowed** to
//! the rendered slice (the same `compute_visible_range` the paint uses), so it
//! exposes exactly the realized rows, not bounds-less off-window ghosts. R902 —
//! the treegrid is `aria-multiselectable`; each rendered row carries explicit
//! `aria-selected = set membership` (several at once), and the keyboard cursor
//! (decoupled — `Ctrl+Space` can leave it on a deselected row) is the
//! `aria-activedescendant`.

use pinion_a11y::{AccessFocus, AccessNode, TreeGridSelection, WidgetA11y, treegrid_nodes};
use pinion_core::composite_tag::{GridTag, split_send_payload};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, QueryOnlyIntrospect, QuerySource, RepaintOwner,
    SchemaArg, SchemaField, StubExternal, ThreadOwnership,
};
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::reactive::batch;
use pinion_core::scene::ContainerNode;
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::undo::{UndoCommand, UndoStack, undo_redo_verb, use_undo_stack};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::tree_nav::{
    MutableTreeNode, TreeNode, VisibleRow, find_node_mut, flat_visible, insert_subtree,
    remove_subtree, toggle_expanded, tree_view_introspection_extra,
};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{Frame, Modifiers, Owner, Scene, SelectionChord, Signal, WidgetCore};
use pinion_shell::typeahead::apply_windowed_tree_key;
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::table::GridScroll;
use pinion_widget_paint::tree_view::{
    TreeGridData, TreeRowClickExternal, TreeViewStyle, view_virtual_treegrid,
};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTreeGridRenderer, HelloTreeGridRendererError);

/// Initial window size. Narrower than the tree column + all metadata
/// columns so the metadata overflows and scrolls while the name column
/// stays pinned.
const WIN_W: u32 = 480;
const WIN_H: u32 = 460;
/// Shared [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key.
const THEME_TAG: &str = "app";
/// Composite-tag prefix the name cells carry (`{TREE_TAG}#{id}`) and the
/// [`TreeRowClickExternal`] anchor clicks route to.
const TREE_TAG: &str = "tgrid";
/// Invisible focusable tree-root anchor (the WAI-ARIA `tree` node lands
/// here); kept distinct from [`TREE_TAG`] like `hello-virtual-tree`.
const ROOT_TAG: &str = "tgrid_root";
/// Query-only tree-state introspection External: for a *virtualized* tree
/// this is the only way the AI reads the full structure (only a window
/// paints).
const TREE_STATE_TAG: &str = "tgrid_state";
/// R865 §5.12 §5.50 — query-only introspection of the metadata CELL values.
/// The off-window read an AI needs: the virtualization paints only a window
/// and [`TREE_STATE_TAG`] reports only row STRUCTURE (id / level / expanded),
/// not the per-column metadata, so an AI inspecting an off-window object could
/// not read its Type / Visible / Layer. Binding-local because the `cell_data`
/// derivation is binding-specific (the shared `pinion-core` tree introspection
/// stays node-type agnostic until a 2nd tree-grid consumer surfaces it —
/// `[[abstraction-needs-second-consumer]]`).
const CELLS_TAG: &str = "tgrid_cells";
/// R902 §5.40 §5.12 — query + write introspection of the multi-select state:
/// the selection **set** (by stable id), its count, and the range anchor. The
/// AI-first ([[ai-first-rpc-introspection-obligation]]) read/write peer of the
/// keyboard chords — `query`/`intervene("selection")` mirror each other and the
/// `invoke` ops (`select` / `toggle` / `extend_to` / `select_all` / `clear`)
/// drive the same funnel the keyboard does, so RPC and keyboard multi-select
/// are one SSOT. Binding-local: the stable-id `BTreeSet<String>` selection is a
/// 1st consumer (the flat-index `VirtualSelectExternal` cannot be reused — a
/// tree row's flat index shifts on every expand / collapse), so the model stays
/// in the example until a 2nd tree-multi-select consumer surfaces it
/// ([[abstraction-needs-second-consumer]]).
const SELECT_TAG: &str = "tgrid_select";
/// Input-router tag for the vertical body `ScrollState` (shared by both
/// panes).
const SCROLL_KEY: &str = "tgrid_scroll";
/// Input-router tag for the horizontal `ScrollState` (the metadata columns).
const H_SCROLL_KEY: &str = "tgrid_hscroll";
/// The dotted wire form of the [`TreeRowClickExternal`] row-click intent.
const CLICK_INTENT_TAG: &str = intent_tag!("tgrid", "click");

/// Metadata column headers (the scrolling pane).
const DATA_HEADERS: [&str; 3] = ["Type", "Visible", "Layer"];
/// The frozen name-column header label (shared by the paint + the a11y
/// `treegrid` so the columnheader name matches the painted header).
const TREE_HEADER: &str = "Name";
/// Frozen tree (name) column width.
const TREE_COL_W: u32 = 200;
/// Each scrolling metadata column width. `3 × 160 = 480` metadata px against
/// a `480 − 200 = 280`px scrolled viewport gives a `200`px horizontal scroll
/// range — wide enough that the freeze (and a scroll-to-max revealing the
/// rightmost column) is a meaningful witness, not a token overflow.
const DATA_COL_W: u32 = 160;
/// Rows built beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Top-level scene folders.
const FOLDERS: usize = 24;
/// Object leaves per folder.
const OBJECTS_PER: usize = 12;
/// Folders expanded at boot (so the visible row count starts well above the
/// window — the virtualization is obvious from frame one).
const EXPANDED_AT_BOOT: usize = 3;
/// Uniform per-row vertical slot pitch. Must equal
/// [`TreeViewStyle::row_height`] (`view_virtual_treegrid` derives its slot
/// pitch from the style); asserted in `tests` and used by the keyboard
/// scroll-into-view (`reveal_cursor`).
const ROW_PITCH: u32 = 48;
/// Page Up / Down jump in rows (a viewport-ful, clamped via `clamp_nav`).
const NAV_PAGE: usize = 10;
/// Owner-cache key for the type-ahead cursor (buffer + last-typed instant),
/// caller-side per the `tree_nav` purity boundary (R811).
const TYPEAHEAD_KEY: &str = "hello_tree_grid::typeahead";

/// One outliner node — a scene folder or object. Carries its own `expanded`
/// flag (the retained flag-on-node storage). The `serde` + `PartialEq`
/// derives satisfy the §5.22 introspect bound `Signal<T>` carries.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct OutlinerNode {
    id: String,
    label: String,
    expanded: bool,
    children: Vec<OutlinerNode>,
}

impl OutlinerNode {
    fn leaf(id: String, label: String) -> Self {
        Self {
            id,
            label,
            expanded: false,
            children: Vec::new(),
        }
    }
}

impl TreeNode for OutlinerNode {
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

impl MutableTreeNode for OutlinerNode {
    fn children_vec_mut(&mut self) -> &mut Vec<Self> {
        &mut self.children
    }
}

/// Build the synthetic outliner. Deterministic ids (`f{folder}` /
/// `f{folder}-o{object}`) so the RPC demo can address rows by a stable
/// composite tag, and so [`cell_data`] derives stable metadata from the id.
fn initial_nodes() -> Vec<OutlinerNode> {
    (0..FOLDERS)
        .map(|f| {
            let children = (0..OBJECTS_PER)
                .map(|o| OutlinerNode::leaf(format!("f{f}-o{o}"), format!("Object {f:02}-{o:02}")))
                .collect();
            OutlinerNode {
                id: format!("f{f}"),
                label: format!("Folder {f:02}"),
                expanded: f < EXPANDED_AT_BOOT,
                children,
            }
        })
        .collect()
}

/// Metadata for a row, derived deterministically from its id (a folder id
/// has no `-`; an object id is `f{f}-o{o}`). A real editor reads these off
/// the scene object; the synthetic derivation keeps the demo dependency-free.
fn cell_data(id: &str, col: usize) -> String {
    let hash: u32 = id.bytes().map(u32::from).sum();
    let is_folder = !id.contains('-');
    match col {
        0 => {
            if is_folder {
                "Folder".to_string()
            } else {
                ["Mesh", "Light", "Camera"][usize::try_from(hash % 3).unwrap_or(0)].to_string()
            }
        }
        1 => if hash % 2 == 0 { "Yes" } else { "No" }.to_string(),
        _ => format!("L{}", hash % 4),
    }
}

/// R865 §5.12 §5.50 — the [`CELLS_TAG`] query-only introspection source: the
/// metadata cell values keyed by `(visible position, column)`, read fresh on
/// every query from the same retained tree the view windows over. It is the
/// metadata peer of the shared structural [`tree_view_introspection_extra`]:
/// where that reports `id_at` / `level_at` / `expanded_at`, this reports
/// `cell_at` so an AI reads the off-window Type / Visible / Layer the paint
/// window cannot expose ([[ai-first-rpc-introspection-obligation]]).
struct CellsIntrospect {
    rows: Box<dyn Fn() -> Vec<VisibleRow>>,
}

impl core::fmt::Debug for CellsIntrospect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CellsIntrospect")
            .field("row_count", &(self.rows)().len())
            .field("col_count", &DATA_HEADERS.len())
            .finish_non_exhaustive()
    }
}

impl QuerySource for CellsIntrospect {
    fn introspect_schema(&self) -> IntrospectSchema {
        // `col_count` — the metadata column count (int).
        // `cell_at`   — `cell_at.<pos>.<col>` visible position + metadata column
        //               index -> the cell's display value (string), Null when
        //               the position or column is out of range.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("col_count", "int"),
                    // R1353 §2 #2 — two arguments: a visible position and a metadata
                    // column. `col` is bounded by this surface's own `col_count`;
                    // `pos` is `open` because the visible-row count belongs to the
                    // tree state external, not here — an unpublished bound is
                    // declared as unknown rather than pointed at a path that does
                    // not exist on this surface.
                    SchemaField::parametric(
                        "cell_at.<pos>.<col>",
                        "string",
                        const {
                            &[
                                SchemaArg::open("pos", "int"),
                                SchemaArg::index("col", "col_count"),
                            ]
                        },
                    ),
                ]
            },
        )
    }

    fn introspect_query(&self, path: &str) -> Option<IntrospectValue> {
        if path == "col_count" {
            return Some(IntrospectValue::Int(
                i64::try_from(DATA_HEADERS.len()).unwrap_or(i64::MAX),
            ));
        }
        let rest = path.strip_prefix("cell_at.")?;
        // `cell_at.<pos>.<col>` — two indices. A malformed or out-of-range
        // address reports Null (present-but-empty), the convention the tree
        // structural introspection uses for an off-the-end position.
        let Some((pos_s, col_s)) = rest.split_once('.') else {
            return Some(IntrospectValue::Null);
        };
        let (Ok(pos), Ok(col)) = (pos_s.parse::<usize>(), col_s.parse::<usize>()) else {
            return Some(IntrospectValue::Null);
        };
        if col >= DATA_HEADERS.len() {
            return Some(IntrospectValue::Null);
        }
        let rows = (self.rows)();
        Some(match rows.get(pos) {
            Some(row) => IntrospectValue::Text(cell_data(&row.id, col)),
            None => IntrospectValue::Null,
        })
    }
}

/// Recursive existence check over the retained tree (every node, expanded or
/// not): the validity bound for a selection id, so a malformed RPC payload can
/// never select a phantom row (the [`VirtualSelect`](pinion_core::widgets::virtual_select)
/// `item_count` guard's stable-id analogue). Selection is keyed by id over the
/// WHOLE tree — not just the visible rows — so a node stays selected while an
/// ancestor is collapsed.
fn contains_id(nodes: &[OutlinerNode], id: &str) -> bool {
    nodes
        .iter()
        .any(|n| n.id == id || contains_id(&n.children, id))
}

// ─────────────────────────────────────────────────────────────────────────
// R905 §5.27 §5.52 — batch edit: structural tree mutation + undo commands.
//
// The mutation operates on the concrete `OutlinerNode` tree (the app owns its
// data model): the `TreeNode::children_mut` trait method returns a `&mut [Self]`
// slice, which cannot resize for remove / insert, so a *generic* tree
// structural-edit substrate would need a `TreeNode` trait extension
// (`children_vec_mut`) — deferred to a second consumer
// ([[abstraction-needs-second-consumer]]). These helpers stay example-local.
// ─────────────────────────────────────────────────────────────────────────

/// R905 — the ids in `selection` whose nearest selected ancestor is themselves
/// (no *ancestor* is selected), in tree pre-order. Deleting only these removes
/// each selected subtree exactly once: a selected descendant of a selected node
/// is removed by its ancestor's deletion, so it must not be deleted again.
fn collect_top_level(nodes: &[OutlinerNode], selection: &BTreeSet<String>, out: &mut Vec<String>) {
    for n in nodes {
        if selection.contains(&n.id) {
            out.push(n.id.clone());
            // Do not descend: this whole subtree goes with `n`.
        } else {
            collect_top_level(&n.children, selection, out);
        }
    }
}

/// R905 — every selected id in tree pre-order (visible AND collapsed-hidden),
/// the stable order batch-rename numbers over.
fn collect_selected_preorder(
    nodes: &[OutlinerNode],
    selection: &BTreeSet<String>,
    out: &mut Vec<String>,
) {
    for n in nodes {
        if selection.contains(&n.id) {
            out.push(n.id.clone());
        }
        collect_selected_preorder(&n.children, selection, out);
    }
}

/// R905 — locate `id` without mutating: its parent id (`None` at root), its
/// index among its siblings, and a clone of its subtree. The capture a
/// [`TreeBatchDelete`] reverses; `None` when `id` is absent.
fn find_location(
    nodes: &[OutlinerNode],
    parent: Option<&str>,
    id: &str,
) -> Option<(Option<String>, usize, OutlinerNode)> {
    if let Some(pos) = nodes.iter().position(|n| n.id == id) {
        return Some((parent.map(str::to_owned), pos, nodes[pos].clone()));
    }
    for n in nodes {
        if let Some(found) = find_location(&n.children, Some(&n.id), id) {
            return Some(found);
        }
    }
    None
}

// R935 §5.51 — `remove_subtree` / `insert_subtree` lifted to
// `pinion_core::widgets::tree_nav` when `hello-tree-reparent` became the second
// consumer of these correctness-critical structural edits (a `Vec::remove` /
// `insert` at the wrong depth silently corrupts the tree). This example now
// imports them and supplies `MutableTreeNode::children_vec_mut` above.

/// R905 — expand a rename template against a 1-based counter: `{n}` is replaced
/// by the counter (`"Layer {n}"` → `Layer 1`, `Layer 2`, …); a template without
/// `{n}` renames every selected node to the same literal.
fn apply_template(template: &str, n: usize) -> String {
    if template.contains("{n}") {
        template.replace("{n}", &n.to_string())
    } else {
        template.to_owned()
    }
}

/// R905 §5.52 — delete a set of selected subtrees as **one** undo step. Captures
/// each removed subtree with its `(parent, index)` so undo restores the exact
/// shape; `redo` removes by id, `undo` re-inserts ascending by index so original
/// positions are recovered. One command (not a macro of single deletes): the
/// removals are position-coupled, so the whole batch reverses atomically — the
/// `hello-node-editor` `GraphEdit` multi-delete shape, applied to a tree.
struct TreeBatchDelete {
    nodes: Signal<Vec<OutlinerNode>>,
    removed: Vec<(Option<String>, usize, OutlinerNode)>,
}

impl UndoCommand for TreeBatchDelete {
    fn label(&self) -> Cow<'static, str> {
        Cow::Borrowed("Delete selected")
    }

    fn redo(&self) {
        let mut tree = self.nodes.get();
        for (_, _, node) in &self.removed {
            remove_subtree(&mut tree, &node.id);
        }
        self.nodes.set(tree);
    }

    fn undo(&self) {
        let mut tree = self.nodes.get();
        // Re-insert ascending by (parent, index): each insert shifts the
        // following originals back into place (the standard multi-undelete).
        let mut ordered: Vec<&(Option<String>, usize, OutlinerNode)> =
            self.removed.iter().collect();
        ordered.sort_by(|a, b| (a.0.as_deref(), a.1).cmp(&(b.0.as_deref(), b.1)));
        for (parent, index, node) in ordered {
            insert_subtree(&mut tree, parent.as_deref(), *index, node.clone());
        }
        self.nodes.set(tree);
    }
}

/// R905 §5.52 — rename a set of selected nodes as **one** undo step. Each
/// entry is `(id, old, new)`; `redo` sets `new`, `undo` restores `old` (both via [`find_node_mut`]). One command
/// groups the batch — the rename is the toolkit `setData`-on-many shape.
struct TreeBatchRename {
    nodes: Signal<Vec<OutlinerNode>>,
    renames: Vec<(String, String, String)>,
}

impl TreeBatchRename {
    fn apply(&self, pick: impl Fn(&(String, String, String)) -> &String) {
        let mut tree = self.nodes.get();
        for entry in &self.renames {
            if let Some(node) = find_node_mut(&mut tree, &entry.0) {
                node.label.clone_from(pick(entry));
            }
        }
        self.nodes.set(tree);
    }
}

impl UndoCommand for TreeBatchRename {
    fn label(&self) -> Cow<'static, str> {
        Cow::Borrowed("Rename selected")
    }

    fn redo(&self) {
        self.apply(|entry| &entry.2);
    }

    fn undo(&self) {
        self.apply(|entry| &entry.1);
    }
}

/// Reactive holder for the retained tree + the keyboard cursor + the R902
/// multi-select state, lifted into [`Owner::cache`] so the view-fn, the a11y
/// pass, the click reducer, the keyboard `apply_key`, and the [`SELECT_TAG`]
/// coordinator all read / mutate the same `Signal`s.
struct TreeState {
    nodes: Signal<Vec<OutlinerNode>>,
    /// The keyboard **cursor** (active row / `aria-activedescendant`): the
    /// roving navigation reference. R902 — decoupled from the selection (the
    /// cursor can sit on a row `Ctrl+Space` has deselected); before R902 the
    /// single-cursor *was* the selection.
    focused_id: Signal<Option<String>>,
    /// R902 — the selected-row **set**, by stable [`OutlinerNode`] id (robust
    /// across expand / collapse, where a flat index would shift). The paint
    /// highlights the visible members, the a11y emits `aria-selected` per
    /// member, and `query("selection")` reports the FULL set (incl. members
    /// hidden inside a collapsed branch).
    selection: Signal<BTreeSet<String>>,
    /// R902 — the range-extension origin: the row a `Shift`-extend grows from,
    /// held put while extending and re-seeded by every plain move / toggle
    /// (the [`VirtualSelect`](pinion_core::widgets::virtual_select) `anchor`'s
    /// stable-id analogue).
    anchor: Signal<Option<String>>,
    /// R905 §5.52 — undo / redo history for the structural batch edits
    /// (delete-selected / rename-selected). Each batch lands as ONE step
    /// ([`TreeBatchDelete`] / [`TreeBatchRename`]) so a single undo reverses it.
    undo_stack: Rc<UndoStack>,
}

fn use_tree_state() -> Rc<TreeState> {
    let owner =
        Owner::current().expect("use_tree_state must run inside a CoreShell view / reducer wrap");
    // R905 — pre-resolve the undo stack before the `state` cache factory: a
    // cache factory must not re-enter `Owner::cache` ([[owner-cache-no-nested-factory]]),
    // and `use_undo_stack` is itself a cache hook.
    let undo_stack = use_undo_stack("hello_tree_grid::undo");
    owner.cache("hello_tree_grid::state", move || TreeState {
        nodes: Signal::new(initial_nodes()),
        // R864 — boot the keyboard cursor on the first row (the WAI-ARIA tree
        // convention `hello-virtual-tree` also uses): the outliner is a single
        // tab stop, so a defined `aria-activedescendant` exists from frame one
        // and the focus highlight paints across both panes immediately. R902 —
        // seed the selection on that same row (selection-follows-focus), so the
        // outliner opens with `f0` both the cursor AND the sole selected row,
        // matching the pre-R902 single-cursor highlight.
        focused_id: Signal::new(Some(String::from("f0"))),
        selection: Signal::new(BTreeSet::from([String::from("f0")])),
        anchor: Signal::new(Some(String::from("f0"))),
        undo_stack,
    })
}

impl TreeState {
    /// Replace the selection with just `id` and move the cursor + anchor there
    /// (a plain click / unmodified nav — [`SelectionChord::Replace`]). Validates
    /// `id` against the retained tree; an unknown id is a no-op. Returns whether
    /// anything changed.
    fn select_only(&self, id: &str) -> bool {
        if !contains_id(&self.nodes.get(), id) {
            return false;
        }
        let next = BTreeSet::from([id.to_owned()]);
        let changed = self.selection.get() != next
            || self.anchor.get().as_deref() != Some(id)
            || self.focused_id.get().as_deref() != Some(id);
        batch(|| {
            self.selection.set(next);
            self.anchor.set(Some(id.to_owned()));
            self.focused_id.set(Some(id.to_owned()));
        });
        changed
    }

    /// `Ctrl`-toggle `id`'s membership, leaving the rest of the set intact, and
    /// make it the cursor + anchor ([`SelectionChord::Toggle`] / `Ctrl+Space`).
    /// Validates `id`; an unknown id is a no-op. Returns whether `id` was valid
    /// (a toggle always flips membership).
    fn toggle(&self, id: &str) -> bool {
        if !contains_id(&self.nodes.get(), id) {
            return false;
        }
        let mut next = self.selection.get();
        if !next.remove(id) {
            next.insert(id.to_owned());
        }
        batch(|| {
            self.selection.set(next);
            self.anchor.set(Some(id.to_owned()));
            self.focused_id.set(Some(id.to_owned()));
        });
        true
    }

    /// `Shift`-extend the selection to `target`: replace it with the inclusive
    /// run from the [`anchor`](Self::anchor) to `target` in **visible-row
    /// order** (`rows` = [`flat_visible`]), moving the cursor to `target` while
    /// the anchor stays put ([`SelectionChord::Extend`] — the ordered-model
    /// meaning of *extend*). With no anchor (or one collapsed/scrolled out of
    /// `rows`) it falls back to [`select_only`](Self::select_only). An unknown
    /// `target` is a no-op. Returns whether anything changed.
    fn extend_range(&self, rows: &[VisibleRow], target: &str) -> bool {
        let Some(target_pos) = rows.iter().position(|r| r.id == target) else {
            return false;
        };
        let anchor = self.anchor.get();
        let Some(anchor_pos) = anchor
            .as_deref()
            .and_then(|a| rows.iter().position(|r| r.id == a))
        else {
            return self.select_only(target);
        };
        let (lo, hi) = (anchor_pos.min(target_pos), anchor_pos.max(target_pos));
        let next: BTreeSet<String> = rows[lo..=hi].iter().map(|r| r.id.clone()).collect();
        let changed =
            self.selection.get() != next || self.focused_id.get().as_deref() != Some(target);
        batch(|| {
            self.selection.set(next);
            self.focused_id.set(Some(target.to_owned()));
        });
        changed
    }

    /// `Ctrl+A` — select every **visible** row (the WAI-ARIA tree select-all
    /// convention: the visible rows, not descendants hidden inside collapsed
    /// branches). Leaves the cursor + anchor put. Returns whether the set grew.
    fn select_visible(&self, rows: &[VisibleRow]) -> bool {
        let next: BTreeSet<String> = rows.iter().map(|r| r.id.clone()).collect();
        if self.selection.get() == next {
            return false;
        }
        self.selection.set(next);
        true
    }

    /// Clear the selection + the range anchor; the keyboard cursor is left put
    /// (cursor ⊥ selection — clearing the selection should not move the
    /// keyboard position). Returns whether anything was selected.
    fn clear(&self) -> bool {
        let had = !self.selection.get().is_empty() || self.anchor.get().is_some();
        batch(|| {
            self.selection.set(BTreeSet::new());
            self.anchor.set(None);
        });
        had
    }

    /// Admin replace the selection with an arbitrary id set (the persisted /
    /// AI-restore channel — not an interaction): unknown ids are dropped, the
    /// range anchor resets, the cursor is left put. Returns whether it changed.
    fn set_selection(&self, ids: &BTreeSet<String>) -> bool {
        let nodes = self.nodes.get();
        let next: BTreeSet<String> = ids
            .iter()
            .filter(|id| contains_id(&nodes, id))
            .cloned()
            .collect();
        if self.selection.get() == next && self.anchor.get().is_none() {
            return false;
        }
        batch(|| {
            self.selection.set(next);
            self.anchor.set(None);
        });
        true
    }

    /// R905 §5.52 — delete every selected subtree as one undo step. Only the
    /// *top-level* selected nodes are removed (a selected descendant rides along
    /// inside its selected ancestor's subtree — [`collect_top_level`]), so each
    /// subtree is deleted once. Returns the number of subtrees removed (0 leaves
    /// the tree + history untouched). The selection is cleared afterwards (its
    /// members no longer exist). The edit applies through
    /// [`UndoStack::record`](pinion_core::undo::UndoStack::record) (which calls
    /// the command's `redo`), so one undo restores the whole batch.
    fn delete_selected(&self) -> usize {
        let nodes = self.nodes.get();
        let selection = self.selection.get();
        let mut top = Vec::new();
        collect_top_level(&nodes, &selection, &mut top);
        let removed: Vec<(Option<String>, usize, OutlinerNode)> = top
            .iter()
            .filter_map(|id| find_location(&nodes, None, id))
            .collect();
        if removed.is_empty() {
            return 0;
        }
        let count = removed.len();
        self.undo_stack.record(TreeBatchDelete {
            nodes: self.nodes.clone(),
            removed,
        });
        self.clear();
        count
    }

    /// R905 §5.52 — rename every selected node from `template` as one undo step.
    /// `{n}` in the template expands to a 1-based counter in tree pre-order
    /// ([`apply_template`]); a no-`{n}` template renames all to the same literal.
    /// Nodes whose label is already the target are skipped. Returns the number
    /// renamed (0 leaves the tree + history untouched). The selection is left
    /// intact (the same nodes, new labels).
    fn rename_selected(&self, template: &str) -> usize {
        let nodes = self.nodes.get();
        let selection = self.selection.get();
        let mut ordered = Vec::new();
        collect_selected_preorder(&nodes, &selection, &mut ordered);
        let mut renames: Vec<(String, String, String)> = Vec::new();
        for (i, id) in ordered.iter().enumerate() {
            if let Some((_, _, node)) = find_location(&nodes, None, id) {
                let new_label = apply_template(template, i + 1);
                if new_label != node.label {
                    renames.push((id.clone(), node.label.clone(), new_label));
                }
            }
        }
        if renames.is_empty() {
            return 0;
        }
        let count = renames.len();
        self.undo_stack.record(TreeBatchRename {
            nodes: self.nodes.clone(),
            renames,
        });
        count
    }

    /// R905 §5.52 — step the batch-edit history back one step. `false` (no-op)
    /// at the bottom of the stack.
    fn undo(&self) -> bool {
        self.undo_stack.undo()
    }

    /// R905 §5.52 — re-apply the next undone batch-edit step.
    fn redo(&self) -> bool {
        self.undo_stack.redo()
    }
}

/// R902 §5.40 §5.12 — the [`SELECT_TAG`] multi-select coordinator: the AI-first
/// query / intervene / invoke surface over the shared [`TreeState`] selection,
/// driving the SAME funnel ([`select_only`](TreeState::select_only) /
/// [`toggle`](TreeState::toggle) / [`extend_range`](TreeState::extend_range) /
/// [`select_visible`](TreeState::select_visible) / [`clear`](TreeState::clear) /
/// [`set_selection`](TreeState::set_selection)) the keyboard + click drive, so
/// RPC and pointer / keyboard multi-select are one SSOT
/// ([[ai-first-rpc-introspection-obligation]]). It owns no state of its own — it
/// holds the cached `TreeState` and mutates its `Signal`s, so a paint follows
/// every write (the framework re-renders after a `scene/invoke`, and the
/// `Signal::set` marks the view's owner dirty).
///
/// Read/write symmetric ([[wire-form-read-write-symmetry]]): `query("selection")`
/// reports the set as a JSON array of ids and `intervene("selection", [..])`
/// replaces it (the [`VirtualSelectExternal`](pinion_core::widgets::virtual_select)
/// set-encode shape, id-keyed); each interaction `invoke` op returns the
/// resulting set so a caller sees the outcome in one round-trip.
struct TreeSelectExternal {
    state: Rc<TreeState>,
}

impl core::fmt::Debug for TreeSelectExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TreeSelectExternal")
            .field("count", &self.state.selection.get().len())
            .finish_non_exhaustive()
    }
}

impl TreeSelectExternal {
    /// The selection set as a JSON array of ids (ascending — a `BTreeSet`): the
    /// uniform return for the `"selection"` query + every mutating `invoke`.
    /// Always an array (empty `[]` when nothing is selected), never `Null`, so
    /// an AI consumer treats the slot as a list unconditionally.
    fn selection_value(&self) -> IntrospectValue {
        IntrospectValue::Json(serde_json::Value::Array(
            self.state
                .selection
                .get()
                .into_iter()
                .map(serde_json::Value::String)
                .collect(),
        ))
    }

    /// The visible-row flattening — the order `extend_to` ranges over and the
    /// set `select_all` collects.
    fn rows(&self) -> Vec<VisibleRow> {
        flat_visible(&self.state.nodes.get())
    }
}

impl External for TreeSelectExternal {
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
}

impl ExternalIntrospect for TreeSelectExternal {
    fn schema(&self) -> IntrospectSchema {
        // `selection` — settable selected-id set as a JSON array (query + intervene).
        // `count`     — selected-row count (query only).
        // `anchor`    — the range-extension origin id (query only), Null when none.
        // The interaction ops (`select` / `toggle` / `extend_to` / `select_all`
        // / `clear`) are `invoke`-only, mirroring `VirtualSelectExternal`.
        // R905 — `delete_selected` / `rename_selected` / `undo` / `redo` are the
        // batch-edit `invoke` ops (delete/rename return the count, undo/redo a
        // bool).
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("selection", "json"),
                    SchemaField::new("count", "int"),
                    SchemaField::new("anchor", "string"),
                    // R1637 — the nine verbs the comment above already lists.
                    // Prose in a doc comment is not a declaration: the three
                    // reads were discoverable and the whole write half was not.
                    SchemaField::action("select", "json"),
                    SchemaField::action("toggle", "json"),
                    SchemaField::action("extend_to", "json"),
                    SchemaField::action("select_all", "json"),
                    SchemaField::action("clear", "json"),
                    SchemaField::action("delete_selected", "int"),
                    SchemaField::action("rename_selected", "int"),
                    SchemaField::action("undo", "bool"),
                    SchemaField::action("redo", "bool"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "selection" => Some(self.selection_value()),
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.state.selection.get().len()).unwrap_or(i64::MAX),
            )),
            "anchor" => Some(
                self.state
                    .anchor
                    .get()
                    .map_or(IntrospectValue::Null, IntrospectValue::Text),
            ),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The full set is a writable axis (admin / AI-restore): a JSON array
            // of ids replaces the selection (unknown ids dropped); Null clears.
            "selection" => match value {
                IntrospectValue::Json(serde_json::Value::Array(items)) => {
                    let ids: BTreeSet<String> = items
                        .into_iter()
                        .filter_map(|v| match v {
                            serde_json::Value::String(s) => Some(s),
                            _ => None,
                        })
                        .collect();
                    self.state.set_selection(&ids);
                    Ok(())
                }
                IntrospectValue::Null => {
                    self.state.set_selection(&BTreeSet::new());
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "count" | "anchor" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // Each op drives the shared funnel and returns the resulting set (the
        // same funnel the keyboard + click drive — RPC parity, not a fork).
        match path {
            "select" => match args {
                IntrospectValue::Text(ref id) => {
                    self.state.select_only(id);
                    Ok(self.selection_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "toggle" => match args {
                IntrospectValue::Text(ref id) => {
                    self.state.toggle(id);
                    Ok(self.selection_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "extend_to" => match args {
                IntrospectValue::Text(ref id) => {
                    self.state.extend_range(&self.rows(), id);
                    Ok(self.selection_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "select_all" => {
                self.state.select_visible(&self.rows());
                Ok(self.selection_value())
            }
            "clear" => {
                self.state.clear();
                Ok(self.selection_value())
            }
            // R905 §5.52 — batch edit on the selection. `delete_selected` (Null)
            // and `rename_selected` (Text template) each land as one undo step
            // and return the count affected; `undo` / `redo` step the history.
            "delete_selected" => match args {
                IntrospectValue::Null => Ok(IntrospectValue::Int(
                    i64::try_from(self.state.delete_selected()).unwrap_or(i64::MAX),
                )),
                _ => Err(InvokeError::TypeMismatch),
            },
            "rename_selected" => match args {
                IntrospectValue::Text(ref template) => Ok(IntrospectValue::Int(
                    i64::try_from(self.state.rename_selected(template)).unwrap_or(i64::MAX),
                )),
                _ => Err(InvokeError::TypeMismatch),
            },
            "undo" => Ok(IntrospectValue::Bool(self.state.undo())),
            "redo" => Ok(IntrospectValue::Bool(self.state.redo())),
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// R902 §5.40 §5.27 — apply one key to the **multi-select** windowed tree-grid.
///
/// Two chords are not navigation and short-circuit (the WAI-ARIA APG
/// multi-select set ops, matching the list/grid keyboard `nav_select_key`):
///
/// - **`Ctrl+A`** — select every visible row ([`select_visible`](TreeState::select_visible)).
/// - **`Ctrl+Space`** — toggle the cursor row's membership
///   ([`toggle`](TreeState::toggle); the cursor stays put — `Ctrl`-arrow is not
///   a tree gesture, so toggle is bound to `Ctrl+Space` only).
///
/// Otherwise the key drives the R864 navigation / expand / type-ahead pipeline
/// (the lifted [`apply_windowed_tree_key`], modifier-blind), then **selection
/// follows the cursor** — but only when the cursor actually *moved* (a pure
/// expand / collapse leaves it put and must not disturb the set). The chord
/// decode is the shared [`SelectionChord`] policy SSOT (R880.1): plain nav
/// **replaces** the selection with the new cursor, `Shift`+nav **extends** the
/// range from the [`anchor`](TreeState::anchor) — exactly the list/grid keyboard
/// semantics, so the three scaled Model/View widgets never diverge. (`Ctrl`+nav
/// decodes as `Toggle`, which has no move-gesture meaning here and collapses to
/// a plain move, matching `nav_select_key`.)
fn apply_key_impl(key: &str, modifiers: Modifiers) -> bool {
    let state = use_tree_state();
    // R906 / R932 — structural-edit keys (the R905 batch ops over the keyboard).
    // The `Ctrl`/`Cmd`+`Z` / `Ctrl`/`Cmd`+`Shift`+`Z` / `Ctrl`/`Cmd`+`Y` undo
    // chord goes through the shared `undo_redo_verb` SSOT (one editor keybinding
    // across graph / grid / tree); `Delete` / `Backspace` remove the selection.
    // Each is one undo step (R905), and the keys reach the methods the RPC ops
    // already drive, so keyboard and `scene/invoke` are one funnel. Plain `z` /
    // `y` (no command modifier) fall through to type-ahead.
    if let Some(verb) = undo_redo_verb(key, modifiers) {
        return if verb == "redo" {
            state.redo()
        } else {
            state.undo()
        };
    }
    let cmd = modifiers.ctrl || modifiers.meta;
    match key {
        "Delete" | "Backspace" if !cmd => return state.delete_selected() > 0,
        _ => {}
    }
    // R902.1 — the non-navigation multi-select chords (Ctrl+A / Ctrl+Space) via
    // the shared [`MultiSelectKeyOp`] gate (the same one `nav_select_key` uses,
    // so list/grid/tree never diverge on which keys are set-ops). Plain Space /
    // 'a' (command_key false -> None) fall through to expand-toggle / type-ahead.
    match pinion_core::input::MultiSelectKeyOp::classify(key, modifiers) {
        Some(pinion_core::input::MultiSelectKeyOp::SelectAll) => {
            let rows = flat_visible(&state.nodes.get());
            state.select_visible(&rows);
            return true;
        }
        Some(pinion_core::input::MultiSelectKeyOp::ToggleCursor) => {
            // Toggle the cursor row's membership (cursor stays put).
            return match state.focused_id.get() {
                Some(cursor) => {
                    state.toggle(&cursor);
                    true
                }
                None => false,
            };
        }
        None => {}
    }
    // Navigation / expand / type-ahead — the R864 cursor + expand pipeline.
    let before = state.focused_id.get();
    let handled = apply_windowed_tree_key(
        &state.nodes,
        &state.focused_id,
        &use_scroll_state(SCROLL_KEY),
        TYPEAHEAD_KEY,
        NAV_PAGE,
        ROW_PITCH,
        key,
    );
    // Selection-follows-cursor, but only when the cursor MOVED (an expand /
    // collapse keeps the cursor put and leaves the selection alone).
    if handled
        && let after = state.focused_id.get()
        && after != before
        && let Some(target) = after
    {
        let rows = flat_visible(&state.nodes.get());
        match SelectionChord::from_modifiers(modifiers) {
            SelectionChord::Extend => state.extend_range(&rows, &target),
            SelectionChord::Replace | SelectionChord::Toggle => state.select_only(&target),
        };
    }
    handled
}

/// view-fn (§6.3): pure sync mapping. The dataset is virtual —
/// `view_virtual_treegrid` builds cells only for the indices in the current
/// scroll window, re-derived from `flat_visible(&nodes)`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    // `view_virtual_treegrid` derives its slot pitch from `style.row_height`;
    // the keyboard scroll-into-view (`reveal_cursor`) uses ROW_PITCH, so keep
    // them in lockstep.
    debug_assert_eq!(TreeViewStyle::m3_default().row_height, ROW_PITCH);

    let theme = use_theme(THEME_TAG).theme_animated();
    let tree_state = use_tree_state();
    let nodes = tree_state.nodes.get();
    let rows = flat_visible(&nodes);
    let focused = tree_state.focused_id.get();
    // R902 — snapshot the selection set once; the paint predicate tests
    // membership per visible row (several rows highlighted at once).
    let selection = tree_state.selection.get();
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);

    let grid = view_virtual_treegrid(
        TREE_TAG,
        GridScroll {
            body: &scroll,
            horizontal: &h_scroll,
        },
        &TreeGridData {
            rows: &rows,
            tree_header: TREE_HEADER,
            data_headers: &DATA_HEADERS,
            tree_col_width: TREE_COL_W,
            data_col_width: DATA_COL_W,
            overscan: OVERSCAN,
            cursor: focused.as_deref(),
        },
        &theme,
        &TreeViewStyle::m3_default(),
        |id| selection.contains(id),
        cell_data,
    );

    // R819 — invisible 0x0 root anchor keeps the WAI-ARIA `tree` node + the
    // focus surface alive (mirrors `hello-virtual-tree`); no visual paints.
    let invisible_root = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(ROOT_TAG)
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(0, 0))
                    .with_focusable(true),
            ),
    );

    Scene::Container(
        ContainerNode::new(vec![grid, invisible_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start),
            ),
    )
}

struct TreeGridView;

impl WidgetCore for TreeGridView {
    type State = ();
    type Event = ();

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    /// The `TreeRowClickExternal` sibling (composite `{TREE_TAG}#{id}` clicks
    /// → the §5.20 intent channel → [`TreeGridView::update`]) + the query-only
    /// tree-state introspection sibling (reads the same `Owner::cache`d
    /// `TreeState` the view windows over, so `scene/query` reports the full
    /// flattening regardless of which window paints).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let tree_state = use_tree_state();
        let struct_nodes = tree_state.nodes.clone();
        let cells_nodes = tree_state.nodes.clone();
        let focused = tree_state.focused_id.clone();
        vec![
            // R902.1 — opt in to modifier-aware clicks (the `click` intent then
            // carries the held modifiers; the keyboard's `SelectionChord` chords
            // get a pointer peer).
            ExtraExternal::new(
                TREE_TAG,
                Box::new(TreeRowClickExternal::new().with_click_modifiers()),
            ),
            tree_view_introspection_extra(
                TREE_STATE_TAG,
                move || flat_visible(&struct_nodes.get()),
                move || focused.get(),
            ),
            // R865 — the metadata-cell peer (off-window value read).
            ExtraExternal::new(
                CELLS_TAG,
                Box::new(QueryOnlyIntrospect::new(Rc::new(CellsIntrospect {
                    rows: Box::new(move || flat_visible(&cells_nodes.get())),
                }))),
            ),
            // R902 — the multi-select coordinator: query / intervene the
            // selection set + invoke the keyboard's funnel ops from RPC.
            ExtraExternal::new(
                SELECT_TAG,
                Box::new(TreeSelectExternal { state: tree_state }),
            ),
        ]
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Click a row → a [`SelectionChord`]-decoded selection (R902.1, the pointer
    /// peer of the keyboard chords): a **plain** click *replaces* the selection
    /// (move cursor + anchor — [`select_only`](TreeState::select_only)) AND
    /// toggles the row's `expanded` flag (the R860 row-click affordance);
    /// `Ctrl`-click *toggles* the row's membership and `Shift`-click *extends*
    /// the range from the anchor — both selection-only (a chord click is a pure
    /// selection gesture, it does not expand). The held modifiers ride the
    /// `click` intent's composite `"{id}:click[:{token}]"` payload because the
    /// [`TreeRowClickExternal`] is built `with_click_modifiers` (R902.1); the
    /// decode is the shared [`split_send_payload`] grammar. Side-effect-only
    /// reducer ([[scxml-as-model-update-transient]]): the `Signal::set`s are the
    /// mutation, so the command list is empty.
    fn update(_state: (), intent: &Intent) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == CLICK_INTENT_TAG
            && let IntrospectValue::Text(payload) = &intent.payload
            && let Some(sent) = split_send_payload(payload)
        {
            let (id, modifiers) = (sent.key, sent.modifiers);
            let tree_state = use_tree_state();
            match SelectionChord::from_modifiers(modifiers) {
                SelectionChord::Replace => {
                    tree_state.select_only(id);
                    toggle_expanded(&tree_state.nodes, id);
                }
                SelectionChord::Toggle => {
                    tree_state.toggle(id);
                }
                SelectionChord::Extend => {
                    let rows = flat_visible(&tree_state.nodes.get());
                    tree_state.extend_range(&rows, id);
                }
            }
        }
        Vec::new()
    }

    /// R864 §5.27 §5.50 — WAI-ARIA APG tree keyboard over the windowed rows:
    /// the lifted `apply_tree_key` resolve → flag-store bridge + caller-side
    /// type-ahead, then `reveal_cursor` scrolls the new cursor into the body
    /// window (keyboard ⊥ virtualization). Single tab stop: keys apply only
    /// while the treegrid root [`ROOT_TAG`] is focused — an ungated outliner
    /// would steal keys from sibling panels in the eventual self-hosted editor
    /// ([[routing-and-focus-are-separate-axes]]); the RPC demo issues
    /// `focus/set` first like every other gated example.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: Modifiers,
    ) -> bool {
        let _ = scene;
        if focused != Some(ROOT_TAG) {
            return false;
        }
        apply_key_impl(key, modifiers)
    }

    fn title() -> &'static str {
        "pinion hello-tree-grid (R860 §5.27 scene-outliner tree-grid)"
    }

    fn fmt_state_log(_state: &()) -> String {
        "display + click-to-expand (no widget state)".to_string()
    }
}

impl WidgetA11y for TreeGridView {
    /// R863 — WAI-ARIA `treegrid` over the windowed rows (the shared
    /// [`treegrid_nodes`]): each `row` carries the tree disclosure axes and a
    /// `rowheader` (the `{TREE_TAG}#{id}` name cell) + one `gridcell` per
    /// metadata column, with the row's bounds spanning the frozen name pane +
    /// the scrolling metadata pane. Resolves the R860 carry (metadata columns
    /// were AT-invisible under the prior `tree`/`treeitem` topology).
    ///
    /// R865 — windows the AT tree to the rendered slice (the same
    /// `compute_visible_range` the paint uses), mirroring `hello-virtual-tree`:
    /// the AT tree exposes exactly the rows the paint realizes, so off-window
    /// rows are not announced as bounds-less ghost nodes.
    ///
    /// R902 — the treegrid is `aria-multiselectable`; each rendered row carries
    /// `aria-selected = selection membership` (several at once), and the keyboard
    /// cursor (decoupled — `Ctrl+Space` may sit it on a deselected row) is the
    /// `aria-activedescendant` via [`with_focused`](AccessNode::with_focused) +
    /// the [`access_focus_target`](Self::access_focus_target) composite.
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let tree_state = use_tree_state();
        let rows = flat_visible(&tree_state.nodes.get());
        let cursor = tree_state.focused_id.get();
        let selection = tree_state.selection.get();
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(
            scroll.offset_y(),
            measured_h,
            rows.len(),
            ROW_PITCH,
            OVERSCAN,
        );
        let slice: &[VisibleRow] = &rows[window.first..window.first + window.count];
        treegrid_nodes(
            ROOT_TAG,
            TREE_TAG,
            Some("Scene outliner"),
            TREE_HEADER,
            &DATA_HEADERS,
            slice,
            &TreeGridSelection {
                selected: &selection,
                cursor: cursor.as_deref(),
            },
        )
    }

    /// R866 §5.40 — composite focus model (WAI-ARIA roving). When the outliner
    /// root owns focus, the keyboard cursor row is the `aria-activedescendant`:
    /// `AccessKit::TreeUpdate::focus` lands on the [`ROOT_TAG`] treegrid while
    /// the active descendant names the cursor ROW node (`{TREE_TAG}_drow{id}`),
    /// so the AT announces "you are on row X" rather than only "row X selected".
    /// Mirrors the listbox / radiogroup composite-focus pattern; the cursor row
    /// carries the matching `focused` flag from [`treegrid_nodes`].
    fn access_focus_target(_state: &(), focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(ROOT_TAG)
            && let Some(cursor) = use_tree_state().focused_id.get()
        {
            return Some(AccessFocus::composite(
                ROOT_TAG,
                GridTag::metadata_row(TREE_TAG, &cursor),
            ));
        }
        focused.map(AccessFocus::atomic)
    }
}

impl WidgetView for TreeGridView {
    type Renderer = HelloTreeGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TreeGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain (no-modifier) key press through the R902 multi-select pipeline.
    fn nav(key: &str) -> bool {
        apply_key_impl(key, Modifiers::empty())
    }
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
        meta: false,
    };
    const CTRL: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
        meta: false,
    };

    /// The current selection as a sorted `Vec<String>` (a `BTreeSet` is already
    /// ordered) for terse assertions.
    fn selection_of(state: &TreeState) -> Vec<String> {
        state.selection.get().into_iter().collect()
    }

    #[test]
    fn metadata_overflows_the_window() {
        // The premise: tree column + all metadata columns exceed the window,
        // so the metadata pane genuinely scrolls horizontally past the freeze.
        // Sum the real per-column widths (a runtime fold, not a const).
        let total: u32 = TREE_COL_W + DATA_HEADERS.iter().map(|_| DATA_COL_W).sum::<u32>();
        assert!(
            total > WIN_W,
            "tree + metadata ({total}) must exceed WIN_W ({WIN_W})"
        );
        // The frozen tree column leaves room for visible metadata beside it.
        let metadata_visible = WIN_W.saturating_sub(TREE_COL_W);
        assert!(
            metadata_visible > 0,
            "frozen column leaves room for metadata"
        );
    }

    #[test]
    fn cell_data_distinguishes_folders_from_objects() {
        assert_eq!(
            cell_data("f3", 0),
            "Folder",
            "a folder id (no '-') is a Folder"
        );
        assert_ne!(
            cell_data("f3-o1", 0),
            "Folder",
            "an object id is not a Folder"
        );
        // Deterministic: same id → same metadata.
        assert_eq!(cell_data("f3-o1", 2), cell_data("f3-o1", 2));
    }

    #[test]
    fn initial_tree_boots_some_folders_expanded() {
        let nodes = initial_nodes();
        assert_eq!(nodes.len(), FOLDERS);
        let rows = flat_visible(&nodes);
        // FOLDERS folder rows + EXPANDED_AT_BOOT × OBJECTS_PER object rows.
        let expected = FOLDERS + EXPANDED_AT_BOOT * OBJECTS_PER;
        assert_eq!(
            rows.len(),
            expected,
            "boot visible rows = folders + expanded children"
        );
    }

    // ── R864 keyboard roving ─────────────────────────────────────────

    #[test]
    fn slot_pitch_matches_style_row_height() {
        // `view_virtual_treegrid` derives its pitch from the style; the
        // keyboard scroll-into-view const must track it.
        assert_eq!(TreeViewStyle::m3_default().row_height, ROW_PITCH);
    }

    #[test]
    fn boot_cursor_sits_on_the_first_row() {
        Owner::new().run(|| {
            // WAI-ARIA tree convention: a defined aria-activedescendant from
            // frame one, so the focus highlight + AT cursor exist at boot.
            assert_eq!(use_tree_state().focused_id.get().as_deref(), Some("f0"));
        });
    }

    #[test]
    fn arrow_keys_move_the_row_cursor() {
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * ROW_PITCH);
            let state = use_tree_state();
            // f0 boots expanded → ArrowDown descends to its first child.
            assert_eq!(state.focused_id.get().as_deref(), Some("f0"));
            assert!(nav("ArrowDown"), "ArrowDown handled");
            assert_eq!(
                state.focused_id.get().as_deref(),
                Some("f0-o0"),
                "Down -> first child"
            );
            assert!(nav("ArrowUp"), "ArrowUp handled");
            assert_eq!(
                state.focused_id.get().as_deref(),
                Some("f0"),
                "Up -> back to parent"
            );
            // A non-navigation, non-typeahead key is unhandled (falls through).
            assert!(!nav("F5"), "F5 is not a tree key");
        });
    }

    #[test]
    fn arrow_right_left_expand_collapse_a_folder() {
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * ROW_PITCH);
            let state = use_tree_state();
            // Focus a collapsed folder (f10), then ArrowRight expands it.
            state.focused_id.set(Some(String::from("f10")));
            let before = flat_visible(&state.nodes.get()).len();
            assert!(nav("ArrowRight"), "ArrowRight handled");
            let after = flat_visible(&state.nodes.get()).len();
            assert_eq!(after, before + OBJECTS_PER, "ArrowRight expanded f10");
            // ArrowLeft on the expanded branch collapses it again.
            assert!(nav("ArrowLeft"), "ArrowLeft handled");
            assert_eq!(
                flat_visible(&state.nodes.get()).len(),
                before,
                "ArrowLeft collapsed f10"
            );
        });
    }

    #[test]
    fn keyboard_reveals_an_off_window_cursor() {
        Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(WIN_W, 10 * ROW_PITCH); // 10-row window
            scroll.set_max(0, 1_000_000); // unclamp so scroll_to is honoured
            // Drive the cursor well past the bottom of the window.
            for _ in 0..40 {
                nav("ArrowDown");
            }
            assert!(
                scroll.offset_y() > 0,
                "navigating down scrolled the body window"
            );
            // The cursor row is inside the re-derived window (scroll-into-view).
            let state = use_tree_state();
            let cursor = state.focused_id.get().expect("cursor set");
            let rows = flat_visible(&state.nodes.get());
            let idx = rows
                .iter()
                .position(|r| r.id == cursor)
                .expect("cursor row visible");
            let (_, mh) = scroll.measured_viewport();
            let window =
                compute_visible_range(scroll.offset_y(), mh, rows.len(), ROW_PITCH, OVERSCAN);
            assert!(
                idx >= window.first && idx < window.first + window.count,
                "cursor row {idx} stays within the revealed window {window:?}",
            );
        });
    }

    #[test]
    fn apply_key_gates_on_root_focus() {
        // Single tab stop: keys apply ONLY when the treegrid root is focused —
        // an ungated outliner would steal keys from sibling panels.
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * ROW_PITCH);
            let mut scene = Scene::Container(pinion_core::scene::ContainerNode::new(Vec::new()));
            assert!(
                !TreeGridView::apply_key(&mut scene, None, "ArrowDown", Modifiers::default()),
                "no focus -> key dropped",
            );
            assert!(
                !TreeGridView::apply_key(
                    &mut scene,
                    Some("other"),
                    "ArrowDown",
                    Modifiers::default()
                ),
                "focus elsewhere -> key dropped",
            );
            assert!(
                TreeGridView::apply_key(
                    &mut scene,
                    Some(ROOT_TAG),
                    "ArrowDown",
                    Modifiers::default()
                ),
                "focus on the treegrid root -> key applies",
            );
        });
    }

    // ── R865 a11y windowing + cell_at introspection ──────────────────

    #[test]
    fn access_node_windows_the_treegrid_to_the_rendered_slice() {
        // The AT tree exposes only the rendered window of rows, not the full
        // boot flattening (the bounds-less off-window ghost rows R863 emitted).
        let owner = Owner::new();
        let nodes = owner.run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 8 * ROW_PITCH);
            TreeGridView::access_node(&(), None)
        });
        assert_eq!(
            nodes[0].role,
            pinion_a11y::AriaRole::TreeGrid,
            "container is a treegrid"
        );
        let rows = nodes
            .iter()
            .filter(|n| n.role == pinion_a11y::AriaRole::Row)
            .count();
        let visible = flat_visible(&initial_nodes()).len();
        assert!(visible > 30, "boot has many visible rows ({visible})");
        // header row + windowed data rows, far fewer than the full flattening.
        assert!(
            rows < visible,
            "AT rows {rows} window the {visible} visible rows"
        );
        // The boot cursor (f0) is in the top window, so its row is present.
        assert!(
            nodes.iter().any(|n| n.tag == "tgrid_drowf0"),
            "cursor row f0 windowed in"
        );
    }

    #[test]
    fn cell_at_reads_metadata_by_position_and_column() {
        // The off-window read: `cell_at.<pos>.<col>` resolves the visible row
        // at <pos> and reports its <col> metadata exactly as `cell_data` does.
        let src = CellsIntrospect {
            rows: Box::new(|| flat_visible(&initial_nodes())),
        };
        let rows = flat_visible(&initial_nodes());
        // col_count mirrors the metadata header count.
        assert_eq!(
            src.introspect_query("col_count"),
            Some(IntrospectValue::Int(
                i64::try_from(DATA_HEADERS.len()).unwrap()
            )),
        );
        // An in-range cell reports the same string the painted cell shows.
        let pos = 5.min(rows.len() - 1);
        let expected = cell_data(&rows[pos].id, 1);
        assert_eq!(
            src.introspect_query(&format!("cell_at.{pos}.1")),
            Some(IntrospectValue::Text(expected)),
        );
        // Out-of-range column / position / malformed address -> Null.
        assert_eq!(
            src.introspect_query("cell_at.0.99"),
            Some(IntrospectValue::Null),
            "col OOR"
        );
        assert_eq!(
            src.introspect_query("cell_at.99999.0"),
            Some(IntrospectValue::Null),
            "pos OOR"
        );
        assert_eq!(
            src.introspect_query("cell_at.0"),
            Some(IntrospectValue::Null),
            "no column"
        );
        assert_eq!(src.introspect_query("bogus"), None, "unknown path");
    }

    // ── R866 aria-activedescendant (roving composite focus) ──────────

    #[test]
    fn focused_outliner_returns_composite_activedescendant() {
        // When the treegrid owns focus, the keyboard cursor row is conveyed via
        // aria-activedescendant (focus on the container, active-descendant = the
        // cursor ROW node), not aria-selected alone.
        Owner::new().run(|| {
            use_tree_state().focused_id.set(Some(String::from("f0-o3")));
            let target = TreeGridView::access_focus_target(&(), Some(ROOT_TAG))
                .expect("focused outliner -> composite focus");
            assert_eq!(
                target.focus_tag, ROOT_TAG,
                "focus lands on the treegrid root"
            );
            assert_eq!(
                target.active_descendant.as_deref(),
                Some("tgrid_drowf0-o3"),
                "aria-activedescendant = the cursor ROW node",
            );
        });
    }

    #[test]
    fn unfocused_outliner_returns_atomic_for_a_sibling() {
        // Focus elsewhere -> atomic on the sibling, no active descendant (the
        // outliner does not claim the cursor while a sibling panel is focused).
        Owner::new().run(|| {
            let target = TreeGridView::access_focus_target(&(), Some("other_widget"))
                .expect("sibling-focused -> atomic");
            assert_eq!(target.focus_tag, "other_widget");
            assert!(target.active_descendant.is_none());
        });
    }

    // ── R902 §5.40 multi-select model + coordinator ──────────────────

    #[test]
    fn boot_selects_the_cursor_row() {
        Owner::new().run(|| {
            // The outliner opens with f0 the cursor AND the sole selected row
            // (selection-follows-focus seed), matching the pre-R902 highlight.
            let state = use_tree_state();
            assert_eq!(state.focused_id.get().as_deref(), Some("f0"));
            assert_eq!(selection_of(&state), vec!["f0"]);
            assert_eq!(state.anchor.get().as_deref(), Some("f0"));
        });
    }

    #[test]
    fn plain_nav_replaces_the_selection_with_the_new_cursor() {
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * ROW_PITCH);
            let state = use_tree_state();
            assert!(nav("ArrowDown"));
            // selection-follows-focus: the set is exactly the new cursor.
            assert_eq!(state.focused_id.get().as_deref(), Some("f0-o0"));
            assert_eq!(selection_of(&state), vec!["f0-o0"], "plain nav replaces");
            assert_eq!(
                state.anchor.get().as_deref(),
                Some("f0-o0"),
                "plain nav re-seeds the anchor"
            );
        });
    }

    #[test]
    fn shift_nav_extends_the_range_from_the_anchor() {
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * ROW_PITCH);
            let state = use_tree_state();
            // anchor boots at f0; f0 boots expanded → the next visible rows are
            // f0-o0, f0-o1, … Shift+Down extends the contiguous run.
            assert!(apply_key_impl("ArrowDown", SHIFT));
            assert_eq!(
                selection_of(&state),
                vec!["f0", "f0-o0"],
                "shift extends from anchor f0"
            );
            assert_eq!(
                state.anchor.get().as_deref(),
                Some("f0"),
                "anchor stays put while extending"
            );
            // Extend further: the range grows, anchor unchanged, cursor at the end.
            assert!(apply_key_impl("ArrowDown", SHIFT));
            assert_eq!(selection_of(&state), vec!["f0", "f0-o0", "f0-o1"]);
            assert_eq!(
                state.focused_id.get().as_deref(),
                Some("f0-o1"),
                "cursor rides the far end"
            );
            assert_eq!(state.anchor.get().as_deref(), Some("f0"));
        });
    }

    #[test]
    fn ctrl_space_toggles_the_cursor_membership_without_moving() {
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * ROW_PITCH);
            let state = use_tree_state();
            // boot: cursor f0, selected {f0}. Ctrl+Space deselects it (cursor stays).
            assert!(apply_key_impl("Space", CTRL));
            assert!(selection_of(&state).is_empty(), "Ctrl+Space toggled f0 off");
            assert_eq!(
                state.focused_id.get().as_deref(),
                Some("f0"),
                "cursor stays put"
            );
            // Ctrl+Space again re-selects it.
            assert!(apply_key_impl("Space", CTRL));
            assert_eq!(selection_of(&state), vec!["f0"]);
        });
    }

    #[test]
    fn ctrl_a_selects_every_visible_row() {
        Owner::new().run(|| {
            use_scroll_state(SCROLL_KEY).set_measured_viewport(WIN_W, 10 * ROW_PITCH);
            let state = use_tree_state();
            let visible = flat_visible(&state.nodes.get()).len();
            assert!(visible > 30, "boot has many visible rows ({visible})");
            assert!(apply_key_impl("a", CTRL));
            assert_eq!(
                state.selection.get().len(),
                visible,
                "Ctrl+A selects all visible rows"
            );
        });
    }

    #[test]
    fn selection_is_stable_across_collapse_by_id_not_index() {
        Owner::new().run(|| {
            let state = use_tree_state();
            // f0 boots expanded → its children are visible. Select one, then
            // collapse f0: the child is no longer visible but stays in the set
            // (keyed by stable id, not by a flat index that would shift).
            assert!(state.toggle("f0-o5"));
            assert!(state.selection.get().contains("f0-o5"));
            toggle_expanded(&state.nodes, "f0"); // collapse f0
            let visible: BTreeSet<String> = flat_visible(&state.nodes.get())
                .iter()
                .map(|r| r.id.clone())
                .collect();
            assert!(
                !visible.contains("f0-o5"),
                "the child is collapsed away (not visible)"
            );
            assert!(
                state.selection.get().contains("f0-o5"),
                "but it stays selected by stable id"
            );
            // Re-expand: it is visible AND still selected.
            toggle_expanded(&state.nodes, "f0");
            assert!(
                state.selection.get().contains("f0-o5"),
                "re-expanding restores the selected row"
            );
        });
    }

    #[test]
    fn clear_empties_selection_but_keeps_the_cursor() {
        Owner::new().run(|| {
            let state = use_tree_state();
            state.toggle("f1");
            assert!(state.clear());
            assert!(selection_of(&state).is_empty());
            assert!(state.anchor.get().is_none(), "clear drops the range anchor");
            assert_eq!(
                state.focused_id.get().as_deref(),
                Some("f1"),
                "cursor ⊥ selection: cursor stays"
            );
            assert!(!state.clear(), "clearing an empty selection is a no-op");
        });
    }

    #[test]
    fn set_selection_and_select_only_reject_phantom_ids() {
        Owner::new().run(|| {
            let state = use_tree_state();
            let ids: BTreeSet<String> = ["f2", "f3", "ghost", "f99-o99"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
            assert!(state.set_selection(&ids));
            assert_eq!(
                selection_of(&state),
                vec!["f2", "f3"],
                "phantom ids dropped, real ids kept"
            );
            assert!(
                state.anchor.get().is_none(),
                "admin restore resets the anchor"
            );
            // The interaction funnel guards too: a malformed id is a no-op.
            assert!(
                !state.select_only("does-not-exist"),
                "unknown id is a no-op"
            );
            assert!(!state.toggle("does-not-exist"), "unknown id is a no-op");
            assert_eq!(
                selection_of(&state),
                vec!["f2", "f3"],
                "selection unchanged by phantom ops"
            );
        });
    }

    /// Build a click `Intent` exactly as the opted-in `TreeRowClickExternal`
    /// emits it (R902.1 — the composite `"{id}:click[:{token}]"` payload).
    fn click_intent(id: &str, modifiers: Modifiers) -> Intent {
        let payload = pinion_core::composite_tag::compose_send_payload(
            Some(id),
            pinion_widget_paint::tree_view::TREE_ROW_CLICK_EVENT,
            modifiers,
            pinion_core::input::PointerButtons::empty(),
        );
        Intent::new_static(CLICK_INTENT_TAG, IntrospectValue::Text(payload))
    }

    #[test]
    fn plain_click_replaces_and_toggles_expand() {
        Owner::new().run(|| {
            let state = use_tree_state();
            // Plain click f7 (a collapsed folder): selects it (replace) AND
            // expands it (the R860 row-click affordance).
            let _ = TreeGridView::update((), &click_intent("f7", Modifiers::empty()));
            assert_eq!(
                selection_of(&state),
                vec!["f7"],
                "plain click replaces the selection"
            );
            assert_eq!(
                state.focused_id.get().as_deref(),
                Some("f7"),
                "plain click moves the cursor"
            );
            assert!(
                flat_visible(&state.nodes.get())
                    .iter()
                    .any(|r| r.id == "f7-o0"),
                "plain click expanded the folder",
            );
        });
    }

    #[test]
    fn ctrl_and_shift_click_drive_the_selection_chord() {
        // R902.1 — the pointer peer of the keyboard chords: Ctrl-click toggles,
        // Shift-click extends — both selection-only (no expand on a chord click).
        Owner::new().run(|| {
            let state = use_tree_state();
            // Plain click a LEAF (f0 boots expanded, f0-o2 is a leaf -> no expand):
            // replaces + anchors at f0-o2.
            let _ = TreeGridView::update((), &click_intent("f0-o2", Modifiers::empty()));
            assert_eq!(
                selection_of(&state),
                vec!["f0-o2"],
                "plain click on a leaf replaces"
            );
            assert_eq!(
                state.anchor.get().as_deref(),
                Some("f0-o2"),
                "plain click seeds the anchor"
            );
            // Shift-click extends the contiguous range from the anchor.
            let _ = TreeGridView::update((), &click_intent("f0-o5", SHIFT));
            assert_eq!(
                selection_of(&state),
                vec!["f0-o2", "f0-o3", "f0-o4", "f0-o5"],
                "Shift-click extends the range from the anchor over the visible run",
            );
            assert_eq!(
                state.anchor.get().as_deref(),
                Some("f0-o2"),
                "Shift-click holds the anchor put"
            );
            // Ctrl-click toggles a non-adjacent row IN without clearing the range,
            // and does NOT expand it.
            let _ = TreeGridView::update((), &click_intent("f7", CTRL));
            assert!(state.selection.get().contains("f7"), "Ctrl-click adds f7");
            assert!(
                state.selection.get().contains("f0-o3"),
                "Ctrl-click keeps the existing range"
            );
            assert!(
                flat_visible(&state.nodes.get())
                    .iter()
                    .all(|r| r.id != "f7-o0"),
                "Ctrl-click on a folder does NOT expand it (pure selection gesture)",
            );
            // Ctrl-click an existing member removes it.
            let _ = TreeGridView::update((), &click_intent("f0-o3", CTRL));
            assert!(
                !state.selection.get().contains("f0-o3"),
                "Ctrl-click toggles a member off"
            );
        });
    }

    #[test]
    fn select_external_query_intervene_invoke_round_trip() {
        Owner::new().run(|| {
            let mut ext = TreeSelectExternal {
                state: use_tree_state(),
            };
            // boot: {f0}. Read mirrors the state.
            assert_eq!(ext.query("count"), Some(IntrospectValue::Int(1)));
            assert_eq!(
                ext.query("selection"),
                Some(IntrospectValue::Json(serde_json::json!(["f0"])))
            );
            assert_eq!(
                ext.query("anchor"),
                Some(IntrospectValue::Text("f0".into()))
            );
            // invoke toggle adds a row and returns the resulting set.
            assert_eq!(
                ext.invoke("toggle", IntrospectValue::Text("f5".into())),
                Ok(IntrospectValue::Json(serde_json::json!(["f0", "f5"]))),
            );
            // intervene (admin restore) replaces; the read mirrors the write,
            // and an unknown id is dropped on the write path.
            ext.intervene(
                "selection",
                IntrospectValue::Json(serde_json::json!(["f3", "ghost"])),
            )
            .expect("array replaces");
            assert_eq!(
                ext.query("selection"),
                Some(IntrospectValue::Json(serde_json::json!(["f3"]))),
                "the unknown id is dropped on the write path",
            );
            // clear via invoke returns the empty set; read-only axes + unknown
            // paths error.
            assert_eq!(
                ext.invoke("clear", IntrospectValue::Null),
                Ok(IntrospectValue::Json(serde_json::json!([]))),
            );
            assert_eq!(
                ext.intervene("count", IntrospectValue::Int(0)),
                Err(InterveneError::ReadOnly)
            );
            assert_eq!(
                ext.invoke("bogus", IntrospectValue::Null),
                Err(InvokeError::UnknownPath)
            );
        });
    }

    #[test]
    fn select_external_extend_to_ranges_over_visible_rows() {
        Owner::new().run(|| {
            let mut ext = TreeSelectExternal {
                state: use_tree_state(),
            };
            // Seed the anchor at f0, then extend over the visible run to f0-o2.
            ext.invoke("select", IntrospectValue::Text("f0".into()))
                .unwrap();
            let set = ext
                .invoke("extend_to", IntrospectValue::Text("f0-o2".into()))
                .unwrap();
            assert_eq!(
                set,
                IntrospectValue::Json(serde_json::json!(["f0", "f0-o0", "f0-o1", "f0-o2"])),
                "extend_to ranges from the anchor over the visible flattening",
            );
        });
    }

    // ─────────────────────────────────────────────────────────────
    // R905 §5.52 — batch delete / rename + undo
    // ─────────────────────────────────────────────────────────────

    fn label_of(state: &TreeState, id: &str) -> Option<String> {
        find_location(&state.nodes.get(), None, id).map(|(_, _, n)| n.label)
    }

    #[test]
    fn r905_delete_selected_removes_subtrees_one_undo_step() {
        Owner::new().run(|| {
            let state = use_tree_state();
            state.set_selection(&BTreeSet::from([String::from("f5"), String::from("f6")]));
            assert_eq!(state.delete_selected(), 2, "two top-level subtrees removed");
            // Both folders and their object children are gone.
            assert!(!contains_id(&state.nodes.get(), "f5"));
            assert!(!contains_id(&state.nodes.get(), "f6"));
            assert!(
                !contains_id(&state.nodes.get(), "f5-o0"),
                "child subtree removed too"
            );
            assert!(
                selection_of(&state).is_empty(),
                "selection cleared after delete"
            );
            // One undo restores the whole batch.
            assert!(state.undo(), "undo steps");
            assert!(contains_id(&state.nodes.get(), "f5"), "f5 restored");
            assert!(
                contains_id(&state.nodes.get(), "f6-o3"),
                "f6's children restored"
            );
            assert!(state.redo(), "redo re-applies");
            assert!(!contains_id(&state.nodes.get(), "f5"), "redo deletes again");
        });
    }

    #[test]
    fn r905_delete_dedups_selected_ancestor_and_descendant() {
        Owner::new().run(|| {
            let state = use_tree_state();
            // f0 is expanded at boot, so f0-o0 is a real selectable descendant.
            state.set_selection(&BTreeSet::from([String::from("f0"), String::from("f0-o0")]));
            assert_eq!(
                state.delete_selected(),
                1,
                "ancestor + its descendant = ONE top-level subtree",
            );
            assert!(!contains_id(&state.nodes.get(), "f0"));
            assert!(!contains_id(&state.nodes.get(), "f0-o0"));
            assert!(state.undo(), "undo restores");
            assert!(
                contains_id(&state.nodes.get(), "f0-o0"),
                "descendant came back with ancestor"
            );
        });
    }

    #[test]
    fn r905_rename_selected_template_numbers_and_undoes() {
        Owner::new().run(|| {
            let state = use_tree_state();
            state.set_selection(&BTreeSet::from([String::from("f10"), String::from("f11")]));
            let before10 = label_of(&state, "f10").unwrap();
            assert_eq!(state.rename_selected("Layer {n}"), 2, "two renamed");
            // Pre-order numbering: f10 then f11.
            assert_eq!(label_of(&state, "f10").as_deref(), Some("Layer 1"));
            assert_eq!(label_of(&state, "f11").as_deref(), Some("Layer 2"));
            assert!(state.undo(), "undo steps");
            assert_eq!(label_of(&state, "f10"), Some(before10), "label restored");
        });
    }

    #[test]
    fn r905_rename_literal_template_renames_all_same() {
        Owner::new().run(|| {
            let state = use_tree_state();
            state.set_selection(&BTreeSet::from([String::from("f2"), String::from("f3")]));
            assert_eq!(state.rename_selected("Group"), 2);
            assert_eq!(label_of(&state, "f2").as_deref(), Some("Group"));
            assert_eq!(label_of(&state, "f3").as_deref(), Some("Group"));
        });
    }

    #[test]
    fn r905_delete_empty_selection_is_noop() {
        Owner::new().run(|| {
            let state = use_tree_state();
            state.clear();
            assert_eq!(
                state.delete_selected(),
                0,
                "nothing selected, nothing deleted"
            );
            assert!(!state.undo(), "no undo step was recorded");
        });
    }

    #[test]
    fn r905_rename_noop_when_label_unchanged() {
        Owner::new().run(|| {
            let state = use_tree_state();
            // Rename f4 to its current label → no change, no undo step.
            let current = label_of(&state, "f4").unwrap();
            state.set_selection(&BTreeSet::from([String::from("f4")]));
            assert_eq!(state.rename_selected(&current), 0, "same label is a no-op");
            assert!(!state.undo(), "no undo step recorded for a no-op rename");
        });
    }

    #[test]
    fn r906_delete_key_removes_selection_and_ctrl_z_undoes() {
        Owner::new().run(|| {
            let state = use_tree_state();
            state.set_selection(&BTreeSet::from([String::from("f7")]));
            // Delete (no command modifier) removes the selected subtree.
            assert!(
                apply_key_impl("Delete", Modifiers::empty()),
                "Delete removes the selection"
            );
            assert!(
                !contains_id(&state.nodes.get(), "f7"),
                "f7 deleted via the keyboard"
            );
            // Ctrl+Z restores it (the same undo the RPC `undo` op drives).
            assert!(apply_key_impl("z", CTRL), "Ctrl+Z undoes");
            assert!(
                contains_id(&state.nodes.get(), "f7"),
                "f7 restored via the keyboard"
            );
            // Ctrl+Shift+Z redoes.
            let shift_ctrl = Modifiers {
                shift: true,
                ctrl: true,
                alt: false,
                meta: false,
            };
            assert!(apply_key_impl("z", shift_ctrl), "Ctrl+Shift+Z redoes");
            assert!(!contains_id(&state.nodes.get(), "f7"), "f7 re-deleted");
        });
    }
}
