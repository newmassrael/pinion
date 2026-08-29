//! R1890 §5.34 §2 #2 §2 #7 — **the address at which a surface answers**,
//! composed by the same expression the transport parses.
//!
//! A `scene/query` path is `/[<scene segments>/]external/<introspect path>`
//! ([`pinion_rpc::path::split_at_external`] is what reads it). Until this
//! module the literal `/external/` existed once, as a `const` inside that
//! parser, and nothing in the workspace could *build* such a path — so a
//! publisher that wanted to tell a client where a surface lives had two
//! choices: spell the separator again, or publish a fragment and leave the
//! composition to whoever read it. Both were taken, and the second one cost a
//! round.
//!
//! # What forced it, measured
//!
//! R1889 asked the assembled analysis tool for the node lab's own introspect
//! paths and got `UnknownIntrospectPath` for every one of them, at
//! `/external/graph`. It concluded that a screen's wire surface does not
//! survive being mounted, registered that as a debt, and routed a demo's whole
//! action section through a **second process** — a standalone lab — so the verb
//! could be reached at all.
//!
//! Re-measured at R1890 against the same two binaries, the conclusion was
//! wrong in the direction that hides work:
//!
//! ```text
//! /external/graph            -> UnknownIntrospectPath
//! /node_lab/external/graph   -> "mesh-failover"
//! /node_lab/external/$schema -> 82 fields
//! ```
//!
//! The surface was there the whole time. `/external/<path>` is the *root*
//! short-circuit — in an assembled application the root surface is the
//! **host's**, so those seven refusals were true statements about the shell.
//! What was missing was never the surface; it was the **address**, which a
//! client had to assemble out of a tag published in one place and a separator
//! spelled nowhere.
//!
//! ★★★★★ The lesson this module exists to make structural: *a published
//! fragment that only becomes an address by applying a rule the publisher did
//! not publish is not a self-describing surface.* §2 #2 makes an agent the
//! primary caller, and an agent cannot be expected to know a syntax rule that
//! lives as a `const` inside a parser.
//!
//! # Why here rather than in the transport
//!
//! `pinion-rpc` parses these paths and `pinion-screen` publishes them, and
//! neither depends on the other — `pinion-core` is the crate they share. Put
//! in either one, the other would have to spell the literal a second time,
//! which is the divergence this module removes rather than relocates. The
//! parser now reads its separator from here, so
//! [`path_at`]'s output and `split_at_external`'s input are the same string by
//! construction; `pinion-rpc`'s own round-trip test is what holds that.
//!
//! # What an address does not carry
//!
//! A window prefix. `/window[<id>]/…` is the transport's, and a roster does not
//! know which window its host put a screen in; the single-window short-circuit
//! makes the plain form resolve, and a caller that needs to name a window
//! prepends it. Stating the limit rather than implying totality.
//!
//! # Where the floor stands
//!
//! The question does not arise on the reference toolkit at 6.11: it has no
//! layer in which a widget publishes its state under named paths, so assembling
//! two screens there composes widget trees and never composes *surfaces*. This
//! is a debt incurred somewhere this tree is ahead, and the floor has nothing
//! to say about it.
//!
//! [`pinion_rpc::path::split_at_external`]: https://docs.rs/pinion-rpc

/// The literal separating a scene walk from the introspect path that follows
/// it (§5.34 R42).
///
/// The **one** place this string exists. `pinion-rpc`'s splitter reads it here,
/// so a change to the grammar reaches the parser and every publisher at once
/// instead of leaving them to be found.
pub const SEPARATOR: &str = "/external/";

/// The address of the surface carried by the scene node tagged `tag`.
///
/// This is the prefix a client appends an introspect path to; [`path_at`] does
/// the appending. An empty `tag` addresses the scene **root**, which is the
/// binding's own primary surface — the form a standalone binary answers on, and
/// the form an assembled application answers *for the host*.
///
/// The returned value carries no trailing slash, so it reads as a place rather
/// than as an unfinished path.
#[must_use]
pub fn surface_at(tag: &str) -> String {
    let sep = SEPARATOR.trim_end_matches('/');
    if tag.is_empty() {
        sep.to_owned()
    } else {
        format!("/{tag}{sep}")
    }
}

/// One introspect path on the surface carried by the node tagged `tag`.
///
/// `path_at("node_lab", "graph")` is `/node_lab/external/graph`;
/// `path_at("", "graph")` is `/external/graph`. Both are what the transport's
/// splitter reads, which is asserted where both are visible.
#[must_use]
pub fn path_at(tag: &str, introspect: &str) -> String {
    format!("{}/{}", surface_at(tag), introspect.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::{SEPARATOR, path_at, surface_at};

    #[test]
    fn a_tagged_surfaces_address_names_that_tag_and_nothing_else() {
        assert_eq!(surface_at("node_lab"), "/node_lab/external");
        assert_eq!(surface_at("packet_view"), "/packet_view/external");
        // Two screens get two addresses. The property a client relies on when
        // it walks a roster: an address it read on one row cannot reach the
        // screen on another.
        assert_ne!(surface_at("node_lab"), surface_at("packet_view"));
    }

    #[test]
    fn the_root_surface_is_addressed_by_the_separator_alone() {
        assert_eq!(surface_at(""), "/external");
        assert_eq!(path_at("", "graph"), "/external/graph");
    }

    #[test]
    fn a_path_is_its_surface_plus_the_introspect_path() {
        assert_eq!(path_at("node_lab", "graph"), "/node_lab/external/graph");
        // A caller that already wrote the leading slash gets the same address
        // rather than an empty segment, which the scene walk would read as a
        // node nobody tagged.
        assert_eq!(path_at("node_lab", "/graph"), "/node_lab/external/graph");
    }

    #[test]
    fn every_address_this_module_builds_contains_the_separator_it_publishes() {
        // Not a spelling of the rule — a comparison against the constant the
        // parser reads, so a change to one cannot leave the other behind.
        for tag in ["", "node_lab", "packet_view"] {
            assert!(
                path_at(tag, "graph").contains(SEPARATOR),
                "an address that does not carry the separator cannot be split \
                 back into a surface and a path: {tag:?}"
            );
        }
    }
}
