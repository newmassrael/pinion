//! Changing what a node IS, without changing which node it is (R1598).
//!
//! The DCC's `swap_node`, and the shape is where this diverges: there the
//! operator **creates a new node and deletes the old one**
//! (`bl_operators/node.py`), so the swapped node's identity dies with it.
//! Every reference to it dies too — a selection, a saved layout, an agent
//! holding the id, an undo record. Here the node keeps its [`NodeId`] and only
//! its body changes, which is what makes a swap an *edit* rather than a
//! replace-and-hope.
//!
//! The hard part is not the body: it is that a kind DECLARES its ports
//! (R1594), so changing the kind changes the signature, and every link and
//! every authored value on the node has to be re-examined against the new one.
//! What survives is decided by one derivation ([`Correspondence`]) and everything that does
//! not is **named**. The DCC drops all of it silently — three swallowed
//! exceptions (`except IndexError: pass`, `except KeyError: pass`, `except (AttributeError, KeyError, TypeError): pass`) and a `tree.links.remove(new_link)` for a link that turned out invalid —
//! so a swap there can quietly cost work the user cannot see they have lost.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    Document, EditError, KindPort, Link, NodeBody, NodeId, NodeKind, Port, PortRef, Side, TreeId,
    crossing,
};

/// One port of the old signature answered by one port of the new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Carried {
    /// Where it was.
    pub from: PortRef,
    /// Where it is now.
    pub to: PortRef,
    /// Whether the two ports have the same name.
    ///
    /// A name match is the author's own statement that these are the same port,
    /// which is why it is tried first and why a caller may want to treat a
    /// positional match as the weaker evidence it is.
    pub by_name: bool,
}

/// What a [`set_kind`](Document::set_kind) did.
///
/// Every field is something the DCC's swap does not report: it drops what does
/// not fit inside swallowed exceptions, so "the swap worked" and "the swap
/// worked and cost you two wires" are the same outcome there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swapped<K: NodeKind> {
    /// Ports of the old signature that the new one answers, ascending.
    pub carried: Vec<Carried>,
    /// Ports the new signature has no answer for, ascending.
    ///
    /// A port is here when no port of the same side survived to take it —
    /// because the new kind has fewer, or because the types cannot cross.
    pub dropped: Vec<PortRef>,
    /// Links that touched a dropped port and are gone.
    pub severed: Vec<Link>,
    /// Authored values ([`Node::values`](crate::Node::values)) that were on a
    /// dropped port and are gone, **with what they were**.
    ///
    /// The value and not just its address, because the address alone cannot
    /// answer the question a report exists for — the swap has already
    /// happened, so "port in1 lost something" leaves the caller nothing to
    /// show or to put back, while "port in1 lost the number 7" does. The DCC
    /// loses these inside `except (AttributeError, KeyError, TypeError): pass`.
    pub discarded: Vec<(PortRef, K::Value)>,
}

impl<K: NodeKind> Default for Swapped<K> {
    fn default() -> Self {
        Self {
            carried: Vec::new(),
            dropped: Vec::new(),
            severed: Vec::new(),
            discarded: Vec::new(),
        }
    }
}

impl<K: NodeKind> Swapped<K> {
    /// Whether the swap kept everything the node had.
    #[must_use]
    pub fn lossless(&self) -> bool {
        self.severed.is_empty() && self.discarded.is_empty()
    }
}

/// How the old signature's ports map onto the new one's, one side at a time.
///
/// **By name, then by position, and never against the type relation.** the DCC
/// picks one of those two rules from a hard-coded pair of node-type sets (`transfer_by_index = both_math_nodes or both_switch_nodes`,
/// two literal lists in a Python file), so a wire between two kinds nobody put
/// in those lists is silently dropped even when the ports line up perfectly.
/// Doing both, in that order, needs no table: a name match is the author
/// saying "this is the same port", and position is the honest fallback when
/// nobody said anything.
///
/// The result is **injective** by construction — each new port is claimed at
/// most once — which is what keeps a swap from over-feeding an input.
struct Correspondence {
    taken: BTreeMap<u32, u32>,
    by_name: BTreeSet<u32>,
}

impl Correspondence {
    /// Match `old` onto `new` for one side, `crosses` deciding whether what
    /// leaves the old port may enter the new one.
    ///
    /// R1599 — the predicate takes the PORTS rather than their types, because a
    /// control port has no type to hand it. A swap therefore carries a control
    /// wire across when both ends are control, and never pairs a control port
    /// with a value one, from the same rule that governs a link.
    fn build<T, V>(
        old: &[Port<T, V>],
        new: &[Port<T, V>],
        crosses: &impl Fn(&Port<T, V>, &Port<T, V>) -> bool,
    ) -> Self {
        let mut taken: BTreeMap<u32, u32> = BTreeMap::new();
        let mut claimed: BTreeSet<u32> = BTreeSet::new();
        let mut by_name: BTreeSet<u32> = BTreeSet::new();
        let at = |i: usize| u32::try_from(i).unwrap_or(u32::MAX);

        // Pass one: the author's own statement.
        for (index, port) in old.iter().enumerate() {
            let found = new.iter().enumerate().find(|(candidate, other)| {
                other.name == port.name
                    && !claimed.contains(&at(*candidate))
                    && crosses(port, other)
            });
            if let Some((candidate, _)) = found {
                taken.insert(at(index), at(candidate));
                claimed.insert(at(candidate));
                by_name.insert(at(index));
            }
        }
        // Pass two: position, for what pass one left over.
        for (index, port) in old.iter().enumerate() {
            if taken.contains_key(&at(index)) || claimed.contains(&at(index)) {
                continue;
            }
            let Some(other) = new.get(index) else {
                continue;
            };
            if crosses(port, other) {
                taken.insert(at(index), at(index));
                claimed.insert(at(index));
            }
        }
        // Pass three, and ONLY for a side that had exactly one port: the first
        // port that will take it.
        //
        // A lone port has no position worth preserving and no name that means
        // anything beside a name it is the only one of — so "wherever it fits"
        // is the honest answer rather than a guess, which is why this is not
        // done when there are several to tell apart. The DCC reaches the same
        // behaviour by testing `old_node.idname == "NodeReroute"`, so there
        // it is one node TYPE's privilege; here it falls out of the arity, and
        // every single-port kind gets it.
        if old.len() == 1 && !taken.contains_key(&0) {
            let found = new
                .iter()
                .enumerate()
                .find(|(candidate, other)| {
                    !claimed.contains(&at(*candidate)) && crosses(&old[0], other)
                })
                .map(|(candidate, _)| at(candidate));
            if let Some(candidate) = found {
                taken.insert(0, candidate);
                claimed.insert(candidate);
            }
        }
        Self { taken, by_name }
    }
}

impl<K: NodeKind> Document<K> {
    /// Change what `node` IS, keeping which node it is.
    ///
    /// The DCC's `swap_node`. The node's [`NodeId`], its position, its label,
    /// its appearance and its place in the frame forest all survive, because
    /// the node is not replaced — only its body is. That is the whole
    /// difference from the reference, where the operator creates a new node
    /// and deletes the old one, so every reference to it dies.
    ///
    /// A kind declares its ports, so the signature changes underneath the
    /// node's links and its authored values. Both are re-examined against the
    /// new signature by one derivation — by name, then by position, never
    /// against the type relation — and everything that does not survive is
    /// reported in [`Swapped`] rather than dropped in silence.
    ///
    /// **No swap can create a cycle or over-feed an input**, which is why this
    /// answers no `ConnectError`: links only ever move between ports of the same
    /// node, so the node-to-node edge set can lose members and never gain one,
    /// and the correspondence is injective so no two links can land on one
    /// input.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`], [`EditError::NoSuchNode`], or
    /// [`EditError::NotAKind`] for a frame, a group instance or an interface
    /// node — a body this crate owns is not the application's to overwrite.
    pub fn set_kind(
        &mut self,
        tree: TreeId,
        node: NodeId,
        kind: K,
    ) -> Result<Swapped<K>, EditError> {
        let Some(before) = self.signature(tree, node) else {
            return Err(if self.tree(tree).is_none() {
                EditError::NoSuchTree(tree)
            } else {
                EditError::NoSuchNode { tree, node }
            });
        };
        let held = self
            .tree(tree)
            .and_then(|t| t.node(node))
            .ok_or(EditError::NoSuchNode { tree, node })?;
        if !matches!(held.body, NodeBody::Kind(_)) {
            return Err(EditError::NotAKind { tree, node });
        }

        // R1632 — the new kind's signature is resolved against the node's own
        // items, clamped to what that kind's declaration allows, because the
        // ports the node is about to have are what the correspondence must be
        // built from. A four-branch sequencer swapped for a two-branch-max kind
        // loses two branches, and losing them HERE means the correspondence
        // reports the wires and the values that went with them, instead of the
        // signature and the model disagreeing afterwards.
        let held = self
            .tree(tree)
            .and_then(|t| t.node(node))
            .map(|held| held.items.clone())
            .unwrap_or_default();
        let items = crate::items::clamp(&kind, held);
        let after_signature = crate::items::resolve(&kind, &items);
        let crosses = |from: &KindPort<K>, to: &KindPort<K>| crossing::<K>(from, to).is_allowed();
        let inputs = Correspondence::build(&before.inputs, &after_signature.inputs, &crosses);
        let outputs = Correspondence::build(&before.outputs, &after_signature.outputs, &crosses);

        let mut swapped = Swapped::<K>::default();
        for (side, map, arity) in [
            (Side::Input, &inputs, before.inputs.len()),
            (Side::Output, &outputs, before.outputs.len()),
        ] {
            for index in 0..u32::try_from(arity).unwrap_or(u32::MAX) {
                let from = PortRef { side, index };
                match map.taken.get(&index) {
                    Some(&to) => swapped.carried.push(Carried {
                        from,
                        to: PortRef { side, index: to },
                        by_name: map.by_name.contains(&index),
                    }),
                    None => swapped.dropped.push(from),
                }
            }
        }

        // A link on a carried port MOVES to where that port went, and one on a
        // dropped port is severed and named — and the authored values follow
        // the same map, which is why one call does both (R1632).
        let moved: BTreeMap<PortRef, PortRef> =
            swapped.carried.iter().map(|c| (c.from, c.to)).collect();
        if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(node)) {
            slot.body = NodeBody::Kind(kind);
            slot.items = items;
        }
        let (severed, discarded) = self.remap_ports(tree, node, &moved);
        swapped.severed = severed;
        swapped.discarded = discarded;
        Ok(swapped)
    }
}
