//! ★★★★★ R1998 — **a paste offers a replacement for what the destination
//! will not take.**
//!
//! [`Document::insert`](crate::Document::insert) refuses a fragment whose
//! carried nodes the destination cannot hold, and the refusal is total: nothing
//! lands. That is the right default — a paste either happens or leaves the
//! document alone — but it is not the only answer available. A taxonomy often
//! knows what to put there instead, and this module is where it says so.
//!
//! # What the engine does, measured at its declaration, its one overrider and
//! its one call site
//!
//! Its schema publishes a hook that hands back a node to use *in place of* one
//! being pasted. The base implementation answers `nullptr`.
//!
//! One class overrides it. There, an **event** node being pasted becomes a
//! **custom event** node: the overrider refuses outright unless the destination
//! is the graph type that may hold events at all, gathers the names already in
//! use, renames the clashing object out of the way, and builds the replacement
//! holding the name the original arrived with. A list of extra names is
//! threaded through so that a batch of pastes cannot mint one name twice.
//!
//! The call site is the paste itself. For every constructed object it asks
//! *may you be pasted here*; if not, it asks the schema for a substitute, adds
//! whatever comes back to a list of substitutes, destroys the original when the
//! two differ, and spawns whatever is left.
//!
//! ## ⚠ The defect this crate does not repeat: one value for two facts
//!
//! `nullptr` is what the base answers — *this schema offers no substitution* —
//! and it is also what the overrider answers when the node may not be pasted
//! into this graph **at all**. The call site cannot tell them apart, and treats
//! both the same way: the node is destroyed and nothing is spawned. A person
//! who pasted five nodes and got four is told nothing about the fifth, and the
//! two reasons they might want to know — *nobody offered a stand-in* and *this
//! kind of node may not live here* — are the same silence.
//!
//! Here they are three different outcomes and all three are said out loud:
//!
//! * the taxonomy offers nothing, and the paste is refused with the reason the
//!   destination gave — [`InsertError::NameTaken`] or
//!   [`InsertError::InterfaceNodeInFragment`], the same refusals as before;
//! * the taxonomy offers a body that cannot land either, and the paste is
//!   refused with [`InsertError::SubstituteUnlandable`], which names the carried
//!   node **and** what was wrong with the stand-in;
//! * the taxonomy offers a body that lands, and the paste happens with
//!   [`Inserted::substituted`] naming every node that arrived as one thing and
//!   was placed as another.
//!
//! [`InsertError::NameTaken`]: crate::InsertError::NameTaken
//! [`InsertError::InterfaceNodeInFragment`]: crate::InsertError::InterfaceNodeInFragment
//! [`InsertError::SubstituteUnlandable`]: crate::InsertError::SubstituteUnlandable
//! [`Inserted::substituted`]: crate::Inserted::substituted
//!
//! ## ⚠ The second defect: a stand-in with different ports
//!
//! A substitute is a different body, so it has different ports, and the wires
//! the fragment carried were drawn against the old ones. The engine re-matches
//! its pins by name after the fact and quietly loses the wires that find no
//! partner. [`Document::insert`](crate::Document::insert) documents a guarantee
//! it would break by doing that — a fragment that satisfies
//! [`validate`](crate::Document::validate) inserts into a document that still
//! does — so a stand-in whose ports cannot carry what the original carried is
//! refused with [`InsertError::SubstituteCannotCarry`], before anything is
//! written.
//!
//! [`InsertError::SubstituteCannotCarry`]: crate::InsertError::SubstituteCannotCarry

use crate::items::{Items, resolve};
use crate::model::{Document, InterfaceSide, KindPort, NodeBody, NodeKind, Signature, crossing};
use crate::{NodeId, Side, TreeId};

/// ★★★★★ R1998 — why a carried body could not land where the paste was aimed.
///
/// The whole population of per-node refusals
/// [`Document::insert`](crate::Document::insert) has, which is what lets the
/// substitution hook be asked at every one of them rather than at whichever
/// happened to be convenient. A test asserts that population.
///
/// ## ⚠ Deliberately NOT `non_exhaustive`, which it was until it was read
///
/// A taxonomy answers [`crate::NodeKind::substitute`] by
/// matching on this, and what it should offer for a reason it has never seen
/// is a decision only that taxonomy can make. `non_exhaustive` would oblige
/// every implementor to write a wildcard arm, so a refusal added here later
/// would land silently in each of them — a default that reads like a decision
/// and is an escape hatch. Exhaustive, a new arm stops the compilation of
/// every taxonomy that has to reconsider, which is the whole point of telling
/// the hook why it is being asked at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unlandable {
    /// The body's kind declares [`Copying::Refused`](crate::Copying::Refused)
    /// and the destination already answers to the name the copy arrived with.
    ///
    /// The engine's own refusal, reached by the overriders that gather the
    /// names already in use.
    NameTaken {
        /// The name the copy arrived with, and which is already spoken for.
        label: String,
        /// The node holding it, and the tree that one is in — the scope may be
        /// wider than the destination tree.
        held_by: (TreeId, NodeId),
    },
    /// The body materialises the interface of the tree the fragment came from.
    ///
    /// Nothing [`Document::extract`](crate::Document::extract) builds carries
    /// one; a fragment that arrived from elsewhere can.
    InterfaceEnd(InterfaceSide),
}

impl core::fmt::Display for Unlandable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NameTaken { label, held_by } => write!(
                f,
                "it is called {label:?} and its kind refuses to be copied under another \
                 name, but node {} in tree {} already answers to it",
                held_by.1.0, held_by.0.0
            ),
            Self::InterfaceEnd(side) => write!(
                f,
                "it materialises the {} interface of the tree it came from",
                match side {
                    InterfaceSide::Input => "input",
                    InterfaceSide::Output => "output",
                }
            ),
        }
    }
}

/// ★★★★★ R1998 — a carried node the destination would not have taken as it
/// came, and what stands in its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Substitution {
    /// The carried node, addressed in the fragment.
    pub node: NodeId,
    /// What was placed instead, addressed in the destination tree.
    pub became: NodeId,
    /// Why the body it arrived with could not land.
    pub why: Unlandable,
}

/// What a body's ports will be, decided before it is placed.
///
/// Not every body can answer with types. A reroute, a beacon and an echo carry
/// whatever the chain they join carries, so their port *count* is fixed by the
/// body and their port *types* are fixed by what wires them — which is why
/// [`Document::declared_signature`](crate::Document) needs a placed node for
/// those three arms and this needs an arm of its own.
pub(crate) enum Prospect<K: NodeKind> {
    /// These ports, types and all.
    Declared(Signature<K>),
    /// This many inputs and outputs, of whatever the chain decides.
    Passing {
        /// How many ports values arrive at.
        inputs: usize,
        /// How many ports values leave from.
        outputs: usize,
    },
}

/// What is at one address on a body that has not been placed yet.
///
/// ★★★★★ Three answers, so a type rather than a nested `Option`. The first
/// draft answered `Option<Option<&KindPort<K>>>` with a comment explaining
/// which nesting meant what, and this crate's standing rule is that a question
/// with three answers gets a name for each — a caller who reads `Undecided`
/// cannot mistake it for *there is no such port*, and `Some(None)` invites
/// exactly that.
pub(crate) enum AtPort<'a, K: NodeKind> {
    /// The body has no port at that address.
    Absent,
    /// The port is there, and this is what it carries.
    Declared(&'a KindPort<K>),
    /// The port is there and what it carries is not decided by the body: it
    /// takes the type of whatever the chain hands it.
    Undecided,
}

impl<K: NodeKind> Prospect<K> {
    /// What is at input `port`.
    fn input(&self, port: u32) -> AtPort<'_, K> {
        match self {
            Self::Declared(signature) => signature
                .inputs
                .get(port as usize)
                .map_or(AtPort::Absent, AtPort::Declared),
            Self::Passing { inputs, .. } => {
                if (port as usize) < *inputs {
                    AtPort::Undecided
                } else {
                    AtPort::Absent
                }
            }
        }
    }

    /// What is at output `port`, on the same terms as [`Self::input`].
    fn output(&self, port: u32) -> AtPort<'_, K> {
        match self {
            Self::Declared(signature) => signature
                .outputs
                .get(port as usize)
                .map_or(AtPort::Absent, AtPort::Declared),
            Self::Passing { outputs, .. } => {
                if (port as usize) < *outputs {
                    AtPort::Undecided
                } else {
                    AtPort::Absent
                }
            }
        }
    }

    /// Whether a value leaving `out` can land on this body's input `port`.
    ///
    /// A [`Self::Passing`] body accepts whatever reaches it — that is what
    /// passing means — so for those this is the port existing.
    pub(crate) fn takes(&self, port: u32, out: &KindPort<K>) -> bool {
        match self.input(port) {
            AtPort::Absent => false,
            AtPort::Undecided => true,
            AtPort::Declared(here) => crossing::<K>(out, here).is_allowed(),
        }
    }
}

impl<K: NodeKind> Document<K> {
    /// What `body`'s ports would be if it were placed in `tree`, before it is.
    ///
    /// The bodies whose ports are the taxonomy's or a definition's answer
    /// resolve fully; the three that pass what wires them answer with counts.
    /// A group instance naming a tree this document does not have resolves to
    /// nothing at all, which is the same answer
    /// [`signature`](Document::signature) gives for it.
    pub(crate) fn prospect(&self, tree: TreeId, body: &NodeBody<K>) -> Option<Prospect<K>> {
        Some(match body {
            // A substitute arrives as a body, never as a node, so there are no
            // authored items to splice: it is a fresh node of its kind.
            NodeBody::Kind(kind) => Prospect::Declared(resolve(kind, &Items::default())),
            NodeBody::Group(inner) => {
                let definition = self.tree(*inner)?;
                Prospect::Declared(Signature {
                    inputs: definition.interface().inputs().to_vec(),
                    outputs: definition.interface().outputs().to_vec(),
                })
            }
            NodeBody::Interface(InterfaceSide::Input) => Prospect::Declared(Signature {
                inputs: Vec::new(),
                outputs: self.tree(tree)?.interface().inputs().to_vec(),
            }),
            NodeBody::Interface(InterfaceSide::Output) => Prospect::Declared(Signature {
                inputs: self.tree(tree)?.interface().outputs().to_vec(),
                outputs: Vec::new(),
            }),
            NodeBody::Frame => Prospect::Declared(Signature {
                inputs: Vec::new(),
                outputs: Vec::new(),
            }),
            NodeBody::Delay(ty) => Prospect::Declared(Signature {
                inputs: vec![KindPort::<K>::new("In", ty.clone())],
                outputs: vec![KindPort::<K>::new("Out", ty.clone())],
            }),
            NodeBody::Reroute | NodeBody::Beacon => Prospect::Passing {
                inputs: 1,
                outputs: 1,
            },
            // R1935 — an echo has no input at all: the value reaches it by name.
            NodeBody::Echo(_) => Prospect::Passing {
                inputs: 0,
                outputs: 1,
            },
        })
    }

    /// Whether `offered` can carry every wire the fragment drew to `node`.
    ///
    /// The port has to be there, and — when both ends can say what they carry —
    /// a value has to be able to cross. The ends that cannot say are the three
    /// [`Prospect::Passing`] bodies, and their silence is not a hole: a reroute
    /// takes the type of whatever feeds it, so a crossing into one is allowed
    /// by construction.
    ///
    /// Answers the offending socket and its side, which is what the refusal
    /// needs to name.
    pub(crate) fn substitute_carries(
        &self,
        tree: TreeId,
        fragment: &crate::Fragment<K>,
        node: NodeId,
        offered: &NodeBody<K>,
    ) -> Result<(), (u32, Side)> {
        let Some(prospect) = self.prospect(tree, offered) else {
            // The stand-in names a definition that is not here. It cannot carry
            // anything, so the first wire that touches it is the complaint.
            return match fragment
                .links()
                .iter()
                .find(|link| link.to.node == node || link.from.node == node)
            {
                Some(link) if link.to.node == node => Err((link.to.port, Side::Input)),
                Some(link) => Err((link.from.port, Side::Output)),
                None => Ok(()),
            };
        };
        for link in fragment.links() {
            if link.to.node == node {
                let here = match prospect.input(link.to.port) {
                    AtPort::Absent => return Err((link.to.port, Side::Input)),
                    AtPort::Undecided => None,
                    AtPort::Declared(here) => Some(here),
                };
                let there = fragment
                    .document()
                    .signature(crate::ROOT, link.from.node)
                    .and_then(|s| s.outputs.get(link.from.port as usize).cloned());
                if let (Some(here), Some(there)) = (here, there.as_ref())
                    && crossing::<K>(there, here).is_refused()
                {
                    return Err((link.to.port, Side::Input));
                }
            }
            if link.from.node == node {
                let here = match prospect.output(link.from.port) {
                    AtPort::Absent => return Err((link.from.port, Side::Output)),
                    AtPort::Undecided => None,
                    AtPort::Declared(here) => Some(here),
                };
                let there = fragment
                    .document()
                    .signature(crate::ROOT, link.to.node)
                    .and_then(|s| s.inputs.get(link.to.port as usize).cloned());
                if let (Some(here), Some(there)) = (here, there.as_ref())
                    && crossing::<K>(here, there).is_refused()
                {
                    return Err((link.from.port, Side::Output));
                }
            }
        }
        Ok(())
    }
}
