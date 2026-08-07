//! `pinion-node-graph` — the mechanism of a Blender/Blueprint-class node system,
//! with the taxonomy left to the application (R1577, Phase B).
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
//!   Blender keeps those two apart, and keeps a third copy for muted nodes.
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
//!   extension point, because the default *is* the identity: eleven Blender
//!   node types register a per-node C callback to redirect their pass-through
//!   and not one of them computes anything this default does not already
//!   produce.
//! * **Frames** — [`NodeBody::Frame`] is a node whose whole content is what it
//!   contains, and [`Node::parent`] is the relation. Read across a tree that
//!   relation is a **forest**: [`Document::set_parent`] refuses a container that
//!   is not a frame and a containment that would close a cycle, naming the
//!   chain, where Blender states both rules as assertions its shipped build
//!   compiles out. Every gesture over it — [`Document::enframe`],
//!   [`Document::unframe`], [`Document::translate`] — is a call site of one
//!   derivation, [`Document::outermost`], which Blender writes three times.
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
//! * **A standing check** — [`Document::validate`], for documents that arrive
//!   from a file or a peer and have promised nothing.
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

mod appearance;
mod bypass;
mod eval;
mod fragment;
mod frame;
mod group;
mod layout;
mod model;
mod numbering;
mod partition;
mod select;
mod swap;
#[cfg(test)]
mod tests;

pub use appearance::{Appearance, VisiblePorts};
pub use bypass::{Bridge, Passthrough, Rewired, Route};
pub use eval::Evaluator;
pub use fragment::{
    Crossings, Definitions, DuplicateError, ExtractError, Fragment, InsertError, Inserted, Severed,
};
pub use frame::{Enframed, Orphaned, ParentError};
pub use group::{
    EditPath, GroupError, Grouped, NestError, PathEntry, PathError, UngroupError, Ungrouped,
    Violation,
};
pub use layout::{Extent, Layered, Organic, Placement, Quality};
pub use model::{
    ConnectError, Connected, Conversion, Document, DroppedLink, EditError, Interface,
    InterfaceSide, KindPort, Link, LinkId, Node, NodeBody, NodeId, NodeKind, Port, PortRef,
    PortValueError, ROOT, Removed, Side, Signature, Socket, Tree, TreeId,
};
pub use partition::{PortChange, RepartitionError, Repartitioned, Sharing};
pub use select::{Grow, Grown, Reach, SelectError};
pub use swap::{Carried, Swapped};

/// Re-exported so a consumer can name the boundary derivation this crate's
/// group operations are built on without adding a second dependency.
pub use pinion_graph::group as boundary;
