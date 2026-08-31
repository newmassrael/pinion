//! ★★★★★ R1926 — **a socket type has a colour, and a port is drawn in the
//! colours of what it carries.**
//!
//! # What the reference does, measured at its own header rather than summarised
//!
//! Its graph schema publishes three overridable answers, and the three are
//! separated by **what each one is asked with** — which is the whole reason
//! they are three rows and not one:
//!
//! * *the colour of a pin TYPE*, asked with a type and no port at all, so a
//!   legend or a type picker can ask it. Answers black when nobody overrides.
//! * *the colour of a PIN*, asked with a port, and whose supplied answer is
//!   simply the first one applied to that port's type.
//! * *the SECOND colour of a pin type*, asked with a type. Answers white when
//!   nobody overrides.
//!
//! Three findings came out of reading them, and each one changed the shape
//! built here:
//!
//! 1. **The pin-level answer is not a per-pin colour.** Its supplied answer IS
//!    the type's, and measured across the whole engine source **twelve**
//!    schemas override the *type* colour while **one** overrides the *pin*
//!    colour — and that one reads the port's type more precisely (through a
//!    sub-category object it carries) and then answers a TYPE colour, falling
//!    back to the type answer otherwise. So nothing in the reference gives one
//!    port a colour of its own. A port's colour is a **derivation** here, and
//!    there is no per-port authored colour to drift from the type's.
//! 2. **The census's reason for the secondary colour was wrong.** It read *a
//!    container whose element type has a colour of its own*. Measured at the
//!    only implementation of substance, the second colour is answered **only
//!    when the type is a MAP**, and what it answers is the colour of the map's
//!    **value** half — an array or a set gets a settings constant. So it is not
//!    about containers and not about elements: it is *this type is made of two,
//!    and the second one is drawn too*.
//! 3. **Absence is not sayable there.** The supplied answer is black, and the
//!    one implementation of substance writes, in its own comment, *this type
//!    does not have a defined colour* before returning a settings default. A
//!    caller cannot tell *declared black* from *never declared*.
//!
//! # What is built, and the three measured ways it is better
//!
//! [`NodeKind::type_colour`] is the one declaration — the taxonomy's, like
//! [`NodeKind::type_description`] and [`NodeKind::composition`] — and
//! everything else derives from it.
//!
//! 1. **Absence is a value.** `Option<Tint>`: a taxonomy that colours some of
//!    its types and not others is the ordinary case, and a reader can tell.
//! 2. **The second colour generalises to the Nth.** [`Palette::members`] is one
//!    entry per member of a **composite** type, derived from
//!    [`NodeKind::composition`] (R1912) — which is already declared, so no
//!    application writes its parts down twice. The reference's map is the
//!    two-member case of this; a three-member composite, which it cannot speak
//!    about at all, is ordinary here.
//! 3. **A port's colour and its type's cannot disagree**, because there is one
//!    declaration and the port's answer is computed from it. The reference has
//!    two virtuals that a schema is free to make inconsistent, and the
//!    measurement above says the freedom is not even used.
//!
//! # Control is not a type, so it is its own declaration
//!
//! The reference reaches an execution pin's colour through the same hook,
//! because there an exec pin is a pin *type* (`PinCategory` is the string
//! `"exec"`). R1599 made that impossible here on purpose: a port carries a
//! value **or** control ([`Flow`]), and control has no type to look one up by.
//! So [`NodeKind::control_colour`] is a second declaration, and that is the
//! price of the stronger model, stated rather than hidden.

use serde::{Deserialize, Serialize};

use crate::appearance::Tint;
use crate::model::{Document, Flow, NodeId, NodeKind, PortRef, TreeId};
use crate::split::Composition;

/// The colours something carrying a socket type is drawn in.
///
/// Never an error and never empty of meaning: a taxonomy that declares no
/// colours answers a palette that says so ([`Palette::is_silent`]), which is a
/// different thing from a black one.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Palette {
    own: Option<Tint>,
    members: Vec<Option<Tint>>,
}

impl Palette {
    /// The colour of the type itself, or `None` when the taxonomy declares
    /// none.
    ///
    /// The reference's *pin type colour*, with the one difference that matters:
    /// there the same call answers `Black` for a type nobody coloured.
    #[must_use]
    pub const fn own(&self) -> Option<Tint> {
        self.own
    }

    /// One colour per member of a **composite** type, in the order
    /// [`NodeKind::composition`] declares them. Empty for an atom, and empty
    /// for a container — a container declares no member ports, so there is
    /// nothing to take a colour from.
    ///
    /// The reference's *secondary pin type colour* is the second entry of the
    /// two-member case. It has no third.
    #[must_use]
    pub fn members(&self) -> &[Option<Tint>] {
        &self.members
    }

    /// Whether nothing at all was declared — no colour of its own and no member
    /// that has one.
    ///
    /// What a renderer asks to decide whether to fall back to its own ink. A
    /// palette whose `own` is `None` may still have coloured members, which is
    /// why this is not `own().is_none()`.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.own.is_none() && self.members.iter().all(Option::is_none)
    }
}

/// The colours a socket **type** is drawn in, asked where no port exists.
///
/// This is the question a legend asks, and a type picker, and anything that
/// draws a taxonomy rather than a graph — which is exactly why the reference
/// keeps it apart from the pin-level call and why it is kept apart here.
#[must_use]
pub fn type_palette<K: NodeKind>(ty: &K::Type) -> Palette {
    Palette {
        own: K::type_colour(ty),
        members: match K::composition(ty) {
            Composition::Members(ports) => ports
                .iter()
                .map(|member| member.flow.value_type().and_then(K::type_colour))
                .collect(),
            Composition::Atom | Composition::Container => Vec::new(),
        },
    }
}

/// The colours a port carrying `flow` is drawn in.
///
/// The general entry point, and the one a renderer holding a resolved port
/// should use: a split's member ports are spliced into the signature
/// ([`Document::resolved_ports`](crate::Document::resolved_ports)), so asking
/// with the port in hand cannot mis-index the way an index computed alongside
/// it can.
#[must_use]
pub fn palette_of<K: NodeKind>(flow: &Flow<K::Type, K::Value>) -> Palette {
    match flow {
        Flow::Value { ty, .. } => type_palette::<K>(ty),
        Flow::Control => Palette {
            own: K::control_colour(),
            members: Vec::new(),
        },
    }
}

impl<K: NodeKind> Document<K> {
    /// The colours one port of one node is drawn in, or `None` when there is no
    /// such port.
    ///
    /// Over the **resolved** signature, so a port a split put there answers for
    /// its own type rather than for the type it came out of — which is the
    /// difference a screen drawing two halves of one address in two colours
    /// depends on.
    ///
    /// `None` rather than a colour for a port that is not there. The reference
    /// answers `Black` for a null pin, which a caller cannot tell from a pin
    /// that is really black.
    #[must_use]
    pub fn port_palette(&self, tree: TreeId, node: NodeId, port: PortRef) -> Option<Palette> {
        let signature = self.signature(tree, node)?;
        let ports = match port.side {
            crate::Side::Input => signature.inputs,
            crate::Side::Output => signature.outputs,
        };
        ports
            .get(port.index as usize)
            .map(|held| palette_of::<K>(&held.flow))
    }
}
