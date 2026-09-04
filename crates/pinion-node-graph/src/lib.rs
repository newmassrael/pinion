//! `pinion-node-graph` — the mechanism of a DCC/visual script-class node
//! system, with the taxonomy left to the application (R1577, Phase B).
//!
//! # Why this crate exists
//!
//! A node graph is one of the things a professional tool is *built out of* — a
//! material editor, a compositor, a visual-scripting canvas, a protocol
//! dissection pipeline. pinion already had every hard *drawing* piece of one,
//! and it had them in an example: `hello-node-editor` is nine thousand lines,
//! and an application that wanted a node system had to copy them. Copying a
//! renderer is a nuisance; copying a *model* is a fork, because the invariants
//! go with it and then drift.
//!
//! What is here is everything a node system needs that is not the application's
//! own subject matter. What is not here is what a node *does* — that arrives as
//! an implementation of [`NodeKind`], which is a taxonomy the application
//! already has and this crate could only ever guess at. The split was written
//! down inside that example long before this crate existed, as the thing "a
//! future trait/registry-dispatched `pinion-node-graph` crate would provide".
//!
//! # What you supply, and what you get
//!
//! You supply [`NodeKind`]: a socket type, a value, port lists, and an
//! `evaluate`. You get:
//!
//! * **A typed model** — [`Document`] of [`Tree`]s, [`Node`]s and [`Link`]s
//!   whose structural edits maintain their own invariants. [`Document::connect`]
//!   checks that the sockets exist, that a value can cross between their types,
//!   that an input takes one link, and that the wire does not close a cycle —
//!   and *names* whichever of those failed, including the path a refused wire
//!   would have closed.
//! * **A directed type relation** — [`NodeKind::conversion`] says whether and
//!   *how* a value crosses from one socket type to another, so a taxonomy whose
//!   scalar broadcasts into a vector without the vector narrowing back is
//!   expressible at all: equality is symmetric, and that relation is not. It is
//!   declared once, **as the conversion itself**, which is what makes "this
//!   wire is legal" and "this is what arrives along it" unable to disagree.
//!   the DCC keeps those two apart, and keeps a third copy for muted nodes.
//! * **An end that moves** — [`Document::relink`] re-aims one end of a link
//!   that is already there, keeping its [`LinkId`], its mute and its place in
//!   the order, refusing atomically, and naming the refusal. Not
//!   disconnect-then-connect, which mints a new id, destroys the link when the
//!   destination refuses it, and lets a link block its own move; see that
//!   module for the measurement against the reference's own relocation verb.
//! * **Groups** — [`Document::group`] collapses a selection into a re-usable
//!   definition plus one instance, with the interface **derived** from the
//!   links that cross the boundary. [`Document::instantiate`] places another
//!   instance; [`Document::ungroup`] inlines one back.
//! * **A boundary that moves** — [`Document::group_insert`] and
//!   [`Document::group_separate`] change which side of a group a node is on and
//!   re-derive the interface from the partition that results, reconnecting
//!   every value whose crossing disappeared. [`Sharing`] says whether the
//!   definition is edited in place or forked first, and the answer names the
//!   instances that came along.
//! * **Fragments** — [`Document::extract`] lifts a selection out as a
//!   serializable value carrying the definitions it depends on and the boundary
//!   it was cut from; [`Document::insert`] puts one anywhere, re-using the
//!   definitions that are already there; [`Document::duplicate`] is the two in
//!   one call. Copy, paste and duplicate are call sites of this.
//! * **Nesting that cannot recurse** — a placement that would make a definition
//!   contain itself is refused, and the refusal names the containment chain.
//! * **An edit path** — [`EditPath`], the breadcrumb into nested definitions,
//!   including [`EditPath::prune`] for when the document changes underneath it.
//! * **Bypass** — [`Document::set_bypassed`] takes a node out of the graph's
//!   *meaning* without taking it out of the graph, and
//!   [`Document::passthrough`] derives what flows through it: a bypassed node
//!   is the identity as far as its signature allows, and the outputs no input
//!   can feed are **named**. [`Document::dissolve`] and [`Document::detach`]
//!   apply the same derivation to the structure, so what a bypass does and what
//!   a delete leaves behind cannot disagree. A [`Link`] can be muted too, which
//!   is the opposite behaviour and therefore a different word. A [`Port`] may
//!   declare itself off that path ([`Port::no_passthrough`]) — the whole
//!   extension point, because the default *is* the identity: eleven the DCC
//!   node types register a per-node C callback to redirect their pass-through
//!   and not one of them computes anything this default does not already
//!   produce.
//! * **Frames** — [`NodeBody::Frame`] is a node whose whole content is what it
//!   contains, and [`Node::parent`] is the relation. Read across a tree that
//!   relation is a **forest**: [`Document::set_parent`] refuses a container that
//!   is not a frame and a containment that would close a cycle, naming the
//!   chain, where the DCC states both rules as assertions its shipped build
//!   compiles out. Every gesture over it — [`Document::enframe`],
//!   [`Document::unframe`], [`Document::translate`] — is a call site of one
//!   derivation, [`Document::outermost`], which the DCC writes three times.
//!   Deleting a frame hands its members to the frame *above* rather than to the
//!   canvas, and every operation that moves nodes between trees says what became
//!   of a container that stayed behind ([`Orphaned`]).
//! * **Looks that travel** — [`Appearance`] is what a node looks like, kept in
//!   the document because a group collapse and a paste move nodes between
//!   trees, and kept apart from the graph's meaning because only one of the two
//!   may be read by the evaluator. [`Document::visible_ports`] is the
//!   derivation a renderer needs and only the document can make.
//! * **Evaluation** — [`Document::evaluator`], memoised, descending into groups,
//!   keyed by *instance* so two instances of one definition do not share a
//!   value.
//! * **Two planes** — a [`Port`] carries a value or **control**
//!   ([`Flow`]), and the two obey opposite laws
//!   ([`Flow::multiplicity`]): a value input takes one producer, a control
//!   output takes one successor. A value cycle is a contradiction and a control
//!   cycle is a **loop** — authorable, named statically by
//!   [`Document::control_loops`]. [`Document::run`] derives the execution order
//!   from the control plane and **descends into group instances**, so a step's
//!   [`Step::instance`] says which occurrence it happened in.
//! * **Memory** — [`NodeBody::Delay`] is a value one step behind: SSA's φ,
//!   Lustre's `pre`, Simulink's Unit Delay. It is the only node a value cycle
//!   may pass through, which is Lustre's causality rule falling out of one
//!   predicate, and its register lives in a [`Machine`] addressed by
//!   [`Instance`] so two instances of one counting group count separately.
//!   [`Document::tick`] advances every register **at one instant**;
//!   [`Document::settle`] runs to a fixed point. A run *reads* the machine and
//!   never advances it, so a tick's outcome is a function of the document and
//!   the registers rather than of the walk.
//! * **Ports that belong to the node** — a kind may declare that one run of
//!   its ports repeats ([`Variadic`]), and then each node carries the
//!   [`Item`]s of that run: a sequencer with four branches and one with two are
//!   the same kind. [`Document::insert_item`], [`Document::remove_item`] and
//!   [`Document::move_item`] are the reference's ten variadic-pin commands as
//!   two verbs and a number, and each one re-points **every** link and
//!   authored value that the change moved — including the fixed ports past the
//!   run, which is where the engine ships a `//@TODO` instead. A removal
//!   *names* the wires it had to cut, where the reference's returns `void`.
//! * **A standing check** — [`Document::validate`], for documents that arrive
//!   from a file or a peer and have promised nothing.
//! * **Where the canvas is pointed** — [`Camera`] is the one affine
//!   (`screen = world · zoom + pan`) with its own inverse, [`Fit`] frames the
//!   graph into a viewport and **says whether it fitted** ([`Fitted::complete`])
//!   rather than reporting success while showing a corner, and [`Margin`]
//!   makes the clear space declare whether it is canvas units or screen pixels
//!   — two different scales for one graph, and the reference toolkit's
//!   `fitInView` offers neither by name.
//! * **A document that can be put away** — [`Archive`] writes the graph, the
//!   camera and the selection as text, and [`Archive::read`] answers an
//!   [`Opening`]: a plan computed **before** anything is installed, naming
//!   which of four things stopped it ([`Unreadable`]), what the document's own
//!   [`validate`](Document::validate) says about it, and what would not
//!   survive ([`Dropped`]). The reference toolkit's `restoreState` runs the
//!   same check pass privately and answers `bool`; the application's own
//!   extras are parsed independently of the graph, so a screen whose saved
//!   state moved on still gets its graph back and is told what was left.
//!
//! There is no renderer here, no reactive runtime and no window: this is pure
//! data, so a node system's rules are testable without one.
//!
//! ```
//! use pinion_node_graph::{Document, NodeBody, NodeKind, Port, Socket, ROOT};
//!
//! // The taxonomy is yours. This one adds numbers.
//! #[derive(Clone, PartialEq, Debug)]
//! enum Op {
//!     Constant(i64),
//!     Add,
//! }
//!
//! impl NodeKind for Op {
//!     type Type = ();
//!     type Value = i64;
//!     fn name(&self) -> String {
//!         match self {
//!             Op::Constant(_) => "Constant".into(),
//!             Op::Add => "Add".into(),
//!         }
//!     }
//!     fn inputs(&self) -> Vec<Port<(), i64>> {
//!         match self {
//!             Op::Constant(_) => Vec::new(),
//!             Op::Add => vec![Port::new("A", ()), Port::new("B", ())],
//!         }
//!     }
//!     fn outputs(&self) -> Vec<Port<(), i64>> {
//!         vec![Port::new("Out", ())]
//!     }
//!     fn evaluate(&self, inputs: &[Option<i64>]) -> Vec<Option<i64>> {
//!         match self {
//!             Op::Constant(n) => vec![Some(*n)],
//!             Op::Add => vec![inputs.first().copied().flatten().zip(
//!                 inputs.get(1).copied().flatten(),
//!             ).map(|(a, b)| a + b)],
//!         }
//!     }
//! }
//!
//! let mut doc = Document::new("root");
//! let two = doc.add_node(ROOT, NodeBody::Kind(Op::Constant(2)), 0, 0).unwrap();
//! let three = doc.add_node(ROOT, NodeBody::Kind(Op::Constant(3)), 0, 60).unwrap();
//! let add = doc.add_node(ROOT, NodeBody::Kind(Op::Add), 200, 30).unwrap();
//! let sink = doc.add_node(ROOT, NodeBody::Kind(Op::Add), 400, 30).unwrap();
//! doc.connect(ROOT, Socket::new(two, 0), Socket::new(add, 0)).unwrap();
//! doc.connect(ROOT, Socket::new(three, 0), Socket::new(add, 1)).unwrap();
//! doc.connect(ROOT, Socket::new(add, 0), Socket::new(sink, 0)).unwrap();
//! doc.connect(ROOT, Socket::new(three, 0), Socket::new(sink, 1)).unwrap();
//! assert_eq!(doc.evaluate(ROOT, sink), vec![Some(8)]);
//!
//! // Collapse the adder into a re-usable definition. Nothing was authored: the
//! // interface is DERIVED from what crosses the boundary — two values in, one
//! // out — and the instance is wired exactly where the selection was.
//! let made = doc.group(ROOT, &[add], "Sum").unwrap();
//! let definition = doc.tree(made.definition).unwrap();
//! assert_eq!(definition.interface().inputs().len(), 2);
//! assert_eq!(definition.interface().outputs().len(), 1);
//! assert_eq!(doc.evaluate(ROOT, sink), vec![Some(8)]);
//!
//! // A second instance of the SAME definition, fed differently. The memo is
//! // keyed by instance, so this does not disturb the first.
//! let seven = doc.add_node(ROOT, NodeBody::Kind(Op::Constant(7)), 0, 200).unwrap();
//! let again = doc.instantiate(ROOT, made.definition, 200, 200).unwrap();
//! doc.connect(ROOT, Socket::new(seven, 0), Socket::new(again, 0)).unwrap();
//! doc.connect(ROOT, Socket::new(seven, 0), Socket::new(again, 1)).unwrap();
//! assert_eq!(doc.evaluate(ROOT, again), vec![Some(14)]);
//! assert_eq!(doc.evaluate(ROOT, made.node), vec![Some(5)]);
//!
//! assert!(doc.validate().is_empty());
//! ```

mod admitted;
mod appearance;
mod archive;
mod arrange;
mod autowire;
mod beacon;
mod bypass;
mod debug;
mod definition;
mod deploy;
mod describe;
mod eval;
mod focus;
mod fragment;
mod frame;
mod group;
mod insert;
mod items;
mod landing;
mod layout;
mod machine;
mod model;
mod naming;
mod naming_scope;
mod numbering;
mod observed;
mod occupancy;
mod palette;
mod partition;
mod relink;
mod reroute;
mod review;
mod run;
mod section;
mod select;
mod sighted;
mod split;
mod swap;
#[cfg(test)]
mod tests;
mod view;
mod warning;

pub use admitted::Admitted;
pub use appearance::{
    Appearance, Drawn, Faces, Hidden, PutAway, PutAwayRefusal, Tint, VisiblePorts,
};
pub use archive::{Archive, Condition, Dropped, Opening, REVISION, Unreadable, Unwritable};
pub use arrange::{
    Align, ArrangePass, ArrangeTail, Axis, Distribute, Edge, Stack, Straighten, Straightened,
};
pub use autowire::{Arrival, AutowireError, Autowired, Declined, Uptake};
pub use beacon::{BeaconError, Gathered, Spread};
pub use bypass::{Bridge, Passthrough, Rewired, Route};
pub use debug::{
    BreakError, Breakpoints, Command, Direction, Halt, Inspectable, Landing, NodeSite, Occurrence,
    Paused, PortSite, Reading, Session, Stride, Timeline, WatchError, Watches,
};
pub use definition::{DefinitionAct, DefinitionError, RemovedTree, Used};
pub use deploy::{Bringup, Configured, Deployed, Placed, Plan, Uncarried, Unplannable};
pub use describe::{Carrying, PortTooltip};
pub use eval::{Descent, Evaluator};
pub use focus::{Focus, Focused, Relatedness, Tie};
pub use fragment::{
    Crossings, Definitions, DuplicateError, ExtractError, Fragment, InsertError, Inserted, Renamed,
    Severed,
};
pub use frame::{Enframed, Orphaned, ParentError};
pub use group::{
    EditPath, GroupError, Grouped, NestError, PathEntry, PathError, UngroupError, Ungrouped,
    Violation,
};
pub use insert::{Room, RoomError, Splice, SpliceError, Spliced, Verdict, Widening};
pub use items::{Item, ItemChange, ItemEdit, ItemEditTail, ItemError, Items, Variadic};
pub use landing::{Berth, LandError, Landed, Landfall};
pub use layout::{Extent, Layered, Organic, Placement, Quality};
pub use machine::{Committed, ForceError, Machine, Tick};
pub use model::{
    Act, Admission, Admits, ConnectError, Connected, Container, Control, Conversion, Described,
    Description, Document, DroppedLink, EditError, Flow, Found, InZone, Instance, Interface,
    InterfaceSide, Judged, KindPort, Link, LinkId, Matched, Multiplicity, Node, NodeBody, NodeId,
    NodeKind, PairError, Port, PortRef, PortValueError, PortValueResult, ROOT, Refusal, Relabelled,
    Removed, Side, Signature, Socket, Tree, TreeId, crossing,
};
pub use naming::{Labelled, NameSource, PortName};
pub use naming_scope::{Copying, Naming};
pub use observed::{
    AdoptError, Discovery, Judgement, Layers, LinkLayer, Observation, ObserveError, Reachability,
    Standing,
};
pub use occupancy::Occupants;
pub use palette::{Palette, palette_of, type_palette};
pub use partition::{PortChange, RepartitionError, Repartitioned, Sharing};
pub use relink::{RelinkError, Relinked};
pub use reroute::{Passing, RerouteError, Rerouted};
pub use review::{Fault, Finding, Fitness, Review, Weight};
pub use run::{Run, RunError, Step, Stop};
pub use section::{InterfacePort, Section, SectionBreach, SectionId, SwitchRefusal};
pub use select::{Grow, Grown, Reach, SelectError};
pub use sighted::{Sighted, SightedTopology, Sighting, Vantage};
pub use split::{
    AddressedPort, Composition, NoSuchMember, NotRecombinable, NotSplittable, PortPath, Recombined,
    RoundTrip, SplitChange, Splittable, round_trips,
};
pub use swap::{Carried, RetypeError, SwapError, Swapped};
pub use view::{Camera, Fit, Fitted, Margin, Unframed, ZoomRange};
pub use warning::{Objection, Surroundings, Warning};

/// Re-exported so a consumer can name the boundary derivation this crate's
/// group operations are built on without adding a second dependency.
pub use pinion_graph::group as boundary;
