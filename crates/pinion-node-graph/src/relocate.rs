//! ★★★★★ R1993 — **every link on one port taken to another port**, moved or
//! copied, and a refusal never costs a link.
//!
//! [`relink`](Document::relink) moves *one end of one link*. This is the same
//! question asked of a whole port: *take everything that is wired here and wire
//! it there instead* — or *as well*. A person re-pinning a node does it; the
//! engine's schema publishes both halves as `MovePinLinks` and `CopyPinLinks`.
//!
//! # What the engine does, measured at its implementation rather than its name
//!
//! Both take a from-pin and a to-pin and return one connection response.
//!
//! * **Move**: it snapshots the from-pin's link list, **breaks every one of
//!   them**, and only then walks the snapshot asking `CanCreateConnection` for
//!   each. Whatever the target admits is re-made on the target.
//! * **Copy**: the same walk with no break, so the source keeps its links.
//! * Both then carry the pin's *default value* across.
//! * Both return a single response, and the loop **overwrites** it on each
//!   failure, so what comes back is the LAST refusal.
//!
//! # The four measured ways this passes it
//!
//! 1. ★★★★★ **A refusal does not cost a link.** The engine breaks first and
//!    asks second, so a link the target will not take is *already gone* — the
//!    graph silently loses an edge and the response says only that something
//!    went wrong. Here a link the target refuses is simply **not moved**: it
//!    stays on the port it was on. This is [`relink`](Document::relink)'s own
//!    argument — see that module's header, *a refusal that also deletes it is
//!    not a refusal* — held at the scale of a whole port.
//! 2. ★★★★★ **Even the links it DOES move survive as themselves.** The
//!    engine's move is break-and-remake, which mints new links: a selection, an
//!    undo entry or a renderer's cache keyed by the old id is holding a
//!    dangling name afterwards. A move here is [`relink`](Document::relink)
//!    per link, so every moved link keeps its [`LinkId`] **and** its
//!    [`muted`](crate::Link::muted) flag.
//! 3. ★★★★★ **It says what happened to each link.** One response for N links
//!    cannot name which of them failed, and being overwritten it does not even
//!    keep the first. [`Relocation::links`] is one verdict per link, in the
//!    order they were tried, each refusal carrying the crate's own typed
//!    reason.
//! 4. **Asking is separable from doing.** [`Document::may_move_links`] and
//!    [`Document::may_copy_links`] answer the whole report without touching the
//!    document, by running the same steps on a copy. The engine has only the
//!    operation, so *what would this do* can be asked only by doing it.
//!
//! # ⚠ What is deliberately absent, and why it is not a gap
//!
//! The engine also carries the pin's **default value** across. Measured in this
//! crate: a default is declared on the port by the node's KIND
//! ([`Flow::Value`](crate::Flow)'s `default`), not held per-socket on an
//! instance, so there is no per-pin literal here to carry anywhere. The engine
//! needs that clause because its pins hold instance data; ours do not. Stated
//! rather than left to look like an omission.
//!
//! # The order is part of the answer
//!
//! Each link is tried against a document that already holds the previous
//! successes, because that is what decides: a target port with room for one
//! more link admits the first and refuses the second, and any answer that
//! pretended otherwise would be wrong for whichever link it looked at second.
//! So the report is **ordered**, ascending by [`LinkId`], and says so — the
//! engine's loop has the same property and never mentions it.

use core::fmt;

use crate::model::{ConnectError, Document, Link, LinkId, NodeId, NodeKind, Side, Socket, TreeId};
use crate::relink::RelinkError;

/// What a plan answers: the report, and the document that would result from
/// applying it.
///
/// Named because both plan functions answer exactly this shape and the pair
/// spelled inline is over this workspace's complexity bound. At module scope
/// rather than inside either function — an item after a statement is its own
/// lint, which is how R1991's identical repair went wrong once already.
type Planned<K, E> = Result<(Relocation<E>, Document<K>), RelocateError>;

/// What happened to one of the links that were on the port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reception<E> {
    /// The target took it.
    ///
    /// `link` is which link that is **now**: for a move it is the id the link
    /// always had — the point of the exercise — and for a copy it is the new
    /// link the copy made, with the original left where it was.
    ///
    /// `displaced` is the link this one replaced on arrival, if the port it
    /// landed on holds one link and already had one. ★★★★★ **It is here
    /// because a value input takes one producer, so a whole-port copy
    /// necessarily replaces on every consumer**, and a report that only said
    /// *taken* would be describing an edit that quietly deleted an edge per
    /// link. Both [`Relinked`](crate::Relinked) and
    /// [`Connected`](crate::Connected) already answer this; dropping it here
    /// would have made the aggregate blinder than the single-link verbs it is
    /// built from. The engine's response has no member for it at all.
    Taken {
        link: LinkId,
        displaced: Option<Link>,
    },
    /// The target would not have it, with the reason.
    ///
    /// ★ **It is still where it was.** The engine has already broken it by the
    /// time it discovers this.
    Refused(E),
}

impl<E> Reception<E> {
    /// Whether the target took it.
    pub const fn taken(&self) -> bool {
        matches!(self, Self::Taken { .. })
    }
}

/// What taking a port's links somewhere else did, or would do.
///
/// Every number here is one the report counted rather than one a reader has to
/// recount — a count that could be recomputed differently is a second copy of
/// the rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relocation<E> {
    /// The port the links were on.
    pub from: Socket,
    /// The port they were asked to go to.
    pub to: Socket,
    /// Which side of a link this is about: [`Side::Output`] for the producing
    /// ends leaving `from`, [`Side::Input`] for the consuming ends arriving at
    /// it.
    pub side: Side,
    /// One verdict per link that was on `from`, ascending by [`LinkId`] —
    /// which is the order they were tried in, and the order matters. Empty when
    /// the port had no links, which is an answer rather than a refusal.
    pub links: Vec<(LinkId, Reception<E>)>,
}

impl<E> Relocation<E> {
    /// How many the target took.
    #[must_use]
    pub fn taken(&self) -> usize {
        self.links.iter().filter(|(_, how)| how.taken()).count()
    }

    /// How many it would not have — each of which is still on `from`.
    #[must_use]
    pub fn refused(&self) -> usize {
        self.links.len() - self.taken()
    }

    /// Whether every link made it.
    ///
    /// ★ True for a port that had nothing on it, which is correct and is the
    /// reason [`links`](Self::links) is published beside this: *everything
    /// moved* and *there was nothing to move* are different facts and a caller
    /// that needs to tell them apart can.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.refused() == 0
    }
}

/// Why a port's links could not be taken anywhere.
///
/// These are the caller's own errors — a tree, a node or a port that is not
/// there, or a port asked to take its own links. A link the target merely
/// refuses is **not** one of these: that is a [`Reception::Refused`] inside the
/// report, because the operation happened and that link did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelocateError {
    /// The tree is not in this document.
    NoSuchTree(TreeId),
    /// One of the two nodes is not in that tree.
    NoSuchNode { tree: TreeId, node: NodeId },
    /// One of the two nodes has no port at that index on that side. The arity
    /// is reported because *which* port was asked for is only meaningful
    /// beside how many there are.
    NoSuchPort {
        node: NodeId,
        side: Side,
        port: u32,
        arity: usize,
    },
    /// The two ports are the same one. Taking a port's links to itself is a
    /// gesture asked for by accident, so it is refused by name rather than
    /// left to answer *nothing moved* — which is what a caller would see, and
    /// is indistinguishable from a port whose links were all refused.
    SamePort(Socket),
}

impl fmt::Display for RelocateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {} in tree {}", node.0, tree.0)
            }
            Self::NoSuchPort {
                node,
                side,
                port,
                arity,
            } => write!(
                f,
                "node {} has no {} port {port} ({arity} there)",
                node.0,
                match side {
                    Side::Input => "accepting",
                    Side::Output => "producing",
                }
            ),
            Self::SamePort(socket) => write!(
                f,
                "port {} of node {} cannot take its own links",
                socket.port, socket.node.0
            ),
        }
    }
}

impl std::error::Error for RelocateError {}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1993 — **what moving this port's links would do**, without doing
    /// it.
    ///
    /// The same steps [`move_links`](Self::move_links) performs, run on a copy,
    /// so the question and the verb cannot answer differently. Asking changes
    /// nothing about the document.
    ///
    /// # Errors
    ///
    /// [`RelocateError`] — the tree, a node or a port is not there, or the two
    /// ports are the same. A link the target refuses is reported inside the
    /// answer, not here.
    pub fn may_move_links(
        &self,
        tree: TreeId,
        side: Side,
        from: Socket,
        to: Socket,
    ) -> Result<Relocation<RelinkError<K::Type>>, RelocateError> {
        self.plan_move(tree, side, from, to)
            .map(|(report, _)| report)
    }

    /// ★★★★★ R1993 — **move every link on one port to another port.**
    ///
    /// Each link keeps its [`LinkId`] and its mute, because each one is moved
    /// by [`relink`](Self::relink) rather than broken and re-made.
    ///
    /// ★ **A link the target will not take stays where it is.** The report says
    /// which, and why. The engine breaks every link before it asks, so the ones
    /// it cannot re-make are gone.
    ///
    /// # Errors
    ///
    /// [`RelocateError`] — exactly what [`may_move_links`](Self::may_move_links)
    /// answers.
    pub fn move_links(
        &mut self,
        tree: TreeId,
        side: Side,
        from: Socket,
        to: Socket,
    ) -> Result<Relocation<RelinkError<K::Type>>, RelocateError> {
        let (report, moved) = self.plan_move(tree, side, from, to)?;
        *self = moved;
        Ok(report)
    }

    /// ★★★★★ R1993 — **what copying this port's links would do**, without
    /// doing it.
    ///
    /// # Errors
    ///
    /// [`RelocateError`], as [`may_move_links`](Self::may_move_links).
    pub fn may_copy_links(
        &self,
        tree: TreeId,
        side: Side,
        from: Socket,
        to: Socket,
    ) -> Result<Relocation<ConnectError<K::Type>>, RelocateError> {
        self.plan_copy(tree, side, from, to)
            .map(|(report, _)| report)
    }

    /// ★★★★★ R1993 — **give another port a copy of every link on this one.**
    ///
    /// The source keeps everything it had. Each copy is a new link — a copy is
    /// a new edge, so a new identity is the honest answer — and
    /// [`Reception::Taken`] names it.
    ///
    /// # Errors
    ///
    /// [`RelocateError`], as [`may_move_links`](Self::may_move_links).
    pub fn copy_links(
        &mut self,
        tree: TreeId,
        side: Side,
        from: Socket,
        to: Socket,
    ) -> Result<Relocation<ConnectError<K::Type>>, RelocateError> {
        let (report, copied) = self.plan_copy(tree, side, from, to)?;
        *self = copied;
        Ok(report)
    }

    /// The links on `socket`'s `side`, ascending — the population both verbs
    /// walk, derived once so the question and the verb cannot disagree about
    /// what *this port's links* means.
    fn links_on(&self, tree: TreeId, side: Side, socket: Socket) -> Vec<LinkId> {
        let mut out: Vec<LinkId> = self
            .tree(tree)
            .into_iter()
            .flat_map(|host| host.links().iter())
            .filter(|link| {
                let end = match side {
                    Side::Output => link.from,
                    Side::Input => link.to,
                };
                end == socket
            })
            .map(|link| link.id)
            .collect();
        out.sort_unstable();
        out
    }

    /// The caller's own errors, answered once for both verbs.
    fn vet_ports(
        &self,
        tree: TreeId,
        side: Side,
        from: Socket,
        to: Socket,
    ) -> Result<(), RelocateError> {
        if self.tree(tree).is_none() {
            return Err(RelocateError::NoSuchTree(tree));
        }
        if from == to {
            return Err(RelocateError::SamePort(from));
        }
        for socket in [from, to] {
            let signature = self
                .signature(tree, socket.node)
                .ok_or(RelocateError::NoSuchNode {
                    tree,
                    node: socket.node,
                })?;
            let arity = match side {
                Side::Input => signature.inputs.len(),
                Side::Output => signature.outputs.len(),
            };
            if usize::try_from(socket.port).unwrap_or(usize::MAX) >= arity {
                return Err(RelocateError::NoSuchPort {
                    node: socket.node,
                    side,
                    port: socket.port,
                    arity,
                });
            }
        }
        Ok(())
    }

    /// The one decision behind [`may_move_links`](Self::may_move_links) and
    /// [`move_links`](Self::move_links), taken on a copy.
    fn plan_move(
        &self,
        tree: TreeId,
        side: Side,
        from: Socket,
        to: Socket,
    ) -> Planned<K, RelinkError<K::Type>> {
        self.vet_ports(tree, side, from, to)?;
        let mut trying = self.clone();
        let mut links = Vec::new();
        for link in self.links_on(tree, side, from) {
            // ★ Applied to `trying` as it goes, so each link is judged against
            // the document the previous ones have already changed. A refused
            // link is simply not relinked, which leaves it on `from`.
            let how = match trying.relink(tree, link, side, to) {
                Ok(moved) => Reception::Taken {
                    link,
                    displaced: moved.displaced,
                },
                Err(why) => Reception::Refused(why),
            };
            links.push((link, how));
        }
        Ok((
            Relocation {
                from,
                to,
                side,
                links,
            },
            trying,
        ))
    }

    /// The one decision behind [`may_copy_links`](Self::may_copy_links) and
    /// [`copy_links`](Self::copy_links), taken on a copy.
    fn plan_copy(
        &self,
        tree: TreeId,
        side: Side,
        from: Socket,
        to: Socket,
    ) -> Planned<K, ConnectError<K::Type>> {
        self.vet_ports(tree, side, from, to)?;
        let mut trying = self.clone();
        let mut links = Vec::new();
        for link in self.links_on(tree, side, from) {
            // The end that is NOT moving, read off the original so a copy made
            // a moment ago cannot be mistaken for one of the originals.
            let Some(held) = self
                .tree(tree)
                .and_then(|host| host.links().iter().find(|l| l.id == link).copied())
            else {
                continue;
            };
            let (a, b) = match side {
                Side::Output => (to, held.to),
                Side::Input => (held.from, to),
            };
            let how = match trying.connect(tree, a, b) {
                Ok(made) => Reception::Taken {
                    link: made.link,
                    displaced: made.displaced,
                },
                Err(why) => Reception::Refused(why),
            };
            links.push((link, how));
        }
        Ok((
            Relocation {
                from,
                to,
                side,
                links,
            },
            trying,
        ))
    }
}
