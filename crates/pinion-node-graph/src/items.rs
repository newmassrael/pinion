//! R1632 — a port count is a property of the **node**, not of its kind.
//!
//! [`NodeKind::inputs`] and
//! [`NodeKind::outputs`] answer per *kind*, so every
//! node of a kind has the same ports. That is right for most nodes and wrong
//! for a whole class of them: a sequencer gains another branch, a selector
//! gains another option, a layer stack gains another layer, a bundle gains
//! another member. Both references have the class and both reach it the same
//! way — by giving each such node type its own hand-written pin arithmetic.
//!
//! # The vocabulary
//!
//! A kind declares that one **run** of its ports repeats
//! ([`NodeKind::variadic`] answering a [`Variadic`]),
//! and each node carries the [`Item`]s of that run ([`Node::items`](crate::Node::items)). Three
//! operations edit them — [`Document::insert_item`], [`Document::remove_item`],
//! [`Document::move_item`] — and between them they are the reference's whole
//! command set:
//!
//! | Reference command | Here |
//! |---|---|
//! | `AddExecutionPin`, `SoundCueGraph::AddInput`, `AnimGraph::AddBlendListPin` | `insert_item(.., side, count, ..)` |
//! | `InsertExecutionPinBefore` / `InsertExecutionPinAfter` | `insert_item(.., i)` / `insert_item(.., i + 1)` |
//! | `AddOptionPin` | `insert_item(.., Side::Input, count, ..)` |
//! | `RemoveExecutionPin`, `RemovePinAt`, `DeleteInput`, `RemoveBlendListPin` | `remove_item(.., i)` |
//! | `RemoveOptionPin` ("removes the **last**") | `remove_item(.., count - 1)` |
//! | the DCC's `socket_items` add / remove-by-index / remove-active | the same two |
//! | the DCC's `socket_items` **move** | `move_item` — the engine has no such command |
//!
//! # Why re-indexing is the whole difficulty
//!
//! A [`Socket`](crate::Socket) addresses a port **by index**, so removing item *i* does not
//! merely delete two ports: every link and every authored value on a port after
//! it must come with it, and the ports of the *fixed* part of the signature
//! that sit past the run move too. Getting that wrong does not fail loudly —
//! it silently re-points wires at the wrong sockets.
//!
//! That is not a hypothetical. The engine's own blend-list node ships with the
//! defect as a comment: its blend-list editor node's
//! `RemovePinFromBlendList` reads
//!
//! ```text
//! //@TODO: ANIMREFACTOR: Need to handle moving pins below up correctly
//! ```
//!
//! at 5.8.1. So every edit here goes through **one** correspondence — old
//! [`PortRef`] to new — and that one map drives the links and the authored
//! values together ([`Document::remap_ports`]), because a link kept while its
//! value is dropped is exactly the corruption this is about.
//!
//! # Where this is past the references
//!
//! * **The removal says what it severed.** The engine's `RemoveExecutionPin`
//!   destroys the pin, and its pin's own destructor calls `BreakAllPinLinks()`
//!   on the way out; the command answers `void`. Here the wires and the
//!   authored values that could not survive are named in [`ItemChange`], so an
//!   editor can undo the edit or tell the author what it cost.
//! * **Which kinds are variadic is a declaration.** One [`Variadic`] fixes the
//!   run's place, its template, and its bounds, so "may I remove one" and "what
//!   happens when I do" cannot disagree. The engine spreads the same facts over
//!   its add-pin interface's `CanAddPin`, a per-class `CanRemoveExecutionPin`,
//!   a per-class `GetPinNameGivenIndex` and a `RemoveInputPin` whose interface
//!   default is an **empty body** — a node type that implements the interface
//!   and forgets the remover silently does nothing.
//! * **No alphabet ceiling.** That interface's `GetMaxInputPinsNum`
//!   returns `'Z' - 'A'` because the pins are *named* `A`…`Z` — an arity limit
//!   that is a consequence of a naming scheme. Here the ordinal is applied to
//!   the template's name when the resolved signature is derived, so a name can
//!   never go stale (the engine re-runs a renaming loop after every insert and
//!   every remove) and 26 is not a number this crate knows.
//! * **An item may carry its own type and name.** Measured against the DCC at
//!   `8cf50599`: of its socket-item accessors, all but the index switch's
//!   declare `has_type` or `has_name`, so a run of *interchangeable* ports is
//!   the exception there rather than the rule. The engine cannot express it at
//!   all.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{
    Document, EditError, Flow, KindPort, Link, NodeBody, NodeId, NodeKind, Port, PortRef, Side,
    Signature, TreeId,
};

/// A kind's declaration that one run of its ports **repeats** (R1632).
///
/// The run is contiguous and sits at a fixed place in the kind's own port list,
/// so a kind can have fixed ports before it (the engine's `Sequence` takes its
/// `Execute` first) and after it (its `Select` takes the `Index` last, which is
/// the case that makes re-indexing observable).
///
/// One declaration and not four hooks: the place, the template, the floor and
/// the ceiling are read together by every operation, so no two of them can
/// describe different runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variadic<T, V> {
    at: u32,
    item: Vec<Port<T, V>>,
    min: u32,
    max: Option<u32>,
}

impl<T, V> Variadic<T, V> {
    /// A run beginning at index `at` of the kind's own port list, each item
    /// contributing `item` — in order, and at least one port.
    ///
    /// An `item` of more than one port is not an exotic case: the engine's
    /// blend list adds a pose **and** a blend time per item, as two parallel
    /// arrays, through its blend-list runtime node's `AddPose`. It is also the case that
    /// distinguishes a correct re-index from one that shifts by one — with a
    /// single-port item the two are the same arithmetic.
    ///
    /// An **empty** `item` would make every item contribute nothing, so the run
    /// would have no observable length and its position would be undefined.
    /// Such a declaration is treated as no run at all ([`Self::is_empty`]), and
    /// the node's side answers [`ItemError::NotVariadic`] — a refusal rather
    /// than a silent run of zero-width items.
    #[must_use]
    pub fn at(at: u32, item: Vec<Port<T, V>>) -> Self {
        Self {
            at,
            item,
            min: 0,
            max: None,
        }
    }

    /// The fewest items this run may hold. Default zero.
    ///
    /// The engine states the same floor per node class, as arithmetic inside a
    /// gate: the execution-sequence node's `CanRemoveExecutionPin` is
    /// `NumOutPins > 2`.
    #[must_use]
    pub const fn at_least(mut self, min: u32) -> Self {
        self.min = min;
        self
    }

    /// The most items this run may hold. Default unbounded.
    #[must_use]
    pub const fn at_most(mut self, max: u32) -> Self {
        self.max = Some(max);
        self
    }

    /// Where the run starts in the kind's own port list.
    #[must_use]
    pub const fn start(&self) -> u32 {
        self.at
    }

    /// The ports one item contributes.
    #[must_use]
    pub fn template(&self) -> &[Port<T, V>] {
        &self.item
    }

    /// How many ports one item contributes.
    #[must_use]
    pub fn stride(&self) -> u32 {
        u32::try_from(self.item.len()).unwrap_or(u32::MAX)
    }

    /// The fewest items.
    #[must_use]
    pub const fn minimum(&self) -> u32 {
        self.min
    }

    /// The most items, or `None` for unbounded.
    #[must_use]
    pub const fn maximum(&self) -> Option<u32> {
        self.max
    }

    /// Whether this declaration describes no ports at all, in which case the
    /// kind is not variadic on that side after all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.item.is_empty()
    }

    /// `count` clamped into this run's declared bounds.
    #[must_use]
    pub fn clamp(&self, count: u32) -> u32 {
        count.max(self.min).min(self.max.unwrap_or(u32::MAX))
    }
}

/// One item of a node's variadic run — the facts the *author* supplies about
/// one repetition (R1632).
///
/// Both fields are `Option`-shaped for the same reason [`Node::label`](crate::Node::label) is: an
/// item that says nothing is named and typed by the kind's template, and that
/// is what a run of interchangeable ports wants. An item that says something
/// is the DCC's case, where a zone item, a bundle member or a bake item all
/// carry a name the author typed and a socket type they chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// Spelled out because a field-level `serde(default)` otherwise demands
// `T: Default` of every taxonomy's socket type — the defaults here are an
// absent label and an empty list, neither of which needs one.
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
pub struct Item<T> {
    /// What the author called this item, or `None` to let the ordinal name it.
    ///
    /// This is the field that removes the reference's arity ceiling. Because an
    /// unlabelled item's name is **derived** from its ordinal every time the
    /// signature is resolved, a name cannot survive the item moving — the
    /// engine renumbers its pins by hand in a loop after each insert and each
    /// remove, and that loop is what its `'Z' - 'A'` limit is protecting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The socket type each of the item's ports carries, one slot per port of
    /// the kind's template, `None` leaving the template's own.
    ///
    /// Positional rather than a single type, because an item may contribute
    /// more than one port and there would otherwise be no saying which of them
    /// an authored type meant. A short list — including the empty one, which is
    /// the common case — leaves the rest of the template alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<Option<T>>,
}

/// Hand-written rather than derived: the derive would demand `T: Default` of
/// every taxonomy's socket type, and an item that authors *no* type is exactly
/// what this default is.
impl<T> Default for Item<T> {
    fn default() -> Self {
        Self::plain()
    }
}

impl<T> Item<T> {
    /// An item that authors nothing: the template names and types it.
    #[must_use]
    pub fn plain() -> Self {
        Self {
            label: None,
            types: Vec::new(),
        }
    }

    /// The same item under a name of the author's own.
    #[must_use]
    pub fn named(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The same item carrying `ty` on the template port at `slot`.
    ///
    /// **A control template port has no type and this does nothing to one** —
    /// the same honest arm [`Port::with_default`] takes, and for the same
    /// reason: control is not a value, so there is no field to write.
    #[must_use]
    pub fn typed(mut self, slot: usize, ty: T) -> Self
    where
        T: Clone,
    {
        if self.types.len() <= slot {
            self.types.resize_with(slot + 1, || None);
        }
        self.types[slot] = Some(ty);
        self
    }
}

/// The items of a node's variadic runs, one list per side (R1632).
///
/// Empty is not "no run": it is "nothing authored", and the run resolves to its
/// declared minimum of plain items. So a node that has never been edited stores
/// nothing, and a node whose items were edited stores the whole list —
/// two spellings of the same signature, which
/// [`Document::signature`] resolves identically and a test asserts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
pub struct Items<T> {
    #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
    inputs: Vec<Item<T>>,
    #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
    outputs: Vec<Item<T>>,
}

impl<T> Default for Items<T> {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }
}

impl<T> Items<T> {
    /// What is authored on one side.
    #[must_use]
    pub fn on(&self, side: Side) -> &[Item<T>] {
        match side {
            Side::Input => &self.inputs,
            Side::Output => &self.outputs,
        }
    }

    /// Whether nothing is authored on either side.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty()
    }

    /// Replace one side's list wholesale — the shape every edit here takes,
    /// because an edit is computed against the *resolved* list and written back
    /// as a whole.
    pub(crate) fn set(&mut self, side: Side, items: Vec<Item<T>>) {
        match side {
            Side::Input => self.inputs = items,
            Side::Output => self.outputs = items,
        }
    }
}

/// What one item edit did (R1632).
///
/// Every field is a fact the reference's equivalent command does not answer:
/// its `RemoveExecutionPin` returns `void` after `BreakAllPinLinks()`, so the
/// wires it took with it are simply gone.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemChange<K: NodeKind> {
    /// How many items the run holds now.
    pub items: u32,
    /// The ports the edit created, ascending. Empty for a remove and for a
    /// move.
    pub added: Vec<PortRef>,
    /// Ports that survived at a **different index**, old then new, ascending by
    /// the old one.
    ///
    /// The half a naive implementation forgets. It includes the fixed ports
    /// that sit *after* the run, which is the part with no visual cue at all.
    pub moved: Vec<(PortRef, PortRef)>,
    /// Links the edit had to cut, as they were — enough to re-make them.
    pub severed: Vec<Link>,
    /// Authored values the edit had to drop, with the port they were on.
    pub discarded: Vec<(PortRef, K::Value)>,
}

impl<K: NodeKind> Default for ItemChange<K> {
    fn default() -> Self {
        Self {
            items: 0,
            added: Vec::new(),
            moved: Vec::new(),
            severed: Vec::new(),
            discarded: Vec::new(),
        }
    }
}

impl<K: NodeKind> ItemChange<K> {
    /// Whether the edit cost the graph nothing — no wire cut, no value dropped.
    #[must_use]
    pub fn is_lossless(&self) -> bool {
        self.severed.is_empty() && self.discarded.is_empty()
    }
}

/// R1642 — which item edit to run, as a **value**.
///
/// The three edits keep their own methods, because they take different
/// arguments and answer differently-shaped changes; this names the CHOICE
/// between them, which is what a caller sends over a wire and what a schema
/// declares. Exactly the move R1638 made for [`ArrangePass`](crate::ArrangePass),
/// and made here for the same reason: the vocabulary was three string literals
/// matched at the call site, so it could not be enumerated, offered in a menu,
/// or published to a client, and every consumer re-wrote the three-way match.
///
/// The wire names are the ones the surface already accepted, so publishing the
/// vocabulary describes the wire rather than changing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum ItemEdit {
    /// [`Document::insert_item`] — add an item at a position.
    Add,
    /// [`Document::remove_item`] — take the item at a position away.
    Remove,
    /// [`Document::move_item`] — carry an item to another position, links and
    /// authored values with it.
    Move,
}

impl ItemEdit {
    /// Every edit, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 3] = [Self::Add, Self::Remove, Self::Move];

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Remove => "remove",
            Self::Move => "move",
        }
    }

    /// Parse a wire name back to the edit, or `None` — the inverse of
    /// [`name`](Self::name), which `r1642_every_item_edit_name_parses_back`
    /// holds it to.
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.name() == name)
    }

    /// The closed vocabulary, projected from [`ALL`](Self::ALL) rather than
    /// written out — see [`ArrangePass::WIRE_NAMES`](crate::ArrangePass::WIRE_NAMES).
    pub const WIRE_NAMES: [&'static str; Self::ARMS] = {
        let mut out = [""; Self::ARMS];
        let mut i = 0;
        while i < Self::ARMS {
            out[i] = Self::ALL[i].name();
            i += 1;
        }
        out
    };

    /// What this edit reads after the position — the one fact a caller needs
    /// that the edit's name does not carry.
    ///
    /// This is what makes the item verb a *conditional* call rather than a
    /// uniform one: the three edits do not merely constrain the last argument
    /// differently, they take a different NUMBER of them. No flat positional
    /// argument list describes `add:in:1` and `move:in:2:0` at once, which is
    /// the shape `ArgDomain::OneOfWith` exists for.
    #[must_use]
    pub const fn tail(self) -> ItemEditTail {
        match self {
            Self::Add => ItemEditTail::Label,
            Self::Remove => ItemEditTail::None,
            Self::Move => ItemEditTail::Destination,
        }
    }
}

/// What an [`ItemEdit`] reads from its trailing argument (R1642).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
pub enum ItemEditTail {
    /// Nothing — the edit is fully determined by the side and the position.
    None,
    /// A label for the new item.
    Label,
    /// The position to carry the item to.
    Destination,
}

impl ItemEditTail {
    /// Whether a caller must supply the trailing argument — the peer of
    /// [`ArrangeTail::required`](crate::ArrangeTail::required), and required for
    /// the same reason: a declaration that says "required" about a segment the
    /// dispatcher defaults, or "optional" about one it demands, is wrong in the
    /// direction that costs a client a refused call.
    ///
    /// A [`Label`](Self::Label) may be left out — an unnamed item is
    /// [`Item::plain`] and perfectly ordinary — while a
    /// [`Destination`](Self::Destination) cannot be defaulted: there is no
    /// position a move to nowhere means.
    #[must_use]
    pub const fn required(self) -> bool {
        match self {
            Self::Destination => true,
            Self::None | Self::Label => false,
        }
    }
}

/// Why an item edit could not happen (R1632).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemError {
    /// No such tree in this document.
    NoSuchTree(TreeId),
    /// No such node in that tree.
    NoSuchNode {
        /// The tree that was searched.
        tree: TreeId,
        /// The node that is not in it.
        node: NodeId,
    },
    /// The node's kind declares no variadic run on that side, so there is
    /// nothing to add to or take away.
    ///
    /// Also the answer for a structural body — a frame, a group instance, an
    /// interface node or a delay. A group instance's ports come from its
    /// definition's interface, which has its own verbs
    /// ([`Document::expose`](crate::Document::expose)), and answering here
    /// would give a definition's shape two owners.
    NotVariadic {
        /// The tree it is in.
        tree: TreeId,
        /// The node.
        node: NodeId,
        /// The side that was asked for.
        side: Side,
    },
    /// The run has no item at that position.
    NoSuchItem {
        /// The side that was asked for.
        side: Side,
        /// The position asked for.
        index: u32,
        /// How many items the run actually holds.
        items: u32,
    },
    /// The run is already at the ceiling its kind declared.
    AtMaximum {
        /// The side that was asked for.
        side: Side,
        /// The declared ceiling.
        max: u32,
    },
    /// The run is already at the floor its kind declared.
    ///
    /// The engine states the same refusal per node class and only as a *menu*
    /// gate (`CanRemoveExecutionPin`), so the operation itself will happily go
    /// below it when called from anywhere else.
    AtMinimum {
        /// The side that was asked for.
        side: Side,
        /// The declared floor.
        min: u32,
    },
}

impl From<EditError> for ItemError {
    fn from(error: EditError) -> Self {
        match error {
            EditError::NoSuchNode { tree, node } => Self::NoSuchNode { tree, node },
            other => Self::NoSuchTree(match other {
                EditError::NoSuchTree(tree)
                | EditError::NoSuchNode { tree, .. }
                | EditError::NotAKind { tree, .. }
                | EditError::NoSuchLink { tree, .. }
                | EditError::NoSuchInterfacePort { tree, .. }
                | EditError::NoSuchSection { tree, .. }
                | EditError::InterfaceEnd { tree, .. }
                | EditError::RootHasNoOutside { tree, .. }
                | EditError::InterfaceEndTaken { tree, .. }
                | EditError::WouldContainItself { tree, .. }
                | EditError::BypassWouldCycle { tree, .. }
                | EditError::LabelTaken { tree, .. }
                | EditError::LabelEmpty { tree, .. }
                | EditError::TypeNotAdmitted { tree, .. } => tree,
            }),
        }
    }
}

/// A kind's signature with a node's items spliced into it (R1632).
///
/// The **one** place an item becomes a port. Nothing else derives a name or a
/// type from an item, so the resolved signature, the correspondence an edit
/// computes, and what a renderer draws are one answer —
/// [`Document::signature`] and [`Document::set_kind`](crate::Document::set_kind)
/// are both call sites of this.
pub(crate) fn resolve<K: NodeKind>(kind: &K, items: &Items<K::Type>) -> Signature<K> {
    Signature {
        inputs: splice::<K>(
            kind.inputs(),
            kind.variadic(Side::Input),
            items.on(Side::Input),
        ),
        outputs: splice::<K>(
            kind.outputs(),
            kind.variadic(Side::Output),
            items.on(Side::Output),
        ),
    }
}

/// `items` cut down to what `kind` declares it may hold.
///
/// A side the kind has no run on keeps no items at all, and a side it bounds
/// keeps at most that many. Used where a node's kind *changes* underneath its
/// items ([`Document::set_kind`](crate::Document::set_kind)) — the one way a
/// stored item list can find itself describing a run that is not there.
pub(crate) fn clamp<K: NodeKind>(kind: &K, mut items: Items<K::Type>) -> Items<K::Type> {
    for side in [Side::Input, Side::Output] {
        let keep = match kind.variadic(side).filter(|run| !run.is_empty()) {
            None => 0,
            Some(run) => run.max.unwrap_or(u32::MAX) as usize,
        };
        if items.on(side).len() > keep {
            let mut kept = items.on(side).to_vec();
            kept.truncate(keep);
            items.set(side, kept);
        }
    }
    items
}

/// The resolved ports of one side: the kind's own list with the run spliced in.
///
/// Topping the authored list up to the run's declared minimum happens here
/// rather than at the call site, so a node that stored nothing and a node that
/// stored its minimum are the same signature by construction.
fn splice<K: NodeKind>(
    base: Vec<KindPort<K>>,
    run: Option<Variadic<K::Type, K::Value>>,
    stored: &[Item<K::Type>],
) -> Vec<KindPort<K>> {
    let Some(run) = run.filter(|run| !run.is_empty()) else {
        return base;
    };
    let mut owned;
    let items = if stored.len() < run.min as usize {
        owned = stored.to_vec();
        owned.resize_with(run.min as usize, Item::plain);
        &owned[..]
    } else {
        stored
    };
    let at = (run.at as usize).min(base.len());
    let mut out = Vec::with_capacity(base.len() + items.len() * run.item.len());
    out.extend(base.iter().take(at).cloned());
    for (ordinal, item) in items.iter().enumerate() {
        for (slot, template) in run.item.iter().enumerate() {
            let mut port = template.clone();
            port.name = resolved_name(&template.name, item.label.as_deref(), ordinal, run.stride());
            if let Some(Some(ty)) = item.types.get(slot)
                && let Flow::Value { ty: slot_ty, .. } = &mut port.flow
            {
                *slot_ty = ty.clone();
            }
            out.push(port);
        }
    }
    out.extend(base.iter().skip(at).cloned());
    out
}

/// What one item's port is called.
///
/// Three cases, and each is forced by a reference:
///
/// * **Unlabelled** — `"{template} {ordinal}"`. The engine's `Then 0`, `Then 1`,
///   derived rather than stamped, so it cannot be stale after a move.
/// * **Labelled, one port per item** — the label alone. The DCC's zone and
///   bundle items name their socket exactly.
/// * **Labelled, several ports per item** — `"{label} {template}"`, so the
///   item's own ports stay distinguishable. Neither reference reaches this
///   case: every accessor the DCC labels contributes one socket, and the
///   engine's multi-port item (the blend list) has no name at all. It is this
///   crate's generalisation and is spelled out rather than left to be
///   discovered.
fn resolved_name(template: &str, label: Option<&str>, ordinal: usize, stride: u32) -> String {
    match label {
        None => format!("{template} {ordinal}"),
        Some(label) if stride == 1 => label.to_owned(),
        Some(label) => format!("{label} {template}"),
    }
}

/// Which run an edit is about: a node's, on one side.
///
/// One value rather than three parameters passed down together, because every
/// step of an edit needs all three and a call that got two of them from one
/// place and the third from another would re-index the wrong node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Site {
    tree: TreeId,
    node: NodeId,
    side: Side,
}

/// A run as an edit finds it: where, what the kind declared, and what is there
/// now.
struct Found<K: NodeKind> {
    site: Site,
    run: Variadic<K::Type, K::Value>,
    items: Vec<Item<K::Type>>,
}

impl<K: NodeKind> Found<K> {
    /// How many items the run holds.
    fn count(&self) -> u32 {
        u32::try_from(self.items.len()).unwrap_or(u32::MAX)
    }

    /// The error for a position this run has no item at.
    fn no_such(&self, index: u32) -> ItemError {
        ItemError::NoSuchItem {
            side: self.site.side,
            index,
            items: self.count(),
        }
    }
}

impl<K: NodeKind> Document<K> {
    /// The variadic run `side` of this node has, if its kind declares one.
    ///
    /// `None` for a structural body, and `None` for an application kind that
    /// does not answer [`NodeKind::variadic`] — which is the default, so a
    /// taxonomy that has no variadic node writes nothing.
    #[must_use]
    pub fn variadic(
        &self,
        tree: TreeId,
        node: NodeId,
        side: Side,
    ) -> Option<Variadic<K::Type, K::Value>> {
        match &self.tree(tree)?.node(node)?.body {
            NodeBody::Kind(kind) => kind.variadic(side).filter(|run| !run.is_empty()),
            // R1934 — a reroute's two ports are derived from the chain it sits
            // on, so there is no run for an item to be added to.
            NodeBody::Group(_)
            | NodeBody::Interface(_)
            | NodeBody::Frame
            | NodeBody::Delay(_)
            | NodeBody::Reroute => None,
        }
    }

    /// This node's items on `side`, **resolved**: what is authored, topped up
    /// to the kind's declared minimum with plain items.
    ///
    /// `None` when the kind declares no run there. The list this answers is the
    /// one every edit operates on, so a node that stored nothing and a node
    /// that stored its minimum behave identically.
    #[must_use]
    pub fn items(&self, tree: TreeId, node: NodeId, side: Side) -> Option<Vec<Item<K::Type>>> {
        let run = self.variadic(tree, node, side)?;
        let stored = self.tree(tree)?.node(node)?.items.on(side);
        let mut items = stored.to_vec();
        items.resize_with(items.len().max(run.min as usize), Item::plain);
        Some(items)
    }

    /// Add an item to a node's variadic run at position `at`.
    ///
    /// `at` equal to the run's current length appends, which is the engine's
    /// `AddExecutionPin` / `AddOptionPin` / `AddInput` / `AddBlendListPin`;
    /// `at` and `at + 1` around an existing item are its
    /// `InsertExecutionPinBefore` and `InsertExecutionPinAfter`. There is one
    /// method because the reference's four names are one operation and a
    /// number.
    ///
    /// The ports of every item after the new one, and every fixed port past the
    /// run, move up — links and authored values with them, reported in
    /// [`ItemChange::moved`]. **An insert never severs anything**, which is a
    /// property of the operation rather than of this fixture, and
    /// [`ItemChange::is_lossless`] is how a caller can hold the crate to it.
    ///
    /// # Errors
    ///
    /// [`ItemError::NotVariadic`] when the kind declares no run on that side,
    /// [`ItemError::NoSuchItem`] for a position past the end, and
    /// [`ItemError::AtMaximum`] when the run is already as long as the kind
    /// allows.
    pub fn insert_item(
        &mut self,
        tree: TreeId,
        node: NodeId,
        side: Side,
        at: u32,
        item: Item<K::Type>,
    ) -> Result<ItemChange<K>, ItemError> {
        let found = self.find_run(tree, node, side)?;
        let count = found.count();
        if at > count {
            return Err(found.no_such(at));
        }
        if let Some(max) = found.run.max
            && count >= max
        {
            return Err(ItemError::AtMaximum { side, max });
        }
        let correspondence: Vec<Option<u32>> = (0..count)
            .map(|old| Some(if old < at { old } else { old + 1 }))
            .collect();
        let mut items = found.items.clone();
        items.insert(at as usize, item);
        let stride = found.run.stride();
        let added = (0..stride)
            .map(|slot| PortRef {
                side,
                index: found.run.at + at * stride + slot,
            })
            .collect();
        let mut change = self.rewrite(&found, items, &correspondence);
        change.added = added;
        Ok(change)
    }

    /// Remove the item at `at` from a node's variadic run.
    ///
    /// The engine's `RemoveExecutionPin`, `RemovePinAt`, `DeleteInput` and
    /// `RemoveBlendListPin`; its `RemoveOptionPin`, whose own description is
    /// "removes the **last** option input pin", is this with `at` one below the
    /// count.
    ///
    /// This is the direction that costs something, so it is the direction that
    /// reports: the wires on the item's own ports are cut and **named**
    /// ([`ItemChange::severed`]), the values authored on them are **handed
    /// back** ([`ItemChange::discarded`]), and everything after the item is
    /// pulled down one item's worth of ports rather than one port.
    ///
    /// # Errors
    ///
    /// [`ItemError::NotVariadic`], [`ItemError::NoSuchItem`] for a position at
    /// or past the end, and [`ItemError::AtMinimum`] when the run is already as
    /// short as the kind allows.
    pub fn remove_item(
        &mut self,
        tree: TreeId,
        node: NodeId,
        side: Side,
        at: u32,
    ) -> Result<ItemChange<K>, ItemError> {
        let found = self.find_run(tree, node, side)?;
        let count = found.count();
        if at >= count {
            return Err(found.no_such(at));
        }
        if count <= found.run.min {
            return Err(ItemError::AtMinimum {
                side,
                min: found.run.min,
            });
        }
        let correspondence: Vec<Option<u32>> = (0..count)
            .map(|old| match old.cmp(&at) {
                std::cmp::Ordering::Less => Some(old),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(old - 1),
            })
            .collect();
        let mut items = found.items.clone();
        items.remove(at as usize);
        Ok(self.rewrite(&found, items, &correspondence))
    }

    /// Move the item at `from` to position `to`, carrying everything on it.
    ///
    /// The DCC's `socket_items::make_move_item_operator`, which the engine has
    /// no equivalent of — it offers `InsertExecutionPinBefore`/`After` and
    /// expects the author to rebuild. **Nothing is severed and nothing is
    /// discarded**: this is a permutation of the run, so every link and every
    /// authored value has somewhere to go, and that is the sharpest test of the
    /// correspondence there is.
    ///
    /// # Errors
    ///
    /// [`ItemError::NotVariadic`], and [`ItemError::NoSuchItem`] for either
    /// position at or past the end.
    pub fn move_item(
        &mut self,
        tree: TreeId,
        node: NodeId,
        side: Side,
        from: u32,
        to: u32,
    ) -> Result<ItemChange<K>, ItemError> {
        let found = self.find_run(tree, node, side)?;
        let count = found.count();
        for index in [from, to] {
            if index >= count {
                return Err(found.no_such(index));
            }
        }
        let correspondence: Vec<Option<u32>> = (0..count)
            .map(|old| {
                Some(if old == from {
                    to
                } else if from < to && old > from && old <= to {
                    old - 1
                } else if to < from && old >= to && old < from {
                    old + 1
                } else {
                    old
                })
            })
            .collect();
        let mut items = found.items.clone();
        let carried = items.remove(from as usize);
        items.insert(to as usize, carried);
        Ok(self.rewrite(&found, items, &correspondence))
    }

    /// The run and the resolved item list, or the error that says why there is
    /// neither.
    fn find_run(&self, tree: TreeId, node: NodeId, side: Side) -> Result<Found<K>, ItemError> {
        if self.tree(tree).is_none() {
            return Err(ItemError::NoSuchTree(tree));
        }
        if self.tree(tree).and_then(|t| t.node(node)).is_none() {
            return Err(ItemError::NoSuchNode { tree, node });
        }
        let run = self
            .variadic(tree, node, side)
            .ok_or(ItemError::NotVariadic { tree, node, side })?;
        let items =
            self.items(tree, node, side)
                .ok_or(ItemError::NotVariadic { tree, node, side })?;
        Ok(Found {
            site: Site { tree, node, side },
            run,
            items,
        })
    }

    /// Write a new item list back, moving every port that survived and naming
    /// everything that did not.
    ///
    /// `correspondence` is over **item ordinals**, one slot per old item, and
    /// the port-level map is derived from it — so the three operations state
    /// only what they are (an insert, a removal, a permutation) and the
    /// arithmetic that actually re-indexes the graph is written once.
    fn rewrite(
        &mut self,
        found: &Found<K>,
        items: Vec<Item<K::Type>>,
        correspondence: &[Option<u32>],
    ) -> ItemChange<K> {
        let Site { tree, node, side } = found.site;
        let run = &found.run;
        let before = u32::try_from(correspondence.len()).unwrap_or(u32::MAX);
        let after = u32::try_from(items.len()).unwrap_or(u32::MAX);
        let stride = run.stride();
        let fixed = self.fixed_arity(tree, node, side);
        let mut moved: BTreeMap<PortRef, PortRef> = BTreeMap::new();

        // The fixed ports before the run keep their indices; the ones after it
        // shift by the whole change in the run's length. That second half is
        // the one with no visual cue on the node at all.
        for index in 0..run.at.min(fixed) {
            moved.insert(PortRef { side, index }, PortRef { side, index });
        }
        for index in run.at..fixed {
            moved.insert(
                PortRef {
                    side,
                    index: index + before * stride,
                },
                PortRef {
                    side,
                    index: index + after * stride,
                },
            );
        }
        for (old, new) in correspondence.iter().enumerate() {
            let Some(new) = *new else { continue };
            let old = u32::try_from(old).unwrap_or(u32::MAX);
            for slot in 0..stride {
                moved.insert(
                    PortRef {
                        side,
                        index: run.at + old * stride + slot,
                    },
                    PortRef {
                        side,
                        index: run.at + new * stride + slot,
                    },
                );
            }
        }
        // The other side of the node is untouched, and saying so explicitly is
        // what keeps `remap_ports` a total map: a port absent from it is
        // severed, so an omission there would silently cut every wire on the
        // side that was not edited.
        let other = side.other();
        for index in 0..self.side_arity(tree, node, other) {
            moved.insert(
                PortRef { side: other, index },
                PortRef { side: other, index },
            );
        }

        if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(node)) {
            slot.items.set(side, items);
        }
        let (severed, discarded) = self.remap_ports(tree, node, &moved);
        let mut ordered: Vec<(PortRef, PortRef)> =
            moved.into_iter().filter(|(from, to)| from != to).collect();
        ordered.sort_unstable();
        ItemChange {
            items: after,
            added: Vec::new(),
            moved: ordered,
            severed,
            discarded,
        }
    }

    /// How many ports one side of this node's kind declares **without** its
    /// run — the fixed part, which is what the run is spliced into.
    ///
    /// Zero for anything but an application kind, which is unreachable here:
    /// only [`Self::find_run`] leads to this, and a structural body has no run.
    fn fixed_arity(&self, tree: TreeId, node: NodeId, side: Side) -> u32 {
        let Some(NodeBody::Kind(kind)) =
            self.tree(tree).and_then(|t| t.node(node)).map(|n| &n.body)
        else {
            return 0;
        };
        let base = match side {
            Side::Input => kind.inputs(),
            Side::Output => kind.outputs(),
        };
        u32::try_from(base.len()).unwrap_or(u32::MAX)
    }

    /// How many ports one side of this node's **resolved** signature has.
    fn side_arity(&self, tree: TreeId, node: NodeId, side: Side) -> u32 {
        let Some(signature) = self.signature(tree, node) else {
            return 0;
        };
        let ports = match side {
            Side::Input => signature.inputs.len(),
            Side::Output => signature.outputs.len(),
        };
        u32::try_from(ports).unwrap_or(u32::MAX)
    }

    /// Move a node's links **and** its authored values through one
    /// correspondence, naming what did not survive.
    ///
    /// One function and not two calls, because the two must agree: a link kept
    /// while the value on the same port is dropped is silent corruption, and
    /// the only way to make that unrepresentable is for one map to drive both.
    /// [`Document::set_kind`](crate::Document::set_kind) is the other caller —
    /// a swap re-signatures a node exactly the way an item edit does, and
    /// before this the two spelled the value half separately.
    pub(crate) fn remap_ports(
        &mut self,
        tree: TreeId,
        node: NodeId,
        moved: &BTreeMap<PortRef, PortRef>,
    ) -> (Vec<Link>, Vec<(PortRef, K::Value)>) {
        let severed = self.remap_node_ports(tree, node, moved);
        let mut discarded = Vec::new();
        if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(node)) {
            let was = std::mem::take(&mut slot.values);
            for (port, value) in was {
                match moved.get(&port) {
                    Some(to) => {
                        slot.values.insert(*to, value);
                    }
                    None => discarded.push((port, value)),
                }
            }
        }
        discarded.sort_by_key(|(port, _)| *port);
        (severed, discarded)
    }
}
