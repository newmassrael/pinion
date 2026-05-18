//! R51.61 §5.40 — Pinion-native AccessKit action mapping.
//!
//! [`AccessAction`] is a pinion-native enum that lifts the subset of
//! `accesskit::Action` that the standard widget catalogue supports.
//! AccessKit ships 22 actions; pinion currently maps only the five
//! relevant to its widgets (Click / Focus / Increment / Decrement /
//! Default) — additional actions land additively per future axes.
//!
//! [`translate_action`] converts a raw `accesskit::ActionRequest` (as
//! delivered by `accesskit_winit` on `WindowEvent::AccessibilityAction`,
//! lands R51.62 wiring) into a pinion-native widget tag + action kind
//! pair so the dispatch layer (R51.67) can route it through the
//! existing `InputRouter` / `FocusManager` / `WidgetView::apply_key`
//! surface without touching `accesskit` types.

use std::collections::HashMap;
use std::hash::BuildHasher;

use accesskit::{Action, ActionRequest, NodeId};

/// Subset of `accesskit::Action` relevant to pinion's standard
/// widget catalogue.
///
/// `Other` is the silent-drop case for the 17 AccessKit actions that
/// have no pinion mapping yet (`ScrollDown`, `Expand`, `SetTextSelection`,
/// etc.) — the dispatch layer ignores `Other` rather than refusing,
/// so AT clients can over-request without breaking the widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccessAction {
    /// Invoke the widget's primary action (Button click, Checkbox
    /// toggle, Radio select, Switch flip).
    Click,
    /// Move keyboard focus to this widget.
    Focus,
    /// Slider value step up (or analogous range bump).
    Increment,
    /// Slider value step down.
    Decrement,
    /// Activate the default-on-Enter action (mirrors `Action::Click`
    /// for now; future widget kinds may diverge — see WAI-ARIA
    /// "default action" semantics).
    Default,
    /// Unrecognised / unmapped AT request. Dispatch layer drops
    /// silently.
    Other,
}

impl AccessAction {
    /// Lift an `accesskit::Action` into the pinion enum.
    ///
    /// Maps the five pinion-mapped actions; everything else becomes
    /// [`AccessAction::Other`].
    #[must_use]
    pub const fn from_accesskit(a: Action) -> Self {
        match a {
            Action::Click => Self::Click,
            Action::Focus => Self::Focus,
            Action::Increment => Self::Increment,
            Action::Decrement => Self::Decrement,
            _ => Self::Other,
        }
    }
}

/// Pinion-native action delivered to the dispatch layer.
///
/// Pairs the widget tag (resolved from the AccessKit `NodeId` via the
/// tree builder's tag map) with the lifted action kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinionAccessAction {
    pub tag: String,
    pub kind: AccessAction,
}

/// Translate an `accesskit::ActionRequest` into a pinion-native
/// action.
///
/// Returns `None` when:
///   * `req.target_node` does not appear in `tag_map` (stale tree
///     or AT race with a re-emitted update);
///   * `req.target_node` resolves to the synthetic root window
///     (no widget tag); pinion does not route window-level actions
///     to widgets, so callers should ignore the `None` result.
#[must_use]
pub fn translate_action<S: BuildHasher>(
    req: &ActionRequest,
    tag_map: &HashMap<NodeId, String, S>,
) -> Option<PinionAccessAction> {
    let tag = tag_map.get(&req.target_node)?.clone();
    if tag.is_empty() {
        return None; // root window — no widget destination
    }
    Some(PinionAccessAction {
        tag,
        kind: AccessAction::from_accesskit(req.action),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{tag_to_node_id, ROOT_NODE_ID};

    fn map_with(tag: &str) -> HashMap<NodeId, String> {
        let mut m = HashMap::new();
        m.insert(ROOT_NODE_ID, String::new());
        m.insert(tag_to_node_id(tag), tag.to_owned());
        m
    }

    #[test]
    fn click_lifts_to_click() {
        assert_eq!(AccessAction::from_accesskit(Action::Click), AccessAction::Click);
    }

    #[test]
    fn focus_lifts_to_focus() {
        assert_eq!(AccessAction::from_accesskit(Action::Focus), AccessAction::Focus);
    }

    #[test]
    fn increment_decrement_lift() {
        assert_eq!(
            AccessAction::from_accesskit(Action::Increment),
            AccessAction::Increment
        );
        assert_eq!(
            AccessAction::from_accesskit(Action::Decrement),
            AccessAction::Decrement
        );
    }

    #[test]
    fn unmapped_actions_become_other() {
        assert_eq!(AccessAction::from_accesskit(Action::ScrollDown), AccessAction::Other);
        assert_eq!(AccessAction::from_accesskit(Action::Expand), AccessAction::Other);
    }

    #[test]
    fn translate_resolves_widget_tag() {
        let map = map_with("main_btn");
        let req = ActionRequest {
            action: Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: tag_to_node_id("main_btn"),
            data: None,
        };
        let out = translate_action(&req, &map).expect("widget tag resolves");
        assert_eq!(out.tag, "main_btn");
        assert_eq!(out.kind, AccessAction::Click);
    }

    #[test]
    fn translate_root_target_returns_none() {
        let map = map_with("main_btn");
        let req = ActionRequest {
            action: Action::Focus,
            target_tree: accesskit::TreeId::ROOT,
            target_node: ROOT_NODE_ID,
            data: None,
        };
        assert!(translate_action(&req, &map).is_none());
    }

    #[test]
    fn translate_unknown_target_returns_none() {
        let map = map_with("main_btn");
        let req = ActionRequest {
            action: Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: NodeId(99_999_999),
            data: None,
        };
        assert!(translate_action(&req, &map).is_none());
    }

    #[test]
    fn translate_preserves_unmapped_action_as_other() {
        let map = map_with("main_btn");
        let req = ActionRequest {
            action: Action::ScrollDown,
            target_tree: accesskit::TreeId::ROOT,
            target_node: tag_to_node_id("main_btn"),
            data: None,
        };
        let out = translate_action(&req, &map).expect("widget tag resolves");
        assert_eq!(out.kind, AccessAction::Other);
    }
}
