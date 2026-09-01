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
    /// [`Appearance::tint`]: crate::Appearance::tint
    /// ★★★★★ R1944 — **every node that stands for this definition**, as (the
    /// tree it is in, the node).
    ///
    /// `instance_count` has answered *how many* since groups existed; this
    /// answers *where*, which is what a refusal has to carry to be actionable
    /// and what a removal has to report to be undoable.
    #[must_use]
    pub fn instances_of(&self, definition: TreeId) -> Vec<(TreeId, NodeId)> {
        let mut found: Vec<(TreeId, NodeId)> = self
            .trees()
            .flat_map(|held| {
                held.nodes()
                    .filter(|node| node.body == crate::NodeBody::Group(definition))
                    .map(move |node| (held.id, node.id))
            })
            .collect();
        found.sort_unstable();
        found
    }

    /// ★★★★★ R1944 — **remove a definition from the document**, saying what
    /// went with it.
    ///
    /// # What forced it, measured in the reference this round
    ///
    /// Its schema is asked to delete a graph, and the editor falls back to its
    /// own procedure when the schema declines. Counted: **one declaration
    /// (answering NO), ZERO overriders, one consumer** — so that extension
    /// point has never once been taken, and every deletion goes down the
    /// fallback. R1938's shape: a hook whose refusal is never exercised is a
    /// hook nobody has had to think about.
    ///
    /// The fallback is what the capability really is, and it does three things
    /// this answers differently:
    ///
    /// * **It removes every node bound to that graph, unconditionally**, and
    ///   answers `void`. A caller cannot report what it cost, and a person who
    ///   deleted a definition in use loses the nodes that used it without being
    ///   asked. Here [`Used`](crate::Used) makes the caller choose, and
    ///   `Refuse` names the sites.
    /// * **Whether a graph may go at all is a FLAG on the graph**
    ///   (`bAllowDeletion`), so *why not* has no answer. Here the refusals are
    ///   named ([`RemoveTreeError`](crate::RemoveTreeError)).
    /// * **It does not look for definitions orphaned by the removal.** A
    ///   definition can hold instances of another, so removing one can leave a
    ///   chain with nothing pointing at it; those are removed and REPORTED here.
    ///
    /// # Errors
    ///
    /// [`RemoveTreeError`](crate::RemoveTreeError).
    pub fn remove_definition(
        &mut self,
        definition: TreeId,
        used: crate::Used,
    ) -> Result<crate::RemovedTree, crate::RemoveTreeError> {
        use crate::{RemoveTreeError, RemovedTree, Used};
        if definition == crate::ROOT {
            return Err(RemoveTreeError::TheRoot);
        }
        if self.tree(definition).is_none() {
            return Err(RemoveTreeError::NoSuchTree(definition));
        }
        let standing = self.instances_of(definition);
        if used == Used::Refuse && !standing.is_empty() {
            return Err(RemoveTreeError::StillUsed { by: standing });
        }
        // ⚠ Taken BEFORE anything is removed: which definitions currently have
        // an instance is what tells an orphan this removal MADE from one that
        // was already standing alone.
        let was_used: std::collections::BTreeSet<TreeId> = self
            .definitions()
            .map(|held| held.id)
            .filter(|id| !self.instances_of(*id).is_empty())
            .collect();
        // ⚠ The instances go FIRST and from every tree, including the one being
        // removed: a definition may hold an instance of itself's peer, and
        // dropping the tree without clearing them would leave a node whose body
        // names a tree that is gone.
        let mut went = RemovedTree {
            instances: standing.clone(),
            definitions: vec![definition],
        };
        // ★ Through `remove_node`, not by reaching into the tree: that verb
        // already drops the links a removed node was on and reports what it
        // orphaned, and a second removal path here would be a second set of
        // invariants free to drift from it.
        for (tree, node) in &standing {
            let _ = self.remove_node(*tree, *node);
        }
        self.drop_tree(definition);
        // ★ And then whatever THIS REMOVAL orphaned, transitively.
        //
        // ⚠ Only what it orphaned. A definition that already stood alone —
        // authored and not yet placed — is a legitimate state this must not
        // sweep up, so the population is the ones that HAD an instance before
        // and have none now. That distinction is the reason `was_used` is taken
        // before anything is removed rather than derived afterwards.
        loop {
            let orphaned: Vec<TreeId> = self
                .definitions()
                .map(|held| held.id)
                .filter(|id| was_used.contains(id) && self.instances_of(*id).is_empty())
                .collect();
            if orphaned.is_empty() {
                break;
            }
            for id in orphaned {
                self.drop_tree(id);
                went.definitions.push(id);
            }
        }
        went.definitions.sort_unstable();
        Ok(went)
    }

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
        for (already, taken) in &held.zones {
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
        let Some(held) = self.tree(tree) else {
            return false;
        };
        let opener = held
            .zones
            .iter()
            .find(|(opens, closes)| **opens == node || **closes == node)
            .map(|(opens, _)| *opens);
        match opener {
            Some(opens) => self.tree_mut(tree).is_some_and(|held| {
                held.zones.remove(&opens);
                true
            }),
            None => false,
        }
    }

    /// ★★★★★ R1943 — **what this node is with respect to zones**, or `None`
    /// when its kind has nothing to do with them.
    ///
    /// Answers from EITHER end, which the reference cannot without scanning:
    /// there the pairing lives on the opener as the closer's id, so asking a
    /// closer what it closes means walking every opening node in the tree.
    #[must_use]
    pub fn in_zone(&self, tree: TreeId, node: NodeId) -> Option<crate::InZone> {
        let held = self.tree(tree)?;
        if let Some(closer) = held.zones.get(&node) {
            return Some(crate::InZone::Opens(*closer));
        }
        if let Some((opener, _)) = held.zones.iter().find(|(_, closes)| **closes == node) {
            return Some(crate::InZone::Closes(*opener));
        }
        match &held.node(node)?.body {
            crate::NodeBody::Kind(kind) if kind.closed_by().is_some() => {
                Some(crate::InZone::OpensNothingYet)
            }
            _ => None,
        }
    }

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
