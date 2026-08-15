//! `scene/conform` — whether the announced tree is one a reader can walk
//! (R1693 §5.12 §5.40 §2 #7).
//!
//! # The question the other two surfaces cannot ask
//!
//! `scene/access` publishes the tree and `scene/voice` classifies the painted
//! regions against it. Both are about one node at a time, and neither can see a
//! node that is perfect on its own terms and structurally a lie: a pane
//! announcing `role = table`, with a name a reader can hear and a rectangle they
//! can land on, that holds no row. An assistive technology says "table" and
//! there is nothing to move into.
//!
//! Measured the day this landed, across the 216 surfaces that answer: **16 carry
//! a violation** — 17 collections owning nothing and 45 members outside the
//! collection their role requires. Two of them were the reference analysis
//! tool's screen B, whose 186 painted regions reached the tree as a `table` with
//! no row and a `tree` with no item.
//!
//! # Against the reference toolkit at 6.11 (built and run, not read)
//!
//! Its item views derive the tree from the model, which is the stronger position
//! for a populated view: a 16x7 view answered **137 nodes** with a table
//! interface reporting `rowCount = 16` and `cellAt(3, 4)` naming the cell. What
//! it cannot do is the two cases either side of that:
//!
//! - the same view with an emptied model answers `role = Table`,
//!   `rowCount = 0` and **no diagnostic** — indistinguishable from a table its
//!   author never filled;
//! - a **custom-painted** pane of 72 cells answers **one** node, empty-named,
//!   with no children. Everything painted rather than modelled is simply absent.
//!
//! There is no conformance notion there to violate, so a malformed tree is not
//! a thing that can be reported.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "judged": 154,
//!     "counts": { "empty": 0, "stray": 0 },
//!     "nodes": [
//!       { "tag": "pv.list", "role": "table", "fault": "empty",
//!         "required": ["row"], "found": null }
//!     ]
//!   }
//! }
//! ```
//!
//! Request — no parameters; the method runs the **same** access-tree producer
//! `scene/access` and `scene/voice` run, so the three surfaces cannot disagree
//! about the tree they are describing.
//!
//! `judged` is how many nodes carried a requirement at all, published because a
//! census with nothing to judge is green and a reader has to be able to tell
//! "this tree is well-formed" from "this tree has no collections in it". Like
//! every other figure on these surfaces it is **derived on each call** rather
//! than stored.

use pinion_a11y::{AccessNode, AriaRole, StructureFault, StructureNode, structure_census};
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One announced node that does not hold its end of the structural relation.
#[derive(Debug, Clone, Serialize)]
pub struct ConformEntry {
    /// The paint tag — the same spelling `scene/access`, `scene/voice` and
    /// `scene/click` address, so a row can be acted on directly.
    pub tag: String,
    /// The role that carries the requirement, as its WAI-ARIA literal.
    pub role: &'static str,
    /// `empty` — it owns none of what its role promises and does not declare
    /// itself empty — or `stray` — it is not inside a role its own role
    /// requires.
    pub fault: &'static str,
    /// What was required: the owned roles for `empty`, the context roles for
    /// `stray`. Carried so a row is actionable without re-deriving the relation.
    pub required: Vec<&'static str>,
    /// For `stray`, the role it is actually inside, or `null` when nothing owns
    /// it. Always `null` for `empty`.
    pub found: Option<&'static str>,
}

/// How many rows carry each arm. Derived on every call from the rows.
#[derive(Debug, Clone, Serialize)]
pub struct ConformCounts {
    /// Collections that own none of what their role promises.
    pub empty: usize,
    /// Members outside the collection their role requires.
    pub stray: usize,
}

/// Response payload for `scene/conform`.
#[derive(Debug, Clone, Serialize)]
pub struct ConformOutcome {
    /// How many announced nodes carried a structural requirement — the
    /// denominator, so a green answer over a tree with no collections cannot be
    /// mistaken for a green answer over a screen full of them.
    pub judged: usize,
    /// The partition.
    pub counts: ConformCounts,
    /// Every failing row, in the order the producer emitted the nodes.
    pub nodes: Vec<ConformEntry>,
}

/// Build the `scene/conform` response from the announced tree.
///
/// # Errors
///
/// Only if the outcome fails to serialize, which for owned strings and counts is
/// unreachable in practice; it is surfaced rather than unwrapped so an RPC
/// handler never panics the shell.
pub fn handle_scene_conform(access_nodes: &[AccessNode]) -> Result<Value, RpcError> {
    let census = structure_census(access_nodes);
    let outcome = ConformOutcome {
        judged: census.judged,
        counts: ConformCounts {
            empty: census.count(StructureFault::Empty),
            stray: census.count(StructureFault::Stray),
        },
        nodes: census.nodes.iter().map(entry).collect(),
    };
    serde_json::to_value(outcome).map_err(RpcError::internal_error)
}

/// One census row, as the wire says it.
fn entry(node: &StructureNode) -> ConformEntry {
    ConformEntry {
        tag: node.tag.clone(),
        role: node.role.aria_name(),
        fault: node.fault.name(),
        required: node.required.iter().map(|r| r.aria_name()).collect(),
        found: node.found.map(AriaRole::aria_name),
    }
}

#[cfg(test)]
mod tests {
    use super::handle_scene_conform;
    use pinion_a11y::{AccessNode, AriaRole, StructureFault};

    fn node(tag: &str, role: AriaRole) -> AccessNode {
        AccessNode::new(tag.to_owned(), role).with_name(format!("the {tag}"))
    }

    fn row<'a>(value: &'a serde_json::Value, tag: &str) -> &'a serde_json::Value {
        value["nodes"]
            .as_array()
            .expect("array")
            .iter()
            .find(|r| r["tag"] == tag)
            .unwrap_or_else(|| panic!("no row for {tag}"))
    }

    #[test]
    fn a_binding_with_no_tree_judges_nothing_and_is_sound() {
        let value = handle_scene_conform(&[]).expect("ok");
        assert_eq!(value["judged"], 0);
        assert_eq!(value["counts"]["empty"], 0);
        assert_eq!(value["counts"]["stray"], 0);
        assert_eq!(value["nodes"].as_array().expect("array").len(), 0);
    }

    /// ★★★★★ R1693 — the whole claim, on the wire: the exact shape screen B was
    /// in, and the row says what was required rather than leaving a reader to
    /// look the relation up.
    #[test]
    fn the_wire_reports_a_collection_that_holds_nothing() {
        let value = handle_scene_conform(&[node("pv.list", AriaRole::Table)]).expect("ok");
        let list = row(&value, "pv.list");
        assert_eq!(list["fault"], "empty");
        assert_eq!(list["role"], "table");
        assert_eq!(list["required"][0], "row");
        assert!(list["found"].is_null());
        assert_eq!(value["counts"]["empty"], 1);
        assert_eq!(value["judged"], 1);
    }

    /// The mirror, and the column that makes it repairable: a cell says what it
    /// is actually inside.
    #[test]
    fn the_wire_reports_a_member_outside_its_collection() {
        let nodes = [
            node("grid", AriaRole::Grid).with_child("grid.cell"),
            node("grid.cell", AriaRole::GridCell),
        ];
        let value = handle_scene_conform(&nodes).expect("ok");
        let cell = row(&value, "grid.cell");
        assert_eq!(cell["fault"], "stray");
        assert_eq!(cell["required"][0], "row");
        assert_eq!(cell["found"], "grid");
        assert_eq!(value["counts"]["stray"], 1);
    }

    /// ★★★★ R1693 — an empty collection is a real state and says so, and the
    /// wire reports the two cases differently. The floor answers `rowCount = 0`
    /// for both.
    #[test]
    fn the_wire_separates_a_forgotten_collection_from_an_empty_one() {
        let forgotten = handle_scene_conform(&[node("results", AriaRole::Grid)]).expect("ok");
        assert_eq!(forgotten["counts"]["empty"], 1);

        let declared =
            handle_scene_conform(&[node("results", AriaRole::Grid).with_row_count(0)]).expect("ok");
        assert_eq!(declared["counts"]["empty"], 0);
        assert_eq!(
            declared["judged"], 1,
            "and it is still judged — the exemption is a declaration, not a skip",
        );
    }

    /// A well-formed table publishes nothing but its denominator.
    #[test]
    fn a_sound_tree_publishes_what_it_judged() {
        let nodes = [
            node("t", AriaRole::Table).with_child("t.r0"),
            node("t.r0", AriaRole::Row).with_child("t.r0.c0"),
            node("t.r0.c0", AriaRole::GridCell),
        ];
        let value = handle_scene_conform(&nodes).expect("ok");
        assert_eq!(value["nodes"].as_array().expect("array").len(), 0);
        assert_eq!(value["judged"], 4);
    }

    /// Every arm the wire publishes is one its own reader accepts (R1616).
    #[test]
    fn every_published_arm_is_a_readable_one() {
        let nodes = [node("t", AriaRole::Table), node("loose", AriaRole::Tab)];
        let value = handle_scene_conform(&nodes).expect("ok");
        for entry in value["nodes"].as_array().expect("array") {
            let published = entry["fault"].as_str().expect("a string");
            assert!(
                StructureFault::from_name(published).is_some(),
                "the wire published {published:?}, which its own reader rejects",
            );
            let role = entry["role"].as_str().expect("a string");
            assert!(
                AriaRole::ALL.iter().any(|r| r.aria_name() == role),
                "the wire published role {role:?}, which is not in the vocabulary",
            );
        }
        assert_eq!(value["counts"]["empty"], 1);
        assert_eq!(value["counts"]["stray"], 1);
    }
}
