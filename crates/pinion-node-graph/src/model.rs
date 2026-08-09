//! R1577 — the model: trees of typed nodes, and the structural edits that
//! maintain their own invariants.
//!
//! The application supplies the taxonomy by implementing [`NodeKind`]; this
//! module supplies everything a node system needs that is *not* taxonomy.

use serde::{Deserialize, Serialize};

use crate::appearance::Appearance;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A tree in the document: the root, or a re-usable group definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TreeId(pub u32);

/// A node, unique **within its tree** — the same numbering the DCC's node
/// names use, and what lets a group collapse move nodes between trees without
/// renumbering them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

/// A link, unique within its tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LinkId(pub u32);

// An id reaches a scene tag, a CSV on a wire and a sentence in a refusal, so it
// has a display form. Without one every consumer reaches through `.0`, which is
// the one thing the newtype exists to stop — and `Socket` and `PortRef` already
// have theirs, so this was an inconsistency rather than a decision (R1596).
macro_rules! displays_as_its_number {
    ($($id:ty),+ $(,)?) => {$(
        impl fmt::Display for $id {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    )+};
}
displays_as_its_number!(TreeId, NodeId, LinkId);

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

/// **Which occurrence** of a node a fact belongs to (R1600).
///
/// A [`NodeId`] is unique within its tree, and a definition's tree is shared by
/// every instance of it — so a node inside a group definition does not name one
/// place in a running document, it names one place *per instance*. This is the
/// missing half: the chain of group nodes descended through, outermost first,
/// empty at the tree the reading started in.
///
/// It is the key of everything a run keeps. The evaluator's memo has been keyed
/// by it since R1577 (two instances of one definition fed different values are
/// two different results), and R1600 makes it the address of a
/// [`Machine`](crate::Machine) register too — so two instances of a counting
/// group count separately, for the same reason and by the same key rather than
/// by a second convention.
///
/// # Against the references
///
/// **the DCC needs the same address and materialises it.** A geometry-nodes
/// simulation zone's state is cached per node in `ModifierCache` as `Map<int, std::unique_ptr<SimulationNodeCache>> simulation_cache_by_id`, and that `int` is a
/// *flattened* path: `bNestedNodeRef { int32_t id; bNestedNodePath path; }` where the path is `{ node_id, id_in_node }`, a side table stored on the
/// root tree and written into the .blend file. So the address exists there as
/// **persisted data that must be kept in step with the tree**, with a struct
/// field (`id_in_node`) its own comment describes as "Unused if the node is the final
/// nested node". Here it is derived by the walk that needs it and stored
/// nowhere.
///
/// **the engine does not have this address at all**, because a macro instance
/// is not an instance: `FKismetCompilerContext` expands one by calling `FEdGraphUtilities::CloneGraph(MacroGraph, ...)`, so N instances are N
/// copies of the nodes and each copy is simply its own node. That is also why
/// its recursion check has to exist (`FindMacroCycle`) — inlining cannot terminate on a
/// cycle — where a group *instance* here is checked by [`Document::containment`] being acyclic and
/// needs no expansion to run.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Instance(Vec<(TreeId, NodeId)>);

impl Instance {
    /// The tree the reading started in: no group descended into.
    #[must_use]
    pub const fn root() -> Self {
        Self(Vec::new())
    }

    /// The instance one level in: this one, then `node` of `tree`.
    ///
    /// `tree` is where the group *instance node* sits, not the definition it
    /// stands for — the definition is reachable from the node and the host is
    /// not, so naming the host is what makes a path resolvable in both
    /// directions.
    #[must_use]
    pub fn inside(&self, tree: TreeId, node: NodeId) -> Self {
        let mut deeper = self.0.clone();
        deeper.push((tree, node));
        Self(deeper)
    }

    /// The chain, outermost first.
    #[must_use]
    pub fn path(&self) -> &[(TreeId, NodeId)] {
        &self.0
    }

    /// How many group instances were descended through.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.0.len()
    }

    /// Whether this is the tree the reading started in.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for Instance {
    /// `/` at the root, else one `/<tree>:<node>` segment per descent.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("/");
        }
        for (tree, node) in &self.0 {
            write!(f, "/{tree}:{node}")?;
        }
        Ok(())
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
    /// The DCC spells this `NODE_LINK_MUTED` and spells a bypassed **node** `NODE_MUTED`, which are
    /// opposite behaviours under one word: a muted link stops a value, a muted
    /// node passes one through. They are named apart here — see [`Node::bypassed`].
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

/// What crosses a port: a value, or control (R1599).
///
/// A node graph has **two** kinds of edge and they obey opposite laws. This is
/// the one declaration that tells them apart, and it is an enum rather than a
/// flag beside the type because control is *not a value* — so a control port has
/// no type at all, and there is no slot left over to hold a meaningless one.
///
/// **the engine spells the same distinction as a string in the type slot.** An
/// execution pin there is an ordinary `UEdGraphPin` whose
/// `PinType.PinCategory` happens to equal the `FName` `"exec"`
/// (`UEdGraphSchema_K2::PC_Exec`), so "is this pin control?" is a string
/// comparison — written out **40 times** across `Editor` and `Runtime` at
/// 5.8.1, beside the 70 uses of the `IsExecPin` helper that exists for it. A
/// site that forgets is not a compile error there; here every read of a port's
/// type has to say what it does about control, which is why this round changed
/// the field's type instead of adding one next to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Flow<T, V> {
    /// A value of the application's socket type. This crate only ever asks
    /// whether one can cross into another ([`NodeKind::conversion`]).
    Value {
        /// The application's socket type.
        ty: T,
        /// The value this port carries when nothing else supplies one — the
        /// "pin default" of the DCC and Blueprint.
        ///
        /// On an **input** that means no link. On an **output** it means the
        /// kind computed nothing there, which is what lets a source node
        /// declare its resting constant here (R1594) instead of the taxonomy
        /// carrying a payload it can never be asked to change. Either way the
        /// node's own [`Node::values`] takes precedence, because a port's
        /// *type* is the kind's and its *value* is the node's.
        ///
        /// Retained while the port is wired, because wiring hides an authored
        /// value rather than discarding it.
        default: Option<V>,
    },
    /// Control: the edge says *when*, never *what*.
    ///
    /// No type and no resting value, and both absences are structural — there
    /// is no field here to put one in.
    Control,
}

impl<T, V> Flow<T, V> {
    /// The socket type, or `None` for a control port.
    pub const fn value_type(&self) -> Option<&T> {
        match self {
            Self::Value { ty, .. } => Some(ty),
            Self::Control => None,
        }
    }

    /// The resting value, or `None` for a control port or a port without one.
    pub const fn default_value(&self) -> Option<&V> {
        match self {
            Self::Value { default, .. } => default.as_ref(),
            Self::Control => None,
        }
    }

    /// Whether this port carries control.
    pub const fn is_control(&self) -> bool {
        matches!(self, Self::Control)
    }

    /// How many links a port carrying this may hold on `side` — the whole
    /// reason the two flows are one declaration.
    ///
    /// |             | input      | output     |
    /// |-------------|------------|------------|
    /// | **Value**   | at most 1  | unbounded  |
    /// | **Control** | unbounded  | at most 1  |
    ///
    /// The table is the **duality**, not two conventions that happen to
    /// differ. A value has one producer and many readers: asking where a value
    /// came from must have exactly one answer, while any number of consumers
    /// may read it. A control transfer has one successor and many predecessors:
    /// after *this* runs, exactly one thing runs next, while any number of
    /// paths may converge on the same next thing. That is `def`/`use` against
    /// `terminator`/`predecessors` — the same duality SSA draws, and the reason
    /// a control-flow graph has join points where a dataflow graph has fan-out.
    ///
    /// The engine derives the same two rules and writes them as two
    /// independent booleans one line apart, each naming the *other* flow to
    /// exclude it (`EdGraphSchema_K2.cpp`, 5.8.1):
    ///
    /// ```text
    /// bBreakExistingDueToExecOutput = IsExecPin(*OutputPin) && OutputPin->LinkedTo.Num() > 0;
    /// bBreakExistingDueToDataInput  = !IsExecPin(*InputPin) && InputPin->LinkedTo.Num() > 0;
    /// ```
    pub const fn multiplicity(&self, side: Side) -> Multiplicity {
        match (self, side) {
            (Self::Value { .. }, Side::Input) | (Self::Control, Side::Output) => Multiplicity::One,
            (Self::Value { .. }, Side::Output) | (Self::Control, Side::Input) => Multiplicity::Many,
        }
    }
}

/// How many links one port may hold (R1599).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Multiplicity {
    /// At most one. A second link **displaces** the first, and the displaced
    /// one is reported so the replacement is undoable.
    One,
    /// Any number.
    Many,
}

/// One socket in a node's signature.
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Port<T, V> {
    /// Human-facing name. Not an identity — ports are addressed by index.
    pub name: String,
    /// What crosses this port: a value of the application's socket type, or
    /// **control** (R1599).
    ///
    /// One field and not two, because a port's flow and a port's type are not
    /// independent facts that could disagree — control is not a value, so a
    /// control port has no type, and that is what the enum says.
    pub flow: Flow<T, V>,
    /// Whether a value may pass through this port while its node is **bypassed**
    /// (R1587). `true` for an ordinary port.
    ///
    /// One declaration, read from both sides, because a pass-through has two
    /// ends: an INPUT that declares `false` is not a source for any output, and
    /// an OUTPUT that declares `false` receives nothing and is reported among
    /// [`Passthrough::dropped_outputs`](crate::Passthrough::dropped_outputs).
    ///
    /// Needed for exactly two shapes, and both were found by *measuring* how
    /// the DCC uses its own equivalents rather than by guessing:
    ///
    /// * A **control** input that happens to share the data type it selects
    ///   between — `Switch(Switch: Bool, False: Bool, True: Bool) -> Bool`.
    ///   The identity rule would pass the *switch* through; declaring the
    ///   control port `no_passthrough` leaves the first data input, which is
    ///   what the DCC's `node_geo_switch` hook returns.
    /// * An output whose value is only meaningful while the node computes — the
    ///   shape `node_geo_menu_switch` reaches by answering `nullptr` for every
    ///   output after its first.
    ///
    /// The DCC spells the same declaration `no_mute_links` (set through a
    /// builder named `no_muted_links`) and uses it widely: **42 declarations
    /// across 17 node files at `8cf50599`, 28 on outputs and 14 on inputs** —
    /// both ends, which is why one field read from both ends is the right
    /// shape here too.
    ///
    /// It also has a *second* mechanism this crate does not need: eleven node
    /// types register a per-node C callback, `internally_linked_input`, and between them those
    /// callbacks compute only the identity (by name or by index) and "skip the
    /// leading control input". The DCC needs a callback to reach the identity
    /// because its default is a static socket-type priority table; this
    /// crate's default *is* the identity, so the per-port declaration is the
    /// whole extension point.
    #[serde(default = "yes")]
    pub passthrough: bool,
}

/// `serde` needs a function to default a `bool` to `true`.
pub(crate) const fn yes() -> bool {
    true
}

impl<T, V> Port<T, V> {
    /// A **value** port with a name, a type, and no default.
    pub fn new(name: impl Into<String>, ty: T) -> Self {
        Self {
            name: name.into(),
            flow: Flow::Value { ty, default: None },
            passthrough: true,
        }
    }

    /// A **control** port with a name (R1599).
    ///
    /// No type and no default, because control is not a value. The name is
    /// still the port's own — the engine names them too (`Then`, `Else`, `Loop
    /// Body`, `Completed`), and a control port that could not be named would
    /// leave a two-way branch with two indistinguishable arms.
    pub fn control(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            flow: Flow::Control,
            passthrough: true,
        }
    }

    /// The socket type, or `None` for a control port.
    pub const fn value_type(&self) -> Option<&T> {
        self.flow.value_type()
    }

    /// The resting value, or `None` for a control port or a port without one.
    pub const fn default_value(&self) -> Option<&V> {
        self.flow.default_value()
    }

    /// Whether this port carries control.
    pub const fn is_control(&self) -> bool {
        self.flow.is_control()
    }

    /// How many links this port may hold on `side` — see
    /// [`Flow::multiplicity`].
    pub const fn multiplicity(&self, side: Side) -> Multiplicity {
        self.flow.multiplicity(side)
    }

    /// The same port carrying a resting value.
    ///
    /// **A control port has no resting value and this does nothing to one.**
    /// That is not a silent swallow but the only honest arm: control is not a
    /// value, so there is no field in [`Flow::Control`] to write, and a
    /// panic would turn an authoring slip into a crash in a crate that refuses
    /// everything else by value. The stored model therefore cannot hold a
    /// control port with a default at all — which is the guarantee, and it is
    /// structural rather than documented.
    #[must_use]
    pub fn with_default(mut self, value: V) -> Self {
        if let Flow::Value { default, .. } = &mut self.flow {
            *default = Some(value);
        }
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
/// Legality and the conversion being one declaration is the point. The DCC
/// keeps them apart, in three places that can disagree: `validate_link` (a per-tree-type
/// predicate that says whether a wire may exist), `DataTypeConversions` (a global `Map<(from, to), ConversionFunctions>` that holds
/// the actual conversion), and `get_internal_link_type_priority` (a static socket-type table used when a
/// node is muted). Here there is one answer, so a wire this crate accepts is a
/// wire it can carry a value along, and a value passing through a bypassed
/// node converts by the same rule it would have converted by along a link.
///
/// [`Conversion::Converted`] carries a plain `fn` pointer rather than a boxed closure because a
/// type-lattice conversion is a property of the pair of types and captures
/// nothing — the same reason the DCC's own conversion table stores `void (*convert_single_to_initialized)(const void *, void *)`.
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
    /// reader wants: the DCC makes it visible by materialising a whole
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
    /// An **associated** function and not a method, because a wire's legality
    /// is a property of the two types and of nothing else: an editor asks it
    /// while a wire is being dragged, before there is a value and often before
    /// there is a node at the far end. The DCC hangs the same question off the
    /// *tree type* (`bNodeTreeType::validate_link`) for that reason.
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
    /// The DCC does not need this because a socket's authored value is a
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

    /// Where this kind hands control on, given its resolved inputs (R1599).
    ///
    /// Asked only of a node that has control outputs, and answered in terms of
    /// their port indices. A node with none is a **pure** node: it is never in
    /// an execution trace at all, it is pulled when someone reads its value,
    /// and this is not asked of it.
    ///
    /// # The default is a behaviour, not a silence
    ///
    /// [`Control::FallThrough`] hands control to *every* control output, in
    /// port order. R1594's audit recorded that a third provided-default
    /// extension point on this trait "would be worth resisting", and the reason
    /// this one earns its place is that its default is not "say nothing" like
    /// [`Self::conversion`]'s and [`Self::value_type`]'s — it is the canonical
    /// answer, and it is the answer for almost every node there is:
    ///
    /// * a node with **one** control output means "then continue", and writes
    ///   nothing;
    /// * a node with **several** is a *sequence* — run the first to completion,
    ///   then the next — and writes nothing either.
    ///
    /// That second one is worth stating against the reference. The engine's
    /// `Sequence` node is a whole `UK2Node_ExecutionSequence` class plus an `FKCHandler_ExecutionSequence` compile handler, which finds
    /// its own output pins by testing whether each pin's name *starts with the
    /// string* `"Then"` and carries the standing admission `//@TODO: Sort the pins by the number appended to the pin!` — so there, the
    /// order control leaves a Sequence by is the order its pins happen to sit
    /// in the array, and the node's own author noted that this is not the
    /// order the user is reading off the screen. Here the order **is** the
    /// port order, because that is the only order a signature has.
    ///
    /// Only a *branch* overrides: it picks among its outputs by looking at what
    /// arrived.
    fn control(&self, inputs: &[Option<Self::Value>]) -> Control {
        let _ = inputs;
        Control::FallThrough
    }
}

/// Where a node hands control on (R1599).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Control {
    /// Every control output, in port order — see [`NodeKind::control`].
    FallThrough,
    /// Exactly these control outputs, in this order.
    ///
    /// Empty means control stops here, which is what an exit node answers.
    /// Indices that are not control outputs of this node are **named** in
    /// [`Step::ignored`](crate::Step::ignored) rather than dropped, because a
    /// taxonomy whose branch quietly does nothing is the hardest kind of bug to
    /// see in a graph.
    Take(Vec<u32>),
}

impl Control {
    /// Hand control to exactly one output.
    #[must_use]
    pub fn to(port: u32) -> Self {
        Self::Take(vec![port])
    }

    /// Stop here: hand control to nothing.
    #[must_use]
    pub const fn halt() -> Self {
        Self::Take(Vec::new())
    }
}

/// A [`Port`] specialised to one taxonomy.
pub type KindPort<K> = Port<<K as NodeKind>::Type, <K as NodeKind>::Value>;

/// Whether and how what leaves `from` may enter `to` (R1599).
///
/// **The one question every derivation in this crate asks about a pair of
/// ports.** Before this round the question was `NodeKind::conversion` over two
/// socket types, which could not be asked at all once a port might carry
/// control instead of a value — so the flow check and the type check are the
/// same call, for the same reason R1593 made legality and conversion one
/// declaration: two checks that must agree are one check written twice.
///
/// * control into control is [`Conversion::Direct`] — control is not a value,
///   so there is nothing to convert and nothing to ask the taxonomy;
/// * value into value defers to [`NodeKind::conversion`], unchanged;
/// * **the two never mix**, which is what stops an execution wire from feeding
///   a number.
///
/// The engine reaches the last of those three through the type system it
/// shares with data: an exec pin's `PinCategory` is the `FName` `"exec"`, so `ArePinsCompatible` refuses
/// exec-to-float the same way it refuses float-to-object — by comparing
/// category strings. It works, and it is why `PC_Exec` has to be excluded by name
/// from promotion, from default values, from the type-tree the editor offers,
/// and from the dependency sort, each in its own place.
pub fn crossing<K: NodeKind>(from: &KindPort<K>, to: &KindPort<K>) -> Conversion<K::Value> {
    match (&from.flow, &to.flow) {
        (Flow::Control, Flow::Control) => Conversion::Direct,
        (Flow::Value { ty: out, .. }, Flow::Value { ty: into, .. }) => K::conversion(out, into),
        (Flow::Control, Flow::Value { .. }) | (Flow::Value { .. }, Flow::Control) => {
            Conversion::Refused
        }
    }
}

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
#[serde(bound(
    serialize = "K: Serialize, K::Type: Serialize",
    deserialize = "K: Deserialize<'de>, K::Type: Deserialize<'de>"
))]
pub enum NodeBody<K: NodeKind> {
    /// An application node.
    Kind(K),
    /// An instance of a group definition. Its signature is that tree's
    /// interface — not a copy of it, so editing the definition re-signatures
    /// every instance at once.
    Group(TreeId),
    /// The inside end of this tree's own interface.
    Interface(InterfaceSide),
    /// A node whose whole content is what it contains: the only body a
    /// [`Node::parent`] may name (R1589). The DCC's `NODE_FRAME`.
    ///
    /// Owned by this crate rather than left to the taxonomy for the reason the
    /// other two structural arms are: a frame is an *editor* affordance and not
    /// application subject matter, so an application that supplied one would be
    /// re-deriving containment, and one that forgot would have no frames at all.
    ///
    /// Its signature is empty, so nothing can be linked to it and evaluation
    /// never reaches it — containment is a fact about the canvas, and this is
    /// the same separation [`Appearance`] draws. The DCC's
    /// frame is an ordinary node type with sockets it happens not to declare.
    Frame,
    /// **A value one step behind**: this node's output is what arrived at its
    /// input at the previous [`Document::tick`], and its resting value until
    /// then (R1600).
    ///
    /// Lustre's `pre`, Simulink's Unit Delay, SSA's φ at a loop header, a
    /// hardware register. R1599 made a control **loop** authorable and left the
    /// reason it could not yet mean anything: [`NodeKind::evaluate`] takes
    /// `&self` and returns values, so a node that ran twice computed the same
    /// thing twice. This is the one node whose output is not a function of its
    /// input *now*, and therefore:
    ///
    /// * it is the **only** node a value cycle may pass through
    ///   ([`Document::connect`] accepts the closing wire exactly when the cycle
    ///   it would close has one of these on it, which is Lustre's causality
    ///   rule), and
    /// * it is where a run keeps its state, addressed by
    ///   [`Instance`] so two instances of one definition do not share a
    ///   register.
    ///
    /// # Why this crate owns it
    ///
    /// The same argument as [`Self::Frame`] and [`Self::Group`], plus a
    /// structural one an application cannot argue with: there is nowhere in
    /// [`NodeKind`] to put the register. `evaluate(&self, inputs)` is a pure
    /// function by design — that purity is what makes
    /// [`Document::evaluate`] safe to call after an arbitrary edit — so a
    /// taxonomy trying to supply a delay would have to reach outside itself,
    /// and one that forgot would have no loops at all.
    ///
    /// It carries the socket **type** it holds, because it holds a value and a
    /// value here is typed; it carries no *initial* value, because that is a
    /// per-node authored value ([`Node::values`], R1594) on its output port —
    /// which is Lustre's `->` and needed no new mechanism.
    ///
    /// # Against the references
    ///
    /// **the DCC cannot express this and unrolls instead.** Its Repeat Zone
    /// builds the body graph once per iteration —
    /// `geometry_nodes_repeat_zone.cc`, "the graph is built with as many body
    /// copies as there are iterations. Since this graph depends on the number
    /// of iterations, it can't be reused in general" — so the count must be
    /// known before evaluation and a data-dependent exit is inexpressible.
    /// Carrying a value across *frames* is a different mechanism again (the
    /// Simulation Zone), whose state does not live in the node tree at all but
    /// in the modifier's bake cache.
    ///
    /// **the engine has no unit delay.** State there is a Blueprint
    /// *variable*: a `UK2Node_VariableSet` writing a property on the object, read back by a `UK2Node_VariableGet`
    /// — arbitrary mutable state, so which value a read sees depends on where
    /// the execution wire happens to have gone, and the graph's meaning is not
    /// a function of the graph. (Its `UK2Node_Delay` is a *latent time* delay, not this.)
    /// The tradeoff is deliberate here: the only state is a delay, so a tick's
    /// result is a function of the registers and the inputs, and that is what
    /// makes [`Document::tick`] reproducible.
    Delay(K::Type),
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
    serialize = "K: Serialize, K::Type: Serialize, K::Value: Serialize",
    deserialize = "K: Deserialize<'de>, K::Type: Deserialize<'de>, K::Value: Deserialize<'de>"
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
    /// The DCC spells this `NODE_MUTED` and keeps it in the same `flag` integer as `NODE_COLLAPSED`,
    /// `NODE_PREVIEW` and even `NODE_SELECT`, so nothing in its model says which of those bits its
    /// evaluator may read.
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
    /// Read on its own this is one edge; read across a tree it is a
    /// **forest**, and that is the invariant [`Document::set_parent`] maintains and [`Document::validate`] checks.
    /// The DCC declares the same field as a bare `bNode *parent` and enforces its two
    /// rules — parent is a frame, and no node contains itself — with `BLI_assert`,
    /// which is compiled out of the build it ships.
    #[serde(default)]
    pub parent: Option<NodeId>,
    /// Values authored on **this node's** ports (R1594).
    ///
    /// A port's type and its name come from the kind, so every node of a kind
    /// shares them. Its *value* does not: two `Swatch` nodes are two different
    /// colours, and the number a user typed into an unwired input belongs to
    /// that input and to no other node's. The DCC keeps exactly this, as
    /// `bNodeSocket::default_value`, per socket per node.
    ///
    /// The rule the evaluator applies is one sentence covering both sides:
    /// **an authored value is what the port carries when nothing else supplies
    /// one.** For an input that means no link; for an output it means the kind
    /// computed nothing there, which is what makes a source node's constant
    /// this same mechanism rather than a second one. The DCC's Value node
    /// reaches its constant through per-node C code that reads its own output
    /// socket (`node_shader_value.cc`), so there the fact is a node type's private arrangement;
    /// here it is a rule.
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
            NodeBody::Delay(_) => "Delay".to_owned(),
        }
    }

    /// Whether this node is a [`NodeBody::Frame`] — the one body that may
    /// contain others.
    #[must_use]
    pub const fn is_frame(&self) -> bool {
        matches!(self.body, NodeBody::Frame)
    }

    /// Whether this node is a [`NodeBody::Delay`] — the one body that holds a
    /// value between ticks (R1600).
    #[must_use]
    pub const fn is_delay(&self) -> bool {
        matches!(self.body, NodeBody::Delay(_))
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

    /// Take on `other`'s id frontier, so nothing this document mints from now on
    /// can collide with an id `other` had already handed out (R1597).
    ///
    /// **What it is for is an UNDO that restores a whole document.** Snapshot
    /// undo is the honest shape for a node editor — the DCC's `node_undosys` copies the
    /// tree per step, and a delta has to enumerate every *kind* of thing an
    /// edit can touch — but restoring a value restores its mint counters with
    /// it, so the next `add_node` after an undo would re-issue an id the undone state
    /// had already used. For an in-process model that is harmless; for a
    /// surface where an agent, a saved selection or a scene tag addresses a
    /// node BY id it is not, because the id would silently name a different
    /// node.
    ///
    /// So a stack that restores a document calls this with the state it is
    /// leaving. Monotonic per tree, and never lowers anything: a document that
    /// is already ahead is unchanged. Trees `other` does not have are left
    /// alone, and vice versa — this moves counters, never structure.
    pub fn advance_ids_from(&mut self, other: &Self) {
        for (tree, source) in self.trees.iter_mut().zip(other.trees.iter()) {
            tree.next_node = tree.next_node.max(source.next_node);
            tree.next_link = tree.next_link.max(source.next_link);
        }
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
    /// two definitions may share one. The DCC must rename a copied node group
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
            // R1600 — one in, one out, both of the type it holds. Derived here
            // rather than authored, so a delay cannot be built that narrows or
            // widens what passes through it: what comes back out a tick later
            // is the thing that went in.
            NodeBody::Delay(ty) => Signature {
                inputs: vec![Port::new("In", ty.clone())],
                outputs: vec![Port::new("Out", ty.clone())],
            },
        })
    }

    /// Every [`NodeBody::Delay`] in `tree`, ascending (R1600).
    ///
    /// The registers of one tree. Which *instance* each belongs to is the
    /// caller's descent — see [`Machine`](crate::Machine).
    #[must_use]
    pub fn delays(&self, tree: TreeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut found: Vec<NodeId> = host
            .nodes()
            .filter(|node| matches!(node.body, NodeBody::Delay(_)))
            .map(|node| node.id)
            .collect();
        found.sort_unstable();
        found
    }

    /// Whether `node` **cuts** the dependency graph: its output this step is
    /// not a function of its input this step (R1600).
    ///
    /// True for a [`NodeBody::Delay`] that is computing. A *bypassed* delay is
    /// not one — bypassing is the request to take the node's behaviour out, and
    /// a delay's whole behaviour is the cut, so a bypassed one is a plain wire
    /// and the cycle it was holding open is live again. That is why
    /// [`Self::set_bypassed`] refuses to bypass a delay a cycle runs through.
    fn cuts_dependency(&self, tree: TreeId, node: NodeId) -> bool {
        self.tree(tree)
            .and_then(|host| host.node(node))
            .is_some_and(|held| matches!(held.body, NodeBody::Delay(_)) && !held.bypassed)
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
    /// **The grandparent, not the canvas.** the DCC's `node_unlink_attached`
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
    /// Move `node`'s links onto the ports `moved` names, answering the ones
    /// that had nowhere to go (R1598).
    ///
    /// Lives here because this is the module that mutates a tree's link list,
    /// and keeping that in one place is what makes "no link can be half-moved"
    /// a property of the code rather than a convention. The correspondence
    /// itself is [`swap`](crate::swap)'s.
    pub(crate) fn remap_node_ports(
        &mut self,
        tree: TreeId,
        node: NodeId,
        moved: &BTreeMap<PortRef, PortRef>,
    ) -> Vec<Link> {
        let Some(host) = self.trees.get_mut(tree.0 as usize) else {
            return Vec::new();
        };
        let mut severed = Vec::new();
        let mut kept = Vec::with_capacity(host.links.len());
        for mut link in std::mem::take(&mut host.links) {
            let ends = [
                (link.from.node == node).then_some((Side::Output, link.from.port)),
                (link.to.node == node).then_some((Side::Input, link.to.port)),
            ];
            let mut survives = true;
            for (side, index) in ends.into_iter().flatten() {
                match moved.get(&PortRef { side, index }) {
                    Some(to) => match side {
                        Side::Input => link.to.port = to.index,
                        Side::Output => link.from.port = to.index,
                    },
                    None => survives = false,
                }
            }
            if survives {
                kept.push(link);
            } else {
                severed.push(link);
            }
        }
        host.links = kept;
        severed
    }

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
    /// The DCC has no equivalent accessor: `validate_link` is a C function
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
        Some(crossing::<K>(out, input))
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
    /// falls back to this and then to the kind's own [`Port::default_value`].
    ///
    /// The port must exist in the node's **signature**, so this refuses on a
    /// group instance whose definition has no such port exactly as it does on
    /// an application kind — the DCC lets a socket's `default_value` be written through
    /// RNA with no such gate, and a stale index simply writes nowhere.
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
        // R1599 — control is not a value, so a control port has nowhere to put
        // one. Checked before the taxonomy is asked anything, because this arm
        // holds even for a taxonomy that declines to classify its values.
        let Some(declared_ty) = declared.value_type() else {
            return Err(PortValueError::NotAValuePort { port });
        };
        if let Some(found) = K::value_type(&value)
            && found != *declared_ty
        {
            return Err(PortValueError::WrongType {
                port,
                expected: declared_ty.clone(),
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
    /// undoable. The DCC performs the same replacement and returns nothing.
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
        // `crossing` is the ONE authority on whether the pair may be wired —
        // R1593's directed type relation, widened by R1599 to cover the flow —
        // and the match below only chooses which refusal to *name*. Stating the
        // mixed-flow rule here as well would be a second copy free to disagree
        // with it, which is what a counterfactual on `crossing`'s mixed arm
        // found: the refusal held with that arm removed, because this site was
        // deciding it independently.
        if crossing::<K>(source, sink).is_refused() {
            return Err(match (source.value_type(), sink.value_type()) {
                (Some(out), Some(into)) => ConnectError::TypeMismatch {
                    from,
                    from_type: out.clone(),
                    to,
                    to_type: into.clone(),
                },
                // Exactly one end carries control: two control ports cross
                // directly and are never refused, so that pair cannot arrive
                // here — asserted by `r1599_control_to_control_is_the_one_pair_
                // with_no_type_question`.
                (source_type, _) => ConnectError::FlowMismatch {
                    from,
                    to,
                    control_end: if source_type.is_none() {
                        Side::Output
                    } else {
                        Side::Input
                    },
                },
            });
        }
        if from.node == to.node {
            return Err(ConnectError::SelfLink(from.node));
        }
        // R1599 — **only a value link may not close a cycle.** A cycle through
        // control links is not a contradiction, it is a LOOP: the thing every
        // real execution graph is built to express. So the acyclicity check
        // walks the data plane alone, and a control cycle is legal here and
        // reported by `Document::control_loops` rather than refused.
        //
        // The engine reaches the same split and states it in a comment on the
        // predicate that implements it — `FKismetCompilerContext::
        // PinIsImportantForDependancies` returns `PinCategory != PC_Exec`,
        // "the execution wires do not form data dependencies, they are only
        // important for final scheduling and that is handled thru gotos". What
        // it does NOT do is notice a control cycle: an execution loop with no
        // exit compiles, and is caught by counting iterations at run time
        // (`EBlueprintExceptionType::InfiniteLoop`).
        //
        // R1600 — and a value link leaving a DELAY adds no dependency at all,
        // so it can no more close a cycle than a control link can: what leaves
        // a delay is what arrived a tick ago. That is the causality rule
        // Lustre states as "every cycle must be broken by a `pre`", and it is
        // asked here as one predicate rather than re-derived, so the wire this
        // accepts is exactly the wire `cycle_nodes` will not report.
        let adds_a_dependency =
            source.value_type().is_some() && !self.cuts_dependency(tree, from.node);
        if adds_a_dependency && let Some(path) = self.data_path_between(tree, to.node, from.node) {
            return Err(ConnectError::WouldCycle { path });
        }

        // R1599 — which end has to give way is the port's own limit, and the
        // two flows put it on opposite ends: a value INPUT takes one producer,
        // a control OUTPUT takes one successor.
        let crowded = if sink.multiplicity(Side::Input) == Multiplicity::One {
            Some(Side::Input)
        } else if source.multiplicity(Side::Output) == Multiplicity::One {
            Some(Side::Output)
        } else {
            None
        };

        // The tree exists: `signature` above resolved through it twice.
        let Some(host) = self.trees.get_mut(tree.0 as usize) else {
            return Err(ConnectError::NoSuchNode(from));
        };
        let displaced = match crowded {
            Some(Side::Input) => host.links.iter().find(|l| l.to == to).copied(),
            Some(Side::Output) => host.links.iter().find(|l| l.from == from).copied(),
            None => None,
        };
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
    /// it. The DCC's `NODE_OT_mute_toggle`.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchNode`], and
    /// [`EditError::BypassWouldCycle`] for the one node whose behaviour *is*
    /// the acyclicity of the data plane (R1600).
    pub fn set_bypassed(
        &mut self,
        tree: TreeId,
        node: NodeId,
        bypassed: bool,
    ) -> Result<bool, EditError> {
        // R1600 — bypassing a delay makes it a plain wire, so the cycle it was
        // holding open becomes live. Refused with the cycle named, for the same
        // reason `connect` refuses the wire that would close one: a document
        // this crate built never has a value cycle in it, and an edit that
        // would create one by *removing* a cut is the same edit.
        if bypassed && self.cuts_dependency(tree, node) {
            let feeds = self
                .tree(tree)
                .into_iter()
                .flat_map(Tree::links)
                .filter(|link| link.from.node == node)
                .map(|link| link.to.node)
                .collect::<Vec<_>>();
            if let Some(path) = feeds
                .into_iter()
                .find_map(|downstream| self.data_path_between(tree, downstream, node))
            {
                return Err(EditError::BypassWouldCycle { tree, node, path });
            }
        }
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
    /// the port it feeds falls back to its own default. The DCC's
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

    /// Whether a link carries control, decided by the port it leaves (R1599).
    ///
    /// One end is enough: [`Self::connect`] refuses a mixed pair and
    /// [`Violation::TypeMismatch`](crate::Violation::TypeMismatch) reports one that arrived from a file, so a
    /// link whose two ends disagree is already named as malformed and does not
    /// need a second opinion here.
    fn link_is_control(&self, tree: TreeId, link: &Link) -> bool {
        self.signature(tree, link.from.node)
            .and_then(|s| s.outputs.get(link.from.port as usize).map(Port::is_control))
            .unwrap_or(false)
    }

    /// Which nodes each node reaches, on one plane of the graph (R1599).
    fn successors_on(&self, tree: TreeId, control: bool) -> BTreeMap<NodeId, Vec<NodeId>> {
        let mut successors: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        let Some(host) = self.tree(tree) else {
            return successors;
        };
        for link in &host.links {
            if self.link_is_control(tree, link) != control {
                continue;
            }
            // R1600 — on the DATA plane a delay is a source: what leaves it is
            // what arrived a tick ago, so nothing downstream of it depends on
            // anything upstream of it *now*. Dropping its outgoing edges is the
            // whole of the causality rule — Lustre's "every cycle must be
            // broken by a `pre`" — and it is one condition rather than a second
            // walk, so `cycle_nodes`, `validate` and `connect` cannot disagree
            // about which cycles are legal.
            if !control && self.cuts_dependency(tree, link.from.node) {
                continue;
            }
            successors
                .entry(link.from.node)
                .or_default()
                .push(link.to.node);
        }
        successors
    }

    /// A **dependency** path from `start` to `goal` inside one tree, following
    /// value links forwards, or `None` when `goal` is unreachable.
    ///
    /// Reported by [`Self::connect`] so a refused wire says which existing
    /// wires would have closed the loop.
    ///
    /// R1599 — value links only, and the name says so. A path through a control
    /// link is not a dependency: it says the two nodes run in an order, not that
    /// one's value is built out of the other's. Following both planes here is
    /// what would make an ordinary execution loop unauthorable.
    #[must_use]
    pub fn data_path_between(
        &self,
        tree: TreeId,
        start: NodeId,
        goal: NodeId,
    ) -> Option<Vec<NodeId>> {
        let host = self.tree(tree)?;
        if start == goal {
            return Some(vec![start]);
        }
        let mut predecessor: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        let mut queue = std::collections::VecDeque::from([start]);
        let mut seen = std::collections::BTreeSet::from([start]);
        while let Some(current) = queue.pop_front() {
            // R1600 — the same cut `successors_on` makes, for the same reason:
            // a walk that continued out of a delay would report a dependency
            // path that does not exist within one tick.
            if self.cuts_dependency(tree, current) {
                continue;
            }
            for link in host
                .links
                .iter()
                .filter(|l| l.from.node == current && !self.link_is_control(tree, l))
            {
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

    /// Which nodes of `tree` lie **on** a dependency cycle, ascending (R1596).
    ///
    /// A node is on one exactly when it is reachable from itself by following
    /// links forwards — a self-loop counts. Being merely *downstream* of a cycle
    /// does not, which is the whole point of the question: an editor showing a
    /// value that cannot be computed needs the knot to break, not the list of
    /// everything the knot spoiled.
    ///
    /// Empty for an acyclic tree, so `cycle_nodes(tree).is_empty()` is the
    /// yes-or-no reading and no second walk is needed to get it. Unreachable
    /// through this crate's API — [`Self::connect`] refuses the wire that would
    /// close a cycle — and reachable through a document that arrived from a file
    /// or a peer, which is what [`Self::validate`] is for.
    ///
    /// **the DCC answers this with a bool and a guess.** `has_available_link_cycle` is one flag for
    /// the whole tree, and the localisation it does offer is to *links*: `update_link_validation`
    /// clears `NODE_LINK_VALID` on every link whose endpoints came out of the toposort in
    /// the wrong order. Which link that is is decided by where the toposort
    /// happened to start — for a tree whose every node is inside the cycle,
    /// `update_toposort` restarts "at this node which is somewhere in the middle of a loop"
    /// in `nodes_by_id` order — so the wire the DCC blames is a function of the order
    /// the nodes were created in, not of the cycle. This answer is a property
    /// of the graph.
    ///
    /// Iterative Tarjan, so an adversarial document cannot overflow the stack of
    /// the process validating it: the components of size two or more are the
    /// cycles, and a one-node component is on a cycle exactly when it links to
    /// itself. `None` when the tree is not in the document.
    ///
    /// R1599 — **value links only.** A control link says the two nodes run in
    /// an order, not that one's value is built out of the other's, so a control
    /// loop is not a dependency cycle and is answered by
    /// [`Self::control_loops`] instead.
    #[must_use]
    pub fn cycle_nodes(&self, tree: TreeId) -> Vec<NodeId> {
        self.on_a_cycle(tree, false)
    }

    /// Which nodes of `tree` lie **on a control loop**, ascending (R1599).
    ///
    /// The same derivation as [`Self::cycle_nodes`], on the other plane — and
    /// the opposite verdict. A cycle through value links is a contradiction, so
    /// [`Self::connect`] refuses to author one and [`Self::validate`] reports
    /// one that arrived from a file. A cycle through **control** links is a
    /// LOOP: an ordinary, intended thing that every execution graph is built to
    /// express, so it is authorable, it is not a violation, and this is how you
    /// ask which nodes are in it.
    ///
    /// **Nothing in the engine answers this.** An execution loop there
    /// compiles — exec pins are excluded from the dependency sort by `PinIsImportantForDependancies`, so
    /// no `Dependency cycle detected` can fire for one — and a loop with no exit is discovered at *run
    /// time*, by a counter (`GMaximumScriptLoopIterations`) raising `EBlueprintExceptionType::InfiniteLoop` after the fact, in a build that
    /// may be shipping. The nodes are named here before it runs, statically.
    ///
    /// Empty for a tree with no control loop, so `control_loops(tree).is_empty()`
    /// is the yes-or-no reading and no second walk is needed to get it.
    #[must_use]
    pub fn control_loops(&self, tree: TreeId) -> Vec<NodeId> {
        self.on_a_cycle(tree, true)
    }

    /// The shared derivation behind [`Self::cycle_nodes`] and
    /// [`Self::control_loops`]: which nodes are reachable from themselves on
    /// one plane.
    fn on_a_cycle(&self, tree: TreeId, control: bool) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let successors = self.successors_on(tree, control);
        let mut state = Tarjan::default();
        for node in host.nodes.keys() {
            if !state.index.contains_key(node) {
                state.run(*node, &successors);
            }
        }
        let mut found: Vec<NodeId> = state
            .components
            .into_iter()
            .flat_map(|component| {
                let cyclic = component.len() > 1
                    || component.first().is_some_and(|&only| {
                        successors
                            .get(&only)
                            .is_some_and(|next| next.contains(&only))
                    });
                component.into_iter().filter(move |_| cyclic)
            })
            .filter(|node| host.nodes.contains_key(node))
            .collect();
        found.sort_unstable();
        found.dedup();
        found
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

/// Tarjan's strongly-connected components, run without recursing (R1596).
///
/// The explicit call stack is not a style preference: [`Document::cycle_nodes`]
/// runs on documents that arrived from a file or a peer, and a recursive walk
/// over one deep enough would take the validating process down with it — which
/// is the same argument [`crate::Evaluator`] answers with its depth cap.
#[derive(Default)]
struct Tarjan {
    /// Discovery order, and the record of having been visited at all.
    index: BTreeMap<NodeId, u32>,
    /// The lowest discovery order reachable from a node's subtree.
    lowlink: BTreeMap<NodeId, u32>,
    on_stack: BTreeSet<NodeId>,
    stack: Vec<NodeId>,
    next: u32,
    components: Vec<Vec<NodeId>>,
}

/// A node with no successors, so the walk reads one slice type either way.
const NO_SUCCESSORS: &[NodeId] = &[];

impl Tarjan {
    /// Begin at `root`, following `successors`, adding every component found.
    fn run(&mut self, root: NodeId, successors: &BTreeMap<NodeId, Vec<NodeId>>) {
        self.open(root);
        // Each frame is a node and how far through its successors the walk is.
        let mut call: Vec<(NodeId, usize)> = vec![(root, 0)];
        while let Some(&(node, cursor)) = call.last() {
            let edges = successors.get(&node).map_or(NO_SUCCESSORS, Vec::as_slice);
            if let Some(&next) = edges.get(cursor) {
                if let Some(frame) = call.last_mut() {
                    frame.1 += 1;
                }
                if self.index.contains_key(&next) {
                    // Already visited: it constrains this node's lowlink only
                    // while it is still on the stack — otherwise it belongs to a
                    // component already closed, and a cross-edge to one says
                    // nothing about a cycle through here.
                    if self.on_stack.contains(&next) {
                        let reached = Self::at(&self.index, next);
                        let low = Self::at(&self.lowlink, node).min(reached);
                        self.lowlink.insert(node, low);
                    }
                } else {
                    self.open(next);
                    call.push((next, 0));
                }
                continue;
            }
            call.pop();
            if Self::at(&self.lowlink, node) == Self::at(&self.index, node) {
                let mut component = Vec::new();
                while let Some(member) = self.stack.pop() {
                    self.on_stack.remove(&member);
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                self.components.push(component);
            }
            if let Some(&(parent, _)) = call.last() {
                let low = Self::at(&self.lowlink, parent).min(Self::at(&self.lowlink, node));
                self.lowlink.insert(parent, low);
            }
        }
    }

    /// Record `node` as reached, at a fresh discovery order.
    fn open(&mut self, node: NodeId) {
        self.index.insert(node, self.next);
        self.lowlink.insert(node, self.next);
        self.next = self.next.saturating_add(1);
        self.stack.push(node);
        self.on_stack.insert(node);
    }

    /// A discovery order that [`Self::open`] has certainly written.
    ///
    /// Every node the walk reads has been opened before it was pushed, so the
    /// absent case is unreachable; it answers `0` rather than panicking because
    /// the caller is a validator running on a document nobody has promised
    /// anything about.
    fn at(map: &BTreeMap<NodeId, u32>, node: NodeId) -> u32 {
        map.get(&node).copied().unwrap_or_default()
    }
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
        K::Type: Serialize,
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
        K::Type: Deserialize<'de>,
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
    /// The link that had to go, if there was one.
    ///
    /// **Which end it was displaced from depends on what the ports carry**
    /// (R1599): a value input takes one producer, so wiring it again displaces
    /// the link that was there; a control output takes one successor, so
    /// wiring *it* again displaces instead. Reporting it is what makes either
    /// replacement undoable — the engine performs the same displacement from
    /// the same two cases (`CONNECT_RESPONSE_BREAK_OTHERS_A`/`_B`) and `TryCreateConnection` answers a bare `bool`, so what it
    /// broke is gone.
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
    /// The port carries **control**, which is not a value (R1599).
    ///
    /// Refused with its own arm rather than through [`Self::WrongType`],
    /// because there is no type to report as expected: a control port has
    /// none. It is also the one refusal here a taxonomy that declines to
    /// classify its values is still held to.
    NotAValuePort {
        /// The port.
        port: PortRef,
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
            Self::NotAValuePort { port } => {
                write!(f, "port {port} carries control, which is not a value")
            }
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
    /// The node is there, but its body is one this crate owns — a frame, a
    /// group instance, an interface node or a delay — so an application kind
    /// cannot be written over it (R1598, widened R1600).
    ///
    /// A structural body is not the application's to overwrite: a frame with a
    /// signature would be linkable, a group instance whose body was replaced
    /// would leave its definition with no instance to reach it by, and a delay
    /// whose body was replaced would be a cut removed from the data plane
    /// without anything having asked whether a cycle depended on it.
    NotAKind {
        /// The tree it is in.
        tree: TreeId,
        /// The node whose body is structural.
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
    /// Bypassing this [`NodeBody::Delay`] would make a value cycle live
    /// (R1600).
    ///
    /// A delay's whole behaviour is that its output does not depend on its
    /// input *this* step, which is what lets a cycle pass through it. Bypassing
    /// it makes it a plain wire, so the cycle it was breaking becomes a
    /// contradiction — the same state [`Document::connect`] refuses to author
    /// directly, reached by taking the cut away instead of adding the wire.
    BypassWouldCycle {
        /// The tree it is in.
        tree: TreeId,
        /// The delay that was going to be bypassed.
        node: NodeId,
        /// The dependency path that would close, from something the delay
        /// feeds back round to the delay itself.
        path: Vec<NodeId>,
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
            Self::NotAKind { tree, node } => write!(
                f,
                "node {} in tree {} is a frame, a group instance, an interface \
                 node or a delay, whose body this crate owns",
                node.0, tree.0
            ),
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
            Self::BypassWouldCycle { tree, node, path } => write!(
                f,
                "bypassing delay {} in tree {} would make a value cycle live: {}",
                node.0,
                tree.0,
                path.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" -> ")
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
    /// One end carries **control** and the other a value (R1599).
    ///
    /// Its own arm rather than a [`Self::TypeMismatch`] with a missing type,
    /// because the two refusals are different facts and an author fixes them
    /// differently: a type mismatch means find a conversion, a flow mismatch
    /// means you have wired an execution pin to a number.
    FlowMismatch {
        /// The producing socket.
        from: Socket,
        /// The consuming socket.
        to: Socket,
        /// Which end carries control — the other carries a value.
        control_end: Side,
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
            Self::FlowMismatch {
                from,
                to,
                control_end,
            } => {
                let (control, value) = match control_end {
                    Side::Output => (from, to),
                    Side::Input => (to, from),
                };
                write!(f, "{control} carries control and {value} carries a value")
            }
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
