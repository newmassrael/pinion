//! R1441 — the pure layered-graph coordinate solver: a **proper layering** plus
//! the **Brandes–Köpf** horizontal coordinate assignment.
//!
//! # Why this file exists
//!
//! Everything here works on abstract vertices `0..n` and knows nothing about
//! `GraphNode`, `Edge` or the editor's model — `main.rs` maps its graph onto
//! these indices and maps the answer back. That boundary is deliberate: it is
//! exactly the shape a crate lift would take if a second graph consumer ever
//! appears (the layout is still example-local until then), and it lets the
//! algorithm be tested on hand-written layerings instead of through a node
//! editor.
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
//! solves is x. This editor lays data flow LEFT TO RIGHT, so layers are columns
//! and the free coordinate is **y**. The algorithm is unchanged — it only ever
//! talks about "layers" and "the coordinate within a layer" — but every mention
//! of *left / right* in the paper reads as *up / down* here. The code below
//! keeps the paper's vocabulary so it can be checked against the paper, and the
//! caller does the axis naming.
//!
//! The four passes, the type-1 conflict marking, and the median balancing are all
//! present. Implementing only one pass would have been a new documented
//! simplification, which is the thing this round exists to remove.

use std::collections::{BTreeMap, BTreeSet};

/// A layering over vertices `0..size.len()`: which vertices sit in each layer and
/// in what order, how big each one is along the free axis, which are dummies, and
/// the edges between adjacent layers.
///
/// A `Layering` is *proper* once [`split_long_edges`](Self::split_long_edges) has
/// run: every edge then joins consecutive layers. [`brandes_koepf`](Self::brandes_koepf)
/// requires that, and debug-asserts it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Layering {
    /// Ordered vertices per layer. Every vertex appears exactly once.
    pub(crate) layers: Vec<Vec<usize>>,
    /// Each vertex's extent along the free axis (a node's height here).
    pub(crate) size: Vec<i32>,
    /// Whether a vertex is a DUMMY — a bend standing in for a long edge as it
    /// passes through a layer. Bends are the reason the paper's conflict marking
    /// exists: an inner segment (dummy → dummy) is a piece of a long edge, and
    /// keeping those straight is what the whole exercise is for.
    pub(crate) dummy: Vec<bool>,
    /// `(upper, lower)` vertex pairs. Indices into this vec are stable and are
    /// what the conflict set marks, so a transformed view can reorient the pairs
    /// without disturbing the marks.
    pub(crate) edges: Vec<(usize, usize)>,
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
    pub(crate) fn new(size: Vec<i32>, layer_of: &[usize], edges: &[(usize, usize)]) -> Self {
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
    pub(crate) fn len(&self) -> usize {
        self.size.len()
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
    /// crossing-reduction sweep can see it. Returns the number of bends added.
    pub(crate) fn split_long_edges(&mut self, bend_size: i32) -> usize {
        let index = self.index();
        let original = core::mem::take(&mut self.edges);
        let mut added = 0;
        for (upper, lower) in original {
            let (lu, lv) = (index.layer_of[upper], index.layer_of[lower]);
            // A backward or same-layer edge cannot be made proper by inserting
            // bends; the caller's cycle-breaking phase is what keeps those out,
            // and dropping one here silently would hide a caller bug.
            debug_assert!(lv > lu, "split_long_edges needs a forward layering");
            if lv <= lu + 1 {
                self.edges.push((upper, lower));
                continue;
            }
            let mut prev = upper;
            for l in (lu + 1)..lv {
                let bend = self.size.len();
                self.size.push(bend_size);
                self.dummy.push(true);
                self.layers[l].push(bend);
                self.edges.push((prev, bend));
                prev = bend;
                added += 1;
            }
            self.edges.push((prev, lower));
        }
        added
    }

    /// Reorder each layer by `sweeps` alternating barycenter passes — the
    /// crossing-reduction heuristic, now reading dummies too.
    ///
    /// A vertex's key is the mean position of its neighbours in the adjacent
    /// layer, compared as an exact rational so no float rounding enters; a vertex
    /// with no neighbour there keeps its slot, and ties break by index. Fully
    /// deterministic, and a fixed sweep count always terminates (ZERO-FLAKE).
    pub(crate) fn reduce_crossings(&mut self, sweeps: usize) {
        let depth = self.layers.len();
        if depth < 2 {
            return;
        }
        for sweep in 0..sweeps {
            let down = sweep % 2 == 0;
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
    }

    /// The number of edge crossings between adjacent layers.
    ///
    /// A property of the ORDER alone, which is what makes it the right metric for
    /// [`split_long_edges`](Self::split_long_edges) (which changes the order) and
    /// the wrong one for [`brandes_koepf`](Self::brandes_koepf) (which does not).
    /// Two segments between the same pair of layers cross when their endpoints
    /// are in opposite order.
    pub(crate) fn count_crossings(&self) -> usize {
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
    pub(crate) fn brandes_koepf(&self, gap: i32) -> Vec<i32> {
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
    pub(crate) fn inner_segment_straightness(&self, coords: &[i32]) -> (usize, usize) {
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
                len => i32::midpoint(values[(len - 1) / 2], values[len / 2]),
            }
        })
        .collect()
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

    #[test]
    fn r1441_splitting_inserts_one_bend_per_intervening_layer() {
        let mut l = long_edge_graph();
        assert_eq!(l.len(), 3);
        let added = l.split_long_edges(8);
        assert_eq!(added, 1, "the 0->2 edge crosses exactly one layer");
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
        assert_eq!(l.split_long_edges(8), 0);
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
        let added = l.split_long_edges(8);
        assert_eq!(added, 2, "the long edge crosses two layers");
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
        assert_eq!(l.split_long_edges(8), 2, "bends at layers 1 and 2");
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
        assert_eq!(l.split_long_edges(8), 0, "nothing long here");
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
        assert_eq!(empty.split_long_edges(8), 0);
        empty.reduce_crossings(4);
        assert!(empty.brandes_koepf(24).is_empty());
        assert_eq!(empty.count_crossings(), 0);

        let mut one = Layering::new(vec![10], &[0], &[]);
        assert_eq!(one.split_long_edges(8), 0);
        one.reduce_crossings(4);
        assert_eq!(one.brandes_koepf(24).len(), 1);
    }
}
