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
//! ⚠ **What is deliberately NOT here.** The split itself — the member ports,
//! the parent hiding, the value distributed across them, and the TREE the
//! reference's recombine walks (a member that is composite splits again) — is
//! the next slice. This one closes the question, and the question is what
//! forces the type-structure hook the act will need
//! ([`NodeKind::composition`](crate::NodeKind::composition)).

use crate::model::{Document, Flow, NodeId, NodeKind, Port, Side, TreeId};

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
    /// The node has no port at that index on that side.
    NoSuchPort {
        /// The side asked about.
        side: Side,
        /// The index asked about.
        index: u32,
        /// How many ports that side actually has.
        of: u32,
    },
    /// ★ A **control** port carries no value, so there is no value to take
    /// apart. The reference reaches the same refusal through its own
    /// not-connectable flag; here it falls out of the port's flow, which is one
    /// fact rather than two that could disagree.
    Control,
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

impl core::fmt::Display for NotSplittable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {node:?} in tree {tree:?}")
            }
            Self::NoSuchPort { side, index, of } => {
                write!(f, "no {side:?} port at {index}; this node has {of} of them")
            }
            Self::Control => write!(
                f,
                "a control port carries no value, so there is nothing to take \
                 apart"
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
        let host = self
            .tree(tree)
            .ok_or(NotSplittable::NoSuchNode { tree, node })?;
        let signature = self
            .signature(tree, node)
            .ok_or(NotSplittable::NoSuchNode { tree, node })?;
        let ports = match side {
            Side::Input => &signature.inputs,
            Side::Output => &signature.outputs,
        };
        let of = u32::try_from(ports.len()).unwrap_or(u32::MAX);
        let port =
            ports
                .get(index as usize)
                .ok_or(NotSplittable::NoSuchPort { side, index, of })?;

        let Flow::Value { ty, .. } = &port.flow else {
            return Err(NotSplittable::Control);
        };

        // ★ The wire check is the reference's own, and it reads the side it was
        // asked about: an input is wired when something arrives at it, an
        // output when something leaves by it. One question, two directions, and
        // a check that only knew one of them would let half the ports through.
        let socket = crate::Socket::new(node, index);
        let wired = match side {
            Side::Input => host.link_into(socket).is_some(),
            Side::Output => host.links().iter().any(|link| link.from == socket),
        };
        if wired {
            return Err(NotSplittable::Wired { side, index });
        }

        match K::composition(ty) {
            Composition::Atom => Err(NotSplittable::Atom),
            Composition::Container => Err(NotSplittable::Container),
            Composition::Members(members) if members.is_empty() => Err(NotSplittable::Atom),
            Composition::Members(members) => Ok(members),
        }
    }
}
