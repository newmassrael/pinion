//! R1577 — the model: trees of typed nodes, and the structural edits that
//! maintain their own invariants.
//!
//! The application supplies the taxonomy by implementing [`NodeKind`]; this
//! module supplies everything a node system needs that is *not* taxonomy.

use serde::{Deserialize, Serialize};

use crate::appearance::Appearance;
use crate::group::EditPath;
use crate::items::{Items, Variadic, resolve};
use crate::landing::Berth;
use crate::split::Composition;
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
/// is not an instance: compiler context expands one by calling
/// `graph utilities::CloneGraph(MacroGraph, ...)`, so N instances are N copies
/// of the nodes and each copy is simply its own node. That is also why its
/// recursion check has to exist (`FindMacroCycle`) — inlining cannot terminate
/// on a cycle — where a group *instance* here is checked by
/// [`Document::containment`] being acyclic and needs no expansion to run.
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

    /// Read a path back from the form [`Display`](fmt::Display) writes, or
    /// `None` (R1644).
    ///
    /// An instance printed as `/0:5/2:1` is an address a client can be *given*
    /// — a trace row, a register row, a breakpoint site all carry one — and
    /// until now nothing could take one back. A published form with no inverse
    /// is two definitions of the same thing kept in step by hand, which is the
    /// finding R1642 recorded about [`Side`]'s wire names.
    #[must_use]
    pub fn from_wire(path: &str) -> Option<Self> {
        if path == "/" {
            return Some(Self::root());
        }
        let body = path.strip_prefix('/')?;
        let mut chain = Vec::new();
        for segment in body.split('/') {
            let (tree, node) = segment.split_once(':')?;
            chain.push((TreeId(tree.parse().ok()?), NodeId(node.parse().ok()?)));
        }
        Some(Self(chain))
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
/// execution pin there is an ordinary graph pin whose
/// `PinType.PinCategory` happens to equal the `FName` `"exec"`
/// (`graph schema K 2::PC_Exec`), so "is this pin control?" is a string
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
        /// "pin default" of the DCC and visual script.
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
    /// ★★★★★ R1934 — **not decided yet**: nothing has said whether this port
    /// carries a value or control, and if a value, of which type.
    ///
    /// A third arm and not `Value { ty: Option<T> }`, because the undecided
    /// state is not a value port missing its type — it does not know it is a
    /// value port. [`NodeBody::Reroute`] is the body that reaches it: a
    /// reroute inherits what crosses it, so a reroute chain nothing else
    /// touches carries *this*, and a chain wired to a control edge carries
    /// [`Control`](Self::Control). Both references reach the same state and
    /// spell it as a type: the engine's knot allocates two wildcard pins and
    /// **reverts to wildcard** when its last link goes; the DCC keeps a stored
    /// socket type it recomputes per reroute component.
    ///
    /// # What it means for the two questions a port is asked
    ///
    /// * [`crossing`] — an undecided end is **accepted by everything**, which
    ///   is what lets the first link decide it. That is the engine's
    ///   `HasAnyWildcards` short-circuit, and this crate's version is one arm
    ///   of the one function that decides a pair rather than a second rule.
    /// * [`multiplicity`](Self::multiplicity) — **`One` on both sides**, the
    ///   *intersection* of the two decided rules rather than either of them.
    ///   Deciding a port can then only ever **widen** what it admits, so no
    ///   link that was legal while the port was undecided becomes illegal when
    ///   it is decided. The other choice — `Many` on both sides — would let a
    ///   value input collect two links and then break that invariant the
    ///   moment a type arrived. The engine has the same asymmetry and lives
    ///   with it as a comment: "knots for exec pins can have only one
    ///   connection".
    Undecided,
}

impl<T, V> Flow<T, V> {
    /// The socket type, or `None` for a control port.
    pub const fn value_type(&self) -> Option<&T> {
        match self {
            Self::Value { ty, .. } => Some(ty),
            Self::Control | Self::Undecided => None,
        }
    }

    /// The resting value, or `None` for a control port or a port without one.
    pub const fn default_value(&self) -> Option<&V> {
        match self {
            Self::Value { default, .. } => default.as_ref(),
            Self::Control | Self::Undecided => None,
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
    /// exclude it (`graph schema K 2.cpp`, 5.8.1):
    ///
    /// ```text
    /// bBreakExistingDueToExecOutput = IsExecPin(*OutputPin) && OutputPin->LinkedTo.Num() > 0;
    /// bBreakExistingDueToDataInput  = !IsExecPin(*InputPin) && InputPin->LinkedTo.Num() > 0;
    /// ```
    pub const fn multiplicity(&self, side: Side) -> Multiplicity {
        match (self, side) {
            // R1934 — an undecided port is `One` on BOTH sides: the
            // intersection of the two decided rules, so deciding it can only
            // ever widen what it admits. See [`Self::Undecided`].
            (Self::Value { .. }, Side::Input)
            | (Self::Control, Side::Output)
            | (Self::Undecided, _) => Multiplicity::One,
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
    /// ★★★★★ R1916 — **what this port is FOR**, in a sentence, or `None` when
    /// its name is the whole of what can be said.
    ///
    /// # What forced it, measured in the reference this round
    ///
    /// The reference has two hooks here and neither of them OWNS the sentence.
    /// A node answers `GetPinHoverText(Pin, out HoverText)`, and a schema
    /// answers `ConstructBasicPinTooltip(Pin, PinDescription, out Tooltip)` —
    /// note the second argument: **the description arrives from outside**, and
    /// nothing in the model says where it came from. Read to the end, the base
    /// schema's implementation is one line:
    ///
    /// ```text
    /// TooltipOut = PinDescription.ToString();
    /// ```
    ///
    /// while its own comment promises it "tacks on any other data important to
    /// the schema (things like the pin's type, etc.)". ⇒ **the composition the
    /// documentation describes does not happen by default**, and the sentence
    /// it composes has no home.
    ///
    /// Here the sentence is the PORT's, so it travels with the thing it
    /// describes — a member port a split makes carries its own, a variadic
    /// run's template carries one for every item it produces, and
    /// [`Document::port_tooltip`] is the one place the composition happens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// ★★★★★ R2001 — whether this port belongs to the **advanced** class:
    /// folded away behind one control on the node rather than always on the
    /// frame. `false` for an ordinary port.
    ///
    /// A DECLARATION, on the kind's port, and that placement is the whole of
    /// what separates this from [`Appearance::put_away_inputs`] one layer up.
    /// Putting a port away is a person's statement about ONE node; the
    /// advanced class is the kind's statement about what its port is FOR, so
    /// every node of the kind starts alike and a person's disagreement is
    /// recorded separately ([`Appearance::reclassified`]) instead of
    /// overwriting it.
    ///
    /// # What forced the separation, measured at the reference
    ///
    /// The reference carries the same fact as a bit on the **pin instance**,
    /// which each node class writes while it allocates its pins — so a
    /// person's choice and the class's declaration share one slot. That is why
    /// it then needs [`NodeKind::advanced_ports_are_authored`]: on every
    /// rebuild of a node's pins it has to decide whether to copy the old bit
    /// forward or let the freshly declared one stand, and its own comment on
    /// that branch says the wrong answer means "ignoring new metadata that
    /// tries to hide old pins". Here the two live in different places, so a
    /// rebuild re-reads the declaration and re-applies the override, and
    /// neither can erase the other.
    ///
    /// [`Appearance::put_away_inputs`]: crate::Appearance::put_away_inputs
    /// [`Appearance::reclassified`]: crate::Appearance::reclassified
    /// ⚠ The skip predicate is the **standard library's** negation rather than
    /// a helper written here. `serde` hands it a reference, so a hand-rolled
    /// one has to take `&bool` and `clippy::pedantic` is right to say a
    /// one-byte value should not be — a predicate this crate cannot write
    /// without an `#[allow]` is a predicate it should not be writing.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub advanced: bool,
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
            description: None,
            advanced: false,
        }
    }

    /// ★ R1934 — a port carrying a flow that was **derived** rather than
    /// declared.
    ///
    /// The general constructor the two named ones are cases of. It exists for
    /// [`NodeBody::Reroute`], whose flow is read off the chain it belongs to
    /// and can be any of the three — so the site building it cannot know which
    /// of `new` / `control` to call.
    pub fn with_flow(name: impl Into<String>, flow: Flow<T, V>) -> Self {
        Self {
            name: name.into(),
            flow,
            passthrough: true,
            description: None,
            advanced: false,
        }
    }

    /// ★ R1916 — the sentence this port carries about itself.
    #[must_use]
    pub fn describing(mut self, sentence: impl Into<String>) -> Self {
        self.description = Some(sentence.into());
        self
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
            description: None,
            advanced: false,
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

    /// ★★★★★ R2001 — the same port, declared into the **advanced** class:
    /// folded away behind the node's one advanced control unless something is
    /// wired to it — see [`Self::advanced`].
    #[must_use]
    pub fn advanced(mut self) -> Self {
        self.advanced = true;
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

/// ★★★★★ R1939 — **what a port will admit as its resting value**, beyond the
/// type it carries.
///
/// See [`NodeKind::takes`] for the measurement that shaped this: the reference
/// spells the same capability as an open key-value store hung on a port, whose
/// consumers all ask one question and whose answers are unchecked strings.
///
/// Three arms and not an `Option`, because there are three answers a taxonomy
/// genuinely gives (R1928's rule): *the type is the whole constraint*, *these
/// exact values*, and *a rule*. The first is a real answer a screen can show —
/// "anything of this type" is what an editor needs to know to offer a free
/// field — rather than a hole it has to infer.
#[derive(Clone, PartialEq)]
pub enum Admits<V> {
    /// Any value of the port's type: the type is the whole constraint.
    Anything,
    /// Exactly these values, in the order an editor should offer them.
    ///
    /// An **empty** list is a real answer and not a mistake: it says this port
    /// takes no value at all right now, which a taxonomy whose options depend
    /// on the document can legitimately reach. It is not the same statement as
    /// a control port, which has no value *by construction*
    /// ([`Flow::Control`]).
    OneOf(Vec<V>),
    /// A rule the declaration applies, said in a sentence and able to produce
    /// the value it would take.
    Shaped {
        /// What this rule wants, in a sentence a screen can show and a refusal
        /// can quote.
        wants: String,
        /// The value this port would take in place of the one it is given: the
        /// value ITSELF when it stands, another when there is a repair, and
        /// `None` when there is none.
        ///
        /// ★ One function and not a predicate beside a repair, for R1938's
        /// reason: a permission and the result of taking it cannot disagree
        /// when they are one answer. A screen offering "use 65535 instead"
        /// cannot offer a value the same declaration would then refuse.
        ///
        /// A plain `fn` pointer rather than a boxed closure, as
        /// [`Conversion::Converted`] is: the rule is a property of the port's
        /// declaration and captures nothing. A kind whose rule varies by socket
        /// type returns a different pointer per type, which its own `match`
        /// makes exhaustive.
        nearest: fn(&V) -> Option<V>,
    },
}

impl<V: fmt::Debug> fmt::Debug for Admits<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anything => f.write_str("Anything"),
            Self::OneOf(values) => f.debug_tuple("OneOf").field(values).finish(),
            // The pointer is not printed: its address says nothing a reader can
            // use, and it would make two equal declarations print differently
            // between runs.
            Self::Shaped { wants, .. } => f.debug_struct("Shaped").field("wants", wants).finish(),
        }
    }
}

impl<V: Clone + PartialEq + fmt::Debug> Admits<V> {
    /// Whether `value` may rest at this port, and what it would take instead.
    ///
    /// The whole judgement in one call, so a caller cannot ask the permission
    /// and the repair separately and get answers from two declarations.
    #[must_use]
    pub fn judge(&self, value: &V) -> Judged<V> {
        match self {
            Self::Anything => Judged::Stands,
            Self::OneOf(values) => {
                if values.contains(value) {
                    Judged::Stands
                } else {
                    Judged::Refused {
                        wants: self.wants(),
                        instead: values.first().cloned(),
                    }
                }
            }
            Self::Shaped { nearest, .. } => match nearest(value) {
                Some(near) if near == *value => Judged::Stands,
                near => Judged::Refused {
                    wants: self.wants(),
                    instead: near,
                },
            },
        }
    }

    /// What this port wants, in a sentence.
    ///
    /// Derived from the declaration rather than written a second time, so a
    /// refusal a person reads cannot describe a rule other than the one that
    /// refused them.
    #[must_use]
    pub fn wants(&self) -> String {
        match self {
            Self::Anything => "any value of this port's type".to_owned(),
            Self::OneOf(values) if values.is_empty() => "no value at all, here".to_owned(),
            Self::OneOf(values) => format!(
                "one of {}",
                values
                    .iter()
                    .map(|value| format!("{value:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Shaped { wants, .. } => wants.clone(),
        }
    }
}

/// ★★★★★ R1939 — the judgement [`Admits::judge`] makes about one value.
///
/// Named apart from [`Admission`], which answers whether two **nodes** may be
/// wired (R1885): the two questions refuse for unrelated reasons and are
/// repaired differently, and one word for both would make a screen's refusal
/// text ambiguous about what the author must change.
#[derive(Debug, Clone, PartialEq)]
pub enum Judged<V> {
    /// The value stands as it is.
    Stands,
    /// The port will not take it.
    Refused {
        /// What the port wants, in a sentence — [`Admits::wants`].
        wants: String,
        /// The nearest value the same declaration WOULD take, when there is
        /// one. `None` is a real answer: not every refusal has a repair.
        instead: Option<V>,
    },
}

impl<V> Judged<V> {
    /// Whether the value stands.
    #[must_use]
    pub const fn stands(&self) -> bool {
        matches!(self, Self::Stands)
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

/// Whether two **nodes** may be wired, and — when they may not — why (R1885).
///
/// The answer [`NodeKind::admits`] gives. Distinct from [`Conversion`] because
/// the two refuse for unrelated reasons and an author repairs them differently:
/// a refused conversion means *no value of that type may enter this port*, and
/// a refused admission means *these two nodes cannot talk to each other*, which
/// is fixed by changing one of the ends rather than by finding a map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// The two nodes may be wired.
    Allowed,
    /// They may not, and this says which end to change and why.
    Refused(Refusal),
}

impl Admission {
    /// Whether this refuses the wire.
    #[must_use]
    pub fn is_refused(&self) -> bool {
        matches!(self, Self::Refused(_))
    }

    /// The refusal, when there is one.
    #[must_use]
    pub fn refusal(&self) -> Option<&Refusal> {
        match self {
            Self::Refused(why) => Some(why),
            Self::Allowed => None,
        }
    }
}

/// Why two nodes may not be wired, in the application's own words (R1885).
///
/// ★ **A refusal that cannot be read is a refusal nobody can act on**, which is
/// why this carries a sentence rather than a code. Only the application knows
/// what makes two of *its* nodes incompatible, so only the application can say
/// it; the crate's contribution is to insist that something is said, and to
/// carry the one fact a screen needs beyond the words — which end is at fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// Which end the author must change.
    ///
    /// [`Side::Output`] blames the producing node, [`Side::Input`] the consuming
    /// one. A rule that genuinely cannot choose should blame the producer, since
    /// that is the end a drag starts from.
    pub end: Side,
    /// The sentence a screen shows. Names both ends' relevant facts, because a
    /// refusal that only names the rule leaves the author guessing which of the
    /// two nodes to change.
    pub because: String,
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

    /// ★★★★★ R1999 — **the kinds of graph this taxonomy has.**
    ///
    /// A [`Tree`] carries one of these and [`Document::graph_kind`] answers it,
    /// which is what lets any rule vary by *what a graph is* rather than only
    /// by what a node is.
    ///
    /// # Why the vocabulary is the taxonomy's and not this crate's
    ///
    /// Measured at the reference: its answer is a **fixed five-member
    /// enumeration** declared beside the schema base class, and the comment
    /// directly above the hook that answers it says, in its own words, that
    /// this is too specific to one editor to belong there and should be
    /// refactored. Every application in that engine — a material graph, a sound
    /// cue, a behaviour tree — is answered out of one visual-scripting
    /// vocabulary it has no use for. An associated type puts the words where
    /// the subject matter is, exactly as [`Type`](NodeKind::Type) and
    /// [`Value`](NodeKind::Value) already do.
    ///
    /// `Default` is the kind a tree gets when nobody chose one — the reference
    /// hard-codes *function* for that, for every application at once. Here the
    /// taxonomy says. A taxonomy that does not distinguish its graphs writes
    /// `type Graph = ();`, which is the honest statement that it has one kind,
    /// and not an escape hatch: with a one-member vocabulary there is no other
    /// answer to leave unchosen.
    type Graph: Clone + PartialEq + fmt::Debug + Default;

    /// ★★★★★ R1999 — **which kinds of graph this node kind is at home in.**
    ///
    /// The reference asks the same question of a node — *are you compatible
    /// with this graph* — and every implementation of it reads the graph's kind
    /// and compares. Measured across the reference's engine source, **53** call
    /// expressions read that kind, and **sixteen** of them, in **fifteen** node
    /// classes, are this exact test written out by hand — the largest group by
    /// a factor of four; a node kind added afterwards is compatible with
    /// everything until somebody remembers to edit one more of them.
    ///
    /// Here it is one declaration, read by the refusal
    /// ([`Document::admits`], and so every verb that goes through it) and by
    /// the offer ([`Document::at_home`], which a palette filters with) — so a
    /// chooser that offered a kind the edit would refuse is unrepresentable,
    /// which is the shape [`Admitted`](crate::Admitted) was built for at R1933.
    ///
    /// The default is [`Anything`](crate::Admitted::Anything): a kind that has not thought about
    /// it belongs everywhere, which is the reference's supplied answer too.
    #[must_use]
    fn at_home(&self) -> crate::Admitted<Self::Graph> {
        crate::Admitted::Anything
    }

    /// A stable identity token — the answer to "what does this node do".
    ///
    /// Never derived from a user-facing label: a node renamed "Foo" still
    /// multiplies.
    fn name(&self) -> String;

    /// ★★★★★ R1997 — **what a tree of this taxonomy is born holding.**
    ///
    /// A new graph in a professional editor is rarely empty: a material comes
    /// with its result node, a state comes with its output, a sound cue comes
    /// with its root. The reference spells this as a schema hook,
    /// `CreateDefaultNodesForGraph(Graph)`, whose base is an empty body and
    /// which seven schemas override — each creating one node (a transition
    /// creates three), positioning it, and marking it so the graph can say
    /// afterwards whether anyone has touched it.
    ///
    /// An associated function and not a `&self` method, because there is no
    /// node yet when a tree is born — the question is asked of the TAXONOMY,
    /// which is what the reference asks it of too.
    ///
    /// The default is empty, which is the reference's base body: a tree born
    /// with nothing is a legitimate answer and most taxonomies want it.
    ///
    /// ⚠ Only [`Document::open_definition`] consults this. `add_definition`
    /// deliberately does not, and neither do `group`, `insert` or the fragment
    /// verbs — a definition that is about to be filled from a selection must
    /// not also be seeded, and the reference draws the same line: it calls the
    /// hook at chosen sites and not on every graph it makes.
    #[must_use]
    fn opening() -> Vec<Seed<Self>>
    where
        Self: Sized,
    {
        Vec::new()
    }

    /// ★★★★★ R1998 — **what this taxonomy will put in place of a body a paste
    /// cannot land**, or `None` when it has nothing to offer.
    ///
    /// Asked only after the destination has already refused, and told *why* it
    /// refused. The engine's own hook is asked the same question and is not
    /// told the reason: it re-decides for itself whether the destination is the
    /// sort of graph that could have held the node, which is the destination's
    /// answer being computed a second time in a second place.
    ///
    /// An associated function, like [`opening`](NodeKind::opening) and for the
    /// same reason: the node being replaced is not in the document — it is a
    /// body in a fragment — so there is no node to ask. The engine asks its
    /// *schema* here too, never the node.
    ///
    /// # What a stand-in inherits
    ///
    /// What a person wrote — the label, the note — and nothing the kind holds.
    /// Values and items belong to the body that arrived, and a different kind
    /// has no use for them. The engine's one overrider takes the same line: it
    /// builds a fresh node and carries only the name across.
    ///
    /// # What a stand-in owes
    ///
    /// It has to be able to carry the wires the original carried, or the paste
    /// is refused with
    /// [`InsertError::SubstituteCannotCarry`](crate::InsertError::SubstituteCannotCarry)
    /// — see [`crate::Unlandable`] for why that refusal exists rather than the
    /// engine's silent loss of the wires that find no partner.
    ///
    /// The default is `None`: nothing is offered and the paste keeps the
    /// refusal it already had. That is the base implementation's answer too,
    /// and it is the honest one — a taxonomy that has no stand-in for a body
    /// should not be made to invent one.
    #[must_use]
    fn substitute(body: &NodeBody<Self>, why: &crate::Unlandable) -> Option<NodeBody<Self>>
    where
        Self: Sized,
    {
        let (_, _) = (body, why);
        None
    }

    /// ★★★★★ R1923 — **the sentence this kind says about itself**, or `None`
    /// when it has nothing to add to its name.
    ///
    /// `&self` and not an associated function, because the reference's own
    /// hook takes the node: a kind that carries a mode or a chosen operation
    /// should be able to describe THAT rather than its family. The engine's
    /// tooltip hook is a virtual on the node for the same reason.
    ///
    /// Defaulted to `None` so an application that has nothing to say is not
    /// forced to say something — a description invented to satisfy a trait is
    /// worse than none, because a reader cannot tell it from a real one.
    /// [`Document::description`] is where that absence becomes an answer.
    fn description(&self) -> Option<String> {
        None
    }

    /// This kind's **fixed** input ports, in order.
    ///
    /// Fixed because a kind may also declare a run that repeats per node
    /// ([`Self::variadic`]); the run is spliced into this list and the result
    /// is [`Document::signature`]. A kind with no run — the overwhelming
    /// majority — has this list *as* its inputs.
    fn inputs(&self) -> Vec<Port<Self::Type, Self::Value>>;

    /// This kind's **fixed** output ports, in order.
    fn outputs(&self) -> Vec<Port<Self::Type, Self::Value>>;

    /// Which run of this kind's ports repeats per **node**, if any (R1632).
    ///
    /// A method on `&self` rather than an associated function, because whether
    /// a node is variadic can depend on what the kind is carrying — the
    /// engine's selector stops accepting options once its index pin is a
    /// boolean or an enumeration (its selector node's `CanAddPin`), and a taxonomy
    /// that models the same thing puts that in its kind.
    ///
    /// The default is `None` on both sides, so a taxonomy with no variadic node
    /// writes nothing. See [`Variadic`] for what the
    /// declaration fixes and why it is one declaration rather than the
    /// reference's four hooks.
    fn variadic(&self, side: Side) -> Option<Variadic<Self::Type, Self::Value>> {
        let _ = side;
        None
    }

    /// ★★★★★ R1980 — **where an arriving end berths** among this kind's ports
    /// that have room for it, when a wire is released on the node's body
    /// ([`Document::land`](crate::Document::land)).
    ///
    /// A method on `&self` for [`variadic`](Self::variadic)'s reason: whether a
    /// node wants a port of its own per arrival can depend on what the kind is
    /// carrying — the same kind configured two ways is two different things to
    /// land on.
    ///
    /// The default is [`Berth::Earliest`], which is what every node did before
    /// this could be asked, so a taxonomy that has never thought about it keeps
    /// the answer a person expects.
    ///
    /// ★ It answers a POLICY and not a socket. The reference's equivalent is
    /// handed the link and moves its end itself, which is why its bool cannot
    /// say whether it did — see [`Berth`] for the three measured consequences.
    fn berth(&self, side: Side) -> Berth {
        let _ = side;
        Berth::Earliest
    }

    /// ★★★★★ R1912 — whether this kind's ports **are** the node, so none of
    /// them may be put away
    /// ([`Document::put_away_ports`](crate::Document::put_away_ports)).
    ///
    /// The default is `false`: an ordinary node is a box with ports on it, and
    /// hiding one leaves a box.
    ///
    /// # What it is for, measured
    ///
    /// The DCC refuses exactly this and writes the refusal as a **name test**
    /// on one node type, with the reason in a comment beside it: *the reroute
    /// node is the socket itself, do not hide this*. A name test covers the
    /// node types somebody remembered; a declaration covers the ones written
    /// next, which is the difference this crate takes everywhere else.
    ///
    /// It is deliberately NOT derived from the port count. A one-in one-out
    /// node is not automatically its ports — an ordinary unary operator has the
    /// same shape and hiding its unused output is a legitimate thing to want —
    /// and a derivation would refuse those too while silently admitting a
    /// pass-through node that happened to grow a third port.
    fn ports_are_the_node(&self) -> bool {
        false
    }

    /// ★★★★★ R2001 — whether a PERSON, rather than this kind, may say which of
    /// a node's ports belong to the **advanced** class
    /// ([`Document::classify_port`](crate::Document::classify_port)).
    ///
    /// The default is `false`: a kind that declares
    /// [`Port::advanced`](crate::Port::advanced) has said what its port is for,
    /// and an editor offering to overrule that on one node would be offering to
    /// disagree with the taxonomy.
    ///
    /// # What it is, measured at the reference rather than summarised
    ///
    /// The reference asks its node class the same question and **two** classes
    /// in its whole tree answer yes — a switch over an enumeration, and an
    /// input-action node. Read at those two, the reason is the same in both:
    /// their advanced set is not a property of the class at all. The switch's
    /// *remove pin* does not delete a case, because the cases are the
    /// enumeration's; it moves that pin into the advanced class and breaks its
    /// links, and *add pin* takes the first hidden one back — so the set is
    /// exactly what the person has been doing, and the class re-deriving it
    /// would undo their work on the next rebuild.
    ///
    /// ⚠ **Here it is a permission and ONLY a permission**, which is a smaller
    /// job than the reference's hook has. There the flag also decides, at the
    /// one place that reads it, whether a rebuilt pin copies the old pin's bit
    /// forward — because there a declaration and a person's choice are the same
    /// storage. This crate keeps them apart
    /// ([`Port::advanced`](crate::Port::advanced) against
    /// [`Appearance::reclassified`](crate::Appearance::reclassified)), so
    /// nothing has to choose which one survives: that half is unrepresentable
    /// rather than answered.
    fn advanced_ports_are_authored(&self) -> bool {
        false
    }

    /// ★★★★★ R1912 — **what a value type is made of**, which decides whether a
    /// port carrying it can be split into one port per member
    /// ([`Document::splittable`](crate::Document::splittable)).
    ///
    /// An associated function and not a method, for [`conversion`]'s reason:
    /// what a type contains is a fact about the taxonomy, not about the node
    /// that happens to carry it. Two nodes of different kinds asking about one
    /// type must get one answer, and a `&self` hook makes that a coincidence.
    ///
    /// The default is [`Composition::Atom`], so a taxonomy with no composite
    /// type writes nothing.
    ///
    /// # What forced it, measured
    ///
    /// Nothing on this trait could answer it. Counted at R1912, the trait
    /// published twelve associated items and the two that speak about a type
    /// answer *what type does this value have* and *does this type reach that
    /// one*. A run of repeated ports ([`Variadic`]) is not the shape either —
    /// it repeats a template the KIND fixes and never looks at a type, where a
    /// split's member list is a property of the type the port carries. The
    /// campaign that tracks this axis had recorded the opposite since R1632,
    /// and the measurement is what overturned it.
    ///
    /// [`conversion`]: NodeKind::conversion
    fn composition(ty: &Self::Type) -> Composition<Self::Type, Self::Value> {
        let _ = ty;
        Composition::Atom
    }

    /// ★★★★★ R1925 — **which of this taxonomy's socket types is the two-state
    /// one**, or `None` when it has none.
    ///
    /// The one declaration behind a section switch
    /// ([`Document::make_section_switch`](crate::Document::make_section_switch)):
    /// it is the type [`Document::new_section_switch`](crate::Document::new_section_switch)
    /// *creates*, and it is what an existing port is checked against. Declared
    /// once and read twice, so "this port may switch a section" and "this is
    /// what a new switch carries" cannot come apart — the same reason
    /// [`conversion`](NodeKind::conversion) is the conversion itself rather
    /// than a predicate beside one.
    ///
    /// An associated function for [`composition`](NodeKind::composition)'s
    /// reason: it is a fact about the taxonomy, not about a node.
    ///
    /// The reference cannot express this question at all — it names
    /// `NodeSocketBool` in the operator, three times, because its socket types
    /// are a fixed set it owns. A crate whose taxonomy is the application's has
    /// to ask, and an application that answers `None` (the default) simply has
    /// no section switches and is told so
    /// ([`SwitchRefusal::NoSwitchType`](crate::SwitchRefusal::NoSwitchType))
    /// rather than having any port accepted.
    #[must_use]
    fn switch_type() -> Option<Self::Type> {
        None
    }

    /// ★★★★★ R1937 — **the type that is MANY of this one, held that way**, or
    /// `None` when this taxonomy has no such type.
    ///
    /// The engine asks its schema whether a pin type may be put in a container
    /// shape; this answers the same question and answers it with the TYPE, so a
    /// caller that may offer the shape also knows what offering it produces.
    ///
    /// # ★ Why the default is a refusal, and the reference's is not
    ///
    /// Measured: there the hook's default body is `return true` — every type
    /// may be put in every container — and its ONE overrider in the whole tree
    /// answers `None || Array || Set || Map`, which is the same four. So the
    /// declaration exists and **nothing in that tree ever refuses through it**;
    /// its two consumers are both the pin type selector, filtering a menu that
    /// is never actually filtered. A hook whose refusal is never taken is a
    /// hook nobody has had to think about.
    ///
    /// Here `None` is the default, so a taxonomy has containers exactly when it
    /// says it does. That is the same choice R1937 made for
    /// [`retyped`](Self::retyped) and for the same reason: what most
    /// applications get should be the answer that cannot be wrong.
    ///
    /// # Why it answers a TYPE rather than a bool
    ///
    /// Because a `bool` leaves the caller to find the container type somewhere
    /// else, and *somewhere else* is a second statement free to disagree with
    /// this one. A selector that may offer "array of Number" wants the type
    /// that is an array of Number, and here the permission and the answer are
    /// the same value.
    #[must_use]
    fn contained(ty: &Self::Type, held: Container) -> Option<Self::Type> {
        let _ = (ty, held);
        None
    }

    /// ★★★★★ R1926 — **what colour a value of this type is drawn in**, or
    /// `None` when the taxonomy gives it none.
    ///
    /// The ONE declaration behind every colour a port is drawn in:
    /// [`type_palette`](crate::type_palette) reads it for the type itself and,
    /// through [`composition`](NodeKind::composition), for each member of a
    /// composite; [`Document::port_palette`](crate::Document::port_palette)
    /// derives a port's from it. So *what colour is this type* and *what colour
    /// is this port* cannot disagree — there is nothing to disagree with.
    ///
    /// An associated function for [`composition`](NodeKind::composition)'s
    /// reason: it is a fact about the taxonomy, not about a node.
    ///
    /// ★ `Option`, where the reference's equivalent returns an actual black.
    /// Measured this round, its own K2 implementation writes `// Type does not
    /// have a defined color!` and then returns a settings default — so there,
    /// *nobody coloured this* and *somebody chose black* are the same answer.
    #[must_use]
    fn type_colour(ty: &Self::Type) -> Option<crate::Tint> {
        let _ = ty;
        None
    }

    /// ★★★★★ R1926 — what colour a **control** port is drawn in.
    ///
    /// Its own declaration and not a case of
    /// [`type_colour`](NodeKind::type_colour), because R1599 made control not a
    /// type: a port carries a value **or** control ([`Flow`]), and control has
    /// no type to look a colour up by. The reference reaches its execution
    /// pin's colour through the type hook, because there an exec pin *is* a pin
    /// type — a string category. That is the price of the stronger model, and
    /// it is one extra default-`None` declaration.
    #[must_use]
    fn control_colour() -> Option<crate::Tint> {
        None
    }

    /// ★★★★★ R1934 — what colour an **undecided** port is drawn in.
    ///
    /// Beside [`control_colour`](NodeKind::control_colour) and for the same
    /// reason: [`Flow::Undecided`] has no type to look a colour up by, so a
    /// taxonomy that wants its reroutes drawn in a resting colour has nowhere
    /// else to say so.
    ///
    /// ★ Measured on the engine rather than assumed: its graph-editor settings
    /// carry a wildcard pin colour of its own (a dark grey) beside the
    /// per-category ones, and its promotable-operator node draws an unresolved
    /// pin in exactly that. So the reference does support a resting colour for
    /// this state, which is why the hook is built (R1926's rule, in the
    /// direction that says *build it*).
    #[must_use]
    fn undecided_colour() -> Option<crate::Tint> {
        None
    }

    /// ★★★★★ R1940 — **what is THIS node drawn as**, when nobody has authored
    /// a colour for it.
    ///
    /// `&self` and not an associated function, which is the whole point and the
    /// difference from [`type_colour`](NodeKind::type_colour): the answer may
    /// depend on what this particular node is doing, so two nodes of one kind
    /// can be drawn differently.
    ///
    /// The supplied answer is [`Drawn::Unstated`]: a kind that says nothing
    /// leaves the node to whatever the application draws a node as, which is
    /// what this crate did before the question could be asked.
    ///
    /// # What forced it, measured in the reference this round
    ///
    /// A node type there may supply an optional override of the CLASS its
    /// header is drawn from, answered per node — and the class then selects a
    /// themed colour. Three things were measured about it:
    ///
    /// * **All three implementations DERIVE the class from the node's own
    ///   authored state**, and none of them stores anything: one reads the
    ///   colour tag of the definition its group instance stands for, and two
    ///   read the node's chosen data type and answer *vector operation* or
    ///   *colour operation* where they would otherwise answer *converter*. So
    ///   the capability is not "a colour per node" — a person authoring a
    ///   colour is a different, already-built axis ([`Appearance::tint`]) — it
    ///   is **a kind saying what its node currently IS**.
    /// * **The fallback is a SECOND declaration of the same fact.** A type that
    ///   supplies the override also declares a fixed class, consulted when the
    ///   override is absent; both of the two data-type implementations answer
    ///   exactly that fixed class in their own default branch. The same fact is
    ///   written twice, in two places, and **nothing in that tree checks that
    ///   the two agree** — measured by searching its sources and tests for any
    ///   assertion relating them, which finds none. Here there is one
    ///   declaration, because a kind IS the node's state (R1937), so there is
    ///   nothing to keep in step.
    /// * **Both consumers carry their own copy of the choosing expression** —
    ///   the header-drawing code and the colour-tag query each spell *use the
    ///   override if there is one, else the fixed class*. Here that lives in
    ///   [`Document::faces`], once.
    ///
    /// ⇒ the answer is a three-arm [`Drawn`], whose third arm says *drawn like
    /// this TYPE* and reaches the very palette a port of that type is drawn
    /// with. That correspondence is the thing the reference cannot state: its
    /// classes and its socket types are separate vocabularies.
    ///
    /// [`Appearance::tint`]: crate::Appearance::tint
    /// [`Drawn`]: crate::Drawn
    /// [`Drawn::Unstated`]: crate::Drawn::Unstated
    fn drawn_as(&self) -> crate::Drawn<Self::Type> {
        crate::Drawn::Unstated
    }

    /// ★★★★★ R1934 — **does a wire pass straight through this node**, and by
    /// which two ports?
    ///
    /// `None` — the supplied answer — for an ordinary node: a wire reaching it
    /// arrives, and what leaves is what the node computed.
    ///
    /// [`NodeBody::Reroute`] is this crate's own always-passing body and does
    /// not go through here. This hook is for an application kind that is *also*
    /// a point on a wire, which is not hypothetical: of the engine's three
    /// overriders of the equivalent hook, two are its two reroute classes and
    /// the third is a **dataflow** node class that answers by asking which
    /// dataflow node it is currently holding — an answer no editor-side
    /// taxonomy could have given for it.
    ///
    /// `&self` and not an associated function for exactly that reason: the
    /// answer is a fact about *this node*, not about the kind.
    #[must_use]
    fn passing(&self) -> Option<crate::Passing> {
        None
    }

    /// ★★★★★ R1937 — **one of my ports has been given a type: what do I
    /// become?**
    ///
    /// `None` — the default — means *this port's type is not a person's to
    /// choose*, which is the ordinary case and is also the answer a screen
    /// needs before it offers a chooser at all. One declaration answers both
    /// questions, which is R1928's shape: a hook that says what the node
    /// becomes also says, by refusing, that nothing can be chosen here.
    ///
    /// # Why it answers a KIND rather than mutating
    ///
    /// Because the reference's equivalent is a `void` notification and that is
    /// its defect, measured rather than argued. There a pin widget calls
    /// `PinTypeChanged` on the owning node — the one external call site of
    /// seven mentions — and the node reacts by *storing the type on itself and
    /// reconstructing*: `IndexPinType = Pin->PinType`, then every pin is marked
    /// discardable and the node is rebuilt. Three consequences follow, and this
    /// hook has none of them:
    ///
    /// * **the node cannot refuse.** The type has already changed when the node
    ///   is told; the hook's name is past tense. Answering `None` here is a
    ///   refusal, and it happens *before* anything moves.
    /// * **nobody learns what it cost.** The reconstruction drops pins, and
    ///   with them wires and authored values, and the notification returns
    ///   nothing. [`Document::set_port_type`] answers a
    ///   [`Swapped`](crate::Swapped) — the same report
    ///   [`set_kind`](Document::set_kind) gives, because it is the same edit.
    /// * **the answer is a second copy of the node's own state.** Here the kind
    ///   IS the state, so a kind that varies by type is a kind that says so in
    ///   its own vocabulary, and there is nothing to keep in agreement.
    ///
    /// # What `port` addresses
    ///
    /// The node's own signature, the reading
    /// [`signature`](Document::signature) gives — so a kind with a variadic run
    /// (R1632) is asked about the port a person actually clicked rather than
    /// about an index into its declaration.
    #[must_use]
    fn retyped(&self, port: PortRef, ty: &Self::Type) -> Option<Self>
    where
        Self: Sized,
    {
        let _ = (port, ty);
        None
    }

    /// ★★★★★ R1932 — **what this kind requires of the name a person gives one of
    /// its nodes**: where it has to be unique, or that it need not be.
    ///
    /// The supplied answer is [`Naming::InTree`](crate::Naming::InTree), which
    /// is what this crate has enforced since R1682 — so a taxonomy that says
    /// nothing keeps exactly the rule it had.
    ///
    /// An associated function and not a method: measured across the reference's
    /// fourteen overriders, every one answers for its CLASS and none looks at
    /// the node's own state. A kind whose instances disagreed about how far
    /// their names must reach would make "is this name taken" a question with
    /// no fixed population.
    ///
    /// ⚠ [`Naming::Free`](crate::Naming::Free) is the reference's commonest
    /// single answer — four of the fourteen hand back a validator that accepts
    /// everything — and it is the one this crate could not previously say at
    /// all.
    #[must_use]
    fn naming() -> crate::Naming {
        crate::Naming::InTree
    }

    /// ★★★★★ R1985 — **what a copy of one of these nodes does about a name its
    /// destination already holds.**
    ///
    /// Asked by [`Document::insert`] and [`Document::duplicate`], and only
    /// where [`Self::naming`] requires uniqueness — a free name is a caption
    /// and has no clash to have a policy about.
    ///
    /// The supplied answer is [`Copying::Renamed`](crate::Copying::Renamed),
    /// which is the DCC's, and it is what this crate did *implicitly and
    /// wrongly* before this round: it copied the label verbatim, leaving two
    /// nodes answering to one name in a scope its own
    /// [`Document::may`] refuses to create that state in. See
    /// [`Copying`](crate::Copying) for both references' behaviour, measured.
    ///
    /// ⚠ Answered per node rather than per kind — unlike [`Self::naming`],
    /// which is a static — because the reference's own overriders are
    /// instance-sensitive: its event class decides by what the destination
    /// already implements, not by being an event.
    #[must_use]
    fn copying(&self) -> crate::Copying {
        crate::Copying::Renamed
    }

    /// ★★★★★ R1928 — **what this node calls the port at `at`**, given the name
    /// the port was declared with.
    ///
    /// The supplied answer is [`Declared`](crate::PortName::Declared): a kind that says nothing
    /// keeps its own declaration, which is the ordinary case and the one the
    /// reference also supplies.
    ///
    /// THREE arms where the reference has two hooks, and the third is the one
    /// its own source needs most: measured this round, four of its six
    /// overriders use the capability to make a pin show **no** name, and it
    /// spells that as the empty text — so a class that overrides the *whether*
    /// hook and forgets the *what* hook suppresses every one of its pin names by
    /// accident, and one of the six is in exactly that state.
    /// [`Silent`](crate::PortName::Silent) makes suppression a thing a kind says.
    ///
    /// `&self` and not an associated function: the reference's own two
    /// title-returning overriders answer from the node's authored comment, so
    /// this is a judgement about THIS node, not about the taxonomy.
    ///
    /// ⚠ This does **not** replace [`Item::label`](crate::Item::label). A node
    /// has been able to name an item of a variadic run since R1632; this is how
    /// a node names a *fixed* port, and how any port is made unlabelled.
    /// [`Document::port_label`](crate::Document::port_label) is where the two
    /// meet, and it reports which of them answered.
    fn port_name(&self, at: crate::PortRef, declared: &str) -> crate::PortName {
        let _ = (at, declared);
        crate::PortName::Declared
    }

    /// ★★★★★ R1939 — **what the port at `at` will TAKE as its RESTING
    /// VALUE**, given the socket type it carries.
    ///
    /// Named apart from [`Self::admits`], which is R1885's question about
    /// whether two **nodes** may be wired at all: this one is about a value
    /// nobody wired, resting on a port.
    ///
    /// The supplied answer is [`Admits::Anything`]: a kind that says nothing
    /// leaves the port's TYPE as the whole constraint, which is what this crate
    /// did before the question could be asked.
    ///
    /// # What forced it, measured in the reference this round
    ///
    /// The reference hangs an OPEN key-value store off a port — a node is asked
    /// for a pin name AND A KEY and answers a string — and read from that
    /// signature alone it is a bag of untyped strings, which is why this row's
    /// own recorded reason called the absence deliberate. Read from its
    /// **consumers** it is not a bag at all: twenty-one call sites ask eighteen
    /// distinct keys, and every one of them is the same question — *what may
    /// rest at this port, and how should an editor offer it?* Four ask for a
    /// numeric range, nine for a filter on what may be picked, one for a closed
    /// list of options, four for how to present the editor.
    ///
    /// Three further measurements decided the shape here:
    ///
    /// * **Nobody authors that metadata on a port.** All eleven overriders
    ///   reach the SAME lookup: from the pin to the declaration it was
    ///   generated from — a struct's field, a function's parameter, the
    ///   function itself — falling back to its parent. ⚠ Not all of them by
    ///   CHAINING, which is this round's own sentence corrected before it was
    ///   published: nine call up, the tenth IS that lookup, and the eleventh
    ///   chains to nothing and runs the same lookup against its own model.
    ///   Four add a case of their own beside it — three a fixed string for one
    ///   pin-and-key pair, one built from the graph — and only TWO of those
    ///   sit AHEAD of the lookup; the other two are a fallback behind it,
    ///   taken only when it answered empty. Not one of the eleven reads a
    ///   store hung on the port, so the constraint is a fact about the
    ///   DECLARATION, which is why it lives on the kind here and not on the
    ///   stored node.
    /// * **Absence is spelled as the empty string**, so *no such key*, *the key
    ///   says nothing* and *the key says ""* are one value; its consumers test
    ///   emptiness to mean "not declared". The R1928 ambiguity again, on
    ///   another axis.
    /// * **And one shipped overrider ignores the key it is asked for**,
    ///   answering one fixed key's value for every question put to it. Nothing
    ///   in that tree can catch it, because a string key is checked against
    ///   nothing. That is the cost of the bag, stated as a measured defect
    ///   rather than as a preference.
    ///
    /// ⇒ the bag is not built. The CAPABILITY is, as a declaration that says
    /// what it wants and produces the value it would have taken instead.
    ///
    /// `&self` and not an associated function, for [`Self::port_name`]'s
    /// reason: the reference's own lookups reach the node's authored state (the
    /// function it calls, the variable it names), so this is a judgement about
    /// THIS node.
    ///
    /// A **control** port is never asked: control is not a value, so there is
    /// nothing for it to admit, and [`Document::set_port_value`] refuses one
    /// before the taxonomy is consulted at all.
    fn takes(&self, at: crate::PortRef, ty: &Self::Type) -> Admits<Self::Value> {
        let _ = (at, ty);
        Admits::Anything
    }

    /// ★★★★★ R1927 — **whether this node is in a questionable state, and what
    /// to say about it** — or `None` when it has nothing to say.
    ///
    /// ONE answer where the reference has two independent ones. Measured this
    /// round, of its two overriders one overrides the *should I warn* half and
    /// leaves the *what does it say* half at its empty supplied answer, so that
    /// node shows a badge carrying no reason. Returning the sentence WITH the
    /// warning makes that state unrepresentable.
    ///
    /// `&self` and not an associated function, unlike
    /// [`type_colour`](NodeKind::type_colour) and its neighbours: this is a
    /// judgement about **this node's own configuration**, not a fact about the
    /// taxonomy, and two nodes of one kind are expected to disagree.
    ///
    /// [`Surroundings`](crate::Surroundings) is what the node is wired to.
    /// Handed over rather than reached for, which is the second measured
    /// difference: the reference's signature gives its overriders nothing, so
    /// one of them walks its chain of containers in a loop to find a setting
    /// and the other asks a global for the node being debugged.
    ///
    /// ⚠ This is **not** [`Document::validate`](crate::Document::validate). That
    /// answers how a document breaks the crate's own structural rules and no
    /// application may add to it or silence it; this is the application's
    /// judgement about one node in a graph that is perfectly well formed.
    ///
    /// ★★★★★ R1943 — **does this kind OPEN a bracketed region, and what kind
    /// closes it?**
    ///
    /// `None` — the supplied answer — for an ordinary node. A kind that opens a
    /// zone answers the kind that must close it, so *what may close this* is a
    /// value a screen can act on rather than a rule it has to know.
    ///
    /// # What forced it, measured in the reference this round
    ///
    /// Its add-a-zone operator does exactly four things: it creates an INPUT
    /// node and an OUTPUT node, pairs them, places them either side of the
    /// cursor, and wires the one socket they share. So a zone is **not a stored
    /// region** — the region is derived from a PAIR, which is what this
    /// declaration makes expressible. Its four zones (a simulation across a
    /// time span, a dynamic repetition, a per-element operation, and a closure
    /// evaluated elsewhere) are four such pairs.
    ///
    /// Two measured defects decide the shape here:
    ///
    /// * **The pairing is a one-way id.** It is stored on the opening node as
    ///   the closer's identifier and nothing is stored on the closer, so *what
    ///   does this close?* is a scan of every opening node in the tree — its own
    ///   pairing routine performs exactly that scan to find out whether a
    ///   closer is already spoken for. Here one map holds it and the reverse is
    ///   derived, so the two directions cannot disagree.
    /// * **Its refusals are REPORTED, not returned.** The routine answers
    ///   `bool` and writes the reason into a report list, so a caller told
    ///   *false* cannot tell *wrong kind of closer* from *that closer is
    ///   already paired*. Here they are two arms of
    ///   [`PairError`] — R1942's class, met on another axis.
    ///
    /// ★ Answering a KIND rather than a name is what removes the reference's
    /// lookup table: it maps a node type to a zone type to that zone's output
    /// type, three hops through registries, where this is one value the
    /// taxonomy already has.
    fn closed_by(&self) -> Option<Self> {
        None
    }

    /// ★★★★★ R1942 — **can a value of this type be LOOKED AT while the graph
    /// runs**, or is it a type that carries something with no value to read?
    ///
    /// The supplied answer is [`Inspectable::Yes`]: a type that carries a value
    /// has a value to look at, which is what this crate assumed before the
    /// question could be asked. A taxonomy declares the exceptions.
    ///
    /// # What forced it, measured in the reference this round
    ///
    /// Its schema is asked whether a pin may show its data, and the answer
    /// gates whether a debugger will let a person inspect that pin at all.
    /// Counted: one supplied declaration (answering **no**, because a bare
    /// schema knows none of its types), **two** overriders and **one**
    /// consumer.
    ///
    /// The two overriders are what decided the shape here. One refuses
    /// **execution** pins and **delegate** pins; the other refuses **pose**
    /// pins and then defers to the first. Execution is already answered here —
    /// control is not a value ([`Flow::Control`]) and
    /// [`WatchError::NotAValue`](crate::WatchError::NotAValue) has refused it
    /// since R1644. The other two are the gap: **a type that carries a value
    /// and still has nothing a person can read**, which nothing here could say.
    ///
    /// ★★★★★ AND THE MEASURED DEFECT IS THAT ITS ANSWER IS A BARE `bool`. Its
    /// one consumer asks five separate questions — the pin is orphaned, the
    /// owning node is disabled, the schema refuses, there is no debug context,
    /// the session is not running — and folds every one of them into the same
    /// `false`. A person told *no* cannot tell which of the five it was, and
    /// nothing downstream can either. Here the refusal carries its sentence, so
    /// [`Document::stale_watches`](crate::Document::stale_watches) can say
    /// which of its reasons applied to which port.
    ///
    /// An associated function and not a method, for
    /// [`type_colour`](NodeKind::type_colour)'s reason: whether a value can be
    /// read is a fact about the TYPE, and two ports of one type must not be
    /// able to disagree about it.
    ///
    /// [`Inspectable::Yes`]: crate::Inspectable::Yes
    fn inspectable(ty: &Self::Type) -> crate::Inspectable {
        let _ = ty;
        crate::Inspectable::Yes
    }

    /// ★★★★★ R1941 — **and the answer carries its WEIGHT**, which is the axis
    /// R1927 left out. Measured in the reference: its graph node is asked to
    /// validate itself during compilation, and what it says goes into the log
    /// whose error count decides whether the build succeeded — across the
    /// editor's blueprint nodes, **27 errors, 31 warnings and 2 notes**. So a
    /// node there may REFUSE, not merely complain, and the weight is chosen by
    /// which log method the implementation called rather than by anything a
    /// caller can read back. Here it is [`Objection`](crate::Objection), a
    /// value, so [`Document::may_run`](crate::Document::may_run) is a question
    /// anybody may ask without compiling anything.
    fn warning(&self, around: &crate::Surroundings) -> Option<crate::Objection> {
        let _ = around;
        None
    }

    /// ★★★★★ R1916 — **what a value of this type IS**, in a sentence, or
    /// `None` when the type's own name is the whole of what can be said.
    ///
    /// An associated function and not a method, for
    /// [`composition`](NodeKind::composition)'s reason: what a type is does not
    /// depend on the node that happens to carry it, and two ports of one type
    /// must not be able to disagree about it.
    ///
    /// This is the half the reference's `ConstructBasicPinTooltip` promises in
    /// its comment — "things like the pin's type" — and does not do: read this
    /// round, its base implementation hands the description straight back
    /// unchanged. [`Document::port_tooltip`](crate::Document::port_tooltip)
    /// composes this WITH the port's own sentence, in one place, so a consumer
    /// cannot get a tooltip that knows the port and not the type.
    ///
    /// The default is `None`, so a taxonomy that has nothing to add writes
    /// nothing.
    fn type_description(ty: &Self::Type) -> Option<String> {
        let _ = ty;
        None
    }

    /// ★★★★★ R1913 — **take a composite value apart**, one slot per member of
    /// [`composition`](NodeKind::composition), in the same order.
    ///
    /// A member the value does not determine is `None`, which is not the same
    /// as a member whose value is a default: the first has nothing to put on
    /// its port and the second has something.
    ///
    /// # What forced it, measured
    ///
    /// Splitting a port is not only a question about the TYPE. Measured at
    /// R1913 in the reference's own schema, splitting parses the parent's
    /// authored value and writes one piece onto each member port — so a split
    /// that knew only the members' types would produce ports a reader has to
    /// fill in again.
    ///
    /// The default is empty, so a taxonomy with no composite type writes
    /// nothing.
    fn explode(ty: &Self::Type, value: &Self::Value) -> Vec<Option<Self::Value>> {
        let _ = (ty, value);
        Vec::new()
    }

    /// ★★★★★ R1913 — **put a composite value back together** from one slot per
    /// member, or `None` when the members do not determine one.
    ///
    /// [`explode`](NodeKind::explode)'s other half, and they are declared
    /// **together on the taxonomy** for a reason this crate can point at.
    ///
    /// # Why one declaration rather than two branch lists
    ///
    /// Measured at R1913, the reference writes both halves as hand-written
    /// `if`-chains inside its editor's schema, over **four named struct types**
    /// — and for one of them the two chains use a *different member order*,
    /// with a comment saying so. Every other composite type gets its value
    /// taken apart on the way out and **nothing put back** on the way in.
    ///
    /// Here the taxonomy that owns the type owns both directions, so the pair
    /// is one author's, and [`round_trips`](crate::round_trips) is a law any
    /// consumer can run against its own types — which is the check two
    /// hand-written chains in an editor cannot be given at all.
    fn implode(ty: &Self::Type, members: &[Option<Self::Value>]) -> Option<Self::Value> {
        let _ = (ty, members);
        None
    }

    /// Compute every output from the already-resolved inputs.
    ///
    /// `inputs` is exactly as long as the node's **resolved** input list —
    /// [`Self::inputs`] for a kind with no variadic run, and that list with the
    /// node's items spliced in for one with (R1632), which is what lets a
    /// variadic kind read `inputs` as the run it declared. A `None` slot is an
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
    /// *tree type* (`node tree type::validate_link`) for that reason.
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
    /// *different C struct per socket type* (node socket value float and its
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

    /// Whether these two **nodes** may be wired at all, and why not (R1885).
    ///
    /// ★★★★★ **The question [`NodeKind::conversion`] cannot be asked.** That one
    /// takes two port *types* and no `self`, so a rule written there is blind to
    /// which nodes the wire runs between — and a graph whose whole subject is
    /// whether two *implementations* interoperate has nothing else to say. The
    /// information was never missing: `Document::vet` resolves both nodes'
    /// signatures and throws the node identities away one line before the rule
    /// is consulted. This hook is that line put back.
    ///
    /// # Why a separate judgement and not a wider `conversion`
    ///
    /// A conversion answers *what happens to a value* and is keyed to types, so
    /// widening it would make every taxonomy's type rule carry an argument it
    /// does not use, and would put two unrelated refusals — "no value of this
    /// type may enter" and "these two peers cannot talk" — behind one word an
    /// author cannot act on differently. They are fixed differently: the first
    /// by finding a conversion, the second by changing one of the two ENDS,
    /// which is why [`Refusal`] names which end.
    ///
    /// # The default admits everything, deliberately
    ///
    /// A taxonomy that classifies only its values has no opinion about node
    /// pairs, and must not be made to acquire one to keep compiling. Every
    /// implementor that existed when this landed is unchanged by it.
    ///
    /// Implementations must be **reflexive**: a node is always compatible with
    /// another of its own kind, or a graph cannot be wired to itself.
    fn admits(source: &Self, sink: &Self) -> Admission {
        let (_, _) = (source, sink);
        Admission::Allowed
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
    /// `Sequence` node is a whole execution sequence node class plus an
    /// `FKCHandler_ExecutionSequence` compile handler, which finds its own
    /// output pins by testing whether each pin's name *starts with the string*
    /// `"Then"` and carries the standing admission
    /// `//@TODO: Sort the pins by the number appended to the pin!` — so there,
    /// the order control leaves a Sequence by is the order its pins happen to
    /// sit in the array, and the node's own author noted that this is not the
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
        // R1934 — an undecided end is admitted by everything, because the link
        // is what decides it. Two undecided ends cross as well — that is a
        // reroute chain nothing else has touched yet.
        (Flow::Undecided, _) | (_, Flow::Undecided) => decided_by_the_link(),
    }
}

/// ★ R1934 — a pair that crosses **because the link is what decides it**.
///
/// The same value as a control pair's [`Conversion::Direct`] and a different
/// fact, so it is a named function rather than a shared arm: clippy is right
/// that two arms with one body are one arm, and the repair is to make the
/// bodies say the two different things (R1928's rule — a lint's refusal is a
/// design question, not something to `allow`). Nothing crosses *here*; what
/// happens is that the reroute's ports stop being undecided the moment this
/// link exists, and [`Document::passing_flow`] re-derives them.
const fn decided_by_the_link<V>() -> Conversion<V> {
    Conversion::Direct
}

/// ★★★★★ R1938 — **the shapes a port may hold MANY of a type in.**
///
/// The engine's own three, and they are three rather than one because they
/// differ in what a consumer may assume: an array has an order and repeats, a
/// set has neither, and a map is keyed. A model that offered only "many" would
/// make those three indistinguishable at the port, which is where the
/// difference has to be visible — it is what decides whether wiring one into
/// the other is a conversion or a mistake.
///
/// ⚠ *Not* a container is deliberately absent from this enum rather than being
/// a fourth arm. The reference spells it `None` and then has to carry that
/// arm everywhere a container type is mentioned; here the absence is
/// [`Option::None`] from [`NodeKind::contained`], so "this is not a container"
/// and "this taxonomy has no such container" are one answer instead of two that
/// can disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Container {
    /// Ordered, and repeats are kept.
    Array,
    /// Unordered, and repeats collapse.
    Set,
    /// Keyed by another value.
    Map,
}

impl Container {
    /// Every shape, in a fixed order.
    ///
    /// Derived vocabularies are built from this rather than listed, so a shape
    /// added later joins every register without anyone remembering a second
    /// list — the rule this workspace applies to every closed vocabulary.
    pub const ALL: [Self; 3] = [Self::Array, Self::Set, Self::Map];

    /// The word the wire uses for this shape.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Array => "array",
            Self::Set => "set",
            Self::Map => "map",
        }
    }
}

impl std::fmt::Display for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.word())
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

impl InterfaceSide {
    /// The word this side is published under — the one spelling behind a
    /// refusal's sentence and anything a client reads (R1920, the shape
    /// [`Matched::wire_word`] set).
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
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
    /// **the engine has no unit delay.** State there is a visual script
    /// *variable*: a variable set node writing a property on the object, read
    /// back by a variable get node — arbitrary mutable state, so which value a
    /// read sees depends on where the execution wire happens to have gone, and
    /// the graph's meaning is not a function of the graph. (Its delay node is
    /// a *latent time* delay, not this.) The tradeoff is deliberate here: the
    /// only state is a delay, so a tick's result is a function of the
    /// registers and the inputs, and that is what makes [`Document::tick`]
    /// reproducible.
    Delay(K::Type),
    /// ★★★★★ R1934 — **a bend in a wire**: one port in, one port out, carrying
    /// whatever the graph around it decided, and meaning nothing of its own.
    ///
    /// Both references have this and both spell it as a node, because a canvas
    /// has nowhere else to put it: the DCC registers a `NodeReroute` in its
    /// *layout* class beside the frame, and the engine ships a knot node whose
    /// editor draws it as a control point rather than as a card.
    ///
    /// # It carries no data, and that is the design
    ///
    /// [`Delay`](Self::Delay) holds the type it stores; this holds nothing,
    /// because **its type is derived rather than authored** —
    /// [`Document::passing_flow`] reads it off the chain the reroute belongs
    /// to. Storing it would be a second copy of a fact the links already
    /// answer, free to disagree with them after any edit.
    ///
    /// The DCC does store it (`NodeReroute::type_idname`) and then keeps a
    /// whole-tree pass, `ntree_update_reroute_nodes`, whose job is to make the
    /// stored copy agree with the links again — a disjoint-set union over every
    /// reroute in the tree, run after every update. The engine stores it on the
    /// pins and reaches agreement the other way, by recursive propagation with
    /// a recursion guard "to prevent infinitely recursing if you manage to
    /// create a loop of knots". **Deriving is what makes both of those
    /// unnecessary.**
    ///
    /// # What it inherits, and from where
    ///
    /// Measured on both references, they agree on the rule and reach it by
    /// different machinery:
    ///
    /// * A **chain** of reroutes wired to each other carries ONE flow. (The
    ///   DCC unions them; the engine recurses through them.)
    /// * The **source** side wins: a type arriving at the chain's input decides
    ///   it, and only when there is none does the sink side decide it. (The DCC
    ///   overwrites its `dst` candidate with the `src` one; the engine tries
    ///   `PropagatePinTypeFromDirection(true)` first, twice.)
    /// * With nothing attached, the chain is [`Flow::Undecided`]. (The engine
    ///   "reverts to wildcard"; the DCC keeps its last stored type, which is
    ///   the one place this crate is deliberately stricter — a remembered type
    ///   nothing supports is a fact with no source.)
    ///
    /// # What it is transparent to
    ///
    /// Everything. It never computes ([`Document::evaluate`] routes through it
    /// the way a bypassed node routes), control falls through it, and it is not
    /// an address a name has to be unique against. The engine says the same in
    /// three separate answers — `IsCompilerRelevant() == false`,
    /// `IsNodeSafeToIgnore() == true`, and an `ExpandNode` that splices its two
    /// pin nets together and deletes itself before compilation.
    Reroute,
    /// ★★★★★ R1935 — **a named endpoint**: what arrives here is reachable from
    /// anywhere in this tree by NAME, so a value crosses the canvas with no
    /// edge at all.
    ///
    /// The other half is [`Echo`](Self::Echo). Together they are the engine's
    /// named-reroute pair, and the reason they are two arms rather than one
    /// with a role field is that their **shapes differ**: this one takes a wire
    /// in and gives one out, exactly like [`Reroute`](Self::Reroute), while an
    /// echo has no way in at all.
    ///
    /// # Why this is not a [`Reroute`](Self::Reroute) with a name
    ///
    /// A reroute is [`Naming::Free`](crate::Naming::Free) *because* a point on
    /// a wire is not an address; this one is an address, and that is the whole
    /// of what it is for. So it answers
    /// [`Naming::InTree`](crate::Naming::InTree) and the existing uniqueness
    /// axis (R1932) is what keeps two of them from answering to one name.
    ///
    /// ★ That is one place this crate is stronger than the reference, and it
    /// is measured rather than argued: there a clash is repaired by **silently
    /// renaming** — a private routine walks the tree appending an index until
    /// the name is free — so the name a person typed is not necessarily the
    /// name they get, and nothing tells them. Here the clash is refused and the
    /// refusal says which node already answers to it.
    ///
    /// # Identity is not the name
    ///
    /// An [`Echo`](Self::Echo) names this node by [`NodeId`], not by string.
    /// The reference does the same and says why in a field comment — it keeps a
    /// stable id beside the name "to support copy across graphs" — so the name
    /// is what a person reads and the id is what the graph resolves. Renaming a
    /// beacon therefore cannot orphan its echoes, which is a property, not an
    /// accident.
    Beacon,
    /// ★★★★★ R1935 — **the far end of a name**: this node's output is whatever
    /// the [`Beacon`](Self::Beacon) it names carries, and **no wire runs
    /// between them**.
    ///
    /// It has one port, an output, and no way in — which is the shape that
    /// makes the value's crossing edgeless. The reference's own code says the
    /// same thing twice, in a comment beside the one pin it indexes.
    ///
    /// # It is still a passing node
    ///
    /// Both halves derive their flow from the beacon's input, so a beacon and
    /// every echo of it carry ONE flow — the chain rule of
    /// [`Reroute`](Self::Reroute) with the chain no longer made of links. The
    /// reference reaches this by deriving both from the same base class, whose
    /// only job is finding the declaration by id.
    ///
    /// # A dangling echo is representable, and is therefore diagnosed
    ///
    /// Deleting a beacon leaves its echoes naming a node that is gone. The type
    /// cannot forbid it — the beacon is deleted by a verb that knows nothing of
    /// echoes — so this crate does the next strongest thing and makes
    /// [`Document::validate`] report it. ★ The reference leaves it to a
    /// predicate an editor may call (`IsDeclarationValid`), which is a question
    /// nobody is obliged to ask; a fact that is only true when someone asks is
    /// the shape R1888 recorded.
    Echo(NodeId),
    /// ★★★★★ R2004 — **a node that stands in for several**: a link authored at
    /// it is one link per node it stands for.
    ///
    /// The engine's state-machine alias, and its own baker says the mechanism
    /// in one line — *"Alias's are simply decompiled into multiple
    /// connections."* [`Document::expanded_links`] is that decompilation as a
    /// reading rather than as a step inside a compile, and
    /// [`Document::stands_for`] and [`Document::crowded`] are the rest of it.
    ///
    /// # Its signature is DERIVED, and that is what the expansion needs
    ///
    /// A stand-in presents the signature its members **share**, so its port *n*
    /// is their port *n* and the expansion carries the index across untouched.
    /// Authoring the ports instead would let a stand-in name a port its members
    /// do not have, and the expansion would have nowhere to land. When the
    /// members do not agree it presents **no** ports, so a stand-in over a
    /// mixed group cannot be wired at all — a checked property where the
    /// reference has an unstated one, every state in a state machine having the
    /// same two transition pins by construction.
    ///
    /// # It never stands in for a stand-in
    ///
    /// [`Document::represent`] refuses one, which is what keeps
    /// [`Document::stands_for`] a single step and therefore total. A chain
    /// could otherwise be closed into a ring by two edits neither of which is
    /// wrong on its own, and the crate would be detecting that instead of it
    /// being unrepresentable.
    StandIn(Represented),
}

/// What a [`NodeBody::StandIn`] stands in for (R2004).
///
/// Two arms and not a set beside a flag, which is how the reference spells it:
/// its global-alias flag sits next to the aliased-state set and makes it
/// irrelevant without clearing it, so the document can hold a global alias that
/// also remembers three states — a state with no meaning, which every reader
/// has to know to ignore. Here *whoever is here* is not a list, and the verbs
/// that edit a list say so
/// ([`Document::represent`] answers
/// [`StandInError::StandsForEveryone`](crate::StandInError::StandsForEveryone))
/// rather than writing into a field nothing reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Represented {
    /// Exactly these nodes.
    ///
    /// Members the tree no longer holds stay in the set and are **reported** —
    /// see [`Document::lost_members`]. The reference prunes them on every load
    /// instead, so a deletion elsewhere quietly shrinks what an alias covers.
    Named(BTreeSet<NodeId>),
    /// Whoever is in the tree: every application node and every group instance.
    ///
    /// The reference's global alias, and an arm rather than an enumeration made
    /// today because it stays right as nodes arrive and leave. What it excludes
    /// is this crate's own furniture — the structural arms and the passing ones
    /// — which is not subject matter anybody is quantifying over.
    Everyone,
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

impl Side {
    /// Both sides, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 2] = [Self::Input, Self::Output];

    /// The other one.
    ///
    /// Two arms and no default, so a third side — if a graph ever grows one —
    /// fails to compile here rather than answering something.
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::Input => Self::Output,
            Self::Output => Self::Input,
        }
    }

    /// A stable name, for a caption or a wire form.
    ///
    /// R1642 — these two words were already the wire form in three places
    /// ([`PortRef`]'s `Display`, the schema's `side` vocabulary, and the parse
    /// that reads it back) and were spelled out at each. One definition, so a
    /// client's published vocabulary and the parser that admits it cannot
    /// disagree.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Input => "in",
            Self::Output => "out",
        }
    }

    /// Parse a wire name back to the side, or `None` — the inverse of
    /// [`name`](Self::name).
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// ★★★★★ R1987 — the side as an **English noun**, for a sentence a person
    /// reads.
    ///
    /// Not [`name`](Self::name), and the difference is a category rather than a
    /// preference. `name` is the **wire form**: it has a parser inverse
    /// ([`from_wire`](Self::from_wire)), it is what a client sends and what a
    /// schema publishes, and it is therefore frozen by every consumer that
    /// parses it. Rendering it into prose produced *"node 1 has no in pin to
    /// take the wire"* — which is how this method came to exist, caught by
    /// [`AutowireError`](crate::AutowireError)'s own census proof rather than by
    /// review.
    ///
    /// So the two must be able to move apart: a wire token cannot be improved
    /// for a reader without breaking a parser, and a sentence cannot be fixed
    /// by editing a protocol. One caller today — R1987's refusal — and that is
    /// the right time to separate them, because the conflation has already
    /// produced one wrong sentence.
    #[must_use]
    pub const fn noun(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }

    /// The closed vocabulary, projected from [`ALL`](Self::ALL) — see
    /// [`ArrangePass::WIRE_NAMES`](crate::ArrangePass::WIRE_NAMES).
    pub const WIRE_NAMES: [&'static str; 2] = {
        let mut out = [""; 2];
        let mut i = 0;
        while i < 2 {
            out[i] = Self::ALL[i].name();
            i += 1;
        }
        out
    };
}

/// Neither side's name is a prefix of the other's (R1644).
///
/// A composite address writes a side and an index with no separator
/// (`in0`, `out3` — [`PortRef`]'s `Display`), so reading one back means finding
/// which name the text starts with. That is only unambiguous while this holds,
/// and it is a property of the **vocabulary**, so it is checked where the
/// vocabulary is rather than inside each parse that depends on it. Renaming a
/// side to something the other starts with fails the build here.
const _: () = {
    let mut outer = 0;
    while outer < Side::ALL.len() {
        let mut inner = 0;
        while inner < Side::ALL.len() {
            if outer != inner {
                let one = Side::ALL[outer].name().as_bytes();
                let other = Side::ALL[inner].name().as_bytes();
                let mut same = one.len() <= other.len();
                let mut at = 0;
                while same && at < one.len() {
                    same = one[at] == other[at];
                    at += 1;
                }
                assert!(!same, "one side's wire name is a prefix of another's");
            }
            inner += 1;
        }
        outer += 1;
    }
};

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

    /// The port at `index` on `side`, for a caller that already holds the side
    /// as a value rather than as a choice between two calls.
    #[must_use]
    pub const fn new(side: Side, index: u32) -> Self {
        Self { side, index }
    }
}

impl fmt::Display for PortRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.side.name(), self.index)
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
    /// ★★★★★ R1923 — **a sentence a person wrote about THIS node**, or `None`
    /// to say whatever its kind says.
    ///
    /// The same shape as [`label`](Self::label) one field up, and for the same
    /// reason: a rename does not stop a node being a multiply, and a note
    /// written on one node does not become the kind's own description. Which
    /// of the two a reader is being shown is [`Described`]'s answer.
    #[serde(default)]
    pub description: Option<String>,
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
    /// Whether the node is **switched off**: it does not run, and nothing comes
    /// out of it (R1682).
    ///
    /// ★★ **Not the same request as [`bypassed`](Node::bypassed), and the
    /// difference is what reaches the nodes downstream.** A bypassed node is
    /// asked not to *compute* and its inputs travel straight through it, so the
    /// graph below carries on unaffected; a disabled one is asked not to *be
    /// there*, so its outputs are empty and everything that depended on it
    /// reads nothing. An editor needs both — "route around this" and "switch
    /// this off" are different intentions — and a model with only the first
    /// makes the second unsayable, which is how it ends up in application state
    /// where no derivation can see it.
    ///
    /// A second fact about the graph's *meaning*, so it is a field beside
    /// `bypassed` rather than a bit in [`Appearance`], which is looks only.
    ///
    /// **It does not cascade through containment.** The reference toolkit's
    /// `setEnabled` does, because its tree is the interaction tree and a child
    /// of a disabled widget cannot be reached either. A frame in a graph is a
    /// grouping on a canvas, and the relation along which "this is not running"
    /// actually travels is the FLOW — which is derived rather than authored:
    /// a node fed only by disabled ones reads `None` without anybody having
    /// marked it. Copying the containment cascade here would author a fact the
    /// flow already answers, and the two would be free to disagree.
    #[serde(default)]
    pub disabled: bool,
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
    /// **forest**, and that is the invariant [`Document::set_parent`]
    /// maintains and [`Document::validate`] checks. The DCC declares the same
    /// field as a bare `node *parent` and enforces its two rules — parent is a
    /// frame, and no node contains itself — with `BLI_assert`, which is
    /// compiled out of the build it ships.
    #[serde(default)]
    pub parent: Option<NodeId>,
    /// Values authored on **this node's** ports (R1594).
    ///
    /// A port's type and its name come from the kind, so every node of a kind
    /// shares them. Its *value* does not: two `Swatch` nodes are two different
    /// colours, and the number a user typed into an unwired input belongs to
    /// that input and to no other node's. The DCC keeps exactly this, as
    /// `node socket::default_value`, per socket per node.
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
    /// The **items** of this node's variadic runs (R1632).
    ///
    /// A port's existence comes from the kind, for every node but the ones
    /// whose kind says otherwise: a sequencer with four branches and one with
    /// two are the same kind, and the difference is here. Empty means nothing
    /// has been authored, which resolves to the kind's declared minimum — see
    /// [`Items`].
    ///
    /// Edited through [`Document::insert_item`], [`Document::remove_item`] and
    /// [`Document::move_item`], never in place: an item's position **is** a
    /// range of port indices, so changing this field without moving the links
    /// and the values that address those indices re-points wires at the wrong
    /// sockets. That is the defect the engine ships with a `//@TODO` beside it.
    #[serde(default, skip_serializing_if = "Items::is_empty")]
    pub items: Items<K::Type>,
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

/// ★★★★★ R1919 — a node a search reached, and the way in to it.
///
/// Separate from a bare [`NodeId`] because an id alone is not an answer to
/// *where is it*: this crate's ids are unique **within a tree**, so a search
/// that crosses trees must say which tree, and a reader who has to go there
/// must be told what to open. See [`Document::find`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The node that matched, in the tree [`at`](Self::at) has reached.
    pub node: NodeId,
    /// ★★★★★ **Where it is** — the way in, as this crate's own editing
    /// position.
    ///
    /// An [`EditPath`] rather than a list of ids, and that is the decision
    /// worth the paragraph. It already answers everything a reader of this
    /// needs: `current()` is the tree the hit lives in, `depth()` is how far
    /// away it is, `entries()` is each group descended together with the tree
    /// it was descended FROM, and `breadcrumb(document)` is the whole way in
    /// by name. Nothing here re-derives any of that.
    ///
    /// And a caller can hand it **straight to its editor**: the move both
    /// references perform, and neither publishes, is a VALUE here.
    ///
    /// ⚠ Two earlier drafts of this round invented a type for it — first a
    /// bare `Vec<NodeId>`, which is not an address at all because an id is
    /// unique only within its tree, then a pair carrying that tree — and the
    /// compiler refused both names in turn (`Step` is already one step of a
    /// RUN, `Descent` one level of an EVALUATION). The collisions were the
    /// tell rather than an annoyance: this crate's word for *where in the
    /// nesting you are* already existed, and the third draft stopped inventing.
    pub at: EditPath,
    /// Which name matched — see [`Matched`].
    pub because: Matched,
    /// The name that matched, as a reader sees it. Carried so a result list can
    /// be shown without re-resolving each hit against the document.
    pub shown: String,
}

/// ★★★★★ R1920 — an edit a caller is **about to** make, so it can be asked
/// about before it happens. See [`Document::may`].
///
/// # Why the subject is inside the act rather than beside it
///
/// The census's eight permission rows read as two axes — a SUBJECT (this node,
/// or this graph) times a VERB — and the first draft of this type took them as
/// two arguments. That made `may(tree, None, Act::Delete)` expressible, which
/// is a question with no subject, and `may(tree, Some(node), Act::Create)`,
/// which names a subject the verb has no use for. Both are nonsense the
/// compiler would have had to be told about. Carrying the subject in the arm
/// that needs it makes each act carry exactly what it is about — R1891's rule,
/// and the same reason [`Found::at`] is an `EditPath` rather than a pair.
///
/// # Why the VALUE is carried too
///
/// A rename is refused for the name it would take, so a permission question
/// that left the name out could only answer half of what it was asked and the
/// caller would still have to try it to learn the rest. Both references split
/// exactly there — a `Can…Rename` predicate for the permission and a separate
/// name validator for the value — and a caller has to consult two things and
/// combine them itself.
// ⚠ `PartialEq` but not `Eq`: an act can carry a body, a body carries the
// application's own values, and the commonest value a node graph carries is a
// float. That is the same reason `NodeKind::Value` is `PartialEq` and not `Eq`.
//
#[derive(Debug, Clone, PartialEq)]
pub enum Act<'a, K: NodeKind> {
    /// ★★★★★ R1922 — put **this body** into this tree.
    ///
    /// R1920 built this arm carrying nothing, which answered only *is this
    /// tree here*. The census's four remaining rows on this axis all ask the
    /// sharper question — the DCC's own comment on the hook is *can this node
    /// be added to a node tree?* — and it cannot be answered without knowing
    /// what is being added. Carrying the body is the same decision
    /// [`Act::Rename`] makes about a name: a question that leaves the value
    /// out can only answer half of what it was asked.
    Create(&'a NodeBody<K>),
    /// Take this node out of it.
    Delete(NodeId),
    /// Give this node this authored name, or take its name away with `None`.
    Rename(NodeId, Option<&'a str>),
}

// ⚠ `Copy` is written out while `Clone` is derived, and the asymmetry is the
// point: a derived `Copy` would put `K: Copy` on the impl, which no application
// kind has to satisfy — this type holds only references and ids, so it is
// copyable whatever the kind is. `Clone` derives cleanly because `NodeKind`
// already requires `Clone`, so the bound a derive adds is one every kind meets.
impl<K: NodeKind> Copy for Act<'_, K> {}

/// ★★★★★ R1923 — a node's description, and **which of its two sources said
/// it**.
///
/// # Why the source travels with the sentence
///
/// The reference answers this question with a bare string: its node tooltip
/// hook returns text and its own default implementation returns the class's,
/// so a caller there is handed one value and cannot tell *a person wrote this
/// about this node* from *this is what nodes of this sort are*. Those are
/// different facts to a reader and to an editor — the first is editable and
/// belongs to this node, the second is not and belongs to every node of the
/// kind — and an interface that flattens them makes "clear the note" and "the
/// kind has nothing to say" indistinguishable.
///
/// Carrying the source is R1919's [`Matched`] decision on a different axis: the
/// answer says which question it answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description {
    /// The sentence itself.
    pub sentence: String,
    /// Who said it.
    pub source: Described,
}

/// ★ R1923 — where a node's description came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Described {
    /// A person wrote it on this node.
    Authored,
    /// The node's kind says it about every node of its sort.
    Kind,
}

impl Described {
    /// The word this source is published under.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Authored => "authored",
            Self::Kind => "kind",
        }
    }
}

/// ★ R1919 — why a node answered a search.
///
/// Named rather than left implicit because the two are different facts about
/// the graph: a node matched by its **authored label** was called that by a
/// person, and one matched by its **kind's own word** was not called anything.
/// A reader shown one list of hits cannot tell them apart otherwise, and the
/// second kind is the majority of any real graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matched {
    /// The name a person authored on this node.
    Label,
    /// The word the node's body describes itself by.
    Kind,
}

impl Matched {
    /// The word this reason is published under.
    #[must_use]
    pub const fn wire_word(self) -> &'static str {
        match self {
            Self::Label => "label",
            Self::Kind => "kind",
        }
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
            // R1934 — both references call it this, in their own words: the
            // DCC's node type is named "Reroute" and the engine's knot node
            // titles itself "Reroute Node".
            NodeBody::Reroute => "Reroute".to_owned(),
            // R1935 — a beacon almost always HAS a label, because its name is
            // its whole purpose; this is what an unnamed one shows, and it
            // reads as the invitation it is.
            NodeBody::Beacon => "Named".to_owned(),
            // ⚠ An echo shows the beacon's name and not its own, which no arm
            // here can do — `display_name` is a method on the node and an echo
            // has to reach the document. `Document::echo_display_name` is the
            // reading that resolves it; this is the fallback for a dangling
            // one, and it says so rather than showing a blank card.
            NodeBody::Echo(_) => "Echo".to_owned(),
            // ★ R2004 — the reference's own default for a fresh alias card, in
            // its own word, and a stand-in almost always carries a label for
            // the same reason a beacon does: what it stands for is the thing a
            // person is naming. Its canned verb writes `Self`, which is why
            // that word is `Document::stand_in_for`'s and not this arm's.
            NodeBody::StandIn(_) => "Alias".to_owned(),
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
    ///
    /// ★★★★★ R1998 added `adopting`, because a **stand-in** is a copy of a node
    /// whose body it does not share: the paste's substitution hook answers with
    /// another kind entirely, and the three fields that belong to the *kind* —
    /// its appearance, its port values, its items — describe a body this node
    /// no longer has. See [`Adopting`].
    pub(crate) fn adopt_from(&mut self, source: &Self, adopting: Adopting) {
        let Self {
            id: _,
            body: _,
            x: _,
            y: _,
            label,
            description,
            bypassed,
            disabled,
            appearance,
            parent: _,
            values,
            items,
        } = source;
        self.label.clone_from(label);
        // R1923 added `description`, and the answer for it is **yes**, for the
        // same reason as `label`: it is a sentence a person wrote about this
        // node, so a copy that arrived without it would have silently dropped
        // what somebody said. The kind's own description is not copied because
        // it is not stored — the copy has the same kind and asks it too.
        self.description.clone_from(description);
        self.bypassed = *bypassed;
        // R1682 added `disabled`, and the answer for it is **yes**, for the
        // same reason as `bypassed`: a copy of a node somebody switched off
        // that arrived switched back on would run something nobody asked to
        // run. Being switched off travels with the node, exactly like being
        // bypassed does.
        self.disabled = *disabled;
        if adopting == Adopting::Everything {
            self.appearance.clone_from(appearance);
            self.values.clone_from(values);
            // R1632 added `items`, and the answer for it is **yes**, for the same
            // reason as `values` and not `parent`: an item names nothing outside
            // this node, and a duplicated four-branch sequencer that came back with
            // two branches would lose the wiring the copy was made to keep.
            self.items.clone_from(items);
        }
    }
}

/// ★★★★★ R1998 — how much of the node it came from a copy takes.
///
/// A copy of a node takes everything; a **stand-in** takes only what a person
/// wrote. The split is *who authored it*: a label and a note are a person's
/// sentences about this node and mean the same thing whatever body sits under
/// them, while an appearance, a port's held value and an authored item all
/// describe the body itself and mean nothing under another one.
///
/// Bypassed and disabled are on the person's side of the line for the same
/// reason [`Node::adopt_from`] gives for copying them at all: switching a node
/// off is something somebody did to it, not something its kind knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Adopting {
    /// A copy of the same body: everything travels.
    Everything,
    /// A stand-in for a body that could not land: only what a person wrote.
    WhatAPersonWrote,
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
    /// ★★★★★ R1925 — the named, collapsible runs the ports are gathered into.
    ///
    /// `#[serde(default)]` on both of these, and no [`REVISION`](crate::REVISION)
    /// bump: an archive written before sections existed still opens, and reads
    /// as a face with none. Bumping would refuse it, which is the opposite of
    /// what a backwards-compatible addition owes.
    #[serde(default = "Vec::new")]
    pub(crate) sections: Vec<crate::section::Section>,
    /// The next section id to hand out. Kept rather than derived from the list's
    /// length, so removing a section cannot make the next one collide with a
    /// member reference that outlived it.
    #[serde(default)]
    pub(crate) next_section: u32,
}

impl<K: NodeKind> Default for Interface<K> {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            sections: Vec::new(),
            next_section: 0,
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

/// ★★★★★ R1997 — one node a tree of some taxonomy is born holding, and where
/// it sits.
///
/// The position travels with the body because the reference's own overriders
/// place theirs — a sound cue's root at `y = -58`, a custom transition's pose
/// evaluators at `x = ±300` — and a seed that arrived at the origin would put
/// every opening node of a three-node tree on top of the others.
#[derive(Debug, Clone, PartialEq)]
pub struct Seed<K: NodeKind> {
    /// What to make.
    pub body: NodeBody<K>,
    /// Where to put it, in canvas units.
    pub at: (i32, i32),
}

impl<K: NodeKind> Seed<K> {
    /// One opening node at the canvas origin.
    #[must_use]
    pub const fn new(body: NodeBody<K>) -> Self {
        Self { body, at: (0, 0) }
    }

    /// The same, placed.
    #[must_use]
    pub const fn at(mut self, x: i32, y: i32) -> Self {
        self.at = (x, y);
        self
    }
}

/// ★★★★★ R1997 — what [`Document::open_definition`] made.
///
/// The reference's hook answers `void`, so every overrider that needs the node
/// afterwards writes it down a second time on its own graph type. This is that
/// answer, given once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Born {
    /// The definition that was made.
    pub tree: TreeId,
    /// The nodes it was born holding, in the order the taxonomy declared them.
    pub nodes: Vec<NodeId>,
}

/// One tree: the root document graph, or a re-usable group definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Serialize, K::Type: Serialize, K::Value: Serialize, K::Graph: Serialize",
    deserialize = "K: Deserialize<'de>, K::Type: Deserialize<'de>, K::Value: Deserialize<'de>, \
                   K::Graph: Deserialize<'de>"
))]
pub struct Tree<K: NodeKind> {
    /// Which tree this is.
    pub id: TreeId,
    /// A human-facing name. For a definition this is what the palette shows.
    pub name: String,
    /// ★★★★★ R1999 — **what kind of graph this is**, in the taxonomy's own
    /// vocabulary ([`NodeKind::Graph`]).
    ///
    /// Stored rather than derived from where the document keeps the tree, which
    /// is how the reference's own answer is computed: it walks up the graph's
    /// owner chain and reports which of three lists holds it. That derivation
    /// has no answer for a graph in none of them and returns *function* — so
    /// there, *this is a function graph* and *I could not classify this* are one
    /// value. Stored, they are not.
    ///
    /// `serde` default, so every document written before this field existed
    /// reads back as the taxonomy's unchosen kind rather than failing to load.
    #[serde(default)]
    pub(crate) kind: K::Graph,
    #[serde(with = "node_map")]
    nodes: BTreeMap<NodeId, Node<K>>,
    links: Vec<Link>,
    interface: Interface<K>,
    /// ★★★★★ R1933 — the socket types this tree admits on its interface.
    ///
    /// `Anything` by default and by `serde` default, so every document written
    /// before this field existed reads back as the unrestricted tree it was.
    #[serde(default = "crate::Admitted::default")]
    pub(crate) admitted: crate::Admitted<K::Type>,
    /// ★★★★★ R1997 — the nodes this tree was BORN holding, in the order
    /// [`NodeKind::opening`] declared them.
    ///
    /// Recorded rather than re-derived, because *what a tree was born with* is
    /// a fact about its history and nothing in its present shape carries it: a
    /// node the taxonomy seeded and a node a person placed a moment later are
    /// indistinguishable by inspection. The reference records the same fact as
    /// per-node metadata; a list on the tree is the same information in the
    /// place that is asked about it.
    ///
    /// `serde` default empty, so every document written before this field
    /// existed reads back as a tree nobody claims to have seeded.
    #[serde(default)]
    born: Vec<NodeId>,
    /// ★★★★★ R1943 — which node CLOSES the zone each opening node opens.
    ///
    /// ⚠ ONE map, from the opener to the closer, and the reverse look-up is
    /// derived rather than stored. That is the decision: two maps could
    /// disagree about one pair, and a pairing that is true in one direction and
    /// not the other is exactly the state R1891's rule says to make
    /// unrepresentable rather than to check for.
    ///
    /// ★ Measured against the reference, which stores the pairing on the
    /// OPENING NODE as the closer's id and nothing on the closer. So *what
    /// closes this?* is a field read there and *what does this close?* is a
    /// scan of every opening node in the tree — its own pairing routine does
    /// exactly that scan to find out whether a closer is already spoken for.
    /// One map here has the same asymmetry in cost and none of it in TRUTH.
    ///
    /// ⚠⚠ **R2003 — the sentence that used to end this paragraph was false.**
    /// It read *a dangling id cannot outlive the node, because
    /// [`Document::unpair`] and node removal both go through the map*, and node
    /// removal did not go near the map: driven, removing a closer left its
    /// opener answering `Opens(<a node that is not there>)` while
    /// [`Document::validate`] reported nothing. What is true now is a different
    /// claim and a weaker one, which is why it is worth writing down — **this
    /// map is a CLAIM, and [`Document::standing_zones`] is the truth about it.**
    /// A pairing is honoured only while both ends are present and the opener's
    /// kind still declares the closer's, so a lapsed one is never reported;
    /// [`Document::remove_node`] and [`Document::set_kind`] additionally drop
    /// it, so it cannot come back when an end is swapped back.
    #[serde(default = "BTreeMap::new", skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) zones: BTreeMap<NodeId, NodeId>,
    next_node: u32,
    next_link: u32,
}

impl<K: NodeKind> Tree<K> {
    /// Every node, ascending by id.
    /// ★ R1922 — place a node WITHOUT asking [`Document::admits`], which is
    /// what reading a document from a file does.
    ///
    /// Test-only and named so, because the point of `admits` is that every
    /// EDIT goes through it. What does not is a whole document arriving from
    /// outside, and `Document::validate` exists for exactly that — so a test
    /// showing validate still reports a state needs a way to build the state
    /// the way a file would, rather than the way an editor cannot.
    #[cfg(test)]
    pub(crate) fn insert_node_for_test(&mut self, node: Node<K>) {
        self.nodes.insert(node.id, node);
    }

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

    /// ★ R1934 — one link for modification, crate-private.
    ///
    /// The one edit a caller may make through it is moving an **end**, and the
    /// one verb that does is [`Document::insert_reroutes`], which re-points a
    /// cut link's source at the reroute it just made rather than deleting the
    /// link and building a new one — so the link a caller was holding is still
    /// the link it is holding. Crate-private because a link's ends were vetted
    /// when it was made ([`Document::vet`]) and an application moving one
    /// behind that check could seat a value on a control port.
    pub(crate) fn link_mut(&mut self, id: LinkId) -> Option<&mut Link> {
        self.links.iter_mut().find(|l| l.id == id)
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
    serialize = "K: Serialize, K::Type: Serialize, K::Value: Serialize, K::Graph: Serialize",
    deserialize = "K: Deserialize<'de>, K::Type: Deserialize<'de>, K::Value: Deserialize<'de>, \
                   K::Graph: Deserialize<'de>"
))]
pub struct Document<K: NodeKind> {
    trees: Vec<Tree<K>>,
    /// ★★★★★ R1944 — one past the highest [`TreeId`] this document has EVER
    /// handed out.
    ///
    /// A field and not a derivation, and that is the whole of it: derived from
    /// the trees that remain, a removal hands the id back, and the next
    /// definition is minted with an id that every `NodeBody::Group` naming the
    /// removed one would silently start naming. `Tree` keeps `next_node` and
    /// `next_link` for exactly this reason and has since the beginning; trees
    /// were the one collection whose ids were positions, which is why nothing
    /// could be removed from it.
    ///
    /// ⚠ `serde` default is 0, and [`Document::next_tree_id`] is held to the
    /// highest id the trees actually carry — so a file written before this
    /// field existed cannot mint a colliding id either.
    #[serde(default)]
    next_tree: u32,
    /// R1645 — the links a source **reported**, which are not links.
    ///
    /// Deliberately not in [`Tree::links`], and that placement is the whole
    /// guarantee: every derivation in this crate walks a tree's links, so an
    /// observation cannot reach one by accident. A layer tag on [`Link`] would
    /// have put reported edges inside the structure they must stay out of,
    /// where only care keeps them there — the argument R1644 made for keeping
    /// breakpoints out of the run.
    ///
    /// `default` so a document written before this round loads.
    #[serde(default = "BTreeSet::new")]
    observed: BTreeSet<crate::observed::Observation>,
    /// Whether links are allowed to arrive undrawn at all (R1645).
    #[serde(default)]
    discovery: crate::observed::Discovery,
}

/// The root tree, which always exists.
pub const ROOT: TreeId = TreeId(0);

impl<K: NodeKind> Document<K> {
    /// R1645 — the reported links, for [`observed`](crate::observed) to read.
    ///
    /// The field is private to this module, and these five are the whole of
    /// what reaches it: nothing in this crate can walk the observations by
    /// accident, which is the placement's entire point.
    pub(crate) fn reports(&self) -> impl Iterator<Item = &crate::observed::Observation> {
        self.observed.iter()
    }

    pub(crate) fn record(&mut self, what: crate::observed::Observation) -> bool {
        self.observed.insert(what)
    }

    pub(crate) fn forget(&mut self, what: &crate::observed::Observation) -> bool {
        self.observed.remove(what)
    }

    pub(crate) fn forget_tree(&mut self, tree: TreeId) -> usize {
        let had = self.observed.len();
        self.observed.retain(|one| one.tree != tree);
        had - self.observed.len()
    }

    pub(crate) const fn discovery_setting(&self) -> crate::observed::Discovery {
        self.discovery
    }

    pub(crate) const fn set_discovery_setting(
        &mut self,
        discovery: crate::observed::Discovery,
    ) -> crate::observed::Discovery {
        std::mem::replace(&mut self.discovery, discovery)
    }

    /// A document holding one empty root tree.
    #[must_use]
    pub fn new(root_name: impl Into<String>) -> Self {
        Self {
            observed: BTreeSet::new(),
            discovery: crate::observed::Discovery::Off,
            // The root is 0, so the next id is 1.
            next_tree: 1,
            trees: vec![Tree {
                id: ROOT,
                name: root_name.into(),
                // ⚠ R1999 — the root is of the taxonomy's unchosen kind, which
                // is the taxonomy's own word for "the ordinary graph here" and
                // not this crate's. `set_graph_kind` re-classifies it.
                kind: K::Graph::default(),
                nodes: BTreeMap::new(),
                links: Vec::new(),
                interface: Interface::default(),
                admitted: crate::Admitted::Anything,
                zones: BTreeMap::new(),
                // ⚠ The ROOT is born empty and stays that way. A taxonomy's
                // opening is consulted by `open_definition`, which makes a
                // DEFINITION — the reference likewise seeds the graphs its
                // editors create and not the document they live in.
                born: Vec::new(),
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
    ///
    /// ⚠★★★★★ R1944 — a SEARCH, not `trees[id]`. A tree's id was its position
    /// for as long as nothing could be removed, and
    /// [`Document::remove_definition`] ends that: after one removal every id
    /// past the gap would name the wrong tree. The cost is a scan of a
    /// collection that holds one entry per DEFINITION — a handful, not a graph
    /// — and the alternative is an identity that quietly changes meaning.
    #[must_use]
    pub fn tree(&self, id: TreeId) -> Option<&Tree<K>> {
        self.trees.iter().find(|held| held.id == id)
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

    /// One tree for modification. A search, for [`Document::tree`]'s reason.
    pub fn tree_mut(&mut self, id: TreeId) -> Option<&mut Tree<K>> {
        self.trees.iter_mut().find(|held| held.id == id)
    }

    /// The id [`Self::add_definition`] would hand out next.
    ///
    /// Public to this crate so a plan can name a tree it has not created yet —
    /// which is what lets an insertion decide, *before mutating anything*,
    /// whether the definitions it is about to add would close a containment
    /// cycle. It is the same expression the allocation uses rather than a second
    /// copy of it, so the two cannot drift.
    #[must_use]
    /// ⚠★★★★★ R1944 — ONE PAST THE HIGHEST EVER HANDED OUT, not the count.
    ///
    /// It was `trees.len()` while nothing could be removed, and those two
    /// agreed for exactly as long as that held. `Document::remove_definition`
    /// ends it: after one removal the count is behind the highest id, so the
    /// next tree would be minted with an id a live tree already has — and every
    /// `NodeBody::Group` naming the removed one would silently start naming the
    /// new one.
    pub(crate) fn next_tree_id(&self) -> TreeId {
        TreeId(self.next_tree.max(self.tree_frontier()))
    }

    /// One past the highest id any tree currently here holds.
    ///
    /// The floor `next_tree` is held to, so a document that arrived from a file
    /// without the field — or one whose frontier was raised by taking on
    /// another document's ids — still cannot mint a colliding id.
    fn tree_frontier(&self) -> u32 {
        self.trees
            .iter()
            .map(|held| held.id.0)
            .max()
            .map_or(0, |highest| highest.saturating_add(1))
    }

    /// ★ R1944 — mint the next tree id, moving the frontier past it.
    ///
    /// ⚠ Goes through [`Document::next_tree_id`] rather than computing its own
    /// answer, and that is not tidiness: written as a second computation it was
    /// a SECOND PATH, and a counterfactual that broke `next_tree_id` was caught
    /// by nothing because every tree here is minted through this one. Two ways
    /// to answer "what id comes next" is exactly the drift this field exists to
    /// stop.
    fn mint_tree_id(&mut self) -> TreeId {
        let id = self.next_tree_id();
        self.next_tree = id.0.saturating_add(1);
        id
    }

    /// Copy a tree wholesale and answer the copy's id.
    ///
    /// The copy keeps the original's name: a name is not an identity here, so
    /// two definitions may share one. The DCC must rename a copied node group
    /// (`Sum` becomes `Sum.001`) because an ID's name *is* its key.
    pub(crate) fn copy_tree(&mut self, source: TreeId) -> Option<TreeId> {
        let mut copy = self.tree(source)?.clone();
        let id = self.mint_tree_id();
        copy.id = id;
        self.trees.push(copy);
        Some(id)
    }

    /// ★★★★★ R1997 — **a new definition, born holding what its taxonomy says a
    /// tree holds** — and it says what it made.
    ///
    /// [`NodeKind::opening`] is the declaration; this is the one place that
    /// consults it. Compare [`add_definition`](Self::add_definition), which
    /// makes an EMPTY tree and is what every verb that fills one from a
    /// selection uses.
    ///
    /// # The three measured ways this passes the reference
    ///
    /// Its `CreateDefaultNodesForGraph(Graph)` returns **void**. So:
    ///
    /// 1. ★★★★★ **Nothing says what it made.** Every overrider that needs the
    ///    node afterwards writes it down a SECOND time on its own graph type
    ///    (`TypedGraph->MyResultNode`) — one fact in two places, and the two are
    ///    maintained by different lines of the same function. [`Born::nodes`] is
    ///    the answer, so no caller has to keep its own copy.
    /// 2. ★★★★★ **The tree remembers, so the question survives the call.** The
    ///    reference marks each node in the package's global metadata map and
    ///    then walks every node in the graph asking, twice, in two functions
    ///    with two different rules — one requires the node to be enabled and
    ///    the other does not. Here it is [`Document::opening_nodes`] and
    ///    [`Document::untouched`], derived from one recorded list.
    /// 3. **A tree born with nothing is legitimate**, which is the reference's
    ///    own base body, and is what a taxonomy that declares no opening gets.
    pub fn open_definition(&mut self, name: impl Into<String>) -> Born {
        let tree = self.add_definition(name);
        let mut nodes = Vec::new();
        for seed in K::opening() {
            // ⚠ A refused seed is SKIPPED rather than aborting the birth, and
            // that is stated rather than implied: `add_node` refuses only a
            // tree that is not there or a body the tree does not admit, and the
            // tree was made a line above — so a taxonomy whose opening is
            // refused has declared something its own document will not hold,
            // which is its defect and not a reason to hand back no tree.
            if let Ok(id) = self.add_node(tree, seed.body, seed.at.0, seed.at.1) {
                nodes.push(id);
            }
        }
        if let Some(host) = self.tree_mut(tree) {
            host.born.clone_from(&nodes);
        }
        Born { tree, nodes }
    }

    /// ★★★★★ R1997 — the nodes a tree was **born** holding that are still in
    /// it, ascending.
    ///
    /// Empty for a tree that was born empty *and* for one whose opening nodes
    /// have all been taken out — [`untouched`](Self::untouched) is the question
    /// that tells those apart from a tree somebody has added to.
    #[must_use]
    pub fn opening_nodes(&self, tree: TreeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut still: Vec<NodeId> = host
            .born
            .iter()
            .copied()
            .filter(|node| host.node(*node).is_some())
            .collect();
        still.sort_unstable();
        still
    }

    /// ★★★★★ R1997 — **has anyone done anything to this tree yet?**
    ///
    /// True while it holds exactly what it was born with and nothing has been
    /// wired. This is what the reference's marker is FOR, measured at its
    /// readers: a blueprint editor asks it to decide what to TELL a person —
    /// an untouched graph is offered *drag off pins to create nodes* and a
    /// touched one is not, and the hint fades on the first placement.
    ///
    /// ⚠ Links count. The reference's `GraphHasUserPlacedNodes` asks only about
    /// nodes, so a graph whose two born nodes someone had wired together still
    /// answers *untouched* there — which is the wrong answer to the question it
    /// is being asked on behalf of.
    #[must_use]
    pub fn untouched(&self, tree: TreeId) -> bool {
        let Some(host) = self.tree(tree) else {
            return false;
        };
        host.links().is_empty() && host.nodes().count() == self.opening_nodes(tree).len()
    }

    /// Add an empty group definition and answer its id.
    ///
    /// A definition created this way has no interface and no instances; it
    /// becomes reachable when something instantiates it.
    ///
    /// ⚠ It is **empty**: [`open_definition`](Self::open_definition) is the one
    /// that consults [`NodeKind::opening`]. Every verb that fills a definition
    /// from a selection — `group`, `insert`, the fragment verbs — uses this
    /// one, because a tree that is about to receive nodes must not also be
    /// seeded with them.
    ///
    /// ⚠ R1999 — the definition is of the taxonomy's **unchosen** graph kind.
    /// [`add_definition_of`](Self::add_definition_of) is the one that says.
    pub fn add_definition(&mut self, name: impl Into<String>) -> TreeId {
        self.add_definition_of(name, K::Graph::default())
    }

    /// ★★★★★ R1999 — add an empty group definition **of a stated graph kind**.
    ///
    /// [`add_definition`](Self::add_definition) is this with the taxonomy's
    /// unchosen kind, so there is one construction and not two — and a kind
    /// given at birth is a kind no node was ever placed under the wrong answer
    /// to, which is what setting it afterwards cannot promise.
    pub fn add_definition_of(&mut self, name: impl Into<String>, kind: K::Graph) -> TreeId {
        let id = self.mint_tree_id();
        self.trees.push(Tree {
            id,
            name: name.into(),
            kind,
            nodes: BTreeMap::new(),
            links: Vec::new(),
            interface: Interface::default(),
            admitted: crate::Admitted::Anything,
            zones: BTreeMap::new(),
            born: Vec::new(),
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

    /// ★ R1944 — drop a tree and everything in it, with no questions asked.
    ///
    /// Crate-private on purpose: the questions are
    /// [`Document::remove_definition`]'s, and a caller reaching this directly
    /// would be re-deciding them. That is the seam the reference does not have
    /// — its editor's delete path asks a schema, falls back to its own
    /// procedure, and is itself the public surface.
    pub(crate) fn drop_tree(&mut self, id: TreeId) {
        self.trees.retain(|held| held.id != id);
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
    ///
    /// ★★★★★ R1914 — **a split is spliced in here**, after the node's variadic
    /// items and for the same reason: it is a per-node declaration that changes
    /// how many ports the node presents. A split port keeps its place and its
    /// member ports follow it, so an index after a split moves — which is why
    /// the declaration is written in [`PortPath`](crate::PortPath)s and the
    /// verbs report what moved.
    #[must_use]
    pub fn signature(&self, tree: TreeId, node: NodeId) -> Option<Signature<K>> {
        let mut signature = self.declared_signature(tree, node)?;
        self.splice_splits(tree, node, &mut signature);
        Some(signature)
    }

    /// R1914 — the signature **before any split is spliced in**.
    ///
    /// What a [`PortPath`](crate::PortPath)'s root index counts against, and
    /// therefore the one reading that does not move when a neighbouring port
    /// comes apart. Every caller outside the split machinery wants
    /// [`signature`](Document::signature) instead.
    pub(crate) fn declared_signature(&self, tree: TreeId, node: NodeId) -> Option<Signature<K>> {
        let host = self.tree(tree)?;
        let node = host.node(node)?;
        Some(match &node.body {
            // R1632 — a kind's own lists are the FIXED part, and the node's
            // items are spliced into them. A kind that declares no run splices
            // nothing, which is why the ordinary case reads the same as before.
            NodeBody::Kind(kind) => resolve(kind, &node.items),
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
            // R1934 — one in, one out, both carrying what the chain this
            // reroute belongs to decided. Derived and not authored, which is
            // why this arm needs the whole document where `Delay` needed only
            // the body: see [`Document::passing_flow`].
            //
            // R1935 — a beacon is the same shape for the same reason, and
            // shares the arm rather than copying the derivation.
            NodeBody::Reroute | NodeBody::Beacon => self.passing_signature(tree, node.id),
            // R1935 — an echo has NO input: the value reaches it by name, and
            // an input port would be a place to wire one, which is precisely
            // the edge this body exists to do without.
            NodeBody::Echo(_) => self.echo_signature(tree, node.id),
            // R2004 — the signature its members SHARE, derived for the same
            // reason the two above are and with a sharper consequence: these
            // ports are what the expansion maps through.
            NodeBody::StandIn(_) => self.stand_in_signature(tree, node.id),
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

    /// ★★★★★ R1920 — **may this edit be made?**, asked *before* making it.
    ///
    /// The reference census names this in three rows across two projects —
    /// *can this node be deleted*, *can this node be renamed*, *can this graph
    /// take new nodes* — and its own `covered_by` said what was missing: **a
    /// node cannot refuse an edit; there is no per-node permission surface.**
    /// Measured at R1920's open, that was exactly true: every refusal this
    /// crate had was reached by ATTEMPTING the edit, so an editor could only
    /// find out by doing it.
    ///
    /// # ★★★★★ Why this answers `Result<(), EditError>` and not a type of its own
    ///
    /// Because the alternative is two sources of one truth. Both references
    /// implement the question and the edit as SEPARATE code — a `Can…`
    /// predicate beside a `Delete` that re-decides — so the two are free to
    /// disagree, and nothing there can notice. Here there is one vocabulary and
    /// one decision: this function IS the decision, and [`Self::remove_node`],
    /// [`Self::relabel`] and [`Self::add_node`] each begin by asking it. An
    /// editor that asks first and an editor that just tries cannot get
    /// different answers, because the second one is asking too.
    ///
    /// That also fixes what the answer MEANS. `may(…).is_ok()` does not
    /// predict the edit — it is the same test the edit will run, so agreement
    /// is not a property this has to maintain. [`Act`] carries the VALUE for
    /// the same reason: a rename is refused for the name it would take, so a
    /// question that left the name out could only answer half of it.
    ///
    /// # Errors
    ///
    /// Whatever the corresponding edit would answer: [`EditError::NoSuchTree`],
    /// [`EditError::NoSuchNode`], [`EditError::InterfaceEnd`],
    /// [`EditError::LabelTaken`], [`EditError::LabelEmpty`].
    pub fn may(&self, tree: TreeId, act: Act<'_, K>) -> Result<(), EditError> {
        if self.tree(tree).is_none() {
            return Err(EditError::NoSuchTree(tree));
        }
        match act {
            Act::Create(body) => self.admits(tree, body),
            Act::Delete(node) => {
                let held = self.held(tree, node)?;
                // ★ The one refusal that exists today, and the reason this
                // surface is not vacuous: see `EditError::InterfaceEnd`.
                if let NodeBody::Interface(side) = held.body {
                    return Err(EditError::InterfaceEnd { tree, node, side });
                }
                Ok(())
            }
            Act::Rename(node, label) => {
                let wanted = Self::wanted_label(tree, node, label)?;
                self.held(tree, node)?;
                // ★★★★★ R1932 — the SCOPE is the kind's, not this function's.
                // Until this round the reach was written here as `nodes_labelled`
                // — one tree, always — so an application could neither widen it
                // nor turn it off, and a frame (this crate's comment) was held
                // to the same uniqueness as a node the graph is addressed by.
                if let Some(name) = wanted.as_deref() {
                    // ★ R1985 — the scope dispatch this arm used to spell out
                    // is `holders_of` now, because the copy path is its second
                    // reader and two inlined copies of one rule is how two
                    // consumers come to disagree.
                    let held = self.held(tree, node)?;
                    let clash = self
                        .holders_of(tree, &held.body, name)
                        .into_iter()
                        .find(|(where_, other)| !(*where_ == tree && *other == node));
                    if let Some((where_, held_by)) = clash {
                        return Err(EditError::LabelTaken {
                            tree: where_,
                            label: name.to_owned(),
                            held_by,
                        });
                    }
                }
                Ok(())
            }
        }
    }

    /// ★★★★★ R1923 — **what this node says about itself**, and which of its two
    /// sources said it.
    ///
    /// The census names this in two rows across both projects — a node asked
    /// for its tooltip text, and a node type asked to describe a given node —
    /// and neither can answer the second half: both hand back a bare string,
    /// so a caller cannot tell an authored note from the kind's own sentence.
    ///
    /// An authored note wins when there is one, which is the same precedence
    /// [`Node::label`] has over the body's name, and for the same reason: the
    /// more specific statement is the one a person made.
    ///
    /// Answers `None` when neither has anything to say. That is a real answer
    /// and not a hole — a kind is not obliged to describe itself, and inventing
    /// a sentence to fill the gap would give a reader something they cannot
    /// tell from a real description.
    #[must_use]
    pub fn description(&self, tree: TreeId, node: NodeId) -> Option<Description> {
        let held = self.tree(tree)?.node(node)?;
        if let Some(sentence) = held.description.clone() {
            return Some(Description {
                sentence,
                source: Described::Authored,
            });
        }
        match &held.body {
            NodeBody::Kind(kind) => kind.description().map(|sentence| Description {
                sentence,
                source: Described::Kind,
            }),
            // ⚠ The structural bodies say nothing, and that is deliberate rather
            // than unimplemented: a frame, a group instance, an interface end
            // and a delay are this CRATE's, so a sentence about them would be
            // this crate describing itself to an application's reader in
            // whatever language this file happens to be written in. An
            // application that wants one writes it on the node.
            _ => None,
        }
    }

    /// ★★★★★ R1922 — **would this tree accept this body?**
    ///
    /// The census's four rows on this axis — the DCC asking a node type and a
    /// node instance *can this be added to this tree*, the engine asking a node
    /// whether it may be created under a schema and whether it is compatible
    /// with a graph — are all this question, and until R1922 nothing here could
    /// be asked it: a `Document` has one kind of tree, so the crate had read
    /// that as *nothing can be refused*. Measured at this round's open, that
    /// conclusion was wrong in the direction that matters — three placements
    /// were accepted that leave a document nothing can use.
    ///
    /// # ★★★★★ Why this is a function and not a second rule beside `validate`
    ///
    /// `Document::validate` ALREADY reports two of these three, after the fact:
    /// a duplicate interface end and a definition that reaches itself. So the
    /// tempting shape is a fresh rule here that refuses early — and that is
    /// two oracles for one rule, free to disagree, which is exactly what R1884
    /// recorded the cost of. Instead this IS the rule: `may` asks it before an
    /// edit, and `validate` asks it of every node already placed, so a
    /// document loaded from a file and an edit about to happen are judged by
    /// one predicate. Neither can drift, because there is nothing to drift
    /// from.
    ///
    /// ⚠ `validate` stays, and is not made redundant by this: a document
    /// arrives from a FILE without passing any verb, so the same rule has to be
    /// askable of a whole document. What changed is that it is asked in one
    /// place.
    ///
    /// # Errors
    ///
    /// [`EditError::RootHasNoOutside`] when an interface end is placed in a
    /// tree nothing instantiates; [`EditError::InterfaceEndTaken`] when that
    /// side already has one; [`EditError::WouldContainItself`] when a group
    /// instance names a tree that already contains it;
    /// [`EditError::KindNotAdmitted`] when the kind does not declare itself at
    /// home in this graph's kind (R1999).
    pub fn admits(&self, tree: TreeId, body: &NodeBody<K>) -> Result<(), EditError> {
        match body {
            // ★★★★★ R1999 — the arm that can vary by WHAT THIS GRAPH IS. The
            // other three read a tree's *role* (the root is the tree nothing
            // instantiates) or the containment relation; none of them could ask
            // the question the reference's own compatibility hook is sixteen
            // hand-written copies of.
            NodeBody::Kind(kind) => {
                if self.at_home(tree, kind) {
                    Ok(())
                } else {
                    Err(EditError::KindNotAdmitted {
                        tree,
                        kind: kind.name(),
                        graph: self.graph_kind_token(tree),
                    })
                }
            }
            NodeBody::Interface(side) => {
                // ★ ROOT is the one tree nothing instantiates, so an interface
                // end there materialises a contract with no outside — nobody
                // can ever wire to it. `validate` did not report this at all,
                // measured at R1922's open: it was the one of the three with
                // no diagnosis whatsoever.
                if tree == ROOT {
                    return Err(EditError::RootHasNoOutside { tree, side: *side });
                }
                if let Some(held) = self
                    .tree(tree)
                    .and_then(|host| host.interface_node(*side))
                    .map(|node| node.id)
                {
                    return Err(EditError::InterfaceEndTaken {
                        tree,
                        side: *side,
                        held_by: held,
                    });
                }
                Ok(())
            }
            NodeBody::Group(definition) => {
                // ★★★★★ `Nesting::cycle` is this question already answered —
                // *may this definition be placed in this host* — over the
                // crate's own containment relation, which is the SAME relation
                // `validate`'s recursion finding reads. Writing a walk here
                // would have been a second implementation of one rule; the
                // compiler said so by refusing the name I gave it, which is
                // R1919's lesson for the second time: the word already existed.
                //
                // And it names the CHAIN, which the reference does not: its own
                // nesting refusal prints the same flat sentence for a direct
                // self-nest and for one four groups deep, so the definitions
                // actually carrying the recursion are never named.
                if let Some(chain) = pinion_graph::group::Nesting::cycle(
                    &self.containment(),
                    tree.0 as usize,
                    definition.0 as usize,
                ) {
                    return Err(EditError::WouldContainItself {
                        tree,
                        definition: *definition,
                        chain: chain
                            .into_iter()
                            .map(|id| TreeId(u32::try_from(id).unwrap_or(u32::MAX)))
                            .collect(),
                    });
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The node, or the error naming why it is not there. Shared by [`Self::may`]
    /// and the verbs, so "no such node" is one sentence rather than four.
    fn held(&self, tree: TreeId, node: NodeId) -> Result<&Node<K>, EditError> {
        self.tree(tree)
            .and_then(|host| host.node(node))
            .ok_or(EditError::NoSuchNode { tree, node })
    }

    /// The label a rename would actually store, or why it cannot be one.
    ///
    /// Whitespace is trimmed before either check — see [`Self::relabel`], whose
    /// argument for that is what this carries.
    fn wanted_label(
        tree: TreeId,
        node: NodeId,
        label: Option<&str>,
    ) -> Result<Option<String>, EditError> {
        match label {
            None => Ok(None),
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    Err(EditError::LabelEmpty { tree, node })
                } else {
                    Ok(Some(trimmed.to_owned()))
                }
            }
        }
    }

    /// Add a node to `tree` and answer its fresh id.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] when `tree` is not in the document, and
    /// whatever [`Self::admits`] refuses this body for (R1922) — asked through
    /// [`Self::may`], so an editor that checks first and one that just calls
    /// this cannot get different answers (R1920).
    pub fn add_node(
        &mut self,
        tree: TreeId,
        body: NodeBody<K>,
        x: i32,
        y: i32,
    ) -> Result<NodeId, EditError> {
        self.may(tree, Act::Create(&body))?;
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
                // R1923 — nothing written about it yet, so it says whatever
                // its kind says.
                description: None,
                bypassed: false,
                disabled: false,
                appearance: Appearance::default(),
                parent: None,
                values: BTreeMap::new(),
                // R1632 — nothing authored, which resolves to whatever minimum
                // the kind declared. A fresh sequencer therefore arrives with
                // its two branches without this having to know that.
                items: Items::default(),
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
    /// ★★★★★ R2003 — and it covers the ZONE the node was in, which until this
    /// round it did not: see [`Removed::unpaired`].
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`], [`EditError::NoSuchNode`], or
    /// [`EditError::InterfaceEnd`] — all of them asked through
    /// [`Self::may`], so an editor that checks first and one that just calls
    /// this cannot get different answers (R1920).
    pub fn remove_node(&mut self, tree: TreeId, node: NodeId) -> Result<Removed, EditError> {
        self.may(tree, Act::Delete(node))?;
        let adopted = self.adopt_orphans(tree, node);
        if let Some(host) = self.tree_mut(tree) {
            host.nodes.remove(&node);
        }
        Ok(Removed {
            links: self.unwire_node(tree, node),
            adopted,
            // ★ Read AFTER the node is gone, so the derivation is what decides
            // the pairing has lapsed rather than this knowing the rule a second
            // time.
            unpaired: self.reap_zones(tree, node),
        })
    }

    /// Every node in `tree` that has **authored** the name `label`.
    ///
    /// The relation, of which [`Self::node_labelled`] is the function. A caller
    /// with the vector can tell the two ways a lookup fails apart — nothing
    /// answers to the name (empty) from more than one does (longer than one) —
    /// and those are different problems with different fixes.
    ///
    /// Authored names only: a node with no [`label`](Node::label) is *described*
    /// by its body's name rather than called it, and two unnamed nodes of a kind
    /// describing themselves the same way is not an ambiguity anybody authored.
    #[must_use]
    pub fn nodes_labelled(&self, tree: TreeId, label: &str) -> Vec<NodeId> {
        self.tree(tree)
            .into_iter()
            .flat_map(|host| host.nodes.values())
            .filter(|node| node.label.as_deref() == Some(label))
            .map(|node| node.id)
            .collect()
    }

    /// The one node in `tree` called `label`, or `None`.
    ///
    /// ★★ **`None` rather than a guess when more than one answers to it.**
    /// Measured on the reference toolkit 6.11.1: two siblings may hold one name
    /// and its by-name lookup then returns one of them with nothing said, so a
    /// caller cannot distinguish "the thing I asked for" from "one of the
    /// several things that answer to what I asked for". [`Self::relabel`]
    /// refuses to *create* that state; a direct write to the public
    /// [`label`](Node::label) field still can, and this is what happens then.
    /// [`Self::nodes_labelled`] says which case it was.
    ///
    /// ⚠★★★★★ R1985 — **that second sentence was false for 407 rounds, and it
    /// was this crate's own copy verb that made it so.** Measured at R1985's
    /// open: [`Self::duplicate`] copied a label verbatim, so duplicating a node
    /// called `Total` left two answering to it, this answered `None`, and
    /// `may(Act::Rename(copy, Some("Total")))` answered `LabelTaken` about a
    /// state the crate had just built. A direct field write was never the only
    /// way in. It is now: see [`Copying`](crate::Copying).
    #[must_use]
    pub fn node_labelled(&self, tree: TreeId, label: &str) -> Option<NodeId> {
        let holders = self.nodes_labelled(tree, label);
        match holders.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// ★★★★★ R1919 — **every node under `from` whose name holds
    /// `needle`, and the way in to each one.**
    ///
    /// [`nodes_labelled`](Self::nodes_labelled) answers an exact name in ONE
    /// tree. This is the other question, and it is the one both references
    /// have and this crate did not: *where is the thing called that, anywhere
    /// in this document, and what do I have to open to get to it.* The
    /// reference census names it in six rows across two projects — the DCC's
    /// `find_node` and the engine's five per-editor finds — and its own
    /// `covered_by` had recorded them as one mechanism before this round
    /// reached for it.
    ///
    /// # ★★★★★ What is past both references: the way in is RETURNED
    ///
    /// Both references *perform* the descent — they open the group the hit is
    /// in and select it — and neither *publishes* it. So a caller there cannot
    /// ask **how far away** a hit is, or show a reader where they are about to
    /// be taken, or open the same path twice. [`Found::at`] is that answer, and
    /// it is this crate's OWN editing position rather than a list invented for
    /// the occasion: [`depth`](EditPath::depth) is how far away the hit is,
    /// [`entries`](EditPath::entries) is each group descended together with the
    /// tree it was descended from, [`breadcrumb`](EditPath::breadcrumb) is the
    /// whole way in by name, and the value goes **straight to an editor**.
    ///
    /// # Matching, stated rather than assumed
    ///
    /// Case-insensitive containment over [`Node::display_name`], which is the
    /// name a reader SEES — an authored label when there is one and the body's
    /// own word otherwise. Matching the authored label alone would make the
    /// unnamed majority of a graph unfindable by the only word a reader has
    /// for it, and [`Matched`] is what keeps the two answers apart in one list.
    ///
    /// An empty `needle` finds **nothing**. It contains-matches everything, so
    /// the alternative is a "result" that is the whole document — which is not
    /// an answer to a question nobody asked. Both references show nothing for
    /// an empty query.
    ///
    /// # Order
    ///
    /// Breadth-first from `from`, so the shallowest hits come first: a reader
    /// scanning a result list meets the nodes nearest to where they already are
    /// before the ones several groups down.
    ///
    /// # Panics
    ///
    /// Never. A group whose definition is missing, or one that reaches itself,
    /// ends that branch — a document is not required to be well-formed for a
    /// search to answer.
    #[must_use]
    pub fn find(&self, from: TreeId, needle: &str) -> Vec<Found> {
        if needle.is_empty() {
            return Vec::new();
        }
        let needle = needle.to_lowercase();
        let mut out = Vec::new();
        // ★ `seen` is over TREES rather than nodes: a group definition reached
        // twice is the same tree, and a document that reaches its own
        // definition would otherwise descend for ever. Guarding here rather
        // than asking the document to be acyclic is what makes the `# Panics`
        // section above say `Never`.
        let mut seen = vec![from];
        // ⚠ The frontier carries the PATH, not the tree: `EditPath::current()`
        // is the tree, so keeping both would be two facts free to disagree.
        let mut frontier = vec![EditPath::at(from)];
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for at in frontier {
                let tree = at.current();
                let Some(host) = self.tree(tree) else {
                    continue;
                };
                let mut nodes: Vec<_> = host.nodes.values().collect();
                nodes.sort_by_key(|node| node.id);
                for node in nodes {
                    let shown = node.display_name();
                    if shown.to_lowercase().contains(&needle) {
                        out.push(Found {
                            node: node.id,
                            at: at.clone(),
                            because: match node.label {
                                Some(_) => Matched::Label,
                                None => Matched::Kind,
                            },
                            shown,
                        });
                    }
                    if let NodeBody::Group(inner) = node.body
                        && !seen.contains(&inner)
                    {
                        seen.push(inner);
                        let mut deeper = at.clone();
                        // The descent is `EditPath`'s own, so a path this
                        // search builds and one a reader walked by hand are
                        // the same value built the same way.
                        if deeper.enter(self, node.id).is_ok() {
                            next.push(deeper);
                        }
                    }
                }
            }
            frontier = next;
        }
        out
    }

    /// Give `node` an authored name, or take its authored name away with
    /// `None`, answering what it was called before (R1682).
    ///
    /// ★★★ **A rename is not a re-creation.** The node keeps its
    /// [`NodeId`], so its links, its position, its containment, its authored
    /// port values, its breakpoints and every table a caller keys by identity
    /// are untouched — the rename moves one string. The reference *prototype*
    /// this screen is built against cannot do that: its author-a-node has no
    /// rename, so it copies the node under the new name and covers the old one
    /// with a deletion, and then has to hand-move ten separate side tables to
    /// compensate. That is the same distinction [`Self::relink`] draws for a
    /// link's endpoint, in the other place identity leaks out of an editor.
    ///
    /// ★★ **A name that does not identify is refused.** Measured on the
    /// reference toolkit 6.11.1: naming a second sibling what the first is
    /// already called is accepted silently, both then hold it, and the
    /// by-name lookup answers an arbitrary one of the two. Here that is
    /// [`EditError::LabelTaken`], which names the node already holding it —
    /// so the invariant *authored names are unique within a tree, and
    /// therefore address exactly one node* is one this crate maintains rather
    /// than one every caller re-checks.
    ///
    /// Whitespace is trimmed before either check, because a name that differs
    /// from another only by a trailing space is exactly the ambiguity the
    /// refusal exists to prevent, and a name that is *only* whitespace is
    /// [`EditError::LabelEmpty`] rather than a silent clear.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`], [`EditError::NoSuchNode`],
    /// [`EditError::LabelTaken`], [`EditError::LabelEmpty`] — all of them
    /// asked through [`Self::may`], so an editor that checks first and one
    /// that just calls this cannot get different answers (R1920).
    pub fn relabel(
        &mut self,
        tree: TreeId,
        node: NodeId,
        label: Option<&str>,
    ) -> Result<Relabelled, EditError> {
        self.may(tree, Act::Rename(node, label))?;
        let wanted = Self::wanted_label(tree, node, label)?;
        let slot = self
            .trees
            .get_mut(tree.0 as usize)
            .and_then(|host| host.nodes.get_mut(&node))
            .ok_or(EditError::NoSuchNode { tree, node })?;
        let was = std::mem::replace(&mut slot.label, wanted.clone());
        Ok(Relabelled {
            changed: was != wanted,
            was,
            now: wanted,
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
        let Some(host) = self.tree_mut(tree) else {
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
        let Some(host) = self.tree_mut(tree) else {
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
        let Some(host) = self.tree_mut(tree) else {
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

    /// Whether the two **nodes** these sockets belong to may be wired (R1885).
    ///
    /// The peer of [`Self::conversion`], and the question an editor asks with
    /// it while a wire is being dragged: that one answers *what happens to the
    /// value*, this one answers *may these two talk at all*. `None` when either
    /// node is not there — which, as with `conversion`, is a different fact
    /// from a refusal.
    ///
    /// ⚠ Answered from the nodes' **bodies**, so a node whose body is not a
    /// plain kind — a group instance, an interface node — is admitted: it has
    /// no kind of its own to judge, and refusing it would refuse every wire
    /// into a subtree.
    #[must_use]
    pub fn admission(&self, tree: TreeId, from: Socket, to: Socket) -> Option<Admission> {
        let source = self.tree(tree)?.node(from.node)?;
        let sink = self.tree(tree)?.node(to.node)?;
        match (&source.body, &sink.body) {
            (NodeBody::Kind(out), NodeBody::Kind(into)) => Some(K::admits(out, into)),
            _ => Some(Admission::Allowed),
        }
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

    /// ★★★★★ R1939 — **what this node's port will TAKE as a resting value.**
    ///
    /// `None` when the tree, the node or the port is not there, and
    /// [`Admits::Anything`] when the port carries **control** — a control port
    /// takes no value at all, which [`Document::set_port_value`] refuses on its
    /// own arm before the taxonomy is asked, so this does not restate it.
    ///
    /// The register a screen reads before it offers an editor: it says what the
    /// port wants without being handed a value first, which is the half a
    /// predicate cannot answer.
    #[must_use]
    pub fn takes(&self, tree: TreeId, node: NodeId, port: PortRef) -> Option<Admits<K::Value>> {
        let signature = self.signature(tree, node)?;
        let ports = match port.side {
            Side::Input => &signature.inputs,
            Side::Output => &signature.outputs,
        };
        let declared = ports.get(port.index as usize)?;
        let ty = declared.value_type()?;
        match &self.tree(tree)?.node(node)?.body {
            NodeBody::Kind(kind) => Some(kind.takes(port, ty)),
            // ⚠ A structural body has no kind to ask: a group instance's ports
            // come from the definition's boundary, and a frame, an interface
            // end and a delay are this CRATE's. Their type is the whole
            // constraint, and saying so here is what keeps `Anything` from
            // reading as an omission.
            _ => Some(Admits::Anything),
        }
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
    ) -> PortValueResult<K> {
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
        // ★★★★★ R1939 — and then what the DECLARATION admits, which is the
        // narrower question: a value may be of the port's type and still not be
        // one this port takes. Asked after the type, because a wrong-typed
        // value is a different repair and reporting the narrower rule about it
        // would send an author to fix the wrong thing.
        if let Judged::Refused { wants, instead } = self
            .takes(tree, node, port)
            .unwrap_or(Admits::Anything)
            .judge(&value)
        {
            return Err(PortValueError::NotAdmitted {
                port,
                wants,
                instead,
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
        let crowded = self.vet(tree, from, to)?;
        // The tree exists: `vet` resolved a signature through it twice.
        self.wire(tree, from, to, crowded)
            .ok_or(ConnectError::NoSuchNode(from))
    }

    /// ★★★★★ R1987 — the **placement** half of [`connect`](Self::connect),
    /// after the vet has already answered.
    ///
    /// Extracted so that [`autowire`](Self::autowire) can act on the decision
    /// it already made instead of asking a second time. Two calls to
    /// [`vet`](Self::vet) around one gesture is not merely wasted work: the
    /// second one is asked of a document the first has not changed, so the only
    /// thing the pair can produce that one call cannot is a **disagreement** —
    /// and the caller would then have to invent a refusal for a case it has
    /// already ruled out, which is an arm no test can reach. This is the split
    /// `plan_relink` made for R1924, reached from the other verb.
    ///
    /// `crowded` is [`vet`](Self::vet)'s own answer and is not re-derived here:
    /// which end has to give way is a property of the two ports, and deciding
    /// it twice is what this extraction exists to stop.
    ///
    /// `None` only when the tree is gone, which a vetted pair cannot reach —
    /// the caller maps it onto whichever "not there" its own vocabulary has,
    /// which is what [`connect`](Self::connect) did inline before this split.
    pub(crate) fn wire(
        &mut self,
        tree: TreeId,
        from: Socket,
        to: Socket,
        crowded: Option<Side>,
    ) -> Option<Connected> {
        let host = self.tree_mut(tree)?;
        let id = LinkId(host.next_link);
        host.next_link += 1;
        let displaced = self.place(
            tree,
            Link {
                id,
                from,
                to,
                muted: false,
            },
            crowded,
            None,
        );
        Some(Connected {
            link: id,
            displaced,
        })
    }

    /// Whether these two sockets may be wired, and — when they may — which end
    /// is the one whose limit a new link would exceed.
    ///
    /// ★ R1681 — extracted so that [`connect`](Self::connect) and
    /// [`relink`](Self::relink) ask the question once. The four rules below are
    /// properties of the graph, not of the gesture that reached them, so a
    /// second copy for the second verb would be a second answer free to drift
    /// from this one — and the drift would be silent, because both verbs would
    /// still be producing links.
    pub(crate) fn vet(
        &self,
        tree: TreeId,
        from: Socket,
        to: Socket,
    ) -> Result<Option<Side>, ConnectError<K::Type>> {
        self.vet_without(tree, from, to, None)
    }

    /// ★★★★★ R2000 — the same four rules, asked of the graph **without one
    /// link in it**: what a *move* has to ask, because the link being moved is
    /// not part of the graph the move would leave.
    ///
    /// See [`data_path_without`](Self::data_path_without) for the measurement
    /// that forced the parameter. Only the acyclicity rule reads links at all,
    /// which is why this is the only rule the exclusion reaches — and that is
    /// asserted rather than asserted-in-prose by
    /// `r2000_the_exclusion_reaches_the_acyclicity_rule_and_nothing_else`.
    pub(crate) fn vet_without(
        &self,
        tree: TreeId,
        from: Socket,
        to: Socket,
        moving: Option<LinkId>,
    ) -> Result<Option<Side>, ConnectError<K::Type>> {
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
        // ★★★★★ R1885 — and now the question the two lines above could not ask.
        // This function had ALREADY resolved both nodes, twice, and handed the
        // rule two ports; the node identities were discarded one line before
        // `crossing` was consulted, so no rule could be written about the PAIR.
        // Asked after the self-link check on purpose: a node is required to
        // admit its own kind, so asking first would make a self-link's refusal
        // depend on the taxonomy rather than on the graph.
        if let Some(Admission::Refused(why)) = self.admission(tree, from, to) {
            return Err(ConnectError::Incompatible {
                from,
                to,
                refusal: why,
            });
        }
        // R1599 — **only a value link may not close a cycle.** A cycle through
        // control links is not a contradiction, it is a LOOP: the thing every
        // real execution graph is built to express. So the acyclicity check
        // walks the data plane alone, and a control cycle is legal here and
        // reported by `Document::control_loops` rather than refused.
        //
        // The engine reaches the same split and states it in a comment on the
        // predicate that implements it — `compiler context::
        // PinIsImportantForDependancies` returns `PinCategory != PC_Exec`,
        // "the execution wires do not form data dependencies, they are only
        // important for final scheduling and that is handled thru gotos". What
        // it does NOT do is notice a control cycle: an execution loop with no
        // exit compiles, and is caught by counting iterations at run time
        // (`visual script exception type::InfiniteLoop`).
        //
        // R1600 — and a value link leaving a DELAY adds no dependency at all,
        // so it can no more close a cycle than a control link can: what leaves
        // a delay is what arrived a tick ago. That is the causality rule
        // Lustre states as "every cycle must be broken by a `pre`", and it is
        // asked here as one predicate rather than re-derived, so the wire this
        // accepts is exactly the wire `cycle_nodes` will not report.
        // ★★★★★ R1934 — the test is "**not** control", not "is a decided
        // value". An undecided port ([`Flow::Undecided`], which a reroute
        // reaches) has no value type yet and is not control either, and reading
        // the absence of a type as "carries no data" let a ring of reroutes be
        // built: every link in it skipped this check, and the cycle was then
        // there for the first decided port to arrive into. A cycle that becomes
        // real later is the state this refusal exists to make unrepresentable,
        // so the undecided case is refused with the value case rather than with
        // the control one. Found by `r1934_a_ring_of_reroutes_answers_rather_
        // than_recursing`, which expected the refusal and got a link.
        let adds_a_dependency = !source.is_control() && !self.cuts_dependency(tree, from.node);
        if adds_a_dependency
            && let Some(path) = self.data_path_without(tree, to.node, from.node, moving)
        {
            return Err(ConnectError::WouldCycle { path });
        }

        // R1599 — which end has to give way is the port's own limit, and the
        // two flows put it on opposite ends: a value INPUT takes one producer,
        // a control OUTPUT takes one successor.
        Ok(if sink.multiplicity(Side::Input) == Multiplicity::One {
            Some(Side::Input)
        } else if source.multiplicity(Side::Output) == Multiplicity::One {
            Some(Side::Output)
        } else {
            None
        })
    }

    /// Take the link at `at` out of the tree's order, answering it.
    ///
    /// ★ R1681 — the counterpart of [`place`](Self::place), for a caller that
    /// has already found the position and is going to put something back
    /// there. [`disconnect`](Self::disconnect) is the public verb and resolves
    /// its own index.
    pub(crate) fn lift(&mut self, tree: TreeId, at: usize) -> Option<Link> {
        let host = self.tree_mut(tree)?;
        (at < host.links.len()).then(|| host.links.remove(at))
    }

    /// Put a vetted `link` into the tree, displacing whatever the crowded end
    /// already held, and answer what was displaced.
    ///
    /// ★ R1681 — the other half of the extraction [`vet`](Self::vet) begins.
    /// Taking a whole [`Link`] rather than minting one is the point: a link
    /// whose end moves keeps its **identity** and its mute, and the only way
    /// for that to be true is for the placement not to invent either.
    ///
    /// `at` is where in the tree's order to put it — `None` to append, which is
    /// what a new link wants; a rewire passes its old position, because a link
    /// that jumped to the end of the order every time an endpoint moved would
    /// re-order what a renderer draws over a change that is not about order.
    pub(crate) fn place(
        &mut self,
        tree: TreeId,
        link: Link,
        crowded: Option<Side>,
        at: Option<usize>,
    ) -> Option<Link> {
        let host = self.tree_mut(tree)?;
        let displaced = match crowded {
            Some(Side::Input) => host.links.iter().find(|l| l.to == link.to).copied(),
            Some(Side::Output) => host.links.iter().find(|l| l.from == link.from).copied(),
            None => None,
        };
        let mut at = at.unwrap_or(host.links.len());
        if let Some(displaced) = displaced {
            // Removing it shifts everything after it down, so a slot beyond it
            // has to come down too or the link lands one place too late.
            if let Some(gone) = host.links.iter().position(|l| l.id == displaced.id)
                && gone < at
            {
                at -= 1;
            }
            host.links.retain(|l| l.id != displaced.id);
        }
        host.links.insert(at.min(host.links.len()), link);
        displaced
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
    /// it. The DCC's `mute_toggle`.
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

    /// Switch `node` off, or back on, answering what it was before (R1682).
    ///
    /// See [`Node::disabled`] for what this means and how it differs from
    /// [`Self::set_bypassed`]. Unlike bypassing, this can never make a cycle
    /// live — a disabled node produces nothing, so it cuts the flow rather than
    /// completing it — which is why there is no refusal here and one there.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchNode`].
    pub fn set_disabled(
        &mut self,
        tree: TreeId,
        node: NodeId,
        disabled: bool,
    ) -> Result<bool, EditError> {
        let host = self
            .trees
            .get_mut(tree.0 as usize)
            .ok_or(EditError::NoSuchTree(tree))?;
        let target = host
            .nodes
            .get_mut(&node)
            .ok_or(EditError::NoSuchNode { tree, node })?;
        Ok(std::mem::replace(&mut target.disabled, disabled))
    }

    /// Mute `link`, or unmute it, answering what it was before.
    ///
    /// A muted link keeps its place in the structure and carries no value, so
    /// the port it feeds falls back to its own default. The DCC's
    /// `links_mute`.
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
        self.tree_mut(tree)?.nodes.remove(&node)
    }

    /// Put a node into a tree under its existing id, keeping the id source
    /// ahead of it.
    pub(crate) fn put_node(&mut self, tree: TreeId, node: Node<K>) {
        let Some(host) = self.tree_mut(tree) else {
            return;
        };
        host.next_node = host.next_node.max(node.id.0 + 1);
        host.nodes.insert(node.id, node);
    }

    /// Take a link out of a tree.
    pub(crate) fn take_link(&mut self, tree: TreeId, link: LinkId) -> Option<Link> {
        let host = self.tree_mut(tree)?;
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
        let Some(host) = self.tree_mut(tree) else {
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
    ///
    /// ★★★★★ R2004 — over the **expanded** links, not the authored ones. A
    /// stand-in is resolved away by [`Self::expanded_links`], so a wire drawn
    /// to one is a wire to each node it stands in for and every derivation
    /// built on this — [`Self::cycle_nodes`], [`Self::control_loops`], and the
    /// path [`Self::connect`] reports when it refuses a cycle — sees the graph
    /// that actually runs. Threading it here rather than at each of those is
    /// this crate's repeating rule: the repair is a derivation, not a list of
    /// the places that read it.
    ///
    /// ⚠ Behaviour-preserving for a document with no stand-in, where the
    /// expansion answers the authored links unchanged and one at a time.
    fn successors_on(&self, tree: TreeId, control: bool) -> BTreeMap<NodeId, Vec<NodeId>> {
        let mut successors: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        if self.tree(tree).is_none() {
            return successors;
        }
        for link in &self.expanded_link_view(tree) {
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
        self.data_path_without(tree, start, goal, None)
    }

    /// ★★★★★ R2000 — the same walk, with one link **taken out of the graph for
    /// the duration of the question**.
    ///
    /// # Why an edit has to be able to ask this
    ///
    /// Because a link that is being *moved* is not part of the graph the move
    /// would leave behind, and the acyclicity rule is about that graph. Ask
    /// without this and the link answers for itself: turning `A -> B` round is
    /// refused as a cycle, because a walk looking for a path from `B` to `A`
    /// finds `A -> B` — the very link the caller asked to remove from there.
    ///
    /// R1924 handled this for a **one-ended** move by proof rather than by
    /// construction, and the proof is exactly as strong as its premise: a
    /// forward search from the standing end cannot use the moving link, because
    /// one end of it never moved. [`Document::retarget`] moves BOTH ends, so
    /// there is no standing end and the premise is simply gone — measured, not
    /// reasoned: the first draft of [`Document::turn`] asked
    /// [`data_path_between`](Self::data_path_between) and every reversal of a
    /// value link came back `WouldCycle` naming the link being reversed.
    ///
    /// So the exclusion is a parameter now instead of an argument in a comment.
    /// One-ended moves pass it too — where R1924's proof says it changes
    /// nothing, which is a claim a test can hold rather than a reader having to
    /// re-derive.
    fn data_path_without(
        &self,
        tree: TreeId,
        start: NodeId,
        goal: NodeId,
        moving: Option<LinkId>,
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
            // ★★★★★ R1935 — the steps out of `current` are its outgoing DATA
            // links **and**, when it is a beacon, every echo of it. A value
            // reaching a named endpoint reaches every far end of that name, and
            // it does so with no edge — which is the whole capability, and
            // therefore exactly the dependency a walk over links alone cannot
            // see.
            //
            // Without this step `connect` accepts a wire closing a ring through
            // a name: beacon -> echo -> … -> beacon. That is R1934's ring of
            // reroutes with the ring drawn in the one ink this walk was blind
            // to, and its lesson applies unchanged — a cycle that becomes real
            // later is the state the refusal exists to make unrepresentable.
            let named: Vec<NodeId> = match host.node(current).map(|held| &held.body) {
                Some(NodeBody::Beacon) => host
                    .nodes()
                    .filter(|held| matches!(held.body, NodeBody::Echo(end) if end == current))
                    .map(|held| held.id)
                    .collect(),
                _ => Vec::new(),
            };
            let steps = host
                .links
                .iter()
                .filter(|l| Some(l.id) != moving)
                .filter(|l| l.from.node == current && !self.link_is_control(tree, l))
                .map(|l| l.to.node)
                .chain(named);
            for next in steps {
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
    /// compiles — exec pins are excluded from the dependency sort by
    /// `PinIsImportantForDependancies`, so no `Dependency cycle detected` can
    /// fire for one — and a loop with no exit is discovered at *run time*, by
    /// a counter (`GMaximumScriptLoopIterations`) raising
    /// `visual script exception type::InfiniteLoop` after the fact, in a build
    /// that may be shipping. The nodes are named here before it runs,
    /// statically.
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

    /// A tree's interface, mutably, or `None` when there is no such tree.
    ///
    /// The one way [`crate::section`] reaches an interface to change it, so the
    /// tree lookup that guards every one of those operations is written once.
    pub(crate) fn interface_of_mut(&mut self, tree: TreeId) -> Option<&mut Interface<K>> {
        self.trees
            .get_mut(tree.0 as usize)
            .map(|host| &mut host.interface)
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
        // ★★★★★ R1933 — the tree's own declaration, asked here because this is
        // where a type ARRIVES on an interface. The DCC refuses in exactly the
        // two places that do the same thing (make an interface socket, retype
        // one), and its third consumer only OFFERS — which is
        // `Document::offered_types`, derived from this same list so the offer
        // and the refusal cannot disagree.
        if let Flow::Value { ty, .. } = &port.flow
            && !host.admitted.admits(ty)
        {
            return Err(EditError::TypeNotAdmitted {
                tree,
                ty: format!("{ty:?}"),
            });
        }
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
        // ★ R1925 — the sections are indexed by the same numbers, so this is
        // where they are kept true. `expose` needs no counterpart: it appends,
        // and an append cannot move an index that already exists.
        host.interface.forget_port(side, index);

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
    /// ★★★★★ R2003 — the node this one was in a ZONE with, now standing alone.
    ///
    /// `None` when the removed node was in no zone, which is every node in a
    /// taxonomy that opens none.
    ///
    /// It is named for the same reason [`adopted`](Self::adopted) is: the zone
    /// is the half of this edit that is not where the gesture happened. A
    /// person deleting the end of a bracketed region has changed what the OTHER
    /// end means, and a screen that draws the region needs to know the region
    /// stopped existing.
    ///
    /// ⚠ Measured at R2003, this crate had written the opposite down: the
    /// stored map's own doc said node removal went through it, and removal did
    /// not touch it at all — so an opener whose closer was deleted went on
    /// answering with an id that resolved to no node, and
    /// [`validate`](Document::validate) said nothing. That is what a prose
    /// invariant with nothing performing it is worth.
    pub unpaired: Option<NodeId>,
}

/// What a successful [`Document::relabel`] did (R1682).
///
/// ★★ **The old name is the payload.** Measured on the reference toolkit
/// 6.11.1, its rename notification carries exactly one argument and that
/// argument is the *new* name — so a listener holding a table keyed by the old
/// one is told that something changed and not what to un-key. Every such table
/// is then rebuilt by scanning, or kept by hand, or silently wrong. Answering
/// [`was`](Relabelled::was) costs nothing here because the edit already had it
/// in hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relabelled {
    /// The authored name before this edit, or `None` when the node had none
    /// and was being called what its body is called.
    pub was: Option<String>,
    /// The authored name now, or `None` when the edit cleared it.
    pub now: Option<String>,
    /// Whether anything actually moved.
    ///
    /// `false` for a rename to the name the node already answers to. The
    /// reference filters that case out of its notification too (measured: a
    /// second set to the same string fires nothing), and the reason is the same
    /// — a listener that rebuilt its world on every no-op would be doing work
    /// no edit asked for.
    pub changed: bool,
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

/// ★ R1939 — what [`Document::set_port_value`] answers: the value it replaced,
/// or why the port would not take the new one.
///
/// A named type rather than the shape spelled inline, because R1939 gave
/// [`PortValueError`] a second parameter — the refusal carries the value the
/// declaration WOULD have taken — and a signature carrying two associated-type
/// projections is one a reader parses instead of reads.
///
/// ⚠ Measured, and NOT a de-duplication: this alias has exactly ONE use in this
/// tree, the function that returns it. It is an API-surface decision — the
/// alias is keyed on the taxonomy and exported, so a DOWNSTREAM caller can name
/// what `set_port_value` answers without spelling `<K as NodeKind>::Value`
/// twice. Saying so is the point: R1939's own changes list first claimed it
/// stopped a repetition "at every call", and there is no every-call.
pub type PortValueResult<K> = Result<
    Option<<K as NodeKind>::Value>,
    PortValueError<<K as NodeKind>::Type, <K as NodeKind>::Value>,
>;

// ★ R1986 — `Used`, `RemoveTreeError` (now `DefinitionError`) and `RemovedTree`
// moved to `definition.rs`, which owns the definition-tree verbs and their one
// permission surface. A module that owns a capability owns its vocabulary.

/// ★★★★★ R1943 — why two nodes could not be made a zone.
///
/// Every arm is a distinct refusal a caller can act on, which is the measured
/// difference from the reference: there the routine answers `bool` and writes
/// its reason into a report list, so *wrong kind of closer* and *that closer is
/// already paired* arrive as the same `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PairError {
    /// No such node in that tree.
    NoSuchNode(NodeId),
    /// The opener's body is not a kind — a group instance, a frame, an
    /// interface end or a delay cannot open a zone.
    NotAKind(NodeId),
    /// That kind opens nothing: it declared no closer.
    OpensNothing(NodeId),
    /// The closer is not the kind this opener's declaration names.
    WrongCloser {
        /// The node that would open.
        opener: NodeId,
        /// The node offered as its closer.
        closer: NodeId,
    },
    /// One of the two is already in a zone, and this says which and with whom.
    ///
    /// Carried rather than left to a second call: a caller offering to re-pair
    /// needs to know what it would break.
    AlreadyPaired {
        /// The node that is already spoken for.
        node: NodeId,
        /// Who it is paired with.
        with: NodeId,
    },
    /// A node cannot close the zone it opens.
    ItsOwnCloser(NodeId),
}

impl fmt::Display for PairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchNode(node) => write!(f, "no node {node}"),
            Self::NotAKind(node) => write!(f, "node {node} is not an application node"),
            Self::OpensNothing(node) => write!(f, "node {node}'s kind opens no zone"),
            Self::WrongCloser { opener, closer } => write!(
                f,
                "node {closer} is not the kind node {opener} declares closes it"
            ),
            Self::AlreadyPaired { node, with } => {
                write!(f, "node {node} is already in a zone with node {with}")
            }
            Self::ItsOwnCloser(node) => write!(f, "node {node} cannot close itself"),
        }
    }
}

impl std::error::Error for PairError {}

/// ★★★★★ R1943 — what a node is, with respect to zones.
///
/// Three arms, and the third is why this is not a `bool`: a node can be an
/// opener that is **not yet paired**, which is a state the reference reaches
/// routinely (its own operator creates the two nodes before pairing them) and
/// cannot name — there, an unpaired opener is simply an opener whose stored id
/// resolves to nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InZone {
    /// This node opens a zone, closed by that node.
    Opens(NodeId),
    /// This node closes a zone, opened by that node.
    Closes(NodeId),
    /// This node's kind opens a zone and nothing closes it yet.
    OpensNothingYet,
}

/// Why a value could not be authored on a port (R1594).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PortValueError<T, V> {
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
    /// ★★★★★ R1939 — the value is of the port's type and the port's own
    /// declaration will not take it ([`NodeKind::takes`]).
    ///
    /// Its own arm rather than a [`Self::WrongType`] because the repairs are
    /// different, which is [`Violation::Incompatible`](crate::Violation::Incompatible)'s
    /// reason: a wrong-typed value is fixed by writing a value of another type,
    /// and this is fixed by writing another value of the SAME type — or by
    /// taking the one this arm hands back.
    NotAdmitted {
        /// The port.
        port: PortRef,
        /// What that port wants, in a sentence — [`Admits::wants`].
        wants: String,
        /// The nearest value the same declaration WOULD have taken, when there
        /// is one. Carried rather than left to a second call, so a caller
        /// offering the repair cannot offer one a re-read of the declaration
        /// would refuse.
        instead: Option<V>,
    },
}

impl<T: fmt::Debug, V: fmt::Debug> fmt::Display for PortValueError<T, V> {
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
            Self::NotAdmitted {
                port,
                wants,
                instead,
            } => match instead {
                Some(near) => write!(f, "port {port} wants {wants}, such as {near:?}"),
                None => write!(f, "port {port} wants {wants}"),
            },
        }
    }
}

impl<T: fmt::Debug, V: fmt::Debug> std::error::Error for PortValueError<T, V> {}

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
    /// ★★★★★ R1920 — **the node is this tree's own interface end, and a tree
    /// cannot be asked to give up the end of its own contract.**
    ///
    /// Measured at R1920's open, before this existed: removing a definition's
    /// [`NodeBody::Interface`] node SUCCEEDED, the tree kept the interface
    /// ports the node was the inside end OF, and [`Document::validate`]
    /// answered with an empty list. So a group's contract could lose the half a
    /// reader wires to, in one ordinary delete, and nothing in the document
    /// would say so.
    ///
    /// Refused rather than reported, because a refusal makes the broken state
    /// unrepresentable while a report only describes it after the fact — and
    /// this is the crate's first per-node refusal, which is what the reference
    /// census's `a node cannot refuse an edit` named.
    InterfaceEnd {
        /// The tree whose contract this node is an end of.
        tree: TreeId,
        /// The interface node.
        node: NodeId,
        /// Which end it is.
        side: InterfaceSide,
    },
    /// ★★★★★ R1922 — an interface end was placed in a tree **nothing
    /// instantiates**, so it would materialise a contract with no outside.
    ///
    /// [`ROOT`] is that tree: a group instance names a definition, and no node
    /// can name the root. Measured at R1922's open, this placement SUCCEEDED
    /// and [`Document::validate`] reported nothing at all about it — the one of
    /// this round's three findings that had no diagnosis whatsoever.
    RootHasNoOutside {
        /// The tree the end was placed in.
        tree: TreeId,
        /// Which end it would have been.
        side: InterfaceSide,
    },
    /// ★★★★★ R1922 — that side of this tree's interface **already has** its
    /// inside end.
    ///
    /// [`Tree::interface_node`] documents itself as answering *the sole node
    /// materialising `side`*, and it answers with the first — so a second one
    /// is drawn, is wired to by nothing, and cannot be found by the accessor
    /// that is supposed to be about it. `validate` reported it after the fact;
    /// this refuses it before.
    InterfaceEndTaken {
        /// The tree.
        tree: TreeId,
        /// The side that is already materialised.
        side: InterfaceSide,
        /// The node already holding it.
        held_by: NodeId,
    },
    /// ★★★★★ R1999 — the node kind does not declare itself at home in **this
    /// kind of graph** ([`NodeKind::at_home`]).
    ///
    /// Both members are identity tokens and not sentences: `kind` is
    /// [`NodeKind::name`], and `graph` is the taxonomy's graph kind rendered by
    /// its own `Debug`. The crate has no way to phrase an application's
    /// vocabulary and does not try — what it guarantees is that a refusal names
    /// *which* kind and *which* graph, which is what lets a screen write the
    /// sentence its own readers use.
    KindNotAdmitted {
        /// The tree the node would have gone in.
        tree: TreeId,
        /// The node kind that was refused.
        kind: String,
        /// The kind of graph that refused it.
        graph: String,
    },
    /// ★★★★★ R1922 — a group instance would put a tree **inside itself**.
    ///
    /// Directly, or through any nesting that already reaches back. `validate`
    /// reports the resulting document as recursive; this refuses the edit that
    /// would make it so.
    WouldContainItself {
        /// The tree the instance would go in.
        tree: TreeId,
        /// The definition it names.
        definition: TreeId,
        /// ★ The chain that would close, host first — the definitions actually
        /// carrying the recursion. The reference refuses the same nesting with
        /// one flat sentence whether it is direct or four groups deep, so the
        /// trees in between are never named there.
        chain: Vec<TreeId>,
    },
    /// No such link in that tree.
    NoSuchLink {
        /// The tree that was searched.
        tree: TreeId,
        /// The link that is not in it.
        link: LinkId,
    },
    /// ★ R1925 — that tree's interface has no such section.
    NoSuchSection {
        /// The tree whose interface was addressed.
        tree: TreeId,
        /// The section asked for.
        section: crate::section::SectionId,
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
    /// Another node in the tree has already authored that name (R1682).
    ///
    /// The refusal the reference toolkit does not have. Measured on 6.11.1
    /// offscreen: naming a second sibling what the first is called is accepted,
    /// both hold it, and the by-name lookup then answers one of the two and
    /// says nothing about the other. A name that does not identify is a name a
    /// caller cannot address anything by, which is the whole reason
    /// [`Document::relabel`] is a verb rather than a field write.
    LabelTaken {
        /// The tree that was searched.
        tree: TreeId,
        /// The name asked for.
        label: String,
        /// The node that already answers to it.
        held_by: NodeId,
    },
    /// A name was asked for that has nothing in it (R1682).
    ///
    /// Separate from clearing the name, which is [`Document::relabel`] with
    /// `None` and means "call it what its body is called". A node whose
    /// authored name is blank displays nothing at all, and silently reading
    /// that as "clear it" would be the edit deciding what the caller meant.
    LabelEmpty {
        /// The tree it is in.
        tree: TreeId,
        /// The node that would have been left showing nothing.
        node: NodeId,
    },
    /// ★★★★★ R1933 — a socket type the tree does not admit on its interface.
    ///
    /// ⚠ `ty` is the type's `Debug` spelling and that is a deliberate, stated
    /// compromise: this enum is not generic over the taxonomy's socket type, so
    /// the alternative is a refusal that names no type at all. A taxonomy whose
    /// `Debug` reads badly to a person will read badly here — which is a reason
    /// for an application to catch this arm and re-word it, the same seam
    /// `LandError::NoRoom` names in R1930.
    TypeNotAdmitted {
        /// The tree that refused it.
        tree: TreeId,
        /// The type, spelled as the taxonomy debugs it.
        ty: String,
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
            // ★ R1920 — says what would be LOST, not merely that it was
            // refused: a reader who is told "you may not" and not "this is the
            // end of the contract your callers wire to" learns nothing they
            // can act on.
            // ★ R1922 — each says what would be LOST or what already holds the
            // place, not merely that it refused.
            Self::RootHasNoOutside { tree, side } => write!(
                f,
                "tree {} is the root, which nothing instantiates, so a {} \
                 interface end there would have no outside to be wired from",
                tree.0,
                side.wire_word()
            ),
            Self::KindNotAdmitted { tree, kind, graph } => write!(
                f,
                "tree {} is a {graph} graph, which is not one a {kind} is at home in",
                tree.0
            ),
            Self::InterfaceEndTaken {
                tree,
                side,
                held_by,
            } => write!(
                f,
                "tree {}'s {} interface end is already node {}",
                tree.0,
                side.wire_word(),
                held_by.0
            ),
            Self::WouldContainItself {
                tree,
                definition,
                chain,
            } => write!(
                f,
                "placing tree {} in tree {} would close the chain {}",
                definition.0,
                tree.0,
                chain
                    .iter()
                    .map(|t| t.0.to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
            Self::InterfaceEnd { tree, node, side } => write!(
                f,
                "node {} is tree {}'s own {} interface end, and removing it \
                 would leave the tree's contract with no inside end",
                node.0,
                tree.0,
                side.wire_word()
            ),
            Self::NoSuchSection { tree, section } => {
                write!(f, "tree {}'s interface has no {section}", tree.0)
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
            // ★★★★ R1719 — named by the LABEL, not by the index. This sentence
            // reaches a person: the node lab puts it on the toast, and it read
            // `refused: node 4 in tree 0 is already called "P-01"` in front of
            // somebody looking at a canvas where that card is called `R-01`.
            // Found by photographing the screen — every gate over it was green,
            // because they all asked whether the refusal happened.
            //
            // The index is dropped rather than kept alongside because here it
            // adds nothing either audience can act on: a label is unique in its
            // tree, which is the whole reason this refusal exists, so the label
            // identifies the node the index would have. The other arms keep
            // their indices — they are about nodes a caller named by index.
            Self::LabelTaken { tree, label, .. } => write!(
                f,
                "another card in tree {} is already called {label:?}",
                tree.0
            ),
            Self::LabelEmpty { tree, node } => write!(
                f,
                "node {} in tree {} would be left showing no name at all",
                node.0, tree.0
            ),
            Self::TypeNotAdmitted { tree, ty } => {
                write!(f, "tree {} does not admit {ty} on its interface", tree.0)
            }
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
    /// The two NODES may not be wired, whatever their ports carry (R1885).
    ///
    /// Its own arm rather than a [`Self::TypeMismatch`], because the types
    /// match — that is what makes this refusal worth having. An author fixes a
    /// type mismatch by finding a conversion and fixes this by changing one of
    /// the two ends, and [`Refusal::end`] says which.
    Incompatible {
        /// The producing socket.
        from: Socket,
        /// The consuming socket.
        to: Socket,
        /// Which end to change, and the application's sentence saying why.
        refusal: Refusal,
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
            // R1885 — the application's own sentence, verbatim. A refusal this
            // crate cannot phrase is one it must not paraphrase.
            Self::Incompatible { from, to, refusal } => {
                let end = match refusal.end {
                    Side::Output => from,
                    Side::Input => to,
                };
                write!(
                    f,
                    "{from} may not reach {to}: {} (change {end})",
                    refusal.because
                )
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
