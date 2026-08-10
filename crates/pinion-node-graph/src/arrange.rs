//! R1631 — moving a **selection** into order: align, distribute, stack,
//! straighten.
//!
//! [`crate::layout`] arranges a whole tree from its topology. This is the other
//! half a node editor needs and the one an author reaches for far more often:
//! six nodes are nearly in a row and should be exactly in one. The engine
//! reference spells that as **eleven separate editor commands** — six aligns,
//! two distributes, two stacks and a straighten — and this module is those
//! eleven, as a closed vocabulary rather than a list.
//!
//! # Eleven commands, three parameters
//!
//! The reference's names encode their arguments: `AlignNodesLeft`,
//! `AlignNodesTop`, `DistributeNodesHorizontally`. Spelled that way the set
//! cannot be enumerated, cannot be stored, and cannot be offered to a user by a
//! generic control — every consumer writes the eleven-way match again. Here the
//! arguments are values:
//!
//! | this module | the reference's names |
//! |---|---|
//! | `Align { axis: Horizontal, edge: Start / Center / End }` | `AlignNodesLeft` / `AlignNodesCenter` / `AlignNodesRight` |
//! | `Align { axis: Vertical, edge: Start / Center / End }` | `AlignNodesTop` / `AlignNodesMiddle` / `AlignNodesBottom` |
//! | `Distribute { axis }` | `DistributeNodesHorizontally` / `…Vertically` |
//! | `Stack { axis, gap }` | `StackNodesHorizontally` / `…Vertically` |
//! | `Straighten` | `StraightenConnections` |
//!
//! # Past the reference, in three places
//!
//! * **The result is a value, not a mutation.** Every pass here answers a
//!   [`Placement`] — the same type [`Layered`](crate::Layered) and
//!   [`Organic`](crate::Organic) answer — so an arrangement can be previewed,
//!   diffed, refused or undone before anything moves, and the four passes
//!   compose with the two topological ones. The reference's commands write
//!   straight into the graph and their transaction is the only record.
//! * **The stack's padding is a parameter.** The reference compiles a constant
//!   into the editor, so an application whose cards are twice as tall cannot
//!   ask for a proportionate gap.
//! * **Straighten says what it could not straighten.** A selection whose links
//!   branch cannot be made straight — two consumers of one producer want the
//!   same free-axis coordinate and they cannot both have it — and the reference
//!   moves what it can, silently. [`Straightened::bent`] names every link left
//!   over, which is the difference between "this is as straight as it goes" and
//!   "this tool is unreliable".
//!
//! # What an arrangement moves
//!
//! Exactly the nodes named, and only along the axis it is about: an align on
//! the horizontal axis never changes a `y`. That is why the passes take a
//! selection rather than a tree — the reference's commands are selection
//! commands too, and an arrangement that quietly moved a neighbour would undo
//! the author's own work.
//!
//! A named node that is not in the tree, or is a frame, simply does not take
//! part. A frame is a region *around* nodes ([`crate::frame`]), so aligning one
//! against the members it annotates would tear it off them — the same exclusion
//! [`Placeable`](crate::layout) makes for the topological passes.

use std::collections::{BTreeMap, BTreeSet};

use crate::layout::{Extent, Placement};
use crate::model::{Document, LinkId, Node, NodeId, NodeKind, TreeId};

/// Which canvas axis an arrangement works along — the axis the nodes MOVE on.
///
/// An [`Align`] on [`Horizontal`](Self::Horizontal) changes `x` and leaves `y`
/// alone, which is the reference's `AlignNodesLeft` family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum Axis {
    /// Left to right — `x`.
    Horizontal,
    /// Top to bottom — `y`.
    Vertical,
}

impl Axis {
    /// Both axes, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 2] = [Self::Horizontal, Self::Vertical];

    /// R1638 — every [`name`](Self::name), in declaration order, as the closed
    /// vocabulary an argument's `ArgDomain::OneOf`
    /// points at.
    ///
    /// Projected from [`ALL`](Self::ALL) rather than written out, so it cannot
    /// disagree with the names; `ALL` is itself held to the variant count by
    /// `#[variant_census(all)]`, which makes a new variant a build failure here
    /// instead of a vocabulary that is quietly one short on the wire.
    pub const WIRE_NAMES: [&'static str; Self::ARMS] = {
        let mut out = [""; Self::ARMS];
        let mut i = 0;
        while i < Self::ARMS {
            out[i] = Self::ALL[i].name();
            i += 1;
        }
        out
    };

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }

    /// This node's leading coordinate on the axis.
    const fn lead<K: NodeKind>(self, node: &Node<K>) -> i32 {
        match self {
            Self::Horizontal => node.x,
            Self::Vertical => node.y,
        }
    }

    /// How far the node reaches along the axis.
    const fn span(self, extent: Extent) -> i32 {
        match self {
            Self::Horizontal => extent.width,
            Self::Vertical => extent.height,
        }
    }

    /// The position a node takes when this axis' coordinate becomes `moved`.
    const fn placed<K: NodeKind>(self, node: &Node<K>, moved: i32) -> (i32, i32) {
        match self {
            Self::Horizontal => (moved, node.y),
            Self::Vertical => (node.x, moved),
        }
    }
}

/// Which edge of the selection's own bounding box the nodes meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum Edge {
    /// The leading edge — left, or top.
    Start,
    /// The midline.
    Center,
    /// The trailing edge — right, or bottom.
    End,
}

/// R1638 — which arrangement pass to run, as a **value**.
///
/// R1631 made the eleven commands of the reference into an axis, an edge and a
/// gap, and left the fourth parameter — *which pass* — as four string literals
/// matched at the call site. That is the same shape one level up: a vocabulary
/// spelled at every consumer cannot be enumerated, offered in a menu, or
/// published to a client, and each consumer re-writes the four-way match.
///
/// The passes keep their own types ([`Align`], [`Distribute`], [`Stack`],
/// [`Straighten`]) because they take different parameters and answer different
/// reports; this names the CHOICE between them, which is what a caller sends
/// over a wire and what a schema declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum ArrangePass {
    /// Bring the selection to one edge of its own bounding box.
    Align,
    /// Even the gaps between the selection's members.
    Distribute,
    /// Pack them against each other with a fixed gap.
    Stack,
    /// Put linked nodes on one line where the links allow it.
    Straighten,
}

impl ArrangePass {
    /// Every pass, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 4] = [Self::Align, Self::Distribute, Self::Stack, Self::Straighten];

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Align => "align",
            Self::Distribute => "distribute",
            Self::Stack => "stack",
            Self::Straighten => "straighten",
        }
    }

    /// Parse a wire name back to the pass, or `None` — the inverse of
    /// [`name`](Self::name), which `r1638_every_arrange_pass_name_parses_back`
    /// holds it to.
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The closed vocabulary, projected from [`ALL`](Self::ALL) — see
    /// [`Axis::WIRE_NAMES`].
    pub const WIRE_NAMES: [&'static str; Self::ARMS] = {
        let mut out = [""; Self::ARMS];
        let mut i = 0;
        while i < Self::ARMS {
            out[i] = Self::ALL[i].name();
            i += 1;
        }
        out
    };

    /// Whether this pass reads the trailing argument, and what it means there —
    /// the one fact a caller needs that the pass name alone does not carry.
    #[must_use]
    pub const fn tail(self) -> ArrangeTail {
        match self {
            Self::Align => ArrangeTail::Edge,
            Self::Stack => ArrangeTail::Gap,
            Self::Distribute | Self::Straighten => ArrangeTail::None,
        }
    }
}

/// What an [`ArrangePass`] reads from its trailing argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, pinion_derive::VariantCensus)]
pub enum ArrangeTail {
    /// Nothing — the pass is fully determined by its axis.
    None,
    /// An [`Edge`].
    Edge,
    /// An integer gap, in graph units.
    Gap,
}

impl Edge {
    /// Every edge, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 3] = [Self::Start, Self::Center, Self::End];

    /// R1638 — every [`name`](Self::name), in declaration order, as the closed
    /// vocabulary an argument's `ArgDomain::OneOf`
    /// points at.
    ///
    /// Projected from [`ALL`](Self::ALL) rather than written out, so it cannot
    /// disagree with the names; `ALL` is itself held to the variant count by
    /// `#[variant_census(all)]`, which makes a new variant a build failure here
    /// instead of a vocabulary that is quietly one short on the wire.
    pub const WIRE_NAMES: [&'static str; Self::ARMS] = {
        let mut out = [""; Self::ARMS];
        let mut i = 0;
        while i < Self::ARMS {
            out[i] = Self::ALL[i].name();
            i += 1;
        }
        out
    };

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
        }
    }
}

/// One node's contribution to an arrangement: where it is and how far it
/// reaches, on the axis in question.
#[derive(Debug, Clone, Copy)]
struct Placed {
    id: NodeId,
    lead: i32,
    span: i32,
}

impl Placed {
    const fn trail(self) -> i32 {
        self.lead + self.span
    }
}

/// The selection's participating nodes on `axis`, in ascending leading-edge
/// order and then by id.
///
/// Ties break by id so an arrangement is a function of the graph rather than of
/// the order a caller happened to build its selection in — two authors who
/// selected the same six nodes get the same answer.
fn participants<K: NodeKind>(
    document: &Document<K>,
    tree: TreeId,
    selection: &BTreeSet<NodeId>,
    axis: Axis,
    extent: &impl Fn(&Node<K>) -> Extent,
) -> Vec<Placed> {
    let Some(host) = document.tree(tree) else {
        return Vec::new();
    };
    let mut out: Vec<Placed> = host
        .nodes()
        .filter(|node| selection.contains(&node.id) && !node.is_frame())
        .map(|node| Placed {
            id: node.id,
            lead: axis.lead(node),
            span: axis.span(extent(node)),
        })
        .collect();
    out.sort_by_key(|p| (p.lead, p.id.0));
    out
}

/// Build a [`Placement`] that moves each named node's `axis` coordinate.
fn placement_on<K: NodeKind>(
    document: &Document<K>,
    tree: TreeId,
    axis: Axis,
    moves: impl IntoIterator<Item = (NodeId, i32)>,
) -> Placement {
    let Some(host) = document.tree(tree) else {
        return Placement::at(BTreeMap::new());
    };
    let positions = moves
        .into_iter()
        .filter_map(|(id, moved)| host.node(id).map(|node| (id, axis.placed(node, moved))))
        .collect();
    Placement::at(positions)
}

/// R1631 — move a selection so its nodes meet one edge of their own bounding
/// box. The reference's six `AlignNodes*` commands, as two parameters.
///
/// **Idempotent**: the box is the selection's own, so a second align changes
/// nothing. That is the reference's behaviour too and it is worth keeping —
/// an author who presses the button twice has not agreed to drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Align {
    /// The axis the nodes move on.
    pub axis: Axis,
    /// Which edge of the bounding box they meet.
    pub edge: Edge,
}

impl Align {
    /// Align to `edge` along `axis`.
    #[must_use]
    pub const fn to(axis: Axis, edge: Edge) -> Self {
        Self { axis, edge }
    }

    /// Where every named node goes.
    ///
    /// A selection of fewer than two nodes is left alone: one node is already
    /// aligned with itself, and answering with its own position would report a
    /// move that is not one.
    #[must_use]
    pub fn run<K: NodeKind>(
        self,
        document: &Document<K>,
        tree: TreeId,
        selection: &BTreeSet<NodeId>,
        extent: impl Fn(&Node<K>) -> Extent,
    ) -> Placement {
        let placed = participants(document, tree, selection, self.axis, &extent);
        if placed.len() < 2 {
            return Placement::at(BTreeMap::new());
        }
        let lead = placed.iter().map(|p| p.lead).min().unwrap_or(0);
        let trail = placed.iter().map(|p| p.trail()).max().unwrap_or(0);
        let moves = placed.iter().map(|p| {
            let moved = match self.edge {
                Edge::Start => lead,
                // Rounds toward the leading edge on a half-unit box, which is
                // the only choice that keeps `Center` idempotent: a rule that
                // rounded away would walk the selection one unit per press.
                Edge::Center => lead + (trail - lead - p.span).div_euclid(2),
                Edge::End => trail - p.span,
            };
            (p.id, moved)
        });
        placement_on(document, tree, self.axis, moves.collect::<Vec<_>>())
    }
}

/// R1631 — spread a selection so the GAPS between consecutive nodes are equal,
/// keeping the two outermost where they are. The reference's
/// `DistributeNodes*`.
///
/// Equal *gaps*, not equal *pitches*: nodes of different sizes laid on an even
/// pitch leave visibly uneven space between them, and space is what a reader
/// sees. The reference distributes the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Distribute {
    /// The axis the nodes spread along.
    pub axis: Axis,
}

impl Distribute {
    /// Distribute along `axis`.
    #[must_use]
    pub const fn along(axis: Axis) -> Self {
        Self { axis }
    }

    /// Where every named node goes.
    ///
    /// Fewer than three nodes is left alone: with two there is one gap and it
    /// is already the only one it can be.
    ///
    /// The extremes are **pinned**, so the arrangement never grows or shrinks
    /// the selection's footprint — an author who spent effort placing the ends
    /// keeps them.
    #[must_use]
    pub fn run<K: NodeKind>(
        self,
        document: &Document<K>,
        tree: TreeId,
        selection: &BTreeSet<NodeId>,
        extent: impl Fn(&Node<K>) -> Extent,
    ) -> Placement {
        let placed = participants(document, tree, selection, self.axis, &extent);
        if placed.len() < 3 {
            return Placement::at(BTreeMap::new());
        }
        let first = placed[0];
        let last = placed[placed.len() - 1];
        let occupied: i32 = placed.iter().map(|p| p.span).sum();
        let free = last.trail() - first.lead - occupied;
        let gaps = i32::try_from(placed.len() - 1).unwrap_or(i32::MAX);
        // Integer division leaves a remainder of at most `gaps - 1` units; it
        // is spread one unit at a time over the leading gaps rather than
        // dropped on the last one, so no single gap is visibly wider than its
        // neighbours.
        let (each, mut spare) = (free.div_euclid(gaps), free.rem_euclid(gaps));
        let mut cursor = first.lead;
        let mut moves = Vec::with_capacity(placed.len());
        for (i, p) in placed.iter().enumerate() {
            moves.push((p.id, cursor));
            cursor += p.span + each;
            if i < placed.len() - 1 && spare > 0 {
                cursor += 1;
                spare -= 1;
            }
        }
        placement_on(document, tree, self.axis, moves)
    }
}

/// R1631 — pack a selection into a run with a fixed gap, starting where its
/// leading node already is. The reference's `StackNodes*`.
///
/// The gap is a **parameter**. The reference hard-codes its padding, so an
/// application whose cards are twice as tall cannot ask for a proportionate
/// space, and one drawing compact chips cannot ask for less.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    /// The axis the nodes pack along.
    pub axis: Axis,
    /// Units of clear space between consecutive nodes. A negative gap is
    /// clamped to zero — an overlap is not a stack.
    pub gap: i32,
}

impl Stack {
    /// Pack along `axis` with `gap` units between neighbours.
    #[must_use]
    pub const fn along(axis: Axis, gap: i32) -> Self {
        Self {
            axis,
            gap: if gap < 0 { 0 } else { gap },
        }
    }

    /// Where every named node goes.
    ///
    /// Unlike [`Distribute`], this MOVES the trailing node: packing is about
    /// the gaps, and the footprint that results is whatever the sizes make it.
    #[must_use]
    pub fn run<K: NodeKind>(
        self,
        document: &Document<K>,
        tree: TreeId,
        selection: &BTreeSet<NodeId>,
        extent: impl Fn(&Node<K>) -> Extent,
    ) -> Placement {
        let placed = participants(document, tree, selection, self.axis, &extent);
        if placed.len() < 2 {
            return Placement::at(BTreeMap::new());
        }
        let mut cursor = placed[0].lead;
        let mut moves = Vec::with_capacity(placed.len());
        for p in &placed {
            moves.push((p.id, cursor));
            cursor += p.span + self.gap;
        }
        placement_on(document, tree, self.axis, moves)
    }
}

/// R1631 — what [`Straighten`] managed, and what it could not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Straightened {
    placement: Placement,
    straight: Vec<LinkId>,
    bent: Vec<LinkId>,
}

impl Straightened {
    /// Where every moved node goes.
    #[must_use]
    pub const fn placement(&self) -> &Placement {
        &self.placement
    }

    /// The links this arrangement made axis-parallel, in link order.
    #[must_use]
    pub fn straight(&self) -> &[LinkId] {
        &self.straight
    }

    /// The links it could not, in link order — **the answer the reference does
    /// not give**.
    ///
    /// A link is left bent when its consumer has already been claimed by an
    /// earlier producer: two consumers of one output both want that output's
    /// free-axis coordinate and only one can have it, so a selection whose
    /// links branch is straight only along the walk that was taken. Reporting
    /// the leftovers is what separates "this is as straight as it goes" from
    /// "this tool is unreliable" — and it is a fact about the GRAPH, so a
    /// caller can decide to split the selection rather than press again.
    #[must_use]
    pub fn bent(&self) -> &[LinkId] {
        &self.bent
    }

    /// Whether every link inside the selection came out straight.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.bent.is_empty()
    }
}

/// R1631 — move a selection's nodes so the links between them run parallel to
/// the flow axis. The reference's `StraightenConnections`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Straighten {
    /// The axis links should run along. A horizontally-flowing graph
    /// straightens by equalising `y`.
    pub axis: Axis,
}

impl Straighten {
    /// Straighten links that run along `axis`.
    #[must_use]
    pub const fn along(axis: Axis) -> Self {
        Self { axis }
    }

    /// Where every moved node goes, and which links are still bent.
    ///
    /// Only links whose BOTH ends are in the selection take part: a link
    /// reaching outside would drag a node the author did not select, and the
    /// reference makes the same restriction.
    ///
    /// The walk is in link order and each consumer is claimed once, so the
    /// result is a function of the document rather than of a traversal seed.
    #[must_use]
    pub fn run<K: NodeKind>(
        self,
        document: &Document<K>,
        tree: TreeId,
        selection: &BTreeSet<NodeId>,
    ) -> Straightened {
        let empty = || Straightened {
            placement: Placement::at(BTreeMap::new()),
            straight: Vec::new(),
            bent: Vec::new(),
        };
        let Some(host) = document.tree(tree) else {
            return empty();
        };
        // The free axis is the one a straight link does NOT run along.
        let free = match self.axis {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        };
        let mut coordinate: BTreeMap<NodeId, i32> = host
            .nodes()
            .filter(|node| selection.contains(&node.id) && !node.is_frame())
            .map(|node| (node.id, free.lead(node)))
            .collect();
        let mut claimed: BTreeSet<NodeId> = BTreeSet::new();
        let mut straight = Vec::new();
        let mut bent = Vec::new();
        for link in host.links() {
            let (from, to) = (link.from.node, link.to.node);
            if !coordinate.contains_key(&from) || !coordinate.contains_key(&to) {
                continue;
            }
            if from == to || !claimed.insert(to) {
                // A consumer already pinned by an earlier producer. Moving it
                // again would straighten this link and bend the other, so the
                // honest answer is to leave it and say so.
                bent.push(link.id);
                continue;
            }
            let target = coordinate[&from];
            coordinate.insert(to, target);
            straight.push(link.id);
        }
        let moves: Vec<(NodeId, i32)> = coordinate
            .into_iter()
            .filter(|&(id, at)| host.node(id).is_some_and(|node| free.lead(node) != at))
            .collect();
        Straightened {
            placement: placement_on(document, tree, free, moves),
            straight,
            bent,
        }
    }
}
