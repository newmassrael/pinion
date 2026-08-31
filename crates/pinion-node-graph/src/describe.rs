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

/// ★★★★★ R1934 — what crosses a port, as the four answers there actually are.
///
/// The vocabulary [`Flow`] admits, projected for a reader: a value (with the
/// type's own sentence when the taxonomy has one), control, or nothing decided
/// yet. Kept beside [`PortTooltip`] rather than inside [`Flow`] because a flow
/// carries the *type* and this carries what a person is told about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Carrying {
    /// A value. `described` is [`NodeKind::type_description`] — `None` when the
    /// taxonomy says nothing about the type, which is **not** the same fact as
    /// carrying control and is no longer rendered as though it were.
    Value {
        /// What the type says about itself, when it says anything.
        described: Option<String>,
    },
    /// Control: the edge says *when*, never *what*.
    Control,
    /// R1934 — nothing has decided yet. Reached by a [`Flow::Undecided`] port,
    /// which today means a [`NodeBody::Reroute`](crate::NodeBody::Reroute)
    /// whose chain nothing has wired to.
    Undecided,
}

/// ★★★★★ R1916 — everything a port can say about itself, in pieces.
///
/// Every field is a fact the reference's own tooltip either loses in a string
/// or never had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortTooltip {
    /// The port's name, as the renderer draws it.
    pub name: String,
    /// ★★★★★ R1934 — **what crosses this port**, said as a closed vocabulary
    /// rather than as a string that might be missing.
    ///
    /// This field was `Option<String>` from R1916 to R1934 and the absence had
    /// **two meanings at once**: a control port, and a value port whose
    /// taxonomy answered no [`NodeKind::type_description`]. [`Self::sentence`]
    /// resolved that ambiguity by picking one — it rendered *both* as "accepts
    /// control" — so a value port with an undescribed type told every reader
    /// it carried control. Measured on this crate's own test taxonomy at
    /// R1934, and asserted since by
    /// `r1934_an_undescribed_value_port_does_not_claim_to_carry_control`.
    ///
    /// The repair is R1928's rule applied again: **when there are more than
    /// two answers, the answer is a type and not an `Option`.**
    pub carries: Carrying,
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
            (
                Carrying::Value {
                    described: Some(what),
                },
                Multiplicity::One,
            ) => {
                let _ = write!(out, " — {facing} one {what}");
            }
            (
                Carrying::Value {
                    described: Some(what),
                },
                Multiplicity::Many,
            ) => {
                let _ = write!(out, " — {facing} any number of {what}");
            }
            // R1934 — a value port whose taxonomy describes nothing still says
            // it carries a VALUE. Until this round it said "control".
            (Carrying::Value { described: None }, Multiplicity::One) => {
                let _ = write!(out, " — {facing} one value");
            }
            (Carrying::Value { described: None }, Multiplicity::Many) => {
                let _ = write!(out, " — {facing} any number of values");
            }
            (Carrying::Control, _) => {
                let _ = write!(out, " — {facing} control");
            }
            // R1934 — the undecided port. It is `One` on both sides, so there
            // is no plural form to write.
            (Carrying::Undecided, _) => {
                let _ = write!(out, " — {facing} whatever the first wire decides");
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
            Flow::Value { ty, .. } => Carrying::Value {
                described: K::type_description(ty),
            },
            Flow::Control => Carrying::Control,
            Flow::Undecided => Carrying::Undecided,
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
