//! R1577 — the boundary of a selected sub-graph, as pure arithmetic.
//!
//! # Why this module exists
//!
//! "Wrap this selection in a re-usable node" is the single largest capability
//! a node editor has — the DCC's node groups, the engine Blueprint's
//! collapse-to- function, a compositor's macro. It looks like an editor
//! gesture and is not: the gesture is a keystroke, and everything that decides
//! whether the gesture is *legal*, and what the resulting node's sockets
//! *are*, is arithmetic over a vertex set. Putting it here keeps that
//! arithmetic testable on hand-written graphs — which is where its adversarial
//! fixtures live — and out of the editor, where it would only ever be
//! exercised through a pointer.
//!
//! # What an interface socket is
//!
//! A group's sockets are not authored. They are **derived from the links that
//! cross the selection boundary**, and the rule is one sentence:
//!
//! > An interface socket is one *value* crossing the boundary, and a value is
//! > identified by the socket that PRODUCES it.
//!
//! So an input socket is keyed by the external producer (two selected nodes fed
//! from one outside output share one group input — it is one value), and an
//! output socket is keyed by the internal producer (one selected output feeding
//! three outside inputs is one group output, for the same reason). Both
//! directions key on the producer end, because "a value" is what the socket
//! carries.
//!
//! The DCC reaches the same two behaviours through two independent booleans on
//! its interface builder (`use_unique_input`, `use_unique_output`) whose struct defaults — measured against
//! `8cf50599` — are the *opposite* of what its own group-make operator passes, so the
//! shared-value rule is a call-site convention there rather than a property of
//! what an interface socket is.
//!
//! An unconnected socket on a selected vertex is therefore **not** part of the
//! interface: nothing crosses. The DCC special-cases a single-node selection
//! to expose the whole signature, and its own source marks that inconsistency
//! with a `TODO`; the rule above is kept whole instead.
//!
//! # Why a collapse can be refused
//!
//! Collapsing a set `S` into one vertex creates a cycle through that vertex
//! exactly when some path leaves `S` and comes back — `s₁ → u → … → s₂` with
//! every interior vertex outside `S`. That is a **reachability** property, and
//! [`Boundary::derive`] tests it as one, reporting the offending path.
//!
//! The DCC tests a *one-hop* approximation of it: no unselected node may have
//! both an input from the selection and an output to it. A two-hop bypass
//! (`s → u → v → s`) passes that test, and the group is created — the resulting tree is
//! cyclic, and the cycle is discovered later by a separate tree-update pass
//! that flags the links rather than by anything that could have refused.
//! Measured at `8cf50599`: `space_node/node_group.cc` does not contain the substring `cycle` at all.
//!
//! ```
//! use pinion_graph::group::{Boundary, Link, Socket};
//!
//! // 0 → 1 → 2 → 3, and we try to group {0, 3}: a path leaves and returns.
//! let s = |v: usize| Socket::new(v, 0);
//! let links = [
//!     Link::new(s(0), s(1)),
//!     Link::new(s(1), s(2)),
//!     Link::new(s(2), s(3)),
//! ];
//! let refusal = Boundary::derive(4, &links, &[0, 3]).unwrap_err();
//! assert_eq!(refusal.bypass(), Some(&[0, 1, 2, 3][..]));
//!
//! // Grouping {1, 2} instead is fine, and has one socket each way.
//! let boundary = Boundary::derive(4, &links, &[1, 2]).unwrap();
//! assert_eq!(boundary.inputs().len(), 1);
//! assert_eq!(boundary.outputs().len(), 1);
//! assert_eq!(boundary.internal(), &[1]);
//! ```

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// One end of a link: a port on a vertex.
///
/// `port` is the caller's own socket index within the vertex's input or output
/// list. This module never interprets it — it only ever compares two of them —
/// so a caller whose sockets are named rather than numbered maps its names onto
/// `0..k` the same way it maps its nodes onto `0..n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Socket {
    /// The vertex this socket belongs to, in the caller's `0..order` numbering.
    pub vertex: usize,
    /// The socket's index within that vertex's port list.
    pub port: u32,
}

impl Socket {
    /// A socket on `vertex` at `port`.
    #[must_use]
    pub const fn new(vertex: usize, port: u32) -> Self {
        Self { vertex, port }
    }
}

/// A directed link: the value produced at `from` is consumed at `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Link {
    /// The producing socket (an output, in the caller's vocabulary).
    pub from: Socket,
    /// The consuming socket (an input).
    pub to: Socket,
}

impl Link {
    /// A link from `from` to `to`.
    #[must_use]
    pub const fn new(from: Socket, to: Socket) -> Self {
        Self { from, to }
    }
}

/// One socket of the derived group interface: a single value crossing the
/// boundary, together with every link that carries it.
///
/// The [`Self::producer`] is the identity — see the module docs. For an input
/// socket the producer is outside the selection and the [`Self::consumers`] are
/// inside; for an output socket it is the other way round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceSocket {
    producer: Socket,
    consumers: Vec<Socket>,
    links: Vec<usize>,
}

impl InterfaceSocket {
    /// The socket producing the value this interface socket carries.
    #[must_use]
    pub const fn producer(&self) -> Socket {
        self.producer
    }

    /// Every socket consuming it, ascending and without repeats.
    ///
    /// Shorter than [`Self::links`] when the caller's graph holds two links
    /// between the same pair of sockets: those are one consumer, two carriers.
    #[must_use]
    pub fn consumers(&self) -> &[Socket] {
        &self.consumers
    }

    /// Indices into the caller's link slice that this socket subsumes,
    /// ascending.
    ///
    /// These are the links the caller rewires when it performs the collapse:
    /// each becomes a link to (or from) the new group vertex.
    #[must_use]
    pub fn links(&self) -> &[usize] {
        &self.links
    }
}

/// The derived boundary of a selection: what the group's sockets are, and which
/// of the caller's links land where.
///
/// Every link index in the caller's slice appears in exactly one of
/// [`Self::inputs`], [`Self::outputs`], [`Self::internal`] and
/// [`Self::outside`] — the four are a partition, which is what lets a caller
/// perform the collapse by consuming this value alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    inputs: Vec<InterfaceSocket>,
    outputs: Vec<InterfaceSocket>,
    internal: Vec<usize>,
    outside: Vec<usize>,
}

impl Boundary {
    /// Partition the links about `selected` **without** asking whether the
    /// selection could be collapsed into one vertex.
    ///
    /// `selected` is read as a set: repeats are ignored and the order carries no
    /// meaning.
    ///
    /// A cut is always possible where a collapse may not be — copying a
    /// sub-graph out of a tree severs the crossing links rather than routing
    /// them through a new vertex, so no cycle can arise and there is nothing to
    /// refuse. R1577 fused the two questions into [`Self::derive`], which is
    /// right for a collapse and wrong for every other consumer of the partition;
    /// R1578 needed the partition alone.
    ///
    /// # Errors
    ///
    /// [`Refusal::Empty`] when nothing is selected and
    /// [`Refusal::UnknownVertex`] when a selection or link index is outside
    /// `0..order`. **Never** [`Refusal::Bypass`] — that arm belongs to
    /// [`Self::derive`].
    pub fn cut(order: usize, links: &[Link], selected: &[usize]) -> Result<Self, Refusal> {
        let inside = membership(order, links, selected)?;

        // The producer keys the socket in BOTH directions — the whole rule.
        let mut inputs = Grouping::default();
        let mut outputs = Grouping::default();
        let mut internal = Vec::new();
        let mut outside = Vec::new();
        for (index, link) in links.iter().enumerate() {
            match (inside[link.from.vertex], inside[link.to.vertex]) {
                (true, true) => internal.push(index),
                (false, true) => inputs.push(link, index),
                (true, false) => outputs.push(link, index),
                (false, false) => outside.push(index),
            }
        }
        Ok(Self {
            inputs: inputs.finish(),
            outputs: outputs.finish(),
            internal,
            outside,
        })
    }

    /// Derive the boundary of `selected` within a graph of `order` vertices,
    /// for a **collapse**: the partition of [`Self::cut`], plus the check that
    /// routing every crossing through one new vertex stays acyclic.
    ///
    /// # Errors
    ///
    /// Everything [`Self::cut`] refuses, and [`Refusal::Bypass`] when collapsing
    /// would create a cycle — see the module docs for why that last one is a
    /// reachability test rather than a local one.
    pub fn derive(order: usize, links: &[Link], selected: &[usize]) -> Result<Self, Refusal> {
        let inside = membership(order, links, selected)?;
        if let Some(path) = bypass_path(order, links, &inside) {
            return Err(Refusal::Bypass { path });
        }
        Self::cut(order, links, selected)
    }

    /// The group's input sockets, ascending by producer.
    #[must_use]
    pub fn inputs(&self) -> &[InterfaceSocket] {
        &self.inputs
    }

    /// The group's output sockets, ascending by producer.
    #[must_use]
    pub fn outputs(&self) -> &[InterfaceSocket] {
        &self.outputs
    }

    /// Link indices wholly inside the selection: these move into the definition
    /// unchanged.
    #[must_use]
    pub fn internal(&self) -> &[usize] {
        &self.internal
    }

    /// Link indices wholly outside the selection: these are untouched by the
    /// collapse.
    #[must_use]
    pub fn outside(&self) -> &[usize] {
        &self.outside
    }

    /// How many of the caller's links cross the boundary.
    ///
    /// Never smaller than `inputs().len() + outputs().len()`, and larger exactly
    /// when some value crosses more than once — which is the whole reason the
    /// interface is keyed by producer rather than counted off the links.
    #[must_use]
    pub fn crossings(&self) -> usize {
        let carried =
            |sockets: &[InterfaceSocket]| -> usize { sockets.iter().map(|s| s.links.len()).sum() };
        carried(&self.inputs) + carried(&self.outputs)
    }
}

/// Why a selection cannot be collapsed into one vertex.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// Nothing was selected. A group of no vertices has no meaning — it is not
    /// an empty group, it is an absent one.
    Empty,
    /// A selection entry or a link endpoint is not a vertex of this graph.
    UnknownVertex {
        /// The index that is out of range.
        vertex: usize,
        /// The graph's vertex count, which the index must be below.
        order: usize,
    },
    /// A path leaves the selection and returns to it, so collapsing would make
    /// the new vertex reach itself.
    Bypass {
        /// The offending walk, starting and ending on a selected vertex with
        /// every interior vertex outside the selection. At least three long.
        path: Vec<usize>,
    },
}

impl Refusal {
    /// The bypassing walk when this is a [`Refusal::Bypass`], else `None`.
    ///
    /// Naming the path is the point: "this selection would be cyclic" leaves a
    /// user hunting for which of their wires said so.
    #[must_use]
    pub fn bypass(&self) -> Option<&[usize]> {
        match self {
            Self::Bypass { path } => Some(path),
            _ => None,
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("nothing is selected"),
            Self::UnknownVertex { vertex, order } => {
                write!(f, "vertex {vertex} is outside a graph of {order}")
            }
            Self::Bypass { path } => {
                f.write_str("a path leaves the selection and returns: ")?;
                for (i, v) in path.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" -> ")?;
                    }
                    write!(f, "{v}")?;
                }
                Ok(())
            }
        }
    }
}

/// Which vertices are inside the selection, with both index ranges checked.
///
/// Shared by [`Boundary::cut`] and [`Boundary::derive`] so the two cannot come
/// to different conclusions about what is in range.
fn membership(order: usize, links: &[Link], selected: &[usize]) -> Result<Vec<bool>, Refusal> {
    if selected.is_empty() {
        return Err(Refusal::Empty);
    }
    let mut inside = vec![false; order];
    for &v in selected {
        *inside
            .get_mut(v)
            .ok_or(Refusal::UnknownVertex { vertex: v, order })? = true;
    }
    for link in links {
        for end in [link.from.vertex, link.to.vertex] {
            if end >= order {
                return Err(Refusal::UnknownVertex { vertex: end, order });
            }
        }
    }
    Ok(inside)
}

/// Accumulates interface sockets keyed by their producing socket.
///
/// A `Vec` of `(key, socket)` rather than a map: an interface is a handful of
/// sockets, the result has to be canonically ordered anyway, and this keeps the
/// crate's zero-dependency, no-hashing shape.
#[derive(Default)]
struct Grouping {
    sockets: Vec<InterfaceSocket>,
}

impl Grouping {
    fn push(&mut self, link: &Link, index: usize) {
        if let Some(at) = self.sockets.iter().position(|s| s.producer == link.from) {
            self.sockets[at].consumers.push(link.to);
            self.sockets[at].links.push(index);
            return;
        }
        self.sockets.push(InterfaceSocket {
            producer: link.from,
            consumers: vec![link.to],
            links: vec![index],
        });
    }

    /// Canonical form: sockets ascending by producer, consumers ascending and
    /// deduplicated, links ascending.
    ///
    /// Canonicality is load-bearing rather than tidy — two callers holding the
    /// same graph with its links listed in different orders must derive the same
    /// interface, or a group's socket numbering would depend on the order its
    /// wires happened to be created in.
    fn finish(mut self) -> Vec<InterfaceSocket> {
        self.sockets.sort_by_key(|s| s.producer);
        for socket in &mut self.sockets {
            socket.consumers.sort_unstable();
            socket.consumers.dedup();
            socket.links.sort_unstable();
        }
        self.sockets
    }
}

/// A walk that leaves the selection and re-enters it, or `None` when none
/// exists.
///
/// Breadth-first from every vertex the selection reaches directly, through
/// unselected vertices only, stopping at the first edge back in — so the walk
/// reported is a shortest one, which is the one a user can follow.
fn bypass_path(order: usize, links: &[Link], inside: &[bool]) -> Option<Vec<usize>> {
    let mut out_adjacency: Vec<Vec<usize>> = vec![Vec::new(); order];
    for link in links {
        out_adjacency[link.from.vertex].push(link.to.vertex);
    }

    let mut predecessor = vec![usize::MAX; order];
    let mut seen = vec![false; order];
    let mut queue = VecDeque::new();
    for (from, targets) in out_adjacency.iter().enumerate() {
        if !inside[from] {
            continue;
        }
        for &to in targets {
            if !inside[to] && !seen[to] {
                seen[to] = true;
                predecessor[to] = from;
                queue.push_back(to);
            }
        }
    }

    while let Some(vertex) = queue.pop_front() {
        for &next in &out_adjacency[vertex] {
            if inside[next] {
                return Some(walk_back(next, vertex, &predecessor, inside));
            }
            if !seen[next] {
                seen[next] = true;
                predecessor[next] = vertex;
                queue.push_back(next);
            }
        }
    }
    None
}

/// Reconstruct `[entry, …, exit, re_entered]` from the predecessor chain.
fn walk_back(re_entered: usize, exit: usize, predecessor: &[usize], inside: &[bool]) -> Vec<usize> {
    let mut path = vec![re_entered];
    let mut cursor = exit;
    loop {
        path.push(cursor);
        // Every seeded vertex has a selected predecessor, so this terminates on
        // the vertex the walk left from.
        if inside[cursor] {
            break;
        }
        cursor = predecessor[cursor];
    }
    path.reverse();
    path
}

/// Whether one group definition may be placed inside another.
///
/// A definition that (transitively) contains an instance of itself has no
/// meaning: expanding it does not terminate. The relation is the caller's
/// `(host, inner)` pairs — "the definition numbered `host` contains an instance
/// of the definition numbered `inner`" — and the question is always asked about
/// a placement that has *not* happened yet.
pub struct Nesting;

impl Nesting {
    /// The containment chain that placing an instance of `definition` inside
    /// `host` would close, or `None` when the placement is legal.
    ///
    /// The chain runs `definition → … → host`; the placement being asked about
    /// is the edge that would join its ends. Placing a definition in itself
    /// yields the one-element chain `[definition]`.
    ///
    /// Naming the chain is what a user needs and what the DCC does not give:
    /// measured at `8cf50599`, `node_group_poll` reports the same flat sentence — "Nesting a node
    /// group inside of itself is not allowed" — for a direct self-nest and for
    /// one four groups deep, so the intermediate definitions that actually
    /// carry the recursion are never named.
    #[must_use]
    pub fn cycle(
        contains: &[(usize, usize)],
        host: usize,
        definition: usize,
    ) -> Option<Vec<usize>> {
        if host == definition {
            return Some(vec![definition]);
        }
        // Walk containment forwards from `definition`: if it reaches `host`,
        // then `host ⊃ definition` closes the loop.
        let mut predecessor = BTreeMap::new();
        let mut queue = VecDeque::from([definition]);
        let mut seen = BTreeSet::from([definition]);
        while let Some(current) = queue.pop_front() {
            for &(outer, inner) in contains.iter().filter(|(outer, _)| *outer == current) {
                debug_assert_eq!(outer, current);
                if inner == host {
                    let mut chain = vec![host, current];
                    let mut cursor = current;
                    while let Some(&previous) = predecessor.get(&cursor) {
                        chain.push(previous);
                        cursor = previous;
                    }
                    chain.reverse();
                    return Some(chain);
                }
                if seen.insert(inner) {
                    predecessor.insert(inner, current);
                    queue.push_back(inner);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Vec;

    /// Port 0 of `vertex` — most fixtures need only one port per vertex.
    const fn s(vertex: usize) -> Socket {
        Socket::new(vertex, 0)
    }

    fn link(from: Socket, to: Socket) -> Link {
        Link::new(from, to)
    }

    /// A chain `0 → 1 → 2 → 3` with one port per vertex.
    fn chain() -> Vec<Link> {
        vec![link(s(0), s(1)), link(s(1), s(2)), link(s(2), s(3))]
    }

    #[test]
    fn an_interior_selection_has_one_socket_each_way() {
        let boundary = Boundary::derive(4, &chain(), &[1, 2]).unwrap();
        assert_eq!(boundary.inputs().len(), 1);
        assert_eq!(boundary.inputs()[0].producer(), s(0));
        assert_eq!(boundary.inputs()[0].consumers(), &[s(1)]);
        assert_eq!(boundary.outputs().len(), 1);
        assert_eq!(boundary.outputs()[0].producer(), s(2));
        assert_eq!(boundary.outputs()[0].consumers(), &[s(3)]);
        assert_eq!(boundary.internal(), &[1]);
        assert_eq!(boundary.outside(), &[] as &[usize]);
    }

    #[test]
    fn the_four_classes_partition_every_link() {
        let links = vec![
            link(s(0), s(1)), // in
            link(s(1), s(2)), // internal
            link(s(2), s(3)), // out
            link(s(4), s(5)), // outside
        ];
        let boundary = Boundary::derive(6, &links, &[1, 2]).unwrap();
        let mut covered: Vec<usize> = boundary
            .inputs()
            .iter()
            .chain(boundary.outputs())
            .flat_map(|socket| socket.links().iter().copied())
            .chain(boundary.internal().iter().copied())
            .chain(boundary.outside().iter().copied())
            .collect();
        covered.sort_unstable();
        assert_eq!(covered, vec![0, 1, 2, 3]);
    }

    #[test]
    fn one_external_producer_feeding_two_selected_nodes_is_one_socket() {
        // 0 feeds both 1 and 2; {1,2} therefore takes ONE group input.
        let links = vec![link(s(0), s(1)), link(s(0), Socket::new(2, 0))];
        let boundary = Boundary::derive(3, &links, &[1, 2]).unwrap();
        assert_eq!(boundary.inputs().len(), 1);
        assert_eq!(boundary.inputs()[0].producer(), s(0));
        assert_eq!(boundary.inputs()[0].consumers(), &[s(1), s(2)]);
        assert_eq!(boundary.inputs()[0].links(), &[0, 1]);
        // Two links carry the one value.
        assert_eq!(boundary.crossings(), 2);
    }

    #[test]
    fn two_external_producers_into_one_selected_node_are_two_sockets() {
        // The mirror of the case above: distinct values, distinct sockets, even
        // though they land on the same selected vertex.
        let links = vec![link(s(0), Socket::new(2, 0)), link(s(1), Socket::new(2, 1))];
        let boundary = Boundary::derive(3, &links, &[2]).unwrap();
        assert_eq!(boundary.inputs().len(), 2);
        assert_eq!(boundary.inputs()[0].producer(), s(0));
        assert_eq!(boundary.inputs()[1].producer(), s(1));
    }

    #[test]
    fn one_selected_producer_feeding_two_outsiders_is_one_socket() {
        let links = vec![link(s(0), s(1)), link(s(0), Socket::new(2, 0))];
        let boundary = Boundary::derive(3, &links, &[0]).unwrap();
        assert_eq!(boundary.outputs().len(), 1);
        assert_eq!(boundary.outputs()[0].producer(), s(0));
        assert_eq!(boundary.outputs()[0].consumers(), &[s(1), s(2)]);
        assert!(boundary.inputs().is_empty());
    }

    #[test]
    fn two_ports_on_one_selected_vertex_are_two_output_sockets() {
        let links = vec![
            link(Socket::new(0, 0), s(1)),
            link(Socket::new(0, 1), Socket::new(2, 0)),
        ];
        let boundary = Boundary::derive(3, &links, &[0]).unwrap();
        assert_eq!(boundary.outputs().len(), 2);
        assert_eq!(boundary.outputs()[0].producer(), Socket::new(0, 0));
        assert_eq!(boundary.outputs()[1].producer(), Socket::new(0, 1));
    }

    #[test]
    fn the_interface_does_not_depend_on_the_order_the_links_were_listed() {
        let forwards = vec![
            link(s(0), s(2)),
            link(s(1), Socket::new(2, 1)),
            link(Socket::new(2, 0), s(3)),
        ];
        let mut backwards = forwards.clone();
        backwards.reverse();
        let a = Boundary::derive(4, &forwards, &[2]).unwrap();
        let b = Boundary::derive(4, &backwards, &[2]).unwrap();
        let producers = |boundary: &Boundary| -> Vec<Socket> {
            boundary
                .inputs()
                .iter()
                .chain(boundary.outputs())
                .map(InterfaceSocket::producer)
                .collect()
        };
        assert_eq!(producers(&a), producers(&b));
    }

    #[test]
    fn an_unconnected_selection_has_no_interface() {
        let boundary = Boundary::derive(3, &[], &[0, 1]).unwrap();
        assert!(boundary.inputs().is_empty());
        assert!(boundary.outputs().is_empty());
        assert_eq!(boundary.crossings(), 0);
    }

    #[test]
    fn a_one_hop_bypass_is_refused_and_named() {
        // 0 → 1 → 2 and 0 → ... the classic: unselected 1 has both an input
        // from the selection and an output to it. This is the case the DCC
        // catches.
        let links = vec![link(s(0), s(1)), link(s(1), s(2))];
        let refusal = Boundary::derive(3, &links, &[0, 2]).unwrap_err();
        assert_eq!(refusal.bypass(), Some(&[0, 1, 2][..]));
    }

    #[test]
    fn a_two_hop_bypass_is_refused_where_blenders_local_test_would_pass() {
        // 0 → 1 → 2 → 3, group {0, 3}. NO unselected vertex has both an input
        // from and an output to the selection: 1 has only the input, 2 only
        // the output. The DCC's `node_group_make_test_selected` therefore accepts this and creates a group
        // that reaches itself; reachability refuses it.
        let links = chain();
        let inside = vec![true, false, false, true];
        assert!(
            blender_would_accept(&links, &inside),
            "the fixture must be one Blender's one-hop rule ACCEPTS, \
             or it proves nothing about the difference"
        );
        let refusal = Boundary::derive(4, &links, &[0, 3]).unwrap_err();
        assert_eq!(refusal.bypass(), Some(&[0, 1, 2, 3][..]));
    }

    /// The DCC's rule at `8cf50599`, re-expressed over this module's types, and
    /// answering `true` when it would ALLOW the collapse: no unselected vertex
    /// may have both an input from the selection and an output to it.
    ///
    /// Present so the divergence above is asserted rather than described — a
    /// test that only checked our own answer could not tell a stricter rule from
    /// an equal one. It earns its keep immediately: it caught this very
    /// assertion being written the wrong way round.
    fn blender_would_accept(links: &[Link], inside: &[bool]) -> bool {
        (0..inside.len()).all(|vertex| {
            if inside[vertex] {
                return true;
            }
            let fed_by_selection = links
                .iter()
                .any(|l| l.to.vertex == vertex && inside[l.from.vertex]);
            let feeds_selection = links
                .iter()
                .any(|l| l.from.vertex == vertex && inside[l.to.vertex]);
            !(fed_by_selection && feeds_selection)
        })
    }

    #[test]
    fn a_cut_partitions_a_selection_a_collapse_would_refuse() {
        // The very fixture `derive` refuses. Copying {0, 3} out of 0 → 1 → 2 → 3
        // is perfectly legal — the crossings are severed, not routed through a
        // new vertex — so the partition must be available without the legality.
        let links = chain();
        assert!(Boundary::derive(4, &links, &[0, 3]).is_err());
        let cut = Boundary::cut(4, &links, &[0, 3]).unwrap();
        assert_eq!(cut.outputs().len(), 1, "0 → 1 leaves the selection");
        assert_eq!(cut.outputs()[0].producer(), s(0));
        assert_eq!(cut.inputs().len(), 1, "2 → 3 enters it");
        assert_eq!(cut.inputs()[0].producer(), s(2));
        assert_eq!(cut.internal(), &[] as &[usize]);
        assert_eq!(cut.outside(), &[1]);
    }

    #[test]
    fn a_cut_and_a_derive_agree_whenever_the_derive_succeeds() {
        // The split must not have changed what a collapse sees. Same partition,
        // every time both answer.
        let links = vec![
            link(s(0), s(1)),
            link(s(1), s(2)),
            link(s(2), s(3)),
            link(s(4), s(5)),
        ];
        for selection in [
            &[1][..],
            &[1, 2][..],
            &[2, 3][..],
            &[0, 1, 2, 3][..],
            &[4][..],
        ] {
            let derived = Boundary::derive(6, &links, selection).unwrap();
            let cut = Boundary::cut(6, &links, selection).unwrap();
            assert_eq!(derived, cut, "selection {selection:?}");
        }
    }

    #[test]
    fn a_cut_still_refuses_what_is_not_a_selection_at_all() {
        assert_eq!(Boundary::cut(3, &[], &[]).unwrap_err(), Refusal::Empty);
        assert_eq!(
            Boundary::cut(2, &[], &[5]).unwrap_err(),
            Refusal::UnknownVertex {
                vertex: 5,
                order: 2
            }
        );
    }

    #[test]
    fn a_long_bypass_reports_a_shortest_walk() {
        // Two ways back in: 0→1→5 and 0→2→3→4→5. The short one is reported.
        let links = vec![
            link(s(0), Socket::new(2, 0)),
            link(s(2), Socket::new(3, 0)),
            link(s(3), Socket::new(4, 0)),
            link(s(4), s(5)),
            link(s(0), s(1)),
            link(s(1), s(5)),
        ];
        let refusal = Boundary::derive(6, &links, &[0, 5]).unwrap_err();
        assert_eq!(refusal.bypass(), Some(&[0, 1, 5][..]));
    }

    #[test]
    fn a_path_that_only_leaves_is_not_a_bypass() {
        let links = chain();
        // {0, 1} reaches 2 and 3 and never comes back.
        assert!(Boundary::derive(4, &links, &[0, 1]).is_ok());
    }

    #[test]
    fn a_cycle_wholly_inside_the_selection_is_not_created_by_the_collapse() {
        // The collapse cannot be blamed for a cycle that was already there and
        // is entirely internal — nothing crosses, so nothing is refused.
        let links = vec![link(s(0), s(1)), link(s(1), s(0))];
        let boundary = Boundary::derive(2, &links, &[0, 1]).unwrap();
        assert_eq!(boundary.internal(), &[0, 1]);
    }

    #[test]
    fn an_empty_selection_is_refused() {
        assert_eq!(Boundary::derive(3, &[], &[]).unwrap_err(), Refusal::Empty);
    }

    #[test]
    fn an_out_of_range_selection_is_refused() {
        assert_eq!(
            Boundary::derive(2, &[], &[5]).unwrap_err(),
            Refusal::UnknownVertex {
                vertex: 5,
                order: 2
            }
        );
    }

    #[test]
    fn an_out_of_range_link_endpoint_is_refused() {
        let links = vec![link(s(0), s(9))];
        assert_eq!(
            Boundary::derive(2, &links, &[0]).unwrap_err(),
            Refusal::UnknownVertex {
                vertex: 9,
                order: 2
            }
        );
    }

    #[test]
    fn a_repeated_selection_entry_is_a_set_not_a_multiset() {
        let plain = Boundary::derive(4, &chain(), &[1, 2]).unwrap();
        let repeated = Boundary::derive(4, &chain(), &[2, 1, 2, 1]).unwrap();
        assert_eq!(plain, repeated);
    }

    #[test]
    fn a_refusal_says_which_wires_it_means() {
        let refusal = Boundary::derive(4, &chain(), &[0, 3]).unwrap_err();
        assert_eq!(
            format!("{refusal}"),
            "a path leaves the selection and returns: 0 -> 1 -> 2 -> 3"
        );
    }

    #[test]
    fn nesting_a_definition_in_itself_is_refused() {
        assert_eq!(Nesting::cycle(&[], 3, 3), Some(vec![3]));
    }

    #[test]
    fn nesting_names_the_whole_chain() {
        // 1 contains 2, 2 contains 3. Putting 1 inside 3 closes 1 → 2 → 3 → 1.
        let contains = [(1, 2), (2, 3)];
        assert_eq!(Nesting::cycle(&contains, 3, 1), Some(vec![1, 2, 3]));
    }

    #[test]
    fn an_unrelated_definition_nests_freely() {
        let contains = [(1, 2), (2, 3)];
        assert_eq!(Nesting::cycle(&contains, 1, 4), None);
        // Deeper is still not a cycle: 3 may contain 4.
        assert_eq!(Nesting::cycle(&contains, 3, 4), None);
    }

    #[test]
    fn a_diamond_of_definitions_is_not_a_cycle() {
        // 1 contains 2 and 3, both of which contain 4. Nothing reaches 1.
        let contains = [(1, 2), (1, 3), (2, 4), (3, 4)];
        assert_eq!(Nesting::cycle(&contains, 2, 4), None);
        assert_eq!(Nesting::cycle(&contains, 4, 1), Some(vec![1, 2, 4]));
    }

    #[test]
    fn nesting_terminates_on_a_containment_relation_that_is_already_cyclic() {
        // Not reachable through this module's own API, but a caller holding a
        // corrupt relation must not hang.
        let contains = [(1, 2), (2, 1)];
        assert!(Nesting::cycle(&contains, 5, 1).is_none());
    }
}
