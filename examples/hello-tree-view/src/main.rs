// R673 §5.16 — example bindings tolerate looser doc-markdown lints
// than substrate crates. The example narrative carries a lot of
// proper-noun identifiers (`TreeView`, `DevTools`, etc.) inside
// architectural doc comments; pushing every one through backticks
// inflates the prose for negligible signal gain.
#![allow(clippy::doc_markdown)]

//! `hello-tree-view` — R673 §5.16 §5.50 second consumer of the
//! `pinion_widget_paint::tree_view` substrate (the R671 first
//! consumer was the read-only `hello-multi-window` inspector).
//!
//! ## Why this binding exists
//!
//! Per [[abstraction-needs-second-consumer]], framework substrates
//! land their canonical maturity at the second consumer. R671 lifted
//! the TreeView paint helper for a single consumer (the inspector
//! tree mirroring main's paint scene) — read-only, no keyboard model,
//! no focus highlight. R673 adds the second consumer to verify the
//! substrate scales: an interactive keyboard-driven hierarchical
//! browser with Arrow Up/Down/Left/Right navigation, Home/End jump
//! to first/last visible row, Space/Enter to toggle expand on the
//! focused branch, and a Material 3 focused-row state-layer overlay
//! that follows the keyboard cursor.
//!
//! ## Keyboard model (WAI-ARIA 1.2 §6.13 Tree)
//!
//! - Arrow Up — move focus to the previous visible row (wraps at
//!   the top of the tree).
//! - Arrow Down — move focus to the next visible row (wraps at the
//!   bottom).
//! - Arrow Right — expand the focused branch (no-op on leaves and
//!   already-expanded branches; future axis: descend into the first
//!   child when already expanded, per the WAI-ARIA spec extended
//!   behaviour).
//! - Arrow Left — collapse the focused branch (no-op on leaves;
//!   future axis: jump to parent when already collapsed, per the
//!   WAI-ARIA spec extended behaviour).
//! - Home — focus the first visible row.
//! - End — focus the last visible row.
//! - Space / Enter — toggle expanded on the focused branch (no-op
//!   on leaves).
//!
//! ## Non-goals for R673
//!
//! Click-to-expand: routing tag-scoped click events into per-row
//! `ExtraExternal` handlers is the canonical pattern (`composite_tag`
//! 5-of-5 in todomvc) but multiplies binding LOC linearly with the
//! row count. R673 demonstrates keyboard-driven interactivity as the
//! WAI-ARIA spec's primary model; click-to-expand is a future axis
//! once a real consumer (file-tree editor, property grid) surfaces
//! the substrate-incompleteness signal.
//!
//! Multi-select, drag-drop, inline rename: all deferred to future
//! consumers per [[abstraction-needs-second-consumer]].

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::IntrospectValue;
use pinion_core::intent::Intent;
use pinion_core::intent_tag;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{reactive, Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::{vello_renderer_impl, SizeStrategy, WidgetView};
use pinion_widget_paint::tree_view::{
    view_tree_focused, TreeItem, TreeRowClickExternal, TreeViewFocus, TreeViewStyle,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloTreeViewRenderer, HelloTreeViewRendererError);

const TREE_TAG: &str = "file_tree";
const ROOT_BTN_TAG: &str = "tree_root";
const THEME_TAG: &str = "app";

const WIN_W: u32 = 480;
const WIN_H: u32 = 400;

/// R674 §5.20 — dotted wire-form intent tag the [`WidgetView::update`]
/// reducer matches against for click-driven row toggles. Compile-time
/// concat of [`TREE_TAG`] and the substrate's `"click"` event name
/// via the [`intent_tag!`] macro so the literal stays in lockstep
/// with the substrate-emitting External + the View tag. (R675 §5.16 —
/// the External lifted to `pinion_widget_paint::tree_view::TreeRowClickExternal`,
/// the event name lifted to
/// [`pinion_widget_paint::tree_view::TREE_ROW_CLICK_EVENT`] = `"click"`;
/// the binding owns the dotted form because the `intent_tag!` macro
/// is `literal`-only at the stable-Rust layer.)
const FILE_TREE_CLICK_INTENT_TAG: &str = intent_tag!("file_tree", "click");

const HEADER_FONT_PX: u32 = 14;
const FOOTER_FONT_PX: u32 = 12;
const HEADER_BOTTOM_GAP: u32 = 4;
const FOOTER_TOP_GAP: u32 = 8;

/// R673 §5.50 — sample tree data. Three top-level branches (`src`,
/// `tests`, `docs`) each carrying a small child tree. The data lives
/// in a `Signal<Vec<FileNode>>` so [`apply_key`] mutations
/// (expand/collapse toggles) re-run the view fn deterministically.
/// `serde::Serialize` / `Deserialize` derive is required because
/// `pinion_core::Signal<T>` carries the §5.22 introspect bound on T;
/// the bound traces back to `pinion_core::reactive::Signal::set`'s
/// preview-snapshot path even when the binding never actually emits
/// preview frames against this signal.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct FileNode {
    id: String,
    label: String,
    expanded: bool,
    children: Vec<FileNode>,
}

impl FileNode {
    fn leaf<I: Into<String>, L: Into<String>>(id: I, label: L) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            expanded: false,
            children: Vec::new(),
        }
    }

    fn branch<I: Into<String>, L: Into<String>>(
        id: I,
        label: L,
        expanded: bool,
        children: Vec<FileNode>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            expanded,
            children,
        }
    }

    fn to_tree_item(&self) -> TreeItem {
        TreeItem::branch(
            self.id.clone(),
            self.label.clone(),
            self.expanded,
            self.children.iter().map(FileNode::to_tree_item).collect(),
        )
    }
}

fn initial_nodes() -> Vec<FileNode> {
    vec![
        FileNode::branch(
            "src",
            "src",
            true,
            vec![
                FileNode::leaf("src/main.rs", "main.rs"),
                FileNode::leaf("src/lib.rs", "lib.rs"),
                FileNode::branch(
                    "src/widgets",
                    "widgets",
                    false,
                    vec![
                        FileNode::leaf("src/widgets/mod.rs", "mod.rs"),
                        FileNode::leaf("src/widgets/tree_view.rs", "tree_view.rs"),
                    ],
                ),
            ],
        ),
        FileNode::branch(
            "tests",
            "tests",
            false,
            vec![
                FileNode::leaf("tests/integration.rs", "integration.rs"),
                FileNode::leaf("tests/snapshot.rs", "snapshot.rs"),
            ],
        ),
        FileNode::branch(
            "docs",
            "docs",
            false,
            vec![FileNode::leaf("docs/README.md", "README.md")],
        ),
    ]
}

/// R673 §5.50 — reactive primitive holding the binding's mutable
/// tree state. `nodes` carries the data + per-node expanded flags;
/// `focused_id` carries the row id the keyboard cursor sits on.
/// Lifted into `Owner::cache` so every view-fn pass reads the same
/// shared instance.
fn use_tree_state() -> std::rc::Rc<TreeState> {
    Owner::current()
        .expect("hello-tree-view: view fn runs inside the substrate root owner scope")
        .cache("hello_tree_view_state", || TreeState {
            nodes: Signal::new(initial_nodes()),
            focused_id: Signal::new(Some(String::from("src"))),
        })
}

struct TreeState {
    nodes: Signal<Vec<FileNode>>,
    focused_id: Signal<Option<String>>,
}

fn view(state: ButtonState) -> Scene {
    let _ = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let surface = theme.resolve(ColorRole::Surface);
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let tree_state = use_tree_state();
    let nodes = tree_state.nodes.get();
    let focused = tree_state.focused_id.get();
    let items: Vec<TreeItem> = nodes.iter().map(FileNode::to_tree_item).collect();
    let focus = TreeViewFocus {
        focused_id: focused.as_deref(),
    };
    let tree_scene = view_tree_focused(
        TREE_TAG,
        &items,
        &theme,
        &TreeViewStyle::m3_default(),
        &focus,
    );

    let header = Scene::Text(TextNode::styled(
        "File explorer (TreeView demo)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_fg(on_surface_muted),
    ));
    let footer = Scene::Text(TextNode::styled(
        "\u{2191}/\u{2193} navigate  \u{2192} expand  \u{2190} collapse  Space toggle  Home/End jump",
        Rect::default(),
        TextStyle::new()
            .with_size_px(FOOTER_FONT_PX)
            .with_fg(on_surface_muted),
    ));
    // R673 §5.50 — invisible Button widget at the scene root keeps
    // the framework's SCXML state surface alive for read_state /
    // forward / RPC introspect. The keyboard interactivity is driven
    // by [`apply_key`] which writes directly to the reactive
    // [`TreeState`] signals; the Button state stays at Idle/Hover
    // depending on the SCXML transition, but no visual button paints.
    let invisible_root = Scene::Container(
        ContainerNode::new(Vec::new())
            .with_tag(ROOT_BTN_TAG)
            .with_layout(LayoutStyle::new().with_size(Size::px(0, 0))),
    );

    let header_text = Scene::Text(TextNode::styled(
        "",
        Rect::default(),
        TextStyle::new()
            .with_size_px(HEADER_FONT_PX)
            .with_fg(on_surface),
    ));
    let _ = header_text;
    Scene::Container(
        ContainerNode::new(vec![header, tree_scene, footer, invisible_root])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    // R673 §5.50 — Stretch so the TreeView fills the
                    // window's cross-axis width; rows then expand to
                    // the full width via the substrate's own
                    // `AlignItems::Stretch` on the tree root.
                    .with_align_items(AlignItems::Stretch)
                    .with_gap(HEADER_BOTTOM_GAP.max(FOOTER_TOP_GAP))
                    .with_padding(Rect::new(12, 12, 12, 12)),
            ),
    )
}

/// Recursive helper: append `node.id` then recurse into expanded
/// children. Hoisted out of [`flat_visible_ids`] so
/// `clippy::items_after_statements` stays clean.
fn walk_visible_ids(node: &FileNode, out: &mut Vec<String>) {
    out.push(node.id.clone());
    if node.expanded {
        for child in &node.children {
            walk_visible_ids(child, out);
        }
    }
}

/// R673 §5.50 — DFS walk that produces the flat visible-row sequence
/// (exactly what `view_tree_focused` paints). Used by `apply_key` to
/// resolve Arrow Up/Down/Home/End targets in O(visible rows).
fn flat_visible_ids(nodes: &[FileNode]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for node in nodes {
        walk_visible_ids(node, &mut out);
    }
    out
}

fn find_node_mut<'a>(
    nodes: &'a mut [FileNode],
    target_id: &str,
) -> Option<&'a mut FileNode> {
    for node in nodes {
        if node.id == target_id {
            return Some(node);
        }
        if let Some(found) = find_node_mut(&mut node.children, target_id) {
            return Some(found);
        }
    }
    None
}

fn has_children(nodes: &[FileNode], target_id: &str) -> bool {
    for node in nodes {
        if node.id == target_id {
            return !node.children.is_empty();
        }
        if has_children(&node.children, target_id) {
            return true;
        }
    }
    false
}

/// R674 §5.50 — single source of truth for "toggle the expanded flag
/// on `id`". Used by both [`apply_key_impl`]'s `Space` / `Enter` arm
/// (keyboard path) and [`TreeViewBinding::update`]'s
/// `file_tree.click` reducer arm (click path), so the two paths
/// produce bit-identical reactive state mutations on identical input
/// and the §6.3 `dry_run` invariant continues to hold under both
/// input modes.
///
/// Leaves are a no-op: only branches with children expand or
/// collapse. Wrapping the mutation in [`reactive::batch`] suppresses
/// double-paint on the single-Signal `set` that follows, matching
/// the keyboard handler's pre-R674 wire.
fn toggle_expanded_in_signal(nodes_signal: &Signal<Vec<FileNode>>, id: &str) {
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

fn apply_key_impl(key: &str) -> bool {
    let tree_state = use_tree_state();
    let nodes_clone = tree_state.nodes.get();
    let visible_ids = flat_visible_ids(&nodes_clone);
    if visible_ids.is_empty() {
        return false;
    }
    let current = tree_state.focused_id.get();
    let current_idx = current
        .as_ref()
        .and_then(|id| visible_ids.iter().position(|v| v == id))
        .unwrap_or(0);
    match key {
        "ArrowUp" => {
            let new_idx = if current_idx == 0 {
                visible_ids.len() - 1
            } else {
                current_idx - 1
            };
            tree_state
                .focused_id
                .set(Some(visible_ids[new_idx].clone()));
            true
        }
        "ArrowDown" => {
            let new_idx = if current_idx + 1 >= visible_ids.len() {
                0
            } else {
                current_idx + 1
            };
            tree_state
                .focused_id
                .set(Some(visible_ids[new_idx].clone()));
            true
        }
        "Home" => {
            tree_state.focused_id.set(Some(visible_ids[0].clone()));
            true
        }
        "End" => {
            let last = visible_ids.len() - 1;
            tree_state.focused_id.set(Some(visible_ids[last].clone()));
            true
        }
        "ArrowRight" => {
            // Expand the focused branch (no-op on leaves + already-expanded).
            let Some(focused_id) = current else {
                return false;
            };
            if !has_children(&nodes_clone, &focused_id) {
                return false;
            }
            reactive::batch(|| {
                let mut nodes = tree_state.nodes.get();
                if let Some(node) = find_node_mut(&mut nodes, &focused_id) {
                    if node.expanded {
                        return;
                    }
                    node.expanded = true;
                    tree_state.nodes.set(nodes);
                }
            });
            true
        }
        "ArrowLeft" => {
            // Collapse the focused branch (no-op on leaves + already-collapsed).
            let Some(focused_id) = current else {
                return false;
            };
            if !has_children(&nodes_clone, &focused_id) {
                return false;
            }
            reactive::batch(|| {
                let mut nodes = tree_state.nodes.get();
                if let Some(node) = find_node_mut(&mut nodes, &focused_id) {
                    if !node.expanded {
                        return;
                    }
                    node.expanded = false;
                    tree_state.nodes.set(nodes);
                }
            });
            true
        }
        "Space" | "Enter" => {
            // R674 §5.50 — toggle expand on the focused branch (no-op
            // on leaves). Routes through the shared
            // [`toggle_expanded_in_signal`] helper so the keyboard
            // path and the [`FileTreeRowExternal`] click path produce
            // identical Signal mutations on identical input.
            let Some(focused_id) = current else {
                return false;
            };
            if !has_children(&nodes_clone, &focused_id) {
                return false;
            }
            toggle_expanded_in_signal(&tree_state.nodes, &focused_id);
            true
        }
        _ => false,
    }
}

struct TreeViewBinding;

impl WidgetCore for TreeViewBinding {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn tag() -> &'static str {
        ROOT_BTN_TAG
    }

    fn title() -> &'static str {
        "pinion hello-tree-view (R674 §5.16)"
    }

    fn create_external() -> Box<dyn pinion_core::external::External> {
        Box::new(ButtonExternal::new())
    }

    /// R675 §5.45 — register the substrate
    /// [`TreeRowClickExternal`] sibling that routes composite-tag
    /// clicks on `{TREE_TAG}#{id}` rows into the §5.20 intent
    /// channel. The substrate composes the state scene as
    /// `Scene::Container([primary, tree_router])` so the input
    /// router's depth-first walk reaches both nodes without further
    /// changes. (R674 introduced this wire as binding-level
    /// `FileTreeRowExternal`; R675 lifted it into
    /// [`pinion_widget_paint::tree_view::TreeRowClickExternal`] when
    /// the hello-multi-window inspector became the 2nd consumer,
    /// firing the [[abstraction-needs-second-consumer]] gate.)
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![ExtraExternal::new(
            TREE_TAG,
            Box::new(TreeRowClickExternal::new()),
        )]
    }

    fn read_state(scene: &Scene) -> Self::State {
        // R55.D.5 — once `create_extra_externals` is non-empty the
        // state scene root is a `Scene::Container`, so locate the
        // primary `Scene::External` by tag rather than pattern
        // matching the root directly.
        if let Some(node) = scene.find_external_with_tag(ROOT_BTN_TAG)
            && let Some(intro) = node.handle.introspect()
            && let Some(IntrospectValue::Text(name)) = intro.query("state")
        {
            return <Self::State as pinion_core::WidgetStateName>::from_name_or_default(
                &name,
            );
        }
        ButtonState::Idle
    }

    fn event_name(event: Self::Event) -> &'static str {
        <Self::Event as pinion_core::WidgetEventName>::as_name(&event)
    }

    fn view(state: Self::State, frame: &Frame) -> Scene {
        let _ = frame;
        view(state)
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let _ = (scene, focused);
        apply_key_impl(key)
    }

    /// R674 §5.23 R27 — bridge [`FileTreeRowExternal`]'s `click`
    /// intent into the shared [`toggle_expanded_in_signal`] sink.
    /// Side-effect-only ([[scxml-as-model-update-transient]]) — empty
    /// `Vec<Command>` return; the `Signal::set` write inside
    /// `toggle_expanded_in_signal` is the mutation.
    ///
    /// Reducer arms compare against the dotted wire form
    /// [`FILE_TREE_CLICK_INTENT_TAG`] per
    /// [[intent-tag-dotted-wire-form]] — bare-event-name matching is
    /// always silent.
    fn update(
        _state: Self::State,
        intent: &Intent,
    ) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == FILE_TREE_CLICK_INTENT_TAG
            && let IntrospectValue::Text(id) = &intent.payload
        {
            let tree_state = use_tree_state();
            toggle_expanded_in_signal(&tree_state.nodes, id);
        }
        Vec::new()
    }
}

/// R674 §5.40 — composite row tag the AT-side
/// [`TreeViewBinding::access_node`] walker emits per visible row.
/// Same format the paint substrate stamps so the AT NodeId hashes
/// through the same key as the hit-test target — AT actions on a
/// row land on the same tag the click router consumes.
fn tree_row_access_tag(node_id: &str) -> String {
    format!("{TREE_TAG}#{node_id}")
}

/// R674 §5.40 — depth-first walk of `nodes` emitting one [`AccessNode`]
/// per visible row in the same order the paint substrate paints it.
/// Each row carries:
///
/// * `role: AriaRole::TreeItem` — AT announces "tree item …".
/// * `name: label` — accessible name (announced first by the AT).
/// * `level: depth + 1` — WAI-ARIA 1.2 §6.6.8 one-based depth (root
///   children → 1, grandchildren → 2, …).
/// * `position_in_set: sibling_idx + 1` / `size_of_set: siblings.len()` —
///   WAI-ARIA 1.2 §6.6.9 / §6.6.10 sibling addressing.
///
/// Collapsed branches contribute the branch row itself but skip
/// their child rows (matching the visible row sequence the paint
/// substrate produces — the AT announces the same set of items the
/// user sees).
fn walk_access_rows(
    nodes: &[FileNode],
    depth: u32,
    out: &mut Vec<AccessNode>,
) {
    let setsize = u32::try_from(nodes.len()).unwrap_or(u32::MAX);
    for (idx, node) in nodes.iter().enumerate() {
        let position = u32::try_from(idx + 1).unwrap_or(u32::MAX);
        out.push(
            AccessNode::new(tree_row_access_tag(&node.id), AriaRole::TreeItem)
                .with_name(node.label.clone())
                .with_level(depth + 1)
                .with_position_in_set(position)
                .with_size_of_set(setsize),
        );
        if node.expanded && !node.children.is_empty() {
            walk_access_rows(&node.children, depth + 1, out);
        }
    }
}

impl WidgetA11y for TreeViewBinding {
    fn access_node(
        _state: &<Self as WidgetCore>::State,
        _focused: Option<&str>,
    ) -> Vec<AccessNode> {
        // R674 §5.40 — root advertises Tree role + lists every
        // visible row as a child so the AT-side topology mirrors the
        // paint topology. Each child row carries hierarchical axes
        // (level / posinset / setsize) per
        // WAI-ARIA 1.2 §6.6.8 / §6.6.9 / §6.6.10 — required for
        // custom-widget roles
        // because AT does NOT infer hierarchy from DOM nesting on
        // `role="treeitem"`.
        let tree_state = use_tree_state();
        let nodes = tree_state.nodes.get();

        let mut out = Vec::new();
        let mut walked = Vec::new();
        walk_access_rows(&nodes, 0, &mut walked);
        let children: Vec<String> = walked.iter().map(|n| n.tag.clone()).collect();

        let mut root = AccessNode::new(ROOT_BTN_TAG, AriaRole::Tree);
        for child in children {
            root = root.with_child(child);
        }
        out.push(root);
        out.extend(walked);
        out
    }
}

impl WidgetView for TreeViewBinding {
    type Renderer = HelloTreeViewRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TreeViewBinding>();
}

#[cfg(test)]
mod r675_substrate_dotted_wire_test {
    //! R675 §5.16 §5.20 — keep one regression pin proving the
    //! binding's dotted intent tag still matches the substrate's
    //! bare event name. The complete substrate state-machine /
    //! payload coverage moved with the External lift to
    //! `pinion_widget_paint::tree_view::r675_tree_row_click_external_tests`;
    //! this binding-side anchor catches a substrate
    //! `TREE_ROW_CLICK_EVENT` rename that would silently break
    //! `WidgetView::update` reducer matching here.

    use super::{FILE_TREE_CLICK_INTENT_TAG, TREE_TAG};
    use pinion_widget_paint::tree_view::TREE_ROW_CLICK_EVENT;

    #[test]
    fn r675_binding_dotted_form_lockstep_with_substrate_event_name() {
        assert_eq!(
            FILE_TREE_CLICK_INTENT_TAG,
            format!("{TREE_TAG}.{TREE_ROW_CLICK_EVENT}"),
        );
    }
}

#[cfg(test)]
mod r674_per_row_access_node_tests {
    //! R674 §5.40 — per-row [`AriaRole::TreeItem`] [`AccessNode`]
    //! emission contract. Substrate-level tests against the
    //! [`walk_access_rows`] helper directly so the WAI-ARIA 1.2
    //! hierarchical axes (level / posinset / setsize) are verified
    //! without going through the framework Owner cache.

    use super::{tree_row_access_tag, walk_access_rows, FileNode, TREE_TAG};
    use pinion_a11y::AriaRole;

    fn sample_tree() -> Vec<FileNode> {
        vec![
            FileNode::branch(
                "src",
                "src",
                true,
                vec![
                    FileNode::leaf("src/main.rs", "main.rs"),
                    FileNode::branch(
                        "src/widgets",
                        "widgets",
                        true,
                        vec![FileNode::leaf("src/widgets/mod.rs", "mod.rs")],
                    ),
                ],
            ),
            FileNode::branch(
                "docs",
                "docs",
                false,
                vec![FileNode::leaf("docs/README.md", "README.md")],
            ),
        ]
    }

    #[test]
    fn r674_walk_emits_one_node_per_visible_row_in_paint_order() {
        // src (expanded) → src/main.rs, src/widgets (expanded) →
        // src/widgets/mod.rs ; docs (collapsed). 5 visible rows.
        let nodes = sample_tree();
        let mut out = Vec::new();
        walk_access_rows(&nodes, 0, &mut out);
        let tags: Vec<&str> = out.iter().map(|n| n.tag.as_str()).collect();
        assert_eq!(
            tags,
            vec![
                "file_tree#src",
                "file_tree#src/main.rs",
                "file_tree#src/widgets",
                "file_tree#src/widgets/mod.rs",
                "file_tree#docs",
            ],
            "paint order = depth-first preorder over visible rows",
        );
    }

    #[test]
    fn r674_all_rows_carry_tree_item_role() {
        let nodes = sample_tree();
        let mut out = Vec::new();
        walk_access_rows(&nodes, 0, &mut out);
        for node in &out {
            assert_eq!(
                node.role,
                AriaRole::TreeItem,
                "per-row tag {} must report treeitem role",
                node.tag,
            );
        }
    }

    #[test]
    fn r674_level_is_depth_plus_one_one_based() {
        // WAI-ARIA 1.2 §6.6.8 — root children are at level 1, not 0.
        let nodes = sample_tree();
        let mut out = Vec::new();
        walk_access_rows(&nodes, 0, &mut out);
        // src, docs → level 1
        // src/main.rs, src/widgets → level 2
        // src/widgets/mod.rs → level 3
        let by_tag: std::collections::HashMap<&str, Option<u32>> = out
            .iter()
            .map(|n| (n.tag.as_str(), n.level))
            .collect();
        assert_eq!(by_tag["file_tree#src"], Some(1));
        assert_eq!(by_tag["file_tree#docs"], Some(1));
        assert_eq!(by_tag["file_tree#src/main.rs"], Some(2));
        assert_eq!(by_tag["file_tree#src/widgets"], Some(2));
        assert_eq!(by_tag["file_tree#src/widgets/mod.rs"], Some(3));
    }

    #[test]
    fn r674_position_in_set_is_sibling_index_plus_one() {
        // WAI-ARIA 1.2 §6.6.9 — one-based; "item N of M" sentence
        // has N = position_in_set.
        let nodes = sample_tree();
        let mut out = Vec::new();
        walk_access_rows(&nodes, 0, &mut out);
        let by_tag: std::collections::HashMap<&str, Option<u32>> = out
            .iter()
            .map(|n| (n.tag.as_str(), n.position_in_set))
            .collect();
        // Root: src is 1st of 2, docs is 2nd of 2.
        assert_eq!(by_tag["file_tree#src"], Some(1));
        assert_eq!(by_tag["file_tree#docs"], Some(2));
        // Under src: main.rs is 1st of 2, widgets is 2nd of 2.
        assert_eq!(by_tag["file_tree#src/main.rs"], Some(1));
        assert_eq!(by_tag["file_tree#src/widgets"], Some(2));
        // Under src/widgets: mod.rs is 1st of 1.
        assert_eq!(by_tag["file_tree#src/widgets/mod.rs"], Some(1));
    }

    #[test]
    fn r674_size_of_set_is_sibling_count() {
        // WAI-ARIA 1.2 §6.6.10 — total siblings in the parent's
        // visible set. Collapsed branches contribute to the parent
        // count (the branch itself is visible).
        let nodes = sample_tree();
        let mut out = Vec::new();
        walk_access_rows(&nodes, 0, &mut out);
        let by_tag: std::collections::HashMap<&str, Option<u32>> = out
            .iter()
            .map(|n| (n.tag.as_str(), n.size_of_set))
            .collect();
        // Root has 2 siblings (src, docs).
        assert_eq!(by_tag["file_tree#src"], Some(2));
        assert_eq!(by_tag["file_tree#docs"], Some(2));
        // src has 2 children siblings (main.rs, widgets).
        assert_eq!(by_tag["file_tree#src/main.rs"], Some(2));
        assert_eq!(by_tag["file_tree#src/widgets"], Some(2));
        // src/widgets has 1 child sibling (mod.rs).
        assert_eq!(by_tag["file_tree#src/widgets/mod.rs"], Some(1));
    }

    #[test]
    fn r674_name_mirrors_label() {
        // AT-side announcement = the visible label, by convention
        // — keeps the AT user and the sighted user reading the same
        // identifier.
        let nodes = sample_tree();
        let mut out = Vec::new();
        walk_access_rows(&nodes, 0, &mut out);
        let by_tag: std::collections::HashMap<&str, Option<&str>> = out
            .iter()
            .map(|n| (n.tag.as_str(), n.name.as_deref()))
            .collect();
        assert_eq!(by_tag["file_tree#src"], Some("src"));
        assert_eq!(by_tag["file_tree#src/main.rs"], Some("main.rs"));
        assert_eq!(by_tag["file_tree#src/widgets/mod.rs"], Some("mod.rs"));
        assert_eq!(by_tag["file_tree#docs"], Some("docs"));
    }

    #[test]
    fn r674_collapsed_branch_hides_children_from_at_tree() {
        // docs is collapsed; docs/README.md must NOT appear in the
        // visible row sequence (AT announces what the user sees).
        let nodes = sample_tree();
        let mut out = Vec::new();
        walk_access_rows(&nodes, 0, &mut out);
        let tags: Vec<&str> = out.iter().map(|n| n.tag.as_str()).collect();
        assert!(
            !tags.contains(&"file_tree#docs/README.md"),
            "collapsed branch must hide descendants from the visible row set",
        );
        // But docs itself stays visible.
        assert!(tags.contains(&"file_tree#docs"));
    }

    #[test]
    fn r674_empty_tree_emits_no_rows() {
        let nodes: Vec<FileNode> = Vec::new();
        let mut out = Vec::new();
        walk_access_rows(&nodes, 0, &mut out);
        assert!(out.is_empty(), "empty tree contributes zero TreeItem rows");
    }

    #[test]
    fn r674_tree_row_access_tag_uses_composite_format() {
        // Lockstep with `composite_row_tag` in the paint substrate —
        // AT NodeId hashes through the same key the hit-test walker
        // uses, so AT-side `Click` actions resolve to the same row
        // the click router consumes.
        assert_eq!(
            tree_row_access_tag("src/lib.rs"),
            format!("{TREE_TAG}#src/lib.rs"),
        );
    }
}
