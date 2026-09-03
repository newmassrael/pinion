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
//! # What is not here, stated rather than hidden
//!
//! The question is asked of a node **that exists**. A menu that wanted to grey
//! out the kinds which will wire nothing would have to ask it of a *kind*, and
//! that cannot be done in this vocabulary today: every arm of [`ConnectError`]
//! names [`Socket`]s, and a socket names a [`NodeId`] a node about to be
//! created does not have yet. Inventing a second refusal vocabulary for the
//! hypothetical case is precisely the drift this crate refuses elsewhere, so
//! the question is left open and registered rather than answered badly.

use std::fmt;

use crate::model::{
    ConnectError, Conversion, Document, Link, LinkId, NodeId, NodeKind, Side, Socket, TreeId,
    crossing,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declined<T> {
    /// The resolved index of the port that declined it.
    pub port: u32,
    /// That port as an address (R1914).
    pub at: PortPath,
    /// Why it declined.
    pub why: ConnectError<T>,
}

/// Why a node could not be wired to the pin that created it (R1987).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AutowireError<T> {
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
    /// The arriving node presents **no port at all** on the side that would
    /// have to take the wire.
    ///
    /// Its own arm rather than a [`Self::NoneTakes`] with an empty list,
    /// because the two are different facts and a person repairs them
    /// differently: *this kind never listens* is a choice about the kind, and
    /// *these pins all refused* is a question about the types. The reference
    /// cannot tell them apart — both are its empty hook body.
    NoPorts {
        /// The node that has none.
        node: NodeId,
        /// The side it has none on.
        side: Side,
    },
    /// It has ports on that side and **not one of them** takes the wire, with
    /// each one's own refusal.
    NoneTakes {
        /// One entry per port that was offered the wire, in declaration order.
        /// Never empty — that case is [`Self::NoPorts`].
        declined: Vec<Declined<T>>,
    },
}

impl<T: fmt::Debug> fmt::Display for AutowireError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {tree}"),
            Self::NoSuchNode(node) => write!(f, "no node {node}"),
            Self::NoSuchPort { socket, arity } => {
                write!(f, "{socket} is not there: that end has {arity} port(s)")
            }
            // ★ R1699/R1719 — a refusal says itself, because this sentence
            // reaches a person in a toast and an agent as a rejection's reason.
            // ★ R1987 — `noun`, not `name`. `name` is the wire form a client
            // parses ("in"/"out"), and putting it here read "has no in pin".
            Self::NoPorts { node, side } => {
                write!(f, "node {node} has no {} pin to take the wire", side.noun())
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

impl<T: fmt::Debug> std::error::Error for AutowireError<T> {}

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
        let offered = self
            .signature(tree, arriving)
            .ok_or(AutowireError::NoSuchNode(arriving))?;
        let side = leaving.other();
        let offered = match side {
            Side::Input => offered.inputs,
            Side::Output => offered.outputs,
        };
        if offered.is_empty() {
            return Err(AutowireError::NoPorts {
                node: arriving,
                side,
            });
        }
        let mut uptakes: Vec<(Uptake, Option<Side>)> = Vec::new();
        let mut declined: Vec<Declined<K::Type>> = Vec::new();
        for index in 0..u32::try_from(offered.len()).unwrap_or(u32::MAX) {
            let landed = Socket::new(arriving, index);
            let (from, to) = ends(dangling, leaving, landed);
            let at = self
                .path_of(tree, arriving, side, index)
                .unwrap_or_else(|| PortPath::root(index));
            // The one authority on whether the pair may be wired. Asked here
            // rather than re-derived, so a pin this admits is a pin `connect`
            // admits — which is what makes the verb above able to place without
            // asking again.
            match self.vet(tree, from, to) {
                Ok(crowded) => {
                    // `vet` passed, so the crossing is not refused: the only
                    // two answers left are the two arms of `Arrival`.
                    let (source, sink) = match leaving {
                        Side::Input => (&offered[index as usize], &held[dangling.port as usize]),
                        Side::Output => (&held[dangling.port as usize], &offered[index as usize]),
                    };
                    let arrival = match crossing::<K>(source, sink) {
                        Conversion::Converted(_) => Arrival::Converted,
                        Conversion::Direct | Conversion::Refused => Arrival::Unchanged,
                    };
                    uptakes.push((
                        Uptake {
                            port: index,
                            at,
                            arrival,
                            displaces: self.standing_at(tree, from, to, crowded),
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
const fn ends(dangling: Socket, leaving: Side, landed: Socket) -> (Socket, Socket) {
    match leaving {
        Side::Output => (dangling, landed),
        Side::Input => (landed, dangling),
    }
}
