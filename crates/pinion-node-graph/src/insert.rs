//! ★★★★★ R1992 — **a node dropped onto a standing wire is spliced into it, and
//! the row makes room for what arrived.**
//!
//! Two capabilities, built together because neither is usable alone: the splice
//! puts a node between two others, and the shove moves the neighbours apart so
//! the card that arrived is not drawn on top of them.
//!
//! # What the reference does, measured at its implementation rather than at its
//! name
//!
//! The DCC's operator is called *automatically offset nodes on insertion*, and
//! R1987 corrected this project's census row for it after finding the covering
//! sentence false in both clauses — it had been equated with the engine's
//! autowire hook, which is a different gesture entirely (see [`crate::autowire`]).
//! What was left absent is what the operator actually is. Measured at its body:
//!
//! * It holds a **prev**, an **insert** and a **next**, and a flag saying which
//!   way the row reads.
//! * It takes the gap either side — `insert.left - prev.right` and
//!   `next.left - insert.right` — and compares them against one margin read
//!   from a preference.
//! * The **anchored** side being tight moves the inserted node itself. The
//!   **growing** side being tight, *or* the two gaps together being under twice
//!   the margin, moves the growing side away.
//! * What moves on that side is not the one neighbour: it walks the tree in
//!   topological order and propagates a mask through valid, visible links, so
//!   the **whole cone** beyond the neighbour travels with it.
//! * It wires nothing. It runs *after* a splice another operator performed.
//!
//! # The four measured ways this passes it
//!
//! 1. ★★★★★ **It says what it moved.** The reference answers a `bool` and
//!    writes each distance into a per-node runtime field an animation reads;
//!    no member of it reports the set, and nothing can ask afterwards which
//!    nodes travelled or how far. [`Room::shoved`] is that list, and
//!    [`Room::shift`] is the inserted node's own distance beside it.
//! 2. ★★★★★ **It says why it did not move, when it did not.** The reference's
//!    `false` covers *both gaps were already clear* and nothing else — one
//!    value for a verdict that has four cases. [`Room::verdict`] names which.
//! 3. **Asking is separable from doing.** [`Document::room_for`] answers
//!    without touching the document, and [`Document::make_room_for`] applies
//!    exactly what it answered. The reference has only the operator, so *would
//!    this move anything* can be asked only by moving it.
//! 4. **The splice is a verb of its own, for an arbitrary node.** Before this
//!    module the tree could splice only reroute bodies
//!    ([`Document::insert_reroutes`]), and the reference reaches its own splice
//!    from a drag handler rather than publishing one.
//!
//! ⚠ **The animation is deliberately not here.** The reference spends a modal
//! operator and a quarter-second easing on the move; that is a screen's
//! business, and this crate publishes the distances a screen would ease along.
//!
//! # The cycle the reference guards by ordering, and this guards by shape
//!
//! Its own comment says that in a graph with a cycle the inserted node can
//! appear in its own propagation mask, so it writes the insert's offset *after*
//! the walk to make sure the insert's value wins. That is a correct fix kept
//! alive by the order of two statements. Here the inserted node is simply not a
//! member of [`Room::shoved`] — its distance has a field of its own — so the
//! two cannot collide whatever the graph looks like.

use core::fmt;

use crate::autowire::{AutowireError, Uptake};
use crate::layout::Extent;
use crate::model::{Document, LinkId, Node, NodeId, NodeKind, Side, Socket, TreeId};
use crate::select::{Grow, Reach};

/// Which way a row is allowed to widen — the side that gives ground, and so
/// the side the cone travels on.
///
/// The reference spells this as one `right_alignment` boolean threaded through
/// four expressions. Named arms instead, because a reader of a call site should
/// not have to know which way `true` points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Widening {
    /// Producers on the left, consumers on the right — the ordinary node
    /// editor. What has to move moves **right**, and it is the consuming cone.
    Rightward,
    /// The mirror: what has to move moves **left**, and it is the producing
    /// cone.
    Leftward,
}

impl Widening {
    /// The sign a distance along this widening carries on the canvas x axis.
    const fn sign(self) -> i32 {
        match self {
            Self::Rightward => 1,
            Self::Leftward => -1,
        }
    }

    /// Which way the cone that moves is reached from the neighbour it starts
    /// at.
    const fn cone(self) -> Grow {
        match self {
            Self::Rightward => Grow::Downstream(Reach::Transitive),
            Self::Leftward => Grow::Upstream(Reach::Transitive),
        }
    }
}

/// What making room had to do.
///
/// Derived once, from the same two conditions that drive the distances, so a
/// reader of the arm and a reader of the numbers cannot be told different
/// things. The reference publishes a `bool`, which is this with three of its
/// four arms collapsed into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Both gaps already cleared the margin and together cleared twice it.
    /// Nothing moved.
    Clear,
    /// Only the inserted node moved, along the widening.
    Shifted,
    /// Only the growing side moved, taking its cone with it.
    Shoved,
    /// Both.
    ShiftedAndShoved,
}

impl Verdict {
    /// Whether anything moved at all — the reference's whole answer.
    #[must_use]
    pub const fn moved(self) -> bool {
        !matches!(self, Self::Clear)
    }
}

/// What a row did to make room, and what it measured to decide.
///
/// Every number here is the one the decision was taken on, published beside the
/// decision rather than left to be recomputed — a report that could be
/// recomputed differently is a second copy of the rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    /// The node this room was made for.
    pub inserted: NodeId,
    /// The two nodes the gaps were measured against: the one behind along the
    /// widening, then the one ahead. The reference is handed these and never
    /// says which they were.
    pub between: (NodeId, NodeId),
    /// The gap on the **anchored** side, before anything moved.
    pub behind: i32,
    /// The gap on the **growing** side, before anything moved.
    pub ahead: i32,
    /// What those two were compared against.
    pub margin: i32,
    /// How far the inserted node itself moves, signed along the canvas x axis.
    /// Zero when it stays.
    pub shift: i32,
    /// Every other node that moves, ascending by id, each with its signed
    /// distance along the canvas x axis. ★ The capability the reference has no
    /// member for.
    pub shoved: Vec<(NodeId, i32)>,
    /// Which of the four cases this was.
    pub verdict: Verdict,
}

impl Room {
    /// Where a node ends up, or `None` for one this did not move.
    ///
    /// The inserted node is reachable here as well as through
    /// [`shift`](Self::shift), so a caller applying the report does not have to
    /// special-case it.
    #[must_use]
    pub fn distance(&self, node: NodeId) -> Option<i32> {
        if node == self.inserted {
            return (self.shift != 0).then_some(self.shift);
        }
        self.shoved
            .iter()
            .find_map(|&(id, by)| (id == node).then_some(by))
    }
}

/// What a splice decided, asked before anything moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    /// The inserted node's port that will take the incoming wire.
    pub took: Uptake,
    /// The inserted node's port that will give the outgoing one.
    pub gave: Uptake,
    /// The producer the wire came from and the consumer it goes to — the
    /// `prev` and `next` the shove then measures against.
    pub between: (NodeId, NodeId),
}

/// What a splice did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spliced {
    /// The link that was **already there**. It keeps its identity and its
    /// consuming end; only its producing end moved onto the inserted node. A
    /// caller holding this id still holds the same link, and an undo has one
    /// thing to put back — the same choice [`Document::insert_reroutes`] made.
    pub kept: LinkId,
    /// The link made from the original producer to the inserted node.
    pub fed: LinkId,
    /// The decision this acted on, unchanged.
    pub splice: Splice,
}

/// Why a node could not be spliced onto a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpliceError<T> {
    /// The tree is not in this document.
    NoSuchTree(TreeId),
    /// The link is not in that tree.
    NoSuchLink { tree: TreeId, link: LinkId },
    /// The node is not in that tree.
    NoSuchNode { tree: TreeId, node: NodeId },
    /// The node is already one of that link's two ends. Splicing it onto its
    /// own wire is a self-loop asked for by accident, so it is refused by name
    /// rather than left to the cycle check — which would answer, but would
    /// blame the cycle instead of the gesture.
    AlreadyAnEnd { node: NodeId, link: LinkId },
    /// No port of the node would take the incoming wire, with the reason.
    NoIntake(AutowireError<T>),
    /// No port of the node would give the outgoing wire, with the reason.
    NoOuttake(AutowireError<T>),
    /// A write both vets had already admitted was refused anyway.
    ///
    /// ⚠ **A stated limit rather than a reachable refusal.** Both pairs are
    /// vetted before either is written, and for a document whose links are
    /// acyclic no test can reach this — which is said here instead of being
    /// hidden behind an arm that reads as if it happened. It is kept because
    /// the copy the verb writes to is what makes *a refusal leaves nothing
    /// behind* a property of the construction: a rule added to the vet later
    /// cannot turn this verb into one that leaves half a splice standing.
    Unvetted { link: LinkId, node: NodeId },
}

impl<T: fmt::Debug> fmt::Display for SpliceError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchLink { tree, link } => {
                write!(f, "no link {} in tree {}", link.0, tree.0)
            }
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {} in tree {}", node.0, tree.0)
            }
            Self::AlreadyAnEnd { node, link } => {
                write!(f, "node {} is already an end of link {}", node.0, link.0)
            }
            Self::NoIntake(why) => write!(f, "nothing takes the incoming wire: {why:?}"),
            Self::NoOuttake(why) => write!(f, "nothing gives the outgoing wire: {why:?}"),
            Self::Unvetted { link, node } => write!(
                f,
                "a vetted write was refused splicing node {} onto link {}",
                node.0, link.0
            ),
        }
    }
}

impl<T: fmt::Debug> std::error::Error for SpliceError<T> {}

/// Why a row could not be asked to make room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomError {
    /// The tree is not in this document.
    NoSuchTree(TreeId),
    /// The node is not in that tree.
    NoSuchNode { tree: TreeId, node: NodeId },
    /// The node is not between two others along the widening: making room is a
    /// property of something that arrived **into a row**, and a node with
    /// nothing on one side of it has no gap there to measure. The counts say
    /// which side was empty.
    ///
    /// They count the neighbours that are **drawn**, because the question is
    /// geometric: a linked neighbour with no box on this canvas cannot be a
    /// side of a gap, and reporting it as one would name a number no measurement
    /// used.
    NotInARow {
        node: NodeId,
        producers: usize,
        consumers: usize,
    },
    /// The node itself has no drawn box, so it has no edges for a gap to be
    /// measured from.
    NotDrawn { node: NodeId },
}

impl fmt::Display for RoomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {} in tree {}", node.0, tree.0)
            }
            Self::NotInARow {
                node,
                producers,
                consumers,
            } => write!(
                f,
                "node {} is not in a row: {producers} drawn producer(s), {consumers} drawn consumer(s)",
                node.0
            ),
            Self::NotDrawn { node } => write!(f, "node {} has no drawn box", node.0),
        }
    }
}

impl std::error::Error for RoomError {}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1992 — **which ports would take a node onto this wire**, asked
    /// before anything moves.
    ///
    /// The same decision [`insert_on_link`](Self::insert_on_link) acts on, made
    /// by the same call, so the question and the verb cannot answer
    /// differently. Asking changes nothing about the document.
    ///
    /// Both ends are chosen through [`may_autowire`](Self::may_autowire), so a
    /// port this admits is a port `connect` admits and the preference order is
    /// the one the rest of the crate already uses.
    ///
    /// # Errors
    ///
    /// [`SpliceError`] — the tree, link or node is not there, the node is
    /// already an end of the link, or one of the two sides has no port that
    /// would take its wire, each with the reason.
    pub fn may_insert_on_link(
        &self,
        tree: TreeId,
        link: LinkId,
        node: NodeId,
    ) -> Result<Splice, SpliceError<K::Type>> {
        self.plan_splice(tree, link, node)
            .map(|(splice, _, _)| splice)
    }

    /// ★★★★★ R1992 — **splice a node onto a standing wire.**
    ///
    /// The wire's producing end moves onto the node, and a new link feeds the
    /// node from where that end used to be. The standing link keeps its
    /// identity and its consuming end.
    ///
    /// ★ The whole thing happens to a copy and `self` is not touched until the
    /// last line, so a refusal on either half is a refusal that changed
    /// nothing — by construction rather than by an unwind somebody has to
    /// remember to write. The reference reaches its splice from a drag handler
    /// and has no such guarantee.
    ///
    /// ⚠ **Stated rather than implied:** every refusal a test can reach today
    /// happens *before* the copy is taken, because both pairs are vetted before
    /// either is written — see [`SpliceError::Unvetted`]. The copy is what
    /// keeps the guarantee true if a later rule makes one of the two writes
    /// refuse; it is not repairing a hole that exists now.
    ///
    /// Wiring is all this does. Moving the neighbours apart afterwards is
    /// [`make_room_for`](Self::make_room_for), which is the same separation the
    /// reference has — its offset operator wires nothing and runs after.
    ///
    /// # Errors
    ///
    /// [`SpliceError`] — exactly what
    /// [`may_insert_on_link`](Self::may_insert_on_link) answers.
    pub fn insert_on_link(
        &mut self,
        tree: TreeId,
        link: LinkId,
        node: NodeId,
    ) -> Result<Spliced, SpliceError<K::Type>> {
        let (splice, fed, trying) = self.plan_splice(tree, link, node)?;
        *self = trying;
        Ok(Spliced {
            kept: link,
            fed,
            splice,
        })
    }

    /// The one decision behind the question and the verb, taken on a copy.
    ///
    /// Returns the decision, the id the feeding link took, and the document
    /// that resulted — so the verb applies what was decided rather than
    /// re-deciding, and the question is answered by the same steps that would
    /// have run.
    fn plan_splice(
        &self,
        tree: TreeId,
        link: LinkId,
        node: NodeId,
    ) -> Result<(Splice, LinkId, Self), SpliceError<K::Type>> {
        let host = self.tree(tree).ok_or(SpliceError::NoSuchTree(tree))?;
        let held = *host
            .links()
            .iter()
            .find(|standing| standing.id == link)
            .ok_or(SpliceError::NoSuchLink { tree, link })?;
        if host.node(node).is_none() {
            return Err(SpliceError::NoSuchNode { tree, node });
        }
        if held.from.node == node || held.to.node == node {
            return Err(SpliceError::AlreadyAnEnd { node, link });
        }

        // Which of the node's outputs gives the wire on to the consumer, asked
        // while the standing link is still in place: the consuming end never
        // moves, so what this vets is what will be there.
        let gave = self
            .may_autowire(tree, held.to, Side::Input, node)
            .map_err(SpliceError::NoOuttake)?;
        // Which of its inputs takes the wire from the producer.
        let took = self
            .may_autowire(tree, held.from, Side::Output, node)
            .map_err(SpliceError::NoIntake)?;

        let mut trying = self.clone();
        // The producing end first: after this the consumer is fed by the node
        // and the producer feeds nothing, so the second step cannot close a
        // cycle through a wire that is on its way out.
        trying
            .relink(tree, link, Side::Output, Socket::new(node, gave.port))
            .map_err(|_| SpliceError::Unvetted { link, node })?;
        let fed = trying
            .connect(tree, held.from, Socket::new(node, took.port))
            .map_err(|_| SpliceError::Unvetted { link, node })?
            .link;

        Ok((
            Splice {
                took,
                gave,
                between: (held.from.node, held.to.node),
            },
            fed,
            trying,
        ))
    }

    /// ★★★★★ R1992 — **what the row would have to do to make room for a node**,
    /// without doing it.
    ///
    /// `margin` is the clearance a card wants either side, in the units
    /// [`Node::x`] is already in — the reference reads one from a preference,
    /// and whose preference it is belongs to the application. A margin of zero
    /// or less lets neighbours touch, which is a legitimate setting rather than
    /// a refusal: the conditions simply never fire.
    ///
    /// `box_of` says where each card is **drawn** and how big — the same
    /// callback shape [`Fit::selection`](crate::Fit::selection) takes, and for
    /// the reason R1991 recorded there: a card's box on a canvas is painted, so
    /// its drawn position is not always [`Node::x`] and a pass handed only an
    /// extent would measure gaps between rectangles nobody sees. `None` is a
    /// card this canvas does not draw.
    ///
    /// The two neighbours are **derived**, not given: the producer whose
    /// trailing edge is nearest behind and the consumer whose leading edge is
    /// nearest ahead. The reference is handed the pair by the splice that just
    /// ran and can therefore only ever answer for a node it inserted itself.
    ///
    /// # Errors
    ///
    /// [`RoomError`] — the tree or node is not there, the node is not drawn, or
    /// it has no drawn neighbour on one of its two sides.
    pub fn room_for(
        &self,
        tree: TreeId,
        node: NodeId,
        widening: Widening,
        margin: i32,
        box_of: impl Fn(&Node<K>) -> Option<((i32, i32), Extent)>,
    ) -> Result<Room, RoomError> {
        let host = self.tree(tree).ok_or(RoomError::NoSuchTree(tree))?;
        if host.node(node).is_none() {
            return Err(RoomError::NoSuchNode { tree, node });
        }
        let span = |id: NodeId| -> Option<(i32, i32)> {
            let held = host.node(id)?;
            let ((x, _), size) = box_of(held)?;
            Some((x, x.saturating_add(size.width.max(0))))
        };
        let (left, right) = span(node).ok_or(RoomError::NotDrawn { node })?;

        // Both sides, by the wires the node actually has. A node linked to the
        // same neighbour twice contributes it once — this is about geometry.
        let mut producers: Vec<NodeId> = Vec::new();
        let mut consumers: Vec<NodeId> = Vec::new();
        for standing in host.links() {
            if standing.to.node == node && standing.from.node != node {
                producers.push(standing.from.node);
            }
            if standing.from.node == node && standing.to.node != node {
                consumers.push(standing.to.node);
            }
        }
        producers.sort_unstable();
        producers.dedup();
        consumers.sort_unstable();
        consumers.dedup();
        // The tightest gap either side is what a card is drawn on top of, so it
        // is what decides. Ties keep the lower id, which is this crate's own
        // tiebreak everywhere else. Only drawn neighbours take part, and the
        // refusal counts those — see [`RoomError::NotInARow`].
        let behind_side: Vec<(NodeId, i32)> = producers
            .iter()
            .filter_map(|&id| span(id).map(|(_, far)| (id, left - far)))
            .collect();
        let ahead_side: Vec<(NodeId, i32)> = consumers
            .iter()
            .filter_map(|&id| span(id).map(|(near, _)| (id, near - right)))
            .collect();
        let not_in_a_row = RoomError::NotInARow {
            node,
            producers: behind_side.len(),
            consumers: ahead_side.len(),
        };
        let (behind_node, behind) = behind_side
            .iter()
            .copied()
            .min_by_key(|&(id, gap)| (gap, id))
            .ok_or_else(|| not_in_a_row.clone())?;
        let (ahead_node, ahead) = ahead_side
            .iter()
            .copied()
            .min_by_key(|&(id, gap)| (gap, id))
            .ok_or(not_in_a_row)?;

        // The widening decides which of the two is anchored and which gives
        // ground.
        let (anchored, growing, start) = match widening {
            Widening::Rightward => (behind, ahead, ahead_node),
            Widening::Leftward => (ahead, behind, behind_node),
        };
        let move_insert = anchored < margin;
        let move_side = growing < margin || anchored.saturating_add(growing) < margin * 2;
        let sign = widening.sign();
        let shift = if move_insert {
            sign * (margin - anchored)
        } else {
            0
        };
        let mut shoved = Vec::new();
        if move_side {
            let by = sign * (margin - growing) + shift;
            // The whole cone beyond the neighbour, so nothing it feeds is left
            // behind on top of what moved. `grow` includes the seed.
            //
            // ⚠ **The fallback is a stated limit, not a case that happens.**
            // `start` is one of the two neighbours, and both were resolved out
            // of this tree a few lines up, so `grow` has no way to refuse them.
            // It is the neighbour alone rather than an early return because a
            // row that moved one card is still a truthful, smaller answer,
            // where refusing outright would lose a shove nobody asked about.
            let cone = self
                .grow(tree, &[start], widening.cone())
                .map_or_else(|_| vec![start], |grown| grown.selection);
            shoved = cone
                .into_iter()
                // ★ The inserted node is never a member, whatever the graph
                // looks like — see the module header on the cycle the reference
                // guards by statement order.
                .filter(|&id| id != node)
                .map(|id| (id, by))
                .collect();
            shoved.sort_unstable();
        }
        let verdict = match (shift != 0, !shoved.is_empty()) {
            (false, false) => Verdict::Clear,
            (true, false) => Verdict::Shifted,
            (false, true) => Verdict::Shoved,
            (true, true) => Verdict::ShiftedAndShoved,
        };
        Ok(Room {
            inserted: node,
            between: (behind_node, ahead_node),
            behind,
            ahead,
            margin,
            shift,
            shoved,
            verdict,
        })
    }

    /// ★★★★★ R1992 — **move the neighbours apart to make room for a node**, and
    /// say what moved.
    ///
    /// Applies exactly what [`room_for`](Self::room_for) answered, and answers
    /// it back, so the record of the move is the move. Only the canvas x axis
    /// changes: the reference offsets one axis too, because a row is an axis.
    ///
    /// # Errors
    ///
    /// [`RoomError`] — exactly what [`room_for`](Self::room_for) answers.
    pub fn make_room_for(
        &mut self,
        tree: TreeId,
        node: NodeId,
        widening: Widening,
        margin: i32,
        box_of: impl Fn(&Node<K>) -> Option<((i32, i32), Extent)>,
    ) -> Result<Room, RoomError> {
        let room = self.room_for(tree, node, widening, margin, box_of)?;
        if let Some(host) = self.tree_mut(tree) {
            if room.shift != 0 {
                if let Some(held) = host.node_mut(node) {
                    held.x = held.x.saturating_add(room.shift);
                }
            }
            for &(id, by) in &room.shoved {
                if let Some(held) = host.node_mut(id) {
                    held.x = held.x.saturating_add(by);
                }
            }
        }
        Ok(room)
    }
}
