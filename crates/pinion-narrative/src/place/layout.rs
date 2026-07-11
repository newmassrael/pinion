//! The layout solver: a relation [`PlaceGraph`] → 2D coordinates.
//!
//! This is the piece the field report undersold as "wiring" (F3): the
//! coordinates are not in the data, the renderer **solves** them from the
//! relations. The solver is deliberately **deterministic** — no RNG, no
//! wall-clock — so a `dry_run` (§2 #3) and a live render agree, and the
//! same graph always lays out the same way.
//!
//! ## Method — directional lattice placement
//!
//! 1. Build an adjacency list from the explicit adjacencies (symmetric:
//!    `B East of A` ⇒ `A West of B`) plus an implicit undirected edge for
//!    each containment (a child is laid out near its parent so the
//!    enclosing box stays tight).
//! 2. Walk each connected component breadth-first in author order. A
//!    directed edge steps the neighbour one lattice cell in its direction
//!    (walking further if the cell is taken); an undirected edge takes the
//!    nearest free cell. Fresh components start to the right of everything
//!    placed so far, so they never overlap.
//! 3. Translate the integer lattice to pixel rects.
//!
//! Directional relations are satisfied exactly for a consistent graph (a
//! lattice cannot honour mutually contradictory directions; there the
//! first edge to reach a node wins, deterministically).

use std::collections::{HashSet, VecDeque};

use pinion_core::scene::Rect;

use crate::place::model::{Direction, PlaceGraph};

/// Placed node width, in pixels.
const NODE_W: u32 = 150;
/// Placed node height, in pixels.
const NODE_H: u32 = 44;
/// Horizontal gap between lattice columns, in pixels.
const GAP_X: u32 = 44;
/// Vertical gap between lattice rows, in pixels.
const GAP_Y: u32 = 46;
/// Outer margin so the leftmost / topmost node (and its container padding)
/// stays within positive coordinates.
const PAD: u32 = 28;
/// Padding a container box adds around the union of its contents.
const CONTAINER_PAD: u32 = 12;

/// A place placed at a solved lattice cell and its pixel rect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacedNode {
    /// The place `id`.
    pub id: String,
    /// The place label.
    pub label: String,
    /// The solved integer lattice cell `(col, row)` (may be negative before
    /// translation; kept for introspection / testing).
    pub cell: (i32, i32),
    /// The pixel rect the node paints at.
    pub rect: Rect,
    /// The index of this node's container, if any.
    pub contained_by: Option<usize>,
}

/// The solved layout: nodes with rects, and edges as index pairs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceLayout {
    /// Placed nodes, parallel to [`PlaceGraph::places`].
    pub nodes: Vec<PlacedNode>,
    /// Adjacency edges as `[from_index, to_index]` pairs (explicit
    /// adjacencies only; containment is drawn as an enclosing box, not a
    /// line).
    pub edges: Vec<[usize; 2]>,
}

impl PlaceLayout {
    /// `true` when the layout has no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The index of the placed node with `id`, if present.
    #[must_use]
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// Whether any node declares `idx` as its container.
    #[must_use]
    pub fn is_container(&self, idx: usize) -> bool {
        self.nodes.iter().any(|n| n.contained_by == Some(idx))
    }

    /// The enclosing rect for the container at `idx`: the union of its own
    /// rect and the (recursive) bounds of everything it contains, plus
    /// padding. Cycle-guarded, so malformed containment cannot loop.
    #[must_use]
    pub fn container_bounds(&self, idx: usize) -> Rect {
        let mut visited = vec![false; self.nodes.len()];
        pad_rect(self.raw_bounds(idx, &mut visited), CONTAINER_PAD)
    }

    fn raw_bounds(&self, idx: usize, visited: &mut [bool]) -> Rect {
        if visited[idx] {
            return self.nodes[idx].rect;
        }
        visited[idx] = true;
        let mut bounds = self.nodes[idx].rect;
        for child in 0..self.nodes.len() {
            if self.nodes[child].contained_by == Some(idx) && !visited[child] {
                bounds = bounds.union(self.raw_bounds(child, visited));
            }
        }
        bounds
    }
}

/// Solve 2D coordinates for a relation place-graph. Deterministic.
#[must_use]
pub fn solve_layout(graph: &PlaceGraph) -> PlaceLayout {
    let n = graph.place_count();
    let adj = build_adjacency(graph);

    let mut cell: Vec<Option<(i32, i32)>> = vec![None; n];
    let mut occupied: HashSet<(i32, i32)> = HashSet::new();
    let mut next_root_x: i32 = 0;

    for root in 0..n {
        if cell[root].is_some() {
            continue;
        }
        // A fresh component starts right of everything placed so far.
        place(root, (next_root_x, 0), &mut cell, &mut occupied);
        let mut queue = VecDeque::new();
        queue.push_back(root);
        while let Some(x) = queue.pop_front() {
            // A dequeued node is always placed; skip rather than unwrap.
            let Some(xc) = cell[x] else { continue };
            for &(neighbour, dir) in &adj[x] {
                if cell[neighbour].is_some() {
                    continue;
                }
                let target = match dir {
                    Some(d) => walk_free(xc, d.offset(), &occupied),
                    None => nearest_free(xc, &occupied),
                };
                place(neighbour, target, &mut cell, &mut occupied);
                queue.push_back(neighbour);
            }
        }
        next_root_x = occupied
            .iter()
            .map(|c| c.0)
            .max()
            .map_or(next_root_x + 2, |m| m + 2);
    }

    let min_x = cell.iter().flatten().map(|c| c.0).min().unwrap_or(0);
    let min_y = cell.iter().flatten().map(|c| c.1).min().unwrap_or(0);

    let nodes = graph
        .places
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (cx, cy) = cell[i].unwrap_or((min_x, min_y));
            let col = u32::try_from(cx - min_x).unwrap_or(0);
            let row = u32::try_from(cy - min_y).unwrap_or(0);
            PlacedNode {
                id: p.id.clone(),
                label: p.label.clone(),
                cell: (cx, cy),
                rect: Rect::new(
                    PAD + col * (NODE_W + GAP_X),
                    PAD + row * (NODE_H + GAP_Y),
                    NODE_W,
                    NODE_H,
                ),
                contained_by: p.contained_by.as_ref().and_then(|pid| graph.index_of(pid)),
            }
        })
        .collect();

    let edges = graph
        .adjacencies
        .iter()
        .filter_map(|e| Some([graph.index_of(&e.from)?, graph.index_of(&e.to)?]))
        .collect();

    PlaceLayout { nodes, edges }
}

/// Symmetric adjacency list: `(neighbour, direction-from-this-node)`.
fn build_adjacency(graph: &PlaceGraph) -> Vec<Vec<(usize, Option<Direction>)>> {
    let mut adj: Vec<Vec<(usize, Option<Direction>)>> = vec![Vec::new(); graph.place_count()];
    for e in &graph.adjacencies {
        if let (Some(a), Some(b)) = (graph.index_of(&e.from), graph.index_of(&e.to)) {
            if a != b {
                adj[a].push((b, e.direction));
                adj[b].push((a, e.direction.map(Direction::opposite)));
            }
        }
    }
    // Containment implies "near" — keep a child adjacent to its parent so
    // the enclosing box is tight.
    for (i, p) in graph.places.iter().enumerate() {
        if let Some(parent) = &p.contained_by {
            if let Some(pi) = graph.index_of(parent) {
                if pi != i {
                    adj[i].push((pi, None));
                    adj[pi].push((i, None));
                }
            }
        }
    }
    adj
}

fn place(
    idx: usize,
    at: (i32, i32),
    cell: &mut [Option<(i32, i32)>],
    occupied: &mut HashSet<(i32, i32)>,
) {
    cell[idx] = Some(at);
    occupied.insert(at);
}

/// Step from `from` in `step` until a free cell is found (the cell one step
/// away if it is free).
fn walk_free(from: (i32, i32), step: (i32, i32), occupied: &HashSet<(i32, i32)>) -> (i32, i32) {
    let mut c = (from.0 + step.0, from.1 + step.1);
    while occupied.contains(&c) {
        c = (c.0 + step.0, c.1 + step.1);
    }
    c
}

/// The nearest free cell around `from`, scanning outward ring by ring in a
/// fixed clockwise order (deterministic).
fn nearest_free(from: (i32, i32), occupied: &HashSet<(i32, i32)>) -> (i32, i32) {
    let mut r = 1;
    loop {
        for (dx, dy) in ring(r) {
            let c = (from.0 + dx, from.1 + dy);
            if !occupied.contains(&c) {
                return c;
            }
        }
        r += 1;
    }
}

/// The cells at Chebyshev radius `r`, in a fixed perimeter order.
fn ring(r: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for x in -r..=r {
        v.push((x, -r));
    }
    for y in (-r + 1)..=r {
        v.push((r, y));
    }
    for x in (-r..r).rev() {
        v.push((x, r));
    }
    for y in ((-r + 1)..r).rev() {
        v.push((-r, y));
    }
    v
}

fn pad_rect(rect: Rect, pad: u32) -> Rect {
    Rect::new(
        rect.x.saturating_sub(pad),
        rect.y.saturating_sub(pad),
        rect.w + 2 * pad,
        rect.h + 2 * pad,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::place::model::{Adjacency, Place};

    fn place_node(id: &str) -> Place {
        Place {
            id: id.to_string(),
            label: id.to_string(),
            contained_by: None,
        }
    }

    fn edge(from: &str, to: &str, direction: Option<Direction>) -> Adjacency {
        Adjacency {
            from: from.to_string(),
            to: to.to_string(),
            direction,
        }
    }

    #[test]
    fn directional_edges_place_neighbour_in_direction() {
        // A directional tree: every edge is a placing edge, so each is
        // satisfied exactly.
        let graph = PlaceGraph {
            places: vec![
                place_node("center"),
                place_node("north"),
                place_node("east"),
                place_node("far_east"),
            ],
            adjacencies: vec![
                edge("center", "north", Some(Direction::North)),
                edge("center", "east", Some(Direction::East)),
                edge("east", "far_east", Some(Direction::East)),
            ],
        };
        let layout = solve_layout(&graph);
        let cell = |id: &str| layout.nodes.iter().find(|n| n.id == id).unwrap().cell;
        let (cx, cy) = cell("center");
        assert!(cell("north").1 < cy, "north has smaller y");
        assert_eq!(cell("north").0, cx, "north keeps the column");
        assert!(cell("east").0 > cx, "east has larger x");
        assert!(
            cell("far_east").0 > cell("east").0,
            "far_east is further east"
        );
    }

    #[test]
    fn is_deterministic() {
        let graph = PlaceGraph {
            places: vec![
                place_node("a"),
                place_node("b"),
                place_node("c"),
                place_node("d"),
            ],
            adjacencies: vec![
                edge("a", "b", Some(Direction::East)),
                edge("a", "c", Some(Direction::South)),
                edge("b", "d", None),
            ],
        };
        assert_eq!(solve_layout(&graph), solve_layout(&graph));
    }

    #[test]
    fn every_node_gets_a_unique_cell() {
        let graph = PlaceGraph {
            places: vec![
                place_node("a"),
                place_node("b"),
                place_node("c"),
                place_node("lonely"),
            ],
            adjacencies: vec![
                edge("a", "b", Some(Direction::East)),
                edge("a", "c", Some(Direction::East)),
            ],
        };
        let layout = solve_layout(&graph);
        let cells: HashSet<(i32, i32)> = layout.nodes.iter().map(|n| n.cell).collect();
        assert_eq!(cells.len(), layout.nodes.len(), "no two nodes share a cell");
    }

    #[test]
    fn contained_node_is_inside_container_bounds() {
        let graph = PlaceGraph {
            places: vec![
                Place {
                    id: "village".to_string(),
                    label: "마을".to_string(),
                    contained_by: None,
                },
                Place {
                    id: "shrine".to_string(),
                    label: "굿당".to_string(),
                    contained_by: Some("village".to_string()),
                },
            ],
            adjacencies: vec![],
        };
        let layout = solve_layout(&graph);
        let village = layout.index_of("village").unwrap();
        let bounds = layout.container_bounds(village);
        let shrine = &layout.nodes[layout.index_of("shrine").unwrap()];
        assert!(layout.is_container(village));
        assert!(
            rect_contains(bounds, shrine.rect),
            "shrine {:?} inside village bounds {bounds:?}",
            shrine.rect
        );
    }

    fn rect_contains(outer: Rect, inner: Rect) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.x + inner.w <= outer.x + outer.w
            && inner.y + inner.h <= outer.y + outer.h
    }
}
