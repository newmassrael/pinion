//! AccessKit builder for a **WAI-ARIA `listbox` holding `option`s**
//! (R51.98 §5.40, lifted R814): a labelled [`AriaRole::Listbox`] container
//! whose children are [`AriaRole::ListBoxOption`] nodes, each carrying its
//! `aria-selected` membership and interaction posture, with the container
//! optionally `aria-multiselectable`.
//!
//! This is the shared a11y shape behind four consumers — `hello-listbox`
//! and `hello-listbox-multi` (full single- / multi-select listboxes), plus
//! the popup of `hello-combobox` and `hello-combobox-editable` (a combobox
//! is a trigger that *controls* a listbox; the trigger node stays bespoke,
//! but the popup is exactly this topology). All four built the identical
//! container-plus-option structure by hand — a divergence between them
//! would be an a11y bug, not a style choice — so it is lifted here as one
//! source of truth (the R758 a11y-axis "divergence-is-a-bug" rule; four
//! consumers is well past the Rule-of-Three threshold the radio / nav /
//! toggle builders were lifted at).
//!
//! Mirrors [`radiogroup_radio_nodes`](crate::radiogroup_radio_nodes): a
//! [`ListOption`] cell slice + container tag / name, returning
//! `[listbox, option_0, option_1, ...]`. The option's interaction posture
//! is a [`ListboxItemState`]; its selection drives `aria-selected` (the
//! container-membership axis, not the two-state `aria-checked` a radio
//! uses).

use crate::node::{AccessNode, AccessState};
use crate::role::AriaRole;
use pinion_core::widgets::listbox_item::ListboxItemState;

/// One `option` within a [`listbox_option_nodes`] listbox.
#[derive(Clone, Copy, Debug)]
pub struct ListOption<'a> {
    /// The option's `External` tag (typically a `{primary}#{i}` composite).
    pub tag: &'a str,
    /// Explicit accessible name (surviving scene enrichment), or `None` to
    /// rely on name-from-contents enrichment of the painted label.
    pub label: Option<&'a str>,
    /// Interaction posture, driving `disabled` / `hovered` / `pressed`.
    pub state: ListboxItemState,
    /// `true` for a selected option — lowered to `aria-selected`. A
    /// single-select listbox marks one option; a multi-select listbox may
    /// mark several (the container also carries `aria-multiselectable`).
    pub selected: bool,
    /// `true` when this option is the roving active descendant (the listbox
    /// owns focus and this is the active index).
    pub focused: bool,
}

/// Build the `listbox` container + one `option` per cell.
///
/// The container carries an explicit `name`, `aria-multiselectable` when
/// `multiselectable`, and references every option tag as a child; each
/// option carries its `aria-selected` value ([`ListOption::selected`]), its
/// interaction state, `focused` when [`ListOption::focused`], the WAI-ARIA
/// `aria-posinset` / `aria-setsize` derived from its index in `options`
/// (so a screen reader announces "option N of M"), and — when
/// [`ListOption::label`] is `Some` — an explicit accessible name (else the
/// name is left to [`enrich_names_from_scene`](crate::enrich_names_from_scene)).
/// The returned vector is `[listbox, option_0, option_1, ...]` — the
/// container first, mirroring the flat-list convention `lower_access_node`
/// resolves into an AT subtree (identical to
/// [`radiogroup_radio_nodes`](crate::radiogroup_radio_nodes)).
///
/// `aria-posinset` / `aria-setsize` track the **passed slice**: a filtered
/// combobox popup passes only its visible options, so each numbers within
/// the filtered set (e.g. "option 1 of 3" after typing narrows the list) —
/// the WAI-ARIA contract for a dynamically filtered listbox.
#[must_use]
pub fn listbox_option_nodes(
    list_tag: &str,
    list_name: &str,
    multiselectable: bool,
    options: &[ListOption<'_>],
) -> Vec<AccessNode> {
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(options.len() + 1);
    let mut list = AccessNode::new(list_tag, AriaRole::Listbox).with_name(list_name);
    if multiselectable {
        list = list.with_multiselectable();
    }
    for option in options {
        list = list.with_child(option.tag);
    }
    nodes.push(list);
    let set_size = u32::try_from(options.len()).unwrap_or(u32::MAX);
    for (i, option) in options.iter().enumerate() {
        let mut node = AccessNode::new(option.tag, AriaRole::ListBoxOption)
            .with_selected(option.selected)
            .with_position_in_set(u32::try_from(i + 1).unwrap_or(u32::MAX))
            .with_size_of_set(set_size)
            .with_state(AccessState {
                focused: option.focused,
                ..AccessState::from_interaction(option.state, None)
            });
        if let Some(label) = option.label {
            node = node.with_name(label);
        }
        nodes.push(node);
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> [ListOption<'static>; 3] {
        [
            ListOption {
                tag: "l#0",
                label: Some("Small"),
                state: ListboxItemState::Idle,
                selected: false,
                focused: false,
            },
            ListOption {
                tag: "l#1",
                label: Some("Medium"),
                state: ListboxItemState::Hover,
                selected: true,
                focused: true,
            },
            ListOption {
                tag: "l#2",
                label: None,
                state: ListboxItemState::Disabled,
                selected: false,
                focused: false,
            },
        ]
    }

    #[test]
    fn emits_listbox_plus_n_options_in_order() {
        let o = options();
        let nodes = listbox_option_nodes("l", "Sizes", false, &o);
        assert_eq!(nodes.len(), o.len() + 1, "one listbox + N options");
        assert_eq!(nodes[0].role, AriaRole::Listbox);
        assert_eq!(nodes[0].name.as_deref(), Some("Sizes"));
        assert_eq!(nodes[0].children.len(), o.len(), "listbox references every option");
        for node in &nodes[1..] {
            assert_eq!(node.role, AriaRole::ListBoxOption);
        }
    }

    #[test]
    fn selected_option_carries_aria_selected() {
        let nodes = listbox_option_nodes("l", "Sizes", false, &options());
        assert_eq!(nodes[1].selected, Some(false), "option 0 not selected");
        assert_eq!(nodes[2].selected, Some(true), "option 1 selected");
        assert_eq!(nodes[3].selected, Some(false), "option 2 not selected");
    }

    #[test]
    fn single_select_omits_multiselectable_flag() {
        let nodes = listbox_option_nodes("l", "Sizes", false, &options());
        assert!(!nodes[0].multiselectable, "single-select listbox is not multiselectable");
    }

    #[test]
    fn multiselectable_flag_lowers_on_the_container() {
        let nodes = listbox_option_nodes("l", "Sizes", true, &options());
        assert!(nodes[0].multiselectable, "multi-select listbox sets aria-multiselectable");
        // The flag is a container property; options are unaffected.
        assert!(nodes[1..].iter().all(|n| !n.multiselectable));
    }

    #[test]
    fn explicit_label_set_and_none_left_for_enrichment() {
        let nodes = listbox_option_nodes("l", "Sizes", false, &options());
        assert_eq!(nodes[1].name.as_deref(), Some("Small"), "explicit label applied");
        assert_eq!(nodes[3].name, None, "label None left to scene enrichment");
    }

    #[test]
    fn focused_option_carries_focused_state() {
        let nodes = listbox_option_nodes("l", "Sizes", false, &options());
        assert!(!nodes[1].state.focused, "option 0 not the active descendant");
        assert!(nodes[2].state.focused, "option 1 is the roving active descendant");
    }

    #[test]
    fn interaction_posture_maps_to_state() {
        let nodes = listbox_option_nodes("l", "Sizes", false, &options());
        // Hover posture → hovered; Disabled posture → disabled.
        assert!(nodes[2].state.hovered, "Hover posture lowers to hovered");
        assert!(nodes[3].state.disabled, "Disabled posture lowers to disabled");
        // aria-selected is the membership axis — options never set aria-checked.
        assert!(nodes[1..].iter().all(|n| n.state.checked.is_none()));
    }

    #[test]
    fn options_carry_posinset_and_setsize_over_the_slice() {
        // WAI-ARIA "option N of M" — derived from the passed slice, so a
        // filtered combobox popup numbers within its visible subset.
        let nodes = listbox_option_nodes("l", "Sizes", false, &options());
        assert_eq!(nodes[1].position_in_set, Some(1));
        assert_eq!(nodes[1].size_of_set, Some(3));
        assert_eq!(nodes[3].position_in_set, Some(3));
        assert_eq!(nodes[3].size_of_set, Some(3));
        // A two-option (e.g. filtered) slice sets setsize = 2.
        let two = listbox_option_nodes("l", "Sizes", false, &options()[..2]);
        assert_eq!(two[1].size_of_set, Some(2));
        assert_eq!(two[2].position_in_set, Some(2));
    }

    #[test]
    fn empty_options_emit_listbox_only() {
        let nodes = listbox_option_nodes("l", "Sizes", false, &[]);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Listbox);
        assert!(nodes[0].children.is_empty());
    }
}
