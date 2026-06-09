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
//! row count changes and the grid re-windows. Clicking also focuses the row
//! (highlighted across both panes). Keyboard roving is a later additive axis
//! (the `hello-virtual-tree` windowed-widget keyboard-defer precedent).
//!
//! ## The witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` reports the windowed `{TREE_TAG}#{id}` name cells +
//! `{TREE_TAG}_drow{id}` metadata strips. `scene/scroll` on the horizontal
//! scroll (`tgrid_hscroll`) shifts the metadata columns left while the name
//! column stays put (the freeze); `scene/scroll` on the body (`tgrid_scroll`)
//! slides BOTH panes in lockstep. `scene/query` on the [`TREE_STATE_TAG`]
//! query-only introspection External reports the FULL visible-row count (the
//! virtualization sees only a window, so the AI reads structure here, not
//! from the painted nodes). See `tools/demos/r860_tree_grid.py`.
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
//! no `gridcell` children in WAI-ARIA).

use pinion_a11y::{treegrid_nodes, AccessNode, WidgetA11y};
use pinion_core::external::{External, IntrospectValue, StubExternal};
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::ContainerNode;
use pinion_core::style::{AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::tree_nav::{
    flat_visible, toggle_expanded, tree_view_introspection_extra, TreeNode,
};
use pinion_core::{Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::table::GridScroll;
use pinion_widget_paint::tree_view::{
    view_virtual_treegrid, TreeGridData, TreeRowClickExternal, TreeViewStyle,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTreeGridRenderer, HelloTreeGridRendererError);

/// Initial window size. Narrower than the tree column + all metadata
/// columns so the metadata overflows and scrolls while the name column
/// stays pinned.
const WIN_W: u32 = 480;
const WIN_H: u32 = 460;
/// Shared [`ThemeProvider`] cache key.
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
        Self { id, label, expanded: false, children: Vec::new() }
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

/// Reactive holder for the retained tree + the focused/selected row, lifted
/// into [`Owner::cache`] so the view-fn, the a11y pass, and the click
/// reducer read the same `Signal`s.
struct TreeState {
    nodes: Signal<Vec<OutlinerNode>>,
    focused_id: Signal<Option<String>>,
}

fn use_tree_state() -> std::rc::Rc<TreeState> {
    let owner = Owner::current().expect("use_tree_state must run inside a CoreShell view / reducer wrap");
    owner.cache("hello_tree_grid::state", || TreeState {
        nodes: Signal::new(initial_nodes()),
        focused_id: Signal::new(None),
    })
}

/// view-fn (§6.3): pure sync mapping. The dataset is virtual —
/// `view_virtual_treegrid` builds cells only for the indices in the current
/// scroll window, re-derived from `flat_visible(&nodes)`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let tree_state = use_tree_state();
    let nodes = tree_state.nodes.get();
    let rows = flat_visible(&nodes);
    let focused = tree_state.focused_id.get();
    let scroll = use_scroll_state(SCROLL_KEY);
    let h_scroll = use_scroll_state(H_SCROLL_KEY);

    let grid = view_virtual_treegrid(
        TREE_TAG,
        GridScroll { body: &scroll, horizontal: &h_scroll },
        &TreeGridData {
            rows: &rows,
            tree_header: TREE_HEADER,
            data_headers: &DATA_HEADERS,
            tree_col_width: TREE_COL_W,
            data_col_width: DATA_COL_W,
            overscan: OVERSCAN,
        },
        focused.as_deref(),
        &theme,
        &TreeViewStyle::m3_default(),
        cell_data,
    );

    // R819 — invisible 0x0 root anchor keeps the WAI-ARIA `tree` node + the
    // focus surface alive (mirrors `hello-virtual-tree`); no visual paints.
    let invisible_root = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(ROOT_TAG)
            .with_layout(LayoutStyle::new().with_size(Size::px(0, 0))),
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
        let nodes = tree_state.nodes.clone();
        let focused = tree_state.focused_id.clone();
        vec![
            ExtraExternal::new(TREE_TAG, Box::new(TreeRowClickExternal::new())),
            tree_view_introspection_extra(
                TREE_STATE_TAG,
                move || flat_visible(&nodes.get()),
                move || focused.get(),
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

    /// Click a row → focus it + toggle its `expanded` flag (a no-op on a
    /// leaf, which `flat_visible` ignores). Side-effect-only reducer
    /// ([[scxml-as-model-update-transient]]): the `Signal::set`s are the
    /// mutation, so the command list is empty.
    fn update(_state: (), intent: &Intent) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == CLICK_INTENT_TAG
            && let IntrospectValue::Text(id) = &intent.payload
        {
            let tree_state = use_tree_state();
            tree_state.focused_id.set(Some(id.clone()));
            toggle_expanded(&tree_state.nodes, id);
        }
        Vec::new()
    }

    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
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
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let tree_state = use_tree_state();
        let rows = flat_visible(&tree_state.nodes.get());
        let focused = tree_state.focused_id.get();
        treegrid_nodes(
            ROOT_TAG,
            TREE_TAG,
            Some("Scene outliner"),
            TREE_HEADER,
            &DATA_HEADERS,
            &rows,
            focused.as_deref(),
        )
    }
}

impl WidgetView for TreeGridView {
    type Renderer = HelloTreeGridRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<TreeGridView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_overflows_the_window() {
        // The premise: tree column + all metadata columns exceed the window,
        // so the metadata pane genuinely scrolls horizontally past the freeze.
        // Sum the real per-column widths (a runtime fold, not a const).
        let total: u32 = TREE_COL_W + DATA_HEADERS.iter().map(|_| DATA_COL_W).sum::<u32>();
        assert!(total > WIN_W, "tree + metadata ({total}) must exceed WIN_W ({WIN_W})");
        // The frozen tree column leaves room for visible metadata beside it.
        let metadata_visible = WIN_W.saturating_sub(TREE_COL_W);
        assert!(metadata_visible > 0, "frozen column leaves room for metadata");
    }

    #[test]
    fn cell_data_distinguishes_folders_from_objects() {
        assert_eq!(cell_data("f3", 0), "Folder", "a folder id (no '-') is a Folder");
        assert_ne!(cell_data("f3-o1", 0), "Folder", "an object id is not a Folder");
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
        assert_eq!(rows.len(), expected, "boot visible rows = folders + expanded children");
    }
}
