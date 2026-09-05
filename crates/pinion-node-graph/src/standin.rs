//! ★★★★★ R2004 — **one node stands in for several**: a link authored at it is
//! one link per node it stands for.
//!
//! [`NodeBody::StandIn`] is the body; this module is the readings that resolve
//! it away and the verbs that edit what it stands for.
//!
//! # The reference's own sentence for the mechanism
//!
//! Measured in its state-machine baker, beside the loop that walks a state's
//! transitions: *"Alias's are simply decompiled into multiple connections."*
//! That is [`Document::expanded_links`] — the derivation this module exists
//! for — and it is worth quoting because the reference says it in a comment
//! and nowhere in a type: there the expansion happens inside the compile, so
//! nothing outside the baker can ask what a stand-in resolves to.
//!
//! # What the census row asked for, and why its sentence was wrong
//!
//! The pin read *a link whose source and sink are the same node*, and it named
//! two obstacles: `connect` refuses a self-link, and the control plane has no
//! self-edge constructor. Both are true and **neither is what the reference's
//! operator does.** Read from the source, `OnCreateSelfTransition` never makes
//! a self-edge: it creates an **alias node**, uniquifies its name, places it up
//! and to the right of the selected state, puts that state in the alias's
//! aliased-state **set**, and links alias → state. The self-loop is the
//! *derived* result of a link to a stand-in that stands for the far end.
//!
//! So the capability is not a self-edge constructor. It is a node that stands
//! in for a set, and the reference's operator is the **one-element case** of it
//! ([`Document::stand_in_for`]).
//!
//! # ★★★★★ The reference's hand-written rule is a THEOREM here
//!
//! Its alias validator says, in an error message discovered at compile time:
//!
//! > A alias (@@) used as a transition's target must alias a single state
//!
//! — so an alias may stand for many when a transition *leaves* it and for
//! exactly one when a transition *arrives* at it. That asymmetry is written by
//! hand, in one editor, for one plane.
//!
//! Here it is not written at all. [`Flow::multiplicity`](crate::Flow::multiplicity)
//! already answers how many links a port holds, and it answers by the duality
//! R1599 recorded: a value has one producer and many readers, a control
//! transfer has one successor and many predecessors. Expanding a stand-in
//! multiplies the links on the socket at the **far** end of the authored link,
//! so the law is:
//!
//! > a stand-in may stand for several exactly when the far socket admits
//! > [`Multiplicity::Many`].
//!
//! Put a control link on it and that recovers the reference's rule exactly —
//! leaving a stand-in piles onto a control *input*, which is `Many`; arriving
//! at one piles onto a control *output*, which is `One`. ★ And on the value
//! plane **the permitted direction inverts**, which the reference cannot
//! express because its state machine has one plane: arriving at a stand-in
//! piles onto a value *output* (`Many`, so several members are fine) and
//! leaving one piles onto a value *input* (`One`, so exactly one).
//!
//! [`Document::crowded`] is that law read off the ports, and
//! [`Document::represent`] refuses in advance the membership that would break
//! it.
//!
//! # What else this does that the reference does not
//!
//! * **"Does it stand for exactly one" is a total answer.** There,
//!   `GetAliasedState()` returns `nullptr` for three different situations — it
//!   is a global alias, it names more than one, or the one it names is not in
//!   this graph any more — and the double-click jump target is that same null,
//!   so a many-alias silently does nothing. [`Alone`] has an arm per reason.
//! * **A member that is gone is reported, not silently dropped.** There,
//!   `RebuildAliasedStateNodeReferences` runs on every load and *removes* the
//!   members the graph no longer holds, so a deletion elsewhere quietly shrinks
//!   what an alias stands for. Here [`Document::lost_members`] names them and
//!   [`Document::validate`] reports them — R1999's rule, that a
//!   re-classification says what it left behind.
//! * **The expansion is a reading anyone can take.** There it happens inside
//!   the bake, and the bake's own target-side path takes the same `nullptr`:
//!   `NextState = NextAliasNode->GetAliasedState()` followed by a null guard,
//!   so a transition into a many-alias is **skipped without a word** even
//!   though a separate validator would have complained about it.
//!
//! # What is deliberately NOT reproduced
//!
//! The reference's operator hard-codes two transition policy defaults on the
//! link it makes, one of which is a measured no-op: its own comment says a self
//! transition *should always have a delay so they dont re-enter each frame
//! (footgun prevention)* and the value it writes is `0.0`, while the field's
//! declared default is `-1.0` and the runtime guard is
//! `MinTimeBeforeReentry >= 0.0f && elapsed < MinTimeBeforeReentry`. Elapsed
//! time is never negative, so `0.0` enables a test that can never fire: the two
//! values differ in what the property panel *reads* and not in what runs. There
//! is no such policy on a link here to write, and inventing one to match would
//! be reproducing a defect: the shape is a sentinel float carrying two
//! questions — *is there a floor at all* and *how big is it* — on one axis, so
//! the value its own footgun-prevention line writes is inside the enabled range
//! and below every reachable elapsed time.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    Document, Link, LinkId, Multiplicity, NodeBody, NodeId, NodeKind, Represented, Side, Signature,
    Socket, TreeId,
};

/// How far to the right of the node it stands for a canned stand-in lands.
///
/// The reference's own `+200`, kept because a person who has used that editor
/// finds the card where they expect it.
const OVER: i32 = 200;

/// How far **up**, the reference's own `-100`.
const UP: i32 = 100;

/// Whether a stand-in stands for exactly one node, and when not, why not.
///
/// ★★★★★ The reference's `GetAliasedState()` answers this with a pointer that
/// is null in three distinguishable situations, and its double-click jump
/// target is that pointer — so *this alias covers a group* and *the state this
/// alias named has been deleted* arrive as the same nothing, and the editor
/// does nothing in both. Four arms and no null is the whole difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alone {
    /// It stands for this one node, and no other.
    Yes(NodeId),
    /// It stands for nothing at all — either it was never given a member, or
    /// every member it names is gone. [`Document::lost_members`] tells those
    /// two apart.
    Nobody,
    /// It stands for this many, so there is no single one to name.
    Several(usize),
    /// Its membership is not a list but *whoever is here*, so there is no
    /// single one even in a tree that currently holds one node. The reference's
    /// global alias, and it answers null for the same reason.
    Everyone,
}

impl Alone {
    /// The one node, when there is one.
    ///
    /// The reference's whole answer, offered as a derivation of this one so a
    /// caller that only wants the pointer is not forced to match — and so the
    /// reason is still there for the caller that wants it.
    #[must_use]
    pub const fn one(self) -> Option<NodeId> {
        match self {
            Self::Yes(node) => Some(node),
            Self::Nobody | Self::Several(_) | Self::Everyone => None,
        }
    }

    /// A stable word for a caption or a wire form.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Yes(_) => "one",
            Self::Nobody => "nobody",
            Self::Several(_) => "several",
            Self::Everyone => "everyone",
        }
    }
}

/// One link of the authored graph with every stand-in on it resolved away.
///
/// The reference's *"decompiled into multiple connections"*, as a value rather
/// than as a step inside a compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    /// The authored link this came from. Several expansions share one id, which
    /// is what says they are one thing a person drew.
    pub authored: LinkId,
    /// The producing socket, on a node that is not a stand-in.
    pub from: Socket,
    /// The consuming socket, on a node that is not a stand-in.
    pub to: Socket,
    /// The stand-ins this passed through, ascending. Empty for a link that
    /// touches none, which is every link in a document that has no stand-in —
    /// so this reading is the plain link list there, not a second one.
    pub through: Vec<NodeId>,
    /// The authored link's own mutedness, carried: expanding a structural fact
    /// must not change a semantic one.
    pub muted: bool,
}

/// Where a stand-in's expansion would put more links on a socket than that
/// socket holds.
///
/// The general form of the reference's *must alias a single state*, derived
/// from [`Flow::multiplicity`](crate::Flow::multiplicity) rather than written
/// out — see this module's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crowding {
    /// The authored link whose expansion piles up.
    pub link: LinkId,
    /// The stand-in on it.
    pub stand_in: NodeId,
    /// The socket at the far end, which is where the expansion lands.
    pub socket: Socket,
    /// Which of that socket's two sides the links land on.
    pub side: Side,
    /// How many links the expansion puts there. Always more than one — a socket
    /// that holds one is not crowded by one.
    pub would_be: usize,
}

/// What [`Document::stand_in_for`] made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoodIn {
    /// The stand-in, standing for exactly the node it was asked about.
    pub stand_in: NodeId,
    /// The link from the stand-in back to that node — the one whose expansion
    /// is the self-loop the direct edit refuses.
    pub link: LinkId,
    /// ★★★★★ The link that had to go, if there was one.
    ///
    /// The wire this verb draws lands on the node's first input, and a port
    /// that holds one link **displaces** what was there
    /// ([`Connected::displaced`](crate::Connected::displaced)). Reporting it is
    /// what makes the verb undoable; dropping it, which the first draft of this
    /// round did, makes a canned convenience destroy authored work silently.
    ///
    /// ⚠ The reference's operator cannot meet this at all — it wires into a
    /// state's transition IN pin, which is a control input and therefore holds
    /// many — so there is nothing there to copy and the answer had to be
    /// derived from what this crate's ports actually say. Measured on the
    /// analyzer topology at R2004: the first card the verb succeeded on already
    /// had a wire on that port, and the link count did not move.
    pub displaced: Option<Link>,
}

/// Why an edit to what a node stands for could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandInError {
    /// No such tree in this document.
    NoSuchTree(TreeId),
    /// No such node in that tree.
    NoSuchNode {
        /// The tree asked.
        tree: TreeId,
        /// The node asked for.
        node: NodeId,
    },
    /// The node is there and is not a stand-in, so it stands for nothing and
    /// there is no membership to edit.
    NotAStandIn {
        /// The tree asked.
        tree: TreeId,
        /// The node that is not one.
        node: NodeId,
    },
    /// A stand-in cannot stand for a stand-in.
    ///
    /// Refused rather than resolved recursively, which is what makes
    /// [`Document::stands_for`] a single step and therefore total: a chain
    /// could be made circular by two edits neither of which is wrong on its
    /// own, and the crate would then have to detect that instead of it being
    /// unrepresentable.
    NestedStandIn {
        /// The tree asked.
        tree: TreeId,
        /// The stand-in being edited.
        stand_in: NodeId,
        /// The member that is itself one.
        member: NodeId,
    },
    /// A stand-in cannot stand for itself.
    ItsOwnMember {
        /// The tree asked.
        tree: TreeId,
        /// The stand-in.
        node: NodeId,
    },
    /// It stands for whoever is in the tree, so there is no list to add to or
    /// take from — [`Document::represent_named`] is what turns it back into one.
    StandsForEveryone {
        /// The tree asked.
        tree: TreeId,
        /// The stand-in.
        node: NodeId,
    },
    /// The membership would crowd a socket the stand-in is already wired to.
    ///
    /// ★ The reference discovers this at compile time and phrases it as an
    /// error about the alias; this refuses the **edit that would cause it** and
    /// names the socket, so the graph never holds the state at all.
    WouldCrowd {
        /// The tree asked.
        tree: TreeId,
        /// The stand-in being edited.
        stand_in: NodeId,
        /// The far socket that would be piled onto.
        socket: Socket,
        /// Which side of it.
        side: Side,
        /// How many links would land there.
        would_be: usize,
    },
    /// The node has no port pair for a stand-in to be wired back through, so
    /// the canned verb has nothing to make.
    ///
    /// A node with no input, or none with an output — the shapes
    /// [`NodeBody::Echo`] and [`NodeBody::Frame`] have.
    NoWayBack {
        /// The tree asked.
        tree: TreeId,
        /// The node with no way back.
        node: NodeId,
    },
}

impl core::fmt::Display for StandInError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {} in tree {}", node.0, tree.0)
            }
            Self::NotAStandIn { tree, node } => write!(
                f,
                "node {} in tree {} stands in for nothing",
                node.0, tree.0
            ),
            Self::NestedStandIn {
                stand_in, member, ..
            } => write!(
                f,
                "node {} stands in for others, so node {} cannot stand in for it",
                member.0, stand_in.0
            ),
            Self::ItsOwnMember { node, .. } => {
                write!(f, "node {} cannot stand in for itself", node.0)
            }
            Self::StandsForEveryone { node, .. } => write!(
                f,
                "node {} stands in for whoever is here, which is not a list",
                node.0
            ),
            Self::WouldCrowd {
                stand_in,
                socket,
                side,
                would_be,
                ..
            } => write!(
                f,
                "node {} would put {would_be} links on the {} of {socket}, which holds one",
                stand_in.0,
                match side {
                    Side::Input => "input",
                    Side::Output => "output",
                },
            ),
            Self::NoWayBack { node, .. } => write!(
                f,
                "node {} has no port pair to be stood in for through",
                node.0
            ),
        }
    }
}

impl core::error::Error for StandInError {}

impl<K: NodeKind> Document<K> {
    /// What `node` stands in for: its live members, ascending.
    ///
    /// Empty for a node that is not a stand-in, so a caller that does not know
    /// what it is holding gets an answer rather than an option to unwrap. The
    /// members a stand-in *names* and no longer has are not here — see
    /// [`Self::lost_members`], which is the other half and is reported by
    /// [`Self::validate`].
    #[must_use]
    pub fn stands_for(&self, tree: TreeId, node: NodeId) -> BTreeSet<NodeId> {
        let Some(host) = self.tree(tree) else {
            return BTreeSet::new();
        };
        let Some(represented) = host.node(node).and_then(|held| match &held.body {
            NodeBody::StandIn(represented) => Some(represented),
            _ => None,
        }) else {
            return BTreeSet::new();
        };
        match represented {
            Represented::Named(named) => named
                .iter()
                .copied()
                .filter(|member| {
                    host.node(*member)
                        .is_some_and(|held| !matches!(held.body, NodeBody::StandIn(_)))
                })
                .collect(),
            // ★ Every application node and every group instance, and nothing
            // else. The structural arms (an interface end, a frame) and the
            // passing ones (a bend, a named endpoint, a far end, a register)
            // are this crate's own furniture rather than the subject matter a
            // person is quantifying over, and a stand-in is excluded because a
            // stand-in standing for stand-ins is what `NestedStandIn` refuses
            // one at a time.
            Represented::Everyone => host
                .nodes()
                .filter(|held| matches!(held.body, NodeBody::Kind(_) | NodeBody::Group(_)))
                .map(|held| held.id)
                .collect(),
        }
    }

    /// Whether `node` stands for exactly one, and when not, why not.
    ///
    /// `None` for a node that is not a stand-in — the one place this module
    /// distinguishes *not one of these* from *one of these standing for
    /// nothing*, because [`Alone::Nobody`] is a real state a stand-in can be in
    /// and reading it off a plain node would be wrong.
    #[must_use]
    pub fn stands_alone(&self, tree: TreeId, node: NodeId) -> Option<Alone> {
        let held = self.tree(tree)?.node(node)?;
        let NodeBody::StandIn(represented) = &held.body else {
            return None;
        };
        if matches!(represented, Represented::Everyone) {
            return Some(Alone::Everyone);
        }
        let members = self.stands_for(tree, node);
        let mut walk = members.iter();
        // ★ Read off the iterator rather than by indexing a length: the count
        // and the member then come from one walk, so there is no arm where a
        // length says one and no element is there to name — which is the shape
        // an `expect` here would have been standing in for.
        Some(match (walk.next(), walk.next()) {
            (None, _) => Alone::Nobody,
            (Some(only), None) => Alone::Yes(*only),
            (Some(_), Some(_)) => Alone::Several(members.len()),
        })
    }

    /// The members `node` names that its tree no longer holds, ascending.
    ///
    /// ★★★★★ The reference **repairs** this instead of reporting it: a private
    /// routine run on every load keeps only the members still in the graph, so
    /// deleting a state quietly shrinks what every alias of it stands for and
    /// nobody is told. Reporting is R1999's rule — a re-classification says
    /// what it left behind — and it is the difference between a person seeing
    /// that their group lost a member and a person seeing a group that behaves
    /// differently for no visible reason.
    ///
    /// A member that is still there but has *become* a stand-in is reported
    /// too: [`Self::represent`] refuses to add one, so the only way to reach
    /// that state is a document that arrived from a file.
    #[must_use]
    pub fn lost_members(&self, tree: TreeId, node: NodeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let Some(NodeBody::StandIn(Represented::Named(named))) =
            host.node(node).map(|held| &held.body)
        else {
            return Vec::new();
        };
        let mut lost: Vec<NodeId> = named
            .iter()
            .copied()
            .filter(|member| {
                !host
                    .node(*member)
                    .is_some_and(|held| !matches!(held.body, NodeBody::StandIn(_)))
            })
            .collect();
        lost.sort_unstable();
        lost
    }

    /// The signature a stand-in presents: **the one its members share**.
    ///
    /// Derived and not authored, the same decision [`NodeBody::Delay`] and
    /// [`NodeBody::Reroute`] make and for a sharper reason: the ports are what
    /// the expansion maps through, so a stand-in whose ports were authored
    /// could name a port its members do not have and the expansion would have
    /// nowhere to land.
    ///
    /// Empty — so nothing can be wired to it — when it stands for nothing, or
    /// when its members do not agree. ★ The reference never has to ask: every
    /// state in a state machine has one transition pin in and one out, so the
    /// uniformity is true by construction there and is written down nowhere.
    /// Here it is a *checked* property, and a stand-in over a mixed group says
    /// so by having no ports rather than by picking one member's.
    pub(crate) fn stand_in_signature(&self, tree: TreeId, node: NodeId) -> Signature<K> {
        let empty = Signature {
            inputs: Vec::new(),
            outputs: Vec::new(),
        };
        let members = self.stands_for(tree, node);
        let mut shared: Option<Signature<K>> = None;
        for member in members {
            // Members are never stand-ins, so this recursion is one level deep.
            let Some(theirs) = self.declared_signature(tree, member) else {
                return empty;
            };
            match &shared {
                None => shared = Some(theirs),
                Some(agreed) if *agreed == theirs => {}
                Some(_) => return empty,
            }
        }
        shared.unwrap_or(empty)
    }

    /// Every link of `tree` with the stand-ins on it resolved away, ascending
    /// by authored link and then by socket.
    ///
    /// This is what the graph **means**: a link drawn at a stand-in is one link
    /// per node it stands for, so a reader that wants the real wiring reads
    /// this and a reader that wants what a person drew reads
    /// [`Tree::links`](crate::Tree::links). Both are needed and they are
    /// different questions —
    /// which is why this is a second reading rather than a rewrite of the
    /// first.
    ///
    /// A link touching no stand-in comes through unchanged and alone, so in a
    /// document with no stand-in this answers the link list.
    #[must_use]
    pub fn expanded_links(&self, tree: TreeId) -> Vec<Expanded> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for link in host.links() {
            let sources = self.ends_of(tree, link.from);
            let sinks = self.ends_of(tree, link.to);
            let mut through = Vec::new();
            if sources.1 {
                through.push(link.from.node);
            }
            if sinks.1 {
                through.push(link.to.node);
            }
            through.sort_unstable();
            for from in &sources.0 {
                for to in &sinks.0 {
                    out.push(Expanded {
                        authored: link.id,
                        from: *from,
                        to: *to,
                        through: through.clone(),
                        muted: link.muted,
                    });
                }
            }
        }
        out.sort_by_key(|held| (held.authored.0, held.from.node.0, held.to.node.0));
        out
    }

    /// The expansion shaped as plain [`Link`]s, for the derivations that walk
    /// the wiring rather than report it.
    ///
    /// The ids are the **authored** ones, so several expansions of one drawn
    /// wire share an id. That is right for a reachability walk — which asks
    /// which nodes reach which — and it is why this is crate-private: a caller
    /// that wants to tell the expansions apart wants [`Expanded`], where the
    /// shared id is a stated fact rather than a collision.
    pub(crate) fn expanded_link_view(&self, tree: TreeId) -> Vec<Link> {
        self.expanded_links(tree)
            .into_iter()
            .map(|held| Link {
                id: held.authored,
                from: held.from,
                to: held.to,
                muted: held.muted,
            })
            .collect()
    }

    /// The sockets one end of an authored link resolves to, and whether that
    /// end was a stand-in at all.
    ///
    /// The port index carries across untouched, which is what the shared
    /// signature buys: a stand-in's port *n* is its members' port *n*, so the
    /// expansion needs no correspondence table and there is none to fall out of
    /// step.
    fn ends_of(&self, tree: TreeId, socket: Socket) -> (Vec<Socket>, bool) {
        let stands_for = self.stands_for(tree, socket.node);
        let is_stand_in = self
            .tree(tree)
            .and_then(|host| host.node(socket.node))
            .is_some_and(|held| matches!(held.body, NodeBody::StandIn(_)));
        if !is_stand_in {
            return (vec![socket], false);
        }
        (
            stands_for
                .into_iter()
                .map(|member| Socket {
                    node: member,
                    port: socket.port,
                })
                .collect(),
            true,
        )
    }

    /// Where a stand-in's expansion puts more links on a socket than that
    /// socket holds, ascending by authored link.
    ///
    /// ★★★★★ The general form of the reference's *an alias used as a
    /// transition's target must alias a single state*, and it is **derived**:
    /// the expansion multiplies the links at the FAR end of the authored link,
    /// so what decides it is that socket's own [`Multiplicity`], which R1599
    /// already derives from
    /// what the port carries. See this module's header for why that recovers
    /// the reference's rule on the control plane and inverts it on the value
    /// one.
    #[must_use]
    pub fn crowded(&self, tree: TreeId) -> Vec<Crowding> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for link in host.links() {
            for (near, far, side) in [
                (link.from, link.to, Side::Input),
                (link.to, link.from, Side::Output),
            ] {
                let members = self.stands_for(tree, near.node);
                if members.len() < 2 {
                    continue;
                }
                if self.socket_multiplicity(tree, far, side) != Multiplicity::One {
                    continue;
                }
                found.push(Crowding {
                    link: link.id,
                    stand_in: near.node,
                    socket: far,
                    side,
                    would_be: members.len(),
                });
            }
        }
        found.sort_by_key(|held| (held.link.0, held.stand_in.0));
        found
    }

    /// How many links one side of one socket holds, or [`Multiplicity::Many`]
    /// when there is no such port to ask.
    ///
    /// The permissive fall-back is deliberate: a socket that is not there is
    /// [`Document::validate`]'s dangling-link finding, and reporting it a second
    /// time as crowding would be one fault under two names.
    fn socket_multiplicity(&self, tree: TreeId, socket: Socket, side: Side) -> Multiplicity {
        let Some(signature) = self.signature(tree, socket.node) else {
            return Multiplicity::Many;
        };
        let ports = match side {
            Side::Input => &signature.inputs,
            Side::Output => &signature.outputs,
        };
        ports
            .get(socket.port as usize)
            .map_or(Multiplicity::Many, |port| port.flow.multiplicity(side))
    }

    /// Add `member` to what `stand_in` stands for.
    ///
    /// # Errors
    ///
    /// [`StandInError::NotAStandIn`] for a node that stands for nothing,
    /// [`StandInError::StandsForEveryone`] when membership is not a list,
    /// [`StandInError::NestedStandIn`] and [`StandInError::ItsOwnMember`] for
    /// the two members that would make [`Self::stands_for`] recursive, and
    /// [`StandInError::WouldCrowd`] when the wider membership would put more
    /// links on an already-wired socket than it holds.
    pub fn represent(
        &mut self,
        tree: TreeId,
        stand_in: NodeId,
        member: NodeId,
    ) -> Result<(), StandInError> {
        self.check_member(tree, stand_in, member)?;
        let mut wanted = self.named_members(tree, stand_in)?;
        if !wanted.insert(member) {
            return Ok(());
        }
        self.set_membership(tree, stand_in, Represented::Named(wanted))
    }

    /// Take `member` out of what `stand_in` stands for.
    ///
    /// # Errors
    ///
    /// [`StandInError::NotAStandIn`] and [`StandInError::StandsForEveryone`],
    /// as [`Self::represent`]. Narrowing can never crowd, so it has no third
    /// refusal.
    pub fn stop_representing(
        &mut self,
        tree: TreeId,
        stand_in: NodeId,
        member: NodeId,
    ) -> Result<(), StandInError> {
        let mut wanted = self.named_members(tree, stand_in)?;
        if !wanted.remove(&member) {
            return Ok(());
        }
        self.set_membership(tree, stand_in, Represented::Named(wanted))
    }

    /// Make `stand_in` stand for whoever is in the tree.
    ///
    /// The reference's global alias, and the reason it is an arm rather than
    /// *every member enumerated* is that it stays right as nodes arrive and
    /// leave: an enumeration made today is a list that silently stops covering
    /// what a person meant.
    ///
    /// # Errors
    ///
    /// [`StandInError::NotAStandIn`], and [`StandInError::WouldCrowd`] when the
    /// tree's population would crowd a socket this stand-in is already wired
    /// to.
    pub fn represent_everyone(
        &mut self,
        tree: TreeId,
        stand_in: NodeId,
    ) -> Result<(), StandInError> {
        self.expect_stand_in(tree, stand_in)?;
        self.set_membership(tree, stand_in, Represented::Everyone)
    }

    /// Make `stand_in` stand for exactly these, replacing whatever it stood for.
    ///
    /// The way back from [`Self::represent_everyone`], and the way an editor
    /// applies a multiple selection in one edit rather than N.
    ///
    /// # Errors
    ///
    /// As [`Self::represent`], for each member.
    pub fn represent_named(
        &mut self,
        tree: TreeId,
        stand_in: NodeId,
        members: impl IntoIterator<Item = NodeId>,
    ) -> Result<(), StandInError> {
        let wanted: BTreeSet<NodeId> = members.into_iter().collect();
        for member in &wanted {
            self.check_member(tree, stand_in, *member)?;
        }
        self.expect_stand_in(tree, stand_in)?;
        self.set_membership(tree, stand_in, Represented::Named(wanted))
    }

    /// ★★★★★ The reference's operator, generalised: **a stand-in for this one
    /// node, wired back to it.**
    ///
    /// What its self-transition command does, step for step — a stand-in placed
    /// up and to the right of the node, standing for exactly that node, with a
    /// link from the stand-in back to it — and the link's *expansion* is the
    /// self-loop [`Document::connect`] refuses to author directly. The refusal
    /// is right and stays: a node feeding itself with no stand-in in the
    /// picture is a mistake, and this is the declaration that says it was
    /// meant.
    ///
    /// # Errors
    ///
    /// [`StandInError::NestedStandIn`] for a node that is itself one,
    /// [`StandInError::NoSuchNode`], and [`StandInError::NoWayBack`] for a node
    /// whose signature has no output or no input to wire between.
    pub fn stand_in_for(&mut self, tree: TreeId, node: NodeId) -> Result<StoodIn, StandInError> {
        let host = self.tree(tree).ok_or(StandInError::NoSuchTree(tree))?;
        let held = host
            .node(node)
            .ok_or(StandInError::NoSuchNode { tree, node })?;
        if matches!(held.body, NodeBody::StandIn(_)) {
            return Err(StandInError::NestedStandIn {
                tree,
                stand_in: node,
                member: node,
            });
        }
        let (x, y) = (held.x + OVER, held.y - UP);
        let signature = self
            .declared_signature(tree, node)
            .ok_or(StandInError::NoSuchNode { tree, node })?;
        if signature.outputs.is_empty() || signature.inputs.is_empty() {
            return Err(StandInError::NoWayBack { tree, node });
        }
        let stand_in = self
            .add_node(
                tree,
                NodeBody::StandIn(Represented::Named(BTreeSet::from([node]))),
                x,
                y,
            )
            .map_err(|_| StandInError::NoSuchTree(tree))?;
        let made = self
            .connect(tree, Socket::new(stand_in, 0), Socket::new(node, 0))
            .map_err(|_| StandInError::NoWayBack { tree, node })?;
        Ok(StoodIn {
            stand_in,
            link: made.link,
            displaced: made.displaced,
        })
    }

    /// The members a stand-in names, or why it has no list.
    fn named_members(
        &self,
        tree: TreeId,
        stand_in: NodeId,
    ) -> Result<BTreeSet<NodeId>, StandInError> {
        match self.expect_stand_in(tree, stand_in)? {
            Represented::Named(named) => Ok(named.clone()),
            Represented::Everyone => Err(StandInError::StandsForEveryone {
                tree,
                node: stand_in,
            }),
        }
    }

    /// What `stand_in` stands for, or why it is not one.
    fn expect_stand_in(
        &self,
        tree: TreeId,
        stand_in: NodeId,
    ) -> Result<&Represented, StandInError> {
        let host = self.tree(tree).ok_or(StandInError::NoSuchTree(tree))?;
        let held = host.node(stand_in).ok_or(StandInError::NoSuchNode {
            tree,
            node: stand_in,
        })?;
        match &held.body {
            NodeBody::StandIn(represented) => Ok(represented),
            _ => Err(StandInError::NotAStandIn {
                tree,
                node: stand_in,
            }),
        }
    }

    /// The two members that would make [`Self::stands_for`] recursive.
    fn check_member(
        &self,
        tree: TreeId,
        stand_in: NodeId,
        member: NodeId,
    ) -> Result<(), StandInError> {
        if member == stand_in {
            return Err(StandInError::ItsOwnMember {
                tree,
                node: stand_in,
            });
        }
        let host = self.tree(tree).ok_or(StandInError::NoSuchTree(tree))?;
        let held = host
            .node(member)
            .ok_or(StandInError::NoSuchNode { tree, node: member })?;
        if matches!(held.body, NodeBody::StandIn(_)) {
            return Err(StandInError::NestedStandIn {
                tree,
                stand_in,
                member,
            });
        }
        Ok(())
    }

    /// Write a membership, but only if the graph that results is not crowded.
    ///
    /// ★ The check runs on a **scratch document**, so the refusal is decided by
    /// the same derivation a reader would call on the result rather than by a
    /// second rule written to predict it. That is this crate's repeating
    /// finding: a gate that re-spells the law is a second copy of it, free to
    /// drift; a gate that asks the derivation cannot be.
    fn set_membership(
        &mut self,
        tree: TreeId,
        stand_in: NodeId,
        wanted: Represented,
    ) -> Result<(), StandInError> {
        let before = self.crowded(tree);
        let known: BTreeMap<LinkId, Crowding> =
            before.into_iter().map(|held| (held.link, held)).collect();
        let previous = self.expect_stand_in(tree, stand_in)?.clone();
        self.write_membership(tree, stand_in, wanted);
        if let Some(new) = self
            .crowded(tree)
            .into_iter()
            .find(|held| held.stand_in == stand_in && !known.contains_key(&held.link))
        {
            self.write_membership(tree, stand_in, previous);
            return Err(StandInError::WouldCrowd {
                tree,
                stand_in,
                socket: new.socket,
                side: new.side,
                would_be: new.would_be,
            });
        }
        Ok(())
    }

    /// Put a membership in place with no questions asked.
    fn write_membership(&mut self, tree: TreeId, stand_in: NodeId, wanted: Represented) {
        if let Some(held) = self.tree_mut(tree).and_then(|host| host.node_mut(stand_in)) {
            held.body = NodeBody::StandIn(wanted);
        }
    }
}
