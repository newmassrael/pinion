//! Shared a11y shape for an element **conditionally described by an
//! auxiliary region** through `aria-describedby` (R759 §5.40, lifted from
//! R729 / R694 tooltip + R759 badge).
//!
//! WAI-ARIA `aria-describedby` must reference a node that *exists*: a
//! description region (a tooltip, a status badge, a form-field hint, an
//! error message) is present in the accessibility tree only while it is
//! shown, and the referencing control must drop the `describedby` link
//! when the region is gone — a dangling reference to an absent node is an
//! AT defect, not a style choice. This is the
//! [`describedby_region`] single source of truth so each consumer cannot
//! forget the gating (the R743.1 / R745 "divergence-is-a-bug" rule, the
//! same reason [`navigation_link_nodes`](crate::navigation_link_nodes) is
//! lifted).
//!
//! Three consumers share this shape: `hello-tooltip` (R694, a hover/focus
//! tooltip), `hello-tooltip-rich` (R729, a titled tooltip body) and
//! `hello-badge` (R759, a count / dot status region). They differ only in
//! the region's *role* ([`AriaRole::Tooltip`] vs [`AriaRole::Status`]) and
//! whether it carries an explicit name — both pure data, so one helper
//! serves all three without a behaviour-bifurcating flag.

use crate::node::AccessNode;
use crate::role::AriaRole;

/// Wire `control` to an auxiliary description `region` via
/// `aria-describedby`, present only while `present` is `true`.
///
/// Returns the flat node list the
/// [`AccessTreeBuilder`](crate::AccessTreeBuilder) lowers:
///
/// * `present == true` → `[control-with-describedby, region]` — the
///   control references the region, and the region node (carrying
///   `region_role` and, when supplied, `region_name`) is emitted;
/// * `present == false` → `[control]` — the bare control with **no**
///   `describedby` link (no dangling reference to an absent region).
///
/// The caller builds `control` fully (role, name, interaction state)
/// before passing it in; this helper only owns the describedby link, the
/// region node, and the presence gating.
#[must_use]
pub fn describedby_region(
    control: AccessNode,
    region_tag: impl Into<String>,
    region_role: AriaRole,
    region_name: Option<String>,
    present: bool,
) -> Vec<AccessNode> {
    if !present {
        return vec![control];
    }
    let region_tag = region_tag.into();
    let control = control.with_described_by(region_tag.clone());
    let mut region = AccessNode::new(region_tag, region_role);
    if let Some(name) = region_name {
        region = region.with_name(name);
    }
    vec![control, region]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control() -> AccessNode {
        AccessNode::new("anchor", AriaRole::Button)
    }

    #[test]
    fn present_links_control_and_emits_named_region() {
        let nodes = describedby_region(
            control(),
            "badge",
            AriaRole::Status,
            Some("3 unread".to_string()),
            true,
        );
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].tag, "anchor");
        assert_eq!(nodes[0].described_by.as_deref(), Some("badge"));
        assert_eq!(nodes[1].tag, "badge");
        assert_eq!(nodes[1].role, AriaRole::Status);
        assert_eq!(nodes[1].name.as_deref(), Some("3 unread"));
    }

    #[test]
    fn present_without_name_emits_a_nameless_region() {
        let nodes = describedby_region(control(), "tip", AriaRole::Tooltip, None, true);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[1].role, AriaRole::Tooltip);
        assert!(nodes[1].name.is_none(), "a nameless region carries no name");
    }

    #[test]
    fn absent_drops_the_link_and_the_region_node() {
        let nodes = describedby_region(
            control(),
            "badge",
            AriaRole::Status,
            Some("x".to_string()),
            false,
        );
        assert_eq!(nodes.len(), 1, "no region node when absent");
        assert_eq!(nodes[0].tag, "anchor");
        assert!(
            nodes[0].described_by.is_none(),
            "no dangling describedby to an absent region",
        );
    }
}
