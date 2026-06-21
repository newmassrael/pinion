//! AccessKit tree builder for a **WAI-ARIA `navigation` landmark holding a
//! list of `link`s** (R731 §5.40, lifted R754): a labelled
//! [`AriaRole::Navigation`] landmark whose children are
//! [`AriaRole::Link`] nodes, with at most one link carrying
//! **`aria-current="page"`**.
//!
//! This is the shared a11y shape behind three consumers:
//! `hello-breadcrumb` (R731, a crumb trail), `hello-nav-rail` (R751, a
//! vertical destination rail) and `hello-pagination` (R754, numbered page
//! links + previous / next). All three build the identical
//! landmark-plus-current-link topology — a divergence between them would be
//! an a11y bug, not a style choice — so it is lifted here as one source of
//! truth (the R743.1 / R745 "divergence-is-a-bug" rule; the third consumer
//! is the trigger flagged in the R751 carry).
//!
//! The links are backed by a single-select
//! [`RadioGroupExternal`](pinion_core::widgets::radio_group::RadioGroupExternal)
//! coordinator, so each link's interaction posture is a
//! [`RadioState`]; the `current` link is the selected one. (Pagination's
//! previous / next controls are themselves non-current links — WAI-ARIA
//! models them as links within the same landmark — so they flow through the
//! same uniform list.)

use crate::node::{AccessNode, AccessState};
use crate::role::{AriaCurrent, AriaRole};
use pinion_core::widgets::radio::RadioState;

/// One `link` within a [`navigation_link_nodes`] landmark.
#[derive(Clone, Copy, Debug)]
pub struct NavLink<'a> {
    /// The link's `External` tag (typically a `{primary}#{i}` composite).
    pub tag: &'a str,
    /// Accessible name (an explicit name, surviving scene enrichment).
    pub label: &'a str,
    /// Interaction posture, driving `disabled` / `hovered` / `pressed`.
    pub state: RadioState,
    /// `true` for the single current link — lowered to `aria-current="page"`.
    pub current: bool,
    /// `true` when this link is the roving active descendant (the landmark
    /// owns focus and this is the active index).
    pub focused: bool,
}

/// Build the `navigation` landmark + one `link` per entry.
///
/// The landmark carries an explicit `name` and references every link tag as
/// a child; each link carries its explicit label name, its interaction
/// state, `aria-current="page"` when [`NavLink::current`], and `focused`
/// when [`NavLink::focused`]. The returned vector is
/// `[landmark, link_0, link_1, ...]` — the landmark first, mirroring the
/// flat-list convention `lower_access_node` resolves into a tree.
#[must_use]
pub fn navigation_link_nodes(
    nav_tag: &str,
    nav_name: &str,
    links: &[NavLink<'_>],
) -> Vec<AccessNode> {
    let mut nodes: Vec<AccessNode> = Vec::with_capacity(links.len() + 1);
    let mut nav = AccessNode::new(nav_tag, AriaRole::Navigation).with_name(nav_name);
    for link in links {
        nav = nav.with_child(link.tag);
    }
    nodes.push(nav);
    for link in links {
        let mut node = AccessNode::new(link.tag, AriaRole::Link)
            .with_name(link.label)
            .with_state(AccessState {
                focused: link.focused,
                ..AccessState::from_interaction(link.state, None)
            });
        if link.current {
            node = node.with_current(AriaCurrent::Page);
        }
        nodes.push(node);
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links() -> [NavLink<'static>; 3] {
        [
            NavLink {
                tag: "n#0",
                label: "Home",
                state: RadioState::Idle,
                current: false,
                focused: false,
            },
            NavLink {
                tag: "n#1",
                label: "Docs",
                state: RadioState::Hover,
                current: false,
                focused: true,
            },
            NavLink {
                tag: "n#2",
                label: "Now",
                state: RadioState::Idle,
                current: true,
                focused: false,
            },
        ]
    }

    #[test]
    fn emits_landmark_plus_n_links() {
        let l = links();
        let nodes = navigation_link_nodes("nav", "Breadcrumb", &l);
        assert_eq!(nodes.len(), l.len() + 1, "one landmark + N links");
        assert_eq!(nodes[0].role, AriaRole::Navigation);
        assert_eq!(nodes[0].name.as_deref(), Some("Breadcrumb"));
        assert_eq!(
            nodes[0].children.len(),
            l.len(),
            "landmark references every link"
        );
        for (i, node) in nodes[1..].iter().enumerate() {
            assert_eq!(node.role, AriaRole::Link, "child is a link");
            assert_eq!(node.name.as_deref(), Some(l[i].label));
        }
    }

    #[test]
    fn only_current_link_carries_aria_current_page() {
        let nodes = navigation_link_nodes("nav", "Primary", &links());
        assert_eq!(nodes[1].current, None, "link 0 not current");
        assert_eq!(nodes[2].current, None, "link 1 not current");
        assert_eq!(
            nodes[3].current,
            Some(AriaCurrent::Page),
            "link 2 is current"
        );
    }

    #[test]
    fn focused_and_hover_state_lower_to_flags() {
        let nodes = navigation_link_nodes("nav", "Primary", &links());
        assert!(nodes[2].state.focused, "link 1 focused");
        assert!(nodes[2].state.hovered, "link 1 hovered");
        assert!(!nodes[1].state.focused, "link 0 not focused");
    }
}
