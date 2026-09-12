//! ★★★★★ R2176 §5.2 §5.38 — **where a painted mark's address comes from, for
//! the live topology view.**
//!
//! ⚠ The header lives HERE rather than on the `mod address;` line: a module
//! carrying both an outer doc at its declaration and an inner `//!` header has
//! the two MERGED and resolved in the DECLARING file's scope, so every link to
//! this module's own items stops resolving. Measured at R2121's push gate.
//!
//! # What this screen had, and what it did not
//!
//! Three `format!` calls spelling `topology.` out, in two different places: the
//! card painter composed a node's and a label's address, and `view()` composed
//! a wire's. Nothing said which families exist, and a walk driving the screen
//! could only type them out — four sites in `r1442_live_topology.py` did.
//!
//! # ★ The wire is why this matters more than the count says
//!
//! A wire's address is not `<seat><key>`: its key is **two service names joined
//! by a hyphen**, a composition rule with an end that has to be chosen. That
//! rule lived at one call site in `view()` and was reproduced by hand in the
//! walk (`topology.wire.gw-eu-warehouse`), where nothing compared the two.
//!
//! ⚠⚠ **And the screen's own test could not compare them either.**
//! `wire_scene` takes its tag as an ARGUMENT, so the composition happens at the
//! call site and the unit test hands the function a literal of its own
//! (`"topology.wire.a-b"`) and asserts it comes back. Measured at R2176 that
//! made `topology.wire` the one family in twelve whose `assertion` pin pinned
//! **nothing**: renaming the composition in `view()` would have failed no test.
//! Moving the rule here is what makes it assertable — [`wire`] is now compared
//! against a literal directly, which is the strongest pin shape in this tree.
//!
//! ⇒ the general form of that finding, which this round does NOT fix: a
//! painter that receives its tag rather than composing it puts the composition
//! beyond its own unit test's reach, and the census cannot see the difference
//! between an assertion that LOOKS a tag up and one that SUPPLIED it.

/// The tag every mark of this screen hangs off.
pub const VIEW: &str = "topology";

/// The prefix a service's card is addressed under.
pub const NODE_SEAT: &str = "topology.node.";

/// The prefix a service's caption is addressed under.
pub const LABEL_SEAT: &str = "topology.label.";

/// The prefix a dependency's wire is addressed under.
pub const WIRE_SEAT: &str = "topology.wire.";

/// A wire's whole address, as a TEMPLATE a reader that cannot call [`wire`]
/// can fill.
///
/// ★★★★★ R2176 — a seat is not enough here. A wire's key is two names JOINED,
/// and publishing the seat alone would leave the hyphen to be chosen again by
/// whoever composes — which is exactly what the walk was doing. The template
/// carries the join, so the rule has one home on both sides of the wire.
///
/// ⚠ Held to [`wire`] by this screen's own test, so the two cannot drift: a
/// template that is published and never compared is a second spelling wearing
/// a declaration's clothes.
pub const WIRE_TEMPLATE: &str = "topology.wire.{from}-{to}";

/// The card drawn for the service called `name`.
#[must_use]
pub fn node(name: &str) -> String {
    format!("{NODE_SEAT}{name}")
}

/// The caption drawn on that card.
#[must_use]
pub fn label(name: &str) -> String {
    format!("{LABEL_SEAT}{name}")
}

/// The wire drawn for the dependency that runs `from` -> `to`.
///
/// ★ The join is the rule this module exists for: a wire is addressed by BOTH
/// ends, and until R2176 the hyphen was chosen in `view()` and re-chosen by
/// hand in a walk.
///
/// ⚠ The join is not invertible, and that is a property of the data rather than
/// a shortcoming here: a service name may itself contain a hyphen (`gw-eu`
/// does), so `topology.wire.gw-eu-warehouse` has three readings and only the
/// topology knows which. There is deliberately no `wire_ends`; a reader that
/// wants the ends composes the address it is looking for instead.
#[must_use]
pub fn wire(from: &str, to: &str) -> String {
    format!("{WIRE_SEAT}{from}-{to}")
}

// ⚠ R2176 — **no inverse here, deliberately.** R2158's third question ("which
// service is this tag") has no Rust reader on this screen: the reader that
// wants it is the walk, and its door is `rpc_verify.seat_member`, shared by
// every screen. Writing one anyway is what R2174 did one screen over, and the
// same round that found it dead wrote this note — see that module's header.
// A composer with no production consumer is not a declaration.

/// Every address this screen declares, for a reader that cannot call the
/// declaration — see the screen's `spec` query.
#[must_use]
pub fn declared() -> serde_json::Value {
    serde_json::json!({
        "view": VIEW,
        "node_seat": NODE_SEAT,
        "label_seat": LABEL_SEAT,
        "wire_seat": WIRE_SEAT,
        "wire_template": WIRE_TEMPLATE,
    })
}
