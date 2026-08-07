//! R1586 — what would flow through a node that is not doing its job.
//!
//! One derivation, two consumers. A node can be taken out of the graph's
//! meaning without being taken out of the graph ([`Document::set_bypassed`]), or
//! taken out of the structure altogether ([`Document::dissolve`]) — and the
//! *same* routing decides both, so "what does bypassing this do" and "what will
//! deleting it leave behind" cannot give different answers. Blender unifies
//! these too, and says so in its own operator description: "Remove nodes and
//! reconnect nodes **as if deletion was muted**". What is chosen differently
//! here is the rule underneath, and what is reported when it cannot be applied.
//!
//! # The rule
//!
//! **A bypassed node is the identity, as far as its signature allows.**
//!
//! Output `n` takes its value from input `n` when their types agree; failing
//! that, from the lowest-indexed input of the right type; failing that, from
//! nothing, and the output is *named* as dropped. That is the whole of it, and
//! it is a function of the node's signature alone.
//!
//! Blender instead scores every input against every output through a static
//! table of socket-type pairs (`get_internal_link_type_priority`) and breaks
//! ties by **whether the input happens to be wired**. That last clause is the
//! reason the rule is worth restating: under it, unplugging one port can change
//! which value comes out of a *different* port of the same bypassed node. A
//! `Mix(Base, Blend, Factor)` with only `Blend` wired passes `Blend` through;
//! wire `Base` as well and the same node now passes `Base`. Here the answer to
//! "what does bypassing this node do" is stable under every edit that does not
//! change the node's own signature — and `Base` either way, because it is
//! first.
//!
//! # Where the two consumers must differ, and where that is said
//!
//! A bypassed node passes its routed input's value on even when nothing is
//! wired to that input, because an unwired port still has its declared default.
//! A *dissolved* node cannot: there is no link to redirect, so the downstream
//! link is removed and the port it fed falls back to its own default instead.
//! The two therefore agree on every value whenever the routed inputs are wired,
//! and the places they cannot agree are exactly the links reported in
//! [`Rewired::severed`]. Blender removes the same links and reports nothing at
//! all — `node_internal_relink` returns `void`.

use crate::model::{Document, EditError, Link, LinkId, NodeId, NodeKind, Socket, TreeId};

/// One value's way through a node that is not computing: which input the value
/// arrives on, and which output it leaves by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Route {
    /// The output port the value leaves by.
    pub output: u32,
    /// The input port it came in on.
    pub input: u32,
}

impl Route {
    /// Whether this route is the identity: the value leaves by the port it
    /// arrived on.
    #[must_use]
    pub const fn is_identity(self) -> bool {
        self.output == self.input
    }
}

/// What a node would pass through if it were not computing.
///
/// Derived from the node's signature at the moment it is asked for. There is no
/// stored copy to go stale — Blender materialises this into
/// `node->runtime->internal_links` and keeps a tree-update pass whose job is to
/// notice when the stored answer has stopped matching the derived one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Passthrough {
    routes: Vec<Route>,
    dropped_outputs: Vec<u32>,
    unreached_inputs: Vec<u32>,
}

impl Passthrough {
    /// Every routing, ascending by output port.
    #[must_use]
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    /// Outputs no input can feed, because the node declares no input of that
    /// type. While the node is bypassed these carry nothing, and whatever they
    /// fed falls back to its own default.
    ///
    /// Named rather than merely absent from [`Self::routes`]: a value
    /// disappearing is the thing an author most needs told, and it is precisely
    /// what Blender's derivation drops on the floor.
    #[must_use]
    pub fn dropped_outputs(&self) -> &[u32] {
        &self.dropped_outputs
    }

    /// Inputs that reach no output at all. Whatever is wired to these is not
    /// consulted while the node is bypassed.
    #[must_use]
    pub fn unreached_inputs(&self) -> &[u32] {
        &self.unreached_inputs
    }

    /// Which input feeds `output`, if any.
    #[must_use]
    pub fn source_of(&self, output: u32) -> Option<u32> {
        self.routes
            .iter()
            .find(|r| r.output == output)
            .map(|r| r.input)
    }

    /// Whether the node passes through as a plain identity: every output routed
    /// from the input at its own index, and nothing dropped.
    ///
    /// True for the shape a filter node has — same arity, same types in and out
    /// — which is the shape most worth bypassing.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.dropped_outputs.is_empty() && self.routes.iter().all(|r| r.is_identity())
    }
}

/// One link created to carry a value past a node that is no longer there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bridge {
    /// The new link's id in the host tree.
    pub link: LinkId,
    /// The producing socket, upstream of the node that went away.
    pub from: Socket,
    /// The consuming socket, downstream of it.
    pub to: Socket,
    /// Whether the bridge is muted, which it is when *either* of the two links
    /// it replaces was: a value that was being stopped must go on being
    /// stopped. Blender propagates the upstream link's flag onto the surviving
    /// one for the same reason.
    pub muted: bool,
}

/// What a [`dissolve`](Document::dissolve) or [`detach`](Document::detach) did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rewired {
    /// Links added to carry values past the node.
    pub bridged: Vec<Bridge>,
    /// Downstream links removed because no value reached them — the output they
    /// left by had no route, or the input that route names was not wired.
    ///
    /// This is the difference between bypassing and dissolving, named. Blender
    /// removes exactly these links and reports nothing.
    pub severed: Vec<Link>,
    /// Every link that touched the node and is now gone, the severed ones
    /// included.
    pub removed: Vec<Link>,
}

impl Rewired {
    /// Whether every value that used to flow through the node still flows.
    #[must_use]
    pub fn lossless(&self) -> bool {
        self.severed.is_empty()
    }
}

impl<K: NodeKind> Document<K> {
    /// What `node` would pass through if it were bypassed.
    ///
    /// Answerable whether or not the node *is* bypassed — an editor offering
    /// the operation needs to show its effect first, and the same question is
    /// what [`Self::dissolve`] is about to act on.
    ///
    /// `None` when the node is not there.
    #[must_use]
    pub fn passthrough(&self, tree: TreeId, node: NodeId) -> Option<Passthrough> {
        let signature = self.signature(tree, node)?;
        let mut routes: Vec<Route> = Vec::new();
        let mut dropped_outputs = Vec::new();

        // R1587 — a port may declare itself off the bypass path, and the
        // declaration is read from both ends: an excluded input is never a
        // source, an excluded output receives nothing.
        let eligible = |input: &crate::model::Port<K::Type, K::Value>, ty: &K::Type| {
            input.passthrough && input.ty == *ty
        };
        for (index, out) in signature.outputs.iter().enumerate() {
            let output = u32::try_from(index).unwrap_or(u32::MAX);
            // The identity first: same index, if the types agree there.
            let same_index = signature
                .inputs
                .get(index)
                .is_some_and(|input| eligible(input, &out.ty));
            let chosen = if !out.passthrough {
                None
            } else if same_index {
                Some(output)
            } else {
                signature
                    .inputs
                    .iter()
                    .position(|input| eligible(input, &out.ty))
                    .map(|at| u32::try_from(at).unwrap_or(u32::MAX))
            };
            match chosen {
                Some(input) => routes.push(Route { output, input }),
                None => dropped_outputs.push(output),
            }
        }

        let unreached_inputs = (0..signature.inputs.len())
            .map(|index| u32::try_from(index).unwrap_or(u32::MAX))
            .filter(|port| !routes.iter().any(|r| r.input == *port))
            .collect();

        Some(Passthrough {
            routes,
            dropped_outputs,
            unreached_inputs,
        })
    }

    /// Remove `node`, reconnecting what flowed through it.
    ///
    /// Blender's `NODE_OT_delete_reconnect`; the general form of the
    /// "delete a reroute knot and keep the wire" gesture, which is the
    /// one-in-one-out special case of this.
    ///
    /// Every link that touched the node goes. In its place, for each downstream
    /// link leaving an output the [`passthrough`](Self::passthrough) routes,
    /// a bridge is added from whatever fed the routed input. Downstream links
    /// that no value reaches are reported in [`Rewired::severed`] rather than
    /// merely vanishing.
    ///
    /// No bridge can close a cycle: its producer was upstream of the node and
    /// its consumer downstream, so a path back would have been a cycle through
    /// the node already. No bridge can displace a link either, since the input
    /// it lands on was fed by the node and that link is going.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchNode`].
    pub fn dissolve(&mut self, tree: TreeId, node: NodeId) -> Result<Rewired, EditError> {
        self.rewire_through(tree, node, true)
    }

    /// Unwire `node`, reconnecting what flowed through it, and leave it where
    /// it is.
    ///
    /// Blender's `NODE_OT_links_detach` and the drag half of
    /// `NODE_OT_move_detach_links`: pull a node out of the flow it is sitting
    /// in, without deleting it. The same derivation and the same report as
    /// [`Self::dissolve`] — the node simply stays, wired to nothing.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] or [`EditError::NoSuchNode`].
    pub fn detach(&mut self, tree: TreeId, node: NodeId) -> Result<Rewired, EditError> {
        self.rewire_through(tree, node, false)
    }

    /// The body of both: bridge across `node`, drop its links, and optionally
    /// drop the node itself.
    fn rewire_through(
        &mut self,
        tree: TreeId,
        node: NodeId,
        remove_node: bool,
    ) -> Result<Rewired, EditError> {
        let passthrough = self
            .passthrough(tree, node)
            .ok_or(EditError::NoSuchNode { tree, node })?;
        let host = self.tree(tree).ok_or(EditError::NoSuchTree(tree))?;

        // Plan against the tree as it stands, then apply — so every lookup sees
        // the same graph and no bridge is derived from a half-edited one.
        let mut plan: Vec<(Socket, Socket, bool)> = Vec::new();
        let mut severed = Vec::new();
        let mut removed = Vec::new();
        for link in host.links() {
            if link.from.node != node && link.to.node != node {
                continue;
            }
            removed.push(*link);
            if link.from.node != node {
                continue;
            }
            let upstream = passthrough
                .source_of(link.from.port)
                .and_then(|input| host.link_into(Socket::new(node, input)));
            match upstream {
                // A bridge back onto its own producer would need the document
                // to have held a cycle through this node already; the value has
                // nowhere to go, so the downstream link is severed like any
                // other that nothing reaches.
                Some(feed) if feed.from.node != link.to.node => {
                    plan.push((feed.from, link.to, feed.muted || link.muted));
                }
                _ => severed.push(*link),
            }
        }

        self.unwire_node(tree, node);
        if remove_node {
            self.take_node(tree, node);
        }

        let bridged = plan
            .into_iter()
            .map(|(from, to, muted)| Bridge {
                link: self.push_link(tree, from, to, muted),
                from,
                to,
                muted,
            })
            .collect();

        Ok(Rewired {
            bridged,
            severed,
            removed,
        })
    }
}
