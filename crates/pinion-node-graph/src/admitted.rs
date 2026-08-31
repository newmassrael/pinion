//! ★★★★★ R1933 — **what socket types a tree will admit**: one declaration, read
//! both by the edit that refuses and by the chooser that offers.
//!
//! # What the two references do, measured separately — and they are NOT one
//! mechanism
//!
//! The slice's two rows sit in different trees and the standing warning applies:
//! measure each, then decide.
//!
//! * **The DCC's is the real per-tree restriction.** A tree TYPE carries a hook
//!   asked with a socket type, and four tree types implement it — a shader tree
//!   answers a nine-member whitelist, and texture, composite and geometry trees
//!   each answer their own. It is consumed in three places: making an interface
//!   socket refuses an unsupported type, retyping an existing interface socket
//!   refuses with *item to be copied to this interface is of an unsupported
//!   socket type*, and an operator uses it to FIND a type it may offer.
//! * **The engine's is a chooser filter.** Its schema is asked with a schema
//!   ACTION and a pin type, supplied `true`, and — measured across the whole
//!   engine source — it has **zero** overriders; its one consumer is the pin
//!   type selector widget, filtering the list a person picks from.
//!
//! ⇒ two different subjects, and both are readings of ONE fact: which socket
//! types are legal in this graph. The DCC reads it to REFUSE and to OFFER; the
//! engine only to offer. So one declaration with two readers is the shape, and
//! writing the rule twice — once for the refusal and once for the list — is the
//! two-oracle defect R1924 and R1930 each paid for.
//!
//! # The three measured ways this passes them
//!
//! 1. **The offer is DERIVED from the refusal, not written beside it.**
//!    [`Document::offered_types`] answers the same list
//!    [`Document::admits_type`] judges against, so a chooser that showed a type
//!    the edit would refuse is unrepresentable. In the DCC these are a hook and
//!    a separate operator that loops over every registered socket type asking
//!    the hook; in the engine the offer exists and the refusal does not.
//! 2. **`Anything` is a value.** A tree that restricts nothing says so, and the
//!    difference between *no restriction* and *a restriction nobody wrote* is a
//!    difference a caller can see. The DCC spells the first as a null function
//!    pointer, which every consumer has to remember to check — three of them do,
//!    in three places.
//! 3. **The restriction is per TREE, not per tree type.** A document here holds
//!    one taxonomy, so a tree type would be a level that does not exist; a
//!    definition that admits a narrower set than the root is expressible, which
//!    is what a library of typed sub-graphs needs and what a per-type hook
//!    cannot say.
//!
//! # What this deliberately does NOT do
//!
//! It does not police [`Document::add_node`]. Neither reference does either —
//! the DCC's three consumers are all about the INTERFACE, and a node dropped
//! into a tree keeps whatever ports its kind declares. A restriction that
//! rejected nodes would make a tree's admitted set a second, weaker copy of the
//! taxonomy's own port declarations.

use serde::{Deserialize, Serialize};

use crate::model::{Document, EditError, NodeKind, TreeId};

/// The socket types a tree will admit on its interface.
///
/// Serialised with the tree, because a restriction that survived only in memory
/// would be a rule a saved graph reopens without — and R1921's persistence gate
/// is what would catch that a round later rather than now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
pub enum Admitted<T> {
    /// Any type the taxonomy has. The supplied answer.
    Anything,
    /// Only these, in the order a chooser should offer them.
    ///
    /// The order is the declaration's, because a list a person picks from has
    /// one and deriving it from anything else would make the offer depend on
    /// how the set happened to be stored.
    These(Vec<T>),
}

impl<T> Default for Admitted<T> {
    fn default() -> Self {
        Self::Anything
    }
}

impl<T: PartialEq> Admitted<T> {
    /// Whether this declaration lets `ty` through.
    #[must_use]
    pub fn admits(&self, ty: &T) -> bool {
        match self {
            Self::Anything => true,
            Self::These(these) => these.contains(ty),
        }
    }

    /// The types to offer, or `None` when the declaration names none — which is
    /// *anything*, and a caller that has to render a list is told to ask the
    /// taxonomy rather than handed an empty one.
    ///
    /// ⚠ `Some(&[])` is a real and different answer: a tree that admits NOTHING.
    /// Collapsing it into `None` would make "offer everything" and "offer
    /// nothing" the same value, which is the conflation R1928 measured on
    /// another axis.
    #[must_use]
    pub fn offered(&self) -> Option<&[T]> {
        match self {
            Self::Anything => None,
            Self::These(these) => Some(these),
        }
    }
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1933 — **declare which socket types `tree` admits on its
    /// interface.**
    ///
    /// One declaration, and everything else is derived from it: the refusal
    /// [`expose`](Document::expose) makes and the list
    /// [`offered_types`](Document::offered_types) answers.
    ///
    /// ⚠ Does not re-check the ports already exposed. That is deliberate and
    /// stated rather than hidden: narrowing a set a tree already breaks is a
    /// judgement about existing content, which [`Document::validate`] is for —
    /// and an edit that silently deleted interface ports would take links with
    /// them. [`Document::unadmitted_ports`] is how a caller asks what a
    /// narrowing left behind.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] when `tree` is not in the document.
    pub fn set_admitted(
        &mut self,
        tree: TreeId,
        admitted: Admitted<K::Type>,
    ) -> Result<(), EditError> {
        let host = self.tree_mut(tree).ok_or(EditError::NoSuchTree(tree))?;
        host.admitted = admitted;
        Ok(())
    }

    /// What `tree` admits. [`Admitted::Anything`] for a tree that is not there,
    /// which is the same answer as a tree that restricts nothing — both mean
    /// *this asks nothing of a type*.
    #[must_use]
    pub fn admitted(&self, tree: TreeId) -> Admitted<K::Type> {
        self.tree(tree)
            .map_or(Admitted::Anything, |host| host.admitted.clone())
    }

    /// Whether `tree` admits `ty` on its interface.
    #[must_use]
    pub fn admits_type(&self, tree: TreeId, ty: &K::Type) -> bool {
        self.tree(tree).is_none_or(|host| host.admitted.admits(ty))
    }

    /// The types a chooser should offer for `tree`'s interface, or `None` when
    /// the tree restricts nothing.
    ///
    /// ★ THE SAME list [`admits_type`](Document::admits_type) judges against, so
    /// an offer and a refusal cannot disagree. That is the whole of what the two
    /// references' two hooks are, put together.
    #[must_use]
    pub fn offered_types(&self, tree: TreeId) -> Option<Vec<K::Type>> {
        match self.admitted(tree) {
            Admitted::Anything => None,
            Admitted::These(these) => Some(these),
        }
    }

    /// The interface ports of `tree` whose type it no longer admits, as
    /// `(side, index)`.
    ///
    /// What a narrowing left behind. Answered rather than prevented, because
    /// deleting an exposed port takes its links with it and that is a decision
    /// for whoever narrowed the set — which is the same reason
    /// [`Document::validate`] reports a document rather than repairing it.
    #[must_use]
    pub fn unadmitted_ports(&self, tree: TreeId) -> Vec<(crate::InterfaceSide, u32)> {
        use crate::InterfaceSide;
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for side in [InterfaceSide::Input, InterfaceSide::Output] {
            let ports = match side {
                InterfaceSide::Input => host.interface().inputs(),
                InterfaceSide::Output => host.interface().outputs(),
            };
            for (index, port) in ports.iter().enumerate() {
                if let crate::Flow::Value { ty, .. } = &port.flow
                    && !host.admitted.admits(ty)
                {
                    found.push((side, u32::try_from(index).unwrap_or(u32::MAX)));
                }
            }
        }
        found
    }
}
