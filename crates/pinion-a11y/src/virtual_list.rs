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

/// Build a **single-select** virtualized `list`: the same windowed
/// container + `listitem` topology as [`windowed_list_nodes`], but every
/// rendered item additionally carries `aria-selected = (index ==
/// selected)`.
///
/// This is the decorated peer the display-only [`windowed_list_nodes`]
/// doc anticipated: a virtualized list whose selection is held by **data
/// index** (a `VirtualSelectExternal`-style coordinator) rather than a
/// per-leaf bit. The `aria-selected` axis is the genuine divergence from
/// the display-only shape — display-only items omit the attribute
/// entirely (they are not selectable), so the two builders are peers, not
/// one wrapping the other. Lifted on the **second** selectable consumer
/// (`hello-virtual-select` pointer/RPC + `hello-virtual-nav` keyboard nav)
/// per the R758 a11y-axis self-grep mandate: the setsize / posinset /
/// child topology plus the per-item `aria-selected` lowering would be a
/// divergence-is-a-bug if hand-rolled twice (the R743.1 / R745 rule).
///
/// The container is a single-select [`AriaRole::List`] — **no**
/// `aria-multiselectable` (this slice is single-select, the listbox /
/// data-grid default; the multi-select index model is the decorated peer
/// [`windowed_list_nodes_multiselected`], R780). `selected` is the
/// absolute data index of the selected row, or `None`. Because selection
/// is held by index, a selected row that has scrolled out of the window
/// simply has no `listitem` node this frame — the selection survives in
/// the coordinator and re-paints `aria-selected=true` when the row scrolls
/// back, exactly as the painted tree does.
#[must_use]
pub fn windowed_list_nodes_selected(
    list_tag: &str,
    list_name: &str,
    set_size: u32,
    window: &VisibleWindow,
    selected: Option<usize>,
) -> Vec<AccessNode> {
    let mut nodes = windowed_list_nodes(list_tag, list_name, set_size, window);
    // `nodes[0]` is the container; `nodes[1..]` are the windowed items in
    // window order, so item `k` is data index `window.first + k`. Decorate
    // each with `aria-selected` against the index model.
    for (offset, item) in nodes[1..].iter_mut().enumerate() {
        let index = window.first + offset;
        item.selected = Some(selected == Some(index));
    }
    nodes
}

/// Build a **multi-select** virtualized `list` (R780 §5.40): the same
/// windowed container + `listitem` topology as [`windowed_list_nodes`],
/// with `aria-multiselectable` on the container and `aria-selected =
/// selection.contains(index)` on every rendered item.
///
/// The decorated peer of [`windowed_list_nodes_selected`] for a
/// [`VirtualSelectExternal`](pinion_core::widgets::virtual_select::VirtualSelectExternal)
/// in `Multi` mode: selection is an arbitrary index **set**, so the
/// container additionally sets `aria-multiselectable="true"` — the genuine
/// divergence from the single-select peer (the eager `hello-table-multi` /
/// `hello-listbox-multi` set the same `with_multiselectable` axis inline on
/// their non-windowed nodes; this is the *windowed* builder's variant).
/// Multiple windowed items can carry `aria-selected=true` at once; selected
/// rows scrolled out of the window have no node this frame, exactly as the
/// single-select peer (the set survives in the coordinator).
#[must_use]
pub fn windowed_list_nodes_multiselected(
    list_tag: &str,
    list_name: &str,
    set_size: u32,
    window: &VisibleWindow,
    selection: &std::collections::BTreeSet<usize>,
) -> Vec<AccessNode> {
    let mut nodes = windowed_list_nodes(list_tag, list_name, set_size, window);
    nodes[0].multiselectable = true;
    for (offset, item) in nodes[1..].iter_mut().enumerate() {
        let index = window.first + offset;
        item.selected = Some(selection.contains(&index));
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
        assert_eq!(
            nodes[0].size_of_set,
            Some(10_000),
            "setsize conveys the FULL dataset"
        );
        assert_eq!(
            nodes[0].children.len(),
            3,
            "list references every windowed row tag"
        );
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

    // ── R776 selectable decorated variant ───────────────────────────

    #[test]
    fn selected_marks_only_the_selected_window_index() {
        // Window 100..103, row 101 selected → only that item carries
        // aria-selected=true; the others carry an explicit false.
        let nodes =
            windowed_list_nodes_selected("vlist", "List", 10_000, &window(100, 3), Some(101));
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(nodes[1].tag, "vlist#100");
        assert_eq!(nodes[1].selected, Some(false));
        assert_eq!(nodes[2].tag, "vlist#101");
        assert_eq!(
            nodes[2].selected,
            Some(true),
            "the selected index is aria-selected"
        );
        assert_eq!(nodes[3].tag, "vlist#102");
        assert_eq!(nodes[3].selected, Some(false));
    }

    #[test]
    fn selected_is_a_superset_of_the_display_only_topology() {
        // The decorated variant must build the IDENTICAL container +
        // posinset + child topology as the display-only builder — only
        // the per-item `aria-selected` axis is added.
        let plain = windowed_list_nodes("vlist", "List", 10_000, &window(0, 3));
        let decorated = windowed_list_nodes_selected("vlist", "List", 10_000, &window(0, 3), None);
        assert_eq!(plain.len(), decorated.len());
        for (p, d) in plain.iter().zip(&decorated) {
            assert_eq!(p.tag, d.tag);
            assert_eq!(p.role, d.role);
            assert_eq!(p.position_in_set, d.position_in_set);
            assert_eq!(p.size_of_set, d.size_of_set);
            assert_eq!(p.children, d.children);
        }
        // The container is single-select: no aria-multiselectable.
        assert!(!decorated[0].multiselectable, "single-select list");
        // Display-only items omit the selected axis entirely; the
        // decorated ones set it (to false here, nothing selected).
        assert_eq!(plain[1].selected, None, "display-only omits aria-selected");
        assert_eq!(decorated[1].selected, Some(false), "selectable sets it");
    }

    #[test]
    fn selected_outside_window_marks_no_visible_item() {
        // Selection 9_999 scrolled out of the 0..3 window: every visible
        // item is aria-selected=false (the selection survives off-window
        // in the coordinator, with no node this frame).
        let nodes =
            windowed_list_nodes_selected("vlist", "List", 10_000, &window(0, 3), Some(9_999));
        for item in &nodes[1..] {
            assert_eq!(item.selected, Some(false));
        }
    }

    #[test]
    fn selected_empty_window_yields_list_only() {
        let nodes =
            windowed_list_nodes_selected("vlist", "List", 10_000, &VisibleWindow::EMPTY, Some(0));
        assert_eq!(nodes.len(), 1, "no items when the window is empty");
        assert_eq!(nodes[0].role, AriaRole::List);
    }

    // ── R780 multi-select decorated variant ─────────────────────────

    #[test]
    fn multiselect_marks_every_member_and_the_container() {
        // Window 100..104; rows 101 and 103 selected → both aria-selected,
        // the container aria-multiselectable.
        let selection: std::collections::BTreeSet<usize> = [101, 103].into_iter().collect();
        let nodes =
            windowed_list_nodes_multiselected("vlist", "List", 10_000, &window(100, 4), &selection);
        assert!(
            nodes[0].multiselectable,
            "multi-select container is aria-multiselectable"
        );
        assert_eq!(nodes[1].tag, "vlist#100");
        assert_eq!(nodes[1].selected, Some(false));
        assert_eq!(nodes[2].tag, "vlist#101");
        assert_eq!(nodes[2].selected, Some(true));
        assert_eq!(nodes[3].tag, "vlist#102");
        assert_eq!(nodes[3].selected, Some(false));
        assert_eq!(nodes[4].tag, "vlist#103");
        assert_eq!(
            nodes[4].selected,
            Some(true),
            "two members aria-selected at once"
        );
    }

    #[test]
    fn multiselect_is_a_superset_of_the_display_only_topology() {
        // Identical container + posinset + child topology as display-only;
        // only aria-multiselectable + per-item aria-selected are added.
        let selection = std::collections::BTreeSet::new();
        let plain = windowed_list_nodes("vlist", "List", 10_000, &window(0, 3));
        let decorated =
            windowed_list_nodes_multiselected("vlist", "List", 10_000, &window(0, 3), &selection);
        assert_eq!(plain.len(), decorated.len());
        for (p, d) in plain.iter().zip(&decorated) {
            assert_eq!(p.tag, d.tag);
            assert_eq!(p.role, d.role);
            assert_eq!(p.position_in_set, d.position_in_set);
            assert_eq!(p.size_of_set, d.size_of_set);
            assert_eq!(p.children, d.children);
        }
        assert!(
            !plain[0].multiselectable,
            "display-only is not multiselectable"
        );
        assert!(decorated[0].multiselectable);
        assert_eq!(plain[1].selected, None, "display-only omits aria-selected");
        assert_eq!(decorated[1].selected, Some(false), "multi-select sets it");
    }

    #[test]
    fn multiselect_empty_window_yields_list_only() {
        let selection: std::collections::BTreeSet<usize> = [0, 1].into_iter().collect();
        let nodes = windowed_list_nodes_multiselected(
            "vlist",
            "List",
            10_000,
            &VisibleWindow::EMPTY,
            &selection,
        );
        assert_eq!(nodes.len(), 1, "no items when the window is empty");
        assert!(nodes[0].multiselectable, "the container axis still applies");
    }
}
