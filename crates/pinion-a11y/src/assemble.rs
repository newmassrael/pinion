//! R984 §5.40 §2 #7 — the backend-agnostic accessibility-tree assembler.
//!
//! [`build_access_tree`] is the SSOT every embedder runs to turn a binding's
//! [`WidgetA11y`](crate::WidgetA11y) projection into the enriched node list a
//! screen reader (the live `AccessKit` emit) and an AI client (the
//! `scene/access` RPC dump) both consume — the two MUST agree, so the assembly
//! lives here once rather than being re-derived in each shell. The GUI shell
//! (`pinion_shell`) was the first consumer (R979); the TUI shell (`pinion_tui`)
//! is the second (R984), so the shared steps lift here at the 2nd consumer
//! (divergence-is-a-bug) instead of being copied per backend.
//!
//! Two steps are backend-agnostic and live here:
//!   1. run the binding's node + focus projection in the reactive owner scope,
//!      enriching accessible names from the paint scene, and stamping the
//!      per-node focus flag from the focus target ([`build_access_tree`]);
//!   2. resolve each node's pixel [`bounds`](AccessNode::bounds) from a
//!      caller-supplied tag -> rect resolver ([`resolve_access_bounds`]).
//!
//! The rect resolver is a closure (not a hard call into a layout engine)
//! because the lookup (`pinion_runtime::rect_for_tag`) lives in a sibling crate
//! this one does not depend on — each shell passes its own, keeping the layering
//! acyclic while sharing the union-bounds policy.

use pinion_core::scene::Rect;
use pinion_core::{Owner, Scene};

use crate::{AccessFocus, AccessNode, enrich_names_from_scene};

/// Assemble the enriched accessibility node list + AT focus target.
///
/// Runs `node_fn` (the binding's `access_node` / `access_node_for_window`) and
/// `focus_fn` (its `access_focus_target`) inside `owner`'s reactive scope, then
/// fills each node's accessible name from `paint_scene` text where present
/// (a never-painted window passes `None`, leaving names unresolved). Pixel
/// bounds are a separate, backend-specific step — see [`resolve_access_bounds`]
/// — because the rect lookup lives above this crate in the layering.
///
/// The two closures (rather than a `WidgetA11y` bound) let a multi-window GUI
/// shell pass `access_node_for_window` and a single-window TUI shell pass
/// `access_node` through the same assembler.
///
/// R1518 — the per-node focus flag is stamped here (`stamp_focus_flag`) from
/// the focus target, so the tree cannot state focus two ways.
pub fn build_access_tree(
    owner: &Owner,
    paint_scene: Option<&Scene>,
    node_fn: impl FnOnce() -> Vec<AccessNode>,
    focus_fn: impl FnOnce() -> Option<AccessFocus>,
) -> (Vec<AccessNode>, Option<AccessFocus>) {
    let mut nodes = owner.run(node_fn);
    if let Some(paint) = paint_scene {
        // R1551 §5.40 §5.36 — the document outline, BEFORE the name pass so a
        // heading appended here is named by the same rule every other node is.
        crate::attach_block_headings(&mut nodes, paint);
        // R1559 §5.40 §5.36 — the document's list structure, from the same
        // painted tree by the same kind of pass, and AFTER the heading pass so
        // a heading that is also an item keeps the stronger role. Also before
        // the name pass, so an item appended here is named by the rule every
        // other node is named by.
        crate::attach_block_lists(&mut nodes, paint);
        // R1560 §5.40 §5.36 — the document's table structure, from the same
        // painted tree by the same kind of pass. After the list pass so a cell
        // that is also a list item keeps both facts, and before the name pass
        // so a cell appended here is named from its painted contents by the
        // rule every other node is named by.
        crate::attach_block_tables(&mut nodes, paint);
        enrich_names_from_scene(&mut nodes, paint);
        // R1543 §5.40 §5.39 — the mnemonic announcement, derived from the same
        // painted tree by the same kind of pass. It rides the assembler for the
        // R1518 reason `stamp_focus_flag` does: a fact every binding would
        // otherwise spell for itself is spelled once, at the chokepoint both
        // the live AT emit and the `scene/access` dump pass through, so the
        // two cannot describe different trees.
        crate::enrich_access_keys_from_scene(&mut nodes, paint);
        // R1554 §5.40 §5.39 — `aria-disabled` for every node inside a region a
        // container declared disabled. Same chokepoint and same argument as
        // `stamp_focus_flag`: a fact the scene already states, spelled once
        // where the live AT emit and the `scene/access` dump both pass, so the
        // two cannot describe different trees — and so a binding cannot leave a
        // control inside a disabled group announced as actionable.
        stamp_inherited_disabled(&mut nodes, paint);
    }
    let focus = owner.run(focus_fn);
    stamp_focus_flag(&mut nodes, focus.as_ref());
    (nodes, focus)
}

/// R1554 §5.40 §5.39 — set [`AccessState::disabled`](crate::AccessState::disabled)
/// on every node the §5.39 disabled cascade resolved as disabled.
///
/// **Widening, not replacing** — the opposite of [`stamp_focus_flag`], and for a reason each
/// way round. Focus has one bearer, so a claim the target does not name is
/// wrong and is cleared. Disabledness has two independent sources: the scene's
/// regions (this pass) and the widget's own state enum, which a binding
/// projects through `AccessState::from_interaction` and which the scene knows nothing about. So the two
/// are OR-ed. That is also the toolkit's rule — a `WA_ForceDisabled` child stays disabled
/// when its parent is re-enabled — and it is the only combination that cannot
/// announce an inert control as live: neither source can *deny* the other.
fn stamp_inherited_disabled(nodes: &mut [AccessNode], paint: &Scene) {
    let census = pinion_core::scene_disabled::disabled_census(paint);
    if census.is_empty() {
        return;
    }
    for node in nodes {
        if census.iter().any(|d| d.tag == node.tag) {
            node.state.disabled = true;
        }
    }
}

/// R1518 §5.40 §2 #7 — set [`AccessState::focused`](crate::AccessState::focused)
/// on the one node the AT reports as focused, and clear it everywhere else.
///
/// The bearer is the focused composite's `active_descendant` when it names one
/// (the `aria-activedescendant` roving-cursor model) and otherwise the
/// [`focus_tag`](AccessFocus::focus_tag) itself; no focus target means no node
/// carries the flag.
///
/// **Why the assembler owns this.** The flag never reaches a screen reader —
/// `lower_access_node` carries `checked` / `mixed` / `disabled` to AccessKit and
/// the AT learns focus solely from [`AccessFocus`], which the shell's focus
/// manager sources. The flag exists on the `scene/access` wire, where it is the
/// AI client's spelling of that same fact (§2 #7). Two spellings of one fact are
/// not two sources: deriving it from the target here means a binding cannot
/// claim focus the shell did not grant, drop one it did, or place it on a
/// different node than the AT was told about.
///
/// Measured across 188 examples before it was written (940 observations: boot
/// plus four `focus/next` stops each), 144 disagreed with the target the same
/// tree published — 130 a focused element whose own node stayed silent, 11 a
/// roving cell flagged for the AI while the AT was told only the container
/// (`hello-data-grid`'s current cell was unannounceable), and 3 a flag with no
/// focus anywhere (`hello-tree-view` at boot). R1517 removed that class in the
/// five bindings a demo walked; a binding outside the walk could still spell it
/// any way it liked, which is what those 144 are.
fn stamp_focus_flag(nodes: &mut [AccessNode], focus: Option<&AccessFocus>) {
    let bearer = focus.map(|f| {
        f.active_descendant
            .as_deref()
            .unwrap_or(f.focus_tag.as_str())
    });
    for node in nodes {
        node.state.focused = bearer == Some(node.tag.as_str());
    }
}

/// Resolve each node's pixel [`bounds`](AccessNode::bounds) from `resolver`, a
/// tag -> rect lookup the caller supplies (the GUI / TUI shells pass
/// `|tag| pinion_runtime::rect_for_tag(paint, tag)`). A node carrying
/// [`bounds_union_tags`](AccessNode::bounds_union_tags) takes the union of its
/// own rect with each fragment's (the frozen-split / tree-grid multi-fragment
/// row, R863). A tag the resolver cannot place leaves `bounds` `None`, the
/// "never painted" honest answer.
pub fn resolve_access_bounds(nodes: &mut [AccessNode], resolver: impl Fn(&str) -> Option<Rect>) {
    for node in nodes {
        let mut bounds = resolver(&node.tag);
        for extra in &node.bounds_union_tags {
            if let Some(rect) = resolver(extra) {
                bounds = Some(bounds.map_or(rect, |b| b.union(rect)));
            }
        }
        node.bounds = bounds;
    }
}

#[cfg(test)]
mod tests {
    use super::{build_access_tree, resolve_access_bounds};
    use crate::{AccessFocus, AccessNode, AriaRole};
    use pinion_core::Owner;
    use pinion_core::scene::Rect;

    #[test]
    fn build_runs_closures_and_returns_nodes_and_focus() {
        let owner = Owner::new();
        let (nodes, focus) = build_access_tree(
            &owner,
            None,
            || vec![AccessNode::new("a", AriaRole::Button).with_name("Save")],
            || None,
        );
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].tag, "a");
        assert_eq!(nodes[0].name.as_deref(), Some("Save"));
        assert!(focus.is_none());
    }

    #[test]
    fn build_enriches_an_unnamed_node_from_the_paint_scene() {
        // R984.1 — covers `build_access_tree`'s enrich branch (the prior test
        // passed `None` paint, so name enrichment was never exercised — the H1
        // gap on the shared SSOT every backend, including the TUI, runs).
        use pinion_core::Scene;
        use pinion_core::scene::{ContainerNode, Rect, TextNode};
        let paint = Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::new(
                "Save",
                Rect::new(0, 0, 40, 16),
            ))])
            .with_tag("btn".to_owned()),
        );
        let owner = Owner::new();
        let (nodes, focus) = build_access_tree(
            &owner,
            Some(&paint),
            || vec![AccessNode::new("btn", AriaRole::Button)],
            || None,
        );
        assert_eq!(
            nodes[0].name.as_deref(),
            Some("Save"),
            "an unnamed node's name is enriched from the paint scene's text",
        );
        assert!(focus.is_none());
    }

    #[test]
    fn resolve_fills_bounds_from_the_resolver_and_unions_fragments() {
        let mut nodes = vec![
            AccessNode::new("solo", AriaRole::Button),
            AccessNode::new("row", AriaRole::Row).with_bounds_union_tag("frag"),
            AccessNode::new("absent", AriaRole::Button),
        ];
        let rects = |tag: &str| match tag {
            "solo" => Some(Rect::new(0, 0, 10, 4)),
            "row" => Some(Rect::new(0, 0, 8, 4)),
            "frag" => Some(Rect::new(8, 0, 8, 4)),
            _ => None,
        };
        resolve_access_bounds(&mut nodes, rects);
        assert_eq!(
            nodes[0].bounds,
            Some(Rect::new(0, 0, 10, 4)),
            "solo node takes its own rect"
        );
        assert_eq!(
            nodes[1].bounds,
            Some(Rect::new(0, 0, 16, 4)),
            "a multi-fragment row unions its own rect with the fragment's",
        );
        assert_eq!(
            nodes[2].bounds, None,
            "a tag the resolver cannot place stays unresolved"
        );
    }

    // ----- R1518 §5.40 §2 #7 — the focus flag is stamped, not claimed -----

    /// The three tags every stamping test addresses: a composite container, the
    /// child its roving cursor sits on, and an unrelated sibling.
    fn trio() -> Vec<AccessNode> {
        vec![
            AccessNode::new("grid", AriaRole::Grid),
            AccessNode::new("grid#0_0", AriaRole::GridCell),
            AccessNode::new("sibling", AriaRole::Button),
        ]
    }

    fn stamped(focus: Option<AccessFocus>) -> Vec<(String, bool)> {
        let owner = Owner::new();
        let (nodes, _) = build_access_tree(&owner, None, trio, || focus);
        nodes
            .into_iter()
            .map(|n| (n.tag, n.state.focused))
            .collect()
    }

    #[test]
    fn r1518_an_atomic_target_flags_the_focused_element() {
        assert_eq!(
            stamped(Some(AccessFocus::atomic("sibling"))),
            vec![
                ("grid".to_owned(), false),
                ("grid#0_0".to_owned(), false),
                ("sibling".to_owned(), true),
            ],
            "the focused element carries the flag even though it never set it — \
             the 130 observations that had a focus target no node echoed",
        );
    }

    #[test]
    fn r1518_a_composite_target_flags_the_active_descendant_not_the_container() {
        assert_eq!(
            stamped(Some(AccessFocus::composite("grid", "grid#0_0"))),
            vec![
                ("grid".to_owned(), false),
                ("grid#0_0".to_owned(), true),
                ("sibling".to_owned(), false),
            ],
            "the AT reports the addressed child as focused, so the wire does too",
        );
    }

    #[test]
    fn r1518_no_focus_target_leaves_every_node_unflagged() {
        // `hello-tree-view` at boot claimed `file_tree#src` with `focus/get`
        // answering `None`. Nothing the binding does can reach that state now.
        let owner = Owner::new();
        let (nodes, _) = build_access_tree(
            &owner,
            None,
            || vec![AccessNode::new("row", AriaRole::TreeItem).with_focused(true)],
            || None,
        );
        assert!(
            !nodes[0].state.focused,
            "a claim with no focus anywhere is cleared",
        );
    }

    #[test]
    fn r1518_a_claim_the_target_does_not_name_is_cleared() {
        let owner = Owner::new();
        let (nodes, _) = build_access_tree(
            &owner,
            None,
            || {
                vec![
                    AccessNode::new("grid", AriaRole::Grid),
                    AccessNode::new("grid#9_9", AriaRole::GridCell).with_focused(true),
                ]
            },
            || Some(AccessFocus::composite("grid", "grid#0_0")),
        );
        assert_eq!(
            nodes.iter().filter(|n| n.state.focused).count(),
            0,
            "the named descendant is absent from the tree, so nobody bears the \
             flag — a stale cell cannot inherit it",
        );
    }

    // ----- R1554 §5.40 §5.39 — aria-disabled from the scene's regions -----

    /// A paint scene with `outer` live and `region` declared disabled, holding
    /// `inner`.
    fn disabled_scene() -> pinion_core::Scene {
        use pinion_core::Scene;
        use pinion_core::scene::ContainerNode;
        use pinion_core::style::LayoutStyle;
        Scene::Container(ContainerNode::new(vec![
            Scene::Container(ContainerNode::new(vec![]).with_tag("outer")),
            Scene::Container(
                ContainerNode::new(vec![Scene::Container(
                    ContainerNode::new(vec![]).with_tag("inner"),
                )])
                .with_tag("region")
                .with_layout(LayoutStyle::new().with_disabled(true)),
            ),
        ]))
    }

    /// The posture a widget projects for itself through
    /// `AccessState::from_interaction` — the source the scene knows nothing
    /// about, and so the one the stamp must not deny.
    fn self_disabled() -> crate::AccessState {
        crate::AccessState {
            disabled: true,
            ..crate::AccessState::default()
        }
    }

    fn disabled_flags(nodes: Vec<AccessNode>) -> Vec<(String, bool)> {
        let owner = Owner::new();
        let scene = disabled_scene();
        let (nodes, _) = build_access_tree(&owner, Some(&scene), || nodes, || None);
        nodes
            .into_iter()
            .map(|n| (n.tag, n.state.disabled))
            .collect()
    }

    #[test]
    fn r1554_every_node_in_a_declared_region_is_announced_disabled() {
        assert_eq!(
            disabled_flags(vec![
                AccessNode::new("outer", AriaRole::Button),
                AccessNode::new("region", AriaRole::Group),
                AccessNode::new("inner", AriaRole::Button),
            ]),
            vec![
                ("outer".to_owned(), false),
                ("region".to_owned(), true),
                ("inner".to_owned(), true),
            ],
            "the declarer and its member, neither of which said so itself — a \
             binding cannot leave a control inside a disabled group announced \
             as actionable",
        );
    }

    #[test]
    fn r1554_the_stamp_widens_and_never_denies() {
        // The opposite of `stamp_focus_flag`, deliberately. Disabledness has two independent
        // sources — the scene's regions and the widget's own state enum, which
        // the scene knows nothing about — so they are OR-ed. The toolkit's
        // rule too: a `WA_ForceDisabled` child stays disabled when its parent is re-enabled.
        assert_eq!(
            disabled_flags(vec![
                AccessNode::new("outer", AriaRole::Button).with_state(self_disabled()),
                AccessNode::new("inner", AriaRole::Button),
            ]),
            vec![("outer".to_owned(), true), ("inner".to_owned(), true)],
            "`outer` is outside every region and keeps its own claim; `inner` \
             gains one it never made",
        );
    }

    #[test]
    fn r1554_a_tree_with_no_regions_leaves_every_claim_alone() {
        use pinion_core::Scene;
        use pinion_core::scene::ContainerNode;
        let owner = Owner::new();
        let scene = Scene::Container(ContainerNode::new(vec![]));
        let (nodes, _) = build_access_tree(
            &owner,
            Some(&scene),
            || vec![AccessNode::new("a", AriaRole::Button).with_state(self_disabled())],
            || None,
        );
        assert!(
            nodes[0].state.disabled,
            "the early return must not clear what the binding projected",
        );
    }

    #[test]
    fn r1518_the_stamp_is_the_only_source_whatever_the_binding_said() {
        // Every node claims focus; the target names one. The stamp is not a
        // filter over what the binding proposed — it replaces it.
        let owner = Owner::new();
        let (nodes, _) = build_access_tree(
            &owner,
            None,
            || trio().into_iter().map(|n| n.with_focused(true)).collect(),
            || Some(AccessFocus::atomic("grid")),
        );
        let flagged: Vec<&str> = nodes
            .iter()
            .filter(|n| n.state.focused)
            .map(|n| n.tag.as_str())
            .collect();
        assert_eq!(flagged, vec!["grid"], "at most one, and it is the target's");
    }
}
