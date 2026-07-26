//! R1441 — the pure layered-graph coordinate solver: a **proper layering** plus
//! the **Brandes–Köpf** horizontal coordinate assignment.
//!
//! # Why this module exists
//!
//! Everything here works on abstract vertices `0..n` and knows nothing about any
//! caller's node type — a caller maps its graph onto these indices and maps the
//! answer back. R1441 wrote this boundary while the code still lived inside the
//! node editor, precisely so that a second consumer would make the crate lift a
//! file move rather than a rewrite; R1442 was that consumer, and it was.
//!
//! # What R1383 left simplified, and what this fixes
//!
//! R1383 landed Sugiyama's first three phases (cycle break, longest-path
//! layering, barycenter ordering) and then placed nodes by *stacking* each
//! column top to bottom. Two documented simplifications came with it:
//!
//! 1. **No long-edge split.** An edge spanning more than one layer was invisible
//!    to the crossing-reduction sweep: the barycenter read only its endpoints, so
//!    a wire passing *through* a column had no slot there and nothing stopped
//!    other nodes being ordered across it.
//! 2. **No coordinate solver.** Stacking gives every column a tidy internal
//!    order but no relationship *between* columns, so an edge almost never runs
//!    straight — the wire router drew the slack as a curve.
//!
//! Both are fixed here, and they are fixed by two DIFFERENT mechanisms, which is
//! worth keeping straight because they are separately observable:
//!
//! * [`Layering::split_long_edges`] makes the layering *proper* — every edge
//!   joins adjacent layers, long edges having been broken over dummy vertices
//!   (bends). This changes the ORDERING, and therefore the crossing count.
//! * [`Layering::brandes_koepf`] assigns the within-layer coordinate. This
//!   changes only POSITIONS, never the order, and therefore cannot change the
//!   crossing count — what it buys is straightness.
//!
//! # What "straight" is actually guaranteed
//!
//! Precisely the paper's claim, which is narrower than "every long edge is
//! straight": every **inner segment** — one whose both endpoints are bends — is
//! drawn on a single coordinate. An edge spanning three or more layers has at
//! least one inner segment and therefore a straight run through the middle; an
//! edge spanning exactly two layers has one bend and so no inner segment at all,
//! and is straightened only when the median alignment happens to choose it over a
//! competing short edge. That is not a shortfall of this implementation — it is
//! what type-1 conflict marking is defined over — and stating it exactly is the
//! difference between a guarantee and a hope.
//!
//! # Brandes–Köpf, transposed
//!
//! "Fast and Simple Horizontal Coordinate Assignment" (Brandes & Köpf, GD 2001)
//! is written for top-to-bottom drawings: layers are rows and the coordinate it
//! solves is x. Both of this crate's consumers lay data flow LEFT TO RIGHT, so
//! layers are columns and the free coordinate is **y**. The algorithm is
//! unchanged — it only ever talks about "layers" and "the coordinate within a
//! layer" — but every mention of *left / right* in the paper reads as *up /
//! down* there. The code below keeps the paper's vocabulary so it can be checked
//! against the paper, and the caller does the axis naming.
//!
//! The four passes, the type-1 conflict marking, and the median balancing are all
//! present. Implementing only one pass would have been a new documented
//! simplification, which is the thing this round exists to remove.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// A layering over vertices `0..size.len()`: which vertices sit in each layer and
/// in what order, how big each one is along the free axis, which are dummies, and
/// the edges between adjacent layers.
///
/// A `Layering` is *proper* once [`split_long_edges`](Self::split_long_edges) has
/// run: every edge then joins consecutive layers. [`brandes_koepf`](Self::brandes_koepf)
/// requires that, and debug-asserts it.
#[derive(Debug, Clone, Default)]
pub struct Layering {
    /// Ordered vertices per layer. Every vertex appears exactly once.
    pub layers: Vec<Vec<usize>>,
    /// Each vertex's extent along the free axis (a node's height here).
    pub size: Vec<i32>,
    /// Whether a vertex is a DUMMY — a bend standing in for a long edge as it
    /// passes through a layer. Bends are the reason the paper's conflict marking
    /// exists: an inner segment (dummy → dummy) is a piece of a long edge, and
    /// keeping those straight is what the whole exercise is for.
    pub dummy: Vec<bool>,
    /// `(upper, lower)` vertex pairs. Indices into this vec are stable and are
    /// what the conflict set marks, so a transformed view can reorient the pairs
    /// without disturbing the marks.
    pub edges: Vec<(usize, usize)>,
}

/// A vertex's layer and position, derived once per algorithm run rather than
/// stored, so a reordering of [`Layering::layers`] cannot leave them stale.
struct Index {
    layer_of: Vec<usize>,
    pos: Vec<usize>,
    /// Ordered `(edge, vertex)` predecessors in the previous layer.
    preds: Vec<Vec<(usize, usize)>>,
    /// Ordered `(edge, vertex)` successors in the next layer.
    succs: Vec<Vec<(usize, usize)>>,
}

/// The output of the paper's vertical alignment: which block each vertex belongs
/// to (`root`) and the cycle that walks a block's members (`align`).
struct Blocks {
    root: Vec<usize>,
    align: Vec<usize>,
}

impl Blocks {
    /// The vertices of the block rooted at `block`, starting with the root.
    fn members(&self, block: usize) -> Vec<usize> {
        let mut out = vec![block];
        let mut next = self.align[block];
        while next != block {
            out.push(next);
            next = self.align[next];
        }
        out
    }
}

impl Layering {
    /// A layering of `size.len()` real vertices, `layer_of[v]` giving each one's
    /// layer, with `edges` as `(from, to)` pairs that may span several layers.
    ///
    /// Vertices start in ascending index order inside each layer — a
    /// deterministic seed for [`reduce_crossings`](Self::reduce_crossings).
    #[must_use]
    pub fn new(size: Vec<i32>, layer_of: &[usize], edges: &[(usize, usize)]) -> Self {
        let n = size.len();
        let depth = layer_of.iter().copied().max().map_or(0, |m| m + 1);
        let mut layers = vec![Vec::new(); depth];
        for v in 0..n {
            layers[layer_of[v]].push(v);
        }
        Self {
            layers,
            dummy: vec![false; n],
            size,
            edges: edges.to_vec(),
        }
    }

    /// The number of vertices, real and dummy.
    #[must_use]
    pub fn len(&self) -> usize {
        self.size.len()
    }

    /// Whether the layering holds no vertices at all — the degenerate input a
    /// caller with an empty graph hands in.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size.is_empty()
    }

    /// Make the layering **proper**: every edge spanning more than one layer is
    /// replaced by a chain through one new dummy vertex per intervening layer.
    ///
    /// `bend_size` is the dummy's extent along the free axis. It should be small
    /// — a bend stands for a wire, not a node — but not zero: two long edges
    /// passing through the same layer still need to be told apart, and a
    /// zero-size vertex would let the compaction stack them on one coordinate.
    ///
    /// This is what gives a long edge a SLOT in every layer it crosses, so the
    /// crossing-reduction sweep can see it.
    ///
    /// Returns, for each edge as it was indexed BEFORE the call, the bends
    /// inserted for it in order from the upper endpoint — empty for an edge that
    /// was already proper. R1442 publishes that instead of a bare count because
    /// a bend is where a wire actually goes: a view that draws its edges as
    /// polylines has to route them through the channel the layout reserved, or
    /// the reservation buys nothing but empty space.
    pub fn split_long_edges(&mut self, bend_size: i32) -> Vec<Vec<usize>> {
        let index = self.index();
        let original = core::mem::take(&mut self.edges);
        let mut chains: Vec<Vec<usize>> = Vec::with_capacity(original.len());
        for (upper, lower) in original {
            let (lu, lv) = (index.layer_of[upper], index.layer_of[lower]);
            // A backward or same-layer edge cannot be made proper by inserting
            // bends; the caller's cycle-breaking phase is what keeps those out,
            // and dropping one here silently would hide a caller bug.
            debug_assert!(lv > lu, "split_long_edges needs a forward layering");
            if lv <= lu + 1 {
                self.edges.push((upper, lower));
                chains.push(Vec::new());
                continue;
            }
            let mut chain = Vec::with_capacity(lv - lu - 1);
            let mut prev = upper;
            for l in (lu + 1)..lv {
                let bend = self.size.len();
                self.size.push(bend_size);
                self.dummy.push(true);
                self.layers[l].push(bend);
                self.edges.push((prev, bend));
                chain.push(bend);
                prev = bend;
            }
            self.edges.push((prev, lower));
            chains.push(chain);
        }
        chains
    }

    /// Reorder each layer to reduce crossings: `sweeps` alternating barycenter
    /// passes, each followed by [`untangle`](Self::untangle), keeping the best
    /// order any of them reached.
    ///
    /// This is the structure of Gansner, Koutsofios, North & Vo's `mincross`
    /// ("A Technique for Drawing Directed Graphs", TSE 1993): the median heuristic
    /// moves a vertex a long way on the strength of an average, the transposition
    /// step then fixes what that average got wrong locally, and — because a
    /// barycenter pass is a heuristic and can leave a layer worse than it found it
    /// — the best order seen is what gets returned rather than the last one.
    ///
    /// Fully deterministic, and a fixed sweep count always terminates
    /// (ZERO-FLAKE).
    pub fn reduce_crossings(&mut self, sweeps: usize) {
        if self.layers.len() < 2 {
            return;
        }
        let mut best = self.layers.clone();
        let mut fewest = self.count_crossings();
        for sweep in 0..sweeps {
            self.barycenter_pass(sweep % 2 == 0);
            self.untangle();
            let crossings = self.count_crossings();
            if crossings < fewest {
                fewest = crossings;
                best.clone_from(&self.layers);
            }
        }
        self.layers = best;
    }

    /// One barycenter pass over every layer, reading the layer before it (`down`)
    /// or after it.
    ///
    /// A vertex's key is the mean position of its neighbours in that adjacent
    /// layer, compared as an exact rational so no float rounding enters; a vertex
    /// with no neighbour there keeps its slot, and ties break by index.
    fn barycenter_pass(&mut self, down: bool) {
        let depth = self.layers.len();
        let order: Vec<usize> = if down {
            (1..depth).collect()
        } else {
            (0..depth - 1).rev().collect()
        };
        let index = self.index();
        let mut pos = index.pos.clone();
        for l in order {
            let mut keyed: Vec<(i64, i64, usize)> = self.layers[l]
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let adjacent = if down {
                        &index.preds[v]
                    } else {
                        &index.succs[v]
                    };
                    let mut sum = 0i64;
                    let mut count = 0i64;
                    for &(_, u) in adjacent {
                        sum += i64::try_from(pos[u]).unwrap_or(0);
                        count += 1;
                    }
                    if count == 0 {
                        (i64::try_from(i).unwrap_or(0), 1, v)
                    } else {
                        (sum, count, v)
                    }
                })
                .collect();
            keyed.sort_by(|a, b| (a.0 * b.1).cmp(&(b.0 * a.1)).then(a.2.cmp(&b.2)));
            self.layers[l] = keyed.iter().map(|t| t.2).collect();
            for (i, &v) in self.layers[l].iter().enumerate() {
                pos[v] = i;
            }
        }
    }

    /// R1443 — the paper's **transpose**: exchange vertices that are ADJACENT in a
    /// layer, and only where the exchange strictly reduces the crossing count.
    /// Returns how many exchanges it made.
    ///
    /// Every move is therefore bought by a crossing it removes, which is what
    /// makes this the operator a *stable* layout can afford. An ordering seeded
    /// from a previous drawing ([`order_by_seed`](Self::order_by_seed)) can never
    /// relieve a tangle that accumulated, because it never chooses an order at
    /// all; running this afterwards relieves exactly the tangles a local exchange
    /// can reach and leaves everything else where the viewer last saw it. Run
    /// after a barycenter pass instead, it is the local repair of
    /// [`reduce_crossings`](Self::reduce_crossings).
    ///
    /// **Termination** is not a sweep budget but an invariant: the total crossing
    /// count is a non-negative integer that every accepted exchange strictly
    /// decreases, so at most that many exchanges can ever be accepted.
    ///
    /// Only the two exchanged vertices' own edges can change relative order — any
    /// other pair of edges keeps both its endpoints' order — so the effect on the
    /// global count is exactly the effect on those, which is what the local
    /// comparison below counts.
    pub fn untangle(&mut self) -> usize {
        let index = self.index();
        let mut pos = index.pos;
        let ends = |lists: &[Vec<(usize, usize)>]| -> Vec<Vec<usize>> {
            lists
                .iter()
                .map(|l| l.iter().map(|&(_, v)| v).collect())
                .collect()
        };
        let (preds, succs) = (ends(&index.preds), ends(&index.succs));
        let mut exchanges = 0;
        loop {
            let mut improved = false;
            for layer in 0..self.layers.len() {
                for i in 0..self.layers[layer].len().saturating_sub(1) {
                    let (v, w) = (self.layers[layer][i], self.layers[layer][i + 1]);
                    let (kept, swapped) = straddling(&preds[v], &preds[w], &pos);
                    let (kept_below, swapped_below) = straddling(&succs[v], &succs[w], &pos);
                    if swapped + swapped_below < kept + kept_below {
                        self.layers[layer].swap(i, i + 1);
                        pos[v] = i + 1;
                        pos[w] = i;
                        exchanges += 1;
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }
        exchanges
    }

    /// The number of edge crossings between adjacent layers.
    ///
    /// A property of the ORDER alone, which is what makes it the right metric for
    /// [`split_long_edges`](Self::split_long_edges) (which changes the order) and
    /// the wrong one for [`brandes_koepf`](Self::brandes_koepf) (which does not).
    /// Two segments between the same pair of layers cross when their endpoints
    /// are in opposite order.
    #[must_use]
    pub fn count_crossings(&self) -> usize {
        let index = self.index();
        let mut total = 0;
        for l in 0..self.layers.len().saturating_sub(1) {
            let between: Vec<(usize, usize)> = self
                .edges
                .iter()
                .filter(|&&(u, _)| index.layer_of[u] == l)
                .map(|&(u, w)| (index.pos[u], index.pos[w]))
                .collect();
            for (i, a) in between.iter().enumerate() {
                for b in &between[i + 1..] {
                    if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                        total += 1;
                    }
                }
            }
        }
        total
    }

    /// **Brandes–Köpf** coordinate assignment: the CENTRE of every vertex along
    /// the free axis, with at least `gap` clear between neighbours in a layer.
    ///
    /// Four alignment/compaction passes — one per combination of layer direction
    /// (down / up) and within-layer direction (the paper's left / right) — then
    /// the balancing step: each candidate is shifted to the smallest-width one
    /// and every vertex takes the average of the two middle candidates. Aligning
    /// on the median neighbour is what pulls a chain of bends onto one
    /// coordinate, which is the same thing as a long edge being straight.
    ///
    /// Centres, not top edges: an edge joins the middles of its endpoints, so
    /// "straight" has to mean equal centres for vertices of unequal size.
    ///
    /// The guarantee is over inner segments — see the module docs. A caller
    /// wanting every long edge dead straight would have to give long edges
    /// priority in the ORDERING phase too, which is a different algorithm.
    #[must_use]
    pub fn brandes_koepf(&self, gap: i32) -> Vec<i32> {
        let n = self.len();
        if n == 0 {
            return Vec::new();
        }
        debug_assert!(self.is_proper(), "brandes_koepf needs a proper layering");
        let marked = self.type1_conflicts();
        let mut candidates: Vec<Vec<i32>> = Vec::with_capacity(4);
        for up in [false, true] {
            for reverse in [false, true] {
                let view = self.oriented(up, reverse);
                let coords = view.align_and_compact(&marked, gap);
                candidates.push(if reverse {
                    // The mirrored pass solved the negated axis; flip it back.
                    coords.iter().map(|c| -c).collect()
                } else {
                    coords
                });
            }
        }
        balance(&candidates, &self.size)
    }

    /// R1441 — how many INNER segments the layering has, and how many of those
    /// `coords` draws on a single coordinate.
    ///
    /// The paper's guarantee, made countable: every inner segment should be
    /// straight, so a caller (or an AI over RPC) can check `straight == total`
    /// rather than take it on trust. A layering with no long edge reports
    /// `(0, 0)`, which is why the count is published beside the ratio — "all
    /// inner segments are straight" is vacuous when there are none.
    #[must_use]
    pub fn inner_segment_straightness(&self, coords: &[i32]) -> (usize, usize) {
        let mut total = 0;
        let mut straight = 0;
        for &(u, w) in &self.edges {
            if self.dummy[u] && self.dummy[w] {
                total += 1;
                if coords.get(u) == coords.get(w) {
                    straight += 1;
                }
            }
        }
        (total, straight)
    }

    /// R1442 — order every layer by the coordinates of a PREVIOUS drawing
    /// instead of by crossing reduction: `seed[v]` is where vertex `v` sat then,
    /// or `None` for one that was not in it.
    ///
    /// Two vertices that both carry a seed therefore end up in the order the
    /// seed records — sorting by a value that already encodes the old order
    /// cannot invert it — which is what keeps a re-laid-out graph recognisable
    /// (the "mental map" of Misue, Eades, Lai & Sugiyama, JVLC 1995).
    ///
    /// An unseeded vertex — a newly added one, and every bend, which is derived
    /// fresh each pass — takes the mean of its keyed neighbours, so it lands
    /// where its own edges point. Keys spread outwards until nothing more can be
    /// resolved, so a bend in the middle of a long edge is placed between its
    /// endpoints rather than parked at the end of a column. What is left
    /// unreachable (an added component connected to nothing remembered) sorts
    /// last, by index.
    ///
    /// This does not reduce crossings and is not trying to: see
    /// [`count_crossings`](Self::count_crossings) for what the stability costs.
    pub fn order_by_seed(&mut self, seed: &[Option<i32>]) {
        let keys = self.seed_keys(seed);
        for layer in &mut self.layers {
            layer.sort_by(|&a, &b| keys[a].total_cmp(&keys[b]).then(a.cmp(&b)));
        }
    }

    /// R1442 — how many remembered pairs the current order reversed: pairs that
    /// both carry a seed and share a layer, drawn in the opposite order to the
    /// one the seed records.
    ///
    /// Zero after [`order_by_seed`](Self::order_by_seed), by construction. Run
    /// against a barycenter-ordered layering it counts what a fresh layout would
    /// have cost the viewer, which is what makes the comparison an observation
    /// rather than a claim. Pairs that shared a coordinate are not counted —
    /// there was no remembered order between them to break.
    #[must_use]
    pub fn seed_order_changes(&self, seed: &[Option<i32>]) -> usize {
        let key = |v: usize| seed.get(v).copied().flatten();
        let mut changed = 0;
        for layer in &self.layers {
            for (i, &v) in layer.iter().enumerate() {
                for &w in &layer[i + 1..] {
                    if let (Some(before), Some(after)) = (key(v), key(w))
                        && before > after
                    {
                        changed += 1;
                    }
                }
            }
        }
        changed
    }

    /// The sort key of every vertex for [`order_by_seed`](Self::order_by_seed):
    /// its own seed where it has one, otherwise the mean of the keys reachable
    /// from it, spread one edge per round until the set stops growing.
    ///
    /// Each round reads the PREVIOUS round's keys, so the answer does not depend
    /// on the order vertices are visited in.
    fn seed_keys(&self, seed: &[Option<i32>]) -> Vec<f64> {
        let count = self.len();
        let index = self.index();
        let mut key: Vec<Option<f64>> = (0..count)
            .map(|v| seed.get(v).copied().flatten().map(f64::from))
            .collect();
        loop {
            let mut next = key.clone();
            let mut spread = false;
            for (vertex, slot) in next.iter_mut().enumerate() {
                if slot.is_some() {
                    continue;
                }
                let mut sum = 0.0;
                let mut count = 0.0;
                for &(_, other) in index.preds[vertex].iter().chain(&index.succs[vertex]) {
                    if let Some(k) = key[other] {
                        sum += k;
                        count += 1.0;
                    }
                }
                if count > 0.0 {
                    *slot = Some(sum / count);
                    spread = true;
                }
            }
            key = next;
            if !spread {
                break;
            }
        }
        key.iter().map(|k| k.unwrap_or(f64::INFINITY)).collect()
    }

    /// Whether every edge joins consecutive layers.
    fn is_proper(&self) -> bool {
        let index = self.index();
        self.edges
            .iter()
            .all(|&(u, w)| index.layer_of[w] == index.layer_of[u] + 1)
    }

    /// Derive layer / position / ordered adjacency from the current order.
    fn index(&self) -> Index {
        let n = self.len();
        let mut layer_of = vec![0usize; n];
        let mut pos = vec![0usize; n];
        for (l, layer) in self.layers.iter().enumerate() {
            for (i, &v) in layer.iter().enumerate() {
                layer_of[v] = l;
                pos[v] = i;
            }
        }
        let mut preds: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
        let mut succs: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
        for (ei, &(u, w)) in self.edges.iter().enumerate() {
            succs[u].push((ei, w));
            preds[w].push((ei, u));
        }
        // Ordered by the neighbour's position, which is what the median
        // alignment and the conflict scan both assume.
        for list in preds.iter_mut().chain(succs.iter_mut()) {
            list.sort_by_key(|&(_, v)| pos[v]);
        }
        Index {
            layer_of,
            pos,
            preds,
            succs,
        }
    }

    /// The same layering seen with the layer order and/or each layer's own order
    /// reversed, edge indices preserved.
    ///
    /// This is how one alignment implementation serves all four passes: rather
    /// than four sign-and-index variants of the same delicate loop, the input is
    /// transformed and the output un-transformed by the caller. Edge indices stay
    /// put so the conflict marks — computed once, on the original — still apply.
    fn oriented(&self, up: bool, reverse: bool) -> Self {
        let mut out = self.clone();
        if up {
            out.layers.reverse();
            for e in &mut out.edges {
                *e = (e.1, e.0);
            }
        }
        if reverse {
            for layer in &mut out.layers {
                layer.reverse();
            }
        }
        out
    }

    /// The paper's type-1 conflicts: an edge with a real endpoint that crosses an
    /// **inner segment** (one whose both endpoints are dummies, i.e. the middle
    /// of a long edge). Marked edges are forbidden from aligning, which is what
    /// gives long edges priority over short ones and keeps them straight.
    ///
    /// Scanned over pairs `(i, i + 1)` from `i = 1`: layer 0 holds no dummy (a
    /// bend lives strictly between its edge's endpoints), so the first pair can
    /// contain no inner segment and needs no marks.
    fn type1_conflicts(&self) -> BTreeSet<usize> {
        let index = self.index();
        let mut marked = BTreeSet::new();
        let depth = self.layers.len();
        for i in 1..depth.saturating_sub(1) {
            let upper_len = self.layers[i].len();
            let lower = &self.layers[i + 1];
            let mut k0 = 0usize;
            let mut cursor = 0usize;
            for (l1, &v) in lower.iter().enumerate() {
                // `v` starts an inner segment when it is a bend whose predecessor
                // is also a bend.
                let inner_pred = if self.dummy[v] {
                    index.preds[v]
                        .iter()
                        .find(|&&(_, u)| self.dummy[u])
                        .map(|&(_, u)| u)
                } else {
                    None
                };
                if inner_pred.is_some() || l1 + 1 == lower.len() {
                    let k1 =
                        inner_pred.map_or_else(|| upper_len.saturating_sub(1), |u| index.pos[u]);
                    while cursor <= l1 {
                        let w = lower[cursor];
                        for &(ei, u) in &index.preds[w] {
                            let k = index.pos[u];
                            if k < k0 || k > k1 {
                                marked.insert(ei);
                            }
                        }
                        cursor += 1;
                    }
                    k0 = k1;
                }
            }
        }
        marked
    }

    /// One pass: align vertices into blocks on their median neighbour in the
    /// previous layer, then compact the blocks apart by `gap`.
    ///
    /// Runs in the canonical direction (layers ascending, positions ascending) —
    /// [`oriented`](Self::oriented) is what turns it into the other three. Split
    /// into the paper's own two halves, which is also how it reads there.
    fn align_and_compact(&self, marked: &BTreeSet<usize>, gap: i32) -> Vec<i32> {
        let index = self.index();
        let blocks = self.vertical_alignment(&index, marked);
        self.horizontal_compaction(&index, &blocks, gap)
    }

    /// The paper's *vertical alignment*:every vertex joins the block of its median
    /// neighbour in the previous layer, unless that segment is a marked conflict
    /// or would cross an alignment already made in this layer.
    ///
    /// `root[v]` names v's block; `align[v]` is the next vertex around the
    /// block's cycle, so walking `align` from a root visits the whole block.
    fn vertical_alignment(&self, index: &Index, marked: &BTreeSet<usize>) -> Blocks {
        let count = self.len();
        let mut root: Vec<usize> = (0..count).collect();
        let mut align: Vec<usize> = (0..count).collect();
        for layer in 1..self.layers.len() {
            // `bound` keeps the alignment non-crossing: within one layer the
            // chosen neighbours must be in increasing position.
            let mut bound: Option<usize> = None;
            for &vertex in &self.layers[layer] {
                let neighbours = &index.preds[vertex];
                if neighbours.is_empty() {
                    continue;
                }
                let arity = neighbours.len();
                for median in [(arity - 1) / 2, arity / 2] {
                    if align[vertex] != vertex {
                        break;
                    }
                    let (edge, upper) = neighbours[median];
                    let at = index.pos[upper];
                    if !marked.contains(&edge) && bound.is_none_or(|b| at > b) {
                        align[upper] = vertex;
                        root[vertex] = root[upper];
                        align[vertex] = root[vertex];
                        bound = Some(at);
                    }
                }
            }
        }
        Blocks { root, align }
    }

    /// The paper's *horizontal compaction*: a block's coordinate is a longest path
    /// over the blocks before it in each layer, and the `sink` / `shift` machinery
    /// then pulls whole *classes* of blocks together.
    ///
    /// Iterative rather than the paper's recursion: blocks are placed in
    /// topological order of "depends on the block before me", which is acyclic
    /// because that dependency always steps to a strictly smaller position in some
    /// layer. Recursion would have been shorter and would also have put a
    /// graph-sized frame count on the stack.
    fn horizontal_compaction(&self, index: &Index, blocks: &Blocks, gap: i32) -> Vec<i32> {
        let count = self.len();
        let root = &blocks.root;
        let roots: Vec<usize> = (0..count).filter(|&v| root[v] == v).collect();

        // "The block before me, in any layer my block passes through."
        let mut needs: BTreeMap<usize, BTreeSet<usize>> =
            roots.iter().map(|&b| (b, BTreeSet::new())).collect();
        let mut needed_by: BTreeMap<usize, BTreeSet<usize>> =
            roots.iter().map(|&b| (b, BTreeSet::new())).collect();
        for &block in &roots {
            for member in blocks.members(block) {
                if let Some(before) = self.before(index, member) {
                    let other = root[before];
                    if other != block {
                        needs.get_mut(&block).expect("root").insert(other);
                        needed_by.get_mut(&other).expect("root").insert(block);
                    }
                }
            }
        }
        let mut ready: BTreeSet<usize> = roots
            .iter()
            .copied()
            .filter(|b| needs[b].is_empty())
            .collect();
        let mut outstanding: BTreeMap<usize, usize> =
            roots.iter().map(|&b| (b, needs[&b].len())).collect();
        let mut order: Vec<usize> = Vec::with_capacity(roots.len());
        while let Some(&block) = ready.iter().next() {
            ready.remove(&block);
            order.push(block);
            for &dependent in &needed_by[&block] {
                let left = outstanding.get_mut(&dependent).expect("root");
                *left -= 1;
                if *left == 0 {
                    ready.insert(dependent);
                }
            }
        }
        debug_assert_eq!(order.len(), roots.len(), "block dependencies are acyclic");

        let mut coord: Vec<i32> = vec![0; count];
        let mut sink: Vec<usize> = (0..count).collect();
        let mut shift: Vec<Option<i32>> = vec![None; count];
        for &block in &order {
            for member in blocks.members(block) {
                let Some(before) = self.before(index, member) else {
                    continue;
                };
                let other = root[before];
                let apart = i32::midpoint(self.size[before], self.size[member]) + gap;
                if sink[block] == block {
                    sink[block] = sink[other];
                }
                if sink[block] == sink[other] {
                    coord[block] = coord[block].max(coord[other] + apart);
                } else {
                    let candidate = coord[block] - coord[other] - apart;
                    let slot = &mut shift[sink[other]];
                    *slot = Some(slot.map_or(candidate, |s: i32| s.min(candidate)));
                }
            }
        }
        (0..count)
            .map(|v| {
                let block = root[v];
                coord[block] + shift[sink[block]].unwrap_or(0)
            })
            .collect()
    }

    /// The vertex immediately before `vertex` in its own layer, if any.
    fn before(&self, index: &Index, vertex: usize) -> Option<usize> {
        let at = index.pos[vertex];
        (at > 0).then(|| self.layers[index.layer_of[vertex]][at - 1])
    }
}

/// R1443 — how many of `left`'s edges cross one of `right`'s, as `(kept,
/// swapped)`: the count if the two vertices stay in the order they are in, and
/// the count if they are exchanged.
///
/// `left` and `right` are the neighbours, on ONE side, of two vertices adjacent
/// in a layer. A pair of their edges crosses when the neighbours sit in the
/// opposite order to the vertices, so exchanging the vertices turns every
/// crossing pair into a clear one and every clear pair into a crossing — except
/// the pairs that share a neighbour, which cross either way round and are
/// counted in neither.
fn straddling(left: &[usize], right: &[usize], pos: &[usize]) -> (usize, usize) {
    let mut kept = 0;
    let mut swapped = 0;
    for &a in left {
        for &b in right {
            match pos[a].cmp(&pos[b]) {
                Ordering::Greater => kept += 1,
                Ordering::Less => swapped += 1,
                Ordering::Equal => {}
            }
        }
    }
    (kept, swapped)
}

/// Combine the four candidate assignments into one: shift each to the
/// smallest-width candidate, then take the average of the two middle values per
/// vertex (the paper's balancing step).
///
/// Averaging the middle two rather than all four is what keeps the result close
/// to at least two passes' opinion instead of splitting the difference with an
/// outlier — the reason BK produces symmetric drawings rather than mushy ones.
fn balance(candidates: &[Vec<i32>], size: &[i32]) -> Vec<i32> {
    let n = size.len();
    let extents: Vec<(i32, i32)> = candidates
        .iter()
        .map(|c| {
            let lo = (0..n).map(|v| c[v] - size[v] / 2).min().unwrap_or(0);
            let hi = (0..n).map(|v| c[v] + size[v] / 2).max().unwrap_or(0);
            (lo, hi)
        })
        .collect();
    let narrowest = extents
        .iter()
        .enumerate()
        .min_by_key(|(i, (lo, hi))| (hi - lo, *i))
        .map_or(0, |(i, _)| i);
    let (target_lo, target_hi) = extents[narrowest];
    // Candidates 0 and 2 ran in the paper's leftward direction and align on the
    // minimum; 1 and 3 are the mirrored pair and align on the maximum.
    let shifts: Vec<i32> = extents
        .iter()
        .enumerate()
        .map(|(i, &(lo, hi))| {
            if i % 2 == 0 {
                target_lo - lo
            } else {
                target_hi - hi
            }
        })
        .collect();
    (0..n)
        .map(|v| {
            let mut values: Vec<i32> = candidates
                .iter()
                .zip(&shifts)
                .map(|(c, s)| c[v] + s)
                .collect();
            values.sort_unstable();
            match values.len() {
                0 => 0,
                len => floor_midpoint(values[(len - 1) / 2], values[len / 2]),
            }
        })
        .collect()
}

/// R1443 — the mean of two coordinates, rounded DOWN. `i32::midpoint` rounds
/// towards zero instead, which is a different direction on each side of the
/// origin, and that is a defect here rather than a preference.
///
/// The compaction leaves consecutive vertices in a layer at least `gap` apart in
/// every candidate, and the exact mean of two candidates inherits that (the k-th
/// smallest of a pointwise-larger set is pointwise larger, so the sort in
/// [`balance`] cannot lose the relation). Rounding preserves it only while the
/// direction is the same for every vertex: round one vertex's half-unit up and
/// its neighbour's down and the pair ends up one unit closer than the compaction
/// placed them — an overlap the four passes had each individually avoided.
fn floor_midpoint(lo: i32, hi: i32) -> i32 {
    let mean = (i64::from(lo) + i64::from(hi)).div_euclid(2);
    i32::try_from(mean).expect("the mean of two i32 coordinates fits in an i32")
}

#[cfg(test)]
mod tests {
    use super::Layering;

    /// A chain `0 -> 1 -> 2` plus a long edge `0 -> 2`: the shape whose long edge
    /// the whole round is about. Sizes differ so "straight" cannot mean "same
    /// top edge".
    fn long_edge_graph() -> Layering {
        Layering::new(vec![40, 20, 60], &[0, 1, 2], &[(0, 1), (1, 2), (0, 2)])
    }

    /// Total bends over every chain — the number `split_long_edges` returned
    /// before R1442 made it report WHICH edge each bend belongs to.
    fn bend_count(chains: &[Vec<usize>]) -> usize {
        chains.iter().map(Vec::len).sum()
    }

    #[test]
    fn r1441_splitting_inserts_one_bend_per_intervening_layer() {
        let mut l = long_edge_graph();
        assert_eq!(l.len(), 3);
        let chains = l.split_long_edges(8);
        assert_eq!(bend_count(&chains), 1, "the 0->2 edge crosses one layer");
        // ★ R1442 — and the chain names the edge it belongs to: the third edge
        // authored is the long one, and the other two got no bend.
        assert_eq!(chains, vec![vec![], vec![], vec![3]]);
        assert_eq!(l.len(), 4, "and the bend is a new vertex");
        assert!(l.dummy[3], "the new vertex is a dummy");
        assert!(
            !l.dummy[0] && !l.dummy[1] && !l.dummy[2],
            "the reals are not"
        );
        assert_eq!(l.layers[1].len(), 2, "the bend shares layer 1 with node 1");
        // Every edge now joins adjacent layers, which is what "proper" means.
        let mut spans: Vec<usize> = Vec::new();
        let layer_of = |v: usize| l.layers.iter().position(|lyr| lyr.contains(&v)).unwrap();
        for &(u, w) in &l.edges {
            spans.push(layer_of(w) - layer_of(u));
        }
        assert!(spans.iter().all(|&s| s == 1), "proper layering: {spans:?}");
    }

    /// Splitting is a no-op on a layering that is already proper — it must not
    /// invent bends for adjacent-layer edges.
    #[test]
    fn r1441_splitting_a_proper_layering_adds_nothing() {
        let mut l = Layering::new(vec![10, 10], &[0, 1], &[(0, 1)]);
        assert_eq!(bend_count(&l.split_long_edges(8)), 0);
        assert_eq!(l.len(), 2);
        assert_eq!(l.edges, vec![(0, 1)]);
    }

    /// ★ Every INNER segment is drawn on one coordinate — the paper's actual
    /// guarantee, and what makes a long edge's middle run straight.
    ///
    /// The fixture spans FOUR layers, so the long edge gets two bends and thus one
    /// inner segment between them. A two-layer span (one bend) has no inner
    /// segment and so no guarantee — asserting straightness there would be
    /// asserting a coincidence, which is what a first draft of this test did.
    #[test]
    fn r1441_every_inner_segment_is_drawn_straight() {
        // 0 -> 1 -> 2 -> 3 with a long 0 -> 3 alongside.
        let mut l = Layering::new(
            vec![40, 20, 60, 30],
            &[0, 1, 2, 3],
            &[(0, 1), (1, 2), (2, 3), (0, 3)],
        );
        let chains = l.split_long_edges(8);
        assert_eq!(bend_count(&chains), 2, "the long edge crosses two layers");
        l.reduce_crossings(4);
        let y = l.brandes_koepf(24);

        let index_layer = |v: usize| l.layers.iter().position(|lyr| lyr.contains(&v)).unwrap();
        let mut inner = 0;
        for &(u, w) in &l.edges {
            if l.dummy[u] && l.dummy[w] {
                assert_eq!(
                    y[u], y[w],
                    "★ inner segment {u}->{w} must be straight: {y:?}"
                );
                inner += 1;
            }
        }
        assert_eq!(inner, 1, "the fixture has exactly one inner segment");
        // And the bends really do sit in the layers between the endpoints.
        let bends: Vec<usize> = (0..l.len()).filter(|&v| l.dummy[v]).collect();
        assert_eq!(bends.len(), 2);
        for &b in &bends {
            let lb = index_layer(b);
            assert!(
                lb > index_layer(0) && lb < index_layer(3),
                "bend {b} is interior"
            );
        }
    }

    /// Separation holds in the layer the long edge passes through: the bend and
    /// the real node sharing that layer do not land on the same coordinate.
    #[test]
    fn r1441_a_bend_and_a_node_sharing_a_layer_stay_apart() {
        let mut l = long_edge_graph();
        l.split_long_edges(8);
        l.reduce_crossings(4);
        let y = l.brandes_koepf(24);
        let bend = (0..l.len()).find(|&v| l.dummy[v]).expect("a bend");
        let need = i32::midpoint(l.size[1], l.size[bend]) + 24;
        let apart = (y[1] - y[bend]).abs();
        assert!(
            apart >= need,
            "layer 1 keeps its separation: {apart} < {need} ({y:?})"
        );
    }

    /// The coordinate solver never reorders anything, so it cannot change the
    /// crossing count — the property that keeps the two mechanisms separable.
    #[test]
    fn r1441_the_solver_changes_positions_not_order() {
        let mut l = long_edge_graph();
        l.split_long_edges(8);
        l.reduce_crossings(4);
        let before = l.layers.clone();
        let crossings = l.count_crossings();
        let _ = l.brandes_koepf(24);
        assert_eq!(l.layers, before, "order untouched");
        assert_eq!(l.count_crossings(), crossings, "so crossings cannot move");
    }

    /// Deterministic and idempotent: identical input, identical output, twice.
    #[test]
    fn r1441_the_solver_is_deterministic() {
        let run = || {
            let mut l = long_edge_graph();
            l.split_long_edges(8);
            l.reduce_crossings(4);
            (l.layers.clone(), l.brandes_koepf(24))
        };
        assert_eq!(run(), run());
    }

    /// Within every layer, consecutive vertices keep their separation — the
    /// compaction's whole job, and the thing a naive median alignment breaks.
    #[test]
    fn r1441_no_two_vertices_in_a_layer_overlap() {
        // A wide fan: one source feeding four sinks, plus a long edge past them.
        let mut l = Layering::new(
            vec![30, 30, 30, 30, 30, 30],
            &[0, 1, 1, 1, 1, 2],
            &[(0, 1), (0, 2), (0, 3), (0, 4), (1, 5), (0, 5)],
        );
        l.split_long_edges(8);
        l.reduce_crossings(4);
        let y = l.brandes_koepf(24);
        for layer in &l.layers {
            for pair in layer.windows(2) {
                let (v, w) = (pair[0], pair[1]);
                let need = i32::midpoint(l.size[v], l.size[w]) + 24;
                assert!(
                    y[w] - y[v] >= need,
                    "layer separation broken between {v} and {w}: {} < {need}",
                    y[w] - y[v]
                );
            }
        }
    }

    /// ★ The dummy split is what improves CROSSINGS, and it is measurable: a
    /// long edge that the barycenter could not see gets a slot and stops being
    /// ordered across.
    #[test]
    fn r1441_splitting_lets_the_sweep_see_a_long_edge() {
        // Two long edges that must not cross, plus short edges that would order
        // the middle layer against them if the long ones were invisible.
        let build = |split: bool| {
            let mut l = Layering::new(
                vec![20; 7],
                &[0, 0, 1, 1, 1, 2, 2],
                &[(0, 5), (1, 6), (0, 2), (1, 4), (3, 5)],
            );
            if split {
                l.split_long_edges(8);
            }
            l.reduce_crossings(4);
            l.count_crossings()
        };
        let unsplit = build(false);
        let split = build(true);
        assert!(
            split <= unsplit,
            "splitting must not make crossings worse: {split} > {unsplit}"
        );
    }

    /// ★ Type-1 conflict marking EARNS ITS KEEP: an order where a short edge
    /// would otherwise be aligned first and block the inner segment.
    ///
    /// The order is set by hand rather than by `reduce_crossings`, because the
    /// point is a specific adversarial arrangement: in layer 1 the bend comes
    /// BEFORE the short edge's tail, and in layer 2 the short edge's head comes
    /// before the bend. Aligning greedily in that order takes the short edge
    /// first, which pushes the alignment bound past the bend and leaves the inner
    /// segment crooked. Marking the crossing segment is exactly what prevents it.
    ///
    /// Without this test the conflict marking was decorative: inverting its guard
    /// left every other test and the RPC demo green.
    #[test]
    fn r1441_conflict_marking_protects_an_inner_segment() {
        // 0 -> 3 spans four layers (two bends); 1 -> 2 is a short edge between
        // the layers those bends occupy.
        let mut l = Layering::new(vec![20, 20, 20, 20], &[0, 1, 2, 3], &[(0, 3), (1, 2)]);
        assert_eq!(
            bend_count(&l.split_long_edges(8)),
            2,
            "bends at layers 1 and 2"
        );
        let bends: Vec<usize> = (0..l.len()).filter(|&v| l.dummy[v]).collect();
        let (b1, b2) = (bends[0], bends[1]);

        // The adversarial order: bend first in layer 1, short-edge head first in
        // layer 2, so the two segments cross.
        l.layers[1] = vec![b1, 1];
        l.layers[2] = vec![2, b2];

        let (total, straight) = {
            let y = l.brandes_koepf(24);
            l.inner_segment_straightness(&y)
        };
        assert_eq!(total, 1, "the fixture has exactly one inner segment");
        assert_eq!(
            straight, 1,
            "★ the inner segment survives a crossing short edge"
        );
    }

    /// ★ The four-pass BALANCE earns its keep: on a symmetric fan the shared
    /// parent sits at the MIDPOINT of its two children.
    ///
    /// A single left-biased pass cannot do this — it aligns the parent with
    /// whichever child the median rule reaches first, parking it off to one side.
    /// Averaging the two middle candidates of the four passes is what recovers the
    /// symmetry the drawing obviously has. Without this test the balancing was
    /// unverified: replacing it with one candidate left everything else green.
    #[test]
    fn r1441_balancing_centres_a_symmetric_fan() {
        let mut l = Layering::new(vec![30, 30, 30], &[0, 1, 1], &[(0, 1), (0, 2)]);
        assert_eq!(bend_count(&l.split_long_edges(8)), 0, "nothing long here");
        l.reduce_crossings(4);
        let y = l.brandes_koepf(24);
        let mid = i32::midpoint(y[1], y[2]);
        assert_eq!(
            y[0], mid,
            "★ the parent centres between its children: {y:?} (mid {mid})"
        );
        assert_ne!(y[1], y[2], "and the children are genuinely apart");
    }

    /// An empty or single-vertex layering is total, not a panic.
    #[test]
    fn r1441_degenerate_layerings_are_total() {
        let mut empty = Layering::default();
        assert!(empty.split_long_edges(8).is_empty());
        empty.reduce_crossings(4);
        assert!(empty.brandes_koepf(24).is_empty());
        assert_eq!(empty.count_crossings(), 0);
        assert_eq!(empty.untangle(), 0);

        let mut one = Layering::new(vec![10], &[0], &[]);
        assert!(one.split_long_edges(8).is_empty());
        one.reduce_crossings(4);
        assert_eq!(one.untangle(), 0, "one vertex has nobody to exchange with");
        assert_eq!(one.brandes_koepf(24).len(), 1);
    }

    /// A three-layer graph with `widths` vertices per layer and `edges` between
    /// consecutive ones — the shape a barycenter heuristic is actually judged on,
    /// and too tangled to reason about by eye.
    ///
    /// The two instances below were found by sweeping randomly-generated graphs
    /// of this shape for the ones that separate an implementation choice from its
    /// alternative, then frozen. A hand-written fixture kept failing to separate
    /// them: on a graph small enough to check by hand, the alternating sweeps
    /// repair each other and every variant agrees.
    fn tangle(widths: [usize; 3], edges: &[(usize, usize)]) -> Layering {
        let mut layer_of = Vec::new();
        for (layer, &width) in widths.iter().enumerate() {
            layer_of.extend(std::iter::repeat_n(layer, width));
        }
        Layering::new(vec![30; layer_of.len()], &layer_of, edges)
    }

    /// A graph the barycenter passes leave three times more tangled than the
    /// exchange step can, and the reason [`Layering::untangle`] runs inside
    /// [`Layering::reduce_crossings`] rather than only after a seeded order.
    fn barycenter_stalls() -> Layering {
        tangle(
            [4, 5, 6],
            &[
                (0, 5),
                (0, 6),
                (0, 8),
                (1, 5),
                (1, 7),
                (2, 7),
                (3, 4),
                (3, 8),
                (4, 10),
                (4, 13),
                (4, 14),
                (5, 10),
                (5, 14),
                (6, 13),
                (8, 9),
                (8, 11),
                (8, 12),
            ],
        )
    }

    /// A graph whose LAST sweep is worse than an earlier one — the case that
    /// makes [`Layering::reduce_crossings`] keep the best order it saw rather
    /// than whichever the budget happened to stop on.
    fn a_sweep_overshoots() -> Layering {
        tangle(
            [7, 7, 5],
            &[
                (0, 10),
                (1, 13),
                (2, 7),
                (3, 11),
                (3, 13),
                (4, 7),
                (4, 9),
                (4, 12),
                (5, 9),
                (5, 11),
                (6, 7),
                (7, 16),
                (8, 17),
                (9, 14),
                (9, 15),
                (10, 14),
                (10, 15),
                (12, 14),
                (13, 18),
            ],
        )
    }

    /// ★ R1443 — the exchange step earns its place in [`Layering::reduce_crossings`]:
    /// on a graph the barycenter passes get wrong, the same passes followed by an
    /// exchange come out strictly tidier.
    ///
    /// Without this the transposition would be inert inside a fresh layout, and
    /// its only exercised caller would be the settled pass.
    #[test]
    fn r1443_an_exchange_beats_an_average_that_misleads() {
        let mut swept = barycenter_stalls();
        for sweep in 0..4 {
            swept.barycenter_pass(sweep % 2 == 0);
        }
        let mut mincross = barycenter_stalls();
        mincross.reduce_crossings(4);
        assert!(
            mincross.count_crossings() < swept.count_crossings(),
            "★ barycenter alone left {} crossings, barycenter + exchange {}",
            swept.count_crossings(),
            mincross.count_crossings()
        );
    }

    /// An exchange is only ever accepted when it strictly reduces the count, so
    /// running the step again finds nothing: it stops at a local optimum rather
    /// than oscillating between two equally good orders.
    #[test]
    fn r1443_the_exchange_step_settles() {
        let mut l = barycenter_stalls();
        let mut exchanges = 0;
        for sweep in 0..4 {
            l.barycenter_pass(sweep % 2 == 0);
            let before = l.count_crossings();
            let made = l.untangle();
            assert!(
                made == 0 || l.count_crossings() < before,
                "{made} exchanges left the count at {} from {before}",
                l.count_crossings()
            );
            exchanges += made;
        }
        assert!(exchanges > 0, "there was something to fix");
        assert_eq!(l.untangle(), 0, "★ and a further run finds nothing left");
    }

    /// ★ R1443 — [`Layering::reduce_crossings`] returns the best order it saw,
    /// not the last: a barycenter pass is a heuristic and is free to leave a
    /// layering worse than it found it.
    #[test]
    fn r1443_a_worsening_sweep_does_not_survive() {
        // What the last sweep happened to leave, exchange step included.
        let mut last = a_sweep_overshoots();
        for sweep in 0..4 {
            last.barycenter_pass(sweep % 2 == 0);
            last.untangle();
        }
        let mut kept = a_sweep_overshoots();
        kept.reduce_crossings(4);
        assert!(
            kept.count_crossings() < last.count_crossings(),
            "★ the 4th sweep ended on {} crossings, an earlier one had found {}",
            last.count_crossings(),
            kept.count_crossings()
        );
        // And the guarantee that follows: no sweep budget is ever worse than
        // spending none at all.
        let unswept = a_sweep_overshoots().count_crossings();
        for sweeps in 0..8 {
            let mut l = a_sweep_overshoots();
            l.reduce_crossings(sweeps);
            assert!(
                l.count_crossings() <= unswept,
                "{sweeps} sweeps made it worse"
            );
        }
    }

    /// ★ R1443 — the balancing step rounds the same way for every vertex, so the
    /// separation the compaction established survives it on BOTH sides of the
    /// origin. `i32::midpoint` rounds towards zero, which does not.
    #[test]
    fn r1443_balancing_rounds_one_way() {
        assert_eq!(super::floor_midpoint(4, 7), 5);
        assert_eq!(
            super::floor_midpoint(-7, -4),
            -6,
            "★ down, not towards zero"
        );
        assert_eq!(super::floor_midpoint(-7, 4), -2);
        assert_eq!(super::floor_midpoint(-4, 4), 0);
        // The property the direction exists for: two coordinates a fixed
        // distance apart stay that far apart once averaged with another pair the
        // same distance apart, wherever they sit relative to zero.
        for lo in -9..9 {
            let near = super::floor_midpoint(lo, lo + 5);
            let far = super::floor_midpoint(lo + 3, lo + 8);
            assert_eq!(far - near, 3, "the gap survives the rounding at {lo}");
        }
    }
}
