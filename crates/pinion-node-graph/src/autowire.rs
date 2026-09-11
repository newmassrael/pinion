//! ★★★★★ R1987 — **a node that has just arrived chooses which of its own ports
//! takes the wire that called it into being.**
//!
//! A hand drags off a pin, lets go over empty canvas, and picks a node from
//! whatever menu the screen offers there. The node that arrives is expected to
//! come in *already wired* — nobody wants to draw the wire they were already
//! holding. Which of the arriving node's ports takes it is the question this
//! module answers.
//!
//! # What the reference does, measured at its own header rather than summarised
//!
//! The engine publishes one hook on a graph node, and the header says what it
//! is for in as many words: *autowire a newly created node*, taking *the source
//! pin that caused the new node to be created (typically a drag-release context
//! menu creation)*. Its supplied answer is an **empty body**. Two of its calls
//! sit in the base schema's own action for spawning a node from a menu — one
//! for a single dragged pin, one that offers the rest of a multi-pin drag to
//! the same hook in turn — and those two are the shape of the gesture, not the
//! count of its callers (there are 25; see below).
//!
//! 🟥🟥🟥 ★★★★★ **The census's covering sentence for it was wrong in both of
//! its clauses** — one more instance of the standing rule that a pin's reason
//! is re-measured before it is acted on, rather than read. (This header first
//! said *the seventh in a row*; that streak was never counted here, so it is
//! stated as the rule it is an instance of instead of as a number nothing
//! re-derives.) It read *dropping a node onto a wire and having it wire
//! itself — the DCC's `insert_offset`*:
//!
//! * **It is not a drop onto a wire.** The parameter is the pin the drag *left
//!   from*, and the node is created in empty space by a menu. Splicing a node
//!   into an existing link is a different gesture the engine reaches elsewhere.
//! * **It is not the DCC's `insert_offset` either**, and that operator's own
//!   description says so: *automatically offset nodes on insertion*. Measured
//!   at its implementation it holds a `prev`, an `insert` and a `next`, compares
//!   the gaps either side against a margin and animates the neighbours apart.
//!   It is **layout**, it wires nothing, and it runs *after* the splice that
//!   another operator performed. So the two census rows are two capabilities
//!   and the equation between them was decoration — `insert_offset` stays
//!   absent here, with its own reason corrected to name the layout half alone.
//!
//! 🟥🟥🟥 ★★★★★ **Re-measured this round, and the three numbers the first draft
//! of this header carried were all wrong** — rule (9) applied to prose written
//! one turn earlier, not only to prose inherited from a distant round. Counted
//! over the **whole** tree rather than the editor source alone (R1986's
//! population lesson), by definitions and by `override` declarations, which
//! agree:
//!
//! * **31 overriders**, not 34 — 17 in the editor source and 14 in plugin
//!   modules. The base body is the empty one above.
//! * **Only 8 of the 31 ask permission at all**, and the 31 partition exactly:
//!   **8** call the schema's `CanCreateConnection`, **19** call only
//!   `TryCreateConnection` — they do not ask, they **attempt**, and read a bool
//!   back — and **4** do neither, picking by direction or delegating. (7 of the
//!   8 askers also attempt, which is why "8 ask" and "26 attempt" overlap; the
//!   number that never asks is 19. ★ This correction is the *closing audit*
//!   catching a number written earlier in this same round — 26 was first
//!   published here as "never form an opinion", and 26 includes the 7 that do.)
//!   The draft this replaced said 28 of them *ask the schema whether the
//!   connection may be made*. So the reference's common shape is not
//!   scan-and-choose at all; it is try-until-one-sticks, and what it
//!   "preferred" is observable only afterwards, by looking at the graph.
//! * **13** of the 31 walk their own pin list; the other 18 name their pins
//!   directly.
//! * The hook has **25 call sites**, not two. *Two* is the count inside the
//!   base schema's own node-from-menu action — one for a single dragged pin,
//!   one that offers the rest of a multi-pin drag to the same hook in turn —
//!   which is the sentence the paragraph above should have made.
//!
//! Two variations are worth naming because they are what this module's answer
//! is *shaped like*:
//!
//! * the visual-script base keeps a **backup** pin — one whose response is
//!   *make, with a conversion node* — and uses it only if no better one is
//!   found. So the reference does have a preference, and it has exactly one
//!   axis: a direct crossing beats a converted one.
//! * the two audio-class nodes do not scan at all: each picks by **direction**,
//!   one pin for a drag off an input and the other otherwise. Which is the same
//!   rule a scan computes, hand-written for a node with two pins — and the two
//!   are **mirror images** of each other, the same gesture landing on the child
//!   pin in one and the parent pin in the other. Nothing in the reference makes
//!   them agree, because there is no shared derivation for them to agree with.
//!
//! # What is built, and the three measured ways it is better
//!
//! [`Document::may_autowire`] answers *which port would take it*, moving
//! nothing; [`Document::autowire`] wires it. They are not two implementations
//! that have to agree — the verb **calls** the question and then places the
//! link through the same primitive [`Document::connect`] places through
//! ([`Document::wire`]), so nothing is decided twice. That is the shape R1924
//! chose for `relink` and R1986 for the definition verbs.
//!
//! 1. **The answer is a value, where the reference's is `void`.** A caller
//!    there cannot tell *wired to my second input* from *wired nothing at all*
//!    — the hook returns nothing and the empty base body is indistinguishable
//!    from a scan that found no candidate. [`Uptake`] names the port, its
//!    address, whether the value arrives unchanged, and which existing link had
//!    to give way. So a screen can *say* what happened.
//! 2. **When nothing takes the wire, every candidate is named with its
//!    reason.** [`AutowireError::NoneTakes`] carries one [`Declined`] per port
//!    the arriving node presents, each holding the [`ConnectError`] that port
//!    was refused with — the two types, the port and its arity, the two kinds'
//!    own sentence, or the path that would close. The reference in the same
//!    situation does nothing and reports nothing, and the person is left
//!    looking at an unwired node.
//! 3. **The preference is one derivation, and it has a second axis.** The
//!    reference answers the hook in **31** places, so what "best" means is 31
//!    opinions — and **19** of them never form an opinion at all: they attempt
//!    connections until one is accepted, so the pin that wins is the first the
//!    schema did not reject rather than the best one. Of the 8 that do ask,
//!    one axis of preference is expressed, and it cannot see that a wire
//!    **displaces** an existing link, because the responses that displace sit
//!    in the same immediate class as the ones that do not.
//!    Here [`Uptake::preference`] is the whole rule: a direct crossing before a
//!    converted one, and among equals a port that destroys nothing before one
//!    that does. Ties keep declaration order, which is the reference's rule and
//!    the only one a person can predict by looking at the node.
//!
//! # ★★★★★ R2133 — and it is asked of a card that does not exist yet
//!
//! R1987 left this open in as many words, and the paragraph it left here is
//! worth keeping because it is the whole shape of the repair: *the question is
//! asked of a node that exists. A menu that wanted to grey out the kinds which
//! will wire nothing would have to ask it of a kind, and that cannot be done in
//! this vocabulary today: every arm of [`ConnectError`] names [`Socket`]s, and
//! a socket names a [`NodeId`] a node about to be created does not have yet.
//! Inventing a second refusal vocabulary for the hypothetical case is precisely
//! the drift this crate refuses elsewhere.*
//!
//! All of that was right, including the refusal to invent a second vocabulary.
//! What it did not see is the third option: **generalise the slot** rather than
//! duplicate the enum. [`ConnectError`] now carries how an end is *named* as a
//! type parameter defaulting to [`Socket`], so every existing caller is
//! unchanged, and [`Document::may_autowire_prospect`] instantiates it at
//! [`Prospect`] — which can say *the arriving card's pin n* and name no node at
//! all. One vocabulary, one set of arms, one set of sentences; only the
//! spelling of an end varies.
//!
//! The consumer had meanwhile been answering the question the only way it
//! could, by copying the whole document once per palette row, and both halves
//! of what that cost were measured before anything was built here:
//!
//! * **Time.** 21 roles on the analyzer lab, release: 98.7 µs per read at 10
//!   cards, 228 µs at 100, 704 µs at 590, **2.10 ms** at 1,580 — linear in the
//!   card count. Through this call, same machine: **21.4 µs at 1,580**, flat.
//! * **A number that pointed at nothing.** The copy's `add_node` minted an id
//!   and it reached the wire: *node 2.0 may not reach **node 10.0*** on a
//!   document whose cards are 0 through 9. Which is why the reason had to be
//!   flattened to a string to be published at all — and why a client could
//!   show a refusal but never branch on it.

use std::fmt;

use crate::model::{
    Act, ConnectError, Conversion, Document, EditError, KindPort, Link, LinkId, NodeBody, NodeId,
    NodeKind, Prospect, Side, Signature, Socket, TreeId, admission_rule, crossing, crossing_rule,
    crowded_end,
};
use crate::split::PortPath;

/// Whether the wire's value arrives unchanged or through a declared map.
///
/// Two arms and not a `bool`, and not three: a port the value may **not** enter
/// is not an arrival at all, it is a [`Declined`]. Ordered so that the better
/// answer is the smaller one, which is what makes [`Uptake::preference`] a
/// plain comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Arrival {
    /// The value crosses as it is.
    Unchanged,
    /// The value crosses through the taxonomy's declared conversion
    /// ([`NodeKind::conversion`]).
    Converted,
}

/// A port of the arriving node that **would take** the wire (R1987).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uptake {
    /// The resolved index of the port on the arriving node, on the side
    /// opposite the one the wire is leaving.
    pub port: u32,
    /// The same port as an address, which is what a screen holding a split pin
    /// needs: a member keeps its place when a neighbouring port comes apart and
    /// a resolved index does not (R1914).
    pub at: PortPath,
    /// How the value gets across.
    pub arrival: Arrival,
    /// **The link this pin would evict**, when it already holds one it may not
    /// share. `None` when nothing gives way.
    ///
    /// 🟥🟥🟥 ★★★★★ Not `vet`'s own `crowded` answer, and the difference is
    /// what `r1987_a_pin_that_displaces_nothing_wins_a_tie` found on its first
    /// run. `vet` answers *which end takes one link* — a **capacity** fact,
    /// true of a value input whether or not anything is wired to it — and the
    /// first draft of this field published that as "would displace". In a
    /// dataflow graph almost every input takes one link, so the tiebreak below
    /// would have ranked every candidate as destructive and therefore ranked
    /// none of them. Occupancy is the fact a person cares about, and it is read
    /// with the same predicate the placement uses, so what this names is what
    /// would actually go.
    ///
    /// The reference cannot say this at all: its connection attempt answers a
    /// bare boolean, so what it broke is simply gone.
    pub displaces: Option<Link>,
}

impl Uptake {
    /// Where this ranks against another — **smaller is better**.
    ///
    /// The whole preference rule, in one place, so that no two node kinds can
    /// disagree about what "best" means the way the reference's 31 hand-written
    /// answers can — 19 of which decide by attempting connections until one is
    /// accepted, which is not a preference at all. Two axes, the first
    /// dominant:
    ///
    /// 1. a value that arrives **unchanged** before one that needs a map. This
    ///    is the reference's own axis, and its only one — it keeps a converted
    ///    candidate as a backup and uses it if nothing better turns up.
    /// 2. among equals, a port that **destroys nothing** before one whose limit
    ///    the wire exceeds. The reference cannot express this: the responses
    ///    that displace an existing link sit in the same immediate class as the
    ///    ones that do not, so it takes whichever comes first in declaration
    ///    order and the person finds out afterwards.
    ///
    /// Ties are broken by declaration order, by the sort being stable — which
    /// is the reference's rule and the only tiebreak a person can predict by
    /// looking at the node.
    #[must_use]
    pub fn preference(&self) -> (Arrival, bool) {
        (self.arrival, self.displaces.is_some())
    }
}

/// A port of the arriving node that **would not** take the wire, and why
/// (R1987).
///
/// The reason is the authoring refusal whole — the same [`ConnectError`]
/// [`Document::connect`] would answer — rather than a bit, for
/// `relink`'s reason: a wire refused for a type that does not cross and one
/// refused for a cycle are repaired by different actions, and this is the
/// difference.
///
/// `S` is how the refusal names an end, defaulting to [`Socket`]; see
/// [`ConnectError`] for why it is a parameter (R2133).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declined<T, S = Socket> {
    /// The resolved index of the port that declined it.
    pub port: u32,
    /// That port as an address (R1914).
    pub at: PortPath,
    /// Why it declined.
    pub why: ConnectError<T, S>,
}

/// Why a node could not be wired to the pin that created it (R1987).
///
/// `S` is how a refusal names an end, defaulting to [`Socket`]. The
/// instantiation at [`Prospect`] is what
/// [`Document::may_autowire_prospect`] answers in, and it is one vocabulary
/// rather than two on purpose — see [`ConnectError`] (R2133).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AutowireError<T, S = Socket> {
    /// No such tree.
    NoSuchTree(TreeId),
    /// The dangling end or the arriving node is not in that tree.
    NoSuchNode(NodeId),
    /// The wire is leaving a port the dangling node does not have.
    NoSuchPort {
        /// The socket that is not there.
        socket: Socket,
        /// How many ports that end actually has.
        arity: u32,
    },
    /// ★★★★★ R2133 — the tree would not take a node of this body **at all**,
    /// so there is no wiring question to ask.
    ///
    /// Exactly what [`Document::add_node`] would refuse with, asked without
    /// adding anything. Only [`Document::may_autowire_prospect`] can reach it:
    /// a node the placed question is asked about is already in the tree, so
    /// the tree has admitted it.
    NotAdmitted(EditError),
    /// ★★★★★ R2133 — this body's ports are derived from the links it lands
    /// among, so there is nothing to answer before it is placed.
    ///
    /// A reroute, a beacon, an echo and a stand-in — see
    /// [`NodeBody::ports_are_derived_from_where_it_lands`]. Its own arm rather
    /// than a [`Self::NoPorts`], because *not knowable yet* and *never has one*
    /// are different facts: a palette greys out a card for the second and not
    /// for the first.
    PortsNotYetDerivable,
    /// The arriving node presents **no port at all** on the side that would
    /// have to take the wire.
    ///
    /// Its own arm rather than a [`Self::NoneTakes`] with an empty list,
    /// because the two are different facts and a person repairs them
    /// differently: *this kind never listens* is a choice about the kind, and
    /// *these pins all refused* is a question about the types. The reference
    /// cannot tell them apart — both are its empty hook body.
    ///
    /// ⚠ R2133 removed the `node` this carried. It was always the `arriving`
    /// argument handed straight back — a second copy of what the caller had
    /// just passed in — and that echo is the one thing that stopped the arm
    /// being sayable about a node with no [`NodeId`].
    NoPorts {
        /// The side it has none on.
        side: Side,
    },
    /// It has ports on that side and **not one of them** takes the wire, with
    /// each one's own refusal.
    NoneTakes {
        /// One entry per port that was offered the wire, in declaration order.
        /// Never empty — that case is [`Self::NoPorts`].
        declined: Vec<Declined<T, S>>,
    },
}

impl<T: fmt::Debug, S: fmt::Display> fmt::Display for AutowireError<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {tree}"),
            Self::NoSuchNode(node) => write!(f, "no node {node}"),
            Self::NoSuchPort { socket, arity } => {
                write!(f, "{socket} is not there: that end has {arity} port(s)")
            }
            Self::NotAdmitted(why) => write!(f, "{why}"),
            Self::PortsNotYetDerivable => f.write_str(
                "this card's pins depend on what it is wired to, so there is \
                 nothing to answer until it is placed",
            ),
            // ★ R1699/R1719 — a refusal says itself, because this sentence
            // reaches a person in a toast and an agent as a rejection's reason.
            // ★ R1987 — `noun`, not `name`. `name` is the wire form a client
            // parses ("in"/"out"), and putting it here read "has no in pin".
            Self::NoPorts { side } => {
                write!(
                    f,
                    "the arriving card has no {} pin to take the wire",
                    side.noun()
                )
            }
            Self::NoneTakes { declined } => {
                write!(f, "no pin takes the wire")?;
                for one in declined {
                    write!(f, "; {}", one.why)?;
                }
                Ok(())
            }
        }
    }
}

impl<T: fmt::Debug, S: fmt::Debug + fmt::Display> std::error::Error for AutowireError<T, S> {}

/// What an autowire did (R1987).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autowired {
    /// The link it made.
    pub link: LinkId,
    /// The port it chose, and how the value gets across it.
    pub took: Uptake,
    /// The link that had to go, if the chosen port already held one it may not
    /// share. Reporting it is what makes the replacement undoable.
    pub displaced: Option<Link>,
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1987 — **every port of `arriving` that would take a wire leaving
    /// `dangling`, best first.**
    ///
    /// `leaving` says which of the dangling node's two port lists `dangling.port`
    /// indexes, so the gesture works in both directions: a wire dragged off a
    /// producing pin is offered the arriving node's inputs, and one dragged off
    /// a consuming pin is offered its outputs. The reference's hook takes the
    /// same pair as one pointer and reads the direction off it.
    ///
    /// Never answers an empty list: a node with no candidate ports is
    /// [`AutowireError::NoPorts`] and one whose every port refused is
    /// [`AutowireError::NoneTakes`], each carrying what the caller needs to say
    /// so. A list that could be empty would make those two facts one.
    ///
    /// # Errors
    ///
    /// [`AutowireError`] — the tree or a node is not there, the wire leaves a
    /// port that is not there, the arriving node has no port on that side, or
    /// every port on it declined, each with its reason.
    pub fn autowire_uptakes(
        &self,
        tree: TreeId,
        dangling: Socket,
        leaving: Side,
        arriving: NodeId,
    ) -> Result<Vec<Uptake>, AutowireError<K::Type>> {
        let plan = self.plan_autowire(tree, dangling, leaving, arriving)?;
        let mut all = Vec::with_capacity(plan.rest.len() + 1);
        all.push(plan.took);
        all.extend(plan.rest);
        Ok(all)
    }

    /// ★★★★★ R1987 — **which port would take it**, asked before anything is
    /// wired.
    ///
    /// The best [`Uptake`] by [`Uptake::preference`]. This is not a prediction
    /// of [`autowire`](Self::autowire): it is the same call
    /// [`autowire`](Self::autowire) makes, so the two cannot answer
    /// differently. Asking changes nothing about the document.
    ///
    /// # Errors
    ///
    /// [`AutowireError`] — exactly what [`autowire`](Self::autowire) would
    /// answer.
    pub fn may_autowire(
        &self,
        tree: TreeId,
        dangling: Socket,
        leaving: Side,
        arriving: NodeId,
    ) -> Result<Uptake, AutowireError<K::Type>> {
        self.plan_autowire(tree, dangling, leaving, arriving)
            .map(|plan| plan.took)
    }

    /// ★★★★★ R1987 — **wire the arriving node to the pin that created it.**
    ///
    /// Asks [`may_autowire`](Self::may_autowire) and places the link the answer
    /// names, through the same primitive [`connect`](Self::connect) places
    /// through. The pair is vetted once: the placement acts on the decision
    /// rather than re-deciding, so there is no second refusal point for a
    /// caller to have to describe.
    ///
    /// # Errors
    ///
    /// [`AutowireError`] — exactly what [`may_autowire`](Self::may_autowire)
    /// answers.
    pub fn autowire(
        &mut self,
        tree: TreeId,
        dangling: Socket,
        leaving: Side,
        arriving: NodeId,
    ) -> Result<Autowired, AutowireError<K::Type>> {
        let plan = self.plan_autowire(tree, dangling, leaving, arriving)?;
        let landed = Socket::new(arriving, plan.took.port);
        let (from, to) = ends(dangling, leaving, landed);
        // The tree resolved twice inside the plan, so the only `None` here is
        // one a vetted pair cannot reach — mapped onto the arm the caller
        // already has rather than given an arm of its own that no test could
        // stand on.
        let made = self
            .wire(tree, from, to, plan.crowded)
            .ok_or(AutowireError::NoSuchTree(tree))?;
        Ok(Autowired {
            link: made.link,
            took: plan.took,
            displaced: made.displaced,
        })
    }

    /// The one decision behind the question and the verb (R1987).
    ///
    /// Answers the best uptake and the rest in preference order, which is what
    /// lets [`autowire_uptakes`](Self::autowire_uptakes) publish a list that
    /// **cannot be empty** without either a panic or an arm nothing can reach.
    /// Building the two halves apart is the type saying what the prose would
    /// otherwise have to promise.
    fn plan_autowire(
        &self,
        tree: TreeId,
        dangling: Socket,
        leaving: Side,
        arriving: NodeId,
    ) -> Result<Plan, AutowireError<K::Type>> {
        let held = self.dangling_ports(tree, dangling, leaving)?;
        let side = leaving.other();
        let offered = Self::offered_ports(
            self.signature(tree, arriving)
                .ok_or(AutowireError::NoSuchNode(arriving))?,
            side,
        )?;
        Self::plan_over(&held, &offered, dangling, leaving, |index| {
            let landed = Socket::new(arriving, index);
            let (from, to) = ends(dangling, leaving, landed);
            let at = self
                .path_of(tree, arriving, side, index)
                .unwrap_or_else(|| PortPath::root(index));
            // The one authority on whether the pair may be wired. Asked here
            // rather than re-derived, so a pin this admits is a pin `connect`
            // admits — which is what makes the verb above able to place without
            // asking again.
            let vetted = self
                .vet(tree, from, to)
                .map(|crowded| (crowded, self.standing_at(tree, from, to, crowded)));
            (at, vetted)
        })
    }

    /// ★★★★★ R2133 — **which port a card that does not exist yet would take it
    /// with**, asked without adding the card.
    ///
    /// The question a palette asks before a person presses anything: *if I
    /// pressed this row, would the wire I am holding land?* Until this round
    /// the only way to answer it was the caller's — `clone()` the whole
    /// document, `add_node` into the copy, ask
    /// [`may_autowire`](Self::may_autowire), throw the copy away — once per row
    /// of the palette.
    ///
    /// # What that cost, measured (R2133)
    ///
    /// On the analyzer lab, release build, 21 palette roles: **98.7 µs** per
    /// read of the register at 10 cards, **228 µs** at 100, **704 µs** at 590
    /// and **2.10 ms** at 1,580 — linear in the document's node count at about
    /// 1.27 µs per node per read, because each role copies every node. At 1,580
    /// cards that is an eighth of a 60 Hz frame for one register, and the
    /// multiplier is the roster: the same graph with a 200-row palette would
    /// spend a whole frame on it.
    ///
    /// This answers on the document itself, so the cost is the ports of one
    /// candidate and does not grow with the graph at all. Measured through the
    /// same register on the same machine and build afterwards: **21.2 µs at
    /// 100 cards, 21.1 at 590, 21.4 at 1,580** — flat, and ~98x at the top of
    /// that range.
    ///
    /// # And the answer is better, not just cheaper
    ///
    /// The copy's `add_node` mints a [`NodeId`], and that id then appeared in
    /// the refusal a client received — measured on the opening graph, *node 2.0
    /// may not reach node 10.0*, where the document the reader holds has no
    /// node 10. So the refusal had to be flattened to a **string** before it
    /// could be published at all. Here the arriving end is
    /// [`Prospect::Arriving`], which says *the arriving card's pin n* and names
    /// no node, so the refusal crosses the wire as the structured
    /// [`ConnectError`] it is and a client can branch on which arm it was.
    ///
    /// # Errors
    ///
    /// [`AutowireError`] — the tree or the dangling node is not there, the wire
    /// leaves a port that is not there, the tree would not admit a node of this
    /// body ([`AutowireError::NotAdmitted`]), the body's ports are not knowable
    /// before it is placed ([`AutowireError::PortsNotYetDerivable`]), it
    /// presents no port on that side, or every port on it declined — each with
    /// its reason.
    pub fn prospective_uptakes(
        &self,
        tree: TreeId,
        dangling: Socket,
        leaving: Side,
        body: &NodeBody<K>,
    ) -> Result<Vec<Uptake>, AutowireError<K::Type, Prospect>> {
        let plan = self.plan_prospect(tree, dangling, leaving, body)?;
        let mut all = Vec::with_capacity(plan.rest.len() + 1);
        all.push(plan.took);
        all.extend(plan.rest);
        Ok(all)
    }

    /// ★★★★★ R2133 — the best [`Uptake`] of
    /// [`prospective_uptakes`](Self::prospective_uptakes), which is
    /// [`may_autowire`](Self::may_autowire)'s question asked of a card that
    /// does not exist yet.
    ///
    /// Not a prediction of what the press will do: it is the same rules on the
    /// same graph with the same ports, and
    /// `r2133_asking_before_the_card_exists_answers_what_asking_after_does` is
    /// the gate that holds the two to each other over a population.
    ///
    /// # Errors
    ///
    /// [`AutowireError`] — exactly what
    /// [`prospective_uptakes`](Self::prospective_uptakes) answers.
    pub fn may_autowire_prospect(
        &self,
        tree: TreeId,
        dangling: Socket,
        leaving: Side,
        body: &NodeBody<K>,
    ) -> Result<Uptake, AutowireError<K::Type, Prospect>> {
        self.plan_prospect(tree, dangling, leaving, body)
            .map(|plan| plan.took)
    }

    /// The prospective half of the one decision (R2133).
    ///
    /// The two graph rules [`Document::vet`] adds to the pair rules — a self
    /// link and a cycle — are the two this cannot reach and does not need to:
    /// the arriving node is not the dangling one, so no pair here is a self
    /// link, and a node with no links is on no path, so none of them can close
    /// a cycle. That is why the rules below are the whole vet and not a weaker
    /// one, and `r2133_the_two_rules_a_prospective_pair_cannot_break` is what
    /// says so out loud.
    fn plan_prospect(
        &self,
        tree: TreeId,
        dangling: Socket,
        leaving: Side,
        body: &NodeBody<K>,
    ) -> Result<Plan, AutowireError<K::Type, Prospect>> {
        // The dangling end first, for the order `dangling_ports` records.
        let held = self.dangling_ports(tree, dangling, leaving)?;
        // Asked the way `add_node` asks it, so a card the palette would be
        // refused outright is not reported as a wiring problem.
        self.may(tree, Act::Create(body))
            .map_err(AutowireError::NotAdmitted)?;
        let offered = Self::offered_ports(
            self.prospective_signature(tree, body)
                .ok_or(AutowireError::PortsNotYetDerivable)?,
            leaving.other(),
        )?;
        let leaving_kind = self.kind_at(tree, dangling.node);
        let arriving_kind = body.kind();
        Self::plan_over(&held, &offered, dangling, leaving, |index| {
            let (from, to) = ends(
                Prospect::Placed(dangling),
                leaving,
                Prospect::Arriving(index),
            );
            let (source, sink) = pair::<K>(&held, dangling.port, &offered, index, leaving);
            let (source_kind, sink_kind) = match leaving {
                Side::Output => (leaving_kind, arriving_kind),
                Side::Input => (arriving_kind, leaving_kind),
            };
            let vetted = crossing_rule::<K, _>(source, sink, &from, &to)
                .and_then(|()| admission_rule::<K, _>(source_kind, sink_kind, &from, &to))
                .map(|()| {
                    let crowded = crowded_end::<K>(source, sink);
                    (
                        crowded,
                        self.prospective_standing(tree, dangling, leaving, crowded),
                    )
                });
            // A node that does not exist yet declares no split, so every one of
            // its ports is at its root index — which is exactly what
            // `add_node` would leave `path_of` answering.
            (PortPath::root(index), vetted)
        })
    }

    /// The ports at the **dangling** end, with the two ways that end is not
    /// there (R2133).
    ///
    /// Shared by the placed question and the prospective one because it is
    /// about the WIRE, which is the same wire in both — and asked FIRST by
    /// both, because that is the order the refusals were in before this was
    /// extracted: a call naming a tree that does not exist answers about the
    /// node it was dragged from, which is the sentence `signature` gives, and
    /// `r1987_a_wire_from_a_pin_that_is_not_there_is_refused_at_that_end`
    /// asserts exactly that.
    fn dangling_ports<S>(
        &self,
        tree: TreeId,
        dangling: Socket,
        leaving: Side,
    ) -> Result<Vec<KindPort<K>>, AutowireError<K::Type, S>> {
        let held = self
            .signature(tree, dangling.node)
            .ok_or(AutowireError::NoSuchNode(dangling.node))?;
        let held = match leaving {
            Side::Input => held.inputs,
            Side::Output => held.outputs,
        };
        let arity = u32::try_from(held.len()).unwrap_or(u32::MAX);
        if dangling.port >= arity {
            return Err(AutowireError::NoSuchPort {
                socket: dangling,
                arity,
            });
        }
        Ok(held)
    }

    /// The candidate's ports on the side that would have to take the wire, or
    /// the fact that it has none (R2133).
    fn offered_ports<S>(
        offered: Signature<K>,
        side: Side,
    ) -> Result<Vec<KindPort<K>>, AutowireError<K::Type, S>> {
        let offered = match side {
            Side::Input => offered.inputs,
            Side::Output => offered.outputs,
        };
        if offered.is_empty() {
            return Err(AutowireError::NoPorts { side });
        }
        Ok(offered)
    }

    /// The preference, the order and the *never empty* guarantee — the part of
    /// the decision that is the same whether the arriving node exists (R2133).
    ///
    /// `vet_pin` is the part that is not: it names the two ends, addresses the
    /// candidate and applies the rules, which is where the two questions
    /// genuinely differ. Everything after it is here once, so the two cannot
    /// rank pins differently or disagree about which of the two "nothing takes
    /// it" facts happened.
    ///
    /// An associated function rather than a method: every fact it reads comes
    /// through its arguments, and the document it does NOT touch is the point —
    /// whatever the graph is, the ranking is the same ranking.
    fn plan_over<S>(
        held: &[KindPort<K>],
        offered: &[KindPort<K>],
        dangling: Socket,
        leaving: Side,
        mut vet_pin: impl FnMut(u32) -> (PortPath, VettedPin<K, S>),
    ) -> Result<Plan, AutowireError<K::Type, S>> {
        let mut uptakes: Vec<(Uptake, Option<Side>)> = Vec::new();
        let mut declined: Vec<Declined<K::Type, S>> = Vec::new();
        for index in 0..u32::try_from(offered.len()).unwrap_or(u32::MAX) {
            let (at, vetted) = vet_pin(index);
            match vetted {
                Ok((crowded, displaces)) => {
                    // The vet passed, so the crossing is not refused: the only
                    // two answers left are the two arms of `Arrival`.
                    let (source, sink) = pair::<K>(held, dangling.port, offered, index, leaving);
                    let arrival = match crossing::<K>(source, sink) {
                        Conversion::Converted(_) => Arrival::Converted,
                        Conversion::Direct | Conversion::Refused => Arrival::Unchanged,
                    };
                    uptakes.push((
                        Uptake {
                            port: index,
                            at,
                            arrival,
                            displaces,
                        },
                        crowded,
                    ));
                }
                Err(why) => declined.push(Declined {
                    port: index,
                    at,
                    why,
                }),
            }
        }
        // Stable, so pins of equal preference keep declaration order — the
        // reference's own tiebreak and the only one a person can predict.
        uptakes.sort_by_key(|(one, _)| one.preference());
        let mut uptakes = uptakes.into_iter();
        let (took, crowded) = uptakes
            .next()
            .ok_or(AutowireError::NoneTakes { declined })?;
        Ok(Plan {
            took,
            crowded,
            rest: uptakes.map(|(one, _)| one).collect(),
        })
    }

    /// What a prospective wire would evict (R2133).
    ///
    /// The arriving end is a card that does not exist, so it holds no link and
    /// nothing there can give way; the dangling end is real and may. Which of
    /// the two `crowded` names is decided by one comparison: `ends` puts the
    /// dangling socket on the side the wire is LEAVING from, so the dangling
    /// end is the crowded one exactly when the two agree.
    fn prospective_standing(
        &self,
        tree: TreeId,
        dangling: Socket,
        leaving: Side,
        crowded: Option<Side>,
    ) -> Option<Link> {
        if crowded? != leaving {
            return None;
        }
        let links = self.tree(tree)?.links();
        links
            .iter()
            .find(|held| match leaving {
                Side::Output => held.from == dangling,
                Side::Input => held.to == dangling,
            })
            .copied()
    }

    /// The link a new one at this pair would evict, which is **occupancy** and
    /// not the capacity `crowded` reports (R1987).
    ///
    /// Reads it with [`place`](Self::place)'s own predicate, so what this names
    /// is what would actually go. `crowded` still gates the question, because
    /// a pin that takes many links evicts nothing however full it is.
    fn standing_at(
        &self,
        tree: TreeId,
        from: Socket,
        to: Socket,
        crowded: Option<Side>,
    ) -> Option<Link> {
        let links = self.tree(tree)?.links();
        match crowded? {
            Side::Input => links.iter().find(|held| held.to == to).copied(),
            Side::Output => links.iter().find(|held| held.from == from).copied(),
        }
    }
}

/// What an autowire WOULD do, worked out without doing any of it (R1987).
///
/// Private because it is the shared decision and not a published answer, and it
/// carries one thing [`Uptake`] deliberately does not: `vet`'s own `crowded`,
/// which is the **capacity** fact the placement wants and not the occupancy
/// fact a person wants. Keeping both apart in one place is what stops
/// either being read as the other, which is the mistake the field note on
/// [`Uptake::displaces`] records.
struct Plan {
    /// The pin the wire should land on.
    took: Uptake,
    /// Which end of that pair takes one link, from the vet.
    crowded: Option<Side>,
    /// The other pins that would take it, in preference order.
    rest: Vec<Uptake>,
}

/// The producing and consuming ends of the wire, given which side it left from.
///
/// One place, so the question and the verb cannot orient the pair differently —
/// which would make the verb wire the mirror image of what it was told.
///
/// ★ R2133 — generic in how an end is named, so the prospective question
/// orients its pair through this same function rather than through a second
/// copy of the `match`. The mirror-image bug this exists to prevent is exactly
/// the one a second copy would reintroduce.
fn ends<S>(dangling: S, leaving: Side, landed: S) -> (S, S) {
    match leaving {
        Side::Output => (dangling, landed),
        Side::Input => (landed, dangling),
    }
}

/// The producing and consuming PORTS of a candidate pair (R2133).
///
/// The mirror of [`ends`] one level down, and separate from it because the
/// ports come out of two different lists while the ends are two values of one
/// type. Written once for the same reason: the two questions must not orient
/// their ports differently from each other or from the wire.
fn pair<'a, K: NodeKind>(
    held: &'a [KindPort<K>],
    dangling_port: u32,
    offered: &'a [KindPort<K>],
    index: u32,
    leaving: Side,
) -> (&'a KindPort<K>, &'a KindPort<K>) {
    let (held, offered) = (&held[dangling_port as usize], &offered[index as usize]);
    match leaving {
        Side::Output => (held, offered),
        Side::Input => (offered, held),
    }
}

/// What a vet says about one candidate pin: which end's limit the wire would
/// exceed, and the link that would actually go (R2133).
///
/// A named alias because it is the return of the closure the two planners hand
/// to [`Document::plan_over`](Document::plan_over), and an unnamed nested
/// `Result<(Option<Side>, Option<Link>), _>` in a function signature is the
/// kind of type nobody reads twice.
type VettedPin<K, S> = Result<(Option<Side>, Option<Link>), ConnectError<<K as NodeKind>::Type, S>>;
