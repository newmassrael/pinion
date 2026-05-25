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
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{reactive, Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::{vello_renderer_impl, SizeStrategy, WidgetView};
use pinion_widget_paint::tree_view::{view_tree_focused, TreeItem, TreeViewFocus, TreeViewStyle};

include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloTreeViewRenderer, HelloTreeViewRendererError);

const TREE_TAG: &str = "file_tree";
const ROOT_BTN_TAG: &str = "tree_root";
const THEME_TAG: &str = "app";

const WIN_W: u32 = 480;
const WIN_H: u32 = 400;

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
            // Toggle expand on the focused branch (no-op on leaves).
            let Some(focused_id) = current else {
                return false;
            };
            if !has_children(&nodes_clone, &focused_id) {
                return false;
            }
            reactive::batch(|| {
                let mut nodes = tree_state.nodes.get();
                if let Some(node) = find_node_mut(&mut nodes, &focused_id) {
                    node.expanded = !node.expanded;
                    tree_state.nodes.set(nodes);
                }
            });
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
        "pinion hello-tree-view (R673 §5.16)"
    }

    fn create_external() -> Box<dyn pinion_core::external::External> {
        Box::new(ButtonExternal::new())
    }

    fn read_state(scene: &Scene) -> Self::State {
        if let Scene::External(node) = scene
            && let Some(intro) = node.handle.introspect()
            && let Some(pinion_core::external::IntrospectValue::Text(name)) =
                intro.query("state")
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
}

impl WidgetA11y for TreeViewBinding {
    fn access_node(
        _state: &<Self as WidgetCore>::State,
        _focused: Option<&str>,
    ) -> Vec<AccessNode> {
        // R673 §5.40 — root advertises Tree role so AT clients
        // recognise the binding's primary content as a hierarchical
        // collection. Per-row TreeItem nodes would land here in a
        // future round once row-level AT events surface (file-tree
        // editor 2nd consumer). The minimal Tree role at root unblocks
        // AccessKit Tree announcement on screen readers.
        vec![AccessNode::new(ROOT_BTN_TAG, AriaRole::Tree)]
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
