//! R51.68 §5.40 — end-to-end conformance for the pinion-a11y
//! substrate.
//!
//! These tests assemble realistic [`AccessNode`] lists representing
//! every widget in the standard catalogue (Button, Switch, `CheckBox`,
//! `RadioButton`, Slider, `RadioGroup` composite) and verify three
//! integration properties the per-module unit tests cannot:
//!
//! 1. **Tree topology.** A mixed scene (atomic widget + composite)
//!    lowers into one synthetic root + N atomic children + composite
//!    parent (+ composite's children listed under the composite, not
//!    under the root). The node-count math must be exactly
//!    `1 + atomic_count + composite_count + sum(composite_children)`.
//!
//! 2. **Focus resolution.** `TreeUpdate::focus` resolves to the
//!    target widget's `NodeId` when present, falls back to the
//!    synthetic root when the focused tag does not match any node
//!    (stale tree / cross-frame race), and survives the composite
//!    focus-redirect substrate (sub-tag delivered by
//!    `WidgetView::access_focus_target` resolves cleanly).
//!
//! 3. **`ActionRequest` round-trip.** A `NodeId` minted by
//!    `tag_to_node_id` is recovered to the original widget tag via
//!    the `AccessTreeBuilder::tag_map` reverse map, and lifted into
//!    the pinion-native [`PinionAccessAction`] by
//!    [`translate_action`]. Root-targeted requests return `None`
//!    (sentinel for "no widget destination") so the dispatch layer
//!    can drop them without crossing into widget logic.

use std::collections::HashMap;

use accesskit::{Action, ActionRequest, NodeId, TreeId};
use pinion_a11y::{
    AccessAction, AccessNode, AccessState, AccessTreeBuilder, AccessValue, AriaRole, ROOT_NODE_ID,
    tag_to_node_id, translate_action,
};
use pinion_core::scene::Rect;

/// Compose a realistic mixed scene: one Button, one Switch, one
/// Slider, and a `RadioGroup` with three `RadioButton` children. Mirrors
/// the most complex pinion app the standard widget catalogue can
/// build today, so the resulting tree shape doubles as a regression
/// guard against any future substrate refactor.
fn mixed_scene() -> Vec<AccessNode> {
    vec![
        AccessNode::new("main_btn", AriaRole::Button)
            .with_name("Save")
            .with_state(AccessState {
                focused: true,
                ..AccessState::default()
            })
            .with_bounds(Rect::new(10, 10, 100, 30)),
        AccessNode::new("dark_toggle", AriaRole::Switch)
            .with_name("Dark mode")
            .with_value(AccessValue::Bool(true))
            .with_state(AccessState {
                checked: Some(true),
                ..AccessState::default()
            })
            .with_bounds(Rect::new(10, 50, 100, 30)),
        AccessNode::new("volume", AriaRole::Slider)
            .with_name("Volume")
            .with_value(AccessValue::Float {
                value: 0.5,
                min: 0.0,
                max: 1.0,
            })
            .with_bounds(Rect::new(10, 90, 200, 30)),
        AccessNode::new("tier_group", AriaRole::RadioGroup)
            .with_name("Subscription tier")
            .with_child("tier_group#0")
            .with_child("tier_group#1")
            .with_child("tier_group#2")
            .with_bounds(Rect::new(10, 130, 200, 90)),
        AccessNode::new("tier_group#0", AriaRole::RadioButton)
            .with_name("Tier 0")
            .with_value(AccessValue::Bool(false))
            .with_state(AccessState {
                checked: Some(false),
                ..AccessState::default()
            })
            .with_bounds(Rect::new(20, 140, 180, 24)),
        AccessNode::new("tier_group#1", AriaRole::RadioButton)
            .with_name("Tier 1")
            .with_value(AccessValue::Bool(true))
            .with_state(AccessState {
                checked: Some(true),
                ..AccessState::default()
            })
            .with_bounds(Rect::new(20, 170, 180, 24)),
        AccessNode::new("tier_group#2", AriaRole::RadioButton)
            .with_name("Tier 2")
            .with_value(AccessValue::Bool(false))
            .with_state(AccessState {
                checked: Some(false),
                ..AccessState::default()
            })
            .with_bounds(Rect::new(20, 200, 180, 24)),
    ]
}

#[test]
fn mixed_scene_emits_root_plus_seven_widget_nodes() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    let update = builder.build(Some(Rect::new(0, 0, 480, 320)));
    // 1 synthetic root + 3 atomic widgets + 1 composite group + 3
    // composite children = 8 nodes total.
    assert_eq!(update.nodes.len(), 8);
    assert_eq!(update.nodes[0].0, ROOT_NODE_ID);
}

#[test]
fn focus_resolves_to_atomic_widget() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    builder.focused(Some("main_btn"));
    let update = builder.build(None);
    assert_eq!(update.focus, tag_to_node_id("main_btn"));
}

#[test]
fn focus_resolves_to_arbitrary_present_tag() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    // The builder accepts whatever tag the shell passes and resolves
    // to the matching `NodeId`. Composite focus now travels via
    // `AccessFocus::composite` + `active_descendant`, but the
    // builder-level `focused()` contract remains: any present tag
    // becomes `TreeUpdate::focus`.
    builder.focused(Some("tier_group#1"));
    let update = builder.build(None);
    assert_eq!(update.focus, tag_to_node_id("tier_group#1"));
}

#[test]
fn composite_focus_with_active_descendant_lands_on_parent() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    // R51.71 §5.40 — ARIA Authoring Practices roving-tabindex:
    // parent owns the tab stop, child is the active descendant.
    builder.focused(Some("tier_group"));
    builder.active_descendant("tier_group", "tier_group#1");
    let update = builder.build(None);
    // `TreeUpdate::focus` lands on the parent group.
    assert_eq!(update.focus, tag_to_node_id("tier_group"));
    // Node count unchanged — active_descendant is a node attribute,
    // not a separate tree node (1 root + 3 atomic + 1 group + 3
    // composite children = 8).
    assert_eq!(update.nodes.len(), 8);
}

#[test]
fn focus_falls_back_to_root_when_tag_absent() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    builder.focused(Some("stale_widget_from_previous_frame"));
    let update = builder.build(None);
    assert_eq!(update.focus, ROOT_NODE_ID);
}

#[test]
fn tag_map_resolves_every_widget_back_to_its_tag() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    let map = builder.tag_map();
    assert_eq!(map.get(&ROOT_NODE_ID).map(String::as_str), Some(""));
    for tag in [
        "main_btn",
        "dark_toggle",
        "volume",
        "tier_group",
        "tier_group#0",
        "tier_group#1",
        "tier_group#2",
    ] {
        let id = tag_to_node_id(tag);
        assert_eq!(map.get(&id).map(String::as_str), Some(tag), "tag {tag}");
    }
}

#[test]
fn action_request_round_trip_recovers_widget_tag() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    let map = builder.tag_map();

    let req = ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: tag_to_node_id("dark_toggle"),
        data: None,
    };
    let action = translate_action(&req, &map).expect("widget tag recovered");
    assert_eq!(action.tag, "dark_toggle");
    assert_eq!(action.kind, AccessAction::Click);
}

#[test]
fn action_request_increment_for_slider_round_trips() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    let map = builder.tag_map();
    let req = ActionRequest {
        action: Action::Increment,
        target_tree: TreeId::ROOT,
        target_node: tag_to_node_id("volume"),
        data: None,
    };
    let action = translate_action(&req, &map).expect("slider tag recovered");
    assert_eq!(action.tag, "volume");
    assert_eq!(action.kind, AccessAction::Increment);
}

#[test]
fn action_request_on_composite_child_recovers_sub_tag() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    let map = builder.tag_map();
    let req = ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: tag_to_node_id("tier_group#2"),
        data: None,
    };
    let action = translate_action(&req, &map).expect("sub-tag recovered");
    assert_eq!(action.tag, "tier_group#2");
}

#[test]
fn action_request_on_root_returns_none() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    let map = builder.tag_map();
    let req = ActionRequest {
        action: Action::Focus,
        target_tree: TreeId::ROOT,
        target_node: ROOT_NODE_ID,
        data: None,
    };
    assert!(translate_action(&req, &map).is_none());
}

#[test]
fn action_request_on_unknown_node_returns_none() {
    let mut builder = AccessTreeBuilder::new();
    for node in mixed_scene() {
        builder.add(&node);
    }
    let map = builder.tag_map();
    let req = ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: NodeId(42_424_242),
        data: None,
    };
    assert!(translate_action(&req, &map).is_none());
}

#[test]
fn unmapped_action_lifts_to_other_variant() {
    let map: HashMap<NodeId, String> = [(tag_to_node_id("main_btn"), "main_btn".to_owned())]
        .into_iter()
        .collect();
    let req = ActionRequest {
        action: Action::ScrollDown,
        target_tree: TreeId::ROOT,
        target_node: tag_to_node_id("main_btn"),
        data: None,
    };
    let action = translate_action(&req, &map).expect("widget tag recovered");
    assert_eq!(action.kind, AccessAction::Other);
}

#[test]
fn initial_emission_includes_tree_metadata() {
    let update = AccessTreeBuilder::new().build(None);
    assert!(
        update.tree.is_some(),
        "first emission must include Tree metadata (AccessKit invariant)",
    );
}

#[test]
fn subsequent_emission_omits_tree_metadata() {
    let mut b = AccessTreeBuilder::new();
    b.initial(false);
    let update = b.build(None);
    assert!(update.tree.is_none());
}

#[test]
fn empty_builder_emits_root_only() {
    let update = AccessTreeBuilder::new().build(None);
    assert_eq!(update.nodes.len(), 1);
    assert_eq!(update.nodes[0].0, ROOT_NODE_ID);
}

#[test]
fn incremental_emit_carries_only_dirty_widget() {
    // R51.72 §5.40 — simulate a "checkbox toggled" frame: the
    // initial frame emits the full mixed scene; the next frame
    // diffs and emits only the changed tag plus the synthetic root.
    let mut builder = AccessTreeBuilder::new();
    builder.initial(false);
    for node in mixed_scene() {
        builder.add(&node);
    }
    let dirty: std::collections::HashSet<String> = ["dark_toggle".to_owned()].into_iter().collect();
    builder.dirty_tags(dirty);
    let update = builder.build(Some(Rect::new(0, 0, 480, 320)));
    // 1 root + 1 dirty widget = 2 nodes. The other 6 (button +
    // slider + group + 3 radios) stay in the AT's cached state.
    assert_eq!(update.nodes.len(), 2);
    assert_eq!(update.nodes[0].0, ROOT_NODE_ID);
    assert_eq!(update.nodes[1].0, tag_to_node_id("dark_toggle"));
    // Tree metadata omitted on incremental emissions.
    assert!(update.tree.is_none());
}

#[test]
fn incremental_empty_dirty_still_carries_focus_and_root() {
    // R51.72 §5.40 — no node body changed, but focus moved. The
    // AT still receives the updated focus via `TreeUpdate::focus`;
    // node payload is just the root carrier.
    let mut builder = AccessTreeBuilder::new();
    builder.initial(false);
    for node in mixed_scene() {
        builder.add(&node);
    }
    builder.dirty_tags(std::collections::HashSet::new());
    builder.focused(Some("main_btn"));
    let update = builder.build(None);
    assert_eq!(update.nodes.len(), 1);
    assert_eq!(update.focus, tag_to_node_id("main_btn"));
}
