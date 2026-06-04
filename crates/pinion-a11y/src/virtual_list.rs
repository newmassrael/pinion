//! AccessKit tree builder for a **WAI-ARIA virtualized `list`** (R744
//! §5.27, lifted R774): one labelled [`AriaRole::List`] container with
//! `aria-setsize = <full dataset size>` claiming the windowed row tags as
//! children, plus one [`AriaRole::ListItem`] per rendered index carrying
//! its absolute 1-based `aria-posinset` + the same `aria-setsize`.
//!
//! This is the canonical AT model for a virtualized list — the *full*
//! extent is conveyed by `aria-setsize`, the *rendered* window by the
//! present `listitem` nodes — and the shared shape behind three
//! **display-only** consumers: `hello-virtual-list` (R744, fixed pitch),
//! `hello-variable-list` (R745, prefix-sum pitch) and
//! `hello-flex-virtual-list` (R774, `AutoSizer` measured viewport). All
//! three build the identical setsize / posinset / child topology from a
//! [`VisibleWindow`]; a divergence between them would be an a11y bug, not
//! a style choice (the R743.1 / R745 "divergence-is-a-bug" rule). The flex
//! binding is the third consumer that triggers the lift, per R758's
//! a11y-axis self-grep mandate.
//!
//! Selectable / sortable virtualized lists (`hello-virtual-select`,
//! `hello-virtual-sort`) add per-item `aria-selected` and (sort) a
//! view-order permutation plus a preceding control node, so they build
//! their `listitem` nodes inline — a genuine per-item-state divergence,
//! not the mechanical display-only wiring lifted here. A future *second*
//! selectable virtualized list is the trigger to lift a decorated variant
//! ([`abstraction-needs-second-consumer`]).

use crate::node::AccessNode;
use crate::role::AriaRole;
use pinion_core::widgets::virtual_list::VisibleWindow;

/// Build the virtualized `list` container + one `listitem` per windowed
/// index.
///
/// The container is tagged `list_tag` with the explicit `list_name` and
/// `aria-setsize = set_size` (the *full* dataset size, not the rendered
/// count), and references every windowed row tag (`{list_tag}#{index}`)
/// as a child. Each rendered index gets one [`AriaRole::ListItem`]
/// carrying its 1-based `aria-posinset` and the same `aria-setsize`. The
/// returned vector is `[list, item_first, item_first+1, …]` — the
/// container first, mirroring the flat-list convention
/// `lower_access_node` resolves into a tree.
///
/// `window` is the [`VisibleWindow`] the binding's view fn windowed
/// against (same `compute_visible_range*` source), so the a11y tree and
/// the painted tree never disagree on which rows exist.
#[must_use]
pub fn windowed_list_nodes(
    list_tag: &str,
    list_name: &str,
    set_size: u32,
    window: &VisibleWindow,
) -> Vec<AccessNode> {
    let mut list = AccessNode::new(list_tag, AriaRole::List)
        .with_name(list_name)
        .with_size_of_set(set_size);
    for index in window.indices() {
        list = list.with_child(format!("{list_tag}#{index}"));
    }
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(window.count + 1);
    nodes.push(list);
    for index in window.indices() {
        let posinset = u32::try_from(index + 1).unwrap_or(u32::MAX);
        nodes.push(
            AccessNode::new(format!("{list_tag}#{index}"), AriaRole::ListItem)
                .with_position_in_set(posinset)
                .with_size_of_set(set_size),
        );
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(first: usize, count: usize) -> VisibleWindow {
        VisibleWindow { first, count }
    }

    #[test]
    fn emits_list_plus_one_item_per_window_index() {
        let nodes = windowed_list_nodes("vlist", "Item list", 10_000, &window(0, 3));
        assert_eq!(nodes.len(), 4, "one list container + 3 windowed items");
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(nodes[0].name.as_deref(), Some("Item list"));
        assert_eq!(nodes[0].size_of_set, Some(10_000), "setsize conveys the FULL dataset");
        assert_eq!(nodes[0].children.len(), 3, "list references every windowed row tag");
        for item in &nodes[1..] {
            assert_eq!(item.role, AriaRole::ListItem);
            assert_eq!(item.size_of_set, Some(10_000));
        }
    }

    #[test]
    fn posinset_is_one_based_and_tracks_the_window() {
        // A deep window: indices 100..103 → posinset 101..104, tags pinned
        // to the absolute index.
        let nodes = windowed_list_nodes("vlist", "List", 10_000, &window(100, 3));
        assert_eq!(nodes[1].tag, "vlist#100");
        assert_eq!(nodes[1].position_in_set, Some(101));
        assert_eq!(nodes[3].tag, "vlist#102");
        assert_eq!(nodes[3].position_in_set, Some(103));
    }

    #[test]
    fn empty_window_yields_list_only() {
        let nodes = windowed_list_nodes("vlist", "List", 10_000, &VisibleWindow::EMPTY);
        assert_eq!(nodes.len(), 1, "an empty window has no listitem nodes");
        assert_eq!(nodes[0].role, AriaRole::List);
        assert!(nodes[0].children.is_empty(), "no children claimed");
    }
}
