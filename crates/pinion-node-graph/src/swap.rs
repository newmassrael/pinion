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

use crate::items::Items;
use crate::model::{
    Container, Document, EditError, KindPort, Link, NodeBody, NodeId, NodeKind, Port, PortRef,
    ROOT, Side, Signature, Socket, TreeId, crossing,
};

/// Why a node could not be made to stand for a definition (R1936).
///
/// Its own type rather than an arm on [`EditError`] because every one of these
/// is about the SWAP — what the node is, what the definition is, and whether
/// the two would nest inside each other — and a caller repairing one of them
/// does something different for each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapError {
    /// The tree is not in this document.
    NoSuchTree(TreeId),
    /// That node is not in the tree.
    NoSuchNode {
        /// The tree asked about.
        tree: TreeId,
        /// The node asked about.
        node: NodeId,
    },
    /// A body this crate owns and an application may not overwrite: a frame, an
    /// interface end, a register, a bend, or either half of a name.
    ///
    /// Named for what is refused rather than for one of the bodies, because the
    /// list grows: R1935 added two to it, and an error naming only the ones
    /// that existed when it was written goes quietly out of date.
    NotSwappable {
        /// The tree it is in.
        tree: TreeId,
        /// The node whose body may not be overwritten.
        node: NodeId,
    },
    /// The root tree is the document, not a definition, so nothing can stand
    /// for it.
    NotADefinition(TreeId),
    /// No such definition.
    NoSuchDefinition(TreeId),
    /// The swap would make a definition contain itself, along this chain.
    Recursion {
        /// The existing containment chain the swap would close.
        chain: Vec<TreeId>,
    },
}

impl std::fmt::Display for SwapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {} in tree {}", node.0, tree.0)
            }
            Self::NotSwappable { tree, node } => write!(
                f,
                "node {} in tree {} is a body this crate owns and cannot be made to stand for a definition",
                node.0, tree.0
            ),
            Self::NotADefinition(tree) => {
                write!(f, "tree {} is the root and cannot be stood for", tree.0)
            }
            Self::NoSuchDefinition(tree) => write!(f, "no definition {}", tree.0),
            Self::Recursion { chain } => {
                f.write_str("that would nest a group inside itself: ")?;
                for (i, tree) in chain.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" contains ")?;
                    }
                    write!(f, "{}", tree.0)?;
                }
                f.write_str(", which would then contain the first")
            }
        }
    }
}

impl std::error::Error for SwapError {}

/// What a [`set_zone_kind`](Document::set_zone_kind) did (R2003).
///
/// Two [`Swapped`] reports and the pairing between them, because a zone swap is
/// one gesture over **two** nodes and a caller repairing it has to be told about
/// each end separately: an end can lose ports the other kept.
///
/// The reference answers `{'FINISHED'}` and nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneSwapped<K: NodeKind> {
    /// What the addressed node was before the edit — exactly the answer
    /// [`in_zone`](Document::in_zone) gave.
    ///
    /// The published vocabulary and not a second one invented for this report,
    /// so *it was a closer* and *it was an opener waiting for a closer* reach a
    /// caller in the words it already reads them in. It also records **which
    /// end the person addressed**, which a screen wants: focus goes back where
    /// the gesture happened, not to whichever end the storage keys on.
    pub was: Option<crate::InZone>,
    /// The node that opens the zone now. The addressed node, unless the person
    /// addressed the closer.
    pub opens: NodeId,
    /// The node that closes it, or `None` — an opener that had nothing closing
    /// it still has nothing closing it, because this edit changes what a zone
    /// IS and does not invent a node to complete one.
    pub closes: Option<NodeId>,
    /// What the swap cost the opening node.
    pub opened: Swapped<K>,
    /// What it cost the closing node, or `None` when this edit MADE that node —
    /// something made a moment ago had nothing to lose.
    pub closed: Option<Swapped<K>>,
    /// The addressed node's outputs, handed to the closer this edit made for
    /// it, ascending.
    ///
    /// Empty for a zone that already had two ends, where nothing is handed
    /// anywhere. It is the record of the flow being SPLIT: what fed the node
    /// goes on feeding the opener, and what the node fed is fed by the closer.
    pub handed: Vec<Carried>,
}

impl<K: NodeKind> ZoneSwapped<K> {
    /// The closer this edit made, or `None` when it made none.
    ///
    /// Derived from [`was`](Self::was) rather than stored beside it, so the two
    /// cannot disagree about whether a node is new: a node is made exactly when
    /// the addressed one was in no zone at all.
    #[must_use]
    pub fn made(&self) -> Option<NodeId> {
        if self.was.is_none() {
            self.closes
        } else {
            None
        }
    }

    /// Whether both ends kept everything they had.
    #[must_use]
    pub fn lossless(&self) -> bool {
        self.opened.lossless() && self.closed.as_ref().is_none_or(Swapped::lossless)
    }
}

/// Why a zone's kind could not be changed (R2003).
///
/// Its own type rather than arms on [`SwapError`] because the question is a
/// different one — that asks what a NODE may become, this asks what a
/// bracketed REGION may become — and one of these refusals is about a node the
/// caller never mentioned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneSwapError {
    /// The tree is not in this document.
    NoSuchTree(TreeId),
    /// That node is not in the tree.
    NoSuchNode {
        /// The tree asked about.
        tree: TreeId,
        /// The node asked about.
        node: NodeId,
    },
    /// A body this crate owns has no kind to swap — a frame, a group instance,
    /// an interface end, a bend, or either half of a name.
    NotAKind {
        /// The tree it is in.
        tree: TreeId,
        /// The node asked about.
        node: NodeId,
    },
    /// The kind asked for opens no zone, so there is no zone to make this into.
    ///
    /// ★ It carries no payload, and that is deliberate: the kind is the
    /// caller's own argument, so handing it back says nothing the caller does
    /// not have. What a caller needs is WHICH of the things it passed was
    /// wrong, and the arm is that.
    KindOpensNothing,
    /// The taxonomy would not place the closer this edit had to make.
    ///
    /// ⚠ The refused node is not the one the caller named. A kind may declare
    /// where it belongs ([`Document::may`]), and a zone's two ends are two
    /// kinds — so a zone can be unopenable in a tree that would have taken its
    /// opener perfectly happily. Reported with the taxonomy's own refusal
    /// rather than folded into a general no.
    CloserRefused {
        /// The tree it would have gone in.
        tree: TreeId,
        /// What the taxonomy said.
        why: EditError,
    },
}

impl std::fmt::Display for ZoneSwapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {} in tree {}", node.0, tree.0)
            }
            Self::NotAKind { tree, node } => write!(
                f,
                "node {} in tree {} is a body this crate owns and has no kind to swap",
                node.0, tree.0
            ),
            Self::KindOpensNothing => f.write_str("that kind opens no zone"),
            Self::CloserRefused { tree, why } => write!(
                f,
                "tree {} would not take the node that closes the zone: {why}",
                tree.0
            ),
        }
    }
}

impl std::error::Error for ZoneSwapError {}

/// Why one port's type could not be chosen (R1937).
///
/// Its own type rather than an arm on [`SwapError`] because the question is a
/// different one — that asks what a node may STAND FOR, this asks what one of
/// its ports may CARRY — and a screen repairs them differently: one offers
/// another definition, the other offers another type or no chooser at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetypeError {
    /// That node is not in the tree.
    NoSuchNode {
        /// The tree asked about.
        tree: TreeId,
        /// The node asked about.
        node: NodeId,
    },
    /// The node has no port at that address.
    NoSuchPort {
        /// The tree it is in.
        tree: TreeId,
        /// The node asked about.
        node: NodeId,
        /// The address that answered nothing.
        port: PortRef,
    },
    /// A body this crate owns has no kind to ask.
    NotAKind {
        /// The tree it is in.
        tree: TreeId,
        /// The node asked about.
        node: NodeId,
    },
    /// The kind declined: this port's type is not a person's to choose, or not
    /// to be that.
    ///
    /// ★ The DEFAULT answer, and therefore what every kind that has not opted
    /// in says. A refusal rather than a silent no-op because the caller asked
    /// for something and is entitled to know it did not happen — and because
    /// the same declaration is what a screen asks before offering a chooser.
    Refused {
        /// The tree it is in.
        tree: TreeId,
        /// The node that declined.
        node: NodeId,
        /// The port it declined for.
        port: PortRef,
    },
}

impl std::fmt::Display for RetypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchNode { tree, node } => {
                write!(f, "no node {} in tree {}", node.0, tree.0)
            }
            Self::NoSuchPort { tree, node, port } => {
                write!(f, "node {} in tree {} has no port {port}", node.0, tree.0)
            }
            Self::NotAKind { tree, node } => write!(
                f,
                "node {} in tree {} is a body this crate owns and has no kind to ask",
                node.0, tree.0
            ),
            Self::Refused { tree, node, port } => write!(
                f,
                "node {} in tree {} does not let port {port} be given that type",
                node.0, tree.0
            ),
        }
    }
}

impl std::error::Error for RetypeError {}

/// ★ R1936 — **what is about to go where**: the signature a node is leaving
/// and the two correspondences that answer it.
///
/// One type rather than three arguments because they are one fact and always
/// travel together — the arity a report counts against comes from `before`, and
/// reading it from anywhere else is how the two halves of a swap come to
/// disagree. Clippy asked for this by refusing an eight-argument function, and
/// it was right: a parameter list that long is usually a type nobody has named.
struct Plan<K: NodeKind> {
    /// The signature the node had.
    before: Signature<K>,
    /// Where each input went, or that it went nowhere.
    inputs: Correspondence,
    /// Where each output went, or that it went nowhere.
    outputs: Correspondence,
}

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
    /// ★★★★★ R2003 — the node this one was in a ZONE with, left standing alone
    /// because the swap took the pairing's support away.
    ///
    /// A zone is a pair of node kinds that declare each other, so changing what
    /// one end IS can end the zone without the caller having mentioned zones at
    /// all: an opener swapped for a kind that opens nothing, or a closer swapped
    /// for a kind the opener never declared.
    ///
    /// ⚠ Measured at R2003, this was **silent**: the pairing stayed in the tree
    /// and both ends went on answering [`in_zone`](Document::in_zone) as though
    /// the zone were still there. It is now dropped and named, and it is named
    /// here rather than left to the caller to notice because the caller asked
    /// about a KIND — nothing in the request mentions a zone, so nothing in the
    /// request would prompt anybody to go and look.
    ///
    /// ★ [`set_zone_kind`](Document::set_zone_kind) is the verb that changes a
    /// zone's kind WITHOUT this happening, and this field is why it has to
    /// exist rather than being two calls to [`set_kind`](Document::set_kind).
    pub unpaired: Option<NodeId>,
}

impl<K: NodeKind> Default for Swapped<K> {
    fn default() -> Self {
        Self {
            carried: Vec::new(),
            dropped: Vec::new(),
            severed: Vec::new(),
            discarded: Vec::new(),
            unpaired: None,
        }
    }
}

impl<K: NodeKind> Swapped<K> {
    /// Whether the swap kept everything the node had.
    ///
    /// ★ R2003 — a lost zone counts. It is something the node had, and a report
    /// that called a swap lossless while the bracketed region it was an end of
    /// stopped existing would be answering about ports rather than about the
    /// node.
    #[must_use]
    pub fn lossless(&self) -> bool {
        self.severed.is_empty() && self.discarded.is_empty() && self.unpaired.is_none()
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

        let plan = Plan {
            before,
            inputs,
            outputs,
        };
        Ok(self.respecify(tree, node, &plan, NodeBody::Kind(kind), items))
    }

    /// ★★★★★ R1936 — the half of a swap that does not care WHAT the node is
    /// becoming: build the report from a correspondence, put the new body in,
    /// and move the wires and the authored values along it.
    ///
    /// Lifted out at R1936 rather than copied, because the round added a second
    /// verb that changes a node's body — [`set_definition`](Self::set_definition)
    /// — and two copies of *what survives a swap* is exactly the drift this
    /// crate keeps paying for. The reference makes the same split and does not:
    /// its `swap_empty_group` calls its `swap_node` and then reaches back in to
    /// fix the result up, so the two disagree about what a swap is.
    fn respecify(
        &mut self,
        tree: TreeId,
        node: NodeId,
        plan: &Plan<K>,
        body: NodeBody<K>,
        items: Items<K::Type>,
    ) -> Swapped<K> {
        let mut swapped = Swapped::<K>::default();
        for (side, map, arity) in [
            (Side::Input, &plan.inputs, plan.before.inputs.len()),
            (Side::Output, &plan.outputs, plan.before.outputs.len()),
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
            slot.body = body;
            slot.items = items;
        }
        let (severed, discarded) = self.remap_ports(tree, node, &moved);
        swapped.severed = severed;
        swapped.discarded = discarded;
        swapped
            .severed
            .extend(self.cut_what_no_longer_crosses(tree, node));
        swapped.severed.sort_by_key(|link| link.id);
        // ★★★★★ R2003 — the node's kind is what a zone pairing rests on, so a
        // swap is one of the two edits that can take that support away. Read
        // after the new body is in, and through the same derivation
        // `remove_node` uses, so the rule lives in one place.
        swapped.unpaired = self.reap_zones(tree, node);
        swapped
    }

    /// ★★★★★ R1937 — **a wire that was carried across may still be illegal at
    /// its OTHER end**, and this is what removes it and names it.
    ///
    /// A correspondence answers *did this port survive*, and a port can survive
    /// under the same name while carrying a different type — which is exactly
    /// what [`set_port_type`](Self::set_port_type) does. The wire is then moved
    /// onto a port whose type its far end never agreed to, and nothing had
    /// looked at the far end.
    ///
    /// ⚠ Found by R1937's own proof and it is a PRE-EXISTING defect of
    /// [`set_kind`](Self::set_kind) (R1598), reachable there the same way: any
    /// swap where a port keeps its name and changes its type. The reference has
    /// the corresponding line — its link transfer ends
    /// `if not new_link.is_valid: tree.links.remove(new_link)` — and this crate
    /// did not, so it was measurably behind on the one thing it claims to do
    /// better. It reports what it removes, which the reference still does not.
    fn cut_what_no_longer_crosses(&mut self, tree: TreeId, node: NodeId) -> Vec<Link> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let touching: Vec<crate::model::LinkId> = host
            .links()
            .iter()
            .filter(|link| link.from.node == node || link.to.node == node)
            .map(|link| link.id)
            .collect();
        let mut cut = Vec::new();
        for id in touching {
            let refused = self
                .tree(tree)
                .and_then(|host| host.link(id).copied())
                .is_some_and(|link| {
                    let source = self.port(tree, link.from, crate::group::PortSide::Out);
                    let sink = self.port(tree, link.to, crate::group::PortSide::In);
                    match (source, sink) {
                        (Some(from), Some(to)) => crossing::<K>(&from, &to).is_refused(),
                        // A socket that no longer resolves is a dangling link,
                        // which `remap_ports` already answered for; leave it.
                        _ => false,
                    }
                });
            if refused && let Ok(link) = self.disconnect(tree, id) {
                cut.push(link);
            }
        }
        cut
    }

    /// ★★★★★ R1936 — **make this node stand for that definition**, keeping the
    /// wires it can.
    ///
    /// The DCC's two group swaps in one verb, because measured they are one
    /// capability with the definition arriving from two places: `swap_group_asset`
    /// takes an existing group, and `swap_empty_group` makes a fresh empty one
    /// first and then does exactly this — see
    /// [`set_new_definition`](Self::set_new_definition), which is that second
    /// spelling.
    ///
    /// # It is one verb for two edits a reader would call different
    ///
    /// A node that is not yet a group instance BECOMES one; a node that already
    /// is one is RE-POINTED at another definition. The census sentence named
    /// only the second — *no verb changes which definition an instance stands
    /// for* — and reading the reference's operator showed the first: it accepts
    /// any swappable node, not only a group. They are one verb here because the
    /// edit is identical: the signature changes, and everything that can be
    /// carried across is.
    ///
    /// ★ And re-pointing is **not** ungroup-then-nest, which is why it needs a
    /// verb at all: that pair destroys the instance and makes another, so the
    /// [`NodeId`] dies and with it every selection, saved layout, held
    /// reference and undo record keyed by it. Here the node keeps its identity
    /// and only what it stands for changes — the same argument R1598 made for
    /// [`set_kind`](Self::set_kind).
    ///
    /// # Errors
    ///
    /// [`SwapError::NotSwappable`] for a body this crate owns and an
    /// application may not overwrite — a frame, an interface end, a register, a
    /// bend or either half of a name. [`SwapError::NotADefinition`] for the
    /// root tree, which is the document rather than a definition, and
    /// [`SwapError::Recursion`] when the node would make a definition contain
    /// itself — the same guard [`instantiate`](Self::instantiate) applies,
    /// asked here rather than re-derived.
    pub fn set_definition(
        &mut self,
        tree: TreeId,
        node: NodeId,
        definition: TreeId,
    ) -> Result<Swapped<K>, SwapError> {
        let Some(before) = self.signature(tree, node) else {
            return Err(if self.tree(tree).is_none() {
                SwapError::NoSuchTree(tree)
            } else {
                SwapError::NoSuchNode { tree, node }
            });
        };
        let held = self
            .tree(tree)
            .and_then(|t| t.node(node))
            .ok_or(SwapError::NoSuchNode { tree, node })?;
        if !matches!(held.body, NodeBody::Kind(_) | NodeBody::Group(_)) {
            return Err(SwapError::NotSwappable { tree, node });
        }
        if definition == ROOT {
            return Err(SwapError::NotADefinition(definition));
        }
        let Some(inner) = self.tree(definition) else {
            return Err(SwapError::NoSuchDefinition(definition));
        };
        let face = inner.interface();
        let after: Signature<K> = Signature {
            inputs: face.inputs().to_vec(),
            outputs: face.outputs().to_vec(),
        };
        // The same guard `nest` applies, asked rather than re-derived: a node
        // standing for a definition that (transitively) contains this tree
        // would make the tree contain itself.
        if let Some(chain) = crate::group::nesting_cycle(self, tree, definition) {
            return Err(SwapError::Recursion { chain });
        }

        let crosses = |from: &KindPort<K>, to: &KindPort<K>| crossing::<K>(from, to).is_allowed();
        let inputs = Correspondence::build(&before.inputs, &after.inputs, &crosses);
        let outputs = Correspondence::build(&before.outputs, &after.outputs, &crosses);
        let plan = Plan {
            before,
            inputs,
            outputs,
        };
        Ok(self.respecify(
            tree,
            node,
            &plan,
            NodeBody::Group(definition),
            Items::default(),
        ))
    }

    /// ★★★★★ R1937 — **give one port a type**, and let the kind say what it
    /// becomes.
    ///
    /// The engine's pair: an editor command a person reaches on one pin, and
    /// the node hook that command reaches through. Measured, they are not two
    /// spellings — one is the VERB and the other is the node's chance to react,
    /// and the reaction there is to store the type and reconstruct.
    ///
    /// ★ The refusal happens BEFORE anything moves, which the reference cannot
    /// do: its hook is a past-tense notification, so by the time a node hears
    /// about the change the change has happened. Here
    /// [`NodeKind::retyped`] answering `None` is the node declining, and it is
    /// also the answer a screen needs before it offers a chooser — one
    /// declaration, two questions.
    ///
    /// What it costs is reported, because this IS
    /// [`set_kind`](Self::set_kind): the kind the hook answers is swapped in
    /// through the same correspondence, so wires and authored values are
    /// carried where they can be and NAMED where they cannot.
    ///
    /// # Errors
    ///
    /// [`RetypeError::NoSuchPort`] when the node has no such port,
    /// [`RetypeError::NotAKind`] for a body this crate owns, and
    /// [`RetypeError::Refused`] when the kind declines — which is the default,
    /// and therefore the answer for every kind that has not opted in.
    pub fn set_port_type(
        &mut self,
        tree: TreeId,
        node: NodeId,
        port: PortRef,
        // By reference, which clippy asked for and which is also the right
        // signature: the hook only READS the type — what it answers is a KIND —
        // so taking ownership would make every caller clone a type it still
        // holds. It is also what makes this verb and
        // [`may_set_port_type`](Self::may_set_port_type) take the same argument,
        // which is what lets a caller ask and then do with one value.
        ty: &K::Type,
    ) -> Result<Swapped<K>, RetypeError> {
        let signature = self
            .signature(tree, node)
            .ok_or(RetypeError::NoSuchNode { tree, node })?;
        let arity = match port.side {
            Side::Input => signature.inputs.len(),
            Side::Output => signature.outputs.len(),
        };
        if port.index as usize >= arity {
            return Err(RetypeError::NoSuchPort { tree, node, port });
        }
        let held = self
            .tree(tree)
            .and_then(|t| t.node(node))
            .ok_or(RetypeError::NoSuchNode { tree, node })?;
        let NodeBody::Kind(kind) = &held.body else {
            return Err(RetypeError::NotAKind { tree, node });
        };
        let became = kind
            .retyped(port, ty)
            .ok_or(RetypeError::Refused { tree, node, port })?;
        self.set_kind(tree, node, became)
            .map_err(|_| RetypeError::NoSuchNode { tree, node })
    }

    /// ★★★★★ R1938 — **the container shapes this type may be held in, and what
    /// each one produces.**
    ///
    /// The list a type chooser offers beside a type, ascending by shape. Empty
    /// when the taxonomy declares none, which is the default and therefore the
    /// answer for every taxonomy that has not opted in.
    ///
    /// ★ The reference's equivalent answers a `bool` per (type, shape) and its
    /// default is `true`, so there the menu is filtered by a predicate that —
    /// measured across its one overrider — never actually refuses. Answering
    /// the TYPE means the permission and the result are one value, so a chooser
    /// cannot offer a shape it then has to look up somewhere else.
    #[must_use]
    pub fn containers_of(&self, ty: &K::Type) -> Vec<(Container, K::Type)> {
        Container::ALL
            .into_iter()
            .filter_map(|held| K::contained(ty, held).map(|made| (held, made)))
            .collect()
    }

    /// ★★★★★ R1937 — **may a person choose this port's type?**, asked without
    /// making the edit.
    ///
    /// The same declaration [`set_port_type`](Self::set_port_type) obeys, so a
    /// screen that offers a chooser and an edit that runs cannot disagree —
    /// R1920's rule, applied to a hook rather than to a permission.
    ///
    /// ⚠ It takes the type it would be given, because a kind may accept one
    /// type on a port and refuse another: *choosable* is not a property of the
    /// port alone. The reference cannot ask this at all — its hook is a
    /// notification, so the only way to find out is to do it.
    #[must_use]
    pub fn may_set_port_type(
        &self,
        tree: TreeId,
        node: NodeId,
        port: PortRef,
        ty: &K::Type,
    ) -> bool {
        let Some(signature) = self.signature(tree, node) else {
            return false;
        };
        let arity = match port.side {
            Side::Input => signature.inputs.len(),
            Side::Output => signature.outputs.len(),
        };
        if port.index as usize >= arity {
            return false;
        }
        self.tree(tree)
            .and_then(|t| t.node(node))
            .is_some_and(|held| match &held.body {
                NodeBody::Kind(kind) => kind.retyped(port, ty).is_some(),
                _ => false,
            })
    }

    /// ★★★★★ R1936 — **make this node stand for a NEW, empty definition**, and
    /// answer which one.
    ///
    /// The reference's `swap_empty_group`, and measured it is exactly this
    /// composition: it builds an empty group with an input end and an output
    /// end, calls its own node swap, and then points the result at the group it
    /// made. Written as a composition here too, so the two cannot disagree
    /// about what a swap is — there they can, because the operator reaches back
    /// in and overwrites the swapped node's tree afterwards.
    ///
    /// ⚠ The new definition's interface is EMPTY, so nothing on the node
    /// survives: every port is dropped and every wire on it is severed. That is
    /// the honest outcome and it is reported rather than hidden — which is the
    /// whole difference from the reference, where the same swap drops the wires
    /// inside three swallowed exceptions. A caller that wants to keep them
    /// should build the definition first and use
    /// [`set_definition`](Self::set_definition).
    ///
    /// # Errors
    ///
    /// As [`set_definition`](Self::set_definition), minus the two that cannot
    /// happen: a definition this call just made is neither the root nor able to
    /// contain anything.
    pub fn set_new_definition(
        &mut self,
        tree: TreeId,
        node: NodeId,
        name: impl Into<String>,
    ) -> Result<(TreeId, Swapped<K>), SwapError> {
        // Refuse BEFORE making the definition, or a refused swap leaves an
        // orphan definition behind that nothing points at and nobody asked for.
        let held = self
            .tree(tree)
            .ok_or(SwapError::NoSuchTree(tree))?
            .node(node)
            .ok_or(SwapError::NoSuchNode { tree, node })?;
        if !matches!(held.body, NodeBody::Kind(_) | NodeBody::Group(_)) {
            return Err(SwapError::NotSwappable { tree, node });
        }
        let definition = self.add_definition(name);
        let swapped = self.set_definition(tree, node, definition)?;
        Ok((definition, swapped))
    }

    /// ★★★★★ R2003 — **change what KIND of zone this is, keeping which nodes it
    /// is made of.**
    ///
    /// The DCC's `swap_zone`, and it is one verb with two arms because the
    /// reference's operator is: a node already in a zone has the whole zone
    /// re-kinded, and an ordinary node BECOMES one. Addressed from either end,
    /// the same argument [`unpair`](Self::unpair) makes — a person clicking a
    /// node has whichever end they clicked, and making them find the opener
    /// first would be this crate's storage decision leaking into its surface.
    ///
    /// `beside` is where a made closer goes relative to the node that became
    /// the opener, and it is a parameter for the reason
    /// [`room_for`](Self::room_for)'s margin is: how far apart a screen likes
    /// its cards is the application's, and the reference agrees — its own
    /// offset is a settable property of the operator. It is unused when the
    /// zone already has a closer, because then nothing is placed.
    ///
    /// # Why this is a verb and not two calls to [`set_kind`](Self::set_kind)
    ///
    /// Because those two calls pass through a state neither of them can name.
    /// A zone rests on the two ends' kinds declaring each other, so re-kinding
    /// one end takes the pairing's support away — [`Swapped::unpaired`] is that
    /// measurement — and re-kinding the other cannot put it back. Doing it here
    /// means the pairing is taken apart and re-made deliberately, so the report
    /// says *the zone was kept* rather than *the zone was lost, then a new one
    /// appeared*.
    ///
    /// # What the reference does, measured at its operator this round
    ///
    /// * It **destroys both ends and builds two new nodes**, so every id dies
    ///   and with it every selection, saved layout, held reference and undo
    ///   record keyed by one. Here both ends keep their [`NodeId`] — R1598's
    ///   argument, met on the zone axis.
    /// * It takes the two ends' kinds as **two independent arguments**, so a
    ///   caller can ask for a zone whose opener never declared that closer.
    ///   Here the closer is [`NodeKind::closed_by`]'s answer, so the pair cannot
    ///   disagree: the wrong request is not refused, it is unsayable.
    /// * Its item transfer **swallows what will not cross**: a loop over the
    ///   old zone's items matching by NAME only, with `except RuntimeError:
    ///   pass` around the one call that can fail, and a silent early return
    ///   when either side has no item list at all. Here every end's ports go
    ///   through the same correspondence a [`set_kind`](Self::set_kind) uses —
    ///   by name, then by position — and what does not survive is named in that
    ///   end's own [`Swapped`].
    /// * Its pair lookup answers *(the node, whatever it is paired with)* for a
    ///   node whose pairing field exists, and that field holds nothing for an
    ///   opener whose closer has been deleted — a state this crate names
    ///   ([`InZone::OpensNothingYet`]) and that one has no branch for.
    ///
    /// # Errors
    ///
    /// [`ZoneSwapError`] — the tree or node is not there, the node's body is
    /// not a kind, the kind asked for opens no zone, or the taxonomy refused to
    /// place the closer this edit had to make.
    ///
    /// [`InZone::OpensNothingYet`]: crate::InZone::OpensNothingYet
    pub fn set_zone_kind(
        &mut self,
        tree: TreeId,
        node: NodeId,
        opener: K,
        beside: (i32, i32),
    ) -> Result<ZoneSwapped<K>, ZoneSwapError> {
        let closer_kind = opener.closed_by().ok_or(ZoneSwapError::KindOpensNothing)?;
        let held = self
            .tree(tree)
            .ok_or(ZoneSwapError::NoSuchTree(tree))?
            .node(node)
            .ok_or(ZoneSwapError::NoSuchNode { tree, node })?;
        if !matches!(held.body, NodeBody::Kind(_)) {
            return Err(ZoneSwapError::NotAKind { tree, node });
        }
        let (x, y) = (held.x, held.y);

        let was = self.in_zone(tree, node);
        let opens = match was {
            Some(crate::InZone::Closes(opener)) => opener,
            _ => node,
        };
        let standing = match was {
            Some(crate::InZone::Opens(closer)) => Some(closer),
            Some(crate::InZone::Closes(_)) => Some(node),
            _ => None,
        };

        // ⚠ There is deliberately no separate permission pre-check here. The
        // one thing that can refuse is the taxonomy declining to place the
        // closer, and `add_node` asks that itself and refuses BEFORE it
        // mutates anything — so a guard above would be a second copy of a rule
        // with no path to firing, which is what this project deletes rather
        // than writes. Every other arm below touches only nodes this function
        // has already resolved.
        match (standing, was) {
            // The zone stands: both ends are re-kinded and the pairing is
            // deliberately taken apart first, so `respecify`'s reaping finds
            // nothing to report and this edit owns the story.
            (Some(closes), _) => {
                self.unpair(tree, opens);
                let cost_to_opener = self.swap_end(tree, opens, opener)?;
                let cost_to_closer = self.swap_end(tree, closes, closer_kind)?;
                self.repair(tree, opens, closes);
                Ok(ZoneSwapped {
                    was,
                    opens,
                    closes: Some(closes),
                    opened: cost_to_opener,
                    closed: Some(cost_to_closer),
                    handed: Vec::new(),
                })
            }
            // An opener with nothing closing it yet stays exactly that. Making
            // a closer here would be inventing a node to answer a request that
            // did not ask for one — the node is already an opener, and what it
            // opens is what changed.
            (None, Some(_)) => {
                let cost_to_opener = self.swap_end(tree, opens, opener)?;
                Ok(ZoneSwapped {
                    was,
                    opens,
                    closes: None,
                    opened: cost_to_opener,
                    closed: None,
                    handed: Vec::new(),
                })
            }
            // An ordinary node BECOMES a zone, and its flow is split across the
            // two ends: what fed it feeds the opener, what it fed is fed by the
            // closer. That is the reference's own answer, and it is the reason
            // this arm cannot be left to the caller — the handover has to
            // happen while the node still has the ports the wires are on.
            (None, None) => {
                let before = self
                    .signature(tree, node)
                    .ok_or(ZoneSwapError::NoSuchNode { tree, node })?;
                let closes = self
                    .add_node(
                        tree,
                        NodeBody::Kind(closer_kind),
                        x.saturating_add(beside.0),
                        y.saturating_add(beside.1),
                    )
                    .map_err(|why| ZoneSwapError::CloserRefused { tree, why })?;
                let handed = self.hand_outputs_over(tree, node, closes, &before);
                let cost_to_opener = self.swap_end(tree, node, opener)?;
                self.repair(tree, node, closes);
                Ok(ZoneSwapped {
                    was,
                    opens: node,
                    closes: Some(closes),
                    opened: cost_to_opener,
                    closed: None,
                    handed,
                })
            }
        }
    }

    /// One end of a zone re-kinded, with the caller's error vocabulary.
    ///
    /// A thin call site of [`set_kind`](Self::set_kind) rather than a second
    /// path through `respecify`: what survives a swap is decided in one place,
    /// and a zone end is not a special kind of node.
    ///
    /// ⚠ The conversion is what the types ask for, not a state this verb can
    /// reach: every node handed here has already been resolved and confirmed to
    /// carry a kind, and the ends of a standing pairing are guaranteed both by
    /// [`standing_zones`](Self::standing_zones).
    fn swap_end(
        &mut self,
        tree: TreeId,
        node: NodeId,
        kind: K,
    ) -> Result<Swapped<K>, ZoneSwapError> {
        self.set_kind(tree, node, kind)
            .map_err(|_| ZoneSwapError::NoSuchNode { tree, node })
    }

    /// Put the pairing back, when this edit has just established every
    /// precondition [`pair`](Self::pair) checks.
    ///
    /// Every arm of [`PairError`](crate::PairError) is excluded here by
    /// construction — the two ends are distinct nodes of this tree, both bodies
    /// are kinds, the opener's kind is one whose `closed_by` answered, the
    /// closer's kind IS that answer, and both were unpaired a moment ago — so
    /// the result carries no information and is dropped rather than checked.
    /// An assertion with no path to failure is one this crate deletes rather
    /// than writes (R1964).
    fn repair(&mut self, tree: TreeId, opens: NodeId, closes: NodeId) {
        drop(self.pair(tree, opens, closes));
    }

    /// The outgoing wires of a node that is becoming a zone opener, moved onto
    /// the closer that was made for it.
    ///
    /// Built on the same [`Correspondence`] a swap uses, so *which output
    /// answers which* is one rule rather than two, and moved with
    /// [`move_links`](Self::move_links) so each wire keeps its
    /// [`LinkId`](crate::LinkId) and its mute.
    ///
    /// ★ A wire the closer's far end will not take stays where it is, and the
    /// swap that follows severs it and NAMES it in the opener's own
    /// [`Swapped::severed`]. So nothing is lost quietly even here, which is the
    /// one place the reference loses wires without a report.
    fn hand_outputs_over(
        &mut self,
        tree: TreeId,
        node: NodeId,
        closes: NodeId,
        before: &Signature<K>,
    ) -> Vec<Carried> {
        let Some(after) = self.signature(tree, closes) else {
            return Vec::new();
        };
        let crosses = |from: &KindPort<K>, to: &KindPort<K>| crossing::<K>(from, to).is_allowed();
        let map = Correspondence::build(&before.outputs, &after.outputs, &crosses);
        let mut handed: Vec<Carried> = Vec::new();
        for (index, to) in &map.taken {
            handed.push(Carried {
                from: PortRef {
                    side: Side::Output,
                    index: *index,
                },
                to: PortRef {
                    side: Side::Output,
                    index: *to,
                },
                by_name: map.by_name.contains(index),
            });
            drop(self.move_links(
                tree,
                Side::Output,
                Socket::new(node, *index),
                Socket::new(closes, *to),
            ));
        }
        handed
    }
}
