//! Where a document's nodes go on a canvas (R1597).
//!
//! [`pinion_graph`] holds the algorithms and works on abstract vertices — an
//! index, a free-axis extent, and index pairs for edges. What was missing is the
//! step between a **document** and that: which nodes take part, what they
//! occupy, and where a solved layer index lands in canvas coordinates.
//!
//! That step is not application-specific and it was living in an application.
//! `hello-node-editor` carried it as `graph_index` / `layout_of` /
//! `layered_layout` / `force_directed_layout`, reading exactly four things — the
//! nodes that compute, the links, each node's extent, and a column pitch — of
//! which only the last two belong to an application at all. So any second
//! node-graph application had to copy it, which is the fork R1577 was written to
//! end, one layer in.
//!
//! **The extent is asked for rather than derived**, which is the same split
//! R1589 drew for a frame: a node's size is a fact about how the application
//! draws a card, and this crate deliberately has no cards. A layout is the one
//! derivation that needs *both* dimensions at once — the free axis decides how
//! far apart two nodes in a column sit, the layer axis where the next column
//! begins — so [`Extent`] carries both.

use std::collections::BTreeMap;

use pinion_graph::Sugiyama;

use crate::model::{Document, Node, NodeId, NodeKind, TreeId};

/// What one node occupies on the canvas, in the application's own units — the
/// units [`Node::x`] and [`Node::y`] are already in.
///
/// Both dimensions, because a layout needs both and they come from different
/// places: a card's width may be authored ([`Appearance::width`]) while its
/// height is usually a function of the ports it draws. Asking for the pair means
/// this crate never has to guess either.
///
/// [`Appearance::width`]: crate::Appearance::width
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent {
    /// Along the layer axis — what decides a column's width.
    pub width: i32,
    /// Along the free axis — what a layered pass keeps clear between neighbours.
    pub height: i32,
}

impl Extent {
    /// An extent of `width` by `height`.
    #[must_use]
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

/// How well a **layered** arrangement reads, as the solver measured it.
///
/// Published beside the positions rather than recomputed, so a reported metric
/// can never describe a different arrangement than the one that was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quality {
    /// Edge crossings in the chosen ordering.
    pub crossings: usize,
    /// Segments joining two consecutive layers with a bend at each end — the
    /// ones Brandes-Köpf guarantees it can draw straight.
    pub inner_segments: usize,
    /// How many of those actually came out straight.
    pub straight_inner: usize,
}

/// Where a pass put every node it placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    positions: BTreeMap<NodeId, (i32, i32)>,
    quality: Option<Quality>,
}

impl Placement {
    /// The new top-left of every node that took part, by id.
    ///
    /// A node this pass did not place — a [`NodeBody::Frame`], or a node in
    /// another tree — is simply absent, so a caller that writes these back moves
    /// exactly what was arranged.
    ///
    /// [`NodeBody::Frame`]: crate::NodeBody::Frame
    #[must_use]
    pub const fn positions(&self) -> &BTreeMap<NodeId, (i32, i32)> {
        &self.positions
    }

    /// How well the arrangement reads, when the pass is one that can say.
    ///
    /// `None` for [`Organic`], and that is the honest answer rather than a zero:
    /// crossings and inner segments are properties of a *layering*, and an
    /// organic relaxation does not build one.
    #[must_use]
    pub const fn quality(&self) -> Option<Quality> {
        self.quality
    }
}

/// The nodes a layout arranges, indexed `0..n` in id order, with their extents
/// and their links as index pairs.
///
/// One projection for both passes, so "which nodes take part" is answered once.
struct Placeable {
    ids: Vec<NodeId>,
    extents: Vec<Extent>,
    edges: Vec<(usize, usize)>,
}

impl Placeable {
    /// Project `tree`, or `None` when it has nothing to arrange.
    ///
    /// **Every node that is not a frame takes part**, group instances and
    /// interface nodes included: those compute, so they sit in the flow and a
    /// layout that skipped them would route wires through empty canvas. A frame
    /// is the exception because it is not in the flow at all — it is a region of
    /// canvas around nodes, and moving it by a layer index would tear it off the
    /// members it annotates.
    fn of<K: NodeKind>(
        document: &Document<K>,
        tree: TreeId,
        extent: &impl Fn(&Node<K>) -> Extent,
    ) -> Option<Self> {
        let host = document.tree(tree)?;
        // Ascending by id already — a tree keeps its nodes in a `BTreeMap` — so
        // the index mapping is stable across calls without a sort.
        let taking_part: Vec<&Node<K>> = host.nodes().filter(|n| !n.is_frame()).collect();
        if taking_part.is_empty() {
            return None;
        }
        let ids: Vec<NodeId> = taking_part.iter().map(|n| n.id).collect();
        let extents: Vec<Extent> = taking_part.iter().map(|n| extent(n)).collect();
        let slot: BTreeMap<NodeId, usize> =
            ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        // In link order, and deliberately NOT canonicalised here. The example
        // this was lifted from sorted the pairs "so the cycle break cannot
        // depend on the order links were authored in", and that reason is
        // false: `Sugiyama` sorts its own successor lists and walks roots in
        // index order, so the break is already a function of the graph. A
        // second sort would be a guarantee stated twice, in the weaker place —
        // `r1597_a_cyclic_document_lays_out_the_same_whatever_order_its_links_arrived_in`
        // is what notices if the solver ever stops making it.
        let edges: Vec<(usize, usize)> = host
            .links()
            .iter()
            .filter_map(|link| Some((*slot.get(&link.from.node)?, *slot.get(&link.to.node)?)))
            .collect();
        Some(Self {
            ids,
            extents,
            edges,
        })
    }
}

/// A **layered (Sugiyama)** arrangement: columns left to right so data flows
/// forward, crossing-reduced within each column.
///
/// Configure, then [`run`](Self::run) — the shape [`Sugiyama`] itself has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layered {
    /// The free-axis solver: row clearance, bend extent, and how many
    /// crossing-reduction sweeps to make.
    pub sugiyama: Sugiyama,
    /// Clear space between two columns, along the layer axis.
    pub column_gap: i32,
}

impl Layered {
    /// Arrange `tree`, anchored so the whole arrangement's top-left sits at
    /// `origin`.
    ///
    /// **A column is as wide as its widest node**, which is the reason this
    /// takes a full [`Extent`] rather than the free-axis size [`Sugiyama`] needs.
    /// `hello-node-editor` advanced by a fixed card width, so a column holding
    /// nothing but reroute knots — a quarter the width of a card — still cost a
    /// full card of canvas, and a node authored wider than a card
    /// ([`Appearance::width`]) overhung the next column. Neither is expressible
    /// here: the pitch is derived from what is actually in the column.
    ///
    /// A layer that ended up holding only *bends* has no node to measure, so it
    /// takes [`Sugiyama::bend_size`] — the same extent this solver already gives
    /// a bend on the other axis, for the same reason: a bend stands for a wire,
    /// which needs room but not a card's worth.
    ///
    /// [`Appearance::width`]: crate::Appearance::width
    #[must_use]
    pub fn run<K: NodeKind>(
        &self,
        document: &Document<K>,
        tree: TreeId,
        origin: (i32, i32),
        extent: impl Fn(&Node<K>) -> Extent,
    ) -> Placement {
        let Some(placeable) = Placeable::of(document, tree, &extent) else {
            return Placement {
                positions: BTreeMap::new(),
                quality: None,
            };
        };
        let heights: Vec<i32> = placeable.extents.iter().map(|e| e.height).collect();
        let solved = self.sugiyama.run(&heights, &placeable.edges);
        let column_x = self.column_positions(origin.0, solved.layers(), &placeable.extents);
        // The solver's free-axis coordinates are relative, and it reports the
        // topmost leading edge over the REAL vertices — bends excluded, since a
        // bend is a wire and letting one set the anchor would drift the whole
        // arrangement by half a wire.
        let top = solved.top();
        let (inner_segments, straight_inner) = solved.inner_segments();
        Placement {
            positions: placeable
                .ids
                .iter()
                .enumerate()
                .map(|(i, &id)| {
                    let x = column_x
                        .get(solved.layers()[i])
                        .copied()
                        .unwrap_or(origin.0);
                    let y =
                        origin.1 + (solved.centres()[i] - placeable.extents[i].height / 2) - top;
                    (id, (x, y))
                })
                .collect(),
            quality: Some(Quality {
                crossings: solved.crossings(),
                inner_segments,
                straight_inner,
            }),
        }
    }

    /// The left edge of every column `0..=max layer`, cumulative from `left`.
    fn column_positions(&self, left: i32, layers: &[usize], extents: &[Extent]) -> Vec<i32> {
        let columns = layers.iter().copied().max().map_or(0, |last| last + 1);
        let mut widths = vec![0i32; columns];
        for (at, &layer) in layers.iter().enumerate() {
            if let (Some(slot), Some(node)) = (widths.get_mut(layer), extents.get(at)) {
                *slot = (*slot).max(node.width);
            }
        }
        let mut x = Vec::with_capacity(columns);
        let mut cursor = left;
        for width in widths {
            x.push(cursor);
            // A column no node landed in is a column of bends.
            cursor += width.max(self.sugiyama.bend_size) + self.column_gap;
        }
        x
    }
}

/// A **force-directed (Fruchterman-Reingold spring-electrical)** arrangement:
/// nodes repel, links spring them together, annealed to a compact symmetric
/// cluster.
///
/// The organic counterpart to [`Layered`] — the mode a professional editor
/// offers for a cyclic or undirected topology (yEd organic, Graphviz `neato`,
/// Gephi `ForceAtlas`), where a layered pass has no forward direction to work
/// with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Organic {
    /// A fixed iteration count, so the annealing always terminates and the
    /// arrangement is reproducible.
    pub iterations: usize,
    /// The natural spring rest length a linked pair settles toward, and the
    /// repulsion's length scale.
    pub ideal_length: f64,
}

/// The distance floor guarding the inverse-distance repulsion from a
/// divide-by-zero when two nodes (near-)coincide.
const COINCIDENT: f64 = 1.0;

impl Organic {
    /// Relax `tree`, translated so the settled cloud's bounding-box top-left
    /// sits at `origin`.
    ///
    /// **Deterministic**, like [`Layered::run`]: it reads only which nodes take
    /// part and how they are linked — never their current positions — seeding
    /// from a fixed grid in id order with no trigonometry, so the arithmetic is
    /// `+ - * / sqrt` only and identical inputs give identical output on any
    /// platform. Running it twice therefore lands the graph in the same place.
    ///
    /// Nodes are relaxed as **points**: their extents do not repel, so two large
    /// cards can settle closer than their boxes allow. That is the stated limit
    /// of this pass rather than an oversight — overlap removal is a separate
    /// step every organic layout engine runs afterwards.
    #[must_use]
    pub fn run<K: NodeKind>(
        &self,
        document: &Document<K>,
        tree: TreeId,
        origin: (i32, i32),
    ) -> Placement {
        let Some(placeable) = Placeable::of(document, tree, &|_| Extent::new(0, 0)) else {
            return Placement {
                positions: BTreeMap::new(),
                quality: None,
            };
        };
        let ids = &placeable.ids;
        let mut pos = self.grid_seed(ids);
        // The springs: non-self links, undirected — direction is irrelevant to
        // an organic arrangement.
        let springs: Vec<(NodeId, NodeId)> = placeable
            .edges
            .iter()
            .filter(|&&(from, to)| from != to)
            .filter_map(|&(from, to)| Some((*ids.get(from)?, *ids.get(to)?)))
            .collect();

        let steps = f64::from(u32::try_from(self.iterations).unwrap_or(1)).max(1.0);
        for step in 0..self.iterations {
            // Linearly-cooling temperature: a full ideal length at first,
            // annealing toward zero so late steps only fine-tune.
            let temp =
                self.ideal_length * (1.0 - f64::from(u32::try_from(step).unwrap_or(0)) / steps);
            let mut push: BTreeMap<NodeId, (f64, f64)> =
                ids.iter().map(|&id| (id, (0.0, 0.0))).collect();
            self.repel(ids, &pos, &mut push);
            self.attract(&springs, &pos, &mut push);
            for &id in ids {
                let (dx, dy) = push[&id];
                let mag = dy.hypot(dx);
                let scale = if mag > temp { temp / mag } else { 1.0 };
                if let Some(at) = pos.get_mut(&id) {
                    at.0 += dx * scale;
                    at.1 += dy * scale;
                }
            }
        }

        let min_x = pos.values().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let min_y = pos.values().map(|p| p.1).fold(f64::INFINITY, f64::min);
        Placement {
            positions: ids
                .iter()
                .map(|&id| {
                    let (x, y) = pos[&id];
                    (
                        id,
                        (
                            origin.0 + round_i32(x - min_x),
                            origin.1 + round_i32(y - min_y),
                        ),
                    )
                })
                .collect(),
            quality: None,
        }
    }

    /// The deterministic starting cloud: a near-square grid in id order, so no
    /// pair starts coincident and no random seed is involved.
    fn grid_seed(&self, ids: &[NodeId]) -> BTreeMap<NodeId, (f64, f64)> {
        // `ceil(sqrt(n))` by integer search — no float rounding to disagree
        // about across platforms.
        let mut cols = 1usize;
        while cols * cols < ids.len() {
            cols += 1;
        }
        ids.iter()
            .enumerate()
            .map(|(i, &id)| {
                let col = f64::from(u32::try_from(i % cols).unwrap_or(0));
                let row = f64::from(u32::try_from(i / cols).unwrap_or(0));
                (id, (col * self.ideal_length, row * self.ideal_length))
            })
            .collect()
    }

    /// Electrical repulsion (`k^2/d`) between every unordered pair.
    fn repel(
        &self,
        ids: &[NodeId],
        pos: &BTreeMap<NodeId, (f64, f64)>,
        push: &mut BTreeMap<NodeId, (f64, f64)>,
    ) {
        for (at, &a) in ids.iter().enumerate() {
            for &b in &ids[at + 1..] {
                let (ux, uy, apart) = unit_between(a, pos[&a], b, pos[&b]);
                let force = self.ideal_length * self.ideal_length / apart;
                if let Some(d) = push.get_mut(&a) {
                    d.0 += ux * force;
                    d.1 += uy * force;
                }
                if let Some(d) = push.get_mut(&b) {
                    d.0 -= ux * force;
                    d.1 -= uy * force;
                }
            }
        }
    }

    /// Spring attraction (`d^2/k`) along each link, pulling its ends together.
    fn attract(
        &self,
        springs: &[(NodeId, NodeId)],
        pos: &BTreeMap<NodeId, (f64, f64)>,
        push: &mut BTreeMap<NodeId, (f64, f64)>,
    ) {
        for &(from, to) in springs {
            let (ux, uy, apart) = unit_between(from, pos[&from], to, pos[&to]);
            let force = apart * apart / self.ideal_length;
            if let Some(d) = push.get_mut(&from) {
                d.0 -= ux * force;
                d.1 -= uy * force;
            }
            if let Some(d) = push.get_mut(&to) {
                d.0 += ux * force;
                d.1 += uy * force;
            }
        }
    }
}

/// The unit vector pointing from `b` toward `a`, and how far apart they are
/// (floored at [`COINCIDENT`]).
///
/// If the two (near-)coincide the direction falls back to a deterministic +/-x
/// by id order, so the force never divides by zero and the arrangement stays
/// reproducible.
fn unit_between(a: NodeId, at: (f64, f64), b: NodeId, bt: (f64, f64)) -> (f64, f64, f64) {
    let dx = at.0 - bt.0;
    let dy = at.1 - bt.1;
    let apart = dy.hypot(dx);
    if apart > COINCIDENT {
        (dx / apart, dy / apart, apart)
    } else if a.0 < b.0 {
        (1.0, 0.0, COINCIDENT)
    } else {
        (-1.0, 0.0, COINCIDENT)
    }
}

/// Round to the nearest whole unit, saturating rather than wrapping.
///
/// A relaxed cloud is bounded by its own forces, but a caller's `ideal_length`
/// is not, so the seed grid alone can exceed the canvas. A float-to-integer
/// `as` saturates at the bounds and maps NaN to zero — defined behaviour since
/// Rust 1.45 — which is exactly the clamp wanted here: an enormous
/// `ideal_length` lands a node at the edge of the coordinate space rather than
/// wrapping it to the far side.
#[allow(clippy::cast_possible_truncation)]
fn round_i32(v: f64) -> i32 {
    v.round() as i32
}
