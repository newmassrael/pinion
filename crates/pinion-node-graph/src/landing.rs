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
use crate::relink::{RelinkError, Relinked, Retargeted};

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

/// ★★★★★ R1980 — **where an arriving end berths on a node of this kind**, among
/// the ports that have room for it.
///
/// The document decides what is *legal* — which ports exist, which have room
/// ([`Occupants`](crate::Occupants) and [`Multiplicity`]), and whether the wire
/// itself may go there — and this is the kind saying which of those it
/// *prefers*. So a kind cannot name a socket that does not exist, cannot take
/// one that is occupied, and cannot make an illegal landing legal: the answer
/// is a policy, not a socket.
///
/// # ★ What the reference does, measured at its header, its three consumers and
/// all 41 sites that install the hook
///
/// Its hook is handed the link and returns a bool, and every one of the 25
/// per-node implementations does the same thing in place: if an end touched
/// this node's **open** end, grow a port from the far end's type and label and
/// **move the end onto it**.
///
/// ⚠ The other 16 do nothing, and they are not one group: **15** install a
/// single shared `return true`, and **one** is a bridge that hands the link to
/// a script-defined function and returns `true` however that answers. R1979.1
/// recorded this split as "14 are `return true`", which is neither number and
/// leaves 41 unaccounted for by two; re-measured by driving the counts at
/// R1980, `15 + 1 + 25 = 41` closes.
///
/// Three things follow, and each is why this is a declaration instead:
///
/// * **Its two answers are the same value.** The bool means *the drop is
///   allowed*, and an implementation that already moved the end returns the
///   same `true` as one that did nothing — so a caller cannot tell *I accepted
///   this* from *I have already dealt with it*.
/// * **It runs on a live document.** The hook edits the graph and then the
///   caller attempts the connection; a refused connection leaves the port
///   behind. [`Document::land`] does the whole thing to a copy (§2 #3), and a
///   hook that can only answer [`Berth`] cannot reach inside that.
/// * **Its two calls are ordered and nothing says so.** Both ends are asked in
///   turn, so the second implementation sees the first one's edit. A policy
///   read once per side has no order to depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Berth {
    /// The earliest port with room, in port order; a port is grown only when
    /// none has room.
    ///
    /// The default, and what every node did before a kind could say otherwise:
    /// the order a person expects, and the one that does not litter a node with
    /// ports every time a wire is re-aimed onto it.
    #[default]
    Earliest,
    /// A port of its own, every time: never take a port that has room, always
    /// grow one.
    ///
    /// What the reference's 25 overriders express by hand. A node whose ports
    /// each stand for something — one accepted address, one named argument —
    /// wants the arriving end on a port nothing else is using, even when an
    /// older one has gone quiet.
    ///
    /// ⚠ It is a preference, not a promise: a kind that declares no run
    /// ([`NodeKind::variadic`]) has nothing to grow, and the landing is
    /// [`LandError::NoRoom`] rather than a port appearing from nowhere.
    Fresh,
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

/// What turning a link round did (R2000).
///
/// [`Landed`]'s answer with a [`Landfall`] per end — because both ends are the
/// subject and either of them may have needed a port that was not there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turned {
    /// Where each end berthed: the producing end first.
    pub falls: (Landfall, Landfall),
    /// The move itself, under the link's own id.
    pub retargeted: Retargeted,
}

impl Turned {
    /// Whether the link came out running between the same two nodes the other
    /// way — the question a caller asks after a turn.
    #[must_use]
    pub fn reversed(&self) -> bool {
        self.retargeted.reversed()
    }
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

    /// ★★★★★ R1980 — **which of the ports with room this kind wants the end on.**
    ///
    /// One declaration per kind, read here and nowhere else. `NodeBody`'s
    /// structural arms answer [`Berth::Earliest`]: a group instance, an
    /// interface end, a frame, a delay, a reroute, a beacon and an echo all
    /// have ports that stand for something fixed, so *a port of its own every
    /// time* is not a thing any of them could mean.
    #[must_use]
    pub fn berth(&self, tree: TreeId, node: NodeId, side: Side) -> Berth {
        match self.tree(tree).and_then(|host| host.node(node)) {
            Some(found) => match &found.body {
                crate::model::NodeBody::Kind(kind) => kind.berth(side),
                _ => Berth::Earliest,
            },
            None => Berth::Earliest,
        }
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
    ///
    /// ★★★★★ **R1980 — what is standing on a port is
    /// [`Document::occupants`](Self::occupants)' answer, both layers of it.**
    /// This used to walk `links()` here, so a socket a *reported* connection was
    /// sitting on read as free and an unrelated re-aim took it — driven on the
    /// node lab's opening graph, where the screen's own three readers had all
    /// counted both layers and this one had not. See
    /// [`occupancy`](crate::Occupants) for the run that measured it.
    ///
    /// ★ And [`Berth::Fresh`] answers `None` without looking: a kind that wants
    /// a port of its own every time is not asking which of the free ones to
    /// take.
    fn free_port_for(&self, tree: TreeId, link: LinkId, end: Side, node: NodeId) -> Option<Socket> {
        if self.berth(tree, node, end) == Berth::Fresh {
            return None;
        }
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
                Multiplicity::One => self.occupants(tree, *socket, end).without(link).is_free(),
                Multiplicity::Many => true,
            })
            .map(|(socket, _)| socket)
            .find(|socket| self.may_relink(tree, link, end, *socket).is_ok())
    }

    /// ★★★★★ R2000 — **would this link turn round, and where would each end
    /// berth?** — asked before anything moves.
    ///
    /// The producing end goes to the node the link arrives at now, the
    /// consuming end to the node it leaves; each is a [`Landfall`], so a screen
    /// can say *these two pins take it* or *a pin will appear here* before a
    /// person presses.
    ///
    /// The same planner [`turn`](Self::turn) acts on, so asking and doing
    /// cannot answer differently — R1924's rule, which [`may_land`](Self::may_land)
    /// already follows one gesture over.
    ///
    /// # Errors
    ///
    /// [`LandError`] — exactly what [`turn`](Self::turn) would answer.
    pub fn may_turn(
        &self,
        tree: TreeId,
        link: LinkId,
    ) -> Result<(Landfall, Landfall), LandError<K::Type>> {
        self.plan_turn(tree, link)
    }

    /// ★★★★★ R2000 — **turn the link round**: the same two nodes, the flow the
    /// other way, under the same [`LinkId`].
    ///
    /// # What this is for
    ///
    /// A wire drawn the wrong way round is the most ordinary authoring mistake
    /// a graph has, and the repair a person reaches for is *not* delete it and
    /// draw it again. Everything holding the link's name — a selection, a
    /// breakpoint, its mute, an undo entry, a renderer's cache — survives this
    /// and does not survive that.
    ///
    /// The mute travels with the link, deliberately: a wiring being A/B-tested
    /// is still being A/B-tested after somebody notices it points the wrong
    /// way.
    ///
    /// # ★★★★★ Why this is a LANDING and not a pair of sockets
    ///
    /// Because *which ports* is a question the reference never has to answer
    /// and this crate already had a decided answer for. There, a transition
    /// runs between two states with one inbound and one outbound pin apiece, so
    /// its verb can be a bare command. A node here has as many ports as its
    /// kind declares, and the first draft of this verb answered that by
    /// **refusing** whenever more than one pair would stand — which is a
    /// refusal a caller cannot act on and which the node lab produced on its
    /// second gesture: a card listening in two places offered two ways round
    /// and the verb declined to turn a wire it had just turned.
    ///
    /// [`Berth`] is the rule that was already there. A drop on a card takes the
    /// earliest port with room and grows one when none has room, and a person
    /// on that canvas does not choose a slot — so a reversal choosing any other
    /// way would be a second policy for one question. This is the same policy
    /// over a **pair**, which is why the search is ordered rather than
    /// exhaustive: the first pair the graph takes wins.
    ///
    /// # Errors
    ///
    /// [`LandError`]. [`LandError::NoRoom`] names the end that could not berth
    /// — a far node that produces nothing, or a near node with no free port and
    /// no run to grow — and is fixed by a different action from
    /// [`LandError::Refused`], which is the wire itself being refused.
    pub fn turn(
        &mut self,
        tree: TreeId,
        link: LinkId,
        item: Item<K::Type>,
    ) -> Result<Turned, LandError<K::Type>> {
        let (out_fall, in_fall) = self.plan_turn(tree, link)?;
        // ★ The copy IS the atomicity, exactly as in `land`: everything happens
        // to it and `self` is untouched until the last line, so a refusal on any
        // step changed nothing by construction rather than by an unwind.
        let mut trying = self.clone();
        let producer = out_fall.socket().node;
        let consumer = in_fall.socket().node;
        if out_fall.is_new() {
            let at = next_ordinal(&trying, tree, producer, Side::Output);
            trying
                .insert_item(tree, producer, Side::Output, at, item.clone())
                .map_err(|_| LandError::NoRoom {
                    node: producer,
                    side: Side::Output,
                })?;
        }
        if in_fall.is_new() {
            let at = next_ordinal(&trying, tree, consumer, Side::Input);
            trying
                .insert_item(tree, consumer, Side::Input, at, item)
                .map_err(|_| LandError::NoRoom {
                    node: consumer,
                    side: Side::Input,
                })?;
        }
        let retargeted = trying
            .retarget(tree, link, out_fall.socket(), in_fall.socket())
            .map_err(LandError::Refused)?;
        *self = trying;
        Ok(Turned {
            falls: (out_fall, in_fall),
            retargeted,
        })
    }

    /// The reversal, decided once and used by both halves.
    ///
    /// ★ Ordered rather than exhaustive, which is [`Berth::Earliest`]'s own
    /// policy read over a pair: the candidates for each end are the ports with
    /// room in port order, then the port a new item would contribute, and the
    /// first pair [`may_retarget`](Self::may_retarget) admits is the answer. So
    /// an existing pair always beats growing a port, and growing on one side
    /// beats growing on both.
    ///
    /// ⚠ Each candidate that GROWS is asked on a copy, because the socket it
    /// names does not exist yet and no vet can be asked about a port that is
    /// not there — the same reason [`plan_landing`](Self::plan_landing) clones.
    fn plan_turn(
        &self,
        tree: TreeId,
        link: LinkId,
    ) -> Result<(Landfall, Landfall), LandError<K::Type>> {
        let host = self.tree(tree).ok_or(LandError::NoSuchTree(tree))?;
        let held = *host
            .links()
            .iter()
            .find(|standing| standing.id == link)
            .ok_or(LandError::NoSuchLink { tree, link })?;
        let producer = held.to.node;
        let consumer = held.from.node;
        let sources = self.berths(tree, link, Side::Output, producer);
        let sinks = self.berths(tree, link, Side::Input, consumer);
        if sources.is_empty() {
            return Err(LandError::NoRoom {
                node: producer,
                side: Side::Output,
            });
        }
        if sinks.is_empty() {
            return Err(LandError::NoRoom {
                node: consumer,
                side: Side::Input,
            });
        }
        let mut refusal = None;
        for from in &sources {
            for to in &sinks {
                let mut trying = self.clone();
                if from.is_new() {
                    let at = next_ordinal(&trying, tree, producer, Side::Output);
                    if trying
                        .insert_item(tree, producer, Side::Output, at, Item::plain())
                        .is_err()
                    {
                        continue;
                    }
                }
                if to.is_new() {
                    let at = next_ordinal(&trying, tree, consumer, Side::Input);
                    if trying
                        .insert_item(tree, consumer, Side::Input, at, Item::plain())
                        .is_err()
                    {
                        continue;
                    }
                }
                match trying.may_retarget(tree, link, from.socket(), to.socket()) {
                    Ok(()) => return Ok((*from, *to)),
                    // ★ The FIRST refusal is kept, not the last: the candidates
                    // are in preference order, so the reason the preferred pair
                    // was declined is the one a person needs. Keeping the last
                    // would report whichever grown port happened to be tried
                    // most recently, which is the least interesting of them.
                    //
                    // ⚠⚠ R2000 MEASURED THIS AND NOTHING OBSERVES IT. A
                    // counterfactual replacing `refusal.or(Some(why))` with
                    // `Some(why)` — keeping the LAST refusal — left every test
                    // in three crates green. The cause is structural rather
                    // than a thin fixture: two candidates can only differ in
                    // their REASON when one is refused as a cycle and another
                    // on type or flow, and the link that would make the cycle
                    // is the same link that fills the port, which
                    // `Multiplicity::One` then removes from the candidate list
                    // before it is ever asked. Only a many-valued input, or a
                    // reachable kind whose two outputs differ in type, lets
                    // them come apart — the taxonomies here have exactly two
                    // such kinds and neither can stand at the producing end.
                    //
                    // KEPT rather than simplified, because the day a taxonomy
                    // grows one the last refusal becomes the wrong answer, and
                    // a reader arriving then should find the decision already
                    // made rather than have to re-derive it. Registered as
                    // `debt-the-first-refusal-of-a-turn-has-no-observer`.
                    Err(why) => refusal = refusal.or(Some(why)),
                }
            }
        }
        Err(refusal.map_or(
            LandError::NoRoom {
                node: consumer,
                side: Side::Input,
            },
            LandError::Refused,
        ))
    }

    /// Where an end could berth on `node`, in preference order: the ports with
    /// room, earliest first, then the port a new item would contribute.
    ///
    /// One list rather than the `free_port_for`-else-`grown_socket` pair
    /// [`plan_landing`](Self::plan_landing) uses, because a **pair** has to be
    /// searched rather than decided one end at a time — and the order is what
    /// carries [`Berth`]'s policy into that search.
    ///
    /// ⚠ The `may_relink` filter `free_port_for` ends with is deliberately NOT
    /// here. That is a one-ended question, and asking it of a move that also
    /// moves the other end would answer about a graph neither the caller nor
    /// this planner ever proposed. The pair is vetted once, by
    /// [`may_retarget`](Self::may_retarget), which is the whole reason that verb
    /// exists.
    fn berths(&self, tree: TreeId, link: LinkId, end: Side, node: NodeId) -> Vec<Landfall> {
        let mut out = Vec::new();
        if self.berth(tree, node, end) != Berth::Fresh
            && let Some(signature) = self.signature(tree, node)
        {
            let ports = match end {
                Side::Input => signature.inputs,
                Side::Output => signature.outputs,
            };
            for (index, port) in ports.iter().enumerate() {
                let socket = Socket::new(node, u32::try_from(index).unwrap_or(u32::MAX));
                // ⚠ `without(link)` is load-bearing: the link being turned is
                // still in the graph while this is asked, and its own ends would
                // otherwise read as occupying the ports it is about to leave.
                let room = match port.multiplicity(end) {
                    Multiplicity::One => self.occupants(tree, socket, end).without(link).is_free(),
                    Multiplicity::Many => true,
                };
                if room {
                    out.push(Landfall::Takes(socket));
                }
            }
        }
        out.extend(self.grown_socket(tree, node, end).map(Landfall::Grows));
        out
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
