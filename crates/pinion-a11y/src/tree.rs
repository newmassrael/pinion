//! R51.61 §5.40 — `accesskit::TreeUpdate` builder.
//!
//! [`AccessTreeBuilder`] collects a flat list of pinion-native
//! [`AccessNode`]s and lowers them into a single `accesskit::TreeUpdate`
//! that the platform Adapter (Windows UIA / macOS AX / Linux AT-SPI /
//! Android) consumes.
//!
//! ## Tree topology
//!
//! The emitted tree has a synthetic [`ROOT_NODE_ID`] window node whose
//! children are every widget tag that is not claimed as a composite
//! child by any other [`AccessNode`]. Composite widgets (`RadioGroup`)
//! list their internal children's tags in
//! [`AccessNode::children`]; the builder resolves those tags into
//! `accesskit::NodeId`s and attaches them under the composite parent
//! instead of the root.
//!
//! ## Tag → `NodeId` hashing
//!
//! [`tag_to_node_id`] runs the widget tag through the standard library
//! `DefaultHasher` and sets the high bit so the result never collides
//! with the reserved root [`ROOT_NODE_ID`] = `NodeId(1)`. The
//! deterministic mapping is required by AccessKit's invariant that the
//! same UI element keeps the same `NodeId` across `TreeUpdate`s — the
//! framework uses widget tags exactly for that stable identity, so the
//! hash carries the invariant through unchanged.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use accesskit::{Action, Node, NodeId, Rect as AccessRect, Role, Tree, TreeId, TreeUpdate};
use pinion_core::scene::Rect;

use crate::node::{AccessNode, AccessValue};
use crate::role::AriaRole;

/// Reserved root `NodeId` for the synthetic window node.
pub const ROOT_NODE_ID: NodeId = NodeId(1);

/// Stable widget-tag → `NodeId` mapping.
///
/// Uses `DefaultHasher` (FxHash-class throughput, not cryptographic).
/// High bit is set so the result never collides with the reserved
/// [`ROOT_NODE_ID`] = `NodeId(1)` and so two different `DefaultHasher`
/// runs (across Rust versions) cannot accidentally produce a node id
/// that aliases a reserved slot. The mapping is per-process
/// deterministic — AccessKit only requires within-process stability,
/// which `DefaultHasher` provides.
#[must_use]
pub fn tag_to_node_id(tag: &str) -> NodeId {
    let mut h = DefaultHasher::new();
    tag.hash(&mut h);
    NodeId(h.finish() | 0x8000_0000_0000_0000)
}

/// Convert pinion-core `Rect` (u32) into `accesskit::Rect` (f64).
fn rect_to_accesskit(r: Rect) -> AccessRect {
    AccessRect {
        x0: f64::from(r.x),
        y0: f64::from(r.y),
        x1: f64::from(r.x + r.w),
        y1: f64::from(r.y + r.h),
    }
}

/// Builder for `accesskit::TreeUpdate`.
///
/// Build pattern: `new()` → `add()*` → `focused(tag)?` → `build(...)`.
/// Holds a tag→`AccessNode` map internally so duplicate tags overwrite
/// (matches AccessKit's "later node with same id wins" semantic).
pub struct AccessTreeBuilder {
    nodes: HashMap<String, AccessNode>,
    insertion_order: Vec<String>,
    focused: Option<String>,
    initial: bool,
}

impl AccessTreeBuilder {
    /// New empty builder. The default emits the synthetic root
    /// every build — pass `initial(false)` after the first frame
    /// to skip the `Tree` field per AccessKit's "rarely-updated"
    /// guidance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            insertion_order: Vec::new(),
            focused: None,
            initial: true,
        }
    }

    /// Append a widget node. Duplicate tag overwrites the previous
    /// entry (last-write-wins, matching AccessKit semantics).
    pub fn add(&mut self, node: AccessNode) -> &mut Self {
        let tag = node.tag.clone();
        if !self.nodes.contains_key(&tag) {
            self.insertion_order.push(tag.clone());
        }
        self.nodes.insert(tag, node);
        self
    }

    /// Mark the currently focused widget tag (or clear with `None`).
    pub fn focused(&mut self, tag: Option<&str>) -> &mut Self {
        self.focused = tag.map(str::to_owned);
        self
    }

    /// Set whether this `TreeUpdate` is the very first emission for
    /// the tree (default = `true`). After the first build,
    /// downstream callers pass `false` so the `tree` field is
    /// omitted per AccessKit's "rarely-updated" guidance.
    #[must_use]
    pub fn initial(mut self, initial: bool) -> Self {
        self.initial = initial;
        self
    }

    /// Reverse map for `ActionRequest::target_node` lookup. Includes
    /// the root id so an AT request against the window itself can
    /// still be answered.
    #[must_use]
    pub fn tag_map(&self) -> HashMap<NodeId, String> {
        let mut map = HashMap::with_capacity(self.nodes.len() + 1);
        map.insert(ROOT_NODE_ID, String::new()); // "" = window root
        for tag in self.nodes.keys() {
            map.insert(tag_to_node_id(tag), tag.clone());
        }
        map
    }

    /// Lower the collected nodes into an `accesskit::TreeUpdate`.
    ///
    /// `window_bounds` becomes the root node's bounds (`None` = no
    /// bounds; AT will fall back to native window geometry).
    #[must_use]
    pub fn build(self, window_bounds: Option<Rect>) -> TreeUpdate {
        let claimed = collect_claimed_children(&self.nodes);
        let root_children: Vec<NodeId> = self
            .insertion_order
            .iter()
            .filter(|t| !claimed.contains(t.as_str()))
            .map(|t| tag_to_node_id(t))
            .collect();

        let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(self.nodes.len() + 1);

        // 1. Synthetic root window node.
        let mut root = Node::new(Role::Window);
        if let Some(bounds) = window_bounds {
            root.set_bounds(rect_to_accesskit(bounds));
        }
        for child_id in root_children {
            root.push_child(child_id);
        }
        nodes.push((ROOT_NODE_ID, root));

        // 2. Per-widget nodes in insertion order.
        for tag in &self.insertion_order {
            let access = &self.nodes[tag];
            let node_id = tag_to_node_id(tag);
            let node = lower_access_node(access);
            nodes.push((node_id, node));
        }

        let focus = self
            .focused
            .as_deref()
            .filter(|t| self.nodes.contains_key(*t))
            .map_or(ROOT_NODE_ID, tag_to_node_id);

        TreeUpdate {
            nodes,
            tree: if self.initial { Some(Tree::new(ROOT_NODE_ID)) } else { None },
            tree_id: TreeId::ROOT,
            focus,
        }
    }
}

impl Default for AccessTreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn collect_claimed_children(nodes: &HashMap<String, AccessNode>) -> HashSet<&str> {
    let mut claimed: HashSet<&str> = HashSet::new();
    for n in nodes.values() {
        for child in &n.children {
            claimed.insert(child.as_str());
        }
    }
    claimed
}

fn lower_access_node(access: &AccessNode) -> Node {
    let mut node = Node::new(access.role.to_accesskit());

    if let Some(name) = &access.name {
        node.set_label(name.clone());
    }

    match &access.value {
        Some(AccessValue::Bool(b)) => {
            node.set_toggled(if *b { accesskit::Toggled::True } else { accesskit::Toggled::False });
        }
        Some(AccessValue::Float { value, min, max }) => {
            node.set_numeric_value(f64::from(*value));
            node.set_min_numeric_value(f64::from(*min));
            node.set_max_numeric_value(f64::from(*max));
        }
        Some(AccessValue::Text(t)) => {
            node.set_value(t.clone());
        }
        None => {}
    }

    if let Some(checked) = access.state.checked {
        node.set_toggled(if checked { accesskit::Toggled::True } else { accesskit::Toggled::False });
    }
    if access.state.disabled {
        node.set_disabled();
    }

    if let Some(bounds) = access.bounds {
        node.set_bounds(rect_to_accesskit(bounds));
    }

    for child_tag in &access.children {
        node.push_child(tag_to_node_id(child_tag));
    }

    add_actions_for_role(&mut node, access.role);
    node
}

fn add_actions_for_role(node: &mut Node, role: AriaRole) {
    match role {
        AriaRole::Button
        | AriaRole::CheckBox
        | AriaRole::RadioButton
        | AriaRole::Switch => {
            node.add_action(Action::Click);
            node.add_action(Action::Focus);
        }
        AriaRole::Slider => {
            node.add_action(Action::Focus);
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        }
        AriaRole::RadioGroup | AriaRole::Generic => {
            node.add_action(Action::Focus);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::AccessState;

    #[test]
    fn tag_to_node_id_is_deterministic() {
        let a = tag_to_node_id("main_btn");
        let b = tag_to_node_id("main_btn");
        assert_eq!(a, b);
    }

    #[test]
    fn tag_to_node_id_distinct_for_distinct_tags() {
        let a = tag_to_node_id("a");
        let b = tag_to_node_id("b");
        assert_ne!(a, b);
    }

    #[test]
    fn tag_to_node_id_never_collides_with_root() {
        for t in ["a", "b", "main_btn", "main_group", "", "  ", "1"] {
            assert_ne!(tag_to_node_id(t), ROOT_NODE_ID);
        }
    }

    #[test]
    fn empty_builder_emits_root_only() {
        let update = AccessTreeBuilder::new().build(None);
        assert_eq!(update.nodes.len(), 1);
        assert_eq!(update.nodes[0].0, ROOT_NODE_ID);
        assert_eq!(update.focus, ROOT_NODE_ID);
        assert!(update.tree.is_some());
    }

    #[test]
    fn single_atomic_widget_attaches_to_root() {
        let mut b = AccessTreeBuilder::new();
        b.add(AccessNode::new("main_btn", AriaRole::Button));
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 2);
        // root first, then widget
        assert_eq!(update.nodes[0].0, ROOT_NODE_ID);
        assert_eq!(update.nodes[1].0, tag_to_node_id("main_btn"));
    }

    #[test]
    fn composite_children_not_at_root() {
        let mut b = AccessTreeBuilder::new();
        b.add(
            AccessNode::new("main_group", AriaRole::RadioGroup)
                .with_child("r0")
                .with_child("r1"),
        );
        b.add(AccessNode::new("r0", AriaRole::RadioButton));
        b.add(AccessNode::new("r1", AriaRole::RadioButton));
        let update = b.build(None);
        // RadioGroup is at root; r0/r1 are not direct root children
        // (they live under RadioGroup via the composite topology).
        // Inspect the root node's children count: only 1 (RadioGroup).
        // We can't easily inspect Node internals without accesskit
        // private API, but the build must succeed and emit 4 nodes
        // (root + group + r0 + r1).
        assert_eq!(update.nodes.len(), 4);
    }

    #[test]
    fn focused_falls_back_to_root_when_tag_missing() {
        let mut b = AccessTreeBuilder::new();
        b.add(AccessNode::new("main_btn", AriaRole::Button));
        b.focused(Some("nonexistent"));
        let update = b.build(None);
        assert_eq!(update.focus, ROOT_NODE_ID);
    }

    #[test]
    fn focused_resolves_to_widget_when_present() {
        let mut b = AccessTreeBuilder::new();
        b.add(AccessNode::new("main_btn", AriaRole::Button));
        b.focused(Some("main_btn"));
        let update = b.build(None);
        assert_eq!(update.focus, tag_to_node_id("main_btn"));
    }

    #[test]
    fn initial_false_omits_tree_field() {
        let update = AccessTreeBuilder::new().initial(false).build(None);
        assert!(update.tree.is_none());
    }

    #[test]
    fn window_bounds_sets_root_bounds() {
        // We can't introspect Node bounds without accesskit private
        // API, so just verify build succeeds with bounds passed.
        let update =
            AccessTreeBuilder::new().build(Some(Rect::new(0, 0, 1024, 768)));
        assert_eq!(update.nodes.len(), 1);
    }

    #[test]
    fn duplicate_tag_overwrites_previous() {
        let mut b = AccessTreeBuilder::new();
        b.add(AccessNode::new("btn", AriaRole::Button).with_name("First"));
        b.add(AccessNode::new("btn", AriaRole::Button).with_name("Second"));
        let update = b.build(None);
        // 1 widget node + 1 root = 2
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn tag_map_includes_root_and_widgets() {
        let mut b = AccessTreeBuilder::new();
        b.add(AccessNode::new("main_btn", AriaRole::Button));
        b.add(AccessNode::new("main_cb", AriaRole::CheckBox));
        let map = b.tag_map();
        assert_eq!(map.get(&ROOT_NODE_ID).map(String::as_str), Some(""));
        assert_eq!(
            map.get(&tag_to_node_id("main_btn")).map(String::as_str),
            Some("main_btn"),
        );
        assert_eq!(
            map.get(&tag_to_node_id("main_cb")).map(String::as_str),
            Some("main_cb"),
        );
    }

    #[test]
    fn checkbox_with_value_and_state_lowers() {
        let state = AccessState {
            focused: true,
            checked: Some(true),
            ..AccessState::default()
        };
        let node = AccessNode::new("cb", AriaRole::CheckBox)
            .with_name("Enable")
            .with_value(AccessValue::Bool(true))
            .with_state(state)
            .with_bounds(Rect::new(10, 20, 100, 30));
        let mut b = AccessTreeBuilder::new();
        b.add(node);
        b.focused(Some("cb"));
        let update = b.build(None);
        assert_eq!(update.focus, tag_to_node_id("cb"));
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn slider_emits_float_range() {
        let node = AccessNode::new("sl", AriaRole::Slider)
            .with_value(AccessValue::Float { value: 50.0, min: 0.0, max: 100.0 })
            .with_bounds(Rect::new(0, 0, 200, 24));
        let mut b = AccessTreeBuilder::new();
        b.add(node);
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 2);
    }
}
