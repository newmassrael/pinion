//! R1577 — the model: trees of typed nodes, and the structural edits that
//! maintain their own invariants.
//!
//! The application supplies the taxonomy by implementing [`NodeKind`]; this
//! module supplies everything a node system needs that is *not* taxonomy.

use serde::{Deserialize, Serialize};

use crate::appearance::Appearance;
use std::collections::BTreeMap;
use std::fmt;

/// A tree in the document: the root, or a re-usable group definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TreeId(pub u32);

/// A node, unique **within its tree** — the same numbering Blender's node names
/// use, and what lets a group collapse move nodes between trees without
/// renumbering them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

/// A link, unique within its tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LinkId(pub u32);

/// One end of a link: a port index on a node.
///
/// Whether `port` indexes the node's inputs or its outputs is decided by which
/// end of a [`Link`] the socket sits on, never by the socket itself — so a
/// socket cannot be half-interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Socket {
    /// The node this socket belongs to.
    pub node: NodeId,
    /// The socket's index in that node's input or output list.
    pub port: u32,
}

impl Socket {
    /// A socket on `node` at `port`.
    #[must_use]
    pub const fn new(node: NodeId, port: u32) -> Self {
        Self { node, port }
    }
}

impl fmt::Display for Socket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "node {}.{}", self.node.0, self.port)
    }
}

/// A directed link inside one tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// Stable identity, so a reference survives the deletion of other links.
    pub id: LinkId,
    /// The producing socket (an output).
    pub from: Socket,
    /// The consuming socket (an input).
    pub to: Socket,
    /// Whether the link is **muted**: it is part of the graph's structure and
    /// carries no value, so the port it feeds falls back to its own default
    /// exactly as if nothing were wired to it (R1586).
    ///
    /// This is how a wiring is A/B-tested without being destroyed. It is a
    /// *semantic* declaration — [`Evaluator`](crate::Evaluator) reads it —
    /// while every structural derivation in this crate ignores it, because a
    /// muted link still occupies its input and still crosses a boundary.
    ///
    /// Blender spells this `NODE_LINK_MUTED` and spells a bypassed **node**
    /// `NODE_MUTED`, which are opposite behaviours under one word: a muted link
    /// stops a value, a muted node passes one through. They are named apart
    /// here — see [`Node::bypassed`].
    #[serde(default)]
    pub muted: bool,
}

/// A consumer a derivation is about to feed, and whether the link it stands in
/// for was muted (R1586).
///
/// Every structural derivation in this crate routes by *consumer*, because an
/// input takes at most one link and so a consumer socket names a link. That is
/// what lets mutedness survive a group collapse, a paste and a boundary move
/// without being threaded through as a separate map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sink {
    /// The consuming socket.
    pub socket: Socket,
    /// Whether the value reaching it was being stopped.
    pub muted: bool,
}

/// One socket in a node's signature.
///
/// `default` is the value the port carries when nothing is linked to it — the
/// "pin default" of Blender and Blueprint. It is retained while the port is
/// wired, because wiring hides an authored value rather than discarding it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port<T, V> {
    /// Human-facing name. Not an identity — ports are addressed by index.
    pub name: String,
    /// The application's socket type. This crate only ever compares two.
    pub ty: T,
    /// The value used when the port is unlinked. `None` on outputs, and on
    /// inputs that have no meaningful resting value.
    pub default: Option<V>,
}

impl<T, V> Port<T, V> {
    /// A port with a name, a type, and no default.
    pub fn new(name: impl Into<String>, ty: T) -> Self {
        Self {
            name: name.into(),
            ty,
            default: None,
        }
    }

    /// The same port carrying a resting value.
    #[must_use]
    pub fn with_default(mut self, value: V) -> Self {
        self.default = Some(value);
        self
    }
}

/// The application's node taxonomy.
///
/// This is the whole of what a consumer supplies. Everything else in this crate
/// — trees, links, groups, the edit path, evaluation — is mechanism that does
/// not know what a node *is*.
///
/// The three structural node kinds a group system needs ([`NodeBody::Group`],
/// [`NodeBody::Interface`]) are owned by this crate rather than left to the
/// implementor, so an application cannot forget to handle them and cannot get
/// them wrong.
/// # Serialization
///
/// [`Document`] and its parts derive serde over this trait's types, so a
/// taxonomy that derives `Serialize` and `Deserialize` makes the whole document
/// saveable and lets it live in a reactive `Signal<T>`, which requires
/// `DeserializeOwned`.
///
/// A taxonomy that carries a **borrowed** field — a `&'static str` for a name,
/// say — is only `Deserialize<'static>`, not `for<'de> Deserialize<'de>`, and
/// that is invisible until something asks for the owned form. Own the strings.
pub trait NodeKind: Clone + PartialEq + fmt::Debug {
    /// The application's socket type. Two ports may be linked when their types
    /// are equal; this crate attaches no other meaning, so an implementor whose
    /// types coerce models that by making the coercion part of equality or by
    /// declaring one type.
    type Type: Clone + PartialEq + fmt::Debug;

    /// The value that flows along a link.
    ///
    /// `PartialEq` and not `Eq`, because the commonest value type a node graph
    /// carries is a float.
    type Value: Clone + PartialEq + fmt::Debug;

    /// A stable identity token — the answer to "what does this node do".
    ///
    /// Never derived from a user-facing label: a node renamed "Foo" still
    /// multiplies.
    fn name(&self) -> String;

    /// This kind's input ports, in order.
    fn inputs(&self) -> Vec<Port<Self::Type, Self::Value>>;

    /// This kind's output ports, in order.
    fn outputs(&self) -> Vec<Port<Self::Type, Self::Value>>;

    /// Compute every output from the already-resolved inputs.
    ///
    /// `inputs` is exactly as long as [`Self::inputs`]; a `None` slot is an
    /// input that could not be resolved at all (no link, no default). The
    /// returned vector is truncated or padded with `None` to the output arity by
    /// the evaluator, so an implementor that returns the wrong length degrades
    /// rather than corrupting the frame.
    fn evaluate(&self, inputs: &[Option<Self::Value>]) -> Vec<Option<Self::Value>>;
}

/// A [`Port`] specialised to one taxonomy.
pub type KindPort<K> = Port<<K as NodeKind>::Type, <K as NodeKind>::Value>;

/// Which half of its tree's interface an interface node materialises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InterfaceSide {
    /// The group's inputs, seen from inside: a node whose OUTPUTS are the
    /// tree's interface inputs.
    Input,
    /// The group's outputs, seen from inside: a node whose INPUTS are the
    /// tree's interface outputs.
    Output,
}

/// What a node is.
///
/// The application's own kinds are the leaves; the two structural arms are this
/// crate's, and are what make a node able to *be* a graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeBody<K> {
    /// An application node.
    Kind(K),
    /// An instance of a group definition. Its signature is that tree's
    /// interface — not a copy of it, so editing the definition re-signatures
    /// every instance at once.
    Group(TreeId),
    /// The inside end of this tree's own interface.
    Interface(InterfaceSide),
}

/// A node: what it is, where it sits, and what it is called.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node<K> {
    /// Stable within its tree.
    pub id: NodeId,
    /// What the node is. Identity for evaluation.
    pub body: NodeBody<K>,
    /// Canvas position, in the application's own units.
    pub x: i32,
    /// Canvas position, in the application's own units.
    pub y: i32,
    /// A user-facing rename. `None` means "call it what its body is called".
    pub label: Option<String>,
    /// Whether the node is **bypassed**: it does not compute, and the values
    /// arriving at its inputs pass straight out of its outputs (R1586).
    ///
    /// This is the one fact on a node, other than its body and its links, that
    /// changes what the graph *means* — so it is a field of its own rather than
    /// a bit in a word shared with the node's looks. Which input reaches which
    /// output is derived, not authored: see [`Document::passthrough`].
    ///
    /// Blender spells this `NODE_MUTED` and keeps it in the same `flag` integer
    /// as `NODE_COLLAPSED`, `NODE_PREVIEW` and even `NODE_SELECT`, so nothing in
    /// its model says which of those bits its evaluator may read.
    #[serde(default)]
    pub bypassed: bool,
    /// What the node looks like — never what it means.
    ///
    /// It lives in the document for the same reason `x` and `y` do: it must
    /// travel with the node through a group collapse, a fragment and a paste,
    /// and a side table keyed by [`NodeId`] would not, because those operations
    /// move nodes between trees.
    #[serde(default)]
    pub appearance: Appearance,
}

impl<K: NodeKind> Node<K> {
    /// The name to show: the rename if there is one, else the body's own.
    #[must_use]
    pub fn display_name(&self) -> String {
        if let Some(label) = &self.label {
            return label.clone();
        }
        match &self.body {
            NodeBody::Kind(kind) => kind.name(),
            NodeBody::Group(_) => "Group".to_owned(),
            NodeBody::Interface(InterfaceSide::Input) => "Group Input".to_owned(),
            NodeBody::Interface(InterfaceSide::Output) => "Group Output".to_owned(),
        }
    }

    /// Take on every fact about `source` that is not its identity, its body or
    /// its place: the rename, the bypass, the looks.
    ///
    /// The one place a node is copied *without* being moved — which is what a
    /// paste and an inline both do, since each mints a fresh id in a tree that
    /// has its own numbering. Before R1586 those two sites copied the label and
    /// nothing else, which was complete at the time and silently would not have
    /// been the moment a field was added; the compiler cannot see a hand-rolled
    /// copy the way it sees a struct literal.
    ///
    /// So `source` is **destructured**: a field added to [`Node`] fails to
    /// compile here until someone has said whether a copy carries it.
    pub(crate) fn adopt_from(&mut self, source: &Self) {
        let Self {
            id: _,
            body: _,
            x: _,
            y: _,
            label,
            bypassed,
            appearance,
        } = source;
        self.label.clone_from(label);
        self.bypassed = *bypassed;
        self.appearance.clone_from(appearance);
    }
}

/// What a tree exposes when it is instanced as a group.
///
/// This is the single statement of a group's shape. The instance node's
/// signature, the inside [`InterfaceSide::Input`] node's outputs and the inside
/// [`InterfaceSide::Output`] node's inputs are all *derived* from it, so those
/// three can never disagree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Serialize, K::Type: Serialize, K::Value: Serialize",
    deserialize = "K: Deserialize<'de>, K::Type: Deserialize<'de>, K::Value: Deserialize<'de>"
))]
pub struct Interface<K: NodeKind> {
    inputs: Vec<KindPort<K>>,
    outputs: Vec<KindPort<K>>,
}

impl<K: NodeKind> Default for Interface<K> {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }
}

impl<K: NodeKind> Interface<K> {
    /// The ports an instance of this tree takes.
    #[must_use]
    pub fn inputs(&self) -> &[KindPort<K>] {
        &self.inputs
    }

    /// The ports an instance of this tree emits.
    #[must_use]
    pub fn outputs(&self) -> &[KindPort<K>] {
        &self.outputs
    }

    /// Whether the tree exposes nothing — a closed sub-graph.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty()
    }

    /// The ports on one side, chosen by value rather than by accessor.
    ///
    /// [`InterfaceSide`] is the thing that says which half is meant, so the
    /// translation from it to a port list belongs in one place — R1584 found
    /// this `match` written out three times, twice here and once in the
    /// boundary move.
    #[must_use]
    pub fn side(&self, side: InterfaceSide) -> &[KindPort<K>] {
        match side {
            InterfaceSide::Input => &self.inputs,
            InterfaceSide::Output => &self.outputs,
        }
    }

    /// The same, for modification.
    pub(crate) fn side_mut(&mut self, side: InterfaceSide) -> &mut Vec<KindPort<K>> {
        match side {
            InterfaceSide::Input => &mut self.inputs,
            InterfaceSide::Output => &mut self.outputs,
        }
    }
}

/// One tree: the root document graph, or a re-usable group definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Serialize, K::Type: Serialize, K::Value: Serialize",
    deserialize = "K: Deserialize<'de>, K::Type: Deserialize<'de>, K::Value: Deserialize<'de>"
))]
pub struct Tree<K: NodeKind> {
    /// Which tree this is.
    pub id: TreeId,
    /// A human-facing name. For a definition this is what the palette shows.
    pub name: String,
    #[serde(with = "node_map")]
    nodes: BTreeMap<NodeId, Node<K>>,
    links: Vec<Link>,
    interface: Interface<K>,
    next_node: u32,
    next_link: u32,
}

impl<K: NodeKind> Tree<K> {
    /// Every node, ascending by id.
    pub fn nodes(&self) -> impl Iterator<Item = &Node<K>> {
        self.nodes.values()
    }

    /// How many nodes the tree holds.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// One node, if it is here.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Option<&Node<K>> {
        self.nodes.get(&id)
    }

    /// One node for modification. The body is deliberately reachable: a rename
    /// or a move is the application's business, and neither can break an
    /// invariant this crate maintains.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node<K>> {
        self.nodes.get_mut(&id)
    }

    /// Every link, in creation order.
    #[must_use]
    pub fn links(&self) -> &[Link] {
        &self.links
    }

    /// One link, if it is here.
    #[must_use]
    pub fn link(&self, id: LinkId) -> Option<&Link> {
        self.links.iter().find(|l| l.id == id)
    }

    /// What this tree exposes when instanced.
    #[must_use]
    pub fn interface(&self) -> &Interface<K> {
        &self.interface
    }

    /// The link feeding `socket`, if any. An input takes at most one.
    #[must_use]
    pub fn link_into(&self, socket: Socket) -> Option<&Link> {
        self.links.iter().find(|l| l.to == socket)
    }

    /// The sole node materialising `side`, if the tree has one.
    #[must_use]
    pub fn interface_node(&self, side: InterfaceSide) -> Option<&Node<K>> {
        self.nodes
            .values()
            .find(|n| n.body == NodeBody::Interface(side))
    }
}

/// The whole document: every tree, with tree `0` the root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Serialize, K::Type: Serialize, K::Value: Serialize",
    deserialize = "K: Deserialize<'de>, K::Type: Deserialize<'de>, K::Value: Deserialize<'de>"
))]
pub struct Document<K: NodeKind> {
    trees: Vec<Tree<K>>,
}

/// The root tree, which always exists.
pub const ROOT: TreeId = TreeId(0);

impl<K: NodeKind> Document<K> {
    /// A document holding one empty root tree.
    #[must_use]
    pub fn new(root_name: impl Into<String>) -> Self {
        Self {
            trees: vec![Tree {
                id: ROOT,
                name: root_name.into(),
                nodes: BTreeMap::new(),
                links: Vec::new(),
                interface: Interface::default(),
                next_node: 0,
                next_link: 0,
            }],
        }
    }

    /// Every tree, root first.
    pub fn trees(&self) -> impl Iterator<Item = &Tree<K>> {
        self.trees.iter()
    }

    /// How many trees the document holds, the root included.
    #[must_use]
    pub fn tree_count(&self) -> usize {
        self.trees.len()
    }

    /// One tree.
    #[must_use]
    pub fn tree(&self, id: TreeId) -> Option<&Tree<K>> {
        self.trees.get(id.0 as usize)
    }

    /// One tree for modification.
    pub fn tree_mut(&mut self, id: TreeId) -> Option<&mut Tree<K>> {
        self.trees.get_mut(id.0 as usize)
    }

    /// The id [`Self::add_definition`] would hand out next.
    ///
    /// Public to this crate so a plan can name a tree it has not created yet —
    /// which is what lets an insertion decide, *before mutating anything*,
    /// whether the definitions it is about to add would close a containment
    /// cycle. It is the same expression the allocation uses rather than a second
    /// copy of it, so the two cannot drift.
    #[must_use]
    pub(crate) fn next_tree_id(&self) -> TreeId {
        TreeId(u32::try_from(self.trees.len()).unwrap_or(u32::MAX))
    }

    /// Copy a tree wholesale and answer the copy's id.
    ///
    /// The copy keeps the original's name: a name is not an identity here, so
    /// two definitions may share one. Blender must rename a copied node group
    /// (`Sum` becomes `Sum.001`) because an ID's name *is* its key.
    pub(crate) fn copy_tree(&mut self, source: TreeId) -> Option<TreeId> {
        let mut copy = self.trees.get(source.0 as usize)?.clone();
        let id = self.next_tree_id();
        copy.id = id;
        self.trees.push(copy);
        Some(id)
    }

    /// Add an empty group definition and answer its id.
    ///
    /// A definition created this way has no interface and no instances; it
    /// becomes reachable when something instantiates it.
    pub fn add_definition(&mut self, name: impl Into<String>) -> TreeId {
        let id = self.next_tree_id();
        self.trees.push(Tree {
            id,
            name: name.into(),
            nodes: BTreeMap::new(),
            links: Vec::new(),
            interface: Interface::default(),
            next_node: 0,
            next_link: 0,
        });
        id
    }

    /// Every group definition (that is, every tree but the root).
    pub fn definitions(&self) -> impl Iterator<Item = &Tree<K>> {
        self.trees.iter().skip(1)
    }

    /// The containment relation over trees: `(host, inner)` for every group
    /// instance. This is what makes nesting acyclicity answerable.
    #[must_use]
    pub fn containment(&self) -> Vec<(usize, usize)> {
        let mut pairs: Vec<(usize, usize)> = self
            .trees
            .iter()
            .flat_map(|tree| {
                tree.nodes.values().filter_map(move |node| match node.body {
                    NodeBody::Group(inner) => Some((tree.id.0 as usize, inner.0 as usize)),
                    _ => None,
                })
            })
            .collect();
        pairs.sort_unstable();
        pairs.dedup();
        pairs
    }

    /// How many instances of `definition` exist across the whole document.
    #[must_use]
    pub fn instance_count(&self, definition: TreeId) -> usize {
        self.trees
            .iter()
            .flat_map(|t| t.nodes.values())
            .filter(|n| n.body == NodeBody::Group(definition))
            .count()
    }

    /// The signature a node presents: its input and output ports.
    ///
    /// The one place the four node bodies are turned into ports, so nothing
    /// downstream — paint, hit-test, evaluation, introspection — re-derives it
    /// and gets a different answer.
    #[must_use]
    pub fn signature(&self, tree: TreeId, node: NodeId) -> Option<Signature<K>> {
        let host = self.tree(tree)?;
        let node = host.node(node)?;
        Some(match &node.body {
            NodeBody::Kind(kind) => Signature {
                inputs: kind.inputs(),
                outputs: kind.outputs(),
            },
            NodeBody::Group(inner) => {
                let definition = self.tree(*inner)?;
                Signature {
                    inputs: definition.interface.inputs.clone(),
                    outputs: definition.interface.outputs.clone(),
                }
            }
            // Seen from inside, the tree's interface inputs are things this node
            // PRODUCES — hence the swap. It is the whole content of the arm.
            NodeBody::Interface(InterfaceSide::Input) => Signature {
                inputs: Vec::new(),
                outputs: host.interface.inputs.clone(),
            },
            NodeBody::Interface(InterfaceSide::Output) => Signature {
                inputs: host.interface.outputs.clone(),
                outputs: Vec::new(),
            },
        })
    }

    /// Add a node to `tree` and answer its fresh id.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] when `tree` is not in the document.
    pub fn add_node(
        &mut self,
        tree: TreeId,
        body: NodeBody<K>,
        x: i32,
        y: i32,
    ) -> Result<NodeId, EditError> {
        let host = self
            .trees
            .get_mut(tree.0 as usize)
            .ok_or(EditError::NoSuchTree(tree))?;
        let id = NodeId(host.next_node);
        host.next_node += 1;
        host.nodes.insert(
            id,
            Node {
                id,
                body,
                x,
                y,
                label: None,
                bypassed: false,
                appearance: Appearance::default(),
            },
        );
        Ok(id)
    }

    /// Remove a node and every link touching it, answering the links dropped.
    ///
    /// Reporting them is what lets an undo stack put the node back whole;
    /// silently dropping incident links is how a "delete" becomes unrepeatable.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchNode`].
    pub fn remove_node(&mut self, tree: TreeId, node: NodeId) -> Result<Vec<Link>, EditError> {
        let host = self
            .trees
            .get_mut(tree.0 as usize)
            .ok_or(EditError::NoSuchTree(tree))?;
        if host.nodes.remove(&node).is_none() {
            return Err(EditError::NoSuchNode { tree, node });
        }
        Ok(self.unwire_node(tree, node))
    }

    /// Remove every link touching `node`, answering them; the node stays.
    ///
    /// The half of [`Self::remove_node`] that [`Self::detach`] also needs, kept
    /// in one place so "which links touch this node" has one definition
    /// (R1586).
    pub(crate) fn unwire_node(&mut self, tree: TreeId, node: NodeId) -> Vec<Link> {
        let Some(host) = self.trees.get_mut(tree.0 as usize) else {
            return Vec::new();
        };
        let (dropped, kept) = host
            .links
            .iter()
            .partition(|l| l.from.node == node || l.to.node == node);
        host.links = kept;
        dropped
    }

    /// Link one socket to another.
    ///
    /// The four things that can be wrong are checked here rather than left to
    /// the caller, because every one of them is a property of the graph and not
    /// of the gesture: the sockets must exist, their types must agree, the
    /// consuming end must be an input (which takes at most one link), and the
    /// link must not close a cycle.
    ///
    /// An input that is already linked is **replaced**, and the displaced link
    /// is reported — that is what a node editor does when a wire is dropped on
    /// an occupied socket, and reporting it is what makes the replacement
    /// undoable. Blender performs the same replacement and returns nothing.
    ///
    /// # Errors
    ///
    /// See [`ConnectError`]; a refusal names the sockets and, for a cycle, the
    /// path it would close.
    pub fn connect(
        &mut self,
        tree: TreeId,
        from: Socket,
        to: Socket,
    ) -> Result<Connected, ConnectError<K::Type>> {
        let out_ports = self
            .signature(tree, from.node)
            .ok_or(ConnectError::NoSuchNode(from))?
            .outputs;
        let in_ports = self
            .signature(tree, to.node)
            .ok_or(ConnectError::NoSuchNode(to))?
            .inputs;
        let source = out_ports
            .get(from.port as usize)
            .ok_or(ConnectError::NoSuchPort {
                socket: from,
                arity: u32::try_from(out_ports.len()).unwrap_or(u32::MAX),
            })?;
        let sink = in_ports
            .get(to.port as usize)
            .ok_or(ConnectError::NoSuchPort {
                socket: to,
                arity: u32::try_from(in_ports.len()).unwrap_or(u32::MAX),
            })?;
        if source.ty != sink.ty {
            return Err(ConnectError::TypeMismatch {
                from,
                from_type: source.ty.clone(),
                to,
                to_type: sink.ty.clone(),
            });
        }
        if from.node == to.node {
            return Err(ConnectError::SelfLink(from.node));
        }
        if let Some(path) = self.path_between(tree, to.node, from.node) {
            return Err(ConnectError::WouldCycle { path });
        }

        // The tree exists: `signature` above resolved through it twice.
        let Some(host) = self.trees.get_mut(tree.0 as usize) else {
            return Err(ConnectError::NoSuchNode(from));
        };
        let displaced = host.links.iter().find(|l| l.to == to).copied();
        if let Some(displaced) = displaced {
            host.links.retain(|l| l.id != displaced.id);
        }
        let id = LinkId(host.next_link);
        host.next_link += 1;
        host.links.push(Link {
            id,
            from,
            to,
            muted: false,
        });
        Ok(Connected {
            link: id,
            displaced,
        })
    }

    /// Remove a link, answering it.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchLink`].
    pub fn disconnect(&mut self, tree: TreeId, link: LinkId) -> Result<Link, EditError> {
        let host = self
            .trees
            .get_mut(tree.0 as usize)
            .ok_or(EditError::NoSuchTree(tree))?;
        let at = host
            .links
            .iter()
            .position(|l| l.id == link)
            .ok_or(EditError::NoSuchLink { tree, link })?;
        Ok(host.links.remove(at))
    }

    /// Bypass `node`, or stop bypassing it, answering what it was before.
    ///
    /// A bypassed node does not compute: the values arriving at its inputs pass
    /// straight out of its outputs, by the routing
    /// [`Self::passthrough`] derives. Nothing structural changes, which is the
    /// point — this is the non-destructive twin of [`Self::dissolve`], and the
    /// two share one derivation so they cannot disagree.
    ///
    /// This is the same field [`Tree::node_mut`] reaches, not a second copy of
    /// it. Blender's `NODE_OT_mute_toggle`.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchNode`].
    pub fn set_bypassed(
        &mut self,
        tree: TreeId,
        node: NodeId,
        bypassed: bool,
    ) -> Result<bool, EditError> {
        let host = self
            .trees
            .get_mut(tree.0 as usize)
            .ok_or(EditError::NoSuchTree(tree))?;
        let target = host
            .nodes
            .get_mut(&node)
            .ok_or(EditError::NoSuchNode { tree, node })?;
        Ok(std::mem::replace(&mut target.bypassed, bypassed))
    }

    /// Mute `link`, or unmute it, answering what it was before.
    ///
    /// A muted link keeps its place in the structure and carries no value, so
    /// the port it feeds falls back to its own default. Blender's
    /// `NODE_OT_links_mute`.
    ///
    /// Narrower than a `link_mut` on purpose: mutedness is the only part of a
    /// link that may be changed in place, because the endpoints are what every
    /// invariant in this crate is stated over.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchLink`].
    pub fn set_link_muted(
        &mut self,
        tree: TreeId,
        link: LinkId,
        muted: bool,
    ) -> Result<bool, EditError> {
        let host = self
            .trees
            .get_mut(tree.0 as usize)
            .ok_or(EditError::NoSuchTree(tree))?;
        let target = host
            .links
            .iter_mut()
            .find(|l| l.id == link)
            .ok_or(EditError::NoSuchLink { tree, link })?;
        Ok(std::mem::replace(&mut target.muted, muted))
    }

    /// Take a node out of a tree without touching any link.
    ///
    /// The link bookkeeping is the caller's here — which is exactly why this is
    /// crate-private. The group collapse is the one operation that moves a node
    /// between trees, and it has already worked out where every incident link
    /// goes; [`Self::remove_node`] would drop them.
    pub(crate) fn take_node(&mut self, tree: TreeId, node: NodeId) -> Option<Node<K>> {
        self.trees.get_mut(tree.0 as usize)?.nodes.remove(&node)
    }

    /// Put a node into a tree under its existing id, keeping the id source
    /// ahead of it.
    pub(crate) fn put_node(&mut self, tree: TreeId, node: Node<K>) {
        let Some(host) = self.trees.get_mut(tree.0 as usize) else {
            return;
        };
        host.next_node = host.next_node.max(node.id.0 + 1);
        host.nodes.insert(node.id, node);
    }

    /// Take a link out of a tree.
    pub(crate) fn take_link(&mut self, tree: TreeId, link: LinkId) -> Option<Link> {
        let host = self.trees.get_mut(tree.0 as usize)?;
        let at = host.links.iter().position(|l| l.id == link)?;
        Some(host.links.remove(at))
    }

    /// Add a link that has already been shown to be valid.
    ///
    /// Crate-private and unchecked on purpose: the group operations derive every
    /// link they add from a boundary that was validated before the first
    /// mutation, so re-running the checks here would be a second opinion that
    /// can only ever disagree by being wrong — and a refusal halfway through a
    /// collapse would leave a half-built document, which the plan-then-perform
    /// split exists to make impossible. [`Self::validate`] is the standing
    /// check that this trust is warranted.
    ///
    /// `muted` is an argument rather than a default because a derived link
    /// stands in for one that already existed, and whether *that* one carried a
    /// value is a fact the derivation must not quietly discard (R1586). Making
    /// it a parameter is what turns "did every link-moving operation preserve
    /// mutedness?" into something the compiler asks at each of its call sites.
    pub(crate) fn push_link(
        &mut self,
        tree: TreeId,
        from: Socket,
        to: Socket,
        muted: bool,
    ) -> LinkId {
        let Some(host) = self.trees.get_mut(tree.0 as usize) else {
            return LinkId(u32::MAX);
        };
        let id = LinkId(host.next_link);
        host.next_link += 1;
        host.links.push(Link {
            id,
            from,
            to,
            muted,
        });
        id
    }

    /// A dependency path from `start` to `goal` inside one tree, following
    /// links forwards, or `None` when `goal` is unreachable.
    ///
    /// Reported by [`Self::connect`] so a refused wire says which existing
    /// wires would have closed the loop.
    #[must_use]
    pub fn path_between(&self, tree: TreeId, start: NodeId, goal: NodeId) -> Option<Vec<NodeId>> {
        let host = self.tree(tree)?;
        if start == goal {
            return Some(vec![start]);
        }
        let mut predecessor: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        let mut queue = std::collections::VecDeque::from([start]);
        let mut seen = std::collections::BTreeSet::from([start]);
        while let Some(current) = queue.pop_front() {
            for link in host.links.iter().filter(|l| l.from.node == current) {
                let next = link.to.node;
                if next == goal {
                    let mut path = vec![goal, current];
                    let mut cursor = current;
                    while let Some(&previous) = predecessor.get(&cursor) {
                        path.push(previous);
                        cursor = previous;
                    }
                    path.reverse();
                    return Some(path);
                }
                if seen.insert(next) {
                    predecessor.insert(next, current);
                    queue.push_back(next);
                }
            }
        }
        None
    }

    /// Append a port to a tree's interface.
    ///
    /// Appending is always safe: it cannot invalidate an existing port index,
    /// so no link has to be touched. Every instance of the tree gains the socket
    /// at once, because an instance's signature IS the interface.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`].
    pub fn expose(
        &mut self,
        tree: TreeId,
        side: InterfaceSide,
        port: KindPort<K>,
    ) -> Result<u32, EditError> {
        let host = self
            .trees
            .get_mut(tree.0 as usize)
            .ok_or(EditError::NoSuchTree(tree))?;
        let ports = host.interface.side_mut(side);
        ports.push(port);
        Ok(u32::try_from(ports.len() - 1).unwrap_or(u32::MAX))
    }

    /// Remove a port from a tree's interface, answering every link that had to
    /// go with it — inside the definition AND at every instance.
    ///
    /// Removal is the operation that *can* invalidate indices, so this is where
    /// the shifting lives: links on the removed port are dropped, and links on
    /// higher ports slide down by one. A caller doing this by hand would have to
    /// remember every instance in every tree.
    ///
    /// Each dropped link is answered **with the tree it was in** (R1584). A link
    /// id is unique within its tree and nowhere else, so a bare [`Link`] here
    /// named a link the caller could not find again — and these are exactly the
    /// links that come from *other* trees, which is the whole reason to report
    /// them.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchInterfacePort`].
    pub fn unexpose(
        &mut self,
        tree: TreeId,
        side: InterfaceSide,
        index: u32,
    ) -> Result<Vec<DroppedLink>, EditError> {
        let host = self
            .trees
            .get_mut(tree.0 as usize)
            .ok_or(EditError::NoSuchTree(tree))?;
        let ports = host.interface.side_mut(side);
        if index as usize >= ports.len() {
            return Err(EditError::NoSuchInterfacePort {
                tree,
                side,
                index,
                arity: u32::try_from(ports.len()).unwrap_or(u32::MAX),
            });
        }
        ports.remove(index as usize);

        // Inside the definition, the interface node carrying this side.
        let inside = host.interface_node(side).map(|n| n.id);
        let mut dropped = Vec::new();
        if let Some(inside) = inside {
            // An Input node's ports are OUTPUTS, an Output node's are INPUTS.
            let on_source = side == InterfaceSide::Input;
            dropped.extend(
                shift_port_links(&mut host.links, inside, on_source, index)
                    .into_iter()
                    .map(|link| DroppedLink { tree, link }),
            );
        }
        // At every instance, the mirror: the group node's inputs are the
        // interface inputs and its outputs are the interface outputs.
        let instance_on_source = side == InterfaceSide::Output;
        for other in &mut self.trees {
            let host_id = other.id;
            let instances: Vec<NodeId> = other
                .nodes
                .values()
                .filter(|n| n.body == NodeBody::Group(tree))
                .map(|n| n.id)
                .collect();
            for instance in instances {
                dropped.extend(
                    shift_port_links(&mut other.links, instance, instance_on_source, index)
                        .into_iter()
                        .map(|link| DroppedLink {
                            tree: host_id,
                            link,
                        }),
                );
            }
        }
        Ok(dropped)
    }
}

/// The integer centre of a set of node positions, or the origin when it is
/// empty.
///
/// A collapse recentres a definition around it and a cut records it as the
/// fragment's origin; both are "where was this selection", so there is one
/// answer.
pub(crate) fn centroid(points: impl Iterator<Item = (i32, i32)>) -> (i32, i32) {
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

/// Drop links on `node`'s port `index` and slide higher ports down one.
///
/// `on_source` selects which end of the link the node's ports are read from.
fn shift_port_links(links: &mut Vec<Link>, node: NodeId, on_source: bool, index: u32) -> Vec<Link> {
    let end = |link: &Link| if on_source { link.from } else { link.to };
    let dropped: Vec<Link> = links
        .iter()
        .filter(|l| end(l).node == node && end(l).port == index)
        .copied()
        .collect();
    links.retain(|l| !(end(l).node == node && end(l).port == index));
    for link in links.iter_mut() {
        let socket = if on_source {
            &mut link.from
        } else {
            &mut link.to
        };
        if socket.node == node && socket.port > index {
            socket.port -= 1;
        }
    }
    dropped
}

/// A tree's nodes travel as a **sequence**, not a map.
///
/// A [`Node`] already carries its own id, so a map on the wire would state it
/// twice and admit a document whose two copies disagree. Rebuilding the index on
/// the way in also means a hand-written or older document cannot arrive with a
/// key that does not match the node under it.
mod node_map {
    use super::{Node, NodeId, NodeKind};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub(super) fn serialize<K, S>(
        nodes: &BTreeMap<NodeId, Node<K>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        K: NodeKind + Serialize,
        S: Serializer,
    {
        nodes.values().collect::<Vec<_>>().serialize(serializer)
    }

    pub(super) fn deserialize<'de, K, D>(
        deserializer: D,
    ) -> Result<BTreeMap<NodeId, Node<K>>, D::Error>
    where
        K: NodeKind + Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Ok(Vec::<Node<K>>::deserialize(deserializer)?
            .into_iter()
            .map(|node| (node.id, node))
            .collect())
    }
}

/// A node's resolved ports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Serialize, K::Type: Serialize, K::Value: Serialize",
    deserialize = "K: Deserialize<'de>, K::Type: Deserialize<'de>, K::Value: Deserialize<'de>"
))]
pub struct Signature<K: NodeKind> {
    /// The ports values arrive at.
    pub inputs: Vec<KindPort<K>>,
    /// The ports values leave from.
    pub outputs: Vec<KindPort<K>>,
}

/// A link that an edit removed, and the tree it was removed from.
///
/// [`LinkId`] is unique within one tree, so a link reported without its tree is
/// a link the caller cannot address — cannot undo, cannot show, cannot even
/// look up. The links worth reporting are precisely the ones in trees the caller
/// was not editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroppedLink {
    /// The tree the link was in.
    pub tree: TreeId,
    /// The link itself, as it was.
    pub link: Link,
}

impl fmt::Display for DroppedLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tree {}: {} -> {}",
            self.tree.0, self.link.from, self.link.to
        )
    }
}

/// What a successful [`Document::connect`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Connected {
    /// The link created.
    pub link: LinkId,
    /// The link that was on the consuming socket and had to go, if there was
    /// one.
    pub displaced: Option<Link>,
}

/// Why a structural edit could not be performed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditError {
    /// No such tree in this document.
    NoSuchTree(TreeId),
    /// No such node in that tree.
    NoSuchNode {
        /// The tree that was searched.
        tree: TreeId,
        /// The node that is not in it.
        node: NodeId,
    },
    /// No such link in that tree.
    NoSuchLink {
        /// The tree that was searched.
        tree: TreeId,
        /// The link that is not in it.
        link: LinkId,
    },
    /// The interface has no port at that index on that side.
    NoSuchInterfacePort {
        /// The tree whose interface was addressed.
        tree: TreeId,
        /// Which half of the interface.
        side: InterfaceSide,
        /// The index asked for.
        index: u32,
        /// How many ports that side actually has.
        arity: u32,
    },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode { tree, node } => {
                write!(f, "tree {} has no node {}", tree.0, node.0)
            }
            Self::NoSuchLink { tree, link } => {
                write!(f, "tree {} has no link {}", tree.0, link.0)
            }
            Self::NoSuchInterfacePort {
                tree,
                side,
                index,
                arity,
            } => write!(
                f,
                "tree {}'s interface has {arity} {side:?} ports, so there is no port {index}",
                tree.0
            ),
        }
    }
}

impl std::error::Error for EditError {}

/// Why two sockets could not be linked.
///
/// Every arm names the sockets it is about. A wire that is refused without
/// saying which end was wrong leaves the user to guess, which is the whole
/// reason this is not a `bool`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectError<T> {
    /// One end names a node that is not in the tree.
    NoSuchNode(Socket),
    /// One end names a port the node does not have.
    NoSuchPort {
        /// The offending socket.
        socket: Socket,
        /// How many ports that end actually has.
        arity: u32,
    },
    /// The two ports carry different types.
    TypeMismatch {
        /// The producing socket.
        from: Socket,
        /// What it produces.
        from_type: T,
        /// The consuming socket.
        to: Socket,
        /// What it expects.
        to_type: T,
    },
    /// Both ends are on one node.
    SelfLink(NodeId),
    /// The link would close a dependency cycle; the existing path that would
    /// close it runs from the consuming node to the producing one.
    WouldCycle {
        /// That path, consumer end first.
        path: Vec<NodeId>,
    },
}

impl<T: fmt::Debug> fmt::Display for ConnectError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchNode(socket) => write!(f, "no such node for {socket}"),
            Self::NoSuchPort { socket, arity } => {
                write!(f, "{socket} names port {} of {arity}", socket.port)
            }
            Self::TypeMismatch {
                from,
                from_type,
                to,
                to_type,
            } => write!(f, "{from} carries {from_type:?}, {to} expects {to_type:?}"),
            Self::SelfLink(node) => write!(f, "node {} cannot feed itself", node.0),
            Self::WouldCycle { path } => {
                f.write_str("that link would close a cycle: ")?;
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

impl<T: fmt::Debug> std::error::Error for ConnectError<T> {}
