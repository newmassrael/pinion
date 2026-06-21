//! AccessKit tree builder for a **WAI-ARIA `radiogroup` holding a list of
//! `radio`s** (R758 §5.40): a labelled [`AriaRole::RadioGroup`] parent whose
//! children are [`AriaRole::RadioButton`] nodes, exactly one of which is
//! selected (`aria-checked="true"`).
//!
//! This is the shared a11y shape behind three single-select composite
//! consumers: `hello-radio-group` (R51.44), `hello-segmented-button` (R728)
//! and `hello-rating` (R758). All three build the identical
//! group-plus-radio-children topology — a divergence between them would be an
//! a11y bug, not a style choice — so it is lifted here as one source of truth
//! (the R743.1 / R745 "divergence-is-a-bug" rule; the third consumer is the
//! trigger). It is the radio-family analogue of the already-lifted
//! [`navigation_link_nodes`](crate::navigation_link_nodes) (R754, the `link`
//! family) and [`toggle_button_group_nodes`](crate::toggle_button_group_nodes)
//! (R753, the toggle-button family); the radio family was the only one of the
//! three composite-radio-coordinator skins still carrying the duplicate.
//!
//! The radios are backed by a single-select
//! [`RadioGroupExternal`](pinion_core::widgets::radio_group::RadioGroupExternal)
//! coordinator, so each radio's interaction posture is a [`RadioState`] and
//! the selected radio is the coordinator's `selected_index`.

use crate::node::{AccessNode, AccessState, AccessValue};
use crate::role::AriaRole;
use pinion_core::widgets::radio::RadioState;

/// One `radio` within a [`radiogroup_radio_nodes`] group.
#[derive(Clone, Copy, Debug)]
pub struct RadioCell<'a> {
    /// The radio's `External` tag (typically a `{primary}#{i}` composite).
    pub tag: &'a str,
    /// Explicit accessible name (surviving scene enrichment), or `None` to
    /// rely on name-from-contents enrichment of the painted label.
    pub label: Option<&'a str>,
    /// Interaction posture, driving `disabled` / `hovered` / `pressed`.
    pub state: RadioState,
    /// `true` for the single selected radio — lowered to `aria-checked`.
    pub selected: bool,
    /// `true` when this radio is the roving active descendant (the group
    /// owns focus and this is the active index).
    pub focused: bool,
}

/// Build the `radiogroup` parent + one `radio` per cell.
///
/// The group carries an explicit `name` and references every radio tag as a
/// child; each radio carries its `aria-checked` value ([`RadioCell::selected`]),
/// its interaction state, `focused` when [`RadioCell::focused`], and — when
/// [`RadioCell::label`] is `Some`— an explicit accessible name (else the name
/// is left to [`enrich_names_from_scene`](crate::enrich_names_from_scene)).
/// The returned vector is `[group, radio_0, radio_1, ...]` — the group first,
/// mirroring the flat-list convention the tree builder resolves into a tree.
#[must_use]
pub fn radiogroup_radio_nodes(
    group_tag: &str,
    group_name: &str,
    cells: &[RadioCell<'_>],
) -> Vec<AccessNode> {
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(cells.len() + 1);
    let mut group = AccessNode::new(group_tag, AriaRole::RadioGroup).with_name(group_name);
    for cell in cells {
        group = group.with_child(cell.tag);
    }
    nodes.push(group);
    for cell in cells {
        let mut node = AccessNode::new(cell.tag, AriaRole::RadioButton)
            .with_value(AccessValue::Bool(cell.selected))
            .with_state(AccessState {
                focused: cell.focused,
                ..AccessState::from_interaction(cell.state, Some(cell.selected))
            });
        if let Some(label) = cell.label {
            node = node.with_name(label);
        }
        nodes.push(node);
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells() -> [RadioCell<'static>; 3] {
        [
            RadioCell {
                tag: "g#0",
                label: Some("One"),
                state: RadioState::Idle,
                selected: false,
                focused: false,
            },
            RadioCell {
                tag: "g#1",
                label: Some("Two"),
                state: RadioState::Hover,
                selected: true,
                focused: true,
            },
            RadioCell {
                tag: "g#2",
                label: None,
                state: RadioState::Idle,
                selected: false,
                focused: false,
            },
        ]
    }

    #[test]
    fn emits_group_plus_n_radios_in_order() {
        let c = cells();
        let nodes = radiogroup_radio_nodes("g", "Choice", &c);
        assert_eq!(nodes.len(), c.len() + 1, "one group + N radios");
        assert_eq!(nodes[0].role, AriaRole::RadioGroup);
        assert_eq!(nodes[0].name.as_deref(), Some("Choice"));
        assert_eq!(
            nodes[0].children.len(),
            c.len(),
            "group references every radio"
        );
        for node in &nodes[1..] {
            assert_eq!(node.role, AriaRole::RadioButton);
        }
    }

    #[test]
    fn only_selected_radio_is_checked_true() {
        let nodes = radiogroup_radio_nodes("g", "Choice", &cells());
        assert_eq!(nodes[1].state.checked, Some(false), "radio 0 unchecked");
        assert_eq!(nodes[2].state.checked, Some(true), "radio 1 selected");
        assert_eq!(nodes[2].value, Some(AccessValue::Bool(true)));
        assert_eq!(nodes[3].state.checked, Some(false), "radio 2 unchecked");
    }

    #[test]
    fn explicit_label_set_and_none_left_for_enrichment() {
        let nodes = radiogroup_radio_nodes("g", "Choice", &cells());
        assert_eq!(
            nodes[1].name.as_deref(),
            Some("One"),
            "explicit label applied"
        );
        assert!(
            nodes[3].name.is_none(),
            "None label left for name-from-contents"
        );
    }

    #[test]
    fn focused_and_hover_state_lower_to_flags() {
        let nodes = radiogroup_radio_nodes("g", "Choice", &cells());
        assert!(nodes[2].state.focused, "radio 1 focused");
        assert!(nodes[2].state.hovered, "radio 1 hovered");
        assert!(!nodes[1].state.focused, "radio 0 not focused");
    }
}
