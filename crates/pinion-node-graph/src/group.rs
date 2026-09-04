//! R1577 — group definitions, instances, and the path through them.
//!
//! A group is two things that are easy to conflate and must not be: a
//! **definition** (a tree that can be instanced any number of times) and an
//! **instance** (a node that references one). Collapsing a selection creates one
//! of each; instancing again creates only the second, and that is what makes a
//! group re-usable rather than a fold.
//!
//! The interface is *derived*, never authored — see
//! [`pinion_graph::group`] for the rule and for why a
//! collapse can be refused.

use pinion_graph::group as boundary;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::frame::{Orphaned, parents_of};
use crate::model::{
    Admission, Document, InterfaceSide, KindPort, Link, LinkId, Multiplicity, Node, NodeBody,
    NodeId, NodeKind, PortRef, ROOT, Refusal, Side, Socket, Tree, TreeId, centroid, crossing,
};
use crate::numbering::Numbering;

/// Canvas gap between a definition's contents and its interface nodes.
pub(crate) const INTERFACE_GAP: i32 = 220;

/// What collapsing a selection produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grouped {
    /// The new definition.
    pub definition: TreeId,
    /// The instance of it left behind in the host tree.
    pub node: NodeId,
    /// Selected nodes whose frame stayed behind in the host tree, and the frame
    /// each was in (R1589).
    ///
    /// The collapse moves nodes into another tree, where a host-tree frame id
    /// means nothing — so a container that was not itself selected cannot come
    /// along. The instance takes the selection's place *inside* that frame
    /// (the DCC does the same, `gnode->parent = find_common_parent_node(...)`), which is why this is a report rather than
    /// a refusal.
    pub orphaned: Vec<Orphaned>,
}

/// What inlining a group instance produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ungrouped {
    /// The nodes now in the host tree, in the order they were copied.
    pub nodes: Vec<NodeId>,
    /// The links now in the host tree.
    pub links: Vec<LinkId>,
    /// Whether the definition has no instances left anywhere.
    ///
    /// The definition is deliberately **kept** either way: other instances may
    /// use it, an undo has to be able to put this one back, and a library that
    /// deletes a definition the moment its last instance goes is a library you
    /// cannot build in.
    pub definition_unused: bool,
}

/// Why a selection could not be collapsed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GroupError {
    /// Nothing was selected.
    Empty,
    /// No such tree.
    NoSuchTree(TreeId),
    /// A selected node is not in that tree.
    NoSuchNode(NodeId),
    /// A selected node materialises the tree's own interface. Moving one into a
    /// group would detach it from the interface it exists to express, so the
    /// selection is refused rather than silently narrowed.
    InterfaceNodeSelected(NodeId),
    /// A link names a node the tree does not hold, so there is no graph to
    /// derive a boundary from. [`Document::validate`] reports these.
    Malformed {
        /// The link.
        link: LinkId,
    },
    /// The boundary derivation refused for a reason this crate has no arm for.
    ///
    /// `pinion_graph::group::Refusal` is `#[non_exhaustive]`, so it may grow an
    /// arm that postdates this mapping. Carrying the sentence is the only
    /// answer that stays true when it does — mapping an unknown refusal onto a
    /// known one would report a fact that is not so.
    Boundary(String),
    /// Collapsing would make the new node reach itself, along this path.
    Bypass {
        /// The walk that leaves the selection and returns, in the caller's own
        /// node ids.
        path: Vec<NodeId>,
    },
}

impl fmt::Display for GroupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("nothing is selected"),
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode(node) => write!(f, "no node {}", node.0),
            Self::Malformed { link } => {
                write!(f, "link {} names a node this tree does not have", link.0)
            }
            Self::Boundary(reason) => write!(f, "the selection is not groupable: {reason}"),
            Self::InterfaceNodeSelected(node) => write!(
                f,
                "node {} is this tree's own interface and cannot be grouped",
                node.0
            ),
            Self::Bypass { path } => {
                f.write_str("grouping that would create a cycle: ")?;
                for (i, node) in path.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" -> ")?;
                    }
                    write!(f, "{}", node.0)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for GroupError {}

/// ★ R1939 — the sentence the THREE faults about a value authored on one port
/// share, with only the clause that distinguishes them passed in.
///
/// They are a family and their own docs say so: a value on a port that is not
/// there ([`Violation::StrayPortValue`]), one the port cannot hold
/// ([`Violation::MistypedPortValue`]), and one the port's declaration will not
/// admit ([`Violation::InadmissiblePortValue`]). They carry the same address
/// and differ only in the reason, so writing the address three times let the
/// three drift — and a fourth would have been written in whatever shape its
/// author happened to pick.
///
/// Extracted because adding the third arm pushed [`Violation`]'s `Display` past
/// the length this workspace lints for, and the lint was RIGHT: the repair is
/// one named function for the property the three share, not an `allow`.
fn port_value_fault(
    f: &mut fmt::Formatter<'_>,
    tree: TreeId,
    node: NodeId,
    port: PortRef,
    because: &str,
) -> fmt::Result {
    node_fault(
        f,
        tree,
        node,
        format_args!("has a value on port {port}, {because}"),
    )
}

/// A sentence about ONE node in one tree, which is the opening **ten** of
/// [`Violation`]'s eighteen arms share — five of them here, two through
/// [`parent_fault`] and three through [`port_value_fault`].
///
/// Named at R1999 for the reason [`side_word`] was named at R1935 and
/// [`port_value_fault`] above it before that: the length lint fires when a
/// fragment several arms repeat has no name, and it is asking for the right
/// thing. `impl Display` rather than `&str` so the predicate half can
/// interpolate: a plain literal where it does not, `format_args!` where it
/// does, and neither allocates.
fn node_fault(
    f: &mut fmt::Formatter<'_>,
    tree: TreeId,
    node: NodeId,
    said: impl fmt::Display,
) -> fmt::Result {
    write!(f, "node {node} in tree {tree} {said}")
}

/// A node that says it is inside another one and the containment relation
/// cannot honour it — the fragment the two arms about a stated parent share.
fn parent_fault(
    f: &mut fmt::Formatter<'_>,
    tree: TreeId,
    node: NodeId,
    parent: NodeId,
    because: &str,
) -> fmt::Result {
    node_fault(
        f,
        tree,
        node,
        format_args!("says it is inside {parent}, {because}"),
    )
}

/// A sentence about ONE link in one tree, which is what three of the arms are
/// about: [`node_fault`]'s rule, for the other subject this enum has.
fn link_fault(
    f: &mut fmt::Formatter<'_>,
    tree: TreeId,
    link: LinkId,
    said: impl fmt::Display,
) -> fmt::Result {
    write!(f, "link {link} in tree {tree} {said}")
}

/// Why a group instance could not be inlined.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UngroupError {
    /// No such tree.
    NoSuchTree(TreeId),
    /// No such node in that tree.
    NoSuchNode(NodeId),
    /// That node is not a group instance.
    NotAGroup(NodeId),
}

impl fmt::Display for UngroupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode(node) => write!(f, "no node {}", node.0),
            Self::NotAGroup(node) => write!(f, "node {} is not a group instance", node.0),
        }
    }
}

impl std::error::Error for UngroupError {}

/// Why a definition could not be instanced.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NestError {
    /// No such tree to place the instance in.
    NoSuchTree(TreeId),
    /// No such definition.
    NoSuchDefinition(TreeId),
    /// The root tree is the document, not a definition, so it cannot be
    /// instanced.
    NotADefinition(TreeId),
    /// The placement would make a definition contain itself, along this chain.
    Recursion {
        /// The existing containment chain the placement would close, running
        /// from the definition being placed to the tree it would be placed in.
        chain: Vec<TreeId>,
    },
}

impl fmt::Display for NestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchDefinition(tree) => write!(f, "no definition {}", tree.0),
            Self::NotADefinition(tree) => {
                write!(f, "tree {} is the root and cannot be instanced", tree.0)
            }
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

impl std::error::Error for NestError {}

impl<K: NodeKind> Document<K> {
    /// Collapse `selection` in `tree` into a new definition plus one instance.
    ///
    /// Every link crossing the boundary is rewired onto the new node, every link
    /// inside it moves into the definition, and the definition's interface is
    /// derived from those crossings — an interface socket is one value crossing
    /// the boundary, and its name comes from its **internal** end, the end that
    /// survives inside the definition.
    ///
    /// # Errors
    ///
    /// See [`GroupError`]. A refused collapse leaves the document **untouched**:
    /// every check runs before the first mutation.
    pub fn group(
        &mut self,
        tree: TreeId,
        selection: &[NodeId],
        name: impl Into<String>,
    ) -> Result<Grouped, GroupError> {
        let plan = self.plan_group(tree, selection)?;
        Ok(self.perform_group(tree, &plan, name.into()))
    }

    /// Everything the collapse needs, computed before anything is mutated.
    fn plan_group(&self, tree: TreeId, selection: &[NodeId]) -> Result<GroupPlan<K>, GroupError> {
        let host = self.tree(tree).ok_or(GroupError::NoSuchTree(tree))?;
        if selection.is_empty() {
            return Err(GroupError::Empty);
        }
        let mut chosen: Vec<NodeId> = selection.to_vec();
        chosen.sort_unstable();
        chosen.dedup();
        for &id in &chosen {
            let node = host.node(id).ok_or(GroupError::NoSuchNode(id))?;
            if matches!(node.body, NodeBody::Interface(_)) {
                return Err(GroupError::InterfaceNodeSelected(id));
            }
        }

        // Map this tree's nodes onto 0..n, which is the contract
        // `pinion-graph` states for every algorithm it offers.
        let numbering = Numbering::of(host).map_err(|link| GroupError::Malformed { link })?;
        let selected = numbering
            .vertices(&chosen)
            .ok_or(GroupError::NoSuchTree(tree))?;

        let derived = boundary::Boundary::derive(numbering.order(), numbering.links(), &selected)
            .map_err(|refusal| match refusal {
            boundary::Refusal::Empty => GroupError::Empty,
            boundary::Refusal::Bypass { path } => GroupError::Bypass {
                path: numbering.path(path),
            },
            // `UnknownVertex` is unreachable — the indices were built from
            // this tree's own nodes a few lines above — and a future arm is
            // unknown, so both are carried verbatim rather than guessed at.
            other => GroupError::Boundary(other.to_string()),
        })?;

        let socket_of = |s: boundary::Socket| numbering.socket(s);
        let inputs: Vec<Crossing> = derived
            .inputs()
            .iter()
            .map(|face| Crossing {
                producer: socket_of(face.producer()),
                consumers: face.consumers().iter().map(|&c| socket_of(c)).collect(),
                links: face.links().iter().map(|&i| host.links()[i].id).collect(),
            })
            .collect();
        let outputs: Vec<Crossing> = derived
            .outputs()
            .iter()
            .map(|face| Crossing {
                producer: socket_of(face.producer()),
                consumers: face.consumers().iter().map(|&c| socket_of(c)).collect(),
                links: face.links().iter().map(|&i| host.links()[i].id).collect(),
            })
            .collect();

        // The interface takes each port from its INTERNAL end: an input is
        // named by what consumes it inside, an output by what produces it
        // inside. Both are the end that stays in the definition.
        let interface_inputs: Vec<KindPort<K>> = inputs
            .iter()
            .map(|face| {
                let inside = face.consumers[0];
                self.port(tree, inside, PortSide::In)
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(GroupError::NoSuchTree(tree))?;
        let interface_outputs: Vec<KindPort<K>> = outputs
            .iter()
            .map(|face| self.port(tree, face.producer, PortSide::Out))
            .collect::<Option<Vec<_>>>()
            .ok_or(GroupError::NoSuchTree(tree))?;

        let internal: Vec<LinkId> = derived
            .internal()
            .iter()
            .map(|&i| host.links()[i].id)
            .collect();
        let centre = centroid(chosen.iter().filter_map(|&id| {
            let n = host.node(id)?;
            Some((n.x, n.y))
        }));
        Ok(GroupPlan {
            host: tree,
            container: self.common_frame(tree, &chosen),
            chosen,
            inputs,
            outputs,
            interface_inputs,
            interface_outputs,
            internal,
            centre,
        })
    }

    /// Apply a validated plan. Nothing here can fail.
    fn perform_group(&mut self, tree: TreeId, plan: &GroupPlan<K>, name: String) -> Grouped {
        let definition = self.add_definition(name);
        for port in plan.interface_inputs.clone() {
            let _ = self.expose(definition, InterfaceSide::Input, port);
        }
        for port in plan.interface_outputs.clone() {
            let _ = self.expose(definition, InterfaceSide::Output, port);
        }

        let (orphaned, min_x, max_x, mid_y) = self.move_selection_into(definition, plan);

        // Internal links move across unchanged; crossing links go away, their
        // meaning carried by the interface.
        let carried: Vec<Link> = plan
            .internal
            .iter()
            .filter_map(|&id| self.take_link(tree, id))
            .collect();
        for link in carried {
            self.push_link(definition, link.from, link.to, link.muted);
        }
        // R1586 — a crossing becomes TWO links, and only one of them can carry
        // the fact that the value was being stopped. It is the per-consumer
        // half: a face aggregates one producer with several consumers, whose
        // crossings may disagree about mutedness, and an input takes at most one
        // link — so the consumer socket is what names a crossing.
        let mut crossing_muted: BTreeMap<Socket, bool> = BTreeMap::new();
        for face in plan.inputs.iter().chain(&plan.outputs) {
            for &id in &face.links {
                if let Some(link) = self.take_link(tree, id) {
                    crossing_muted.insert(link.to, link.muted);
                }
            }
        }
        let was_muted = |consumer: Socket| crossing_muted.get(&consumer).copied().unwrap_or(false);

        // The interface nodes: the inside end of the group's own sockets.
        let entry = self
            .add_node(
                definition,
                NodeBody::Interface(InterfaceSide::Input),
                min_x - INTERFACE_GAP,
                mid_y,
            )
            .unwrap_or(NodeId(0));
        let exit = self
            .add_node(
                definition,
                NodeBody::Interface(InterfaceSide::Output),
                max_x + INTERFACE_GAP,
                mid_y,
            )
            .unwrap_or(NodeId(0));
        for (index, face) in plan.inputs.iter().enumerate() {
            let port = u32::try_from(index).unwrap_or(u32::MAX);
            for &consumer in &face.consumers {
                self.push_link(
                    definition,
                    Socket::new(entry, port),
                    consumer,
                    was_muted(consumer),
                );
            }
        }
        for (index, face) in plan.outputs.iter().enumerate() {
            let port = u32::try_from(index).unwrap_or(u32::MAX);
            // The shared half of an outbound crossing: several consumers may
            // read this port and disagree, so the fact rides the outside links.
            self.push_link(definition, face.producer, Socket::new(exit, port), false);
        }

        // The instance, wired where the selection was wired — and sitting where
        // the selection sat, inside whatever contained all of it (R1589). A
        // pipeline stage collapsed into a group stays in its pipeline.
        let node = self
            .add_node(
                tree,
                NodeBody::Group(definition),
                plan.centre.0,
                plan.centre.1,
            )
            .unwrap_or(NodeId(0));
        if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(node)) {
            slot.parent = plan.container;
        }
        for (index, face) in plan.inputs.iter().enumerate() {
            let port = u32::try_from(index).unwrap_or(u32::MAX);
            // The shared half of an inbound crossing — see `crossing_muted`.
            self.push_link(tree, face.producer, Socket::new(node, port), false);
        }
        for (index, face) in plan.outputs.iter().enumerate() {
            let port = u32::try_from(index).unwrap_or(u32::MAX);
            for &consumer in &face.consumers {
                self.push_link(tree, Socket::new(node, port), consumer, was_muted(consumer));
            }
        }
        Grouped {
            definition,
            node,
            orphaned,
        }
    }

    /// Move a collapse's chosen nodes into the fresh definition, answering the
    /// containers left behind and the extent the interface nodes are placed
    /// against.
    ///
    /// R1589 — node ids survive a collapse (they are per tree), so a container
    /// that is itself moving stays correctly named and needs no map. One that is
    /// NOT moving is a host-tree id that would name a different node inside the
    /// definition, so it is cleared and reported; the instance takes the
    /// selection's place inside it, which is where the containment goes on
    /// living.
    fn move_selection_into(
        &mut self,
        definition: TreeId,
        plan: &GroupPlan<K>,
    ) -> (Vec<Orphaned>, i32, i32, i32) {
        let chosen: BTreeSet<NodeId> = plan.chosen.iter().copied().collect();
        let mut orphaned = Vec::new();
        let mut moved: Vec<Node<K>> = Vec::new();
        for &id in &plan.chosen {
            let Some(mut node) = self.take_node(plan.host, id) else {
                continue;
            };
            node.x -= plan.centre.0;
            node.y -= plan.centre.1;
            if let Some(frame) = node.parent.filter(|frame| !chosen.contains(frame)) {
                orphaned.push(Orphaned { node: id, frame });
                node.parent = None;
            }
            moved.push(node);
        }
        let (mut min_x, mut max_x, mut mid_y) = (0_i32, 0_i32, 0_i32);
        for node in &moved {
            min_x = min_x.min(node.x);
            max_x = max_x.max(node.x);
            mid_y += node.y;
        }
        if !moved.is_empty() {
            mid_y /= i32::try_from(moved.len()).unwrap_or(1);
        }
        for node in moved {
            self.put_node(definition, node);
        }
        (orphaned, min_x, max_x, mid_y)
    }

    /// Inline a group instance back into its host tree.
    ///
    /// The definition is kept — see [`Ungrouped::definition_unused`].
    ///
    /// # Errors
    ///
    /// See [`UngroupError`]. A refusal leaves the document untouched.
    pub fn ungroup(&mut self, tree: TreeId, node: NodeId) -> Result<Ungrouped, UngroupError> {
        let host = self.tree(tree).ok_or(UngroupError::NoSuchTree(tree))?;
        let instance = host.node(node).ok_or(UngroupError::NoSuchNode(node))?;
        let NodeBody::Group(definition) = instance.body else {
            return Err(UngroupError::NotAGroup(node));
        };
        let (origin_x, origin_y) = (instance.x, instance.y);
        let container = instance.parent;
        let Some(inner) = self.tree(definition) else {
            return Err(UngroupError::NoSuchTree(definition));
        };

        let entry = inner.interface_node(InterfaceSide::Input).map(|n| n.id);
        let exit = inner.interface_node(InterfaceSide::Output).map(|n| n.id);
        let is_interface = |id: NodeId| Some(id) == entry || Some(id) == exit;

        let body: Vec<Node<K>> = inner
            .nodes()
            .filter(|n| !is_interface(n.id))
            .cloned()
            .collect();
        let inner_forest = parents_of(body.iter());
        let inner_links: Vec<Link> = inner.links().to_vec();

        // The instance's own wiring, read before anything moves.
        let incoming: Vec<Link> = host
            .links()
            .iter()
            .filter(|l| l.to.node == node)
            .copied()
            .collect();
        let outgoing: Vec<Link> = host
            .links()
            .iter()
            .filter(|l| l.from.node == node)
            .copied()
            .collect();

        let (renamed, nodes) = self.inline_body(tree, body, (origin_x, origin_y));
        // R1589 — the definition's own frames come along, so its forest is
        // rebuilt in the host's numbering and only its ROOTS are grafted onto
        // whatever contained the instance. The DCC assigns the group node's
        // parent to *every* copied node (`node_group.cc`, the `if
        // (group_node.parent)` loop), which overwrites the parent/child
        // relationships its own copy step had just recreated — so ungrouping a
        // definition that contains frames, inside a frame, flattens it.
        self.remap_parents(tree, &renamed, &inner_forest, container);

        let mut links = Vec::new();
        for link in &inner_links {
            if is_interface(link.from.node) || is_interface(link.to.node) {
                continue;
            }
            let (Some(&from), Some(&to)) =
                (renamed.get(&link.from.node), renamed.get(&link.to.node))
            else {
                continue;
            };
            links.push(self.push_link(
                tree,
                Socket::new(from, link.from.port),
                Socket::new(to, link.to.port),
                link.muted,
            ));
        }

        // An incoming link fed interface input `port`; inside, that port is the
        // entry node's OUTPUT `port`, feeding one or more consumers. The
        // external producer now feeds each of them directly.
        for link in incoming {
            let Some(entry) = entry else { continue };
            for inner_link in inner_links
                .iter()
                .filter(|l| l.from.node == entry && l.from.port == link.to.port)
            {
                if let Some(&to) = renamed.get(&inner_link.to.node) {
                    // R1586 — the inline JOINS the two halves a collapse split,
                    // so the one link that replaces them is muted when either
                    // was: a value that was being stopped goes on being stopped.
                    // This is the exact inverse of the split above, which is
                    // what makes a collapse-then-inline round trip preserve it.
                    links.push(self.push_link(
                        tree,
                        link.from,
                        Socket::new(to, inner_link.to.port),
                        link.muted || inner_link.muted,
                    ));
                }
            }
        }
        // An outgoing link carried interface output `port`; inside, that port is
        // the exit node's INPUT `port`, fed by one producer.
        for link in outgoing {
            let Some(exit) = exit else { continue };
            let Some(inner_link) = inner_links
                .iter()
                .find(|l| l.to.node == exit && l.to.port == link.from.port)
            else {
                continue;
            };
            if let Some(&from) = renamed.get(&inner_link.from.node) {
                links.push(self.push_link(
                    tree,
                    Socket::new(from, inner_link.from.port),
                    link.to,
                    link.muted || inner_link.muted,
                ));
            }
        }

        let _ = self.remove_node(tree, node);
        Ok(Ungrouped {
            nodes,
            links,
            definition_unused: self.instance_count(definition) == 0,
        })
    }

    /// Copy a definition's body into the host tree with FRESH ids, answering the
    /// old-to-new map and the new nodes in copy order.
    ///
    /// The host has its own numbering, and the definition keeps its nodes for
    /// its other instances — so this is a copy, not a move.
    fn inline_body(
        &mut self,
        tree: TreeId,
        body: Vec<Node<K>>,
        origin: (i32, i32),
    ) -> (BTreeMap<NodeId, NodeId>, Vec<NodeId>) {
        let mut renamed: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        let mut nodes = Vec::new();
        for old in body {
            let Ok(fresh) =
                self.add_node(tree, old.body.clone(), origin.0 + old.x, origin.1 + old.y)
            else {
                continue;
            };
            if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(fresh)) {
                slot.adopt_from(&old, crate::model::Adopting::Everything);
            }
            renamed.insert(old.id, fresh);
            nodes.push(fresh);
        }
        (renamed, nodes)
    }

    /// Place another instance of an existing definition.
    ///
    /// This is what makes a group a re-usable definition: the same tree, drawn
    /// in two places, evaluated with different inputs.
    ///
    /// # Errors
    ///
    /// See [`NestError`]; a recursive placement names the chain it would close.
    pub fn instantiate(
        &mut self,
        tree: TreeId,
        definition: TreeId,
        x: i32,
        y: i32,
    ) -> Result<NodeId, NestError> {
        if self.tree(tree).is_none() {
            return Err(NestError::NoSuchTree(tree));
        }
        if definition == ROOT {
            return Err(NestError::NotADefinition(definition));
        }
        if self.tree(definition).is_none() {
            return Err(NestError::NoSuchDefinition(definition));
        }
        if let Some(chain) = nesting_cycle(self, tree, definition) {
            return Err(NestError::Recursion { chain });
        }
        self.add_node(tree, NodeBody::Group(definition), x, y)
            .map_err(|_| NestError::NoSuchTree(tree))
    }

    /// One port of one socket, on the side asked for.
    pub(crate) fn port(&self, tree: TreeId, socket: Socket, side: PortSide) -> Option<KindPort<K>> {
        let signature = self.signature(tree, socket.node)?;
        let ports = match side {
            PortSide::In => signature.inputs,
            PortSide::Out => signature.outputs,
        };
        ports.get(socket.port as usize).cloned()
    }
}

/// Which half of a signature a port is read from.
#[derive(Clone, Copy)]
pub(crate) enum PortSide {
    In,
    Out,
}

/// One value crossing the boundary, in the caller's own ids.
struct Crossing {
    producer: Socket,
    consumers: Vec<Socket>,
    links: Vec<LinkId>,
}

/// Everything a collapse needs, computed before anything is mutated.
struct GroupPlan<K: NodeKind> {
    chosen: Vec<NodeId>,
    inputs: Vec<Crossing>,
    outputs: Vec<Crossing>,
    interface_inputs: Vec<KindPort<K>>,
    interface_outputs: Vec<KindPort<K>>,
    internal: Vec<LinkId>,
    centre: (i32, i32),
    /// The tree the selection is being lifted out of.
    host: TreeId,
    /// The frame that contained all of the selection, which the instance
    /// inherits (R1589).
    container: Option<NodeId>,
}

/// One step of the path into the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathEntry {
    /// The tree being edited at this depth.
    pub tree: TreeId,
    /// The group instance descended through to reach it. `None` at the root.
    pub via: Option<NodeId>,
}

/// Where in the document the user is editing.
///
/// The DCC calls this the tree path, and shows it as a breadcrumb. It is a
/// property of the *view*, not of the document — two panes can be at different
/// depths in one document — so it is a separate value rather than a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditPath {
    entries: Vec<PathEntry>,
}

impl Default for EditPath {
    fn default() -> Self {
        Self::root()
    }
}

/// Why the path could not be moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathError {
    /// The current tree is gone from the document.
    NoSuchTree(TreeId),
    /// No such node in the current tree.
    NoSuchNode(NodeId),
    /// That node is not a group instance, so there is nothing to enter.
    NotAGroup(NodeId),
    /// Already at the root.
    AtRoot,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode(node) => write!(f, "no node {}", node.0),
            Self::NotAGroup(node) => write!(f, "node {} is not a group instance", node.0),
            Self::AtRoot => f.write_str("already at the root"),
        }
    }
}

impl std::error::Error for PathError {}

impl EditPath {
    /// A path sitting at the root tree.
    #[must_use]
    pub fn root() -> Self {
        Self {
            entries: vec![PathEntry {
                tree: ROOT,
                via: None,
            }],
        }
    }

    /// ★ R1919 — a path whose FLOOR is `tree` rather than the document root.
    ///
    /// [`root`](Self::root) is this with `ROOT`, and is kept as its own name
    /// because it is what a view opens at. This one exists because a search
    /// answers *how far from where you asked*, not *how far from the root*:
    /// `Document::find(from, …)` starts here, so a hit's
    /// [`depth`](Self::depth) is measured from the caller's own footing.
    ///
    /// ⚠ `exit` from such a path stops at `tree`, which is the honest reading:
    /// a path that never descended below its floor has nothing above it that
    /// it knows about.
    #[must_use]
    pub fn at(tree: TreeId) -> Self {
        Self {
            entries: vec![PathEntry { tree, via: None }],
        }
    }

    /// The tree being edited.
    ///
    /// Never empty: the root entry cannot be popped, which is why this returns a
    /// value rather than an option.
    #[must_use]
    pub fn current(&self) -> TreeId {
        self.entries.last().map_or(ROOT, |e| e.tree)
    }

    /// How deep the path runs. `0` at the root.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.entries.len() - 1
    }

    /// Every step, root first.
    #[must_use]
    pub fn entries(&self) -> &[PathEntry] {
        &self.entries
    }

    /// Descend into a group instance in the current tree.
    ///
    /// A definition already on the path cannot be re-entered, and that is a
    /// consequence of nesting acyclicity rather than a check here: an ancestor
    /// is never reachable as an instance from its own descendants.
    ///
    /// # Errors
    ///
    /// See [`PathError`].
    pub fn enter<K: NodeKind>(
        &mut self,
        document: &Document<K>,
        node: NodeId,
    ) -> Result<TreeId, PathError> {
        let current = self.current();
        let tree = document
            .tree(current)
            .ok_or(PathError::NoSuchTree(current))?;
        let found = tree.node(node).ok_or(PathError::NoSuchNode(node))?;
        let NodeBody::Group(inner) = found.body else {
            return Err(PathError::NotAGroup(node));
        };
        self.entries.push(PathEntry {
            tree: inner,
            via: Some(node),
        });
        Ok(inner)
    }

    /// Go back up one level, answering the tree now being edited.
    ///
    /// # Errors
    ///
    /// [`PathError::AtRoot`] when there is nothing above.
    pub fn exit(&mut self) -> Result<TreeId, PathError> {
        if self.entries.len() <= 1 {
            return Err(PathError::AtRoot);
        }
        self.entries.pop();
        Ok(self.current())
    }

    /// Drop every step that the document no longer supports, answering how many
    /// were dropped.
    ///
    /// A path is a view over a document that other code edits — inline the group
    /// you are inside, and the step you took to get there no longer exists. The
    /// alternative to pruning is a breadcrumb that names a node that is gone.
    pub fn prune<K: NodeKind>(&mut self, document: &Document<K>) -> usize {
        let mut keep = 1;
        while keep < self.entries.len() {
            let entry = self.entries[keep];
            let parent = self.entries[keep - 1].tree;
            let still_there = entry.via.is_some_and(|via| {
                document
                    .tree(parent)
                    .and_then(|t| t.node(via))
                    .is_some_and(|n| n.body == NodeBody::Group(entry.tree))
            });
            if !still_there {
                break;
            }
            keep += 1;
        }
        let dropped = self.entries.len() - keep;
        self.entries.truncate(keep);
        dropped
    }

    /// The names to show, root first.
    #[must_use]
    pub fn breadcrumb<K: NodeKind>(&self, document: &Document<K>) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| {
                document
                    .tree(entry.tree)
                    .map_or_else(|| format!("tree {}", entry.tree.0), |t| t.name.clone())
            })
            .collect()
    }
}

/// A way the document's invariants are broken.
///
/// Nothing this crate's own edits can produce — every constructor and every
/// operation maintains all four. It exists because a document that arrives from
/// somewhere else (a file, a network peer, a hand-built fixture) has made no
/// such promise, and because an invariant nothing checks is an invariant nobody
/// can rely on.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Violation {
    /// A link names a socket that does not exist.
    DanglingLink {
        /// The tree it is in.
        tree: TreeId,
        /// The link.
        link: LinkId,
    },
    /// ★★★★★ R1999 — a node whose kind is **not at home** in the kind of graph
    /// holding it ([`crate::NodeKind::at_home`]).
    ///
    /// Not reachable through [`Document::add_node`], which refuses it. The two
    /// ways to be holding one are the two this crate always names: a document
    /// arriving from a file written against another rule, and
    /// [`Document::set_graph_kind`] re-classifying a tree that already holds
    /// nodes — which is deliberately allowed to leave this behind rather than
    /// deleting work, exactly as narrowing an admitted set is (R1933).
    ///
    /// ★ The reference has no equivalent. A graph's type changes there by the
    /// document moving it to another list, and nothing walks what is in it
    /// afterwards.
    NotAtHome {
        /// The tree.
        tree: TreeId,
        /// The node whose kind the tree's graph kind does not admit.
        node: NodeId,
    },
    /// ★★★★★ R1935 — a far end of a name whose named endpoint is **gone**.
    ///
    /// A [`NodeBody::Echo`] holds the id of the [`NodeBody::Beacon`] it shows,
    /// and deleting
    /// that beacon is an ordinary edit that knows nothing about echoes — so the
    /// state is representable and this is what makes it *reported*.
    ///
    /// ★ The reference leaves the same question to a predicate an editor may
    /// call and nothing obliges anyone to; a fact that is only true when
    /// someone asks is the shape R1888 recorded. Here it is in the standing
    /// check, so a document says it about itself.
    ///
    /// Its own arm rather than a [`Self::DanglingLink`] because the repairs are
    /// different: a dangling link is removed, and this is repaired by pointing
    /// the echo at a beacon or by removing it — and a screen offering the wrong
    /// one of those is offering to delete work.
    DanglingEcho {
        /// The tree it is in.
        tree: TreeId,
        /// The far end left naming nothing.
        node: NodeId,
    },
    /// A socket holds more links than its [`Multiplicity`] allows (R1599).
    ///
    /// Which sockets those are is not a fixed side: a **value input** takes at
    /// most one link and a **control output** takes at most one link, and the
    /// two are the opposite ends of a link. Named for the rule rather than for
    /// one of its two cases, because before this round only the first case
    /// existed and the name said so (`OverfedInput`).
    Overlinked {
        /// The tree it is in.
        tree: TreeId,
        /// The socket holding too many.
        socket: Socket,
        /// Which side of its node the socket is on — which, with what the port
        /// carries, is what made the limit one.
        side: Side,
    },
    /// A link joins two ports of different types.
    TypeMismatch {
        /// The tree it is in.
        tree: TreeId,
        /// The link.
        link: LinkId,
    },
    /// A link joins two nodes that may not be wired, whatever their ports
    /// carry (R1885).
    ///
    /// Its own arm rather than a [`Self::TypeMismatch`] for the reason
    /// [`ConnectError::Incompatible`](crate::ConnectError::Incompatible) is:
    /// the types match, and an author repairs the two differently.
    Incompatible {
        /// The tree it is in.
        tree: TreeId,
        /// The link.
        link: LinkId,
        /// Which end to change, and the application's sentence saying why.
        refusal: Refusal,
    },
    /// A tree's links form a dependency cycle.
    Cycle {
        /// The tree it is in.
        tree: TreeId,
        /// Every node that lies **on** the cycle, ascending — not the ones
        /// merely downstream of it (R1596).
        ///
        /// A cycle spoils every value below it, so "which nodes have no value"
        /// is a much larger set than "which nodes are the knot", and only the
        /// second one can be acted on. The DCC reports a bool for the tree
        /// (`has_available_link_cycle`) and blames whichever *link* its toposort happened to come out
        /// of order on.
        nodes: Vec<NodeId>,
    },
    /// ★ R1925 — a section of a tree's interface breaks its own rules.
    SectionBroken {
        /// The tree it is in.
        tree: TreeId,
        /// The section.
        section: crate::SectionId,
        /// What is wrong with it.
        breach: crate::SectionBreach,
    },
    /// A group instance names a tree that is not in the document.
    DanglingInstance {
        /// The tree the instance is in.
        tree: TreeId,
        /// The instance.
        node: NodeId,
        /// The definition it names.
        definition: TreeId,
    },
    /// A definition contains itself, directly or transitively.
    Recursion {
        /// The definition that reaches itself.
        definition: TreeId,
    },
    /// A tree has more than one node materialising one side of its interface.
    DuplicateInterfaceNode {
        /// The tree.
        tree: TreeId,
        /// Which side is doubled.
        side: InterfaceSide,
    },
    /// A node says it is inside a node that is not in its tree (R1589).
    DanglingParent {
        /// The tree it is in.
        tree: TreeId,
        /// The contained node.
        node: NodeId,
        /// The container it names.
        parent: NodeId,
    },
    /// A node says it is inside something that is not a
    /// [`NodeBody::Frame`], which is the only body that
    /// contains (R1589).
    ParentNotAFrame {
        /// The tree it is in.
        tree: TreeId,
        /// The contained node.
        node: NodeId,
        /// The container it names.
        parent: NodeId,
    },
    /// A tree's containment is not a forest: this node is inside itself,
    /// directly or through a chain of frames (R1589).
    ///
    /// Unreachable through this crate's API — [`Document::set_parent`] refuses it — and reachable
    /// through a document that arrived from a file or a peer, which is what
    /// `validate` is for. The DCC reaches it through its own editor, because both of
    /// its guards are assertions.
    ContainmentCycle {
        /// The tree it is in.
        tree: TreeId,
        /// A node on the cycle.
        node: NodeId,
    },
    /// A value is authored on a port the node's signature does not have
    /// (R1594).
    ///
    /// Unreachable through [`Document::set_port_value`], which checks the
    /// signature — and reachable two ways from outside it: a document from a
    /// file, and a definition whose interface *shrank* under an instance that
    /// had authored a value on the port that went away. The second is why this
    /// is a violation rather than an impossibility.
    StrayPortValue {
        /// The tree it is in.
        tree: TreeId,
        /// The node holding it.
        node: NodeId,
        /// The port it names.
        port: PortRef,
    },
    /// A node has authored a value its port cannot hold (R1597).
    ///
    /// Distinct from [`Violation::StrayPortValue`], which is a value on a port
    /// that is not there at all: this port exists and the value is the wrong
    /// kind for it. `Document::set_port_value` refuses exactly this, so it can
    /// only reach a document that came from a file or a peer — and until this
    /// existed nothing reported it, so such a node evaluated to no value with
    /// no explanation.
    MistypedPortValue {
        /// The tree it is in.
        tree: TreeId,
        /// The node holding it.
        node: NodeId,
        /// The port it names.
        ///
        /// The two socket types are deliberately NOT carried: this enum is not
        /// generic over the taxonomy, and [`Violation::TypeMismatch`] names a
        /// link the same way. An address is enough to look both up, and making
        /// the whole enum generic to inline two values a caller already has
        /// would reach every consumer of every other variant.
        port: PortRef,
    },
    /// ★★★★★ R1939 — a node has authored a value of the port's own type that
    /// the port's DECLARATION will not admit
    /// ([`NodeKind::takes`]).
    ///
    /// Distinct from [`Violation::MistypedPortValue`], which is a value of the
    /// wrong type: this one type-checks and is still not a value this port
    /// takes — a service number outside the range, a word outside the closed
    /// set. `Document::set_port_value` refuses exactly this, so like its
    /// neighbour it can only reach a document that came from a file or a peer,
    /// **or one whose kind's rule changed under a value authored when it was
    /// still admitted.** That second route is why this is reported rather than
    /// made impossible, and it is the route the neighbouring arms do not have.
    ///
    /// The sentence and the repair are deliberately NOT carried, for
    /// [`Violation::MistypedPortValue`]'s recorded reason: this enum is not
    /// generic over the taxonomy, and an address is enough to ask
    /// [`Document::admits`](crate::Document::admits) for both.
    InadmissiblePortValue {
        /// The tree it is in.
        tree: TreeId,
        /// The node holding it.
        node: NodeId,
        /// The port it names.
        port: PortRef,
    },
    /// A node's variadic run holds more items than its kind declared a ceiling
    /// for (R1632).
    ///
    /// The floor needs no variant: an item list shorter than the minimum is
    /// *topped up* when the signature is resolved, so a short list and a
    /// minimum-length one are the same node. A list past the ceiling cannot be
    /// answered that way — cutting it would take ports that links address —
    /// so it is reported instead.
    TooManyItems {
        /// The tree it is in.
        tree: TreeId,
        /// The node holding them.
        node: NodeId,
        /// Which side's run is too long.
        side: Side,
    },
}

/// ★ R1689 — every *refusal* in this crate has had a sentence since it was
/// written; this — the verdict on a document that arrived from outside — did
/// not, so a consumer reporting one reached for `{:?}` and put Rust syntax in
/// front of a person. It is the type most likely to be shown to one, because
/// the documents that break invariants are the ones that came from a file.
/// ★★★★★ R1936 — **would putting `definition` inside `tree` close a
/// containment cycle?**, answering the chain it would close.
///
/// Lifted out of [`Document::instantiate`] at R1936 because a second verb needs
/// the same guard — [`Document::set_definition`], which points an existing node
/// at a definition and can close exactly the same cycle. Asked rather than
/// re-derived, so the two verbs cannot come to disagree about what nests.
pub(crate) fn nesting_cycle<K: NodeKind>(
    document: &Document<K>,
    tree: TreeId,
    definition: TreeId,
) -> Option<Vec<TreeId>> {
    boundary::Nesting::cycle(
        &document.containment(),
        tree.0 as usize,
        definition.0 as usize,
    )
    .map(|chain| {
        chain
            .into_iter()
            .map(|t| TreeId(u32::try_from(t).unwrap_or(u32::MAX)))
            .collect()
    })
}

/// The word a person reads for a side of a node's signature.
///
/// Lifted out of [`Violation`]'s `Display` at R1935 rather than `allow`-ing the
/// length lint the new arm tipped over: a sentence fragment two arms share is a
/// thing with a name, and the lint asking for it was right.
const fn side_word(side: Side) -> &'static str {
    match side {
        Side::Input => "input",
        Side::Output => "output",
    }
}

/// The same word for a side of a TREE's interface, which is a different type
/// carrying the same two answers.
///
/// Beside [`side_word`] rather than inline in the arm that reads it: a match
/// spelled inside a `write!` argument is a rule with nowhere to be cited from.
const fn interface_word(side: InterfaceSide) -> &'static str {
    match side {
        InterfaceSide::Input => "input",
        InterfaceSide::Output => "output",
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DanglingLink { tree, link } => {
                link_fault(f, *tree, *link, "names a socket that is not there")
            }
            // R1935 — says which node and what a repair would be, because the
            // two repairs are opposite and a reader who cannot tell them apart
            // may delete work. R1924's rule: the sentence a person reads is the
            // one that has to be right.
            Self::DanglingEcho { tree, node } => {
                node_fault(f, *tree, *node, "shows a name whose endpoint is not there")
            }
            // R1999 — names the node and the tree and stops there: what KIND of
            // graph this is, and what that kind is called, are the taxonomy's
            // words and a screen renders them from `Document::graph_kind`.
            Self::NotAtHome { tree, node } => {
                node_fault(f, *tree, *node, "is of a kind this tree does not admit")
            }
            Self::Overlinked { tree, socket, side } => write!(
                f,
                "{} socket {socket} in tree {tree} holds more links than it may",
                side_word(*side)
            ),
            Self::TypeMismatch { tree, link } => {
                link_fault(f, *tree, *link, "joins two ports whose types cannot cross")
            }
            // R1885 — the application's sentence, verbatim, for the reason
            // `Refusal` carries one: only the application knows what makes two
            // of its nodes incompatible, so a paraphrase here would be this
            // crate inventing the fact it was told.
            Self::Incompatible {
                tree,
                link,
                refusal,
            } => link_fault(
                f,
                *tree,
                *link,
                format_args!(
                    "joins two nodes that may not be wired: {} (change its {})",
                    refusal.because,
                    side_word(refusal.end)
                ),
            ),
            Self::Cycle { tree, nodes } => write!(
                f,
                "tree {tree} has a dependency cycle through {}",
                nodes
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::SectionBroken {
                tree,
                section,
                breach,
            } => write!(f, "{section} of tree {tree}: {breach}"),
            Self::DanglingInstance {
                tree,
                node,
                definition,
            } => node_fault(
                f,
                *tree,
                *node,
                format_args!("instances definition {definition}, which is not in the document"),
            ),
            Self::Recursion { definition } => {
                write!(f, "definition {definition} contains itself")
            }
            Self::DuplicateInterfaceNode { tree, side } => write!(
                f,
                "tree {tree} has more than one node for its {} interface",
                interface_word(*side)
            ),
            Self::DanglingParent { tree, node, parent } => {
                parent_fault(f, *tree, *node, *parent, "which is not there")
            }
            Self::ParentNotAFrame { tree, node, parent } => {
                parent_fault(f, *tree, *node, *parent, "which is not a frame")
            }
            Self::ContainmentCycle { tree, node } => node_fault(f, *tree, *node, "contains itself"),
            Self::StrayPortValue { tree, node, port } => {
                port_value_fault(f, *tree, *node, *port, "which it does not have")
            }
            Self::MistypedPortValue { tree, node, port } => {
                port_value_fault(f, *tree, *node, *port, "which that port cannot hold")
            }
            Self::InadmissiblePortValue { tree, node, port } => {
                port_value_fault(f, *tree, *node, *port, "which that port will not admit")
            }
            Self::TooManyItems { tree, node, side } => node_fault(
                f,
                *tree,
                *node,
                format_args!("has more {} items than its kind allows", side_word(*side)),
            ),
        }
    }
}

impl<K: NodeKind> Document<K> {
    /// Every way one node's authored values fail its signature (R1594 / R1597),
    /// ascending by port.
    ///
    /// **Both halves of what `set_port_value` refuses**, because an invariant
    /// checked only at the edit is not an invariant: a value arrives from a file
    /// or a peer too. R1594 checked the first half — a value on a port the
    /// signature does not have — and R1597 added the second, found by migrating
    /// a consumer whose blob could carry a `Float` on a `Vector` port. Nothing
    /// coerced it and nothing flagged it, so the node silently evaluated to no
    /// value at all; a document that cannot be trusted must fail LOUD rather
    /// than be quietly repaired, which is why this reports rather than converts.
    /// A node holding more items than its kind's run allows (R1632).
    ///
    /// Unreachable through [`Document::insert_item`], which refuses at the
    /// ceiling, and reachable two ways from outside it: a document from a file,
    /// and a taxonomy that *lowered* a ceiling between one release and the next.
    /// Reported rather than clamped on read, because clamping would drop ports
    /// that links still address — the honest answer is that the document says
    /// something the kind does not allow, and to say which node.
    fn over_long_runs(tree: &Tree<K>, node: &Node<K>) -> Vec<Violation> {
        let NodeBody::Kind(kind) = &node.body else {
            return Vec::new();
        };
        [Side::Input, Side::Output]
            .into_iter()
            .filter(|&side| {
                kind.variadic(side)
                    .and_then(|run| run.maximum())
                    .is_some_and(|max| node.items.on(side).len() > max as usize)
            })
            .map(|side| Violation::TooManyItems {
                tree: tree.id,
                node: node.id,
                side,
            })
            .collect()
    }

    fn stray_port_values(&self, tree: &Tree<K>, node: &Node<K>) -> Vec<Violation> {
        let Some(signature) = self.signature(tree.id, node.id) else {
            // The node's body names a definition that is gone, which
            // `DanglingInstance` already reports; one cause, one finding.
            return Vec::new();
        };
        node.values
            .iter()
            .filter_map(|(port, value)| {
                let ports = match port.side {
                    Side::Input => &signature.inputs,
                    Side::Output => &signature.outputs,
                };
                match ports.get(port.index as usize) {
                    None => Some(Violation::StrayPortValue {
                        tree: tree.id,
                        node: node.id,
                        port: *port,
                    }),
                    // R1599 — a CONTROL port carries no value at all, so any
                    // authored value on one is wrong, and that is decidable
                    // without asking the taxonomy anything. It is the one arm
                    // a taxonomy that declines to classify its values can still
                    // be held to.
                    Some(declared) if declared.is_control() => Some(Violation::MistypedPortValue {
                        tree: tree.id,
                        node: node.id,
                        port: *port,
                    }),
                    // A taxonomy that declines to classify its own values says
                    // nothing here rather than condemning every one of them:
                    // `value_type` is a provided default whose answer is `None`.
                    Some(declared) => K::value_type(value)
                        .filter(|held| Some(held) != declared.value_type())
                        .map(|_| Violation::MistypedPortValue {
                            tree: tree.id,
                            node: node.id,
                            port: *port,
                        })
                        // ★★★★★ R1939 — and when the TYPE is right, whether the
                        // port's own declaration admits the value. Only when
                        // the type check passed: a mistyped value would be
                        // judged by a rule written for the type it is not, and
                        // two findings about one value send an author to fix it
                        // twice.
                        .or_else(|| {
                            self.takes(tree.id, node.id, *port)
                                .is_some_and(|admits| !admits.judge(value).stands())
                                .then_some(Violation::InadmissiblePortValue {
                                    tree: tree.id,
                                    node: node.id,
                                    port: *port,
                                })
                        }),
                }
            })
            .collect()
    }

    /// Every way one node's containment breaks the forest (R1589).
    fn forest_violations(&self, tree: &Tree<K>, node: &Node<K>) -> Vec<Violation> {
        let Some(parent) = node.parent else {
            return Vec::new();
        };
        let mut found = Vec::new();
        match tree.node(parent) {
            None => found.push(Violation::DanglingParent {
                tree: tree.id,
                node: node.id,
                parent,
            }),
            Some(container) if !container.is_frame() => {
                found.push(Violation::ParentNotAFrame {
                    tree: tree.id,
                    node: node.id,
                    parent,
                });
            }
            Some(_) => {}
        }
        // `ancestry` stops at the first repeat, so a node on a cycle is one
        // whose ancestry ends at a node STILL CLAIMING a parent: the walk
        // stopped because it had been there, not because it reached the canvas.
        let ancestry = self.ancestry(tree.id, node.id);
        let top = ancestry.first().copied().unwrap_or(node.id);
        if tree.node(top).is_some_and(|n| n.parent.is_some()) {
            found.push(Violation::ContainmentCycle {
                tree: tree.id,
                node: node.id,
            });
        }
        found
    }

    /// The pair question for a link that is ALREADY there (R1885).
    ///
    /// ★★★★★ Not redundant with [`Document::connect`]'s refusal. `connect` bars
    /// an inadmissible wire, so the only way to be HOLDING one is to have made
    /// it legal and then broken it — by re-authoring a node's kind, or by
    /// loading a document written against a different rule. Both are exactly
    /// what a validation pass exists to catch and neither is reachable through
    /// `connect`, so leaving this out would have made the rule enforceable only
    /// at the moment of drawing.
    ///
    /// Its own function because [`Document::validate`] is at its line budget and
    /// because this is a different question from the per-link type walk it sits
    /// beside: that one asks about two ports, this one about two nodes.
    fn inadmissible(&self, tree: TreeId, link: &Link) -> Option<Violation> {
        match self.admission(tree, link.from, link.to) {
            Some(Admission::Refused(refusal)) => Some(Violation::Incompatible {
                tree,
                link: link.id,
                refusal,
            }),
            _ => None,
        }
    }

    /// ★★★★★ R1935 — the breaches a walk over LINKS cannot see, because they
    /// are about a name.
    ///
    /// A far end reaches its named endpoint by name, so deleting the endpoint
    /// leaves nothing for a link check to trip over. The reading is
    /// [`Document::dangling_echoes`], called rather than re-implemented, so the
    /// diagnosis and the naming module cannot drift apart.
    ///
    /// Its own function because [`validate`](Self::validate) was already at its
    /// length limit, and because *what has no link to be found by* is a
    /// legitimate section of that check rather than a line inside the link
    /// loop.
    #[must_use]
    fn naming_breaches(&self, tree: TreeId) -> Vec<Violation> {
        self.dangling_echoes(tree)
            .into_iter()
            .map(|node| Violation::DanglingEcho { tree, node })
            .collect()
    }

    /// Every way one tree's LINKS break their own rules — each link on its own,
    /// then what the set of them puts on a socket.
    ///
    /// Lifted out of [`validate`](Self::validate) at R1999, when the graph-kind
    /// pass tipped that function past the length lint. This is the cohesive
    /// piece to take: the two loops are one derivation, because the second
    /// reads a tally the first builds, and nothing else in the pass needs it.
    #[must_use]
    fn wiring_violations(&self, tree: &Tree<K>) -> Vec<Violation> {
        let mut found = Vec::new();
        let mut fed: BTreeMap<(Socket, Side), usize> = BTreeMap::new();
        for link in tree.links() {
            let (Some(source), Some(sink)) = (
                self.signature(tree.id, link.from.node),
                self.signature(tree.id, link.to.node),
            ) else {
                found.push(Violation::DanglingLink {
                    tree: tree.id,
                    link: link.id,
                });
                continue;
            };
            let (Some(out), Some(inp)) = (
                source.outputs.get(link.from.port as usize),
                sink.inputs.get(link.to.port as usize),
            ) else {
                found.push(Violation::DanglingLink {
                    tree: tree.id,
                    link: link.id,
                });
                continue;
            };
            // R1599 — one question covers the type relation AND the flow: a
            // value never crosses into control and control never into a value,
            // and between two values it is the taxonomy's own directed
            // relation.
            if crossing::<K>(out, inp).is_refused() {
                found.push(Violation::TypeMismatch {
                    tree: tree.id,
                    link: link.id,
                });
            }
            found.extend(self.inadmissible(tree.id, link));
            *fed.entry((link.to, Side::Input)).or_default() += 1;
            *fed.entry((link.from, Side::Output)).or_default() += 1;
        }
        for ((socket, side), count) in fed {
            // R1599 — the limit is the port's own, and it inverts with what the
            // port carries: one producer for a value, one successor for
            // control. A socket whose limit is `Many` is never in breach.
            let limit = self
                .signature(tree.id, socket.node)
                .and_then(|s| {
                    let ports = match side {
                        Side::Input => s.inputs,
                        Side::Output => s.outputs,
                    };
                    ports
                        .get(socket.port as usize)
                        .map(|p| p.multiplicity(side))
                })
                .unwrap_or(Multiplicity::Many);
            if limit == Multiplicity::One && count > 1 {
                found.push(Violation::Overlinked {
                    tree: tree.id,
                    socket,
                    side,
                });
            }
        }
        found
    }

    /// Every way this document breaks its own rules, in tree order.
    ///
    /// Empty for any document built through this crate's API.
    #[must_use]
    pub fn validate(&self) -> Vec<Violation> {
        let mut found = Vec::new();
        for tree in self.trees() {
            // ★ R1935 — before the links, because this defect has none.
            found.extend(self.naming_breaches(tree.id));
            // ★★★★★ R1999 — and the nodes this tree's graph kind does not
            // admit, read from `not_at_home` rather than re-walked here: the
            // predicate that refuses the edit and the one that reports the
            // document are one derivation, so a re-classification cannot be
            // judged two ways (R1884).
            found.extend(
                self.not_at_home(tree.id)
                    .into_iter()
                    .map(|node| Violation::NotAtHome {
                        tree: tree.id,
                        node,
                    }),
            );
            found.extend(self.wiring_violations(tree));
            for side in [InterfaceSide::Input, InterfaceSide::Output] {
                let count = tree
                    .nodes()
                    .filter(|n| n.body == NodeBody::Interface(side))
                    .count();
                if count > 1 {
                    found.push(Violation::DuplicateInterfaceNode {
                        tree: tree.id,
                        side,
                    });
                }
            }
            // ★ R1925 — the interface's sections. Nothing this crate's own
            // edits can produce: `assign_section` removes before it adds and
            // `unexpose` keeps the members true, so every arm here is a state a
            // document arriving from a file can be in and an edit cannot.
            found.extend(self.section_violations(tree.id));
            // R1596 — one derivation answers both halves: whether there is a
            // cycle at all is whether the localisation is empty, so the two can
            // never disagree about a tree the way a bool beside a node list can.
            let on_cycle = self.cycle_nodes(tree.id);
            if !on_cycle.is_empty() {
                found.push(Violation::Cycle {
                    tree: tree.id,
                    nodes: on_cycle,
                });
            }
            for node in tree.nodes() {
                if let NodeBody::Group(definition) = node.body {
                    if self.tree(definition).is_none() {
                        found.push(Violation::DanglingInstance {
                            tree: tree.id,
                            node: node.id,
                            definition,
                        });
                    }
                }
                found.extend(self.forest_violations(tree, node));
                found.extend(self.stray_port_values(tree, node));
                found.extend(Self::over_long_runs(tree, node));
            }
        }
        // `Nesting::cycle(_, t, t)` answers the question "may `t` be placed in
        // `t`", which is trivially no; the question HERE is whether the
        // relation as it stands already lets a tree reach itself.
        let containment = self.containment();
        for tree in self.trees() {
            if reaches_itself(&containment, tree.id.0 as usize) {
                found.push(Violation::Recursion {
                    definition: tree.id,
                });
            }
        }
        found
    }
}

/// Whether `tree` reaches itself through containment.
fn reaches_itself(containment: &[(usize, usize)], tree: usize) -> bool {
    let mut queue: Vec<usize> = containment
        .iter()
        .filter(|&&(host, _)| host == tree)
        .map(|&(_, inner)| inner)
        .collect();
    let mut seen: std::collections::BTreeSet<usize> = queue.iter().copied().collect();
    while let Some(current) = queue.pop() {
        if current == tree {
            return true;
        }
        for &(host, inner) in containment.iter().filter(|&&(host, _)| host == current) {
            debug_assert_eq!(host, current);
            if seen.insert(inner) {
                queue.push(inner);
            }
        }
    }
    false
}
