//! R1442 — the layered (Sugiyama) pipeline over abstract vertices: cycle
//! breaking, layer assignment, ordering, and coordinates, in one call.
//!
//! [`Layering`] (R1441) is the back half — ordering and the Brandes–Köpf
//! coordinate solver. This module is the front half plus the assembly, so a
//! consumer maps its graph onto `0..n` once and reads positions back, instead of
//! re-deriving a cycle-break and a longest-path layering of its own.
//!
//! # Two ways to lay the same graph out
//!
//! [`Sugiyama::run`] draws a graph as if for the first time: each layer starts
//! in index order and barycenter sweeps reduce crossings. It is the right answer
//! for a one-shot "tidy this up" command, and the wrong one for a view that
//! re-lays itself out whenever the graph changes — a fresh ordering is free to
//! swap two nodes that the viewer had already learned the position of, and the
//! whole picture jumps for a reason nothing on screen explains.
//!
//! [`Sugiyama::run_seeded`] is the answer to that: the ordering is taken from
//! the PREVIOUS drawing's coordinates rather than from crossing minimisation, so
//! anything the viewer has already seen stays where it was and only the new
//! vertices have to find a slot. This is the *mental map* of Misue, Eades, Lai
//! and Sugiyama ("Layout Adjustment and the Mental Map", JVLC 1995), and the
//! same strategy Eclipse ELK exposes as its interactive crossing minimiser.
//!
//! **The two are in tension, and neither is free.** A seeded pass guarantees no
//! remembered pair is reordered ([`Layout::order_changes`] is 0 by
//! construction) but it never gets to *choose* an order, so its crossing count
//! ([`Layout::crossings`]) can only be worse than or equal to what a fresh pass
//! would find. Both numbers are published rather than argued about, so a caller
//! — or an AI over RPC — can see the trade it is making.

use crate::layering::Layering;

/// A layered-graph layout pass: the tunables shared by every phase.
///
/// The sweep count is fixed rather than iterated-to-convergence so a layout
/// always terminates and is deterministic (ZERO-FLAKE): identical input, byte
/// identical output, every time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sugiyama {
    /// Clear space between two vertices adjacent in a layer, along the free axis.
    pub row_gap: i32,
    /// The free-axis extent a long edge's BEND occupies in a layer it passes
    /// through. Small — a bend stands for a wire, not a node — but never zero:
    /// two wires crossing one layer must still be separable, and a zero-size
    /// vertex would let the compaction stack them on one coordinate.
    pub bend_size: i32,
    /// Alternating barycenter passes used by [`Sugiyama::run`]. Ignored by
    /// [`Sugiyama::run_seeded`], which does not reorder at all.
    pub sweeps: usize,
}

impl Default for Sugiyama {
    fn default() -> Self {
        Self {
            row_gap: 24,
            bend_size: 12,
            sweeps: 4,
        }
    }
}

impl Sugiyama {
    /// Lay out `sizes.len()` vertices connected by `edges` (`(from, to)` index
    /// pairs), choosing each layer's order to reduce crossings.
    ///
    /// A self-loop, and any pair naming a vertex outside `0..sizes.len()`, is
    /// skipped: neither can constrain a forward layering.
    #[must_use]
    pub fn run(&self, sizes: &[i32], edges: &[(usize, usize)]) -> Layout {
        self.solve(sizes, edges, None)
    }

    /// Lay the same graph out again, **keeping the drawing the viewer already
    /// has**: `seed[v]` is the free-axis coordinate vertex `v` held in the
    /// previous drawing, or `None` for one that did not appear in it.
    ///
    /// Each layer is ordered by those coordinates, so two vertices that both
    /// have a seed are drawn in the order the viewer remembers — whatever has
    /// changed elsewhere in the graph. A vertex without one (a newly added
    /// vertex, and every bend, which is derived afresh each pass) takes the mean
    /// of its keyed neighbours' seeds, which places it where its own edges point
    /// rather than at the end of the column; one with no keyed neighbour at all
    /// sorts last in its layer.
    ///
    /// The cost is that this pass never minimises crossings — see the module
    /// docs, and [`Layout::crossings`], which reports what the trade cost.
    #[must_use]
    pub fn run_seeded(
        &self,
        sizes: &[i32],
        edges: &[(usize, usize)],
        seed: &[Option<i32>],
    ) -> Layout {
        self.solve(sizes, edges, Some(seed))
    }

    /// The shared pipeline: the two entry points differ only in how the ORDER
    /// within each layer is chosen, so everything else has one definition.
    fn solve(
        &self,
        sizes: &[i32],
        edges: &[(usize, usize)],
        seed: Option<&[Option<i32>]>,
    ) -> Layout {
        let reals = sizes.len();
        if reals == 0 {
            return Layout::empty();
        }
        let kept = forward_edges(reals, edges);
        let forward: Vec<(usize, usize)> = kept.iter().map(|&(_, pair)| pair).collect();
        let layer_of = assign_layers(reals, &forward);

        let mut layering = Layering::new(sizes.to_vec(), &layer_of, &forward);
        let chains = layering.split_long_edges(self.bend_size);
        match seed {
            Some(seed) => layering.order_by_seed(seed),
            None => layering.reduce_crossings(self.sweeps),
        }
        let centre = layering.brandes_koepf(self.row_gap);

        // Back onto the caller's edge indices: a dropped edge (a self-loop, or
        // the one broken out of a cycle) keeps an empty route, so `routes` can
        // be read positionally against the `edges` that came in.
        let mut routes: Vec<Vec<(usize, i32)>> = vec![Vec::new(); edges.len()];
        for (chain, &(source, (upper, _))) in chains.iter().zip(&kept) {
            routes[source] = chain
                .iter()
                .enumerate()
                .map(|(step, &bend)| (layer_of[upper] + 1 + step, centre[bend]))
                .collect();
        }

        Layout {
            layering,
            layer_of,
            centre,
            sizes: sizes.to_vec(),
            reals,
            routes,
        }
    }
}

/// A solved layout: which layer each vertex landed in, where it sits along the
/// free axis, and the metrics that say how good the answer is.
///
/// Coordinates are **centres**, not top edges — an edge joins the middles of its
/// endpoints, so "straight" has to mean equal centres for vertices of unequal
/// size.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The solved layering, bends included — the metrics read it.
    layering: Layering,
    /// Layer of each real vertex.
    layer_of: Vec<usize>,
    /// Free-axis centre of every vertex; the real ones are the prefix.
    centre: Vec<i32>,
    /// Free-axis extent of each real vertex, as given.
    sizes: Vec<i32>,
    /// How many of the vertices are real (the rest are bends).
    reals: usize,
    /// `(layer, centre)` of each bend, per edge of the caller's input.
    routes: Vec<Vec<(usize, i32)>>,
}

impl Layout {
    fn empty() -> Self {
        Self {
            layering: Layering::default(),
            layer_of: Vec::new(),
            centre: Vec::new(),
            sizes: Vec::new(),
            reals: 0,
            routes: Vec::new(),
        }
    }

    /// The layer of each real vertex, by input index. Layer `l` is the `l`-th
    /// column (or row — the caller names the axis).
    #[must_use]
    pub fn layers(&self) -> &[usize] {
        &self.layer_of
    }

    /// The free-axis CENTRE of each real vertex, by input index.
    #[must_use]
    pub fn centres(&self) -> &[i32] {
        &self.centre[..self.reals]
    }

    /// The smallest leading edge over the real vertices — subtract it to anchor
    /// a drawing whose topmost vertex should sit at a chosen origin.
    ///
    /// Bends are excluded deliberately: a bend is a wire, and letting one set
    /// the anchor would drift the whole drawing by half a wire.
    #[must_use]
    pub fn top(&self) -> i32 {
        (0..self.reals)
            .map(|v| self.centre[v] - self.sizes[v] / 2)
            .min()
            .unwrap_or(0)
    }

    /// How many layers the drawing has.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.layering.layers.len()
    }

    /// Where edge `edge` — indexed as the caller passed it — passes through the
    /// layers between its endpoints: `(layer, centre)` per bend, in order.
    ///
    /// A view that draws edges as polylines routes them through these points.
    /// The layout reserved the space, so a wire drawn straight from endpoint to
    /// endpoint instead is free to cut across a card sitting in a column it
    /// passes over — the reservation only pays if it is used. Empty for an edge
    /// between adjacent layers (nothing to route around), and for one the cycle
    /// break dropped, which the caller draws however it marks feedback.
    #[must_use]
    pub fn route(&self, edge: usize) -> &[(usize, i32)] {
        self.routes.get(edge).map_or(&[], Vec::as_slice)
    }

    /// How many bends were inserted — the number of layers long edges pass
    /// through, and the reason the ordering could see them at all.
    #[must_use]
    pub fn bends(&self) -> usize {
        self.layering.len() - self.reals
    }

    /// Edge crossings between adjacent layers: the tidiness metric, and a
    /// property of the ORDER alone.
    #[must_use]
    pub fn crossings(&self) -> usize {
        self.layering.count_crossings()
    }

    /// `(inner segments, of those drawn straight)` — Brandes–Köpf's guarantee,
    /// countable. Both numbers ride together because "every inner segment is
    /// straight" says nothing about a graph that has none.
    #[must_use]
    pub fn inner_segments(&self) -> (usize, usize) {
        self.layering.inner_segment_straightness(&self.centre)
    }

    /// How many remembered pairs this drawing reordered: pairs of vertices that
    /// both appear in `seed` and share a layer here, drawn in the opposite order
    /// to the one the seed records.
    ///
    /// **Zero for a [`run_seeded`](Sugiyama::run_seeded) pass by construction**,
    /// and the point of one. Measuring a [`run`](Sugiyama::run) pass against the
    /// same seed is what makes the difference between the two an observation
    /// rather than a claim.
    #[must_use]
    pub fn order_changes(&self, seed: &[Option<i32>]) -> usize {
        self.layering.seed_order_changes(seed)
    }
}

/// The acyclic forward edges of `edges`, each tagged with **its index in
/// `edges`** so a caller can read an answer back positionally: self-loops and
/// out-of-range pairs dropped, then back-edges removed by an iterative forward
/// DFS — an edge into a vertex still on the DFS stack is a back-edge.
///
/// A layered form needs an acyclic base, and a caller's graph may legitimately
/// contain a cycle (a feedback dependency is a real shape, not a bug), so the
/// cycle is broken here rather than rejected.
fn forward_edges(count: usize, edges: &[(usize, usize)]) -> Vec<(usize, (usize, usize))> {
    let mut kept: Vec<(usize, (usize, usize))> = Vec::with_capacity(edges.len());
    for (source, &(from, to)) in edges.iter().enumerate() {
        debug_assert!(
            from < count && to < count,
            "edge ({from}, {to}) names a vertex outside 0..{count}"
        );
        if from != to && from < count && to < count {
            kept.push((source, (from, to)));
        }
    }
    // Successor lists, each entry tagged with its index in `kept` so the DFS can
    // name the exact edge it classifies. Sorted by target so the walk — and
    // therefore which edge of a cycle is dropped — is deterministic.
    let mut succ: Vec<Vec<(usize, usize)>> = vec![Vec::new(); count];
    for (index, &(_, (from, to))) in kept.iter().enumerate() {
        succ[from].push((index, to));
    }
    for list in &mut succ {
        list.sort_by_key(|&(_, to)| to);
    }

    // 0 = unvisited, 1 = on the stack, 2 = done.
    let mut color = vec![0u8; count];
    let mut back = vec![false; kept.len()];
    for root in 0..count {
        if color[root] != 0 {
            continue;
        }
        color[root] = 1;
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(vertex, cursor)) = stack.last() {
            let kids = &succ[vertex];
            if cursor < kids.len() {
                let (edge, to) = kids[cursor];
                stack.last_mut().expect("non-empty").1 += 1;
                match color[to] {
                    1 => back[edge] = true,
                    2 => {}
                    _ => {
                        color[to] = 1;
                        stack.push((to, 0));
                    }
                }
            } else {
                color[vertex] = 2;
                stack.pop();
            }
        }
    }
    kept.into_iter()
        .enumerate()
        .filter(|&(index, _)| !back[index])
        .map(|(_, edge)| edge)
        .collect()
}

/// Longest-path layering by Kahn topological order over acyclic `forward` edges:
/// a source (no in-edge) is layer 0, every other vertex one past its deepest
/// predecessor, so every edge spans at least one layer.
fn assign_layers(count: usize, forward: &[(usize, usize)]) -> Vec<usize> {
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut indegree = vec![0usize; count];
    for &(from, to) in forward {
        succ[from].push(to);
        indegree[to] += 1;
    }
    let mut layer = vec![0usize; count];
    // A vertex is only ready once every predecessor is final, so the layer it
    // ends on is the longest path to it whatever order the queue drains in —
    // which is why a plain stack is enough and the result is still canonical.
    let mut ready: Vec<usize> = (0..count).filter(|&v| indegree[v] == 0).collect();
    while let Some(vertex) = ready.pop() {
        let next = layer[vertex] + 1;
        for &to in &succ[vertex] {
            layer[to] = layer[to].max(next);
            indegree[to] -= 1;
            if indegree[to] == 0 {
                ready.push(to);
            }
        }
    }
    layer
}

#[cfg(test)]
mod tests {
    use super::{Sugiyama, assign_layers, forward_edges};

    /// A diamond: `0` feeds `1` and `2`, both feeding `3`.
    fn diamond() -> (Vec<i32>, Vec<(usize, usize)>) {
        (vec![30; 4], vec![(0, 1), (0, 2), (1, 3), (2, 3)])
    }

    /// The kept edges without their provenance tags — what the layering itself
    /// consumes.
    fn forward_pairs(count: usize, edges: &[(usize, usize)]) -> Vec<(usize, usize)> {
        forward_edges(count, edges)
            .into_iter()
            .map(|(_, pair)| pair)
            .collect()
    }

    #[test]
    fn r1442_layers_follow_the_longest_path() {
        let (_, edges) = diamond();
        let forward = forward_pairs(4, &edges);
        assert_eq!(assign_layers(4, &forward), vec![0, 1, 1, 2]);
    }

    /// A cycle is broken, not rejected — one edge of it is dropped and the rest
    /// layers normally.
    #[test]
    fn r1442_a_cycle_is_broken_rather_than_rejected() {
        let forward = forward_pairs(3, &[(0, 1), (1, 2), (2, 0)]);
        assert_eq!(forward.len(), 2, "exactly one edge of the cycle is dropped");
        assert_eq!(assign_layers(3, &forward), vec![0, 1, 2]);
    }

    /// Self-loops cannot constrain a forward layering and are skipped.
    #[test]
    fn r1442_a_self_loop_is_skipped() {
        // The self-loop is dropped, and the survivor still names its index in the
        // caller's list — index 1, not 0.
        assert_eq!(forward_edges(2, &[(0, 0), (0, 1)]), vec![(1, (0, 1))]);
    }

    /// The layout of an empty graph is empty, not a panic.
    #[test]
    fn r1442_an_empty_graph_lays_out_empty() {
        let out = Sugiyama::default().run(&[], &[]);
        assert!(out.centres().is_empty());
        assert_eq!(out.depth(), 0);
        assert_eq!(out.crossings(), 0);
        assert_eq!(out.inner_segments(), (0, 0));
        assert_eq!(out.order_changes(&[]), 0);
    }

    /// Vertices in one layer keep their separation, and the layer assignment
    /// reaches the caller as given indices.
    #[test]
    fn r1442_a_run_separates_a_layer_and_reports_its_layers() {
        let (sizes, edges) = diamond();
        let pass = Sugiyama::default();
        let out = pass.run(&sizes, &edges);
        assert_eq!(out.layers(), &[0, 1, 1, 2]);
        let apart = (out.centres()[1] - out.centres()[2]).abs();
        assert!(
            apart >= 30 + pass.row_gap,
            "the two middle vertices clear each other: {apart}"
        );
    }

    /// ★ A seeded pass keeps every remembered pair in order. The seed here is
    /// the REVERSE of what a fresh pass chooses, so agreeing with it cannot be a
    /// coincidence.
    #[test]
    fn r1442_a_seeded_pass_draws_the_order_it_was_given() {
        let (sizes, edges) = diamond();
        let pass = Sugiyama::default();
        let fresh = pass.run(&sizes, &edges);
        let (above, below) = if fresh.centres()[1] < fresh.centres()[2] {
            (1, 2)
        } else {
            (2, 1)
        };
        // Ask for the opposite of what the fresh pass drew.
        let mut seed = vec![None; 4];
        seed[above] = Some(500);
        seed[below] = Some(100);
        let seeded = pass.run_seeded(&sizes, &edges, &seed);
        assert!(
            seeded.centres()[below] < seeded.centres()[above],
            "the seeded pass honours the seed, not the barycenter: {:?}",
            seeded.centres()
        );
        assert_eq!(seeded.order_changes(&seed), 0, "★ nothing was reordered");
        assert!(
            fresh.order_changes(&seed) > 0,
            "and the fresh pass DID reorder against that seed — \
             without this the assertion above would be vacuous"
        );
    }

    /// ★ A new vertex lands beside the neighbours it connects to, not at the end
    /// of its column — the key propagation, and the reason a seeded pass is
    /// usable at all when the graph grows.
    ///
    /// The fixture is built so that "placed by its neighbours" and "appended
    /// last" give DIFFERENT answers: the new source feeds the sink at the TOP,
    /// while sorting an unkeyed vertex last would drop it to the bottom. A first
    /// version of this test had the new vertex feeding the BOTTOM sink, where
    /// the two rules agree — and it passed with the propagation disabled.
    #[test]
    fn r1442_a_new_vertex_is_placed_by_its_neighbours() {
        // 0 and 1 are remembered sources, far apart; 3 and 4 their sinks. The
        // new source 2 feeds sink 3, the TOP one.
        let sizes = vec![30; 5];
        let edges = vec![(0, 3), (1, 4), (2, 3)];
        let seed = vec![Some(0), Some(400), None, Some(0), Some(400)];
        let out = Sugiyama::default().run_seeded(&sizes, &edges, &seed);
        let centres = out.centres();
        assert!(
            centres[2] < centres[1],
            "★ the new source sits with the sink it feeds, near the top — not \
             parked below everything remembered: {centres:?}"
        );
        assert_eq!(out.order_changes(&seed), 0, "and nothing remembered moved");
    }

    /// The trade is real and measurable in the direction claimed: on a graph
    /// whose remembered order is bad, staying stable costs crossings.
    #[test]
    fn r1442_stability_costs_crossings_and_says_so() {
        // Two parallel chains, seeded CROSSED: source A remembered above source
        // B, but A's sink remembered below B's.
        let sizes = vec![30; 4];
        let edges = vec![(0, 2), (1, 3)];
        let seed = vec![Some(0), Some(200), Some(200), Some(0)];
        let pass = Sugiyama::default();
        let seeded = pass.run_seeded(&sizes, &edges, &seed);
        let fresh = pass.run(&sizes, &edges);
        assert_eq!(seeded.order_changes(&seed), 0, "the seed is honoured");
        assert_eq!(
            seeded.crossings(),
            1,
            "and the remembered crossing survives"
        );
        assert_eq!(fresh.crossings(), 0, "a fresh pass would have untangled it");
        assert!(
            fresh.order_changes(&seed) > 0,
            "at the cost of moving something the viewer had learned"
        );
    }

    /// ★ A long edge's route names the layers it passes through and the track it
    /// takes in each — and that track is where the inner segment was solved, so
    /// a view drawing the polyline draws the straight run the solver promised.
    #[test]
    fn r1442_a_long_edge_reports_the_track_it_takes() {
        // 0 -> 1 -> 2 -> 3, a long 0 -> 3, a self-loop, and a back-edge.
        let sizes = vec![30; 4];
        let edges = vec![(0, 1), (1, 2), (2, 3), (0, 3), (2, 2), (3, 0)];
        let out = Sugiyama::default().run(&sizes, &edges);

        let long = out.route(3);
        assert_eq!(long.len(), 2, "one bend per layer it passes through");
        assert_eq!(
            long.iter().map(|&(layer, _)| layer).collect::<Vec<_>>(),
            vec![1, 2],
            "and they are the layers BETWEEN its endpoints"
        );
        assert_eq!(
            long[0].1, long[1].1,
            "★ the inner segment is one coordinate, so the middle runs straight"
        );

        assert!(
            out.route(0).is_empty(),
            "an adjacent-layer edge needs no bend"
        );
        assert!(out.route(4).is_empty(), "nor does a dropped self-loop");
        assert!(
            out.route(5).is_empty(),
            "nor a back-edge the cycle break took"
        );
        assert!(
            out.route(99).is_empty(),
            "and an unknown edge is not a panic"
        );
    }

    /// Bends are unseeded every pass (they are derived, not remembered), so a
    /// long edge still lands on a sensible track under a seeded layout.
    #[test]
    fn r1442_a_seeded_pass_still_routes_a_long_edge() {
        // 0 -> 1 -> 2 -> 3 with a long 0 -> 3 alongside: one inner segment.
        let sizes = vec![30; 4];
        let edges = vec![(0, 1), (1, 2), (2, 3), (0, 3)];
        let seed = vec![Some(0), Some(0), Some(0), Some(0)];
        let out = Sugiyama::default().run_seeded(&sizes, &edges, &seed);
        assert_eq!(out.bends(), 2, "the long edge crosses two layers");
        let (total, straight) = out.inner_segments();
        assert_eq!(total, 1);
        assert_eq!(straight, 1, "and its middle still runs straight");
    }
}
