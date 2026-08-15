//! R1693 §5.40 §2 #7 — **whether the announced tree is a tree a reader can walk**.
//!
//! ## The question nothing could ask
//!
//! R1691 and R1692 asked whether every painted region has a voice and whether
//! that voice says anything usable. Both questions are about one node at a time.
//! Neither can see the failure this module exists for: a node that is perfect on
//! its own terms and **structurally a lie**.
//!
//! A pane announces itself `role = table`, with a name a reader can hear and a
//! rectangle they can land on. Every check passes. It holds no row. An assistive
//! technology tells somebody "table" and there is nothing to move into — the
//! sixteen messages painted inside it are not in the tree at all. The mirror
//! failure is a `gridcell` emitted beside its table rather than inside a row: the
//! cell is impeccable and cannot be placed.
//!
//! Measured on this tree the day this landed: the reference analysis tool's
//! screen B painted **186** addressable regions and announced **three** nodes,
//! two of which were exactly this — a `table` with no row and a `tree` with no
//! item.
//!
//! ## Against the reference toolkit at 6.11 (built and run, not read)
//!
//! Its item views derive the whole accessibility tree from the model, which is
//! the stronger position for a *populated* view and is the reason the question
//! cannot be asked there:
//!
//! * a 16x7 item view answered **137 nodes**, a table interface reporting
//!   `rowCount = 16`, `columnCount = 7`, and `cellAt(3, 4)` naming the cell —
//!   complete, and better than a hand-built tree that forgets a row.
//! * the same view with its model emptied answered `role = Table`,
//!   `rowCount = 0`, **and no diagnostic**. A tree with no items answered the
//!   same way. Nothing there distinguishes a collection that is empty from one
//!   whose author never filled it.
//! * a **custom-painted** pane of 72 cells answered **one** node, with an empty
//!   name and no children — which is what a byte pane is, and what screen B's
//!   third pane is.
//!
//! So the floor gives a complete tree for the shapes it owns and nothing at all
//! for the shapes it does not, and in neither case is there a verdict about the
//! tree's *form*.
//!
//! ## Emptiness is declared, not inferred
//!
//! A collection with no members is a real state — a filter that matched nothing,
//! a capture with no messages — and a rule that refused it would make that state
//! unrepresentable. So the rule is the same one R1691 applied to silence: **an
//! empty collection is well-formed when it says it is empty.** ARIA already has
//! the vocabulary, so nothing is invented: `aria-rowcount` /
//! `aria-colcount` / `aria-setsize` of zero
//! ([`AccessNode::declares_empty`](crate::AccessNode::declares_empty)).
//!
//! That is the distinction the floor cannot draw. There, `rowCount = 0` is the
//! answer for both the forgotten table and the empty one.

use std::collections::{BTreeMap, BTreeSet};

use crate::node::AccessNode;
use crate::role::AriaRole;

/// How an announced node fails WAI-ARIA's structural relation.
///
/// Two arms, because the relation has two directions and the repairs differ: an
/// empty collection wants members, a displaced member wants a parent. Folding
/// them into one "malformed" would report both and say neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StructureFault {
    /// The role promises it owns members of certain roles
    /// ([`AriaRole::required_owned`]), it owns none of them, **and it does not
    /// declare itself empty**.
    ///
    /// A reader is told a collection is there and cannot enter it.
    Empty,
    /// The role must sit inside one of certain roles
    /// ([`AriaRole::required_context`]) and does not — it is under something
    /// else, or it is at the top of the tree.
    ///
    /// A member an assistive technology cannot place. Every check about the node
    /// itself passes, which is what makes this invisible without the relation.
    Stray,
}

impl StructureFault {
    /// The lowercase wire spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            StructureFault::Empty => "empty",
            StructureFault::Stray => "stray",
        }
    }

    /// Parse a wire spelling back — so what a surface publishes is what it
    /// accepts (the R1616 symmetry rule).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| f.name() == name)
    }

    /// Every arm, in declaration order.
    pub const ALL: [StructureFault; 2] = [StructureFault::Empty, StructureFault::Stray];
}

/// Every wire spelling a [`StructureFault`] can take, derived from the arms so a
/// published vocabulary cannot lag the enum (R1616).
pub const STRUCTURE_FAULT_WIRE_NAMES: [&str; StructureFault::ALL.len()] = {
    let mut names = [""; StructureFault::ALL.len()];
    let mut i = 0;
    while i < StructureFault::ALL.len() {
        names[i] = StructureFault::ALL[i].name();
        i += 1;
    }
    names
};

/// One announced node that does not hold its end of the structural relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureNode {
    /// The tag — the same spelling `scene/access` and `scene/voice` address.
    pub tag: String,
    /// The role that carries the requirement.
    pub role: AriaRole,
    /// Which direction failed.
    pub fault: StructureFault,
    /// What was required: the owned roles for [`StructureFault::Empty`], the
    /// context roles for [`StructureFault::Stray`].
    ///
    /// Carried rather than re-derived by the reader, so a row is actionable on
    /// its own — the same posture [`VoiceNode::name`](pinion_core::voice::VoiceNode::name)
    /// takes for a name that failed.
    pub required: &'static [AriaRole],
    /// For [`StructureFault::Stray`], the role it is actually inside, or [`None`]
    /// when nothing owns it. Always [`None`] for [`StructureFault::Empty`].
    pub found: Option<AriaRole>,
}

/// Every announced node that carries a structural requirement, and which of
/// them fail it.
///
/// Counts are **derived** from the rows on demand, never stored: a coverage
/// figure written down is one that stops falling when the thing it measures does
/// (R1690).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StructureCensus {
    /// The failing rows, in the order the nodes were given.
    pub nodes: Vec<StructureNode>,
    /// How many nodes carried a requirement at all — the denominator.
    ///
    /// Published because a census with nothing to judge is green, and a reader
    /// has to be able to tell "this tree is well-formed" from "this tree has no
    /// collections in it".
    pub judged: usize,
}

impl StructureCensus {
    /// How many rows carry `fault`.
    #[must_use]
    pub fn count(&self, fault: StructureFault) -> usize {
        self.nodes.iter().filter(|n| n.fault == fault).count()
    }

    /// Whether every judged node holds its end of the relation.
    #[must_use]
    pub fn is_sound(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Check a produced accessibility tree against WAI-ARIA's structural relation.
///
/// `nodes` is the flat list the shell lowers into a subtree, and
/// [`AccessNode::children`] is what makes it a tree — so ownership here is
/// exactly the ownership an assistive technology will see, and not a second
/// model of it.
///
/// A tag listed as a child of two nodes takes the **first** parent, matching the
/// lowering: a node can only be attached once, and a census that disagreed with
/// the lowering about who owns what would be measuring a tree nobody has.
#[must_use]
pub fn structure_census(nodes: &[AccessNode]) -> StructureCensus {
    let by_tag: BTreeMap<&str, &AccessNode> = nodes.iter().map(|n| (n.tag.as_str(), n)).collect();
    let mut parent: BTreeMap<&str, &AccessNode> = BTreeMap::new();
    for node in nodes {
        for child in &node.children {
            parent.entry(child.as_str()).or_insert(node);
        }
    }

    let mut out = Vec::new();
    let mut judged = 0;
    for node in nodes {
        let owned = node.role.required_owned();
        if !owned.is_empty() {
            judged += 1;
            let holds = node.children.iter().any(|child| {
                by_tag
                    .get(child.as_str())
                    .is_some_and(|c| owned.contains(&c.role))
            });
            // ★ R1693 — `aria-busy` is the third answer. A collection that has
            // asked for its members and not received them is neither full nor
            // empty, and declaring zero would be a lie in the other direction.
            // WAI-ARIA's own required-owned rule carves out exactly this, and it
            // was found by a demo driving 95 bindings after a sweep of every
            // surface missed it — the load lands before an ordinary probe looks.
            if !holds && !node.declares_empty() && !node.busy {
                out.push(StructureNode {
                    tag: node.tag.clone(),
                    role: node.role,
                    fault: StructureFault::Empty,
                    required: owned,
                    found: None,
                });
            }
        }
        let context = node.role.required_context();
        if !context.is_empty() {
            judged += 1;
            let found = parent.get(node.tag.as_str()).map(|p| p.role);
            if !found.is_some_and(|role| context.contains(&role)) {
                out.push(StructureNode {
                    tag: node.tag.clone(),
                    role: node.role,
                    fault: StructureFault::Stray,
                    required: context,
                    found,
                });
            }
        }
    }
    StructureCensus { nodes: out, judged }
}

/// Every role that carries a structural requirement in either direction.
///
/// Derived from the two relations rather than listed, so a role that gains a
/// requirement joins this set without anybody remembering to add it — the
/// failure mode a hand-written roster has (R1687).
#[must_use]
pub fn roles_with_structure() -> BTreeSet<AriaRole> {
    AriaRole::ALL
        .iter()
        .copied()
        .filter(|r| !r.required_owned().is_empty() || !r.required_context().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::AccessNode;

    fn node(tag: &str, role: AriaRole) -> AccessNode {
        AccessNode::new(tag.to_owned(), role).with_name(format!("the {tag}"))
    }

    fn row<'a>(census: &'a StructureCensus, tag: &str) -> &'a StructureNode {
        census
            .nodes
            .iter()
            .find(|n| n.tag == tag)
            .unwrap_or_else(|| panic!("no row for {tag}"))
    }

    /// ★★★★★ R1693 — the defect this module exists for, and the exact shape the
    /// reference analysis tool's screen B was in: a pane that announces a table
    /// and holds nothing.
    #[test]
    fn a_collection_that_owns_nothing_is_empty() {
        let census = structure_census(&[node("pv.list", AriaRole::Table)]);
        assert_eq!(row(&census, "pv.list").fault, StructureFault::Empty);
        assert_eq!(row(&census, "pv.list").required, &[AriaRole::Row]);
        assert!(!census.is_sound());
        assert_eq!(census.judged, 1);
    }

    /// And the repair: one row of the required role, and the promise is kept.
    #[test]
    fn a_collection_holding_one_member_is_sound() {
        let census = structure_census(&[
            node("pv.list", AriaRole::Table).with_child("pv.list.row.0"),
            node("pv.list.row.0", AriaRole::Row).with_child("pv.list.cell.0_0"),
            node("pv.list.cell.0_0", AriaRole::GridCell),
        ]);
        assert!(census.is_sound(), "{:?}", census.nodes);
        assert_eq!(
            census.judged, 4,
            "table, row-owns, row-context, cell-context"
        );
    }

    /// ★★★★★ R1693 — an empty collection is a real state, and the rule is the
    /// one R1691 applied to silence: it has to be **declared**. The floor
    /// answers `rowCount = 0` for the forgotten table and the empty one alike;
    /// here they are two different answers.
    #[test]
    fn an_empty_collection_is_sound_when_it_says_it_is_empty() {
        let forgotten = structure_census(&[node("results", AriaRole::Grid)]);
        assert_eq!(row(&forgotten, "results").fault, StructureFault::Empty);

        let declared = structure_census(&[node("results", AriaRole::Grid).with_row_count(0)]);
        assert!(
            declared.is_sound(),
            "a grid that reports zero rows has told the reader what it holds",
        );
        assert_eq!(declared.judged, 1, "and it is still judged");
    }

    /// ★★★★ R1693 — the **third** answer. A collection that has asked for its
    /// members and not received them is neither full nor empty, and declaring
    /// zero would be a lie in the other direction: the list below has forty
    /// assets, it just has not got them yet.
    ///
    /// Found the way this round's own sweep could not find it — by a demo
    /// driving 95 bindings, because the load lands before an ordinary probe
    /// looks and the boot gate looks earlier.
    #[test]
    fn a_collection_that_is_still_arriving_is_sound() {
        let loading = structure_census(&[node("assets", AriaRole::List)
            .with_size_of_set(40)
            .with_busy()]);
        assert!(loading.is_sound(), "a busy list is waiting, not empty");
        assert_eq!(loading.judged, 1, "and it is still judged");

        // Without the declaration it is the defect it was: a list announcing
        // forty items and holding none.
        let silent = structure_census(&[node("assets", AriaRole::List).with_size_of_set(40)]);
        assert_eq!(row(&silent, "assets").fault, StructureFault::Empty);

        // ★ And busy does not excuse a *displaced* member: the exemption is
        // about what a collection holds, never about where a member sits.
        let stray = structure_census(&[
            node("grid", AriaRole::Grid).with_busy().with_child("cell"),
            node("cell", AriaRole::GridCell),
        ]);
        assert_eq!(row(&stray, "cell").fault, StructureFault::Stray);
    }

    /// A set-shaped collection declares the same thing with the set vocabulary.
    #[test]
    fn a_set_declares_its_emptiness_with_the_set_attribute() {
        let census = structure_census(&[node("filters", AriaRole::List).with_size_of_set(0)]);
        assert!(census.is_sound());

        // And a NON-zero declared extent is not a licence: a list that says it
        // has nine items and holds none is the defect, louder.
        let census = structure_census(&[node("filters", AriaRole::List).with_size_of_set(9)]);
        assert_eq!(row(&census, "filters").fault, StructureFault::Empty);
    }

    /// ★★★★ R1693 — the mirror. The cell is perfect and cannot be placed, so
    /// every per-node check passes and a reader still cannot reach it.
    #[test]
    fn a_member_outside_its_collection_is_stray() {
        let census = structure_census(&[
            node("grid", AriaRole::Grid).with_child("grid.cell"),
            node("grid.cell", AriaRole::GridCell),
        ]);
        let cell = row(&census, "grid.cell");
        assert_eq!(cell.fault, StructureFault::Stray);
        assert_eq!(cell.required, &[AriaRole::Row]);
        assert_eq!(
            cell.found,
            Some(AriaRole::Grid),
            "the row says what it is actually inside, which is the repair",
        );
        // And the grid is Empty in the same breath — it owns a cell, not a row.
        assert_eq!(row(&census, "grid").fault, StructureFault::Empty);
    }

    /// A member nothing owns at all: the commonest shape, since a flat list of
    /// nodes with no `children` references is a forest of roots.
    #[test]
    fn a_member_nothing_owns_is_stray_with_no_parent() {
        let census = structure_census(&[node("loose", AriaRole::Tab)]);
        let tab = row(&census, "loose");
        assert_eq!(tab.fault, StructureFault::Stray);
        assert_eq!(tab.found, None);
    }

    /// A `group` is an allowed intermediary for the set-shaped collections, so a
    /// tree of groups of items is well-formed — the shape a decode tree takes.
    #[test]
    fn a_group_is_a_legal_intermediary() {
        let census = structure_census(&[
            node("tree", AriaRole::Tree).with_child("tree.l1"),
            node("tree.l1", AriaRole::Group).with_child("tree.l1.sn"),
            node("tree.l1.sn", AriaRole::TreeItem),
        ]);
        assert!(census.is_sound(), "{:?}", census.nodes);
    }

    /// Nothing in the tree carries a requirement: sound, and the denominator
    /// says why. A green census with `judged == 0` is not the same claim as a
    /// green census over a screen full of tables.
    #[test]
    fn a_tree_with_no_collections_is_sound_and_judges_nothing() {
        let census =
            structure_census(&[node("save", AriaRole::Button), node("t", AriaRole::Status)]);
        assert!(census.is_sound());
        assert_eq!(census.judged, 0);
    }

    /// A tag claimed by two parents is attached once, and the census agrees with
    /// the lowering about which one — otherwise it would report on a tree
    /// nobody has.
    #[test]
    fn a_child_claimed_twice_takes_its_first_parent() {
        let census = structure_census(&[
            node("row", AriaRole::Row).with_child("cell"),
            node("elsewhere", AriaRole::Toolbar).with_child("cell"),
            node("cell", AriaRole::GridCell),
        ]);
        assert!(
            census.nodes.iter().all(|n| n.tag != "cell"),
            "the first parent is a `row`, which is what the lowering attaches",
        );
    }

    /// Both directions come from one relation, and the asymmetry is deliberate:
    /// a `radiogroup` requires radios and a lone radio is well-formed.
    #[test]
    fn the_relation_is_asymmetric_where_aria_says_it_is() {
        assert_eq!(
            AriaRole::RadioGroup.required_owned(),
            &[AriaRole::RadioButton]
        );
        assert!(AriaRole::RadioButton.required_context().is_empty());
        let census = structure_census(&[node("r", AriaRole::RadioButton)]);
        assert!(census.is_sound());
    }

    /// Every role a collection may own can legally sit inside it — the two
    /// tables have to agree or a screen could satisfy one and fail the other by
    /// construction, with no way to write a correct tree at all.
    #[test]
    fn a_role_a_collection_may_own_may_sit_inside_it() {
        for holder in AriaRole::ALL {
            for owned in holder.required_owned() {
                let context = owned.required_context();
                assert!(
                    context.is_empty() || context.contains(&holder),
                    "{} may own {} and {} may not be inside {}",
                    holder.aria_name(),
                    owned.aria_name(),
                    owned.aria_name(),
                    holder.aria_name(),
                );
            }
        }
    }

    /// ★★★ R1693 — every role the relation NAMES is in the roster.
    ///
    /// [`AriaRole::ALL`] is a hand-written list and no non-macro construction
    /// makes it self-proving, so this closes the half that matters here: a role
    /// dropped from `ALL` while still being required by some collection would
    /// make [`roles_with_structure`] silently incomplete and every sweep over
    /// `ALL` vacuous for it. The arms a collection promises are exactly the arms
    /// that must not go missing.
    #[test]
    fn every_role_the_relation_names_is_in_the_roster() {
        for role in AriaRole::ALL {
            for named in role.required_owned().iter().chain(role.required_context()) {
                assert!(
                    AriaRole::ALL.contains(named),
                    "{} names {} and the roster does not have it",
                    role.aria_name(),
                    named.aria_name(),
                );
            }
        }
    }

    /// The roster is derived, so a role that gains a requirement joins it
    /// without anybody remembering.
    #[test]
    fn the_structured_roles_are_derived_from_the_relation() {
        let roles = roles_with_structure();
        assert!(roles.contains(&AriaRole::Table));
        assert!(roles.contains(&AriaRole::GridCell));
        assert!(!roles.contains(&AriaRole::Button));
        for role in &roles {
            assert!(
                !role.required_owned().is_empty() || !role.required_context().is_empty(),
                "{} is in the roster and requires nothing",
                role.aria_name(),
            );
        }
    }

    #[test]
    fn every_published_fault_is_a_readable_one() {
        for fault in StructureFault::ALL {
            assert_eq!(StructureFault::from_name(fault.name()), Some(fault));
        }
        assert_eq!(StructureFault::from_name("nonsense"), None);
    }
}
