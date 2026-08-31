//! ★★★★★ R1934 — **a bend in a wire**: the node that carries what crosses it
//! and means nothing of its own.
//!
//! [`NodeBody::Reroute`] is the body; this module is
//! the two derivations it needs and the verb that puts one on a wire.
//!
//! # Why the flow is derived and not stored
//!
//! A reroute's ports are whatever the graph around it decided. Both references
//! store that answer on the node and then run machinery to keep the stored copy
//! agreeing with the links:
//!
//! * the DCC stores a socket type in the node (`NodeReroute::type_idname`) and
//!   runs a whole-tree pass after every update — a disjoint-set union over
//!   every reroute in the tree, so that a *chain* of them settles on one type;
//! * the engine stores it on the two pins and propagates recursively on every
//!   connection change, with a boolean recursion guard whose own comment says
//!   it is there "to prevent `PropagatePinType` from infinitely recursing if
//!   you manage to create a loop of knots".
//!
//! Deriving it makes both unnecessary: [`Document::passing_flow`] answers from
//! the links that exist *now*, so there is no stored copy to disagree with them
//! and no pass to forget to run. The recursion guard becomes a visited set,
//! which is a property of the walk rather than a field on the node.
//!
//! # The rule, measured on both references
//!
//! They agree, and reach it by that different machinery:
//!
//! 1. reroutes wired to one another form a **chain**, and a chain carries one
//!    flow;
//! 2. the **source** side wins — a flow arriving at the chain decides it, and
//!    the sink side decides it only when no source does;
//! 3. with several candidates on the winning side, the choice is **ordered**,
//!    so the answer does not depend on link insertion order (the DCC picks the
//!    least `(node index, socket index)`; this crate picks the least
//!    `(NodeId, port index)`, which is the same rule over this crate's own
//!    stable identities);
//! 4. attached to nothing, the chain is [`Flow::Undecided`].
//!
//! ★ Point 4 is the one deliberate divergence, and it is the stricter reading:
//! the DCC keeps the last type it stored (a fresh reroute is a *colour*), so a
//! reroute attached to nothing still claims a type nothing in the graph
//! supports. The engine reverts to wildcard, which is this.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    Document, Flow, LinkId, NodeBody, NodeId, NodeKind, Port, Side, Signature, Socket, TreeId,
};

/// The flow a reroute inherits, **stripped of the resting value** the port it
/// was read from carried.
///
/// A [`Flow::Value`]'s `default` is the value that port holds when nothing is
/// wired to it — a fact about *that* port, authored on the node declaring it.
/// A reroute holds nothing: it carries what arrives. Copying the default across
/// would give a reroute a resting value nobody authored and, worse, one that
/// changes when the far end of the chain is rewired.
///
/// ★ Measured on the DCC rather than reasoned: its own socket-value routine
/// returns early for a link with a reroute at either end, saying "reroute node
/// can't have ownership of socket value directly".
fn resting<T, V>(flow: Flow<T, V>) -> Flow<T, V> {
    match flow {
        Flow::Value { ty, default: _ } => Flow::Value { ty, default: None },
        other => other,
    }
}

/// ★★★★★ R1934 — **this node is a point a wire passes through**, and these are
/// the two ends of it.
///
/// The answer to the engine's `ShouldDrawNodeAsControlPointOnly`, whose name
/// says *draw* and whose seven call sites do no drawing at all. Measured across
/// every one of them, what they use it for is:
///
/// * **which end to take** — three sites, all of them a drag: a node spawned
///   under a dragged wire resumes the drag from the end facing the other way; a
///   drag hovering the point picks the end that maximises the chance of a legal
///   connection; a drag that has not committed to a direction picks the end the
///   pointer is heading towards;
/// * **passing through** — hovering a wire spreads the highlight along the
///   chain, recursing across each point;
/// * **not an address** — node alignment skips these pins, because a point on a
///   wire is not where a wire is *going*;
/// * **a precondition** — the control-point widget asserts it in its
///   constructor.
///
/// So the capability is "a wire passes through here", and the two indices are
/// what makes it usable: without them a caller knows a node is transparent and
/// still cannot say which port is the way in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Passing {
    /// The index of the port a wire arrives at.
    pub inbound: u32,
    /// The index of the port it leaves by.
    pub outbound: u32,
}

impl Passing {
    /// The two ends of the ordinary shape: port 0 in, port 0 out.
    ///
    /// The reference's three overriders all answer a constant pair, and all
    /// three answer `0, 1` — because there the two are indices into **one**
    /// array holding both directions. Ports here are addressed per side, so the
    /// same shape is `0, 0`.
    pub const ENDS: Self = Self {
        inbound: 0,
        outbound: 0,
    };

    /// The end on `side`.
    ///
    /// The question every one of the reference's drag sites asks, written once:
    /// each of them indexes the pair by hand.
    ///
    /// ⚠ Measured, and the reason this is worth a method: at one of those sites
    /// the two locals are declared `OutPinIndex, InPinIndex` and passed, in that
    /// order, to a parameter list declared `OutInputPinIndex,
    /// OutOutputPinIndex`. So the variable called `OutPinIndex` receives the
    /// INPUT index. Whether the site's later use compensates is not asserted
    /// here — what is asserted is that reading the pair by hand puts a name and
    /// a meaning in opposite places, which is what asking `end(side)` removes.
    #[must_use]
    pub const fn end(self, side: Side) -> u32 {
        match side {
            Side::Input => self.inbound,
            Side::Output => self.outbound,
        }
    }
}

/// What [`Document::insert_reroutes`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rerouted {
    /// The reroute nodes made, ascending. **One per source socket**, not one
    /// per cut link — see [`Document::insert_reroutes`].
    pub made: Vec<NodeId>,
    /// The links made to feed them, ascending: one per reroute.
    pub feeds: Vec<LinkId>,
    /// The cut links, which were **kept** and re-pointed at a reroute rather
    /// than removed and remade — so a caller holding a [`LinkId`] still holds
    /// the same link, and an undo has one thing to put back.
    pub rerouted: Vec<LinkId>,
}

/// Why a reroute could not be inserted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RerouteError {
    /// The tree is not in this document.
    NoSuchTree(TreeId),
    /// One of the named links is not in that tree.
    NoSuchLink(LinkId),
    /// No link was named. Refused rather than answering an empty
    /// [`Rerouted`], because a gesture that cut nothing is a gesture that
    /// should leave no undo entry.
    NothingCut,
}

impl std::fmt::Display for RerouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchLink(link) => write!(f, "no link {}", link.0),
            Self::NothingCut => write!(f, "nothing was cut"),
        }
    }
}

impl std::error::Error for RerouteError {}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1934, widened R1935 — **what a transparent node's ports carry**,
    /// derived from the chain it belongs to.
    ///
    /// [`Flow::Undecided`] when nothing in the chain touches a decided port.
    /// See the module header for the rule and for what each reference does
    /// instead.
    ///
    /// Answers `Undecided` for a node that is not a transparent one as well —
    /// asking what a chain carries when there is no chain has no better answer,
    /// and every caller in this crate reaches it through the signature.
    ///
    /// ★ R1935 renamed this from `reroute_flow`, because the chain stopped
    /// being made only of reroutes: a [`NodeBody::Beacon`] and its
    /// [`NodeBody::Echo`]s are on it too, joined by name rather than by link
    /// (see `passing_chain`). A name that had become
    /// half-false is worth the rename — this crate has paid for the other
    /// choice repeatedly.
    #[must_use]
    pub fn passing_flow(&self, tree: TreeId, node: NodeId) -> Flow<K::Type, K::Value> {
        let Some(host) = self.tree(tree) else {
            return Flow::Undecided;
        };
        let chain = self.passing_chain(tree, node);
        // Ordered maps, so the winner is the least address rather than
        // whichever link happens to be earliest in the tree's link list. That
        // is the DCC's `RerouteTargetPriority` rule over this crate's own
        // stable identities.
        let mut from_source: BTreeMap<(NodeId, u32), Flow<K::Type, K::Value>> = BTreeMap::new();
        let mut from_sink: BTreeMap<(NodeId, u32), Flow<K::Type, K::Value>> = BTreeMap::new();
        for link in host.links() {
            // A link INTO the chain from outside decides it as a source; a link
            // OUT of the chain to outside decides it as a sink. A link with
            // both ends in the chain decides nothing — that is the union step.
            let into_chain = chain.contains(&link.to.node) && !chain.contains(&link.from.node);
            let out_of_chain = chain.contains(&link.from.node) && !chain.contains(&link.to.node);
            if into_chain {
                if let Some(flow) = self.port_flow(tree, link.from, Side::Output) {
                    from_source.insert((link.from.node, link.from.port), flow);
                }
            } else if out_of_chain {
                if let Some(flow) = self.port_flow(tree, link.to, Side::Input) {
                    from_sink.insert((link.to.node, link.to.port), flow);
                }
            }
        }
        // The source side wins. Both references reach for it first and fall
        // back to the sink only when it answers nothing.
        from_source
            .into_values()
            .next()
            .or_else(|| from_sink.into_values().next())
            .map_or(Flow::Undecided, resting)
    }

    /// Every transparent node reachable from `node` without leaving them,
    /// `node` included when it is one.
    ///
    /// The visited set is what the engine needs a recursion guard field for: a
    /// loop of them terminates here because a node already in the set is not
    /// walked again.
    ///
    /// # ★★★★★ R1935 — a chain is joined by LINKS *and* by NAMES
    ///
    /// R1934 walked links between reroutes. A [`NodeBody::Beacon`] and its
    /// [`NodeBody::Echo`]s are transparent in exactly the same way and are
    /// joined by neither a link nor a wire — the whole point of them is that a
    /// value crosses the canvas with no edge — so the walk steps across that
    /// naming too.
    ///
    /// ⚠ **This is not a nicety, it is what keeps the derivation finite.**
    /// Were the two kinds of chain separate, a beacon wired to a reroute would
    /// make each ask the other for its flow: `passing_flow(reroute)` reads the
    /// beacon's port, which resolves the beacon's signature, which derives the
    /// beacon's flow, which reads the reroute's port… One chain covering both
    /// makes that pair a single connected component with a single answer, so
    /// the recursion has nowhere to happen. It is the same argument R1934 made
    /// for deriving rather than storing, one level up.
    fn passing_chain(&self, tree: TreeId, node: NodeId) -> BTreeSet<NodeId> {
        let mut chain = BTreeSet::new();
        let Some(host) = self.tree(tree) else {
            return chain;
        };
        let transparent = |id: NodeId| {
            matches!(
                host.node(id).map(|n| &n.body),
                Some(NodeBody::Reroute | NodeBody::Beacon | NodeBody::Echo(_))
            )
        };
        if !transparent(node) {
            return chain;
        }
        let mut pending = vec![node];
        while let Some(here) = pending.pop() {
            if !chain.insert(here) {
                continue;
            }
            let step_to = |next: NodeId, chain: &BTreeSet<NodeId>, pending: &mut Vec<NodeId>| {
                if transparent(next) && !chain.contains(&next) {
                    pending.push(next);
                }
            };
            for link in host.links() {
                let step = if link.from.node == here {
                    link.to.node
                } else if link.to.node == here {
                    link.from.node
                } else {
                    continue;
                };
                step_to(step, &chain, &mut pending);
            }
            // The naming steps, both directions: an echo reaches its beacon,
            // and a beacon reaches every echo of it. A dangling echo names a
            // node that is gone and simply steps nowhere.
            match host.node(here).map(|n| &n.body) {
                Some(NodeBody::Echo(beacon)) => step_to(*beacon, &chain, &mut pending),
                Some(NodeBody::Beacon) => {
                    for other in host.nodes() {
                        if matches!(other.body, NodeBody::Echo(named) if named == here) {
                            step_to(other.id, &chain, &mut pending);
                        }
                    }
                }
                _ => {}
            }
        }
        chain
    }

    /// The flow of one socket's port, resolved through the signature.
    fn port_flow(
        &self,
        tree: TreeId,
        socket: Socket,
        side: Side,
    ) -> Option<Flow<K::Type, K::Value>> {
        let signature = self.signature(tree, socket.node)?;
        let ports = match side {
            Side::Input => signature.inputs,
            Side::Output => signature.outputs,
        };
        ports
            .get(socket.port as usize)
            .map(|port| port.flow.clone())
    }

    /// ★★★★★ R1934 — **the signature of a node a wire passes through**: one
    /// port in, one out, both carrying what
    /// [`passing_flow`](Self::passing_flow) derived.
    ///
    /// R1935 — a [`NodeBody::Beacon`] has exactly this shape too, and for the
    /// same reason, so it reads the same derivation rather than a second copy
    /// of it. An [`NodeBody::Echo`] does NOT: it has no way in, which is what
    /// makes the value's crossing edgeless, so it gets
    /// [`echo_signature`](Self::echo_signature) — the OUTPUT half of this one.
    pub(crate) fn passing_signature(&self, tree: TreeId, node: NodeId) -> Signature<K> {
        let flow = self.passing_flow(tree, node);
        Signature {
            inputs: vec![Port::with_flow("In", flow.clone())],
            outputs: vec![Port::with_flow("Out", flow)],
        }
    }

    /// ★★★★★ R1935 — **the signature of an echo**: one output, nothing in.
    ///
    /// The output carries what the whole chain carries, which is the beacon's
    /// answer reached by name. Deliberately not `passing_signature` with the
    /// input dropped afterwards: the shape difference IS the capability, and a
    /// caller that built one and trimmed it would be one edit away from an
    /// echo a wire could be run into.
    pub(crate) fn echo_signature(&self, tree: TreeId, node: NodeId) -> Signature<K> {
        Signature {
            inputs: Vec::new(),
            outputs: vec![Port::with_flow("Out", self.passing_flow(tree, node))],
        }
    }

    /// ★★★★★ R1934 — **is a wire passing through this node**, and by which two
    /// ports?
    ///
    /// `None` for a node a wire does not pass through. Two sources answer:
    ///
    /// * [`NodeBody::Reroute`] always passes — it is the body that exists for
    ///   nothing else;
    /// * an application kind may declare that it does, through
    ///   [`NodeKind::passing`]. The engine needs this
    ///   for the same reason: one of its three overriders is a *dataflow* node
    ///   class that answers by looking at which dataflow node it is holding,
    ///   so the answer cannot live in the editor's node taxonomy alone.
    #[must_use]
    pub fn passing(&self, tree: TreeId, node: NodeId) -> Option<Passing> {
        let held = self.tree(tree)?.node(node)?;
        match &held.body {
            // R1935 — a beacon has the same two ends as a bend and is drawn as
            // an ordinary card; what makes it different is that its OTHER end
            // is reachable by name, which is not what this question asks.
            NodeBody::Reroute | NodeBody::Beacon => Some(Passing::ENDS),
            // ⚠ And an echo answers `None` even though it is on the same chain
            // and carries the same flow. That is not an oversight: this
            // question is "which two PORTS are the way in and the way out", and
            // an echo has no way in — the value reaches it by name. A caller
            // handed `Passing::ENDS` here would index an input port that is not
            // there. Being on a chain and having two ends are two facts, and
            // R1935 is where they stopped coinciding.
            // ★ Enumerated rather than swept into a wildcard, and clippy asking
            // for the wildcard back is what made the choice explicit: an echo's
            // `None` is a DECISION this round had to make, and a body that
            // answers it by falling through is a body nobody decided about.
            // Listing every arm also puts the compiler back in the position of
            // asking the next round what a new body answers here.
            NodeBody::Echo(_)
            | NodeBody::Frame
            | NodeBody::Group(_)
            | NodeBody::Interface(_)
            | NodeBody::Delay(_) => None,
            NodeBody::Kind(kind) => kind.passing(),
        }
    }

    /// ★★★★★ R1934 — **put a reroute on each of these wires.**
    ///
    /// The verb the DCC reaches by drawing a line across a canvas: its operator
    /// takes the polyline the pointer drew, intersects it with every drawable
    /// link, and inserts reroutes at the crossings. The geometry is the
    /// screen's — a wire's drawn curve is not a fact this crate holds — so what
    /// arrives here is the **result** of that question: which links were cut,
    /// and where each crossing was.
    ///
    /// Four behaviours are measured from that operator and reproduced, and the
    /// first is the one a caller would not guess:
    ///
    /// 1. **One reroute per source socket, not per cut link.** Cutting a fan-out
    ///    of three links leaving one output makes ONE reroute that all three
    ///    then leave from, which is what makes the gesture useful for tidying:
    ///    the DCC's own comment is that "deduplicating new reroutes per output
    ///    socket is useful because it allows reusing reroutes for connected
    ///    intersections".
    /// 2. **The cut links are kept and re-pointed**, not deleted and remade —
    ///    the operator assigns `link->fromnode = reroute`. So a caller holding a
    ///    [`LinkId`] still holds it afterwards.
    /// 3. **The feeding link is muted exactly when every cut link was**, so a
    ///    muted branch stays muted end to end.
    /// 4. **The reroute lands at the average of its own crossings**, and joins
    ///    the frame the wires it replaces were already inside.
    ///
    /// ★ Point 4's second half is where this deliberately does not copy the
    /// operator's mechanism. The DCC hit-tests the landing point against every
    /// frame's *drawn bounds*, which is a fact about the screen: a frame's
    /// extent here is whatever the renderer gives it, so this crate cannot ask
    /// that question and a version that took the rectangle as an argument would
    /// be asking its caller to re-derive containment. The model's own reading
    /// of "inside the frame the wire was in" is
    /// [`common_frame`](Self::common_frame) over the cut wires' endpoints,
    /// which is the same answer whenever the frames are drawn around what they
    /// contain — and unlike the hit test it cannot put the reroute in a frame
    /// neither end of the wire belongs to.
    ///
    /// Refuses [`RerouteError::NothingCut`] on an empty cut list: a gesture that
    /// crossed no wire should leave nothing behind, including an undo entry.
    ///
    /// # Errors
    ///
    /// [`RerouteError`] when the tree or one of the links is not there, or when
    /// nothing was cut.
    pub fn insert_reroutes(
        &mut self,
        tree: TreeId,
        cuts: &[(LinkId, i32, i32)],
    ) -> Result<Rerouted, RerouteError> {
        if cuts.is_empty() {
            return Err(RerouteError::NothingCut);
        }
        let host = self.tree(tree).ok_or(RerouteError::NoSuchTree(tree))?;
        // Group by SOURCE socket, keeping every group's cut points, and refuse
        // the whole gesture if any named link is absent — a partial insertion
        // would leave a canvas nobody asked for.
        let mut by_source: BTreeMap<Socket, Vec<(LinkId, i32, i32)>> = BTreeMap::new();
        let mut touched: BTreeMap<Socket, Vec<NodeId>> = BTreeMap::new();
        for &(id, x, y) in cuts {
            let link = host.link(id).copied().ok_or(RerouteError::NoSuchLink(id))?;
            by_source.entry(link.from).or_default().push((id, x, y));
            let ends = touched.entry(link.from).or_default();
            ends.push(link.from.node);
            ends.push(link.to.node);
        }

        let mut made = Vec::new();
        let mut feeds = Vec::new();
        let mut rerouted = Vec::new();
        for (source, group) in by_source {
            let count = i32::try_from(group.len()).unwrap_or(1).max(1);
            let x = group.iter().map(|&(_, x, _)| x).sum::<i32>() / count;
            let y = group.iter().map(|&(_, _, y)| y).sum::<i32>() / count;
            // 3 — read before anything moves: the feed is muted exactly when
            // every cut link was.
            let all_muted = group.iter().all(|&(id, _, _)| {
                self.tree(tree)
                    .and_then(|host| host.link(id))
                    .is_some_and(|link| link.muted)
            });
            // 4 — the frame the replaced wires were already inside.
            let frame = touched
                .get(&source)
                .and_then(|ends| self.common_frame(tree, ends));
            let reroute = self
                .add_node(tree, NodeBody::Reroute, x, y)
                .map_err(|_| RerouteError::NoSuchTree(tree))?;
            let feed = self.push_link(tree, source, Socket::new(reroute, 0), all_muted);
            // 2 — the cut links are re-pointed, keeping their identity.
            let out = Socket::new(reroute, 0);
            if let Some(host) = self.tree_mut(tree) {
                for &(id, _, _) in &group {
                    if let Some(link) = host.link_mut(id) {
                        link.from = out;
                        rerouted.push(id);
                    }
                }
            }
            if let Some(frame) = frame {
                let _ = self.set_parent(tree, reroute, Some(frame));
            }
            made.push(reroute);
            feeds.push(feed);
        }
        made.sort_unstable();
        feeds.sort_unstable();
        rerouted.sort_unstable();
        Ok(Rerouted {
            made,
            feeds,
            rerouted,
        })
    }
}
