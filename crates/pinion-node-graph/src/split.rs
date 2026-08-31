//! R1912 — **whether a port carrying a composite value can be split into one
//! port per member**, and when it cannot, which of six reasons.
//!
//! This is the *question*, not the act. The engine asks it as
//! `CanSplitPin` — a node-side predicate the base class answers `false` to,
//! so a node kind opts in — and answers it with a conjunction:
//!
//! ```text
//! Pin->GetOwningNode() == this && !Pin->bNotConnectable
//!     && Pin->LinkedTo.Num() == 0 && Pin->PinType.PinCategory == PC_Struct
//! ```
//!
//! plus, at the moment of splitting, `StructType != nullptr && !IsContainer()`.
//!
//! ★★★★★ **Five conditions and one word back.** A caller that is told `false`
//! learns nothing about which one failed, and the repairs are entirely
//! different: unplug the wire, pick another port, or accept that this type has
//! no members at all. So the crate answers a [`NotSplittable`] that names the
//! reason, which is this axis's standing shape and the reference's own gap.
//!
//! ⚠ R1912 wrote here that the split itself was "deliberately NOT here".
//! **R1914 built it**, so that paragraph is gone rather than left standing as a
//! sentence a reader would have to date: the act is [`Document::split_port`]
//! and [`Document::recombine_port`], the member ports are spliced into
//! [`Document::signature`], the parent is [`Hidden::Split`](crate::Hidden), and the shape is a
//! TREE because the reference's recombine recurses.
//!
//! # ★★★★★ R1914 — the act, and the one decision it had to make first
//!
//! A split can be modelled two ways, and R1913 left the choice named rather
//! than taken: it either **changes the resolved signature**, so the member
//! ports are real ports every index after them shifts around, or it is a second
//! reading laid over an unchanged signature.
//!
//! It changes the signature, and the deciding argument is that **this crate
//! already has the same mechanism** for the same reason: a variadic run is a
//! per-node declaration spliced into a per-kind list ([`Variadic`], R1632), and
//! its three edits move links and authored values through **one**
//! correspondence so the two cannot disagree. A split is that shape exactly —
//! a per-node declaration that changes how many ports the node presents — and
//! modelling it as a second reading would mean a member port is somewhere a
//! wire cannot land, which is most of what a split is *for*.
//!
//! So the two addressing schemes are both here and they are not redundant:
//!
//! * a [`PortPath`] is the **declaration's** address — `member 1 of port 2` —
//!   and it is stable across splits of other ports;
//! * a resolved index is what the renderer draws, what [`Socket`] names and
//!   what a wire lands on, and it moves when anything before it splits.
//!
//! [`Document::path_of`] and [`Document::index_of`] are the correspondence, and
//! the split verbs report every port that moved the way an item edit does.
//!
//! [`Socket`]: crate::Socket
//! [`Variadic`]: crate::Variadic

use crate::model::{
    Document, Flow, KindPort, Link, NodeId, NodeKind, Port, PortRef, Side, Signature, TreeId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// ★ R1914 — a resolved port together with **the address it was declared at**.
///
/// Named because the pair is the whole point of the split model: the port is
/// what the renderer draws and what a wire lands on, and the address is what
/// survives another port splitting. Every reading of the expansion answers in
/// these, so a caller never has to hold one and re-derive the other.
pub type AddressedPort<K> = (PortPath, KindPort<K>);

/// R1912 — what a value type is **made of**.
///
/// The hook this crate did not have. Measured at R1912, the taxonomy trait
/// published twelve associated items and the two that speak about a type
/// answered *what type does this value have* and *does this type reach that
/// one* — neither decomposes one, and a run of repeated ports
/// ([`Variadic`](crate::Variadic)) is not the shape either: that repeats a
/// template the KIND fixes and never looks at a type.
///
/// Three arms rather than `Option<Vec<Port>>`, because the reference's own
/// precondition is a conjunction of two facts about the type and a caller told
/// `None` cannot tell them apart:
///
/// * [`Atom`](Composition::Atom) — nothing to split into.
/// * [`Container`](Composition::Container) — this holds elements, and the
///   reference refuses to split it **even when the element would split**. A
///   caller can offer "split an element" or say why not; with `None` it could
///   only say no.
/// * [`Members`](Composition::Members) — the ports one per member, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Composition<T, V> {
    /// Not composite: this type has no members.
    Atom,
    /// A container of some element type. Does not split, by the reference's own
    /// rule, even if its element would.
    Container,
    /// One port per member, in declaration order, each carrying the member's
    /// own name, type and resting value.
    Members(Vec<Port<T, V>>),
}

impl<T, V> Composition<T, V> {
    /// The members, or `None` for a type that does not split.
    #[must_use]
    pub const fn members(&self) -> Option<&Vec<Port<T, V>>> {
        match self {
            Self::Members(ports) => Some(ports),
            _ => None,
        }
    }
}

/// R1912 — the answer to *can this port be split*: the member ports it would
/// become, or the reason it would not.
///
/// Named rather than spelled at the one call site, and that is not only
/// clippy's line: the members are what an editor draws to preview the split, so
/// a caller holds this value rather than the question, and a value a caller
/// holds deserves a word.
pub type Splittable<K> =
    Result<Vec<Port<<K as NodeKind>::Type, <K as NodeKind>::Value>>, NotSplittable>;

/// ★★★★★ R1913 — whether taking a composite value apart and putting it back
/// gives the value back.
///
/// The law the reference cannot be given. Its two halves are hand-written
/// `if`-chains in an editor's schema over four named struct types, and for one
/// of them they use a different member order from each other — a disagreement
/// nothing there can check, because there is no one place the pair belongs to.
/// Here both halves are the taxonomy's, so a consumer can run this over its own
/// types and find out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundTrip {
    /// Apart and back again gave the same value.
    Holds,
    /// This type is not composite, so there was nothing to take apart. Reported
    /// rather than passing: a law that answers "fine" for every atom would be
    /// green on a taxonomy with no composite type at all.
    NotComposite,
    /// Taking it apart gave a different number of slots than the type has
    /// members.
    WrongArity {
        /// How many slots came back.
        got: usize,
        /// How many members the type declares.
        want: usize,
    },
    /// It came apart and would not go back together.
    LostIt,
    /// It went back together as something else. The arm the reference's
    /// order mismatch would land in.
    CameBackDifferent,
}

impl RoundTrip {
    /// Whether the law held. [`NotComposite`](RoundTrip::NotComposite) is
    /// **not** a hold: it means the law was never exercised, and a caller that
    /// treated it as one would be reporting coverage it does not have.
    #[must_use]
    pub const fn held(self) -> bool {
        matches!(self, Self::Holds)
    }
}

/// ★★★★★ R1913 — run the round-trip law for one value of one type.
///
/// `K::explode` then `K::implode`, compared with what went in. Any taxonomy can
/// run this over its own types; a consumer that does is checking the pair the
/// reference leaves unchecked.
pub fn round_trips<K: NodeKind>(ty: &K::Type, value: &K::Value) -> RoundTrip {
    let want = match K::composition(ty) {
        Composition::Members(members) => members.len(),
        Composition::Atom | Composition::Container => return RoundTrip::NotComposite,
    };
    let apart = K::explode(ty, value);
    if apart.len() != want {
        return RoundTrip::WrongArity {
            got: apart.len(),
            want,
        };
    }
    match K::implode(ty, &apart) {
        None => RoundTrip::LostIt,
        Some(back) if &back == value => RoundTrip::Holds,
        Some(_) => RoundTrip::CameBackDifferent,
    }
}

/// ★★★★★ R1913 — **a port address that can name a member.**
///
/// A [`Socket`](crate::Socket) is a node and a port INDEX, so `member 1 of port
/// 2` is unsayable — and a member port is a place a wire lands. Measured at
/// R1913, that is what stands between this crate and the split ACT: not the
/// verbs, and not the types, but somewhere to put what the verbs make.
///
/// The root is a resolved port index — the same number [`Socket::port`] carries
/// — and `members` is the path taken from there, one index per level. An empty
/// path is the port itself, so every existing address is a `PortPath` that
/// happens to go nowhere, which is the property that lets this be introduced
/// without a second addressing scheme.
///
/// ⚠ It NESTS, because the reference's recombine does: a member that is itself
/// composite splits again, so `[1, 0]` names member 0 of member 1. A path of
/// fixed depth would have been the wrong shape, and that is not a guess — it
/// was read off the reference's own recursion.
///
/// [`Socket::port`]: crate::Socket::port
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct PortPath {
    /// The port index this path starts at, **in the declaration** — the index
    /// the port would have if nothing on this side were split.
    ///
    /// ⚠ Not the resolved index, and R1914 keeps them apart on purpose: a
    /// resolved index moves when a port before it splits, so a declaration
    /// written in resolved indices would re-point itself every time a
    /// neighbouring port came apart. [`Document::index_of`] converts.
    pub port: u32,
    /// One member index per level, outermost first. Empty is the port itself.
    pub members: Vec<u32>,
}

impl PortPath {
    /// The port itself, naming no member.
    #[must_use]
    pub const fn root(port: u32) -> Self {
        Self {
            port,
            members: Vec::new(),
        }
    }

    /// This path with one more level taken.
    #[must_use]
    pub fn then(mut self, member: u32) -> Self {
        self.members.push(member);
        self
    }

    /// How many levels down this path goes. Zero is the port itself.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.members.len()
    }

    /// The path this one is a member of, or `None` for a root.
    ///
    /// The reference's parent-pin link, derived rather than stored: a stored
    /// back-pointer is a second fact that can disagree with the path, and this
    /// crate has paid for that class before.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        let mut up = self.clone();
        up.members.pop()?;
        Some(up)
    }

    /// ★ R1914 — whether this address is `other`, or lies underneath it.
    ///
    /// What makes folding a split fold everything under it. Written as a prefix
    /// test rather than by walking [`parent`](PortPath::parent) upward, because
    /// the walk answers the same question in time proportional to the depth
    /// **and** would have to allocate at every level to do it.
    #[must_use]
    pub fn is_at_or_below(&self, other: &Self) -> bool {
        self.port == other.port
            && self.members.len() >= other.members.len()
            && self.members[..other.members.len()] == other.members[..]
    }
}

/// R1913 — why an address names no port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoSuchMember {
    /// The path starts at a port this node does not have.
    NoSuchPort {
        /// The side asked about.
        side: Side,
        /// The root index asked about.
        index: u32,
        /// How many ports that side has.
        of: u32,
    },
    /// A level of the path went through a **control** port, which carries no
    /// value and therefore has no members.
    ThroughControl {
        /// How many levels in the path had been taken when it did.
        at: usize,
    },
    /// ★ R1934 — a level went through a port **nothing has decided yet**, which
    /// has no members because it does not know what it carries. Distinct from
    /// [`ThroughControl`](Self::ThroughControl): that one is permanent, this
    /// one ends the moment a wire decides the port.
    ThroughUndecided {
        /// How many levels had been taken.
        at: usize,
    },
    /// A level went through a type with no members.
    ThroughAtom {
        /// How many levels had been taken.
        at: usize,
        /// The member index that was asked for.
        member: u32,
    },
    /// A level went through a container, which does not split even when its
    /// element would — the reference's own refusal, met here as an address
    /// rather than as a gesture.
    ThroughContainer {
        /// How many levels had been taken.
        at: usize,
    },
    /// A level asked for a member past the end.
    NoSuchIndex {
        /// How many levels had been taken.
        at: usize,
        /// The member index asked for.
        member: u32,
        /// How many members that level has.
        of: usize,
    },
}

impl NoSuchMember {
    /// ★ R1914 — the word this reason is published under.
    ///
    /// The model's vocabulary, like [`NotSplittable::wire_word`] and
    /// [`Hidden::wire_word`](crate::Hidden::wire_word). `no_such_port` is
    /// deliberately the spelling R1913's screen already publishes: this arm
    /// absorbed the `NotSplittable::NoSuchPort` that used to answer it, and a
    /// re-spelling would have been a wire change dressed up as a refactor.
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::NoSuchPort { .. } => "no_such_port",
            Self::ThroughControl { .. } => "control",
            Self::ThroughUndecided { .. } => "undecided",
            Self::ThroughAtom { .. } => "atom",
            Self::ThroughContainer { .. } => "container",
            Self::NoSuchIndex { .. } => "no_such_member",
        }
    }
}

impl core::fmt::Display for NoSuchMember {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSuchPort { side, index, of } => {
                write!(f, "no {side:?} port at {index}; this node has {of}")
            }
            Self::ThroughControl { at } => write!(
                f,
                "level {at} of this path goes through a control port, which \
                 carries no value and so has no members"
            ),
            Self::ThroughUndecided { at } => write!(
                f,
                "level {at} of this path goes through a port nothing has \
                 decided yet, so what it is made of is not known"
            ),
            Self::ThroughAtom { at, member } => write!(
                f,
                "level {at} asks for member {member} of a type that has none"
            ),
            Self::ThroughContainer { at } => write!(
                f,
                "level {at} goes through a container, which does not split \
                 even when its element would"
            ),
            Self::NoSuchIndex { at, member, of } => {
                write!(f, "level {at} asks for member {member} of {of}")
            }
        }
    }
}

impl std::error::Error for NoSuchMember {}

/// R1912 — why a port cannot be split, in the caller's terms.
///
/// Six arms, and each is a **different repair**. The reference answers one
/// boolean over five conditions, which is why its own editor can only grey the
/// menu entry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotSplittable {
    /// No such node in that tree.
    NoSuchNode {
        /// The tree asked about.
        tree: TreeId,
        /// The node asked about.
        node: NodeId,
    },
    /// ★ R1914 — the **address** names no port: no root at that index, or a
    /// level of the path that could not be taken.
    ///
    /// This arm replaced a `NoSuchPort` that carried the same three fields as
    /// [`NoSuchMember::NoSuchPort`]. Two spellings of one fact is the thing
    /// this crate refuses everywhere else, and the address walk is the only
    /// place either is derived.
    Address(NoSuchMember),
    /// ★ A **control** port carries no value, so there is no value to take
    /// apart. The reference reaches the same refusal through its own
    /// not-connectable flag; here it falls out of the port's flow, which is one
    /// fact rather than two that could disagree.
    Control,
    /// ★★★★★ R1934 — **nothing has decided what this port carries yet**, so
    /// what it is made of is not known. A reroute's ports before a wire reaches
    /// the chain.
    ///
    /// Its own arm and not [`Control`](Self::Control), for the reason
    /// [`AlreadySplit`](Self::AlreadySplit) is not `Atom`: they are opposite
    /// repairs. A control port will NEVER split; this one splits as soon as
    /// something decides it, so the answer a screen should give a person is
    /// "wire it up first" rather than "this cannot be done".
    ///
    /// ⚠ The engine reaches the same state and collapses it: its own bend node
    /// answers the splittability predicate `false` unconditionally, so a bend
    /// carrying a composite — which its pins would happily split — is refused
    /// with the same word as one carrying an execution wire.
    Undecided,
    /// ★★★★★ R1914 — this address is **already split**.
    ///
    /// The reference reaches the same state and cannot say so: its own
    /// predicate answers `false` once a pin has sub-pins, so *already done* and
    /// *cannot be done* arrive as one word. They are opposite repairs — one is
    /// recombine, the other is give up — which is why they are separate arms.
    AlreadySplit,
    /// ★★★★★ Something is **wired** to this port.
    ///
    /// The condition a reading of the split alone would miss, and it is in the
    /// reference's predicate verbatim (`LinkedTo.Num() == 0`): a wire lands on
    /// the parent, and the parent is about to stop being a place a wire can
    /// land. Naming it is what lets an editor say *unplug it first* instead of
    /// greying a menu entry with no reason.
    Wired {
        /// The side the wired port is on.
        side: Side,
        /// Its index.
        index: u32,
    },
    /// This port's type has no members.
    Atom,
    /// This port's type is a container. The reference refuses this even when
    /// the element type would split.
    Container,
}

impl NotSplittable {
    /// ★★★★★ R1913 — the word this reason is published under.
    ///
    /// The vocabulary belongs to the model, the way
    /// [`Hidden::wire_word`](crate::Hidden::wire_word) does: a screen that
    /// spelled these itself would be a second list, and the two would drift the
    /// first time an arm is added. That is what makes a client's reading of
    /// them a reading of the rule rather than of a transcription.
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::NoSuchNode { .. } => "no_such_node",
            Self::Address(why) => why.wire_word(),
            Self::Control => "control",
            Self::Undecided => "undecided",
            Self::AlreadySplit => "already_split",
            Self::Wired { .. } => "wired",
            Self::Atom => "atom",
            Self::Container => "container",
        }
    }
}

impl core::fmt::Display for NotSplittable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {node:?} in tree {tree:?}")
            }
            Self::Address(why) => write!(f, "{why}"),
            Self::Control => write!(
                f,
                "a control port carries no value, so there is nothing to take \
                 apart"
            ),
            Self::Undecided => write!(
                f,
                "nothing has decided what this port carries yet, so what it is \
                 made of is not known; wire it up first"
            ),
            Self::AlreadySplit => write!(
                f,
                "this port is already split; recombine it before splitting it \
                 again"
            ),
            Self::Wired { side, index } => write!(
                f,
                "something is wired to {side:?} port {index}; splitting would \
                 take away the place that wire lands"
            ),
            Self::Atom => write!(f, "this port's type has no members"),
            Self::Container => write!(
                f,
                "this port's type is a container, which does not split even \
                 when its element would"
            ),
        }
    }
}

impl std::error::Error for NotSplittable {}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1912 — **can this port be split into one port per member, and if
    /// not, why not.**
    ///
    /// The engine's `CanSplitPin`, answered with a reason instead of a boolean.
    /// Returns the member ports the split would produce, in order, so a caller
    /// that asks the question does not then have to derive the answer a second
    /// way to draw it.
    ///
    /// # Errors
    ///
    /// [`NotSplittable`] — an absent node or port, a control port, a port
    /// something is **wired** to, a type with no members, or a container.
    pub fn splittable(&self, tree: TreeId, node: NodeId, side: Side, index: u32) -> Splittable<K> {
        // ★ R1914 — the node first, because `path_of` cannot tell an absent
        // node from a node with no ports and the repairs differ.
        if self.tree(tree).and_then(|host| host.node(node)).is_none() {
            return Err(NotSplittable::NoSuchNode { tree, node });
        }
        let path = self
            .path_of(tree, node, side, index)
            .ok_or(NotSplittable::Address(NoSuchMember::NoSuchPort {
                side,
                index,
                of: self.side_width(tree, node, side),
            }))?;
        self.splittable_at(tree, node, side, &path)
    }

    /// ★★★★★ R1914 — [`splittable`](Document::splittable) for an address that
    /// can name a **member**.
    ///
    /// The general question, of which the index form is the root case. The
    /// reference has no equivalent: its predicate takes a pin, and a sub-pin is
    /// a pin, so *can this member be split further* is askable there — but the
    /// answer is a boolean over five conditions, and the sub-pin case adds a
    /// sixth (already split) that arrives as the same word as the other five.
    ///
    /// # Errors
    ///
    /// [`NotSplittable`] — an absent node, an address naming no port, a control
    /// port, a port already split, a port something is **wired** to, a type
    /// with no members, or a container.
    pub fn splittable_at(
        &self,
        tree: TreeId,
        node: NodeId,
        side: Side,
        path: &PortPath,
    ) -> Splittable<K> {
        let host = self
            .tree(tree)
            .ok_or(NotSplittable::NoSuchNode { tree, node })?;
        if host.node(node).is_none() {
            return Err(NotSplittable::NoSuchNode { tree, node });
        }
        let port = self
            .port_at(tree, node, side, path)
            .map_err(NotSplittable::Address)?;

        if self.split_paths(tree, node, side).contains(path) {
            return Err(NotSplittable::AlreadySplit);
        }

        // ★ The wire check is the reference's own, and it reads the side it was
        // asked about: an input is wired when something arrives at it, an
        // output when something leaves by it. One question, two directions, and
        // a check that only knew one of them would let half the ports through.
        //
        // ★ R1914 — it reads the RESOLVED index, because that is what a wire
        // lands on. A member port is a place a wire lands, so this question is
        // about the same port whether or not the path went through a member.
        if let Some(index) = self.index_of(tree, node, side, path) {
            let socket = crate::Socket::new(node, index);
            let wired = match side {
                Side::Input => host.link_into(socket).is_some(),
                Side::Output => host.links().iter().any(|link| link.from == socket),
            };
            if wired {
                return Err(NotSplittable::Wired { side, index });
            }
        }

        member_ports::<K>(&port).map_err(|why| match why {
            NoMembers::Control => NotSplittable::Control,
            NoMembers::Undecided => NotSplittable::Undecided,
            NoMembers::Atom => NotSplittable::Atom,
            NoMembers::Container => NotSplittable::Container,
        })
    }

    /// ★★★★★ R1913 — **the port an address names**, following the path one
    /// member at a time.
    ///
    /// This is what makes [`PortPath`] mean something rather than being a pair
    /// of numbers: each level is resolved by asking the taxonomy what the
    /// level above is made of, so an address that cannot be walked is refused
    /// with the level it failed at.
    ///
    /// A root path answers the port itself, which is why an existing
    /// [`Socket`](crate::Socket) is this call with an empty path.
    ///
    /// ★ R1914 — the member ports it answers carry **their share of the
    /// parent's resting value** ([`NodeKind::explode`]), not an empty default.
    /// The reference does the same at the moment of splitting, and the reason
    /// is the same: a member port a reader has to fill in again has lost what
    /// the parent already said.
    ///
    /// ⚠ **This does not judge whether the node exists.** An absent node has no
    /// ports, so it answers `NoSuchPort { of: 0 }` — the same answer a node
    /// with no ports on that side gives. The two are a different repair (there
    /// is no node to look at, against look at another port), so every verb here
    /// asks about the node FIRST and answers
    /// [`NotSplittable::NoSuchNode`] itself. Stated rather than left for a
    /// caller to discover, because a refusal that quietly covers two facts is
    /// the class R1890 recorded.
    ///
    /// # Errors
    ///
    /// [`NoSuchMember`] — a root the node does not have, or a level that goes
    /// through a control port, an atom, a container, or past the end.
    pub fn port_at(
        &self,
        tree: TreeId,
        node: NodeId,
        side: Side,
        path: &PortPath,
    ) -> Result<Port<K::Type, K::Value>, NoSuchMember> {
        let signature = self
            .declared_signature(tree, node)
            .ok_or(NoSuchMember::NoSuchPort {
                side,
                index: path.port,
                of: 0,
            })?;
        let ports = match side {
            Side::Input => &signature.inputs,
            Side::Output => &signature.outputs,
        };
        let of = u32::try_from(ports.len()).unwrap_or(u32::MAX);
        let mut here = ports
            .get(path.port as usize)
            .cloned()
            .ok_or(NoSuchMember::NoSuchPort {
                side,
                index: path.port,
                of,
            })?;

        for (at, member) in path.members.iter().enumerate() {
            let members = member_ports::<K>(&here).map_err(|why| why.at(at, *member))?;
            let of = members.len();
            here = members
                .into_iter()
                .nth(*member as usize)
                .ok_or(NoSuchMember::NoSuchIndex {
                    at,
                    member: *member,
                    of,
                })?;
        }
        Ok(here)
    }
}

/// R1914 — why one level of a [`PortPath`] could not be taken, before the level
/// number is known.
///
/// A private half-error, and it exists so [`member_ports`] can be the **one**
/// place a member list is derived. Both callers — the address walk and the
/// signature splice — would otherwise re-spell the same three refusals, and a
/// splice that disagreed with the walk about what a port is made of is exactly
/// the silent corruption this crate spends its item edits avoiding.
enum NoMembers {
    Control,
    /// R1934 — nothing has decided what this port carries, so there is nothing
    /// to take apart YET. A separate arm from [`Control`](Self::Control)
    /// because they are opposite repairs: a control port will never split, and
    /// an undecided one splits as soon as a wire decides it.
    Undecided,
    Atom,
    Container,
}

impl NoMembers {
    /// This refusal, told which level it happened at.
    const fn at(self, at: usize, member: u32) -> NoSuchMember {
        match self {
            Self::Control => NoSuchMember::ThroughControl { at },
            Self::Undecided => NoSuchMember::ThroughUndecided { at },
            Self::Atom => NoSuchMember::ThroughAtom { at, member },
            Self::Container => NoSuchMember::ThroughContainer { at },
        }
    }
}

/// ★★★★★ R1914 — the ports one port splits into, **with the parent's value
/// shared out among them**.
///
/// The single derivation of a member list. Type structure comes from
/// [`NodeKind::composition`] and the values from [`NodeKind::explode`], and the
/// two are joined here rather than at each caller: a member whose type came
/// from one place and whose value came from another is two facts that can
/// disagree, and this crate has paid for that class before.
///
/// A member the parent's value does not determine keeps whatever resting value
/// the composition declared for it. That is not the same as `None` meaning
/// "empty": `explode` answering `None` says *the parent said nothing about this
/// member*, and the member's own declared default is then the best answer
/// available.
fn member_ports<K: NodeKind>(parent: &KindPort<K>) -> Result<Vec<KindPort<K>>, NoMembers> {
    // ★ R1934 — three flows, three answers. Before this round the `else` said
    // `Control` for everything that was not a value, so an UNDECIDED port —
    // which is not control, and which will split the moment a wire decides
    // it — was refused under the one word that means "never". A refusal whose
    // reason is wrong is the defect this round repaired in `PortTooltip`, and
    // an `if let ... else` is where the compiler cannot ask for the new arm.
    let flow = match &parent.flow {
        Flow::Value { ty, default } => (ty, default),
        Flow::Control => return Err(NoMembers::Control),
        Flow::Undecided => return Err(NoMembers::Undecided),
    };
    let (ty, default) = flow;
    let mut members = match K::composition(ty) {
        Composition::Atom => return Err(NoMembers::Atom),
        Composition::Container => return Err(NoMembers::Container),
        Composition::Members(members) if members.is_empty() => return Err(NoMembers::Atom),
        Composition::Members(members) => members,
    };
    if let Some(value) = default {
        let pieces = K::explode(ty, value);
        for (member, piece) in members.iter_mut().zip(pieces) {
            if let (Flow::Value { default, .. }, Some(piece)) = (&mut member.flow, piece) {
                *default = Some(piece);
            }
        }
    }
    Ok(members)
}

/// ★★★★★ R1914 — what a [`Document::split_port`] did.
///
/// [`ItemChange`](crate::ItemChange)'s shape, and deliberately so: a split
/// re-signatures a node exactly the way an item edit does, so the facts a
/// caller needs to undo it or to tell an author what it cost are the same
/// facts. The reference's own split command answers `void`.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitChange<K: NodeKind> {
    /// The address that came apart.
    pub parent: PortPath,
    /// The addresses it came apart into, in order.
    pub members: Vec<PortPath>,
    /// The resolved ports the split created, ascending.
    pub added: Vec<PortRef>,
    /// Ports that survived at a **different** resolved index, old then new.
    ///
    /// The half a naive implementation forgets — every port after the parent
    /// moves, on the same side, including ports of the fixed signature that
    /// have nothing to do with the split.
    pub moved: Vec<(PortRef, PortRef)>,
    /// Links the split had to cut, as they were. Empty in every case the
    /// reference permits, because it refuses to split a wired port — kept
    /// because a later relaxation must not become silent.
    pub severed: Vec<Link>,
    /// Authored values the split had to drop, with the port they were on.
    pub discarded: Vec<(PortRef, K::Value)>,
    /// ★★★★★ The pieces of the parent's **authored** value written onto the
    /// member ports, with the port each landed on.
    ///
    /// The reference does this and cannot report it: its split parses the
    /// parent's value into per-member defaults inside the command, so an editor
    /// wanting to undo the split has to know how to take those apart again.
    pub shared_out: Vec<(PortRef, K::Value)>,
}

impl<K: NodeKind> SplitChange<K> {
    /// Whether the split cost the graph nothing — no wire cut, no value
    /// dropped. Sharing the parent's value out is not a cost: the parent keeps
    /// it, and [`Document::recombine_port`] puts it back together.
    #[must_use]
    pub fn is_lossless(&self) -> bool {
        self.severed.is_empty() && self.discarded.is_empty()
    }
}

/// ★★★★★ R1914 — what a [`Document::recombine_port`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Recombined<K: NodeKind> {
    /// The address that went back together. ★ Not necessarily the address
    /// asked about: the verb is catchable at **either end**, so asking at a
    /// member folds the member's parent, which is what the reference does when
    /// its command is invoked on a sub-pin.
    pub parent: PortPath,
    /// How many split declarations went away, this one included. Greater than
    /// one when a member was itself split: the shape is a tree, and folding a
    /// parent folds everything under it.
    pub folded: usize,
    /// Ports that survived at a different resolved index, old then new.
    pub moved: Vec<(PortRef, PortRef)>,
    /// Links the recombine had to cut, as they were — a wire landing on a
    /// member port has nowhere to go once the member is not a port.
    pub severed: Vec<Link>,
    /// Authored values dropped that were **not** composed back into the parent.
    pub discarded: Vec<(PortRef, K::Value)>,
    /// ★★★★★ The value [`NodeKind::implode`] made from the members and wrote
    /// onto the parent, or `None` when the members did not determine one.
    ///
    /// **The half the reference does not have.** Measured at R1913, its
    /// recombine re-composes a parent's value only for four named struct types,
    /// with a hand-written chain that disagrees with its own split's chain
    /// about member order for one of them; every other composite type simply
    /// keeps whatever the parent had. Here the taxonomy owns both directions
    /// and [`round_trips`] is the law that holds them together.
    pub composed: Option<K::Value>,
}

/// ★★★★★ R1914 — why a port could not be put back together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotRecombinable {
    /// No such node in that tree.
    NoSuchNode {
        /// The tree asked about.
        tree: TreeId,
        /// The node asked about.
        node: NodeId,
    },
    /// The address names no port.
    Address(NoSuchMember),
    /// ★ Neither this address nor any ancestor of it is split, so there is
    /// nothing to fold.
    ///
    /// Distinct from [`NotSplittable::AlreadySplit`]'s mirror image on purpose:
    /// a caller told this knows the gesture was a no-op, where a caller told
    /// only `false` — which is all the reference's greyed-out menu entry says —
    /// cannot tell that from a port it is not allowed to touch.
    NotSplit,
}

impl NotRecombinable {
    /// The word this reason is published under.
    #[must_use]
    pub const fn wire_word(&self) -> &'static str {
        match self {
            Self::NoSuchNode { .. } => "no_such_node",
            Self::Address(why) => why.wire_word(),
            Self::NotSplit => "not_split",
        }
    }
}

impl core::fmt::Display for NotRecombinable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSuchNode { tree, node } => write!(f, "no node {node:?} in tree {tree:?}"),
            Self::Address(why) => write!(f, "{why}"),
            Self::NotSplit => write!(
                f,
                "neither this port nor any port it is a member of is split"
            ),
        }
    }
}

impl std::error::Error for NotRecombinable {}

impl<K: NodeKind> Document<K> {
    /// R1914 — the split addresses declared on one side of a node, ascending.
    #[must_use]
    pub fn split_paths(&self, tree: TreeId, node: NodeId, side: Side) -> Vec<PortPath> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let Some(node) = host.node(node) else {
            return Vec::new();
        };
        let mut paths = match side {
            Side::Input => node.appearance.split_inputs.clone(),
            Side::Output => node.appearance.split_outputs.clone(),
        };
        paths.sort();
        paths
    }

    /// ★★★★★ R1914 — the node's ports as the renderer sees them, each with the
    /// **address** it was declared at.
    ///
    /// The one expansion. The spliced signature, the resolved-index/address
    /// correspondence in both directions, and the set of hidden split parents
    /// are all read off this list, so they cannot disagree about where a member
    /// port went — which is precisely the corruption class the item edits
    /// document at length.
    #[must_use]
    pub fn resolved_ports(&self, tree: TreeId, node: NodeId, side: Side) -> Vec<AddressedPort<K>> {
        let Some(signature) = self.declared_signature(tree, node) else {
            return Vec::new();
        };
        let declared = match side {
            Side::Input => signature.inputs,
            Side::Output => signature.outputs,
        };
        let splits = self.split_paths(tree, node, side);
        let mut out = Vec::new();
        for (index, port) in declared.into_iter().enumerate() {
            let root = PortPath::root(u32::try_from(index).unwrap_or(u32::MAX));
            expand::<K>(&splits, &root, port, &mut out);
        }
        out
    }

    /// R1914 — splice the node's splits into a declared signature, in place.
    pub(crate) fn splice_splits(&self, tree: TreeId, node: NodeId, signature: &mut Signature<K>) {
        // ★ The cheap exit is not an optimisation: `resolved_ports` calls
        // `declared_signature`, and a node with no split must not pay for a
        // second derivation of the list it was just handed.
        if self.split_paths(tree, node, Side::Input).is_empty()
            && self.split_paths(tree, node, Side::Output).is_empty()
        {
            return;
        }
        signature.inputs = self
            .resolved_ports(tree, node, Side::Input)
            .into_iter()
            .map(|(_, port)| port)
            .collect();
        signature.outputs = self
            .resolved_ports(tree, node, Side::Output)
            .into_iter()
            .map(|(_, port)| port)
            .collect();
    }

    /// R1914 — the **resolved** indices of the ports a split hid, on one side.
    pub(crate) fn split_parents(&self, tree: TreeId, node: NodeId, side: Side) -> Vec<u32> {
        let splits = self.split_paths(tree, node, side);
        if splits.is_empty() {
            return Vec::new();
        }
        self.resolved_ports(tree, node, side)
            .into_iter()
            .enumerate()
            .filter(|(_, (path, _))| splits.contains(path))
            .map(|(index, _)| u32::try_from(index).unwrap_or(u32::MAX))
            .collect()
    }

    /// ★★★★★ R1914 — the **resolved index** an address currently sits at, or
    /// `None` when the address names no port.
    ///
    /// One half of the correspondence between the two addressing schemes. A
    /// caller holding an address and wanting to draw, hit-test or wire the port
    /// asks this; the answer changes when a port before it splits, which is
    /// exactly why the declaration is not written this way.
    #[must_use]
    pub fn index_of(&self, tree: TreeId, node: NodeId, side: Side, path: &PortPath) -> Option<u32> {
        self.resolved_ports(tree, node, side)
            .iter()
            .position(|(at, _)| at == path)
            .map(|index| u32::try_from(index).unwrap_or(u32::MAX))
    }

    /// ★★★★★ R1914 — the address the port at a resolved index was declared at,
    /// or `None` when there is no such port.
    ///
    /// The other half. A gesture arrives carrying the index the renderer drew,
    /// and every split verb takes an address, so this is the conversion an
    /// editor makes on the way in.
    #[must_use]
    pub fn path_of(&self, tree: TreeId, node: NodeId, side: Side, index: u32) -> Option<PortPath> {
        self.resolved_ports(tree, node, side)
            .into_iter()
            .nth(index as usize)
            .map(|(path, _)| path)
    }

    /// R1914 — how many ports one side **declares**, before any split.
    fn side_width(&self, tree: TreeId, node: NodeId, side: Side) -> u32 {
        let Some(signature) = self.declared_signature(tree, node) else {
            return 0;
        };
        let width = match side {
            Side::Input => signature.inputs.len(),
            Side::Output => signature.outputs.len(),
        };
        u32::try_from(width).unwrap_or(u32::MAX)
    }

    /// ★★★★★ R1914 — **split a port into one port per member of its type.**
    ///
    /// The engine's `SplitPin` / `SplitStructPin`. The parent keeps its place
    /// and is hidden ([`Hidden::Split`](crate::Hidden::Split)); its members are
    /// spliced in immediately after it, each carrying its share of the parent's
    /// value; and every port after them on that side moves, with its links and
    /// its authored values, through one correspondence.
    ///
    /// Catchable at **either end** of a tree that is already split: a member
    /// that is itself composite splits again, which is why the address is a
    /// [`PortPath`] and not an index.
    ///
    /// # Errors
    ///
    /// [`NotSplittable`] — see [`splittable_at`](Document::splittable_at),
    /// which this asks first and never contradicts.
    pub fn split_port(
        &mut self,
        tree: TreeId,
        node: NodeId,
        side: Side,
        path: &PortPath,
    ) -> Result<SplitChange<K>, NotSplittable> {
        let members = self.splittable_at(tree, node, side, path)?;
        let before = self.address_index(tree, node, side);

        // ★ The parent's AUTHORED value, not its resting one: the resting value
        // is already shared out by the splice (`member_ports`), and doing it
        // twice would write the same pieces through two different paths. What
        // the splice cannot reach is the value this NODE was given, which lives
        // on the node and outranks the kind's.
        let authored = self
            .index_of(tree, node, side, path)
            .and_then(|index| self.authored_value(tree, node, PortRef { side, index }));
        let ty = match self.port_at(tree, node, side, path).map(|port| port.flow) {
            Ok(Flow::Value { ty, .. }) => Some(ty),
            _ => None,
        };

        let slot = self
            .tree_mut(tree)
            .and_then(|t| t.node_mut(node))
            .ok_or(NotSplittable::NoSuchNode { tree, node })?;
        let declared = match side {
            Side::Input => &mut slot.appearance.split_inputs,
            Side::Output => &mut slot.appearance.split_outputs,
        };
        declared.push(path.clone());
        declared.sort();

        let after = self.address_index(tree, node, side);
        let mut how = correspondence(&before, &after, side);
        self.keep_other_side(tree, node, side, &mut how.total);
        let (severed, discarded) = self.remap_ports(tree, node, &how.total);
        let (moved, added) = (how.moved, how.added);

        let member_paths: Vec<PortPath> = (0..members.len())
            .map(|m| path.clone().then(u32::try_from(m).unwrap_or(u32::MAX)))
            .collect();

        // ★ The authored value is shared out AFTER the remap, so the indices it
        // is written at are the new ones. Doing it before would have written
        // the pieces onto ports the remap then moved.
        let mut shared_out = Vec::new();
        if let (Some(value), Some(ty)) = (authored, ty) {
            let pieces = K::explode(&ty, &value);
            for (member, piece) in member_paths.iter().zip(pieces) {
                let (Some(piece), Some(index)) = (piece, self.index_of(tree, node, side, member))
                else {
                    continue;
                };
                let at = PortRef { side, index };
                if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(node)) {
                    slot.values.insert(at, piece.clone());
                }
                shared_out.push((at, piece));
            }
        }

        Ok(SplitChange {
            parent: path.clone(),
            members: member_paths,
            added,
            moved,
            severed,
            discarded,
            shared_out,
        })
    }

    /// ★★★★★ R1914 — **put a split port back together.**
    ///
    /// The engine's `RecombinePin` / `RecombineStructPin`, and it is catchable
    /// at **either end** the way the reference's is: asked at a member, it folds
    /// the port that member belongs to; asked at a split parent, it folds that
    /// parent. Folding a parent folds everything under it, because a member
    /// that was itself split stops being a port.
    ///
    /// The members' authored values are composed back onto the parent with
    /// [`NodeKind::implode`] — the half the reference does not have for any type
    /// outside a hand-written chain of four.
    ///
    /// # Errors
    ///
    /// [`NotRecombinable`] — an absent node, an address naming no port, or an
    /// address with no split at it or above it.
    pub fn recombine_port(
        &mut self,
        tree: TreeId,
        node: NodeId,
        side: Side,
        path: &PortPath,
    ) -> Result<Recombined<K>, NotRecombinable> {
        if self.tree(tree).and_then(|host| host.node(node)).is_none() {
            return Err(NotRecombinable::NoSuchNode { tree, node });
        }
        self.port_at(tree, node, side, path)
            .map_err(NotRecombinable::Address)?;

        // ★ EITHER END. The address asked about is folded when it is itself
        // split; otherwise the nearest ancestor that is split is, which is what
        // makes the gesture work from a member port. The reference reaches the
        // same place by delegating a parent's command to its first sub-pin.
        let declared = self.split_paths(tree, node, side);
        let mut walk = path.clone();
        let target = loop {
            if declared.contains(&walk) {
                break walk;
            }
            match walk.parent() {
                Some(up) => walk = up,
                None => return Err(NotRecombinable::NotSplit),
            }
        };

        let before = self.address_index(tree, node, side);
        let composed = self.compose_upward(tree, node, side, &target, &declared);

        let doomed: Vec<PortPath> = declared
            .iter()
            .filter(|at| at.is_at_or_below(&target))
            .cloned()
            .collect();
        let folded = doomed.len();
        if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(node)) {
            let kept = match side {
                Side::Input => &mut slot.appearance.split_inputs,
                Side::Output => &mut slot.appearance.split_outputs,
            };
            kept.retain(|at| !doomed.contains(at));
        }

        let after = self.address_index(tree, node, side);
        let mut how = correspondence(&before, &after, side);
        self.keep_other_side(tree, node, side, &mut how.total);
        let (severed, discarded) = self.remap_ports(tree, node, &how.total);
        let moved = how.moved;

        // ★ The composed value is written LAST, at the parent's new resolved
        // index, because the remap has just moved every surviving value and a
        // value written before it would have been moved again or dropped.
        if let (Some(value), Some(index)) =
            (composed.clone(), self.index_of(tree, node, side, &target))
        {
            if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(node)) {
                slot.values.insert(PortRef { side, index }, value);
            }
        }

        Ok(Recombined {
            parent: target,
            folded,
            moved,
            severed,
            discarded,
            composed,
        })
    }

    /// R1914 — the addresses of one side's resolved ports, in resolved order.
    fn address_index(&self, tree: TreeId, node: NodeId, side: Side) -> Vec<PortPath> {
        self.resolved_ports(tree, node, side)
            .into_iter()
            .map(|(path, _)| path)
            .collect()
    }

    /// R1914 — the value this NODE was given for a resolved port, if any.
    fn authored_value(&self, tree: TreeId, node: NodeId, at: PortRef) -> Option<K::Value> {
        self.tree(tree)?.node(node)?.values.get(&at).cloned()
    }

    /// R1914 — say, explicitly, that the side a split did not touch is
    /// unchanged.
    ///
    /// ⚠ Not a formality. [`Document::remap_ports`] treats a port absent from
    /// its map as **gone**, so leaving the untouched side out would sever every
    /// wire on it and hand back every value authored there. The item edits
    /// carry the same paragraph beside the same three lines, and it is written
    /// twice rather than shared because the two derive their maps differently.
    fn keep_other_side(
        &self,
        tree: TreeId,
        node: NodeId,
        side: Side,
        into: &mut BTreeMap<PortRef, PortRef>,
    ) {
        let other = side.other();
        let width = self.resolved_ports(tree, node, other).len();
        for index in 0..u32::try_from(width).unwrap_or(u32::MAX) {
            into.insert(
                PortRef { side: other, index },
                PortRef { side: other, index },
            );
        }
    }

    /// ★★★★★ R1914 — the value the members of `target` compose to, folding the
    /// deepest level first.
    ///
    /// Depth first, and that is the whole reason this is a function rather than
    /// a line: a member that is itself split has no authored value of its own —
    /// its value lives on ITS members — so composing the outer level before the
    /// inner one would compose from a slot that has not been filled yet. The
    /// reference's recombine recurses for the same reason and then drops the
    /// value anyway for every type outside its chain of four.
    fn compose_upward(
        &mut self,
        tree: TreeId,
        node: NodeId,
        side: Side,
        target: &PortPath,
        declared: &[PortPath],
    ) -> Option<K::Value> {
        let port = self.port_at(tree, node, side, target).ok()?;
        let Flow::Value { ty, default } = &port.flow else {
            return None;
        };
        let ty = ty.clone();
        let resting = default.clone();
        let members = member_ports::<K>(&port).ok()?;

        let mut pieces: Vec<Option<K::Value>> = Vec::with_capacity(members.len());
        for (m, member) in members.iter().enumerate() {
            let at = target.clone().then(u32::try_from(m).unwrap_or(u32::MAX));
            if declared.contains(&at) {
                pieces.push(self.compose_upward(tree, node, side, &at, declared));
                continue;
            }
            // ★ The node's authored value outranks the port's resting one,
            // which is R1594's rule read in the direction this needs it: a
            // member a hand edited must be what comes back.
            let authored = self
                .index_of(tree, node, side, &at)
                .and_then(|index| self.authored_value(tree, node, PortRef { side, index }));
            pieces.push(authored.or_else(|| member.flow.default_value().cloned()));
        }

        // ★ The members' own authored values come off the node here rather than
        // being left for the remap to report as `discarded`: they were not
        // lost, they were folded into the parent, and reporting a fold as a
        // loss is a lie an editor would show to an author.
        for m in 0..members.len() {
            let at = target.clone().then(u32::try_from(m).unwrap_or(u32::MAX));
            if let Some(index) = self.index_of(tree, node, side, &at) {
                if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(node)) {
                    slot.values.remove(&PortRef { side, index });
                }
            }
        }

        K::implode(&ty, &pieces).or(resting)
    }
}

/// R1914 — one port's expansion into itself and, when it is split, its members.
///
/// The parent comes FIRST and its members follow it, which is the reference's
/// own order (`bHidden` on the parent, sub-pins appended after it) and the one
/// that keeps an index stable under a split of a LATER port.
fn expand<K: NodeKind>(
    splits: &[PortPath],
    path: &PortPath,
    port: KindPort<K>,
    out: &mut Vec<AddressedPort<K>>,
) {
    let members = splits
        .contains(path)
        .then(|| member_ports::<K>(&port).ok())
        .flatten();
    out.push((path.clone(), port));
    let Some(members) = members else { return };
    for (index, member) in members.into_iter().enumerate() {
        let at = path.clone().then(u32::try_from(index).unwrap_or(u32::MAX));
        expand::<K>(splits, &at, member, out);
    }
}

/// R1914 — the old-to-new port correspondence between two resolved orders, plus
/// the ports the new order added.
///
/// Addresses are the identity, which is what lets this be a comparison rather
/// than arithmetic over the split's position — and arithmetic over a position
/// is precisely how the reference's own blend-list node acquired the
/// re-indexing defect its source carries as a `@TODO`.
///
/// ⚠ The map is **total over the side**, identities included, because
/// [`Document::remap_ports`] severs every link and drops every value whose port
/// it cannot find. `unchanged_side` is what makes it total over the *node*: the
/// side a split did not touch has to say so, or every wire on it is cut.
struct Correspondence {
    /// Every old port to its new one, identities included.
    total: BTreeMap<PortRef, PortRef>,
    /// Only the ports that actually changed index — what a caller is told.
    moved: Vec<(PortRef, PortRef)>,
    /// Ports the new order has and the old one did not.
    added: Vec<PortRef>,
}

fn correspondence(before: &[PortPath], after: &[PortPath], side: Side) -> Correspondence {
    let mut total = BTreeMap::new();
    let mut moved = Vec::new();
    for (old, path) in before.iter().enumerate() {
        let Some(new) = after.iter().position(|at| at == path) else {
            continue;
        };
        let (from, to) = (
            PortRef {
                side,
                index: u32::try_from(old).unwrap_or(u32::MAX),
            },
            PortRef {
                side,
                index: u32::try_from(new).unwrap_or(u32::MAX),
            },
        );
        total.insert(from, to);
        if old != new {
            moved.push((from, to));
        }
    }
    let added = after
        .iter()
        .enumerate()
        .filter(|(_, path)| !before.contains(path))
        .map(|(index, _)| PortRef {
            side,
            index: u32::try_from(index).unwrap_or(u32::MAX),
        })
        .collect();
    Correspondence {
        total,
        moved,
        added,
    }
}
