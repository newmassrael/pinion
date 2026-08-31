//! ★★★★★ R1930 — **releasing a wire on a node's BODY**: the node grows a port
//! and the end lands on it, in one act that either happens or leaves nothing
//! behind.
//!
//! # What the reference does, measured at its header, its ONE consumer and both
//! overriders
//!
//! Its schema publishes a pair: *may a pin be dropped on this node* (a bool with
//! an out-parameter) and *drop it* (which answers the pin it made). The one
//! consumer is the drag handler: while the hand hovers it asks the question and
//! shows whatever the out-parameter holds; when the hand lets go it calls the
//! verb, and then **separately** asks the schema to connect the dragged pin to
//! the pin that came back.
//!
//! Three findings shaped this module, and each contradicts something:
//!
//! 1. ⚠ **The census's covering sentence was half false, in the direction that
//!    hides work.** It read *dragging a pin onto a node and having the node grow
//!    one*, and growing a port is not what is absent —
//!    [`insert_item`](Document::insert_item) has done it since R1632. What is
//!    absent is the DROP AS ONE ACT.
//! 2. ★★★★★ **The reference's own drop is NOT atomic.** It creates the pin, and
//!    the consumer then attempts the connection; if that connection is refused,
//!    the pin it made is still there. A person who dropped a wire on a node and
//!    was refused is left with a port nobody asked for.
//! 3. 🟥 **The question's out-parameter is not an error channel, whatever its
//!    header says.** Documented as *only filled with an error if there is pin
//!    add support but there is an error with the pin type*, the implementation
//!    fills it on SUCCESS too, with the sentence the hover displays. Its own doc
//!    comment and its own code disagree about what that argument carries.
//!
//! # The three measured ways this passes it
//!
//! 1. **It is one act, and a refusal leaves the document untouched.** Not a
//!    promise kept by careful ordering: [`Document::land`] does the whole thing
//!    to a COPY (§2 #3's dry run) and only then takes it, so *nothing was
//!    changed* is a property of the construction rather than of an undo path
//!    somebody has to remember to write. The reference has no undo path at all,
//!    and this repository's own node lab had one — open a slot, relink, and
//!    close the slot again on the way out.
//! 2. ★★★★★ **The question answers WHAT WOULD HAPPEN, as a type.** [`Landfall`]
//!    says *an existing port takes it* or *the node grows one*, which is the
//!    distinction the reference spells with three different hard-coded strings
//!    in the argument its header says is for errors. A screen reads the arm; it
//!    does not parse a sentence.
//! 3. **The question IS the first half of the act.** [`Document::land`] calls
//!    the same planner [`Document::may_land`] does, so the two cannot answer
//!    differently — R1924's rule, which that round learned by shipping a canvas
//!    that said *this card will take it* and then refused the drop.
//!
//! # What this is NOT
//!
//! [`Document::relink`] moves an end to a **socket** — a port that already
//! exists. That is a different gesture with a different vocabulary, and every
//! refusal here that is about the wire itself is [`RelinkError`] carried whole,
//! because a wire refused for a type mismatch and a wire refused for a cycle are
//! fixed by different actions whichever gesture asked.

use core::fmt;

use crate::items::Item;
use crate::model::{Document, LinkId, Multiplicity, NodeId, NodeKind, Side, Socket, TreeId};
use crate::relink::{RelinkError, Relinked};

/// What releasing a wire on a node's body would do.
///
/// Two arms and not a bool, because a screen has two different things to say —
/// *this port takes it* and *a port will appear for it* — and the reference
/// spells that difference as three hard-coded strings in an out-parameter its
/// own header documents as an error channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landfall {
    /// A port that is already there takes the end.
    Takes(Socket),
    /// The node grows a port, and the end lands on it.
    ///
    /// The socket is where that port WILL be, so a caller can say which pin is
    /// about to appear rather than only that one will.
    Grows(Socket),
}

impl Landfall {
    /// Where the end ends up, either way.
    #[must_use]
    pub const fn socket(self) -> Socket {
        match self {
            Self::Takes(socket) | Self::Grows(socket) => socket,
        }
    }

    /// Whether a port has to appear for this to happen.
    #[must_use]
    pub const fn is_new(self) -> bool {
        matches!(self, Self::Grows(_))
    }
}

/// Why a wire cannot be released on that node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandError<T> {
    /// No such tree.
    NoSuchTree(TreeId),
    /// No such link in that tree.
    NoSuchLink {
        /// The tree.
        tree: TreeId,
        /// The link that is not in it.
        link: LinkId,
    },
    /// No such node in that tree.
    NoSuchNode {
        /// The tree.
        tree: TreeId,
        /// The node that is not in it.
        node: NodeId,
    },
    /// The node has no port free for the end and cannot grow one.
    ///
    /// A distinct arm from [`Refused`](Self::Refused) because the two are fixed
    /// by different actions: this one is answered by declaring a variadic run or
    /// by freeing a port, and that one by changing the wire.
    NoRoom {
        /// The node the wire was released on.
        node: NodeId,
        /// Which side of it the end wanted.
        side: Side,
    },
    /// A port was available and the graph still would not hold the wire there.
    ///
    /// Carries the relink refusal whole — a type mismatch and a cycle are
    /// different problems and this is the difference.
    Refused(RelinkError<T>),
}

impl<T: fmt::Debug> fmt::Display for LandError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {tree}"),
            Self::NoSuchLink { tree, link } => write!(f, "tree {tree} has no link {link}"),
            Self::NoSuchNode { tree, node } => write!(f, "tree {tree} has no node {node}"),
            // ⚠ R1930 — the one arm whose sentence a SCREEN is expected to
            // replace, and it says so here rather than leaving that to be
            // discovered. Every other arm names something the crate knows
            // better than any caller (a type, an arity, the path a wire would
            // close); this one names only a node id and a side, and an
            // application that has a NAME for that card can say it far better.
            // The first walk to read this one wrote `node 3` where the screen
            // had `Q-01` ready — R1924's class, one level down.
            Self::NoRoom { node, side } => write!(
                f,
                "node {node} has no free {} port and cannot grow one",
                match side {
                    Side::Input => "input",
                    Side::Output => "output",
                }
            ),
            // ★ The refusal SAYS ITSELF, for R1924's reason: the first hand to
            // read one of these is a hand mid-drag, and `{:?}` put
            // `SelfLink(NodeId(4))` in front of a person on that round.
            Self::Refused(why) => write!(f, "{why}"),
        }
    }
}

impl<T: fmt::Debug> std::error::Error for LandError<T> {}

/// What a landing did.
#[derive(Debug, Clone, PartialEq)]
pub struct Landed {
    /// Which of the two things happened, and where.
    pub fall: Landfall,
    /// The move itself, under the link's own id.
    pub relinked: Relinked,
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1930 — **what releasing `link`'s `end` on `node` would do**, asked
    /// while the hand still holds the wire.
    ///
    /// Not a prediction of [`land`](Self::land): it is the same planner
    /// [`land`](Self::land) runs, so a hand that asked and a hand that just
    /// dropped cannot be told different things. Asking changes nothing.
    ///
    /// The answer prefers a port that is already free and grows one only when
    /// none is — the order a person expects, and the one that does not litter a
    /// node with ports every time a wire is re-aimed onto it.
    ///
    /// # Errors
    ///
    /// [`LandError`] — exactly what [`land`](Self::land) would answer.
    pub fn may_land(
        &self,
        tree: TreeId,
        link: LinkId,
        end: Side,
        node: NodeId,
    ) -> Result<Landfall, LandError<K::Type>> {
        self.plan_landing(tree, link, end, node)
    }

    /// ★★★★★ R1930 — **release `link`'s `end` on `node`**: an existing port takes
    /// it, or the node grows one and it lands there.
    ///
    /// `item` describes the port that would be grown — a label and a socket type
    /// the application chooses — and is ignored when an existing port takes the
    /// end instead. One argument rather than two verbs, because *drop this wire
    /// here* is one intention and which of the two happens is the model's answer
    /// rather than the caller's.
    ///
    /// ★★★★★ **A refusal leaves this document equal to what it was.** The whole
    /// thing is done to a copy and taken only on success, so that is a property
    /// of the construction: there is no undo path to write, to forget, or to get
    /// wrong. The reference has none — its consumer creates the pin and then
    /// attempts the connection, and a refused connection leaves the pin behind.
    ///
    /// # Errors
    ///
    /// [`LandError`] — and nothing has been changed when it is returned.
    pub fn land(
        &mut self,
        tree: TreeId,
        link: LinkId,
        end: Side,
        node: NodeId,
        item: Item<K::Type>,
    ) -> Result<Landed, LandError<K::Type>> {
        let fall = self.plan_landing(tree, link, end, node)?;
        // ★ The copy IS the atomicity. Everything below happens to it and
        // `self` is not touched until the last line, so a refusal on any step is
        // a refusal that changed nothing — by construction, rather than by an
        // unwind somebody has to remember to write.
        let mut trying = self.clone();
        if fall.is_new() {
            let at = next_ordinal(&trying, tree, node, end);
            trying
                .insert_item(tree, node, end, at, item)
                .map_err(|_| LandError::NoRoom { node, side: end })?;
        }
        let relinked = trying
            .relink(tree, link, end, fall.socket())
            .map_err(LandError::Refused)?;
        *self = trying;
        Ok(Landed { fall, relinked })
    }

    /// The landing, decided once and used by both halves.
    ///
    /// ★ The `Grows` answer is checked on a COPY, because the port it names does
    /// not exist yet and [`may_relink`](Self::may_relink) can only be asked
    /// about a port that does — §2 #3's dry run, and the reason a screen no
    /// longer has to open a slot in the real document to find out whether it may.
    fn plan_landing(
        &self,
        tree: TreeId,
        link: LinkId,
        end: Side,
        node: NodeId,
    ) -> Result<Landfall, LandError<K::Type>> {
        let host = self.tree(tree).ok_or(LandError::NoSuchTree(tree))?;
        if !host.links().iter().any(|held| held.id == link) {
            return Err(LandError::NoSuchLink { tree, link });
        }
        host.node(node)
            .ok_or(LandError::NoSuchNode { tree, node })?;

        if let Some(socket) = self.free_port_for(tree, link, end, node) {
            return Ok(Landfall::Takes(socket));
        }

        let grown = self
            .grown_socket(tree, node, end)
            .ok_or(LandError::NoRoom { node, side: end })?;
        let mut trying = self.clone();
        trying
            .insert_item(
                tree,
                node,
                end,
                next_ordinal(self, tree, node, end),
                Item::plain(),
            )
            .map_err(|_| LandError::NoRoom { node, side: end })?;
        trying
            .may_relink(tree, link, end, grown)
            .map_err(LandError::Refused)?;
        Ok(Landfall::Grows(grown))
    }

    /// A port of `node` on `end` that has ROOM for this link's end right now.
    ///
    /// ⚠⚠ **Two questions, and asking only the first is a defect this round
    /// walked into and its own proof caught.** [`may_relink`](Self::may_relink)
    /// admits a move onto a port that is already full, because a value input
    /// takes one producer and a second arrival **displaces** the first — that is
    /// a legal relink and it is reported in `Relinked::displaced`. So a search
    /// that used it alone found every node "already has a free port",
    /// [`Landfall::Grows`] was unreachable, and the capability this module
    /// exists for could never have fired. Measured, not reasoned: the proof
    /// asserted that a full node grows one and was answered `Takes`.
    ///
    /// Room is asked of the port's own [`Multiplicity`] — a `One` port is free
    /// when nothing lands on it, a `Many` port always has room — and only then
    /// is the wire itself put to [`may_relink`](Self::may_relink). So *would
    /// take it* still means what it means everywhere else, and this adds the
    /// half relink deliberately does not ask.
    ///
    /// The port the end is already on is skipped: landing a wire where it
    /// already is must not read as an existing port taking it, or a re-drop on
    /// the same card would never grow the slot a person is asking for.
    fn free_port_for(&self, tree: TreeId, link: LinkId, end: Side, node: NodeId) -> Option<Socket> {
        let signature = self.signature(tree, node)?;
        let ports = match end {
            Side::Input => signature.inputs,
            Side::Output => signature.outputs,
        };
        let host = self.tree(tree)?;
        let standing = host
            .links()
            .iter()
            .find(|held| held.id == link)
            .map(|held| match end {
                Side::Input => held.to,
                Side::Output => held.from,
            })?;
        let occupied = |socket: Socket| {
            host.links()
                .iter()
                .filter(|held| held.id != link)
                .any(|held| match end {
                    Side::Input => held.to == socket,
                    Side::Output => held.from == socket,
                })
        };
        ports
            .iter()
            .enumerate()
            .map(|(index, port)| {
                (
                    Socket::new(node, u32::try_from(index).unwrap_or(u32::MAX)),
                    port,
                )
            })
            .filter(|(socket, _)| *socket != standing)
            .filter(|(socket, port)| match port.multiplicity(end) {
                Multiplicity::One => !occupied(*socket),
                Multiplicity::Many => true,
            })
            .map(|(socket, _)| socket)
            .find(|socket| self.may_relink(tree, link, end, *socket).is_ok())
    }

    /// Where the port a new item contributes would sit.
    ///
    /// `run.start() + ordinal * run.stride()` — R1928's arithmetic, and that
    /// round measured what each half of it costs when it is dropped: without the
    /// start a fixed port before the run is mistaken for an item's, and without
    /// the stride the second port of a multi-port item reads as the next item's.
    /// `None` when the kind declares no run on that side, which is the whole of
    /// what *this node cannot grow one* means.
    fn grown_socket(&self, tree: TreeId, node: NodeId, side: Side) -> Option<Socket> {
        let run = self.variadic(tree, node, side)?;
        let ordinal = next_ordinal(self, tree, node, side);
        Some(Socket::new(node, run.start() + ordinal * run.stride()))
    }
}

/// Where the run's next item goes: at the end of the resolved item list.
///
/// Read off [`Document::items`] rather than off the signature, because the
/// signature counts PORTS and one item may contribute several.
fn next_ordinal<K: NodeKind>(
    document: &Document<K>,
    tree: TreeId,
    node: NodeId,
    side: Side,
) -> u32 {
    document
        .items(tree, node, side)
        .map_or(0, |items| u32::try_from(items.len()).unwrap_or(u32::MAX))
}
