//! ★★★★★ R1916 — **what a port says about itself**, composed in one place.
//!
//! The reference has two hooks for this and neither owns the sentence. A node
//! answers a *hover text* hook taking a pin and filling in a string; a schema
//! answers a *construct a basic pin tooltip* hook taking a pin, **a description
//! given to it**, and a string to fill in — and that middle argument is the
//! whole difficulty: the description arrives from outside, with nothing in the
//! model saying where it came from. Read to the end, the base schema's
//! implementation is one line that hands that description straight back
//! unchanged, while the comment directly above it promises the hook "tacks on
//! any other data important to the schema (things like the pin's type, etc.)".
//!
//! ⇒ **the composition the documentation describes does not happen**, and there
//! is no one place it could be checked from.
//!
//! # What is here instead
//!
//! [`PortTooltip`] is the composition, and it is **structured rather than
//! joined**. That is the whole of how this is past the reference: a client is
//! handed the pieces — the port's name, what the type says it carries, the
//! port's own sentence, which way it faces, how many links it may hold, and
//! what it is carrying right now — and decides how to draw them. The reference
//! hands back one `FString`, so a tooltip that wanted the type on its own line
//! would have to parse the sentence back apart.
//!
//! [`PortTooltip::sentence`] is the default rendering, for a consumer that
//! wants one string. It is *derived from the pieces*, so a consumer reading the
//! pieces and a consumer reading the sentence cannot disagree.

use crate::model::{Document, Flow, Multiplicity, NodeId, NodeKind, Side, TreeId};
use crate::split::PortPath;

/// ★★★★★ R1916 — everything a port can say about itself, in pieces.
///
/// Every field is a fact the reference's own tooltip either loses in a string
/// or never had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortTooltip {
    /// The port's name, as the renderer draws it.
    pub name: String,
    /// What the TYPE says it carries ([`NodeKind::type_description`]), or
    /// `None` for a control port or a taxonomy that says nothing.
    pub carries: Option<String>,
    /// The PORT's own sentence ([`Port::description`](crate::Port::description)),
    /// or `None`.
    pub says: Option<String>,
    /// Which way it faces. A person reading a tooltip on a canvas full of pins
    /// wants this said, and the reference's string leaves it to the reader's
    /// eyes.
    pub side: Side,
    /// How many links it may hold — the duality
    /// [`Flow::multiplicity`](crate::Flow::multiplicity) derives, said rather
    /// than left to be discovered by trying.
    pub multiplicity: Multiplicity,
    /// Whether anything is wired to it right now.
    pub wired: bool,
    /// ★ The address, when this port is a MEMBER a split put there. `None` for
    /// a port the node declares itself.
    ///
    /// The reference cannot say this at all: its sub-pins are pins, so a
    /// tooltip on one reads exactly like a tooltip on a whole pin.
    pub member_of: Option<PortPath>,
}

impl PortTooltip {
    /// ★ R1916 — the default one-string rendering, **derived from the pieces**.
    ///
    /// Derived and not stored, which is the reason a consumer can read either
    /// and get the same answer. A consumer that wants a different arrangement
    /// reads the fields; this exists so the common case is one call.
    #[must_use]
    pub fn sentence(&self) -> String {
        use std::fmt::Write as _;

        let mut out = self.name.clone();
        let facing = match self.side {
            Side::Input => "accepts",
            Side::Output => "gives",
        };
        match (&self.carries, self.multiplicity) {
            (Some(what), Multiplicity::One) => {
                let _ = write!(out, " — {facing} one {what}");
            }
            (Some(what), Multiplicity::Many) => {
                let _ = write!(out, " — {facing} any number of {what}");
            }
            (None, _) => {
                let _ = write!(out, " — {facing} control");
            }
        }
        if let Some(says) = &self.says {
            let _ = write!(out, ". {says}");
        }
        if self.member_of.is_some() {
            out.push_str(" (one half of a port that was split)");
        }
        out.push_str(if self.wired {
            ". Something is wired to it."
        } else {
            ". Nothing is wired to it."
        });
        out
    }
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1916 — **what the port at this address says about itself.**
    ///
    /// The one composition. The type's sentence and the port's sentence are
    /// joined here rather than at each consumer, so a screen that draws a
    /// tooltip and an agent that reads one over the wire are reading the same
    /// derivation — which is exactly what the reference's arrangement cannot
    /// offer, because its description arrives from outside its own model.
    ///
    /// `None` when the address names no port.
    #[must_use]
    pub fn port_tooltip(
        &self,
        tree: TreeId,
        node: NodeId,
        side: Side,
        at: &PortPath,
    ) -> Option<PortTooltip> {
        let port = self.port_at(tree, node, side, at).ok()?;
        let multiplicity = port.flow.multiplicity(side);
        let carries = match &port.flow {
            Flow::Value { ty, .. } => K::type_description(ty),
            Flow::Control => None,
        };
        // ★ Wiring is asked of the RESOLVED port, because that is what a wire
        // lands on. An address the signature does not currently expose has
        // nothing wired to it by construction, and answering `false` there is
        // the honest reading rather than a refusal.
        let wired = self
            .index_of(tree, node, side, at)
            .zip(self.tree(tree))
            .is_some_and(|(index, host)| {
                let socket = crate::Socket::new(node, index);
                match side {
                    Side::Input => host.link_into(socket).is_some(),
                    Side::Output => host.links().iter().any(|link| link.from == socket),
                }
            });
        Some(PortTooltip {
            name: port.name,
            carries,
            says: port.description,
            side,
            multiplicity,
            wired,
            member_of: (at.depth() > 0).then(|| at.clone()),
        })
    }
}
