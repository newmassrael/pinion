//! AccessKit tree builder for a **WAI-ARIA toggle-button group** (R733
//! §5.40, lifted R753): a labelled [`AriaRole::Group`] parent holding one
//! [`AriaRole::Button`] per segment, each carrying **`aria-pressed`**
//! ([`AccessState::checked`] = `Some(on)` on a `button` role).
//!
//! The `aria-pressed` ("pressed" / "not pressed") a button reflects is
//! distinct from the `aria-checked` a checkbox / switch / radio reflects,
//! even though both lower through the same AccessKit `set_toggled` call —
//! the split is purely a function of the role, exactly as WAI-ARIA defines.
//! The `group` parent (WAI-ARIA §3.6 `group`) is a labelled passive
//! container, distinct from `RadioGroup` (single-select `radio`) and
//! `Toolbar` (roving-tabindex).
//!
//! Shared by `hello-segmented-multi` (R733) and `hello-filter-chip` (R753).
//! The interaction half (keyboard / introspect reader / boot seed) lives in
//! [`pinion_core::widgets::toggle_group`]; this module is the a11y half.

use crate::node::{AccessNode, AccessState};
use crate::role::AriaRole;
use pinion_core::widgets::toggle::ToggleState;

/// One segment's identity and live interaction state for
/// [`toggle_button_group_nodes`].
#[derive(Clone, Copy, Debug)]
pub struct ToggleSegment<'a> {
    /// Dispatch tag (the segment's own `External` tag).
    pub tag: &'a str,
    /// Accessible name — an **explicit** name (explicit names survive
    /// `enrich_names_from_scene`, so a leading check-glyph `TextNode`
    /// cannot corrupt the accessible name).
    pub label: &'a str,
    /// Interaction state, driving the `disabled` / `hovered` / `pressed`
    /// flags.
    pub state: ToggleState,
    /// Whether this segment is pressed — lowered to `aria-pressed`.
    pub on: bool,
}

/// Build the `group` parent + one `button[aria-pressed]` per segment.
///
/// The group carries an explicit `name` and references every segment tag
/// as a child; each segment button carries its explicit label name, the
/// `aria-pressed` state ([`AccessState::checked`] = `Some(on)`), and
/// `focused` when its tag equals `focused`. The returned vector is
/// `[group, button_0, button_1, ...]` — the group first, mirroring the
/// flat-list convention `lower_access_node` resolves into a tree.
#[must_use]
pub fn toggle_button_group_nodes(
    group_tag: &str,
    group_name: &str,
    segments: &[ToggleSegment<'_>],
    focused: Option<&str>,
) -> Vec<AccessNode> {
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(segments.len() + 1);
    let mut group = AccessNode::new(group_tag, AriaRole::Group).with_name(group_name);
    for seg in segments {
        group = group.with_child(seg.tag);
    }
    nodes.push(group);
    for seg in segments {
        nodes.push(
            AccessNode::new(seg.tag, AriaRole::Button)
                .with_name(seg.label)
                .with_state(AccessState {
                    focused: focused == Some(seg.tag),
                    // R733 — `Some(on)` on a `button` role = aria-pressed.
                    ..AccessState::from_interaction(seg.state, Some(seg.on))
                }),
        );
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs() -> [ToggleSegment<'static>; 3] {
        [
            ToggleSegment { tag: "g_0", label: "Photos", state: ToggleState::Idle, on: true },
            ToggleSegment { tag: "g_1", label: "Videos", state: ToggleState::Hover, on: false },
            ToggleSegment { tag: "g_2", label: "Audio", state: ToggleState::Idle, on: false },
        ]
    }

    #[test]
    fn emits_group_parent_plus_n_aria_pressed_buttons() {
        let s = segs();
        let nodes = toggle_button_group_nodes("grp", "Show", &s, None);
        assert_eq!(nodes.len(), s.len() + 1, "one group + N segments");
        assert_eq!(nodes[0].role, AriaRole::Group);
        assert_eq!(nodes[0].name.as_deref(), Some("Show"));
        assert_eq!(nodes[0].children.len(), s.len(), "group references every segment");
        for (i, node) in nodes[1..].iter().enumerate() {
            assert_eq!(node.role, AriaRole::Button, "segment is a button (aria-pressed)");
            assert_eq!(node.state.checked, Some(s[i].on), "aria-pressed mirrors on/off");
            assert_eq!(node.name.as_deref(), Some(s[i].label));
        }
    }

    #[test]
    fn focused_marks_only_that_node() {
        let s = segs();
        let nodes = toggle_button_group_nodes("grp", "Show", &s, Some("g_1"));
        assert!(!nodes[1].state.focused, "segment 0 not focused");
        assert!(nodes[2].state.focused, "segment 1 focused");
        assert!(!nodes[3].state.focused, "segment 2 not focused");
    }

    #[test]
    fn hover_state_lowers_to_hovered_flag() {
        let s = segs();
        let nodes = toggle_button_group_nodes("grp", "Show", &s, None);
        assert!(nodes[2].state.hovered, "segment 1 is Hover");
        assert!(!nodes[1].state.hovered, "segment 0 is Idle");
    }
}
