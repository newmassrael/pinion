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
use std::collections::BTreeMap;
use std::fmt;

use crate::model::{
    Document, InterfaceSide, KindPort, Link, LinkId, Node, NodeBody, NodeId, NodeKind, ROOT,
    Socket, TreeId,
};

/// Canvas gap between a definition's contents and its interface nodes.
const INTERFACE_GAP: i32 = 220;

/// What collapsing a selection produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grouped {
    /// The new definition.
    pub definition: TreeId,
    /// The instance of it left behind in the host tree.
    pub node: NodeId,
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
        let node_of: Vec<NodeId> = host.nodes().map(|n| n.id).collect();
        let vertex_of: BTreeMap<NodeId, usize> =
            node_of.iter().enumerate().map(|(i, &n)| (n, i)).collect();
        let links: Vec<boundary::Link> = host
            .links()
            .iter()
            .map(|l| {
                boundary::Link::new(
                    boundary::Socket::new(vertex_of[&l.from.node], l.from.port),
                    boundary::Socket::new(vertex_of[&l.to.node], l.to.port),
                )
            })
            .collect();
        let selected: Vec<usize> = chosen.iter().map(|n| vertex_of[n]).collect();

        let derived =
            boundary::Boundary::derive(node_of.len(), &links, &selected).map_err(|refusal| {
                match refusal {
                    boundary::Refusal::Empty => GroupError::Empty,
                    boundary::Refusal::Bypass { path } => GroupError::Bypass {
                        path: path.into_iter().map(|v| node_of[v]).collect(),
                    },
                    // `UnknownVertex` is unreachable — the indices were built from
                    // this tree's own nodes a few lines above — and a future arm is
                    // unknown, so both are carried verbatim rather than guessed at.
                    other => GroupError::Boundary(other.to_string()),
                }
            })?;

        let socket_of = |s: boundary::Socket| Socket::new(node_of[s.vertex], s.port);
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

        // Move the nodes, keeping their ids (ids are per tree) and their
        // relative positions, recentred so the definition opens around origin.
        let mut moved: Vec<Node<K>> = Vec::new();
        for &id in &plan.chosen {
            if let Some(mut node) = self.take_node(tree, id) {
                node.x -= plan.centre.0;
                node.y -= plan.centre.1;
                moved.push(node);
            }
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

        // Internal links move across unchanged; crossing links go away, their
        // meaning carried by the interface.
        let carried: Vec<Link> = plan
            .internal
            .iter()
            .filter_map(|&id| self.take_link(tree, id))
            .collect();
        for link in carried {
            self.push_link(definition, link.from, link.to);
        }
        for face in plan.inputs.iter().chain(&plan.outputs) {
            for &id in &face.links {
                let _ = self.take_link(tree, id);
            }
        }

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
                self.push_link(definition, Socket::new(entry, port), consumer);
            }
        }
        for (index, face) in plan.outputs.iter().enumerate() {
            let port = u32::try_from(index).unwrap_or(u32::MAX);
            self.push_link(definition, face.producer, Socket::new(exit, port));
        }

        // The instance, wired where the selection was wired.
        let node = self
            .add_node(
                tree,
                NodeBody::Group(definition),
                plan.centre.0,
                plan.centre.1,
            )
            .unwrap_or(NodeId(0));
        for (index, face) in plan.inputs.iter().enumerate() {
            let port = u32::try_from(index).unwrap_or(u32::MAX);
            self.push_link(tree, face.producer, Socket::new(node, port));
        }
        for (index, face) in plan.outputs.iter().enumerate() {
            let port = u32::try_from(index).unwrap_or(u32::MAX);
            for &consumer in &face.consumers {
                self.push_link(tree, Socket::new(node, port), consumer);
            }
        }
        Grouped { definition, node }
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

        // Copy the body in with FRESH ids: the host has its own numbering, and
        // the definition keeps its nodes for its other instances.
        let mut renamed: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        let mut nodes = Vec::new();
        for old in body {
            let Ok(fresh) =
                self.add_node(tree, old.body.clone(), origin_x + old.x, origin_y + old.y)
            else {
                continue;
            };
            if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(fresh)) {
                slot.label.clone_from(&old.label);
            }
            renamed.insert(old.id, fresh);
            nodes.push(fresh);
        }

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
                    links.push(self.push_link(
                        tree,
                        link.from,
                        Socket::new(to, inner_link.to.port),
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
                links.push(self.push_link(tree, Socket::new(from, inner_link.from.port), link.to));
            }
        }

        let _ = self.remove_node(tree, node);
        Ok(Ungrouped {
            nodes,
            links,
            definition_unused: self.instance_count(definition) == 0,
        })
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
        if let Some(chain) =
            boundary::Nesting::cycle(&self.containment(), tree.0 as usize, definition.0 as usize)
        {
            return Err(NestError::Recursion {
                chain: chain
                    .into_iter()
                    .map(|t| TreeId(u32::try_from(t).unwrap_or(u32::MAX)))
                    .collect(),
            });
        }
        self.add_node(tree, NodeBody::Group(definition), x, y)
            .map_err(|_| NestError::NoSuchTree(tree))
    }

    /// One port of one socket, on the side asked for.
    fn port(&self, tree: TreeId, socket: Socket, side: PortSide) -> Option<KindPort<K>> {
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
enum PortSide {
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
}

/// The integer centre of a set of positions, or the origin when it is empty.
fn centroid(points: impl Iterator<Item = (i32, i32)>) -> (i32, i32) {
    let (mut sum_x, mut sum_y, mut count) = (0_i64, 0_i64, 0_i64);
    for (x, y) in points {
        sum_x += i64::from(x);
        sum_y += i64::from(y);
        count += 1;
    }
    if count == 0 {
        return (0, 0);
    }
    (
        i32::try_from(sum_x / count).unwrap_or(0),
        i32::try_from(sum_y / count).unwrap_or(0),
    )
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
/// Blender calls this the tree path, and shows it as a breadcrumb. It is a
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
    /// Two links feed one input.
    OverfedInput {
        /// The tree it is in.
        tree: TreeId,
        /// The input taking more than one link.
        socket: Socket,
    },
    /// A link joins two ports of different types.
    TypeMismatch {
        /// The tree it is in.
        tree: TreeId,
        /// The link.
        link: LinkId,
    },
    /// A tree's links form a dependency cycle.
    Cycle {
        /// The tree it is in.
        tree: TreeId,
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
}

impl<K: NodeKind> Document<K> {
    /// Every way this document breaks its own rules, in tree order.
    ///
    /// Empty for any document built through this crate's API.
    #[must_use]
    pub fn validate(&self) -> Vec<Violation> {
        let mut found = Vec::new();
        for tree in self.trees() {
            let mut fed: BTreeMap<Socket, usize> = BTreeMap::new();
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
                if out.ty != inp.ty {
                    found.push(Violation::TypeMismatch {
                        tree: tree.id,
                        link: link.id,
                    });
                }
                *fed.entry(link.to).or_default() += 1;
            }
            for (socket, count) in fed {
                if count > 1 {
                    found.push(Violation::OverfedInput {
                        tree: tree.id,
                        socket,
                    });
                }
            }
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
            if tree.links().iter().any(|link| {
                link.from.node == link.to.node
                    || self
                        .path_between(tree.id, link.to.node, link.from.node)
                        .is_some()
            }) {
                found.push(Violation::Cycle { tree: tree.id });
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
