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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port<T, V> {
    /// Human-facing name. Not an identity — ports are addressed by index.
    pub name: String,
    /// The application's socket type. This crate only ever asks whether a
    /// value can cross from one to another ([`NodeKind::conversion`]).
    pub ty: T,
    /// The value this port carries when nothing else supplies one — the "pin
    /// default" of Blender and Blueprint.
    ///
    /// On an **input** that means no link. On an **output** it means the kind
    /// computed nothing there, which is what lets a source node declare its
    /// resting constant here (R1594) instead of the taxonomy carrying a payload
    /// it can never be asked to change. Either way the node's own
    /// [`Node::values`] takes precedence, because a port's *type* is the kind's
    /// and its *value* is the node's.
    ///
    /// Retained while the port is wired, because wiring hides an authored value
    /// rather than discarding it.
    pub default: Option<V>,
    /// Whether a value may pass through this port while its node is **bypassed**
    /// (R1587). `true` for an ordinary port.
    ///
    /// One declaration, read from both sides, because a pass-through has two
    /// ends: an INPUT that declares `false` is not a source for any output, and
    /// an OUTPUT that declares `false` receives nothing and is reported among
    /// [`Passthrough::dropped_outputs`](crate::Passthrough::dropped_outputs).
    ///
    /// Needed for exactly two shapes, and both were found by *measuring* how
    /// Blender uses its own equivalents rather than by guessing:
    ///
    /// * A **control** input that happens to share the data type it selects
    ///   between — `Switch(Switch: Bool, False: Bool, True: Bool) -> Bool`.
    ///   The identity rule would pass the *switch* through; declaring the
    ///   control port `no_passthrough` leaves the first data input, which is
    ///   what Blender's `node_geo_switch` hook returns.
    /// * An output whose value is only meaningful while the node computes — the
    ///   shape `node_geo_menu_switch` reaches by answering `nullptr` for every
    ///   output after its first.
    ///
    /// Blender spells the same declaration `no_mute_links` (set through a
    /// builder named `no_muted_links`) and uses it widely: **42 declarations
    /// across 17 node files at `8cf50599`, 28 on outputs and 14 on inputs** —
    /// both ends, which is why one field read from both ends is the right
    /// shape here too.
    ///
    /// It also has a *second* mechanism this crate does not need: eleven node
    /// types register a per-node C callback, `internally_linked_input`, and
    /// between them those callbacks compute only the identity (by name or by
    /// index) and "skip the leading control input". Blender needs a callback to
    /// reach the identity because its default is a static socket-type priority
    /// table; this crate's default *is* the identity, so the per-port
    /// declaration is the whole extension point.
    #[serde(default = "yes")]
    pub passthrough: bool,
}

/// `serde` needs a function to default a `bool` to `true`.
pub(crate) const fn yes() -> bool {
    true
}

impl<T, V> Port<T, V> {
    /// A port with a name, a type, and no default.
    pub fn new(name: impl Into<String>, ty: T) -> Self {
        Self {
            name: name.into(),
            ty,
            default: None,
            passthrough: true,
        }
    }

    /// The same port carrying a resting value.
    #[must_use]
    pub fn with_default(mut self, value: V) -> Self {
        self.default = Some(value);
        self
    }

    /// The same port, kept off the bypass path — see [`Self::passthrough`].
    ///
    /// On an input: never a source for a bypassed node's outputs. On an output:
    /// carries nothing while the node is bypassed.
    #[must_use]
    pub fn no_passthrough(mut self) -> Self {
        self.passthrough = false;
        self
    }
}

/// Whether and how a value crosses from an output into an input (R1593).
///
/// A node system's type relation is **directed**: a scalar feeds a colour by
/// broadcasting, and a colour never narrows back into a scalar. That is not
/// something equality can express — equality is symmetric — so it is a relation
/// the taxonomy declares, and it is declared once, as *the conversion itself*.
///
/// Legality and the conversion being one declaration is the point. Blender
/// keeps them apart, in three places that can disagree: `validate_link` (a
/// per-tree-type predicate that says whether a wire may exist),
/// `DataTypeConversions` (a global `Map<(from, to), ConversionFunctions>` that
/// holds the actual conversion), and `get_internal_link_type_priority` (a static
/// socket-type table used when a node is muted). Here there is one answer, so a
/// wire this crate accepts is a wire it can carry a value along, and a value
/// passing through a bypassed node converts by the same rule it would have
/// converted by along a link.
///
/// [`Conversion::Converted`] carries a plain `fn` pointer rather than a boxed
/// closure because a type-lattice conversion is a property of the pair of types
/// and captures nothing — the same reason Blender's own conversion table stores
/// `void (*convert_single_to_initialized)(const void *, void *)`.
pub enum Conversion<V> {
    /// No value of the source type may enter this port.
    Refused,
    /// The value arrives unchanged.
    Direct,
    /// The value arrives through this map.
    ///
    /// The map may still answer `None` — a conversion that is *declared* for a
    /// pair of types can fail on a particular value, and that is a different
    /// fact from the wire being illegal.
    Converted(fn(V) -> Option<V>),
}

// Derived by hand: `#[derive(Clone, Copy)]` would bound `V`, and a crossing is
// a decision about types that holds no value at all.
impl<V> Clone for Conversion<V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<V> Copy for Conversion<V> {}

impl<V> fmt::Debug for Conversion<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl<V> Conversion<V> {
    /// Whether no value may cross at all — the answer that refuses a wire.
    #[must_use]
    pub const fn is_refused(&self) -> bool {
        matches!(self, Self::Refused)
    }

    /// Whether a value crosses, changed or not.
    #[must_use]
    pub const fn is_allowed(&self) -> bool {
        !self.is_refused()
    }

    /// Whether a value that crosses is changed on the way.
    ///
    /// Published rather than hidden because it is a fact about the graph a
    /// reader wants: Blender makes it visible by materialising a whole
    /// `implicit_conversion` node.
    #[must_use]
    pub const fn converts(&self) -> bool {
        matches!(self, Self::Converted(_))
    }

    /// The wire-form token — `refused`, `direct` or `converted`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Refused => "refused",
            Self::Direct => "direct",
            Self::Converted(_) => "converted",
        }
    }

    /// Take `value` across, or `None` if it may not go or the conversion
    /// declined it.
    pub fn apply(&self, value: V) -> Option<V> {
        match self {
            Self::Refused => None,
            Self::Direct => Some(value),
            Self::Converted(map) => map(value),
        }
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
    /// The application's socket type.
    ///
    /// Equality is the *default* relation between two of them and not the only
    /// one available: see [`NodeKind::conversion`], which is what actually decides
    /// whether a value may go from one port to another.
    ///
    /// Before R1593 this doc said an implementor whose types coerce "models that
    /// by making the coercion part of equality". That was **false**, and the
    /// crate's own flagship consumer is the counter-example: a scalar assigns to
    /// a vector and a vector does not narrow to a scalar, so the relation is
    /// asymmetric, and no equality relation is. Making them equal would have
    /// admitted the narrowing too.
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

    /// Whether and how a value leaving an output of type `from` may enter an
    /// input of type `to` (R1593).
    ///
    /// An **associated** function and not a method, because a wire's legality is
    /// a property of the two types and of nothing else: an editor asks it while
    /// a wire is being dragged, before there is a value and often before there
    /// is a node at the far end. Blender hangs the same question off the *tree
    /// type* (`bNodeTreeType::validate_link`) for that reason.
    ///
    /// The default is the strictest relation there is — identical types cross
    /// unchanged and nothing else crosses at all — which is what this crate did
    /// before the question could be asked. A taxonomy with a coercion lattice
    /// overrides it; one without writes nothing.
    ///
    /// This one declaration decides four separate things, which is the whole
    /// reason it is one: whether [`Document::connect`] accepts a wire, what
    /// value arrives at the far end of one, which input a **bypassed** node
    /// routes to which output ([`Document::passthrough`]), and whether a
    /// document that arrived from a file still type-checks
    /// ([`Document::validate`]).
    ///
    /// Implementations must be **consistent**: `crossing(t, t)` should never be
    /// [`Conversion::Refused`], or a node's own value cannot reach a port of its
    /// own type.
    /// Which socket type `value` is one of, if the taxonomy classifies its
    /// values at all (R1594).
    ///
    /// The default is `None`: a taxonomy whose value type does not carry its own
    /// type — one flat `f64`, say — cannot answer, and is not asked to. What
    /// that costs is that [`Document::set_port_value`] then accepts any value on
    /// any port, exactly as [`Port::with_default`] always has.
    ///
    /// Blender does not need this because a socket's authored value is a
    /// *different C struct per socket type* (`bNodeSocketValueFloat` and its
    /// siblings), so a mismatch there is a type error at the call site. One
    /// `Value` type across the taxonomy is the price of this trait being
    /// generic, and this is how that price is paid back.
    fn value_type(value: &Self::Value) -> Option<Self::Type> {
        let _ = value;
        None
    }

    fn conversion(from: &Self::Type, to: &Self::Type) -> Conversion<Self::Value> {
        if from == to {
            Conversion::Direct
        } else {
            Conversion::Refused
        }
    }
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
/// The application's own kinds are the leaves; the three structural arms are
/// this crate's, and are what make a node able to *be* a graph and to *contain*
/// one.
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
    /// A node whose whole content is what it contains: the only body a
    /// [`Node::parent`] may name (R1589). Blender's `NODE_FRAME`.
    ///
    /// Owned by this crate rather than left to the taxonomy for the reason the
    /// other two structural arms are: a frame is an *editor* affordance and not
    /// application subject matter, so an application that supplied one would be
    /// re-deriving containment, and one that forgot would have no frames at all.
    ///
    /// Its signature is empty, so nothing can be linked to it and evaluation
    /// never reaches it — containment is a fact about the canvas, and this is
    /// the same separation [`Appearance`] draws. Blender's
    /// frame is an ordinary node type with sockets it happens not to declare.
    Frame,
}

/// Which side of a node's own signature a port sits on (R1594).
///
/// Deliberately **not** [`InterfaceSide`], which names a half of a *tree's*
/// interface. The two would read alike and mean different structures, and one
/// word for two structures is the confusion R1586 and R1593 each spent effort
/// undoing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Side {
    /// A port a value arrives at.
    Input,
    /// A port a value leaves by.
    Output,
}

/// One port of one node, named from inside that node (R1594).
///
/// A [`Socket`] names a port of a *named* node, which is what a link needs. This
/// names a port of the node you already have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PortRef {
    /// Which side of the signature.
    pub side: Side,
    /// The port's index on that side.
    pub index: u32,
}

impl PortRef {
    /// Input port `index`.
    #[must_use]
    pub const fn input(index: u32) -> Self {
        Self {
            side: Side::Input,
            index,
        }
    }

    /// Output port `index`.
    #[must_use]
    pub const fn output(index: u32) -> Self {
        Self {
            side: Side::Output,
            index,
        }
    }
}

impl fmt::Display for PortRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let side = match self.side {
            Side::Input => "in",
            Side::Output => "out",
        };
        write!(f, "{side}{}", self.index)
    }
}

/// A node: what it is, where it sits, what it is called, and what its own ports
/// have been given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Serialize, K::Value: Serialize",
    deserialize = "K: Deserialize<'de>, K::Value: Deserialize<'de>"
))]
pub struct Node<K: NodeKind> {
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
    /// The [`NodeBody::Frame`] this node sits inside, or `None` for the tree's
    /// own canvas (R1589).
    ///
    /// A **within-tree** reference, exactly like a [`Link`]'s sockets: a
    /// [`NodeId`] is unique in its tree and nowhere else, which is why every
    /// operation that moves a node between trees has to say what happens to it.
    ///
    /// Read on its own this is one edge; read across a tree it is a **forest**,
    /// and that is the invariant [`Document::set_parent`] maintains and
    /// [`Document::validate`] checks. Blender declares the same field as a bare
    /// `bNode *parent` and enforces its two rules — parent is a frame, and no
    /// node contains itself — with `BLI_assert`, which is compiled out of the
    /// build it ships.
    #[serde(default)]
    pub parent: Option<NodeId>,
    /// Values authored on **this node's** ports (R1594).
    ///
    /// A port's type and its name come from the kind, so every node of a kind
    /// shares them. Its *value* does not: two `Swatch` nodes are two different
    /// colours, and the number a user typed into an unwired input belongs to
    /// that input and to no other node's. Blender keeps exactly this, as
    /// `bNodeSocket::default_value`, per socket per node.
    ///
    /// The rule the evaluator applies is one sentence covering both sides: **an
    /// authored value is what the port carries when nothing else supplies one.**
    /// For an input that means no link; for an output it means the kind computed
    /// nothing there, which is what makes a source node's constant this same
    /// mechanism rather than a second one. Blender's Value node reaches its
    /// constant through per-node C code that reads its own output socket
    /// (`node_shader_value.cc`), so there the fact is a node type's private
    /// arrangement; here it is a rule.
    ///
    /// Sparse, because most ports have nothing authored, and a map because two
    /// values for one port is a state worth not having: a document that arrives
    /// from a peer with a repeated key keeps the last, the way any JSON object
    /// does.
    #[serde(default, with = "port_values")]
    pub values: BTreeMap<PortRef, K::Value>,
}

/// `serde` for [`Node::values`]: JSON has no map key but a string, and
/// [`PortRef`] is a struct, so the map travels as a sequence of pairs.
mod port_values {
    use std::collections::BTreeMap;

    use serde::de::{Deserialize, Deserializer};
    use serde::ser::{Serialize, Serializer};

    use super::PortRef;

    pub(super) fn serialize<V: Serialize, S: Serializer>(
        values: &BTreeMap<PortRef, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        values.iter().collect::<Vec<_>>().serialize(serializer)
    }

    pub(super) fn deserialize<'de, V: Deserialize<'de>, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<PortRef, V>, D::Error> {
        Ok(Vec::<(PortRef, V)>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}

impl<K: NodeKind> Node<K> {
    /// What has been authored on `port` of this node, if anything.
    #[must_use]
    pub fn port_value(&self, port: PortRef) -> Option<&K::Value> {
        self.values.get(&port)
    }
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
            NodeBody::Frame => "Frame".to_owned(),
        }
    }

    /// Whether this node is a [`NodeBody::Frame`] — the one body that may
    /// contain others.
    #[must_use]
    pub const fn is_frame(&self) -> bool {
        matches!(self.body, NodeBody::Frame)
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
    ///
    /// R1589 added `parent`, and the answer for it is **no**: a parent is a
    /// [`NodeId`] in the *source's* numbering, and every caller of this is
    /// minting fresh ids in another one, so copying the field would name
    /// whichever node happened to hold that id in the destination. Each caller
    /// remaps it through the map it already has — and the ones that carry a
    /// selection out of a tree must additionally decide what becomes of a parent
    /// the selection left behind.
    ///
    /// R1594 added `values`, and the answer for it is **yes**: a port reference
    /// is an index into this node's own signature, which the copy has too, so
    /// unlike `parent` it needs no remapping — and a duplicated `Swatch` that
    /// came out grey would be the defect the field exists to prevent.
    pub(crate) fn adopt_from(&mut self, source: &Self) {
        let Self {
            id: _,
            body: _,
            x: _,
            y: _,
            label,
            bypassed,
            appearance,
            parent: _,
            values,
        } = source;
        self.label.clone_from(label);
        self.bypassed = *bypassed;
        self.appearance.clone_from(appearance);
        self.values.clone_from(values);
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
            // A frame takes part in the canvas, never in the graph — so there is
            // nothing to link to it, and `connect` refuses either end of one
            // with `NoSuchPort` rather than needing an arm of its own.
            NodeBody::Frame => Signature {
                inputs: Vec::new(),
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
                parent: None,
                values: BTreeMap::new(),
            },
        );
        Ok(id)
    }

    /// Remove a node and every link touching it, answering what that cost.
    ///
    /// Reporting the links is what lets an undo stack put the node back whole;
    /// silently dropping incident links is how a "delete" becomes unrepeatable.
    /// The same argument covers what the node **contained**: deleting a frame is
    /// not deleting a pipeline stage, so its members survive — see
    /// [`Removed::adopted`] for where they land and why.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchNode`].
    pub fn remove_node(&mut self, tree: TreeId, node: NodeId) -> Result<Removed, EditError> {
        let host = self
            .trees
            .get_mut(tree.0 as usize)
            .ok_or(EditError::NoSuchTree(tree))?;
        if !host.nodes.contains_key(&node) {
            return Err(EditError::NoSuchNode { tree, node });
        }
        let adopted = self.adopt_orphans(tree, node);
        if let Some(host) = self.trees.get_mut(tree.0 as usize) {
            host.nodes.remove(&node);
        }
        Ok(Removed {
            links: self.unwire_node(tree, node),
            adopted,
        })
    }

    /// Hand `dying`'s direct members to `dying`'s own parent, answering them.
    ///
    /// The one place a node's disappearance is reconciled with the forest, so
    /// every path that removes a node — [`Self::remove_node`],
    /// [`Self::dissolve`](crate::Document::dissolve) and its detach twin —
    /// reaches the same answer.
    ///
    /// **The grandparent, not the canvas.** Blender's `node_unlink_attached`
    /// clears every child's parent outright, so deleting the middle frame of
    /// `Outer > Inner > node` moves `node` to the root even though `Outer` is
    /// still there and still contains where the node was. Only the containment
    /// the deletion actually destroyed is destroyed here.
    pub(crate) fn adopt_orphans(&mut self, tree: TreeId, dying: NodeId) -> Vec<NodeId> {
        let Some(host) = self.trees.get_mut(tree.0 as usize) else {
            return Vec::new();
        };
        let Some(grandparent) = host.nodes.get(&dying).map(|n| n.parent) else {
            return Vec::new();
        };
        let mut adopted = Vec::new();
        for member in host.nodes.values_mut() {
            if member.parent == Some(dying) {
                member.parent = grandparent;
                adopted.push(member.id);
            }
        }
        adopted
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

    /// What would happen to a value travelling from the output `from` to the
    /// input `to` (R1593).
    ///
    /// The question an editor asks while a wire is being *dragged*, and the one
    /// [`Self::connect`] answers with when it refuses. `None` when either socket
    /// is not there — which is a different answer from
    /// [`Conversion::Refused`], because "there is no
    /// such port" and "no value may go there" are different facts.
    ///
    /// Blender has no equivalent accessor: `validate_link` is a C function
    /// pointer on the tree type, so the only way to ask is to reach through
    /// `ntree.typeinfo` yourself, and whether the value would be *changed* on
    /// the way lives in a different table again.
    #[must_use]
    pub fn conversion(
        &self,
        tree: TreeId,
        from: Socket,
        to: Socket,
    ) -> Option<Conversion<K::Value>> {
        let source = self.signature(tree, from.node)?;
        let sink = self.signature(tree, to.node)?;
        let out = source.outputs.get(from.port as usize)?;
        let input = sink.inputs.get(to.port as usize)?;
        Some(K::conversion(&out.ty, &input.ty))
    }

    /// The crossing along an existing link, which is what its value went
    /// through (R1593).
    ///
    /// `None` when the link is not there or either of its ends has stopped
    /// existing — the state [`Violation::DanglingLink`](crate::Violation::DanglingLink)
    /// names.
    #[must_use]
    pub fn link_conversion(&self, tree: TreeId, link: LinkId) -> Option<Conversion<K::Value>> {
        let link = self.tree(tree)?.link(link)?;
        self.conversion(tree, link.from, link.to)
    }

    /// What has been authored on one of a node's ports (R1594).
    ///
    /// `None` when nothing has, when the node is not there, or when the tree is
    /// not — the three are told apart by asking about the node.
    #[must_use]
    pub fn port_value(&self, tree: TreeId, node: NodeId, port: PortRef) -> Option<&K::Value> {
        self.tree(tree)?.node(node)?.port_value(port)
    }

    /// Author a value on one of a node's ports, answering what it replaced.
    ///
    /// The value a port *carries* is not the same question: see
    /// [`Evaluator::inputs`](crate::Evaluator::inputs), which prefers a link and
    /// falls back to this and then to the kind's own [`Port::default`].
    ///
    /// The port must exist in the node's **signature**, so this refuses on a
    /// group instance whose definition has no such port exactly as it does on an
    /// application kind — Blender lets a socket's `default_value` be written
    /// through RNA with no such gate, and a stale index simply writes nowhere.
    ///
    /// # Errors
    ///
    /// [`PortValueError::NoSuchPort`] when the index is past the signature's
    /// arity, and [`PortValueError::WrongType`] when the taxonomy classifies its
    /// values ([`NodeKind::value_type`]) and this one is not the port's type. A
    /// taxonomy that does not classify accepts anything, which is what
    /// [`Port::with_default`] has always done.
    pub fn set_port_value(
        &mut self,
        tree: TreeId,
        node: NodeId,
        port: PortRef,
        value: K::Value,
    ) -> Result<Option<K::Value>, PortValueError<K::Type>> {
        let signature = self
            .signature(tree, node)
            .ok_or(PortValueError::NoSuchNode(node))?;
        let ports = match port.side {
            Side::Input => &signature.inputs,
            Side::Output => &signature.outputs,
        };
        let declared = ports
            .get(port.index as usize)
            .ok_or(PortValueError::NoSuchPort {
                port,
                arity: u32::try_from(ports.len()).unwrap_or(u32::MAX),
            })?;
        if let Some(found) = K::value_type(&value)
            && found != declared.ty
        {
            return Err(PortValueError::WrongType {
                port,
                expected: declared.ty.clone(),
                found,
            });
        }
        // The signature resolved, so both the tree and the node are there.
        let held = self
            .trees
            .get_mut(tree.0 as usize)
            .and_then(|host| host.nodes.get_mut(&node))
            .ok_or(PortValueError::NoSuchNode(node))?;
        Ok(held.values.insert(port, value))
    }

    /// Take an authored value back off a port, answering it.
    ///
    /// Distinct from writing the kind's own default over it: after this the port
    /// carries whatever the *kind* says, so a later change to the kind's default
    /// reaches this node again.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchNode`].
    pub fn clear_port_value(
        &mut self,
        tree: TreeId,
        node: NodeId,
        port: PortRef,
    ) -> Result<Option<K::Value>, EditError> {
        let host = self
            .trees
            .get_mut(tree.0 as usize)
            .ok_or(EditError::NoSuchTree(tree))?;
        let held = host
            .nodes
            .get_mut(&node)
            .ok_or(EditError::NoSuchNode { tree, node })?;
        Ok(held.values.remove(&port))
    }

    /// Link one socket to another.
    ///
    /// The four things that can be wrong are checked here rather than left to
    /// the caller, because every one of them is a property of the graph and not
    /// of the gesture: the sockets must exist, a value must be able to cross
    /// between their types, the consuming end must be an input (which takes at
    /// most one link), and the link must not close a cycle.
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
        // R1593 — the relation is directed and the taxonomy declares it, so a
        // scalar may broadcast into a colour while the colour never narrows
        // back. The default `crossing` is equality, which is what this was.
        if K::conversion(&source.ty, &sink.ty).is_refused() {
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
        K::Value: Serialize,
        S: Serializer,
    {
        nodes.values().collect::<Vec<_>>().serialize(serializer)
    }

    pub(super) fn deserialize<'de, K, D>(
        deserializer: D,
    ) -> Result<BTreeMap<NodeId, Node<K>>, D::Error>
    where
        K: NodeKind + Deserialize<'de>,
        K::Value: Deserialize<'de>,
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

/// What a successful [`Document::remove_node`] cost.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Removed {
    /// Every link that touched the node, as it was.
    pub links: Vec<Link>,
    /// The nodes the removed one contained, which its own parent has taken on —
    /// ascending, and empty for a node that contained nothing (R1589).
    ///
    /// Named rather than merely re-parented, because "deleting this frame moved
    /// six nodes" is the half of the edit that is not on screen where the
    /// gesture happened.
    pub adopted: Vec<NodeId>,
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

/// Why a value could not be authored on a port (R1594).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PortValueError<T> {
    /// No such node in that tree.
    NoSuchNode(NodeId),
    /// The node has no such port, and the arity says how far it goes.
    NoSuchPort {
        /// The port asked for.
        port: PortRef,
        /// How many ports that side actually has.
        arity: u32,
    },
    /// The taxonomy classifies its values and this one is not the port's type.
    WrongType {
        /// The port.
        port: PortRef,
        /// The type it declares.
        expected: T,
        /// The type the value turned out to be.
        found: T,
    },
}

impl<T: fmt::Debug> fmt::Display for PortValueError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchNode(node) => write!(f, "no node {}", node.0),
            Self::NoSuchPort { port, arity } => {
                write!(f, "no port {port}: that side has {arity}")
            }
            Self::WrongType {
                port,
                expected,
                found,
            } => write!(
                f,
                "port {port} takes {expected:?}, and that value is {found:?}"
            ),
        }
    }
}

impl<T: fmt::Debug> std::error::Error for PortValueError<T> {}

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
