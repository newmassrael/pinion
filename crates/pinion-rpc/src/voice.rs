//! `scene/voice` — which painted regions a reader is told about, and why the
//! silent ones are silent (R1691 §5.12 §5.40 §2 #7).
//!
//! # Against the reference toolkit at 6.11
//!
//! It has the tree and not the question. Its accessibility layer builds an
//! interface for every widget automatically, so *everything* is in the tree and
//! nothing says which entries carry a name anybody can use. Built and run at
//! 6.11.1: a window of six children answered 7 nodes, **4 of them with an empty
//! accessible name** — a button whose name the author forgot, a decorative rule,
//! a custom painted region, and the window itself. The defect and the three
//! correct silences are the same answer there.
//!
//! Three things this answers that no accessor there can:
//!
//! - **Which regions were forgotten.** `unvoiced` is a painted, addressable
//!   region that nobody gave a voice and nobody declared quiet. There, the same
//!   region is a node with an empty name, indistinguishable from ornament.
//! - **Why a region is quiet.** Every silence carries a kind and a detail, and a
//!   [`relay`](pinion_core::voice::Relay) derived from the kind saying where a
//!   reader receives the information instead. The floor's widget has three
//!   author-settable accessibility slots and none of them is a reason —
//!   measured: clearing all three changed the tree by nothing.
//! - **Whether a name has a region.** `ghost` is the other direction: a node
//!   announced for a tag nothing paints and nobody refers to, so a reader can be
//!   sent somewhere that is not there.
//!
//! And one more the census only has because it asks both questions at once:
//! `dangling` — a region declared quiet *because another node says its name*,
//! where that other node does not speak either. It reads as handled and is a
//! hole, which is why it is not folded into `silent`.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "total": 166,
//!     "counts": { "announced": 115, "silent": 51, "unvoiced": 0,
//!                 "ghost": 0, "dangling": 0 },
//!     "nodes": [
//!       { "tag": "lab.toolbar.run", "voice": "announced" },
//!       { "tag": "lab.toolbar.run.label", "voice": "silent",
//!         "reason": "name_of", "detail": "lab.toolbar.run",
//!         "relay": "peer", "self_declared": true, "declared_by": null }
//!     ]
//!   }
//! }
//! ```
//!
//! Request — no parameters; the method reads the last painted scene and the
//! same access-tree producer `scene/access` runs, so the two surfaces cannot
//! disagree about what a reader is told.
//!
//! Rows are in paint order, and a declaring node precedes the members it
//! covers (a declaration is an ancestor and the walk is depth-first); any
//! `ghost` rows follow, since they have no place in paint order.
//!
//! `total` is the painted, addressable population, so `counts` sums to it plus
//! the ghosts. Both are **derived from the rows on every call** rather than
//! stored: a coverage figure that is written down is one that stops falling
//! when the thing it measures does.
//!
//! (The numbers above are the reference analysis tool's first screen as R1691
//! left it, read off the running application — not an illustration. Its census
//! was `35` announced of the same `166` when the round began.)

use std::collections::BTreeSet;

use pinion_a11y::{AccessNode, referenced_tags};
use pinion_core::Scene;
use pinion_core::voice::{Silence, Voice, VoiceCensus, VoiceNode, voice_census};
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One addressable region of the painted scene, and what a reader is told.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceEntry {
    /// The paint tag — the same spelling `scene/click`, `scene/invoke` and
    /// `scene/access` address, so a row can be acted on directly.
    pub tag: String,
    /// `announced`, `silent`, `unvoiced`, `ghost` or `dangling`. Three of the
    /// five are defects, and they are three *different* defects.
    pub voice: &'static str,
    /// The class of the silence that reached this node — `decorative`,
    /// `layout`, `name_of`, `part_of` — or `null` when none did.
    pub reason: Option<&'static str>,
    /// What [`reason`](Self::reason) points at: what the region draws, what it
    /// arranges, or the tag it hands a reader to.
    pub detail: Option<String>,
    /// Where a reader receives the information instead — `nowhere`,
    /// `children`, `peer`, `ancestor`.
    ///
    /// **Derived** from the reason, never declared, so a screen and an agent
    /// cannot disagree about it.
    pub relay: Option<&'static str>,
    /// The node carries its own declaration rather than inheriting one.
    pub self_declared: bool,
    /// The nearest ancestor whose declaration covers this node, or `null`.
    /// The column that says which one region to re-voice.
    pub declared_by: Option<String>,
}

/// How many rows carry each arm. Derived on every call from the rows.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceCounts {
    /// Painted, addressable, and the tree has a node for it.
    pub announced: usize,
    /// Painted, no node, and the scene says why.
    pub silent: usize,
    /// Painted, no node, and nobody decided. **The defect this exists for.**
    pub unvoiced: usize,
    /// Announced for a tag nothing paints and nobody refers to.
    pub ghost: usize,
    /// Quiet by a reason that hands a reader to a node which does not speak.
    pub dangling: usize,
}

/// Response payload for `scene/voice`.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceOutcome {
    /// The painted, addressable population — the denominator of every figure
    /// above, published so a reader never has to re-derive it from the rows.
    pub total: usize,
    /// The partition.
    pub counts: VoiceCounts,
    /// Every row, painted ones in paint order and ghosts after.
    pub nodes: Vec<VoiceEntry>,
}

/// Build the `scene/voice` response from the last painted scene and the access
/// tree built beside it.
///
/// # Errors
///
/// Only if the outcome fails to serialize, which for owned strings, counts and
/// bools is unreachable in practice; it is surfaced rather than unwrapped so an
/// RPC handler never panics the shell.
pub fn handle_scene_voice(
    last_paint_scene: Option<&Scene>,
    access_nodes: &[AccessNode],
) -> Result<Value, RpcError> {
    let announced: BTreeSet<String> = access_nodes.iter().map(|n| n.tag.clone()).collect();
    let referenced = referenced_tags(access_nodes);
    let census = last_paint_scene.map_or_else(VoiceCensus::default, |scene| {
        voice_census(scene, &announced, &referenced)
    });
    let outcome = VoiceOutcome {
        total: census
            .nodes
            .iter()
            .filter(|n| n.voice != Voice::Ghost)
            .count(),
        counts: VoiceCounts {
            announced: census.count(Voice::Announced),
            silent: census.count(Voice::Silent),
            unvoiced: census.count(Voice::Unvoiced),
            ghost: census.count(Voice::Ghost),
            dangling: census.count(Voice::Dangling),
        },
        nodes: census.nodes.iter().map(entry).collect(),
    };
    serde_json::to_value(outcome).map_err(RpcError::internal_error)
}

/// One census row, as the wire says it.
fn entry(node: &VoiceNode) -> VoiceEntry {
    VoiceEntry {
        tag: node.tag.clone(),
        voice: node.voice.name(),
        reason: node.silence.as_ref().map(|s| s.kind().name()),
        detail: node.silence.as_ref().map(|s| s.detail().to_owned()),
        relay: node.silence.as_ref().map(|s| Silence::relay(s).name()),
        self_declared: node.self_declared,
        declared_by: node.declared_by.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::handle_scene_voice;
    use pinion_a11y::{AccessNode, AriaRole};
    use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
    use pinion_core::style::LayoutStyle;
    use pinion_core::voice::Silence;

    fn text(tag: &'static str) -> Scene {
        Scene::Text(TextNode::new(tag, Rect::default()).with_tag(tag))
    }

    fn quiet(tag: &'static str, silence: Silence) -> Scene {
        Scene::Container(
            ContainerNode::new(vec![])
                .with_tag(tag)
                .with_layout(LayoutStyle::new().with_silence(silence)),
        )
    }

    fn node(tag: &str) -> AccessNode {
        AccessNode::new(tag.to_owned(), AriaRole::Button).with_name(tag.to_owned())
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
    fn an_unpainted_binding_answers_an_empty_census() {
        let value = handle_scene_voice(None, &[]).expect("ok");
        assert_eq!(value["total"], 0);
        assert_eq!(value["counts"]["unvoiced"], 0);
        assert_eq!(value["nodes"].as_array().expect("array").len(), 0);
    }

    /// The whole claim, on the wire: a forgotten control and a decorative rule
    /// are the same answer on the floor and two different arms here.
    #[test]
    fn the_wire_separates_a_forgotten_control_from_ornament() {
        let scene = Scene::Container(ContainerNode::new(vec![
            text("named"),
            text("forgotten"),
            quiet("rule", Silence::decorative("a separator")),
        ]));
        let value = handle_scene_voice(Some(&scene), &[node("named")]).expect("ok");
        assert_eq!(value["total"], 3);
        assert_eq!(value["counts"]["announced"], 1);
        assert_eq!(value["counts"]["silent"], 1);
        assert_eq!(value["counts"]["unvoiced"], 1);

        assert_eq!(row(&value, "forgotten")["voice"], "unvoiced");
        assert!(row(&value, "forgotten")["reason"].is_null());

        let rule = row(&value, "rule");
        assert_eq!(rule["voice"], "silent");
        assert_eq!(rule["reason"], "decorative");
        assert_eq!(rule["detail"], "a separator");
        assert_eq!(rule["relay"], "nowhere");
        assert_eq!(rule["self_declared"], true);
    }

    /// The reason is published, and so is where it sends a reader — a column
    /// derived rather than declared, so the two cannot disagree.
    #[test]
    fn a_redirect_publishes_its_target_and_where_it_leads() {
        let scene = Scene::Container(ContainerNode::new(vec![
            text("run"),
            quiet("run.label", Silence::name_of("run")),
        ]));
        let value = handle_scene_voice(Some(&scene), &[node("run")]).expect("ok");
        let label = row(&value, "run.label");
        assert_eq!(label["voice"], "silent");
        assert_eq!(label["reason"], "name_of");
        assert_eq!(label["detail"], "run");
        assert_eq!(label["relay"], "peer");
    }

    /// A redirect to a node that does not speak reads as handled and is a hole.
    #[test]
    fn a_redirect_to_a_silent_node_is_its_own_arm_on_the_wire() {
        let scene = Scene::Container(ContainerNode::new(vec![
            text("run"),
            quiet("run.label", Silence::name_of("run")),
        ]));
        let value = handle_scene_voice(Some(&scene), &[]).expect("ok");
        assert_eq!(row(&value, "run.label")["voice"], "dangling");
        assert_eq!(value["counts"]["dangling"], 1);
        // And it is NOT counted as silent, which is what makes it visible.
        assert_eq!(value["counts"]["silent"], 0);
    }

    /// The other direction, and the exemption that keeps it honest: the form
    /// painter's description regions are announced with no rectangle of their
    /// own, and the control that points at them is what makes them reachable.
    #[test]
    fn an_announced_tag_with_no_region_is_a_ghost_unless_something_points_at_it() {
        let scene = Scene::Container(ContainerNode::new(vec![text("control")]));

        let value = handle_scene_voice(Some(&scene), &[node("control"), node("said")]).expect("ok");
        assert_eq!(row(&value, "said")["voice"], "ghost");
        assert_eq!(value["counts"]["ghost"], 1);
        assert_eq!(
            value["total"], 1,
            "a ghost is not part of the painted total"
        );

        let described = node("control").with_described_by("said".to_owned());
        let value = handle_scene_voice(Some(&scene), &[described, node("said")]).expect("ok");
        assert_eq!(value["counts"]["ghost"], 0);
        assert_eq!(value["nodes"].as_array().expect("array").len(), 1);
    }

    /// Every arm the wire publishes is one its own reader accepts — checked on
    /// the wire shape rather than on the enum alone (R1616).
    #[test]
    fn every_published_arm_is_a_readable_one() {
        use pinion_core::voice::{Relay, SilenceKind, Voice};

        for kind in SilenceKind::ALL {
            let scene = quiet("one", Silence::new(kind, "target"));
            // `name_of` / `part_of` name a node; give it a voice so the row is
            // `silent` rather than `dangling` and the reason column is the one
            // under test either way.
            let value = handle_scene_voice(Some(&scene), &[node("target")]).expect("ok");
            let row = row(&value, "one");
            let published = row["reason"].as_str().expect("a string");
            assert_eq!(
                SilenceKind::from_name(published),
                Some(kind),
                "the wire published {published:?}, which its own reader rejects",
            );
            let relay = row["relay"].as_str().expect("a string");
            assert_eq!(
                Relay::from_name(relay),
                Some(kind.relay()),
                "the wire published relay {relay:?}, which its own reader rejects",
            );
            let voice = row["voice"].as_str().expect("a string");
            assert_eq!(Voice::from_name(voice), Some(Voice::Silent));
        }
    }

    /// A region covers its members, and the row names the one declaration to
    /// revisit — the column that makes a hundred silent descendants one
    /// decision rather than a hundred.
    #[test]
    fn a_member_names_the_ancestor_that_quieted_it() {
        let legend = Scene::Container(
            ContainerNode::new(vec![text("swatch.a"), text("swatch.b")])
                .with_tag("legend")
                .with_layout(LayoutStyle::new().with_silence(Silence::decorative("swatches"))),
        );
        let value = handle_scene_voice(Some(&legend), &[]).expect("ok");
        assert_eq!(value["counts"]["silent"], 3);
        assert_eq!(value["counts"]["unvoiced"], 0);
        let inner = row(&value, "swatch.a");
        assert_eq!(inner["self_declared"], false);
        assert_eq!(inner["declared_by"], "legend");
        assert_eq!(inner["reason"], "decorative");
    }
}
