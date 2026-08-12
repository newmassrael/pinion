//! `scene/disabled` — which controls are inert, and **which ancestor made them
//! so** (R1554 §5.12 §5.39 §2 #2 §2 #7).
//!
//! # Against the toolkit 6.11
//!
//! The toolkit has the property and not the question. `isEnabled()` answers a bool about
//! one widget. `isEnabledTo(ancestor)` answers a bool about an ancestor the caller has already
//! picked — so it can confirm a guess, never produce one. `testAttribute(WA_ForceDisabled)` separates
//! "disabled itself" from "disabled by a parent" but names no parent. Nothing
//! in the toolkit's public API answers *which* ancestor greyed a control: that
//! is a `parentWidget()` walk in a debugger, which is exactly the position an external
//! driver cannot get into.
//!
//! Three things this answers that the toolkit's surface cannot:
//!
//! - **The cause, by name.** Every row carries `declared_by`. An agent that
//!   finds a button unresponsive learns the tag to click instead, in the same
//!   round trip.
//! - **The set, enumerated.** the toolkit has no way to ask a window "what is disabled
//!   right now"; the answer exists only as a bool per widget, and a driver
//!   would have to already know every widget to poll them all.
//! - **Whether the ink followed.** A disabled GL widget keeps drawing
//!   whatever it draws, and the toolkit says nothing about that. `ink` states it, so an
//!   agent comparing a screenshot against "this is disabled" knows in advance
//!   which regions will look unchanged.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "disabled": [
//!       { "tag": "detail_rows", "self_declared": true,
//!         "declared_by": null, "ink": "faded" },
//!       { "tag": "detail_rows#threshold", "self_declared": false,
//!         "declared_by": "detail_rows", "ink": "faded" }
//!     ]
//!   }
//! }
//! ```
//!
//! Request — no parameters; the method reads the last painted scene, so the set
//! it reports is the set the pointer router and the Tab order are refusing.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/disabled", "id": 1 }
//! ```
//!
//! Rows are in paint order, and the declaring node precedes the members it
//! disabled (a declaration is an ancestor, and the walk is depth-first).
//!
//! A binding that has not painted, or one with nothing disabled, answers with
//! an empty list rather than an error — "everything is live" is the true answer
//! in both cases.
//!
//! The grouping by declaring ancestor is deliberately **not** also published:
//! it is a `group_by` over `declared_by`, and one fact published two ways is
//! two things that can disagree.

use pinion_core::Scene;
use pinion_core::scene_disabled::disabled_census;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// One disabled node in the painted scene.
#[derive(Debug, Clone, Serialize)]
pub struct DisabledEntry {
    /// The paint tag that is disabled — the same spelling `scene/click`,
    /// `scene/invoke` and `focus/set` address, so an agent can feed it straight
    /// back. Untagged disabled nodes are absent: they cannot be addressed, so a
    /// row for one would name nothing.
    pub tag: String,
    /// The node carries its own declaration (the toolkit `WA_ForceDisabled`). With `declared_by` also
    /// set, re-enabling the region leaves this one disabled.
    pub self_declared: bool,
    /// The nearest ancestor that declared the region, or `null` when the node's
    /// own declaration is the only one. **The column the toolkit has no
    /// accessor for.**
    pub declared_by: Option<String>,
    /// What the disabled cascade did to this node's ink: `"faded"`, or
    /// `"opaque_content"` for a node whose pixels the framework does not author
    /// (an image, an `External` surface, an immediate-mode driver, a terminal
    /// grid). Inert and announced disabled either way.
    pub ink: &'static str,
    /// (R1668) **Why** — the class of the declaration that reached this node:
    /// `"precondition"`, `"permission"`, `"busy"`, `"reserved"`,
    /// `"unsupported"`, or `"unstated"` when the declaration named no reason.
    ///
    /// The node's own class when `self_declared`, otherwise the region's. The
    /// floor this is measured against has no such column on any of its four
    /// disable surfaces, and its accessibility layer carries one bit.
    pub reason: &'static str,
    /// (R1668) The specific thing [`reason`](Self::reason) points at — the
    /// condition, the authority, the holder, the release, the missing
    /// capability. Empty when the declaration named no reason.
    pub detail: String,
    /// (R1668) What a person can do about it — `"satisfy"`, `"authorize"`,
    /// `"wait"`, `"await_release"` or `"nothing"`.
    ///
    /// **Derived** from [`reason`](Self::reason), never declared, so an agent
    /// and a screen cannot disagree about it. Published rather than left to the
    /// reader because the mapping is the framework's answer, not a convention
    /// each consumer re-invents.
    pub recourse: &'static str,
}

/// Response payload for `scene/disabled`.
#[derive(Debug, Clone, Serialize)]
pub struct DisabledOutcome {
    /// Every disabled, addressable node in the painted scene, in paint order.
    pub disabled: Vec<DisabledEntry>,
}

/// Build the `scene/disabled` response from the last painted scene.
///
/// # Errors
///
/// Only if the outcome fails to serialize, which for a `Vec` of owned strings
/// and bools is unreachable in practice; it is surfaced rather than unwrapped
/// so an RPC handler never panics the shell.
pub fn handle_scene_disabled(last_paint_scene: Option<&Scene>) -> Result<Value, RpcError> {
    let disabled = last_paint_scene
        .map(disabled_census)
        .unwrap_or_default()
        .into_iter()
        .map(|d| DisabledEntry {
            tag: d.tag,
            self_declared: d.self_declared,
            declared_by: d.declared_by,
            ink: d.ink.name(),
            reason: d.reason.kind().name(),
            detail: d.reason.detail().to_owned(),
            recourse: d.reason.recourse().name(),
        })
        .collect();
    serde_json::to_value(DisabledOutcome { disabled }).map_err(RpcError::internal_error)
}

#[cfg(test)]
mod tests {
    use super::handle_scene_disabled;
    use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
    use pinion_core::style::LayoutStyle;

    fn leaf(tag: &'static str) -> Scene {
        Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::new(tag, Rect::default()))])
                .with_tag(tag),
        )
    }

    fn region(tag: &'static str, children: Vec<Scene>) -> Scene {
        Scene::Container(
            ContainerNode::new(children)
                .with_tag(tag)
                .with_layout(LayoutStyle::new().with_disabled(true)),
        )
    }

    #[test]
    fn an_unpainted_binding_answers_with_an_empty_list() {
        let value = handle_scene_disabled(None).expect("ok");
        assert_eq!(value["disabled"].as_array().expect("array").len(), 0);
    }

    #[test]
    fn a_live_tree_answers_with_an_empty_list() {
        let scene = Scene::Container(ContainerNode::new(vec![leaf("a"), leaf("b")]));
        let value = handle_scene_disabled(Some(&scene)).expect("ok");
        assert_eq!(value["disabled"].as_array().expect("array").len(), 0);
    }

    #[test]
    fn a_member_names_the_ancestor_that_disabled_it() {
        // The column the toolkit has no accessor for.
        let scene = Scene::Container(ContainerNode::new(vec![
            leaf("live"),
            region("group", vec![leaf("inner")]),
        ]));
        let value = handle_scene_disabled(Some(&scene)).expect("ok");
        let rows = value["disabled"].as_array().expect("array");
        assert_eq!(rows.len(), 2, "the declarer and its one member");
        assert_eq!(rows[0]["tag"], "group");
        assert_eq!(rows[0]["self_declared"], true);
        assert!(rows[0]["declared_by"].is_null(), "nothing above it");
        assert_eq!(rows[1]["tag"], "inner");
        assert_eq!(rows[1]["self_declared"], false);
        assert_eq!(rows[1]["declared_by"], "group");
    }

    /// R1668 — the wire says why, and derives what to do about it.
    ///
    /// The two arms that matter most are the ones the floor cannot tell apart:
    /// a widget reserved for a release that has not shipped, and one this build
    /// will never have. Both are inert; only one will stop being.
    #[test]
    fn r1668_the_wire_separates_reserved_from_unsupported() {
        use pinion_core::availability::Unavailable;

        fn booked(tag: &'static str, reason: Unavailable) -> Scene {
            Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag(tag)
                    .with_layout(LayoutStyle::new().with_unavailable(reason)),
            )
        }

        let scene = Scene::Container(ContainerNode::new(vec![
            booked("later", Unavailable::reserved("requirement 16")),
            booked("never", Unavailable::unsupported("no such device here")),
            region("quiet", vec![]),
        ]));
        let value = handle_scene_disabled(Some(&scene)).expect("ok");
        let rows = value["disabled"].as_array().expect("array");
        assert_eq!(rows.len(), 3);

        assert_eq!(rows[0]["tag"], "later");
        assert_eq!(rows[0]["reason"], "reserved");
        assert_eq!(rows[0]["detail"], "requirement 16");
        assert_eq!(rows[0]["recourse"], "await_release");

        assert_eq!(rows[1]["tag"], "never");
        assert_eq!(rows[1]["reason"], "unsupported");
        assert_eq!(rows[1]["recourse"], "nothing");

        // Same bool, same ink, same accessibility bit -- and the wire keeps
        // them apart, which is the whole claim.
        assert_eq!(rows[0]["ink"], rows[1]["ink"]);
        assert_ne!(rows[0]["reason"], rows[1]["reason"]);
        assert_ne!(rows[0]["recourse"], rows[1]["recourse"]);

        // And a declaration that names no reason says exactly that rather than
        // going missing.
        assert_eq!(rows[2]["tag"], "quiet");
        assert_eq!(rows[2]["reason"], "unstated");
        assert_eq!(rows[2]["detail"], "");
        assert_eq!(rows[2]["recourse"], "nothing");
    }

    /// R1668 — every member of the published vocabulary is one the reader
    /// accepts, checked on the wire shape rather than on the enum alone.
    #[test]
    fn r1668_every_published_reason_is_a_readable_one() {
        use pinion_core::availability::{Recourse, Unavailable, UnavailableKind};

        for kind in UnavailableKind::ALL {
            let scene = Scene::Container(
                ContainerNode::new(vec![])
                    .with_tag("one")
                    .with_layout(LayoutStyle::new().with_unavailable(Unavailable::new(kind, "d"))),
            );
            let value = handle_scene_disabled(Some(&scene)).expect("ok");
            let row = &value["disabled"][0];
            let published = row["reason"].as_str().expect("a string");
            assert_eq!(
                UnavailableKind::from_name(published),
                Some(kind),
                "the wire published {published:?}, which its own reader rejects",
            );
            let recourse = row["recourse"].as_str().expect("a string");
            assert_eq!(
                Recourse::from_name(recourse),
                Some(kind.recourse()),
                "the wire published recourse {recourse:?}, which its own reader rejects",
            );
        }
    }

    #[test]
    fn a_self_disabled_node_inside_a_region_reports_both() {
        // The toolkit's `WA_ForceDisabled` case: re-enabling `group` must leave `inner` disabled, and
        // the wire is what tells an agent so BEFORE it tries.
        let scene = region("group", vec![region("inner", vec![])]);
        let value = handle_scene_disabled(Some(&scene)).expect("ok");
        let rows = value["disabled"].as_array().expect("array");
        assert_eq!(rows[1]["tag"], "inner");
        assert_eq!(rows[1]["self_declared"], true);
        assert_eq!(rows[1]["declared_by"], "group");
    }

    #[test]
    fn the_nearest_declaring_ancestor_wins_not_the_outermost() {
        let scene = region("outer", vec![region("middle", vec![leaf("deep")])]);
        let value = handle_scene_disabled(Some(&scene)).expect("ok");
        let rows = value["disabled"].as_array().expect("array");
        let deep = rows.iter().find(|r| r["tag"] == "deep").expect("present");
        assert_eq!(
            deep["declared_by"], "middle",
            "the row names the ancestor to re-enable FIRST, which is the near one",
        );
    }

    #[test]
    fn a_node_the_cascade_cannot_repaint_says_so() {
        use pinion_core::scene::ImageNode;
        let img =
            Scene::Image(ImageNode::new("logo.png".to_owned(), Rect::default()).with_tag("logo"));
        let scene = region("group", vec![img]);
        let value = handle_scene_disabled(Some(&scene)).expect("ok");
        let rows = value["disabled"].as_array().expect("array");
        assert_eq!(rows[0]["ink"], "faded", "the declaring container's own box");
        assert_eq!(
            rows[1]["ink"], "opaque_content",
            "an image's pixels are not the framework's to fade — the toolkit cannot \
             grey a GL widget either, and says nothing about it",
        );
    }
}
