//! `hello-virtual-tree` — R819 §5.16 §5.27 §5.50 **virtualized
//! `TreeView`**.
//!
//! R671 landed the `TreeView` paint substrate; R809/R811 lifted the
//! tree-navigation algorithm (`flat_visible` / `resolve_tree_key`); R744 /
//! R774 landed list virtualization (`compute_visible_range` + the
//! `AutoSizer` windowing). This binding closes the gap the
//! `pinion_widget_paint::tree_view` module doc named for the Phase D
//! editor: a tree large enough that a full flat row walk is a paint-cycle
//! bottleneck, so only the rows inside the scroll window become scene
//! nodes. It is the first consumer of
//! [`pinion_widget_paint::tree_view::view_virtual_tree`].
//!
//! ## The dataset
//!
//! [`SECTIONS`] top-level "Section NNN" folders, each holding
//! [`CHILDREN_PER`] "Item NNNN" leaves — [`TOTAL_NODES`] nodes in all. The
//! first [`EXPANDED_AT_BOOT`] sections boot expanded, so the visible-row
//! count starts at several hundred while the rendered node count stays
//! ~viewport-sized. Expanding or collapsing a section changes the visible
//! count by [`CHILDREN_PER`]; the window re-derives from the new length.
//!
//! ## The witness (§2 #7 scene-as-data, virtualization)
//!
//! `scene/snapshot` over the painted tree reports only the
//! `{TREE_TAG}#{id}` rows in the current scroll window — never the
//! hundreds (or thousands, once sections expand) of visible rows, and
//! never the [`TOTAL_NODES`] dataset. Scroll and a different id set
//! materializes; the off-window rows never existed as scene nodes (see
//! `tools/r819_virtual_tree.py`). The WAI-ARIA `treeitem` nodes window
//! with the paint — the AT tree exposes exactly the realized rows, each
//! carrying its own sibling-group `aria-level` / `aria-posinset` /
//! `aria-setsize` / `aria-expanded`.
//!
//! ## Interaction
//!
//! Click a section row to expand / collapse it (the R674
//! [`TreeRowClickExternal`] composite-tag click path, routed into the
//! shared [`toggle_expanded`] sink). Keyboard roving + scroll-into-view
//! over the windowed rows is a later additive axis — this slice is
//! pointer + RPC + AT, the R730 / `hello-virtual-select` windowed-widget
//! keyboard-defer precedent. The vertical motion, when it lands, reuses
//! the `clamp_nav` + `scroll_offset_to_reveal` SSOTs the family already
//! owns; no new substrate.

use pinion_a11y::{tree_access_nodes, AccessNode, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::tree_nav::{flat_visible, TreeNode, VisibleRow};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::{reactive, Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::{vello_renderer_impl, SizeStrategy, WidgetView};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::tree_view::{view_virtual_tree, TreeViewFocus, TreeViewStyle};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloVirtualTreeRenderer, HelloVirtualTreeRendererError);

const WIN_W: u32 = 480;
const WIN_H: u32 = 520;
const THEME_TAG: &str = "app";
/// Composite-tag prefix the windowed rows carry (`{TREE_TAG}#{id}`) and
/// the [`TreeRowClickExternal`] anchor clicks route to.
const TREE_TAG: &str = "vtree";
/// Focusable tree-root External tag (the WAI-ARIA `tree` node + the
/// `read_state` anchor). Kept distinct from [`TREE_TAG`] like
/// `hello-tree-view` (root External vs row container).
const ROOT_TAG: &str = "vtree_root";
const SCROLL_KEY: &str = "vtree_scroll";
const SCROLLBAR_TAG: &str = "vtree_scrollbar";

/// Number of top-level section folders.
const SECTIONS: usize = 100;
/// Leaf children per section.
const CHILDREN_PER: usize = 99;
/// Total dataset size: each section row + its leaves.
const TOTAL_NODES: usize = SECTIONS * (1 + CHILDREN_PER);
/// Sections expanded at boot, so the visible-row count starts well above
/// the rendered window (the virtualization is obvious from frame one).
const EXPANDED_AT_BOOT: usize = 4;

/// Uniform per-row vertical slot. Must equal
/// [`TreeViewStyle::row_height`] (`view_virtual_tree` derives its slot
/// pitch from the style); asserted in the view.
const ROW_PITCH: u32 = 48;
/// Rows rendered beyond the strict window on each side.
const OVERSCAN: usize = 3;
/// Gutter reserved for the scrollbar peer.
const SCROLLBAR_W: u32 = 12;
const HEADER_FONT_PX: u32 = 14;

/// The dotted wire form of the [`TreeRowClickExternal`] row-click intent,
/// composed via [`intent_tag!`] so the literal stays in lockstep with the
/// substrate's bare `"click"` event name (pinned in [`tests`]).
const CLICK_INTENT_TAG: &str = intent_tag!("vtree", "click");

/// One tree node. Carries its own `expanded` flag (the retained
/// flag-on-node storage `hello-tree-view` also uses; the dock-panels
/// inspector instead uses a collapsed-path overlay — R811 "storage is
/// caller-choice", the lifted SSOT is the `flat_visible` algorithm all
/// three share). The `serde` + `PartialEq` derives satisfy the §5.22
/// introspect bound `Signal<T>` carries on its payload.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct TreeRow {
    id: String,
    label: String,
    expanded: bool,
    children: Vec<TreeRow>,
}

impl TreeRow {
    fn leaf(id: String, label: String) -> Self {
        Self {
            id,
            label,
            expanded: false,
            children: Vec::new(),
        }
    }
}

impl TreeNode for TreeRow {
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
}

/// Build the synthetic [`TOTAL_NODES`]-node tree. Deterministic ids
/// (`s{section}` / `s{section}-i{item}`) so the RPC demo can address rows
/// by a stable composite tag.
fn initial_nodes() -> Vec<TreeRow> {
    (0..SECTIONS)
        .map(|s| {
            let children = (0..CHILDREN_PER)
                .map(|i| {
                    TreeRow::leaf(format!("s{s}-i{i}"), format!("Item {s:03}-{i:04}"))
                })
                .collect();
            TreeRow {
                id: format!("s{s}"),
                label: format!("Section {s:03}"),
                expanded: s < EXPANDED_AT_BOOT,
                children,
            }
        })
        .collect()
}

/// Reactive holder for the retained tree. Lifted into [`Owner::cache`] so
/// every view-fn pass + the a11y pass + the click reducer read the same
/// `Signal`. Mirrors `hello-tree-view`'s `TreeState`; the storage glue is
/// per-consumer by R811 (the inspector diverges to a collapsed-path set).
struct TreeState {
    nodes: Signal<Vec<TreeRow>>,
}

fn use_tree_state() -> Rc<TreeState> {
    let owner = Owner::current()
        .expect("use_tree_state must run inside a CoreShell view / reducer wrap");
    owner.cache("hello_virtual_tree::state", || TreeState {
        nodes: Signal::new(initial_nodes()),
    })
}

/// Recursive find-by-id for a mutable node. Pure tree iteration (no
/// decision encoded), so per the R818 "lift decisions, not iterations"
/// rule it stays binding-side; the decision-bearing SSOT is
/// `resolve_tree_key` / `flat_visible`. `hello-tree-view` has the twin;
/// the inspector (3rd tree consumer) diverges to a path-overlay set, so
/// the pair is below the Rule-of-Three for a lift.
fn find_node_mut<'a>(nodes: &'a mut [TreeRow], id: &str) -> Option<&'a mut TreeRow> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

/// Toggle the `expanded` flag on branch `id` (the click path). Leaves are
/// a no-op. [`reactive::batch`]ed so the single `Signal::set` repaints
/// once.
fn toggle_expanded(nodes_signal: &Signal<Vec<TreeRow>>, id: &str) {
    reactive::batch(|| {
        let mut nodes = nodes_signal.get();
        if let Some(node) = find_node_mut(&mut nodes, id) {
            if node.children.is_empty() {
                return;
            }
            node.expanded = !node.expanded;
            nodes_signal.set(nodes);
        }
    });
}

/// Header status line: the dataset size + the current visible-row count.
/// Pure scene-as-data so the RPC demo reads the counts (§2 #7); the
/// rendered window count is read from the painted `{TREE_TAG}#` rows.
fn header(visible: usize, theme: &Theme) -> Scene {
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    Scene::Text(TextNode::styled(
        format!("{TOTAL_NODES} nodes \u{00B7} {visible} rows visible \u{00B7} windowed render"),
        Rect::default(),
        TextStyle::new().with_size_px(HEADER_FONT_PX).with_fg(muted),
    ))
}

/// view-fn (§6.3): pure sync mapping. The dataset is virtual —
/// `view_virtual_tree` builds rows only for the indices in the current
/// scroll window, re-derived from `flat_visible(&nodes)`.
#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(_state: ButtonState, _frame: &Frame) -> Scene {
    // `view_virtual_tree` derives its slot pitch from `style.row_height`;
    // keep ROW_PITCH (the a11y windowing math uses it) in lockstep.
    debug_assert_eq!(TreeViewStyle::m3_default().row_height, ROW_PITCH);

    let theme = use_theme(THEME_TAG).theme_animated();
    let tree_state = use_tree_state();
    let nodes = tree_state.nodes.get();
    let rows = flat_visible(&nodes);
    let scroll = use_scroll_state(SCROLL_KEY);

    let tree = view_virtual_tree(
        TREE_TAG,
        &scroll,
        &rows,
        &TreeViewFocus::default(),
        OVERSCAN,
        &theme,
        &TreeViewStyle::m3_default(),
    );

    let (_, measured_h) = scroll.measured_viewport();
    let scrollbar_style = VerticalScrollbarStyle::material(measured_h, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual =
        view_vertical_scrollbar(&scroll, &theme, &scrollbar_style, scrollbar_interaction.get());

    // Row band: the windowed tree beside the scrollbar peer, flex-grow so
    // it fills the window below the header. The measured-viewport
    // windowing reads its height from this flex-computed band.
    let band = Scene::Container(
        ContainerNode::new(vec![tree, scrollbar_visual]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_flex_grow(1.0),
        ),
    );

    // R819 — invisible 0x0 root External keeps the SCXML state surface
    // alive for read_state / RPC introspect (the WAI-ARIA `tree` node also
    // lands here). No visual button paints. Mirrors `hello-tree-view`.
    let invisible_root = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(ROOT_TAG)
            .with_layout(LayoutStyle::new().with_size(Size::px(0, 0))),
    );

    Scene::Container(
        ContainerNode::new(vec![header(rows.len(), &theme), band, invisible_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start)
                    .with_gap(8)
                    .with_padding(Rect::new(12, 12, 12 + SCROLLBAR_W, 12)),
            ),
    )
}

struct VirtualTreeView;

impl WidgetCore for VirtualTreeView {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn title() -> &'static str {
        "pinion hello-virtual-tree (R819 §5.27 virtualized TreeView)"
    }

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    /// `TreeRowClickExternal` sibling (composite `{TREE_TAG}#{id}` clicks
    /// → the §5.20 intent channel → [`VirtualTreeView::update`]) + the
    /// `ScrollBarExternal` sharing the tree's `Rc<ScrollState>`.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![
            ExtraExternal::new(
                TREE_TAG,
                Box::new(pinion_widget_paint::tree_view::TreeRowClickExternal::new()),
            ),
            scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG),
        ]
    }

    fn read_state(scene: &Scene) -> Self::State {
        if let Some(node) = scene.find_external_with_tag(ROOT_TAG)
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return <Self::State as pinion_core::WidgetStateName>::from_name_or_default(&name);
        }
        ButtonState::Idle
    }

    fn event_name(event: Self::Event) -> &'static str {
        <Self::Event as pinion_core::WidgetEventName>::as_name(&event)
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, frame)
    }

    /// Click a section row → toggle its `expanded` flag. Side-effect-only
    /// reducer ([[scxml-as-model-update-transient]]): the `Signal::set`
    /// inside [`toggle_expanded`] is the mutation, so the command list is
    /// empty. Matches the dotted wire form per [[intent-tag-dotted-wire-form]].
    fn update(_state: Self::State, intent: &Intent) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == CLICK_INTENT_TAG
            && let IntrospectValue::Text(id) = &intent.payload
        {
            let tree_state = use_tree_state();
            toggle_expanded(&tree_state.nodes, id);
        }
        Vec::new()
    }

    /// Pointer + RPC this slice; the windowed tree is not yet a keyboard
    /// tab stop (roving + scroll-into-view is the R820 additive axis, the
    /// `hello-virtual-select` windowed-widget defer precedent). Empty so
    /// Tab never lands on a windowed row.
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn fmt_state_log(_state: &ButtonState) -> String {
        format!("virtualized tree of {TOTAL_NODES} nodes (windowed render)")
    }
}

impl WidgetA11y for VirtualTreeView {
    /// WAI-ARIA virtualized `tree`: one `AriaRole::Tree` root (at the
    /// focusable [`ROOT_TAG`]) referencing only the **windowed** rows,
    /// plus one `AriaRole::TreeItem` per rendered row carrying its
    /// sibling-group `aria-level` / `aria-posinset` / `aria-setsize` /
    /// `aria-expanded` (preserved from the `flat_visible` SSOT, so the AT
    /// announces exactly the rows the paint window renders). Built through
    /// the lifted [`tree_access_nodes`] (R812) over the same window the
    /// view paints — the a11y tree and the painted tree never diverge.
    /// This slice carries no selection model, so `selected_id` is `None`.
    fn access_node(_state: &ButtonState, _focused: Option<&str>) -> Vec<AccessNode> {
        let nodes = use_tree_state().nodes.get();
        let rows = flat_visible(&nodes);
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window =
            compute_visible_range(scroll.offset_y(), measured_h, rows.len(), ROW_PITCH, OVERSCAN);
        let slice: &[VisibleRow] = &rows[window.first..window.first + window.count];
        tree_access_nodes(ROOT_TAG, TREE_TAG, Some("Virtual file tree"), slice, None)
    }
}

impl WidgetView for VirtualTreeView {
    type Renderer = HelloVirtualTreeRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<VirtualTreeView>();
}

#[cfg(test)]
mod tests {
    use super::{
        flat_visible, initial_nodes, toggle_expanded, use_tree_state, view, TreeRow,
        VirtualTreeView, CHILDREN_PER, CLICK_INTENT_TAG, EXPANDED_AT_BOOT, ROOT_TAG, ROW_PITCH,
        SCROLL_KEY, SECTIONS, TOTAL_NODES, TREE_TAG,
    };
    use pinion_a11y::{AriaRole, WidgetA11y};
    use pinion_core::widgets::button::ButtonState;
    use pinion_core::widgets::scroll::use_scroll_state;
    use pinion_core::{Frame, Owner, Scene};
    use pinion_widget_paint::tree_view::{TreeViewStyle, TREE_ROW_CLICK_EVENT};

    fn count_nodes(nodes: &[TreeRow]) -> usize {
        nodes.iter().map(|n| 1 + count_nodes(&n.children)).sum()
    }

    #[test]
    fn dataset_size_matches_total_nodes() {
        let nodes = initial_nodes();
        assert_eq!(count_nodes(&nodes), TOTAL_NODES, "generated node count");
        assert_eq!(nodes.len(), SECTIONS, "top-level section count");
    }

    #[test]
    fn boot_visible_count_reflects_expanded_sections() {
        // Sections collapsed contribute 1 row; the first EXPANDED_AT_BOOT
        // contribute 1 + CHILDREN_PER.
        let visible = flat_visible(&initial_nodes()).len();
        let expected = SECTIONS + EXPANDED_AT_BOOT * CHILDREN_PER;
        assert_eq!(visible, expected, "boot visible rows");
        assert!(visible > 64, "boot visible far exceeds a viewport window");
    }

    #[test]
    fn slot_pitch_matches_style_row_height() {
        // `view_virtual_tree` derives its pitch from the style; the a11y
        // windowing const must track it.
        assert_eq!(TreeViewStyle::m3_default().row_height, ROW_PITCH);
    }

    #[test]
    fn click_intent_tag_matches_substrate_event() {
        // Lockstep: the dotted reducer arm vs the substrate bare name.
        assert_eq!(CLICK_INTENT_TAG, format!("{TREE_TAG}.{TREE_ROW_CLICK_EVENT}"));
    }

    #[test]
    fn toggle_collapses_then_expands_a_branch() {
        Owner::new().run(|| {
            let state = use_tree_state();
            let before = flat_visible(&state.nodes.get()).len();
            // Section 0 boots expanded → collapsing it drops CHILDREN_PER.
            toggle_expanded(&state.nodes, "s0");
            let collapsed = flat_visible(&state.nodes.get()).len();
            assert_eq!(collapsed, before - CHILDREN_PER, "collapse drops children");
            toggle_expanded(&state.nodes, "s0");
            let reexpanded = flat_visible(&state.nodes.get()).len();
            assert_eq!(reexpanded, before, "re-expand restores children");
        });
    }

    #[test]
    fn toggle_on_leaf_is_a_noop() {
        Owner::new().run(|| {
            let state = use_tree_state();
            let before = flat_visible(&state.nodes.get()).len();
            toggle_expanded(&state.nodes, "s0-i0"); // a leaf
            assert_eq!(flat_visible(&state.nodes.get()).len(), before, "leaf toggle no-op");
        });
    }

    /// Count the rendered `{TREE_TAG}#<id>` row containers, descending the
    /// `Scroll` wrapper.
    fn rendered_row_count(scene: &Scene) -> usize {
        fn walk(scene: &Scene, prefix: &str) -> usize {
            match scene {
                Scene::Scroll(s) => walk(s.content.as_ref(), prefix),
                Scene::Container(c) => {
                    let here = usize::from(
                        c.tag.as_deref().is_some_and(|t| t.starts_with(prefix)),
                    );
                    here + c.children.iter().map(|ch| walk(ch, prefix)).sum::<usize>()
                }
                _ => 0,
            }
        }
        walk(scene, &format!("{TREE_TAG}#"))
    }

    #[test]
    fn view_renders_only_the_window_not_all_visible_rows() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(440, 10 * ROW_PITCH); // 10-row viewport
            view(ButtonState::Idle, &Frame::default())
        });
        let visible = flat_visible(&initial_nodes()).len();
        let rendered = rendered_row_count(&scene);
        assert!(rendered > 0, "some rows render");
        assert!(
            rendered < 32,
            "rendered window {rendered} is a small slice of {visible} visible rows"
        );
    }

    #[test]
    fn access_node_windows_the_treeitems() {
        // The AT tree exposes only the rendered window: one `Tree` root +
        // one `TreeItem` per windowed row (far fewer than the visible
        // rows), each carrying its WAI-ARIA hierarchical axes.
        let owner = Owner::new();
        let nodes = owner.run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(440, 10 * ROW_PITCH);
            VirtualTreeView::access_node(&ButtonState::Idle, None)
        });
        let visible = flat_visible(&initial_nodes()).len();
        let root = &nodes[0];
        assert_eq!(root.role, AriaRole::Tree, "first node is the tree root");
        assert_eq!(root.tag, ROOT_TAG, "root at the focusable tree tag");
        let items = &nodes[1..];
        assert!(!items.is_empty(), "some treeitems exposed");
        assert!(
            items.len() < 32,
            "AT exposes only the window ({}) of {visible} visible rows",
            items.len()
        );
        for item in items {
            assert_eq!(item.role, AriaRole::TreeItem, "windowed rows are treeitems");
            assert!(item.level.is_some(), "each treeitem carries aria-level");
            assert!(item.position_in_set.is_some(), "each treeitem carries aria-posinset");
            assert!(item.size_of_set.is_some(), "each treeitem carries aria-setsize");
        }
        // The top section row (a branch) advertises its expanded state.
        let s0 = items
            .iter()
            .find(|n| n.tag.ends_with("#s0"))
            .expect("section 0 in the boot window");
        assert_eq!(s0.expanded, Some(true), "s0 boots expanded (aria-expanded=true)");
    }
}
