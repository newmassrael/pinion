//! ★★★★★ R1928 — **what THIS node calls one of its ports**, and who chose it.
//!
//! # What the reference does, measured at its own header, its one consumer and
//! all six of its overriders
//!
//! Its graph node publishes two overridable answers: *should this node be given
//! the chance to override pin names* (a bool) and *what is this pin called*
//! (a text). The header's own comment says how they compose — if the first
//! answers yes, the second is called **for each pin, each frame** — and there
//! is exactly one consumer, the schema's display-name call, which takes the
//! whole ordinary naming path as its `else`.
//!
//! Reading all six overriders is what shaped this module, and three of the
//! findings changed what was built:
//!
//! 1. ★★★★★ **The capability is mostly used to take a name AWAY, not to give a
//!    different one.** Two reroute classes answer the empty text for every pin
//!    with the comment *keep the pin size tiny*; a setter node answers the empty
//!    text for its output and its control pins and the ordinary name for the
//!    rest; and a fourth class answers the bool alone. **Four of the six
//!    suppress.** The other two hand back the node's own title — the same text
//!    for every pin of the node.
//! 2. ★★★★★ **Nobody names a pin per pin.** The signature is handed the pin, and
//!    five of the six ignore it. The one that reads it reads it to decide
//!    whether to suppress.
//! 3. 🟥🟥🟥 **"Show no name" and "I have nothing to say" are the same value
//!    there, and a class sits on the ambiguity.** The supplied answer to the
//!    second hook is the empty text, so a class that overrides the *bool* and
//!    not the *text* silently suppresses every one of its pin names — and one of
//!    the six does exactly that. Whether that is the intent or an omission
//!    cannot be told from the source.
//!
//! # The three measured ways this passes it
//!
//! 1. **One answer with three arms, not two hooks.** [`PortName`] says *keep the
//!    declared name*, *call it this instead*, or *show no name at all*. The
//!    state finding 3 describes — a node that opts in and then says nothing —
//!    is unrepresentable, and suppression is a thing a kind SAYS rather than
//!    something it falls into.
//! 2. ★★★★★ **The answer says who chose it.** [`Labelled`] carries a
//!    [`NameSource`], so a reader can tell the kind's own declaration from a
//!    name the author gave one item of a variadic run from this node's own
//!    answer. The reference hands back a bare text, which is the same half a
//!    bare string could not carry for descriptions in R1923 — and the name axis
//!    still had it.
//! 3. **One resolution point.** [`Document::port_label`] is where the three
//!    sources meet, in the order the reference's own consumer composes them:
//!    the node's answer wins outright, then the item's authored label, then the
//!    kind's declaration. Nothing else in this crate resolves a port's name, so
//!    two callers cannot disagree about what a port is called.
//!
//! # What was already here, and is NOT re-built
//!
//! A node has been able to name a port of a **variadic run** since R1632:
//! [`Item::label`](crate::Item::label) is the author's name for one item, and
//! `resolved_name` derives the unlabelled case from the ordinal. That is a real
//! instance-level naming power and this module does not duplicate it — it
//! *reports* it, as [`NameSource::Item`]. What was missing is a node naming a
//! **fixed** port at all, and any way at all to say a port shows no name.

use crate::model::{Document, NodeBody, NodeId, NodeKind, PortRef, Side, TreeId};

/// What a node calls one of its ports.
///
/// Three arms and not an `Option<String>`, because **an empty name is a real
/// answer here** — the reference's commonest use of this capability is to
/// suppress a pin's name entirely — and a type in which "show nothing" and
/// "I have nothing to say" are the same value is the type the reference has.
/// R1927 folded a two-hook pair into one `Option` for the opposite reason: there
/// the empty text meant nothing at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PortName {
    /// Keep the name the port was declared with. The supplied answer.
    #[default]
    Declared,
    /// Call it this instead.
    Instead(String),
    /// Show no name for this port at all.
    ///
    /// Not the empty string: a reader is told there is deliberately nothing to
    /// show, which is a different fact from a name that happens to be blank.
    Silent,
}

/// Which of the three sources produced a port's name.
///
/// The half a bare string cannot carry, on the naming axis. R1923 established
/// it for descriptions; a name had the same gap and nothing said so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameSource {
    /// The kind's own declaration — [`Port::name`](crate::Port::name).
    Kind,
    /// A name the author gave one item of a variadic run
    /// ([`Item::label`](crate::Item::label)).
    Item,
    /// This node's own answer, through [`NodeKind::port_name`].
    Node,
}

/// A port's name as a reader gets it, and who chose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labelled {
    /// The name to show, or `None` when the port is deliberately unlabelled.
    ///
    /// `None` only ever comes from [`PortName::Silent`], so a reader that finds
    /// it knows a decision was made rather than that a string was empty.
    pub text: Option<String>,
    /// Where that answer came from.
    pub source: NameSource,
}

impl Labelled {
    /// Whether anything is shown for this port.
    #[must_use]
    pub const fn is_shown(&self) -> bool {
        self.text.is_some()
    }
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1928 — **what this node calls the port at `at`**, and who chose
    /// that name — or `None` when the node has no port there.
    ///
    /// The one resolution point. Three sources, composed in the order the
    /// reference's own consumer composes its two:
    ///
    /// 1. the node's answer ([`NodeKind::port_name`]) wins outright, exactly as
    ///    the reference's override replaces the whole ordinary naming path;
    /// 2. otherwise the name the resolved signature carries, which is the
    ///    item's authored label when the port belongs to a labelled item of a
    ///    variadic run;
    /// 3. otherwise the kind's own declaration.
    ///
    /// A body that is not a kind — a group instance, an interface end, a frame,
    /// a delay — has no hook to ask, so its ports are always [`NameSource::Kind`].
    #[must_use]
    pub fn port_label(&self, tree: TreeId, node: NodeId, at: PortRef) -> Option<Labelled> {
        let signature = self.declared_signature(tree, node)?;
        let ports = match at.side {
            Side::Input => signature.inputs,
            Side::Output => signature.outputs,
        };
        let port = ports.get(at.index as usize)?;
        let held = self.tree(tree)?.node(node)?;
        if let NodeBody::Kind(kind) = &held.body {
            match kind.port_name(at, &port.name) {
                PortName::Instead(name) => {
                    return Some(Labelled {
                        text: Some(name),
                        source: NameSource::Node,
                    });
                }
                PortName::Silent => {
                    return Some(Labelled {
                        text: None,
                        source: NameSource::Node,
                    });
                }
                PortName::Declared => {}
            }
        }
        Some(Labelled {
            text: Some(port.name.clone()),
            source: if self.is_authored_item_port(tree, node, at) {
                NameSource::Item
            } else {
                NameSource::Kind
            },
        })
    }

    /// Every port's name on one side, in index order.
    ///
    /// Answers an empty list for a node that is not there, which is the same
    /// answer as a node with no ports on that side: what a caller does with
    /// either is to draw nothing.
    #[must_use]
    pub fn port_labels(&self, tree: TreeId, node: NodeId, side: Side) -> Vec<Labelled> {
        let Some(signature) = self.declared_signature(tree, node) else {
            return Vec::new();
        };
        let count = match side {
            Side::Input => signature.inputs.len(),
            Side::Output => signature.outputs.len(),
        };
        (0..count)
            .filter_map(|index| {
                self.port_label(
                    tree,
                    node,
                    PortRef {
                        side,
                        index: u32::try_from(index).unwrap_or(u32::MAX),
                    },
                )
            })
            .collect()
    }

    /// Whether the port at `at` belongs to an item of a variadic run that the
    /// author has NAMED.
    ///
    /// The arithmetic is the splice's, read the other way round: the run begins
    /// at the kind's own index `start`, each item contributes `stride` ports,
    /// and an item past the authored list is one the minimum topped up — which
    /// carries no label by construction.
    fn is_authored_item_port(&self, tree: TreeId, node: NodeId, at: PortRef) -> bool {
        let Some(run) = self.variadic(tree, node, at.side) else {
            return false;
        };
        let stride = run.stride();
        if stride == 0 || at.index < run.start() {
            return false;
        }
        let ordinal = (at.index - run.start()) / stride;
        self.items(tree, node, at.side)
            .and_then(|items| items.get(ordinal as usize).cloned())
            .is_some_and(|item| item.label.is_some())
    }
}
