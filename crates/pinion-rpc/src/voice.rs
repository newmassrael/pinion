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
//! # What a node says, not that it exists (R1692)
//!
//! `mumbled` is the arm the floor cannot have: the tree has a node and what it
//! announces is unusable — nothing, an address, or a symbol. The same probe is
//! why the arm needs the *rest* of this surface to mean anything. Run against
//! 6.11.1, those three rules flag **8 of 9 nodes** on a six-region window, of
//! which one is a defect and four are ornament and structure. There, a name has
//! no owedness to be judged against; here the `silent` declarations supply it,
//! so a `mumbled` row is a defect and not a suspicion.
//!
//! `hollow` completes the other half: `layout` is a promise that the children
//! speak, and it is the one promise checkable only from below.
//!
//! # What arrives, not that it arrived (R2002)
//!
//! `misquoted` is the arm `dangling` leaves room for. A caption declaring
//! `name_of` says *my ink is that node's NAME*, and until this round the only
//! thing anybody checked was that the redirect arrived somewhere that speaks.
//! Whether what arrives is what was painted is WAI-ARIA's **label-in-name** — a
//! speech-input user says the visible label out loud and a sighted helper reads
//! it to somebody who cannot — and it is a comparison between some INK and some
//! NAME, so only a reader holding the scene can make it. Measured the round the
//! census learned to: the reference analysis tool had **four** such regions, of
//! which one screen's walk could see one.
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
//!                 "ghost": 0, "dangling": 0, "mumbled": 0, "hollow": 0,
//!                 "misquoted": 0 },
//!     "nodes": [
//!       { "tag": "lab.toolbar.run", "voice": "announced", "name": "Run",
//!         "fault": null },
//!       { "tag": "lab.toolbar.run.label", "voice": "silent", "name": null,
//!         "fault": null, "reason": "name_of", "detail": "lab.toolbar.run",
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
//! the ghosts. Ghosts carry a `name` and never a `fault`: what is wrong with a
//! ghost is that nothing is behind it, and judging its name too would report one
//! defect as two. Both are **derived from the rows on every call** rather than
//! stored: a coverage figure that is written down is one that stops falling
//! when the thing it measures does.
//!
//! (The numbers above are the reference analysis tool's first screen as R1691
//! left it, read off the running application — not an illustration. Its census
//! was `35` announced of the same `166` when the round began.)

use pinion_a11y::{AccessNode, announcements, referenced_tags};
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
    /// `announced`, `silent`, `unvoiced`, `ghost`, `dangling`, `mumbled`,
    /// `hollow` or `misquoted`. Six of the eight are defects, and they are six
    /// *different* defects.
    pub voice: &'static str,
    /// What a reader actually hears, or `null` when the tree has no node for
    /// this tag.
    pub name: Option<String>,
    /// Why [`name`](Self::name) is not usable — `absent`, `address` or
    /// `wordless` — and `null` when it is. Non-null exactly on a `mumbled` row.
    pub fault: Option<&'static str>,
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
    /// Announced for a tag nothing paints, nobody refers to and nothing
    /// composes.
    pub ghost: usize,
    /// Quiet by a reason that hands a reader to a node which does not speak.
    pub dangling: usize,
    /// In the tree, and what it says is not usable. **A node is not a voice.**
    pub mumbled: usize,
    /// Quiet by a reason that promises its children speak, over a subtree where
    /// nothing does.
    pub hollow: usize,
    /// Quiet by a reason that lends its ink out as another node's name, where
    /// that node speaks and says something else. **WAI-ARIA's label-in-name.**
    pub misquoted: usize,
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
    let announced = announcements(access_nodes);
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
            mumbled: census.count(Voice::Mumbled),
            hollow: census.count(Voice::Hollow),
            misquoted: census.count(Voice::Misquoted),
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
        name: node.name.clone(),
        fault: node.fault.map(pinion_core::voice::NameFault::name),
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
    use std::collections::BTreeSet;

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

    /// A node that speaks: named after what it is, never after where it is —
    /// the tag would be an `address` fault the moment the tag is structured.
    fn node(tag: &str) -> AccessNode {
        AccessNode::new(tag.to_owned(), AriaRole::Button).with_name(format!("the {tag}"))
    }

    fn row<'a>(value: &'a serde_json::Value, tag: &str) -> &'a serde_json::Value {
        value["nodes"]
            .as_array()
            .expect("array")
            .iter()
            .find(|r| r["tag"] == tag)
            .unwrap_or_else(|| panic!("no row for {tag}"))
    }

    /// ★★★★★ R2002 — [`VoiceCounts`] is a SECOND spelling of `Voice::ALL`, and
    /// a hand-kept copy of a derived list is the thing this project keeps
    /// finding rotted. The partition is published as one field per arm, so an
    /// arm added to the enum with no field here would be counted nowhere and
    /// the census would go on summing to a smaller total in silence.
    ///
    /// ★ The comparison is against the DERIVED vocabulary rather than a list
    /// written here, and it is an equality in both directions: a field with no
    /// arm behind it is a column that can never be anything but zero, which
    /// reads to a client as a checked-and-clear answer.
    ///
    /// ⚠ This is what the existing `sum(counts) == total` gate cannot do. That
    /// one only parts company with the truth on a screen that HAS a row on the
    /// forgotten arm — so on a clean tree it passes while blind, which is the
    /// shape of a gate that stops working without saying so.
    #[test]
    fn every_arm_of_the_census_has_a_column_in_the_published_partition() {
        let value = handle_scene_voice(None, &[]).expect("ok");
        let published: BTreeSet<&str> = value["counts"]
            .as_object()
            .expect("counts is an object")
            .keys()
            .map(String::as_str)
            .collect();
        let arms: BTreeSet<&str> = pinion_core::voice::VOICE_WIRE_NAMES
            .iter()
            .copied()
            .collect();
        assert_eq!(
            published, arms,
            "the published partition and the arms it partitions have to be the same set",
        );
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

    /// ★★★★★ R1692 — the arm the floor cannot have, on the wire: a node whose
    /// name says nothing is not an announced region, and the row says which of
    /// the three ways it failed and what it actually said.
    #[test]
    fn the_wire_separates_a_node_from_a_voice() {
        let scene = Scene::Container(ContainerNode::new(vec![
            text("named"),
            text("mute"),
            text("panel.status"),
            text("close"),
        ]));
        let nodes = [
            node("named"),
            AccessNode::new("mute".to_owned(), AriaRole::Button),
            AccessNode::new("panel.status".to_owned(), AriaRole::Status)
                .with_name("panel.status".to_owned()),
            AccessNode::new("close".to_owned(), AriaRole::Button).with_name("×".to_owned()),
        ];
        let value = handle_scene_voice(Some(&scene), &nodes).expect("ok");
        assert_eq!(value["counts"]["announced"], 1);
        assert_eq!(value["counts"]["mumbled"], 3);

        assert_eq!(row(&value, "mute")["fault"], "absent");
        assert!(
            row(&value, "mute")["name"]
                .as_str()
                .unwrap_or("x")
                .is_empty(),
            "a node with no name at all still reports what it said",
        );
        assert_eq!(row(&value, "panel.status")["fault"], "address");
        assert_eq!(row(&value, "close")["fault"], "wordless");
        assert_eq!(row(&value, "close")["name"], "×");
        assert!(row(&value, "named")["fault"].is_null());
    }

    /// ★★★★★ R1692 — `layout` promises the children speak, and this is the
    /// promise nothing could check: every node under the box is correctly
    /// quiet, and the region is inaudible whole.
    #[test]
    fn the_wire_reports_a_box_whose_children_do_not_speak() {
        let body = Scene::Container(
            ContainerNode::new(vec![text("swatch.a"), text("swatch.b")])
                .with_tag("body")
                .with_layout(LayoutStyle::new().with_silence(Silence::layout("stacks the rail"))),
        );
        let value = handle_scene_voice(Some(&body), &[]).expect("ok");
        assert_eq!(row(&value, "body")["voice"], "hollow");
        assert_eq!(value["counts"]["hollow"], 1);
        assert_eq!(
            value["counts"]["unvoiced"], 2,
            "the members are unvoiced on their own terms — which is why the \
             box's false promise needs its own arm",
        );

        // One member in the tree keeps the promise.
        let value = handle_scene_voice(Some(&body), &[node("swatch.a")]).expect("ok");
        assert_eq!(row(&value, "body")["voice"], "silent");
        assert_eq!(value["counts"]["hollow"], 0);
    }

    /// ★★★★ R1692 — a composite container is announced for a tag the scene
    /// never paints, and what it holds is what anchors it. Measured when this
    /// landed, eight of the tree's nine `ghost` rows were exactly this and none
    /// was a defect.
    #[test]
    fn a_container_announced_over_painted_members_is_not_a_ghost() {
        let scene = Scene::Container(ContainerNode::new(vec![text("legend_0"), text("legend_1")]));
        let legend = AccessNode::new("chart_legend".to_owned(), AriaRole::Group)
            .with_name("Series".to_owned())
            .with_child("legend_0")
            .with_child("legend_1");
        let value = handle_scene_voice(Some(&scene), &[legend, node("legend_0"), node("legend_1")])
            .expect("ok");
        assert_eq!(value["counts"]["ghost"], 0);

        // And a status region that composes nothing is still a name a reader
        // can be sent to and never find.
        let status = AccessNode::new("search_status".to_owned(), AriaRole::Status)
            .with_name("19 properties".to_owned());
        let value = handle_scene_voice(Some(&scene), &[status, node("legend_0"), node("legend_1")])
            .expect("ok");
        assert_eq!(row(&value, "search_status")["voice"], "ghost");
        assert!(
            row(&value, "search_status")["fault"].is_null(),
            "one defect is reported once",
        );
    }

    /// Every arm the wire publishes is one its own reader accepts — checked on
    /// the wire shape rather than on the enum alone (R1616).
    #[test]
    fn every_published_arm_is_a_readable_one() {
        use pinion_core::voice::{Relay, SilenceKind, Voice};

        for kind in SilenceKind::ALL {
            let scene = Scene::Container(
                ContainerNode::new(vec![text("target")])
                    .with_tag("one")
                    .with_layout(LayoutStyle::new().with_silence(Silence::new(kind, "target"))),
            );
            // `name_of` / `part_of` name a node; give it a voice so the row is
            // `silent` rather than `dangling`, and it is inside the box so
            // `layout`'s own promise is kept too. The reason column is the one
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
