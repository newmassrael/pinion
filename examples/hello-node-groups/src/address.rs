//! ★★★★★ R2174 §5.2 §5.11 — **where a painted mark's address comes from, for
//! the node-groups screen.**
//!
//! ⚠ The header lives HERE rather than on the `mod address;` line: a module
//! carrying both an outer doc at its declaration and an inner `//!` header has
//! the two MERGED and resolved in the DECLARING file's scope, so every link to
//! this module's own items stops resolving. Measured at R2121's push gate, one
//! screen over.
//!
//! # What this screen had, and what it did not
//!
//! It had ONE name for its prefix — a `VIEW_TAG` const — and composed every
//! address inline from it: `format!("{VIEW_TAG}.frame.{}", node.id.0)` and nine
//! more. That is better than ten spelled prefixes and it is not a declaration:
//! nothing states which families exist, nothing states what a member's address
//! is, and **a walk cannot call any of it**.
//!
//! Measured at R2174 the cost was ten walk sites across three walks, every one
//! of them typing `nodegroups.node.…` / `.frame.…` / `.wire.…` out. The census
//! could not even see the Rust half — `"{VIEW_TAG}.frame.{}"` opens with a
//! brace, so its needle reads no address there at all, and the family showed as
//! walk-only with nothing pinning it.
//!
//! # The two questions a composer does not answer
//!
//! R2158 named them and they apply here: a composer says *what is this one
//! thing's address*, and a reader also needs *what is the prefix of this
//! family* (to count or classify the marks under it) and *which member is this
//! tag* (to recover an id from paint). The seats below answer the second; the
//! screen publishes both halves so a walk can ask.

/// The tag every mark of this screen hangs off.
pub const VIEW: &str = "nodegroups";

/// The prefix a node's own marks are addressed under.
pub const NODE_SEAT: &str = "nodegroups.node.";

/// The prefix a frame's marks are addressed under.
pub const FRAME_SEAT: &str = "nodegroups.frame.";

/// The prefix a wire's marks are addressed under.
pub const WIRE_SEAT: &str = "nodegroups.wire.";

/// The breadcrumb naming where in the tree the view is.
pub const BREADCRUMB: &str = "nodegroups.breadcrumb";

/// The one line saying what the last edit did, or refused to do.
pub const STATUS: &str = "nodegroups.status";

/// A node's box.
#[must_use]
pub fn node(id: impl std::fmt::Display) -> String {
    format!("{NODE_SEAT}{id}")
}

/// A node's title text.
#[must_use]
pub fn node_title(id: impl std::fmt::Display) -> String {
    format!("{VIEW}.title.{id}")
}

/// A frame's box — a node that holds other nodes.
#[must_use]
pub fn frame(id: impl std::fmt::Display) -> String {
    format!("{FRAME_SEAT}{id}")
}

/// A frame's own caption.
#[must_use]
pub fn frame_title(id: impl std::fmt::Display) -> String {
    format!("{FRAME_SEAT}{id}.title")
}

/// One pin of a node, on the side `side` (`"in"` / `"out"`) at `port`.
#[must_use]
pub fn pin(id: impl std::fmt::Display, side: &str, port: impl std::fmt::Display) -> String {
    format!("{VIEW}.pin.{id}.{side}.{port}")
}

/// The label drawn beside that pin.
#[must_use]
pub fn pin_label(id: impl std::fmt::Display, side: &str, port: impl std::fmt::Display) -> String {
    format!("{VIEW}.pinlabel.{id}.{side}.{port}")
}

/// A wire between two pins.
#[must_use]
pub fn wire(id: impl std::fmt::Display) -> String {
    format!("{WIRE_SEAT}{id}")
}

/// The pass-through a frame draws for a route that crosses it.
#[must_use]
pub fn through(id: impl std::fmt::Display, output: impl std::fmt::Display) -> String {
    format!("{VIEW}.through.{id}.{output}")
}

/// The node a tag names, or `None` when the tag is not a node's box.
///
/// ★ The inverse R2158 found missing one screen over: a composer answers "what
/// is this node's address" and cannot answer "which node is this tag", so every
/// reader that wanted the second wrote `strip_prefix` by hand.
#[must_use]
pub fn node_id(tag: &str) -> Option<&str> {
    let rest = tag.strip_prefix(NODE_SEAT)?;
    (!rest.contains('.')).then_some(rest)
}

/// Every address this screen declares, for a reader that cannot call the
/// declaration — see the screen's `spec` query.
#[must_use]
pub fn declared() -> serde_json::Value {
    serde_json::json!({
        "view": VIEW,
        "node_seat": NODE_SEAT,
        "frame_seat": FRAME_SEAT,
        "wire_seat": WIRE_SEAT,
        "breadcrumb": BREADCRUMB,
        "status": STATUS,
    })
}
