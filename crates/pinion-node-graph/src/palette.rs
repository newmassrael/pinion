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
        // R1934 — an undecided port has no type to look a colour up by, so it
        // reads its own resting colour. No members: nothing has decided what it
        // carries, let alone what that is made of.
        Flow::Undecided => Palette {
            own: K::undecided_colour(),
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

    // ★ R1986 — `instances_of` and `remove_definition` moved to
    // `definition.rs`, beside the two verbs R1986 added and the permission
    // surface all three are now decided by. They were here because R1944 wrote
    // them next to the definition palette; the family is what they belong to.
    //
    // ⚠ AND MOVING THEM SURFACED A DEFECT THIS FILE HAD CARRIED SINCE R1944.
    // The R1940 block explaining `faces` sat HERE, a hundred lines from that
    // function, so it documented whichever item came next — which since R1944
    // was `instances_of`, published as "the faces this node is actually drawn
    // with". Nothing could see it: rustdoc renders a doc comment against the
    // item that follows it, and both of these are `pub`, so the pages were
    // wrong rather than missing. It is back on `faces` below, where it says
    // what it means. Clippy names it as an empty line after a doc comment only
    // once the item between them goes away.

    /// ★★★★★ R1943 — **make two nodes a zone**: one opens it, the other closes
    /// it, and the region between them is derived rather than stored.
    ///
    /// # Errors
    ///
    /// [`PairError`](crate::PairError), one arm per distinct refusal — which is
    /// the measured difference from the reference, whose equivalent answers
    /// `bool` and writes its reason into a report list.
    pub fn pair(
        &mut self,
        tree: TreeId,
        opener: NodeId,
        closer: NodeId,
    ) -> Result<(), crate::PairError> {
        use crate::PairError;
        if opener == closer {
            return Err(PairError::ItsOwnCloser(opener));
        }
        let held = self.tree(tree).ok_or(PairError::NoSuchNode(opener))?;
        let want = match &held.node(opener).ok_or(PairError::NoSuchNode(opener))?.body {
            crate::NodeBody::Kind(kind) => {
                kind.closed_by().ok_or(PairError::OpensNothing(opener))?
            }
            _ => return Err(PairError::NotAKind(opener)),
        };
        match &held.node(closer).ok_or(PairError::NoSuchNode(closer))?.body {
            crate::NodeBody::Kind(kind) if *kind == want => {}
            crate::NodeBody::Kind(_) => return Err(PairError::WrongCloser { opener, closer }),
            _ => return Err(PairError::NotAKind(closer)),
        }
        // ⚠ BOTH ends are checked, not only the closer. The reference checks
        // only whether the closer is spoken for — its opener's own stored id is
        // simply overwritten, so re-pairing an opener silently abandons the
        // zone it was in.
        //
        // ★ R2003 — against what STANDS, not against what is stored: a pairing
        // whose support has gone must not refuse a new one, or a node whose
        // partner was removed could never be paired again.
        let standing = self.standing_zones(tree);
        for (already, taken) in &standing {
            if *already == opener || *taken == opener {
                return Err(PairError::AlreadyPaired {
                    node: opener,
                    with: if *already == opener { *taken } else { *already },
                });
            }
            if *already == closer || *taken == closer {
                return Err(PairError::AlreadyPaired {
                    node: closer,
                    with: if *already == closer { *taken } else { *already },
                });
            }
        }
        self.tree_mut(tree)
            .ok_or(PairError::NoSuchNode(opener))?
            .zones
            .insert(opener, closer);
        Ok(())
    }

    /// ★ R1943 — take a zone apart, answering whether there was one.
    ///
    /// Addressed by either end, because a person clicking a node to un-zone it
    /// has whichever end they clicked — and making them find the opener first
    /// would be this crate's storage decision leaking into its surface.
    pub fn unpair(&mut self, tree: TreeId, node: NodeId) -> bool {
        if self.tree(tree).is_none() {
            return false;
        }
        // ★ R2003 — what STANDS, so this answers `false` for a pairing whose
        // support has already gone rather than reporting that it took one apart.
        let opener = self
            .standing_zones(tree)
            .into_iter()
            .find(|(opens, closes)| *opens == node || *closes == node)
            .map(|(opens, _)| opens);
        match opener {
            Some(opens) => self.tree_mut(tree).is_some_and(|held| {
                held.zones.remove(&opens);
                true
            }),
            None => false,
        }
    }

    /// ★★★★★ R2003 — **the pairings this tree still SUPPORTS**, which is not
    /// the same set as the pairings it has stored.
    ///
    /// A zone is a pair of nodes and a pair is a claim about two things that go
    /// on living their own lives. Two public verbs can take the support away
    /// without ever naming a zone:
    ///
    /// * [`set_kind`](Self::set_kind) changes what one end IS. An opener swapped
    ///   for a kind that opens nothing, or a closer swapped for a kind the
    ///   opener never declared, leaves a pairing whose two ends no longer agree.
    /// * [`remove_node`](Self::remove_node) takes an end away entirely, leaving
    ///   an id that resolves to no node.
    ///
    /// ⚠ **Both were measured at R2003, and the second one falsified a sentence
    /// this crate had written down.** [`Tree::zones`]'s own doc said *a dangling
    /// id cannot outlive the node, because `unpair` and node removal both go
    /// through the map* — and node removal did not go near it. Driven: pair two
    /// nodes, remove the closer, and the opener still answered
    /// `Opens(<a node that is not there>)` with [`validate`](Self::validate)
    /// reporting nothing. The swap case was the same shape one verb over.
    ///
    /// # Why the repair is a derivation rather than a longer list of writers
    ///
    /// Making each writer maintain the map is the repair that has to be
    /// performed again for every writer added afterwards, and nothing would
    /// catch the one that forgot — the failure mode is silence. Deriving what
    /// is *standing* from what the tree currently holds cannot go out of date,
    /// because a verb that has not been written yet is already covered: the
    /// pairing simply stops being honoured the moment its support goes.
    ///
    /// The stored map is then a **claim** and this is the truth about it, which
    /// is the shape [`advanced_view`](Self::advanced_view) already has for
    /// another declaration (R2001).
    ///
    /// [`Tree::zones`]: crate::Tree
    pub(crate) fn standing_zones(
        &self,
        tree: TreeId,
    ) -> std::collections::BTreeMap<NodeId, NodeId> {
        let Some(held) = self.tree(tree) else {
            return std::collections::BTreeMap::new();
        };
        held.zones
            .iter()
            .filter(|(opens, closes)| {
                let want = match held.node(**opens).map(|held| &held.body) {
                    Some(crate::NodeBody::Kind(kind)) => kind.closed_by(),
                    _ => None,
                };
                match (want, held.node(**closes).map(|held| &held.body)) {
                    (Some(want), Some(crate::NodeBody::Kind(kind))) => *kind == want,
                    _ => false,
                }
            })
            .map(|(opens, closes)| (*opens, *closes))
            .collect()
    }

    /// ★★★★★ R2003 — drop the pairings this tree no longer supports, and name
    /// the partner each one leaves standing alone.
    ///
    /// [`standing_zones`](Self::standing_zones) is what makes a lapsed pairing
    /// harmless; this is what stops it coming BACK. Ids are handed out once per
    /// tree and never reused, so a pairing whose closer was removed can never
    /// be true again — but one whose opener was swapped away can, the moment
    /// somebody swaps that opener back, and a zone reappearing because of an
    /// edit two gestures ago is not something anybody asked for.
    ///
    /// Called by the two verbs measured able to lapse one. The derivation above
    /// is what covers the third writer nobody has written yet.
    pub(crate) fn reap_zones(&mut self, tree: TreeId, about: NodeId) -> Option<NodeId> {
        let standing = self.standing_zones(tree);
        let held = self.tree(tree)?;
        let partner = held
            .zones
            .iter()
            .find(|(opens, closes)| {
                (**opens == about || **closes == about) && !standing.contains_key(*opens)
            })
            .map(|(opens, closes)| if *opens == about { *closes } else { *opens });
        if held.zones.len() != standing.len() {
            self.tree_mut(tree)?.zones = standing;
        }
        partner
    }

    /// ★★★★★ R1943 — **what this node is with respect to zones**, or `None`
    /// when its kind has nothing to do with them.
    ///
    /// Answers from EITHER end, which the reference cannot without scanning:
    /// there the pairing lives on the opener as the closer's id, so asking a
    /// closer what it closes means walking every opening node in the tree.
    ///
    /// ★ R2003 — read against what the tree currently SUPPORTS rather than
    /// against the stored pairing, so a pairing whose support has gone is never
    /// reported as one that stands: a zone is honoured only while both ends are
    /// present and the opener's kind still declares the closer's. A caller
    /// therefore never has to ask whether the answer is still true.
    ///
    /// ⚠ Which is a guarantee this crate did not have until R2003 — removing
    /// one end left the other answering with an id that resolved to no node,
    /// and [`validate`](Self::validate) said nothing about it.
    #[must_use]
    pub fn in_zone(&self, tree: TreeId, node: NodeId) -> Option<crate::InZone> {
        let held = self.tree(tree)?;
        let standing = self.standing_zones(tree);
        if let Some(closer) = standing.get(&node) {
            return Some(crate::InZone::Opens(*closer));
        }
        if let Some((opener, _)) = standing.iter().find(|(_, closes)| **closes == node) {
            return Some(crate::InZone::Closes(*opener));
        }
        match &held.node(node)?.body {
            crate::NodeBody::Kind(kind) if kind.closed_by().is_some() => {
                Some(crate::InZone::OpensNothingYet)
            }
            _ => None,
        }
    }

    /// ★★★★★ R1940 — **the faces this node is actually drawn with**, or `None`
    /// when neither the person nor the kind has said.
    ///
    /// The one place the two sources are ranked, and that ranking is the whole
    /// of what this function decides:
    ///
    /// 1. **What a person authored wins** ([`Appearance::tint`]). A colour
    ///    somebody chose is not a suggestion, and a kind that recomputed over
    ///    the top of it would make the author's gesture silently ineffective.
    /// 2. **Else what the kind says** ([`NodeKind::drawn_as`]) — its own
    ///    colour, or the colour of a type, resolved through the same
    ///    [`type_colour`](NodeKind::type_colour) a PORT of that type is drawn
    ///    with.
    /// 3. **Else nothing**, which the application draws however it draws a
    ///    node. `None` and not a black: a colour nobody chose is not a colour,
    ///    and the reference's port-colour hook is measured returning an actual
    ///    black for exactly this case, where *nobody coloured this* and
    ///    *somebody chose black* become one answer.
    ///
    /// ⚠ ★ Ranked HERE and not at each drawing site, which is the difference
    /// from the reference: there, the choose-the-override-else-the-fixed-class
    /// expression is written out at BOTH of its consumers, and the authored
    /// colour is a third path again — three places that can disagree about what
    /// a node looks like. One function is what makes "the register a screen
    /// reads and the colour it paints cannot differ" a property rather than a
    /// habit.
    ///
    /// ⚠ A **structural** body has no kind to ask — a group instance, a frame,
    /// an interface end, a delay — so it reaches step 3 unless a person
    /// authored a colour. Stated rather than left implicit, because a group
    /// instance is precisely where the reference's own third implementation
    /// does something (it reads the colour tag of the definition the instance
    /// stands for), and this crate does not yet carry a colour on a definition
    /// to read.
    ///
    /// ⚠ ★ R1986 — this block was a hundred lines above until this round, where
    /// it documented `instances_of` instead. See the note there.
    ///
    /// [`Appearance::tint`]: crate::Appearance::tint
    #[must_use]
    pub fn faces(&self, tree: TreeId, node: NodeId) -> Option<crate::Faces> {
        let held = self.tree(tree)?.node(node)?;
        if let Some(authored) = held.appearance.tint {
            return Some(crate::Faces::of(authored));
        }
        match &held.body {
            crate::NodeBody::Kind(kind) => match kind.drawn_as() {
                crate::Drawn::Unstated => None,
                crate::Drawn::In(tint) => Some(crate::Faces::of(tint)),
                crate::Drawn::LikeType(ty) => K::type_colour(&ty).map(crate::Faces::of),
            },
            _ => None,
        }
    }
}
