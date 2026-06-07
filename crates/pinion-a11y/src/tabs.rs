//! AccessKit builder for a **WAI-ARIA `tablist` + `tab`s + active
//! `tabpanel`** (R690 §5.40, lifted R815): a labelled
//! [`AriaRole::TabList`] whose children are [`AriaRole::Tab`] nodes (one
//! carrying `aria-selected`, the roving one carrying focus), followed by
//! the single visible [`AriaRole::TabPanel`] for the selected tab.
//!
//! This is the shared a11y shape behind two consumers — `hello-tabs` (a
//! settings section switcher) and `hello-tab-reorder` (a draggable editor
//! tab strip). Both built the identical `tablist` + `tab` + `tabpanel`
//! skeleton by hand, differing only in per-tab data (tag / label /
//! selected / focused) and the two container names — a divergence between
//! them would be an a11y bug, not a style choice — so the skeleton is
//! lifted here as one source of truth (the R758 a11y-axis
//! "divergence-is-a-bug" rule).
//!
//! Mirrors [`radiogroup_radio_nodes`](crate::radiogroup_radio_nodes) /
//! [`listbox_option_nodes`](crate::listbox_option_nodes): a [`TabCell`]
//! slice + container tag / name (+ the panel tag / name), returning
//! `[tablist, tab_0, ..., tabpanel]`. `aria-posinset` / `aria-setsize`
//! are derived from each tab's index in the slice. A tab uses
//! `aria-selected` (the active tab), distinct from the `aria-checked` a
//! radio uses; tabs here carry no hover / pressed posture (the painted
//! state-layer is visual only), so only `focused` rides the state.

use crate::node::{AccessNode, AccessState};
use crate::role::AriaRole;

/// One `tab` within a [`tablist_tab_nodes`] tab strip.
#[derive(Clone, Copy, Debug)]
pub struct TabCell<'a> {
    /// The tab's `External` tag (typically a `{primary}#{i}` composite).
    pub tag: &'a str,
    /// Explicit accessible name (surviving scene enrichment), or `None` to
    /// rely on name-from-contents enrichment of the painted label.
    pub label: Option<&'a str>,
    /// `true` for the single selected tab — lowered to `aria-selected`.
    pub selected: bool,
    /// `true` when this tab is the roving active descendant (the tab list
    /// owns focus and this is the active index).
    pub focused: bool,
}

/// Build the `tablist` + one `tab` per cell + the active `tabpanel`.
///
/// The tab list carries an explicit `name` and references every tab tag as
/// a child; each tab carries its `aria-selected` value
/// ([`TabCell::selected`]), `focused` when [`TabCell::focused`], the
/// WAI-ARIA `aria-posinset` / `aria-setsize` derived from its index in
/// `tabs`, and — when [`TabCell::label`] is `Some` — an explicit accessible
/// name (else left to [`enrich_names_from_scene`](crate::enrich_names_from_scene)).
/// The single visible `tabpanel` (the selected tab's content region) is
/// appended last with `panel_name`. The returned vector is
/// `[tablist, tab_0, ..., tab_n, tabpanel]` — the container first,
/// mirroring the flat-list convention `lower_access_node` resolves into an
/// AT subtree. The panel is a sibling of the tab list (not a child), so it
/// attaches at the window root like the painted layout.
#[must_use]
pub fn tablist_tab_nodes(
    list_tag: &str,
    list_name: &str,
    tabs: &[TabCell<'_>],
    panel_tag: &str,
    panel_name: &str,
) -> Vec<AccessNode> {
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(tabs.len() + 2);
    let mut tablist = AccessNode::new(list_tag, AriaRole::TabList).with_name(list_name);
    for tab in tabs {
        tablist = tablist.with_child(tab.tag);
    }
    nodes.push(tablist);
    for (i, tab) in tabs.iter().enumerate() {
        let mut node = AccessNode::new(tab.tag, AriaRole::Tab)
            .with_selected(tab.selected)
            .with_set_position(i, tabs.len())
            .with_state(AccessState {
                focused: tab.focused,
                ..AccessState::default()
            });
        if let Some(label) = tab.label {
            node = node.with_name(label);
        }
        nodes.push(node);
    }
    nodes.push(AccessNode::new(panel_tag, AriaRole::TabPanel).with_name(panel_name));
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs() -> [TabCell<'static>; 3] {
        [
            TabCell { tag: "t#0", label: Some("General"), selected: true, focused: true },
            TabCell { tag: "t#1", label: Some("Network"), selected: false, focused: false },
            TabCell { tag: "t#2", label: None, selected: false, focused: false },
        ]
    }

    #[test]
    fn emits_tablist_tabs_then_panel() {
        let t = tabs();
        let nodes = tablist_tab_nodes("tl", "Sections", &t, "panel", "General");
        assert_eq!(nodes.len(), t.len() + 2, "tablist + N tabs + one panel");
        assert_eq!(nodes[0].role, AriaRole::TabList);
        assert_eq!(nodes[0].name.as_deref(), Some("Sections"));
        assert_eq!(nodes[0].children.len(), t.len(), "tablist references every tab");
        for node in &nodes[1..=t.len()] {
            assert_eq!(node.role, AriaRole::Tab);
        }
        let panel = nodes.last().expect("panel present");
        assert_eq!(panel.role, AriaRole::TabPanel);
        assert_eq!(panel.name.as_deref(), Some("General"));
    }

    #[test]
    fn panel_is_not_a_child_of_the_tablist() {
        let nodes = tablist_tab_nodes("tl", "Sections", &tabs(), "panel", "General");
        assert!(
            !nodes[0].children.iter().any(|c| c == "panel"),
            "the tabpanel is a sibling of the tablist, not a child",
        );
    }

    #[test]
    fn only_selected_tab_carries_aria_selected() {
        let nodes = tablist_tab_nodes("tl", "Sections", &tabs(), "panel", "General");
        assert_eq!(nodes[1].selected, Some(true), "tab 0 selected");
        assert_eq!(nodes[2].selected, Some(false), "tab 1 not selected");
        assert_eq!(nodes[3].selected, Some(false), "tab 2 not selected");
    }

    #[test]
    fn tabs_carry_posinset_and_setsize_over_the_slice() {
        let nodes = tablist_tab_nodes("tl", "Sections", &tabs(), "panel", "General");
        assert_eq!(nodes[1].position_in_set, Some(1));
        assert_eq!(nodes[1].size_of_set, Some(3));
        assert_eq!(nodes[3].position_in_set, Some(3));
        assert_eq!(nodes[3].size_of_set, Some(3));
    }

    #[test]
    fn focused_tab_carries_focused_state() {
        let nodes = tablist_tab_nodes("tl", "Sections", &tabs(), "panel", "General");
        assert!(nodes[1].state.focused, "tab 0 is the roving active descendant");
        assert!(!nodes[2].state.focused, "tab 1 not the active descendant");
        // Tabs use aria-selected, never aria-checked.
        assert!(nodes[1..=3].iter().all(|n| n.state.checked.is_none()));
    }

    #[test]
    fn explicit_label_set_and_none_left_for_enrichment() {
        let nodes = tablist_tab_nodes("tl", "Sections", &tabs(), "panel", "General");
        assert_eq!(nodes[1].name.as_deref(), Some("General"), "explicit label applied");
        assert_eq!(nodes[3].name, None, "label None left to scene enrichment");
    }
}
