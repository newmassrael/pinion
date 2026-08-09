//! R1584 — a group's boundary is a partition, and the partition can move.
//!
//! [`Document::group`](crate::Document::group) *creates* a partition: these
//! nodes are inside, the rest are outside, and the interface is derived from
//! what crosses. [`Document::ungroup`](crate::Document::ungroup) destroys one.
//! Neither can move one, and moving one is what an editor does all day — this
//! node belongs in the group after all, that one does not.
//!
//! Both directions are one operation seen from two sides. A pair
//! `(host tree, instance node)` names one boundary, a selection names the nodes
//! changing sides, and the interface is re-derived from the partition that
//! results. [`Document::group_insert`] moves nodes inward,
//! [`Document::group_separate`] outward.
//!
//! # Re-deriving is the whole job
//!
//! A node that changes sides takes its wiring with it, and wiring that crosses
//! the boundary can only be expressed as an interface port. So a move
//!
//! * **appends** a port for each value that crosses now and did not before,
//! * **re-uses** a port for a value that already crosses at this instance — one
//!   value is one port, the rule [`pinion_graph::group`] states for a collapse,
//!   which does not stop applying afterwards,
//! * **removes** a port the move leaves unwired on its inside end, and
//! * **reconnects** every value whose crossing disappeared, so the graph goes
//!   on computing what it computed.
//!
//! That last one is what the DCC does not do. Measured against the DCC reference tree at `8cf50599`: `node_group_separate_selected`
//! copies the selected nodes into the parent tree and, for the Move arm,
//! deletes them from the group. It does not touch the interface, so the group
//! keeps sockets that reach nothing, and the separated nodes arrive wired only
//! to each other — the value that used to flow through them is gone. This
//! crate's tests hold that rule as a helper and assert the divergence rather
//! than describing it.
//!
//! # A round trip keeps the meaning and not the order
//!
//! Move a node in and back out and the graph computes what it computed, every
//! value crossing where it crossed — but the interface's port ORDER is not
//! restored: a port that stops describing a crossing is removed, and one that
//! starts is **appended**.
//!
//! That is forced rather than sloppy. Ports are addressed by index, and other
//! instances of the definition are wired by that index, so putting a returning
//! port back at its old position would silently rewire every one of them.
//! Appending is the only placement that cannot reach through the boundary it
//! is being moved across. A caller that needs a particular order has
//! [`Document::expose`](crate::Document::expose) and
//! [`Document::unexpose`](crate::Document::unexpose), which state the cost of
//! a removal in dropped links.
//!
//! # A definition is shared; an edit through one instance is not
//!
//! The nodes move into (or out of) a *definition*, and a definition can have
//! many instances, so editing it through one changes all of them. The DCC does
//! that silently: `node_group_insert_exec` appends to the group its active node references, and
//! every other user of that group gains the sockets and the behaviour without
//! being told.
//!
//! Here the caller says which they meant — [`Sharing`] — and is told what it
//! cost either way: [`Repartitioned::other_instances`] counts the instances that
//! came along and [`Repartitioned::severed`] names every link that died
//! anywhere in the document, each with the tree it was in.
//!
//! # Separate's other arm
//!
//! The DCC's Separate offers Copy as well as Move. Copy is
//! [`Document::extract`](crate::Document::extract) followed by [`Document::insert`](crate::Document::insert)
//! — R1578 — which copies the nodes, leaves the group untouched and *names*
//! the boundary it cut. There is one spelling of each thing here, so [`Document::group_separate`] is
//! the move.

use pinion_graph::group as boundary;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::frame::{Orphaned, parents_of};
use crate::group::{INTERFACE_GAP, PortSide};
use crate::model::{
    Document, DroppedLink, InterfaceSide, KindPort, LinkId, NodeBody, NodeId, NodeKind, Sink,
    Socket, Tree, TreeId,
};
use crate::numbering::Numbering;

/// Whose definition an edit made through one instance changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sharing {
    /// Edit the definition itself, so every instance of it changes at once.
    /// That is what a definition being shared *means*, and it is the only
    /// thing the DCC's Group Insert and Separate can do.
    Shared,
    /// Copy the definition first and point this instance at the copy, leaving
    /// the other instances with what they had.
    ///
    /// The DCC reaches the same place in two steps and a different vocabulary:
    /// a node group is an ID datablock, so you make it single-user first.
    Fork,
}

/// One interface port a boundary move added or removed.
#[derive(Debug, Clone, PartialEq)]
pub struct PortChange<K: NodeKind> {
    /// Which half of the interface.
    pub side: InterfaceSide,
    /// Where the port sits in the interface **this change is stated against**:
    /// an added port is indexed in the interface as it now is, a removed one in
    /// the interface as it was. No single numbering could hold both, because
    /// removing a port is what moves the others.
    pub index: u32,
    /// The port itself, so a removal is undoable without having kept a copy.
    pub port: KindPort<K>,
}

/// What moving a boundary did.
#[derive(Debug, Clone, PartialEq)]
pub struct Repartitioned<K: NodeKind> {
    /// The definition that now holds the changed content. Not the one the
    /// instance named before, when [`Sharing::Fork`] copied it.
    pub definition: TreeId,
    /// The definition the instance named before a fork replaced it.
    pub forked_from: Option<TreeId>,
    /// The nodes that changed sides, named in the tree they landed in, in
    /// ascending order of the ids they left behind.
    pub moved: Vec<NodeId>,
    /// Ports appended to the interface.
    pub exposed: Vec<PortChange<K>>,
    /// Ports removed from it, because the move left their inside end unwired.
    pub unexposed: Vec<PortChange<K>>,
    /// Every link the move removed and did not replace, anywhere in the
    /// document. The ones outside the tree being edited are the price of
    /// editing a shared definition.
    pub severed: Vec<DroppedLink>,
    /// How many *other* instances of [`Self::definition`] this changed. Always
    /// zero after a fork, by construction.
    pub other_instances: usize,
    /// Moved nodes whose frame could not cross with them, and the frame each was
    /// in, named in the tree it stayed in (R1589).
    pub orphaned: Vec<Orphaned>,
}

/// Why a boundary could not be moved.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RepartitionError {
    /// No such tree.
    NoSuchTree(TreeId),
    /// No such node in that tree.
    NoSuchNode {
        /// The tree that was searched.
        tree: TreeId,
        /// The node that is not in it.
        node: NodeId,
    },
    /// The node named as the boundary is not a group instance, so there is no
    /// boundary to move.
    NotAGroup(NodeId),
    /// Nothing was selected. A move of no nodes is not an empty move, it is an
    /// absent one.
    Empty,
    /// A selected node materialises a tree's own interface. It is a projection
    /// of that tree rather than content, so it cannot change sides.
    InterfaceNodeSelected(NodeId),
    /// The instance itself was selected for a move into its own definition.
    InstanceSelected(NodeId),
    /// A selected group instance would put a definition inside itself, along
    /// this chain.
    Recursion {
        /// The containment chain the move would close.
        chain: Vec<TreeId>,
    },
    /// The move would make the instance reach itself, along this path.
    ///
    /// Inward, a walk leaves the nodes going in and comes back to them.
    /// Outward, a walk leaves the nodes staying behind and comes back to them —
    /// the same statement about the other side of the same partition.
    Bypass {
        /// The walk, in the ids of the tree it runs through.
        path: Vec<NodeId>,
    },
    /// A link names a node its tree does not hold, so there is no graph to
    /// derive a boundary from. [`Document::validate`] reports these.
    Malformed {
        /// The tree holding it.
        tree: TreeId,
        /// The link.
        link: LinkId,
    },
    /// The boundary derivation refused for a reason this crate has no arm for.
    ///
    /// [`boundary::Refusal`] is `#[non_exhaustive]`; carrying the sentence is
    /// the only answer that stays true when it grows an arm.
    Boundary(String),
}

impl fmt::Display for RepartitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode { tree, node } => {
                write!(f, "tree {} has no node {}", tree.0, node.0)
            }
            Self::NotAGroup(node) => write!(f, "node {} is not a group instance", node.0),
            Self::Empty => f.write_str("nothing is selected"),
            Self::InterfaceNodeSelected(node) => write!(
                f,
                "node {} materialises its tree's interface and cannot change sides",
                node.0
            ),
            Self::InstanceSelected(node) => write!(
                f,
                "node {} is the group itself, which cannot be moved into itself",
                node.0
            ),
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
            Self::Bypass { path } => {
                f.write_str("that move would create a cycle: ")?;
                for (i, node) in path.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" -> ")?;
                    }
                    write!(f, "{}", node.0)?;
                }
                Ok(())
            }
            Self::Malformed { tree, link } => write!(
                f,
                "tree {} holds link {}, which names a node it does not have",
                tree.0, link.0
            ),
            Self::Boundary(reason) => write!(f, "the boundary cannot be moved: {reason}"),
        }
    }
}

impl std::error::Error for RepartitionError {}

impl<K: NodeKind> Document<K> {
    /// Copy the definition an instance names, and point that instance at the
    /// copy.
    ///
    /// The DCC's "make single user", reached here in one call because the
    /// thing copied is a tree rather than an ID datablock. The copy keeps the
    /// original's name — a name is not an identity in this crate, where the
    /// DCC must rename because an ID's name *is* its key — and the copy's own
    /// group instances still name the same inner definitions, which is what
    /// makes this a fork of one level rather than of a whole subtree.
    ///
    /// # Errors
    ///
    /// See [`RepartitionError`]; only the three "no such" arms are reachable.
    pub fn fork_definition(
        &mut self,
        tree: TreeId,
        instance: NodeId,
    ) -> Result<TreeId, RepartitionError> {
        let definition = self.definition_of(tree, instance)?;
        let copy = self
            .copy_tree(definition)
            .ok_or(RepartitionError::NoSuchTree(definition))?;
        if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(instance)) {
            slot.body = NodeBody::Group(copy);
        }
        Ok(copy)
    }

    /// Move `selection` out of `tree` and into the definition `instance` names.
    ///
    /// The DCC's `group_insert`, with the interface re-derived rather than
    /// only appended to: a value that already crosses at this instance keeps
    /// its port instead of gaining a second, and a port whose feed ends up
    /// inside is removed rather than left describing nothing.
    ///
    /// # Errors
    ///
    /// See [`RepartitionError`]. A refused move leaves the document
    /// **untouched**, the fork included: every check runs before the first
    /// mutation.
    pub fn group_insert(
        &mut self,
        tree: TreeId,
        instance: NodeId,
        selection: &[NodeId],
        sharing: Sharing,
    ) -> Result<Repartitioned<K>, RepartitionError> {
        let definition = self.definition_of(tree, instance)?;
        let plan = self.plan_inward(tree, instance, definition, selection)?;
        let (definition, forked_from) = self.apply_sharing(tree, instance, definition, sharing)?;
        Ok(self.perform_inward(tree, instance, definition, forked_from, &plan))
    }

    /// Move `selection` out of the definition `instance` names and into `tree`.
    ///
    /// The DCC's `group_separate` with `type='MOVE'`, except that the values
    /// which used to cross the boundary are **reconnected**: a moved node fed
    /// from the group's input is fed by whatever feeds that input here, and a
    /// moved node that fed the group's output now feeds whatever that output
    /// fed. The graph goes on computing what it computed.
    ///
    /// `selection` names nodes in the *definition*, not in `tree`.
    ///
    /// # Errors
    ///
    /// See [`RepartitionError`]. A refused move leaves the document untouched.
    pub fn group_separate(
        &mut self,
        tree: TreeId,
        instance: NodeId,
        selection: &[NodeId],
        sharing: Sharing,
    ) -> Result<Repartitioned<K>, RepartitionError> {
        let definition = self.definition_of(tree, instance)?;
        let plan = self.plan_outward(tree, instance, definition, selection)?;
        let (definition, forked_from) = self.apply_sharing(tree, instance, definition, sharing)?;
        Ok(self.perform_outward(tree, instance, definition, forked_from, &plan))
    }

    /// The definition an instance node names.
    fn definition_of(&self, tree: TreeId, instance: NodeId) -> Result<TreeId, RepartitionError> {
        let host = self.tree(tree).ok_or(RepartitionError::NoSuchTree(tree))?;
        let node = host.node(instance).ok_or(RepartitionError::NoSuchNode {
            tree,
            node: instance,
        })?;
        match node.body {
            NodeBody::Group(definition) => Ok(definition),
            _ => Err(RepartitionError::NotAGroup(instance)),
        }
    }

    /// Resolve [`Sharing`] into the definition the move will actually edit.
    fn apply_sharing(
        &mut self,
        tree: TreeId,
        instance: NodeId,
        definition: TreeId,
        sharing: Sharing,
    ) -> Result<(TreeId, Option<TreeId>), RepartitionError> {
        match sharing {
            Sharing::Shared => Ok((definition, None)),
            Sharing::Fork => Ok((self.fork_definition(tree, instance)?, Some(definition))),
        }
    }

    /// The selection, sorted and deduplicated, with every node checked.
    fn chosen(&self, tree: TreeId, selection: &[NodeId]) -> Result<Vec<NodeId>, RepartitionError> {
        let host = self.tree(tree).ok_or(RepartitionError::NoSuchTree(tree))?;
        if selection.is_empty() {
            return Err(RepartitionError::Empty);
        }
        let mut chosen = selection.to_vec();
        chosen.sort_unstable();
        chosen.dedup();
        for &id in &chosen {
            let node = host
                .node(id)
                .ok_or(RepartitionError::NoSuchNode { tree, node: id })?;
            if matches!(node.body, NodeBody::Interface(_)) {
                return Err(RepartitionError::InterfaceNodeSelected(id));
            }
        }
        Ok(chosen)
    }

    /// The node materialising one side of a definition's interface, created if
    /// the definition has none.
    ///
    /// A definition assembled by hand — [`Document::add_definition`] plus
    /// [`Document::expose`] — has ports and no interface nodes, and a move that
    /// wires something to the inside of a port has to have one.
    fn interface_node_or_create(&mut self, definition: TreeId, side: InterfaceSide) -> NodeId {
        if let Some(existing) = self
            .tree(definition)
            .and_then(|t| t.interface_node(side))
            .map(|n| n.id)
        {
            return existing;
        }
        let (mut min_x, mut max_x, mut sum_y, mut count) = (0_i32, 0_i32, 0_i64, 0_i64);
        if let Some(inner) = self.tree(definition) {
            for node in inner.nodes() {
                min_x = min_x.min(node.x);
                max_x = max_x.max(node.x);
                sum_y += i64::from(node.y);
                count += 1;
            }
        }
        let mid_y = if count == 0 {
            0
        } else {
            i32::try_from(sum_y / count).unwrap_or(0)
        };
        let x = match side {
            InterfaceSide::Input => min_x - INTERFACE_GAP,
            InterfaceSide::Output => max_x + INTERFACE_GAP,
        };
        self.add_node(definition, NodeBody::Interface(side), x, mid_y)
            .unwrap_or(NodeId(0))
    }

    /// Apply an interface edit: the removals first, in descending index order so
    /// no surviving index moves under a later removal, then the additions.
    fn edit_interface(
        &mut self,
        definition: TreeId,
        dying: &Ports,
        adding: &[(InterfaceSide, KindPort<K>)],
    ) -> InterfaceEdit<K> {
        let mut edit = InterfaceEdit {
            dying: dying.clone(),
            ..InterfaceEdit::default()
        };
        for side in [InterfaceSide::Input, InterfaceSide::Output] {
            for &index in dying.on(side).iter().rev() {
                let Some(port) = self
                    .tree(definition)
                    .map(|t| t.interface().side(side))
                    .and_then(|ports| ports.get(index as usize))
                    .cloned()
                else {
                    continue;
                };
                if let Ok(lost) = self.unexpose(definition, side, index) {
                    edit.killed.extend(lost);
                }
                edit.unexposed.push(PortChange { side, index, port });
            }
        }
        for (side, port) in adding {
            let index = self
                .expose(definition, *side, port.clone())
                .unwrap_or_default();
            edit.exposed.push(PortChange {
                side: *side,
                index,
                port: port.clone(),
            });
        }
        edit
    }

    /// Move nodes from one tree to another, keeping **everything about them
    /// that is not their identity or their place**, and answer the old-to-new id
    /// map together with the containers that could not come along.
    ///
    /// Ids are per tree, so a node crossing trees is renumbered. Positions are
    /// offset by the instance's own, which is what makes an insert and an
    /// [`Document::ungroup`](crate::Document::ungroup) land in the same place.
    ///
    /// `roots` is what a node whose frame stayed behind becomes a member of —
    /// the instance's own container going outward, and nothing going inward,
    /// because a host-tree frame is not in the definition at all.
    ///
    /// R1589 found this copying the **label alone**, where R1586 had introduced
    /// [`Node::adopt_from`](crate::Node) precisely so a field added to a node
    /// could not be silently dropped by a hand-rolled copy — and then swept two
    /// of the three sites. So a node moved across a group boundary arrived
    /// un-bypassed, un-collapsed and full-width, which is the defect that
    /// mechanism exists to prevent, in the mechanism's own crate. The third site
    /// now routes through it as well.
    fn move_nodes(
        &mut self,
        from: TreeId,
        to: TreeId,
        nodes: &[NodeId],
        offset: (i32, i32),
        roots: Option<NodeId>,
    ) -> (BTreeMap<NodeId, NodeId>, Vec<Orphaned>) {
        let moving: BTreeSet<NodeId> = nodes.iter().copied().collect();
        let forest = parents_of(
            self.tree(from)
                .into_iter()
                .flat_map(Tree::nodes)
                .filter(|node| moving.contains(&node.id)),
        );
        let mut mapping = BTreeMap::new();
        for &id in nodes {
            let Some(node) = self.take_node(from, id) else {
                continue;
            };
            let Ok(fresh) =
                self.add_node(to, node.body.clone(), node.x + offset.0, node.y + offset.1)
            else {
                continue;
            };
            if let Some(slot) = self.tree_mut(to).and_then(|t| t.node_mut(fresh)) {
                slot.adopt_from(&node);
            }
            mapping.insert(id, fresh);
        }
        let orphaned = self.remap_parents(to, &mapping, &forest, roots);
        (mapping, orphaned)
    }

    /// Take every link in `tree` with an endpoint in `selected`, answering the
    /// ones the plan says have nowhere left to go.
    fn take_incident_links(
        &mut self,
        tree: TreeId,
        selected: &BTreeSet<NodeId>,
        lost: &[LinkId],
    ) -> Vec<DroppedLink> {
        let doomed: Vec<LinkId> = self.tree(tree).map_or_else(Vec::new, |host| {
            host.links()
                .iter()
                .filter(|l| selected.contains(&l.from.node) || selected.contains(&l.to.node))
                .map(|l| l.id)
                .collect()
        });
        let mut severed = Vec::new();
        for id in doomed {
            let Some(link) = self.take_link(tree, id) else {
                continue;
            };
            if lost.contains(&id) {
                severed.push(DroppedLink { tree, link });
            }
        }
        severed
    }
}

/// A set of interface port indices per side, held sorted.
#[derive(Debug, Clone, Default)]
struct Ports {
    inputs: Vec<u32>,
    outputs: Vec<u32>,
}

impl Ports {
    fn insert(&mut self, side: InterfaceSide, index: u32) {
        let slot = match side {
            InterfaceSide::Input => &mut self.inputs,
            InterfaceSide::Output => &mut self.outputs,
        };
        if let Err(at) = slot.binary_search(&index) {
            slot.insert(at, index);
        }
    }

    fn on(&self, side: InterfaceSide) -> &[u32] {
        match side {
            InterfaceSide::Input => &self.inputs,
            InterfaceSide::Output => &self.outputs,
        }
    }
}

/// What [`Document::edit_interface`] did.
struct InterfaceEdit<K: NodeKind> {
    exposed: Vec<PortChange<K>>,
    unexposed: Vec<PortChange<K>>,
    killed: Vec<DroppedLink>,
    dying: Ports,
}

impl<K: NodeKind> Default for InterfaceEdit<K> {
    fn default() -> Self {
        Self {
            exposed: Vec::new(),
            unexposed: Vec::new(),
            killed: Vec::new(),
            dying: Ports::default(),
        }
    }
}

impl<K: NodeKind> InterfaceEdit<K> {
    /// Where a port that survived the removals sits now.
    fn survivor(&self, side: InterfaceSide, before: u32) -> u32 {
        let removed_below = self
            .dying
            .on(side)
            .iter()
            .filter(|&&index| index < before)
            .count();
        before - u32::try_from(removed_below).unwrap_or(0)
    }

    /// Where the `nth` port this edit appended on `side` sits.
    fn added(&self, side: InterfaceSide, nth: usize) -> u32 {
        self.exposed
            .iter()
            .filter(|change| change.side == side)
            .nth(nth)
            .map_or(0, |change| change.index)
    }
}

/// One value that has to cross the boundary after the move.
///
/// `outer` names it on the host's side of the instance, `inner` on the
/// definition's; each is read in the numbering of the tree it currently lives
/// in, and the ones that are moving are remapped when they land.
struct Face<K: NodeKind> {
    port: KindPort<K>,
    outer: Vec<Socket>,
    inner: Vec<Socket>,
    /// Those of this face's *consumers* whose crossing link was muted, in the
    /// identity they had before anything moved (R1586).
    ///
    /// A subset rather than a flag beside each socket, because which of `outer`
    /// and `inner` holds the consumers depends on the face's direction, and a
    /// field that meant two things by position is the shape this crate spends
    /// its refusals avoiding.
    muted: Vec<Socket>,
}

impl<K: NodeKind> Face<K> {
    /// Whether this face's crossing at `consumer` was muted before the move.
    fn was_muted(&self, consumer: Socket) -> bool {
        self.muted.contains(&consumer)
    }
}

/// The muted subset of a `(consumer, muted)` list, in the form [`Face`] holds.
fn muted_of(consumers: &[(Socket, bool)]) -> Vec<Socket> {
    consumers
        .iter()
        .filter(|&&(_, muted)| muted)
        .map(|&(socket, _)| socket)
        .collect()
}

/// An interface input whose inside consumers change hands: the feed moved in,
/// so the value is produced inside now.
struct Takeover {
    producer: Socket,
    consumers: Vec<Sink>,
}

/// Everything the inward move needs, computed before anything is mutated.
struct InwardPlan<K: NodeKind> {
    moving: Vec<NodeId>,
    entry: Option<NodeId>,
    /// Links between two moved nodes, to re-create inside.
    carried: Vec<(Socket, Sink)>,
    /// Extra inside consumers for a value that already crosses at a port.
    shared: Vec<(u32, Vec<Sink>)>,
    inbound: Vec<Face<K>>,
    outbound: Vec<Face<K>>,
    takeovers: Vec<Takeover>,
    /// An interface output consumed by a moved node: the producer inside feeds
    /// it directly, and the port stays, because an output may feed many.
    passthroughs: Vec<(Socket, Sink)>,
    dying: Ports,
    lost: Vec<LinkId>,
}

/// Everything the outward move needs, computed before anything is mutated.
struct OutwardPlan<K: NodeKind> {
    moving: Vec<NodeId>,
    /// Links between two moved nodes, to re-create in the host.
    carried: Vec<(Socket, Sink)>,
    /// An interface input feeding moved nodes: they are fed by whatever feeds
    /// the instance's port, when anything does.
    entry_feeds: Vec<(Option<Socket>, Vec<Sink>)>,
    /// An interface output fed by a moved node: it feeds whatever the
    /// instance's port fed.
    exit_takes: Vec<(Socket, Vec<Sink>)>,
    inbound: Vec<Face<K>>,
    outbound: Vec<Face<K>>,
    dying: Ports,
    lost: Vec<LinkId>,
}

impl<K: NodeKind> Document<K> {
    /// Everything the inward move needs, with every refusal decided here.
    ///
    /// Long because it is one derivation with four exhaustive cases over the
    /// links; splitting it would hand the pieces a partly-built plan.
    #[allow(clippy::too_many_lines)]
    fn plan_inward(
        &self,
        tree: TreeId,
        instance: NodeId,
        definition: TreeId,
        selection: &[NodeId],
    ) -> Result<InwardPlan<K>, RepartitionError> {
        let host = self.tree(tree).ok_or(RepartitionError::NoSuchTree(tree))?;
        let moving = self.chosen(tree, selection)?;
        let containment = self.containment();
        for &id in &moving {
            if id == instance {
                return Err(RepartitionError::InstanceSelected(id));
            }
            let Some(node) = host.node(id) else { continue };
            if let NodeBody::Group(inner) = node.body {
                if let Some(chain) =
                    boundary::Nesting::cycle(&containment, definition.0 as usize, inner.0 as usize)
                {
                    return Err(RepartitionError::Recursion {
                        chain: chain.into_iter().map(tree_id).collect(),
                    });
                }
            }
        }

        // The partition after the move is the nodes going in plus the instance
        // they are going into. Every check a collapse runs applies, because this
        // IS a collapse — of a set that already has one member.
        let numbering =
            Numbering::of(host).map_err(|link| RepartitionError::Malformed { tree, link })?;
        let mut inside = moving.clone();
        inside.push(instance);
        inside.sort_unstable();
        let vertices = numbering
            .vertices(&inside)
            .ok_or(RepartitionError::NoSuchNode {
                tree,
                node: instance,
            })?;
        let derived = boundary::Boundary::derive(numbering.order(), numbering.links(), &vertices)
            .map_err(|refusal| refusal_error(&numbering, refusal))?;

        let inner = self
            .tree(definition)
            .ok_or(RepartitionError::NoSuchTree(definition))?;
        let selected: BTreeSet<NodeId> = moving.iter().copied().collect();
        let mut plan = InwardPlan {
            moving,
            entry: inner.interface_node(InterfaceSide::Input).map(|n| n.id),
            carried: Vec::new(),
            shared: Vec::new(),
            inbound: Vec::new(),
            outbound: Vec::new(),
            takeovers: Vec::new(),
            passthroughs: Vec::new(),
            dying: Ports::default(),
            lost: Vec::new(),
        };
        let exit = inner.interface_node(InterfaceSide::Output).map(|n| n.id);

        for face in derived.inputs() {
            let producer = numbering.socket(face.producer());
            let mut existing: Vec<u32> = Vec::new();
            let mut fresh: Vec<Socket> = Vec::new();
            for consumer in face.consumers() {
                let socket = numbering.socket(*consumer);
                if socket.node == instance {
                    existing.push(socket.port);
                } else {
                    fresh.push(socket);
                }
            }
            let Some(&first) = fresh.first() else {
                continue;
            };
            existing.sort_unstable();
            // R1586 — each of these consumers is fed by one link today, and the
            // derived link stands in for exactly that one, so its mutedness is
            // read here rather than lost.
            let sinks: Vec<Sink> = fresh
                .iter()
                .map(|&socket| Sink {
                    socket,
                    muted: host.link_into(socket).is_some_and(|l| l.muted),
                })
                .collect();
            if let Some(&port) = existing.first() {
                // One value is one port, and this value already crosses here.
                plan.shared.push((port, sinks));
            } else {
                let port =
                    self.port(tree, first, PortSide::In)
                        .ok_or(RepartitionError::NoSuchNode {
                            tree,
                            node: first.node,
                        })?;
                plan.inbound.push(Face {
                    port,
                    outer: vec![producer],
                    muted: sinks.iter().filter(|s| s.muted).map(|s| s.socket).collect(),
                    inner: fresh,
                });
            }
        }

        for face in derived.outputs() {
            let producer = numbering.socket(face.producer());
            if producer.node == instance {
                continue;
            }
            let port =
                self.port(tree, producer, PortSide::Out)
                    .ok_or(RepartitionError::NoSuchNode {
                        tree,
                        node: producer.node,
                    })?;
            let consumers: Vec<Socket> = face
                .consumers()
                .iter()
                .map(|c| numbering.socket(*c))
                .collect();
            plan.outbound.push(Face {
                port,
                muted: consumers
                    .iter()
                    .copied()
                    .filter(|&c| host.link_into(c).is_some_and(|l| l.muted))
                    .collect(),
                outer: consumers,
                inner: vec![producer],
            });
        }

        for &index in derived.internal() {
            let Some(link) = host.links().get(index) else {
                continue;
            };
            match (
                selected.contains(&link.from.node),
                selected.contains(&link.to.node),
            ) {
                (true, true) => plan.carried.push((
                    link.from,
                    Sink {
                        socket: link.to,
                        muted: link.muted,
                    },
                )),
                (true, false) => {
                    // A moved node feeds the instance, so the value is produced
                    // inside now and the port's own feed is gone. The one link
                    // that replaces the two is muted when either was (R1586) —
                    // the rule an inline and a dissolve both use.
                    let consumers: Vec<Sink> = plan
                        .entry
                        .map(|entry| {
                            inner
                                .links()
                                .iter()
                                .filter(|l| l.from == Socket::new(entry, link.to.port))
                                .map(|l| Sink {
                                    socket: l.to,
                                    muted: l.muted || link.muted,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if consumers.is_empty() {
                        plan.lost.push(link.id);
                    } else {
                        plan.dying.insert(InterfaceSide::Input, link.to.port);
                        plan.takeovers.push(Takeover {
                            producer: link.from,
                            consumers,
                        });
                    }
                }
                (false, true) => {
                    // The instance feeds a moved node: inside, whatever produces
                    // that output feeds it directly.
                    let producer = exit.and_then(|exit| {
                        inner
                            .links()
                            .iter()
                            .find(|l| l.to == Socket::new(exit, link.from.port))
                            .map(|l| (l.from, l.muted))
                    });
                    match producer {
                        Some((producer, muted)) => plan.passthroughs.push((
                            producer,
                            Sink {
                                socket: link.to,
                                muted: muted || link.muted,
                            },
                        )),
                        None => plan.lost.push(link.id),
                    }
                }
                (false, false) => {}
            }
        }
        Ok(plan)
    }

    /// Apply a validated inward plan. Nothing here can fail.
    fn perform_inward(
        &mut self,
        tree: TreeId,
        instance: NodeId,
        definition: TreeId,
        forked_from: Option<TreeId>,
        plan: &InwardPlan<K>,
    ) -> Repartitioned<K> {
        let other_instances = self.instance_count(definition).saturating_sub(1);
        let origin = self
            .tree(tree)
            .and_then(|t| t.node(instance))
            .map_or((0, 0), |n| (n.x, n.y));
        let selected: BTreeSet<NodeId> = plan.moving.iter().copied().collect();

        let mut severed = self.take_incident_links(tree, &selected, &plan.lost);
        // Inward: a host-tree frame is not in the definition, so a container the
        // selection left behind cannot be re-derived — the moved nodes land on
        // the definition's own canvas and the frame is named (R1589).
        let (mapping, orphaned) =
            self.move_nodes(tree, definition, &plan.moving, (-origin.0, -origin.1), None);

        let adding = faces_as_ports(&plan.inbound, &plan.outbound);
        let interface_edit = self.edit_interface(definition, &plan.dying, &adding);
        // A link the interface edit killed inside the definition is one whose
        // consumers this move re-feeds; every other one is a real loss, and the
        // ones at other instances are the whole point of reporting them.
        severed.extend(
            interface_edit
                .killed
                .iter()
                .filter(|dropped| {
                    !(dropped.tree == definition && Some(dropped.link.from.node) == plan.entry)
                })
                .copied(),
        );

        let entry = (!plan.inbound.is_empty() || !plan.shared.is_empty())
            .then(|| self.interface_node_or_create(definition, InterfaceSide::Input));
        let exit = (!plan.outbound.is_empty())
            .then(|| self.interface_node_or_create(definition, InterfaceSide::Output));

        for (from, to) in &plan.carried {
            if let (Some(from), Some(landed)) = (remap(&mapping, *from), remap(&mapping, to.socket))
            {
                self.push_link(definition, from, landed, to.muted);
            }
        }
        for takeover in &plan.takeovers {
            let Some(producer) = remap(&mapping, takeover.producer) else {
                continue;
            };
            for consumer in &takeover.consumers {
                self.push_link(definition, producer, consumer.socket, consumer.muted);
            }
        }
        for (producer, consumer) in &plan.passthroughs {
            if let Some(landed) = remap(&mapping, consumer.socket) {
                self.push_link(definition, *producer, landed, consumer.muted);
            }
        }
        if let Some(entry) = entry {
            for (port, consumers) in &plan.shared {
                let at = interface_edit.survivor(InterfaceSide::Input, *port);
                for consumer in consumers {
                    if let Some(landed) = remap(&mapping, consumer.socket) {
                        self.push_link(definition, Socket::new(entry, at), landed, consumer.muted);
                    }
                }
            }
            for (nth, face) in plan.inbound.iter().enumerate() {
                let at = interface_edit.added(InterfaceSide::Input, nth);
                for consumer in &face.inner {
                    // Read against the identity the consumer had before it
                    // moved, which is the identity the face recorded (R1586).
                    let muted = face.was_muted(*consumer);
                    if let Some(landed) = remap(&mapping, *consumer) {
                        self.push_link(definition, Socket::new(entry, at), landed, muted);
                    }
                }
                for producer in &face.outer {
                    // The shared half of the crossing; the fact rides the
                    // per-consumer half, as it does in a collapse.
                    self.push_link(tree, *producer, Socket::new(instance, at), false);
                }
            }
        }
        if let Some(exit) = exit {
            for (nth, face) in plan.outbound.iter().enumerate() {
                let at = interface_edit.added(InterfaceSide::Output, nth);
                for producer in &face.inner {
                    if let Some(producer) = remap(&mapping, *producer) {
                        self.push_link(definition, producer, Socket::new(exit, at), false);
                    }
                }
                for consumer in &face.outer {
                    let muted = face.was_muted(*consumer);
                    self.push_link(tree, Socket::new(instance, at), *consumer, muted);
                }
            }
        }

        Repartitioned {
            definition,
            forked_from,
            moved: landed(&mapping, &plan.moving),
            exposed: interface_edit.exposed,
            unexposed: interface_edit.unexposed,
            severed,
            other_instances,
            orphaned,
        }
    }

    /// Everything the outward move needs, with every refusal decided here.
    ///
    /// Long for the same reason as [`Self::plan_inward`], which it mirrors.
    #[allow(clippy::too_many_lines)]
    fn plan_outward(
        &self,
        tree: TreeId,
        instance: NodeId,
        definition: TreeId,
        selection: &[NodeId],
    ) -> Result<OutwardPlan<K>, RepartitionError> {
        let inner = self
            .tree(definition)
            .ok_or(RepartitionError::NoSuchTree(definition))?;
        let host = self.tree(tree).ok_or(RepartitionError::NoSuchTree(tree))?;
        let moving = self.chosen(definition, selection)?;
        let selected: BTreeSet<NodeId> = moving.iter().copied().collect();
        let entry = inner.interface_node(InterfaceSide::Input).map(|n| n.id);
        let exit = inner.interface_node(InterfaceSide::Output).map(|n| n.id);

        // What stays behind, minus the interface, becomes one vertex out in the
        // host: the instance. A walk that leaves that set and returns to it is a
        // cycle there. The interface nodes are excluded because they are not
        // part of that vertex — a value reaching a moved node through the entry
        // comes from outside the instance, not from it — and they could not be
        // interior to such a walk anyway, nothing feeding an entry and nothing
        // an exit produces staying in.
        let numbering = Numbering::of(inner).map_err(|link| RepartitionError::Malformed {
            tree: definition,
            link,
        })?;
        let staying: Vec<NodeId> = inner
            .nodes()
            .map(|n| n.id)
            .filter(|id| !selected.contains(id) && Some(*id) != entry && Some(*id) != exit)
            .collect();
        if !staying.is_empty() {
            let vertices = numbering
                .vertices(&staying)
                .ok_or(RepartitionError::NoSuchTree(definition))?;
            boundary::Boundary::derive(numbering.order(), numbering.links(), &vertices)
                .map_err(|refusal| refusal_error(&numbering, refusal))?;
        }

        let mut plan = OutwardPlan {
            moving,
            carried: Vec::new(),
            entry_feeds: Vec::new(),
            exit_takes: Vec::new(),
            inbound: Vec::new(),
            outbound: Vec::new(),
            dying: Ports::default(),
            lost: Vec::new(),
        };
        // One value is one port, so the new crossings are grouped by producer.
        let mut entering: BTreeMap<u32, Vec<Sink>> = BTreeMap::new();
        let mut leaving: BTreeMap<Socket, Vec<(Socket, bool)>> = BTreeMap::new();
        let mut arriving: BTreeMap<Socket, Vec<(Socket, bool)>> = BTreeMap::new();
        for link in inner.links() {
            match (
                selected.contains(&link.from.node),
                selected.contains(&link.to.node),
            ) {
                (true, true) => plan.carried.push((
                    link.from,
                    Sink {
                        socket: link.to,
                        muted: link.muted,
                    },
                )),
                (false, true) if Some(link.from.node) == entry => {
                    entering.entry(link.from.port).or_default().push(Sink {
                        socket: link.to,
                        muted: link.muted,
                    });
                }
                (false, true) => arriving
                    .entry(link.from)
                    .or_default()
                    .push((link.to, link.muted)),
                (true, false) if Some(link.to.node) == exit => {
                    // Two links become one, so the survivor is muted when
                    // either was (R1586).
                    let consumers: Vec<Sink> = host
                        .links()
                        .iter()
                        .filter(|l| l.from == Socket::new(instance, link.to.port))
                        .map(|l| Sink {
                            socket: l.to,
                            muted: l.muted || link.muted,
                        })
                        .collect();
                    plan.dying.insert(InterfaceSide::Output, link.to.port);
                    if consumers.is_empty() {
                        plan.lost.push(link.id);
                    }
                    plan.exit_takes.push((link.from, consumers));
                }
                (true, false) => leaving
                    .entry(link.from)
                    .or_default()
                    .push((link.to, link.muted)),
                (false, false) => {}
            }
        }

        for (port, mut consumers) in entering {
            let feed = host.link_into(Socket::new(instance, port));
            // The outside half of the chain this replaces (R1586).
            if feed.is_some_and(|l| l.muted) {
                for sink in &mut consumers {
                    sink.muted = true;
                }
            }
            let feed = feed.map(|l| l.from);
            if feed.is_none() {
                plan.lost.extend(
                    inner
                        .links()
                        .iter()
                        .filter(|l| {
                            Some(l.from.node) == entry
                                && l.from.port == port
                                && selected.contains(&l.to.node)
                        })
                        .map(|l| l.id),
                );
            }
            let still_feeds_inside = entry.is_some_and(|entry| {
                inner
                    .links()
                    .iter()
                    .any(|l| l.from == Socket::new(entry, port) && !selected.contains(&l.to.node))
            });
            if !still_feeds_inside {
                plan.dying.insert(InterfaceSide::Input, port);
            }
            plan.entry_feeds.push((feed, consumers));
        }
        // A value a moved node sends back inside enters the definition now, so
        // its port is named by the consumer that stays — the inside end, the
        // same rule a collapse uses.
        for (producer, consumers) in leaving {
            let port = self.port(definition, consumers[0].0, PortSide::In).ok_or(
                RepartitionError::NoSuchNode {
                    tree: definition,
                    node: consumers[0].0.node,
                },
            )?;
            plan.inbound.push(Face {
                port,
                outer: vec![producer],
                muted: muted_of(&consumers),
                inner: consumers.into_iter().map(|(socket, _)| socket).collect(),
            });
        }
        for (producer, consumers) in arriving {
            let port = self.port(definition, producer, PortSide::Out).ok_or(
                RepartitionError::NoSuchNode {
                    tree: definition,
                    node: producer.node,
                },
            )?;
            plan.outbound.push(Face {
                port,
                muted: muted_of(&consumers),
                outer: consumers.into_iter().map(|(socket, _)| socket).collect(),
                inner: vec![producer],
            });
        }
        Ok(plan)
    }

    /// Apply a validated outward plan. Nothing here can fail.
    fn perform_outward(
        &mut self,
        tree: TreeId,
        instance: NodeId,
        definition: TreeId,
        forked_from: Option<TreeId>,
        plan: &OutwardPlan<K>,
    ) -> Repartitioned<K> {
        let other_instances = self.instance_count(definition).saturating_sub(1);
        let origin = self
            .tree(tree)
            .and_then(|t| t.node(instance))
            .map_or((0, 0), |n| (n.x, n.y));
        let selected: BTreeSet<NodeId> = plan.moving.iter().copied().collect();

        let mut severed = self.take_incident_links(definition, &selected, &plan.lost);
        // Outward: a node leaving the definition lands where the instance is,
        // which means inside whatever contains the instance (R1589) — the same
        // rule an inline follows, so separating a node and inlining the whole
        // group put it in the same frame.
        let container = self
            .tree(tree)
            .and_then(|t| t.node(instance))
            .and_then(|n| n.parent);
        let (mapping, orphaned) =
            self.move_nodes(definition, tree, &plan.moving, origin, container);

        let adding = faces_as_ports(&plan.inbound, &plan.outbound);
        let interface_edit = self.edit_interface(definition, &plan.dying, &adding);
        // A link killed at THIS instance is one the move re-routes; the rest are
        // losses, and the ones at other instances are why this is reported.
        severed.extend(
            interface_edit
                .killed
                .iter()
                .filter(|dropped| {
                    !(dropped.tree == tree
                        && (dropped.link.to.node == instance || dropped.link.from.node == instance))
                })
                .copied(),
        );

        let entry = (!plan.inbound.is_empty())
            .then(|| self.interface_node_or_create(definition, InterfaceSide::Input));
        let exit = (!plan.outbound.is_empty())
            .then(|| self.interface_node_or_create(definition, InterfaceSide::Output));

        for (from, to) in &plan.carried {
            if let (Some(from), Some(landed)) = (remap(&mapping, *from), remap(&mapping, to.socket))
            {
                self.push_link(tree, from, landed, to.muted);
            }
        }
        for (feed, consumers) in &plan.entry_feeds {
            let Some(feed) = feed else { continue };
            for consumer in consumers {
                if let Some(landed) = remap(&mapping, consumer.socket) {
                    self.push_link(tree, *feed, landed, consumer.muted);
                }
            }
        }
        for (producer, consumers) in &plan.exit_takes {
            let Some(producer) = remap(&mapping, *producer) else {
                continue;
            };
            for consumer in consumers {
                self.push_link(tree, producer, consumer.socket, consumer.muted);
            }
        }
        if let Some(entry) = entry {
            for (nth, face) in plan.inbound.iter().enumerate() {
                let at = interface_edit.added(InterfaceSide::Input, nth);
                for consumer in &face.inner {
                    let muted = face.was_muted(*consumer);
                    self.push_link(definition, Socket::new(entry, at), *consumer, muted);
                }
                for producer in &face.outer {
                    if let Some(producer) = remap(&mapping, *producer) {
                        self.push_link(tree, producer, Socket::new(instance, at), false);
                    }
                }
            }
        }
        if let Some(exit) = exit {
            for (nth, face) in plan.outbound.iter().enumerate() {
                let at = interface_edit.added(InterfaceSide::Output, nth);
                for producer in &face.inner {
                    self.push_link(definition, *producer, Socket::new(exit, at), false);
                }
                for consumer in &face.outer {
                    let muted = face.was_muted(*consumer);
                    if let Some(landed) = remap(&mapping, *consumer) {
                        self.push_link(tree, Socket::new(instance, at), landed, muted);
                    }
                }
            }
        }

        Repartitioned {
            definition,
            forked_from,
            moved: landed(&mapping, &plan.moving),
            exposed: interface_edit.exposed,
            unexposed: interface_edit.unexposed,
            severed,
            other_instances,
            orphaned,
        }
    }
}

/// A socket in the numbering of the tree its node landed in.
fn remap(mapping: &BTreeMap<NodeId, NodeId>, socket: Socket) -> Option<Socket> {
    mapping
        .get(&socket.node)
        .map(|&node| Socket::new(node, socket.port))
}

/// The moved nodes under their new ids, in the order they were planned.
fn landed(mapping: &BTreeMap<NodeId, NodeId>, moving: &[NodeId]) -> Vec<NodeId> {
    moving
        .iter()
        .filter_map(|id| mapping.get(id).copied())
        .collect()
}

/// A refusal from the boundary derivation, in the caller's own ids.
fn refusal_error(numbering: &Numbering, refusal: boundary::Refusal) -> RepartitionError {
    match refusal {
        boundary::Refusal::Empty => RepartitionError::Empty,
        boundary::Refusal::Bypass { path } => RepartitionError::Bypass {
            path: numbering.path(path),
        },
        other => RepartitionError::Boundary(other.to_string()),
    }
}

/// The ports a plan's new crossings ask for, inputs first.
fn faces_as_ports<K: NodeKind>(
    inbound: &[Face<K>],
    outbound: &[Face<K>],
) -> Vec<(InterfaceSide, KindPort<K>)> {
    inbound
        .iter()
        .map(|face| (InterfaceSide::Input, face.port.clone()))
        .chain(
            outbound
                .iter()
                .map(|face| (InterfaceSide::Output, face.port.clone())),
        )
        .collect()
}

/// A `pinion-graph` tree index as a [`TreeId`].
fn tree_id(index: usize) -> TreeId {
    TreeId(u32::try_from(index).unwrap_or(u32::MAX))
}
