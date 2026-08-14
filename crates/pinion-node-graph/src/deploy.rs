//! R1687 — **the order the graph is STARTED in**, which is not the order it
//! runs in.
//!
//! [`run`](crate::run) answers *which node gets control next, inside one
//! instant*. This answers *which process has to be up before which other one
//! can reach it* — a question about a graph whose nodes are **peers on a
//! network** rather than steps in a walk. The two never meet: a control walk
//! has a single entry and a stack, and a deployment has neither.
//!
//! # The rule, and why it is the link direction
//!
//! A link here means *this end reaches out to that one*. So a node nothing
//! reaches out to has nobody to wait for, and a node everything reaches out to
//! has to be standing first. The order falls out of two questions asked of the
//! links alone:
//!
//! | is dialled | dials out | [`Bringup`] | why |
//! |---|---|---|---|
//! | yes | no | [`Bringup::First`] | somebody dials it and it dials nobody |
//! | yes | yes | [`Bringup::Between`] | it is dialled *and* it dials |
//! | no | yes | [`Bringup::Last`] | it only dials, so everything it needs is up |
//! | no | no | [`Bringup::Alone`] | nothing to wait for and nothing waiting |
//!
//! # ★★★ The order is topological, and the four words above are only the REASON
//!
//! The first draft ordered by those four buckets alone and justified it with
//! *"a topological sort would have to refuse, because two peers that dial each
//! other are a legal mesh"*. **Both halves were wrong, and the model said so:**
//! [`Document::connect`](crate::Document::connect) refuses a cycle outright
//! (`WouldCycle`), so an authored tree is acyclic by construction — mutual
//! reachability lives in the *observed* layer ([`crate::observed`]), which is
//! not authored and not deployed.
//!
//! And with the excuse gone the bucketing was simply incorrect. In a chain of
//! four, `a → b → c → d`, both `b` and `c` are "reached and reaching", so a
//! bucket puts them in the same class and the tie-break — a NAME — decides
//! their order. But `b` reaches out to `c`: `c` has to be standing first, and a
//! name cannot know that. Three links deep the buckets are right by luck;
//! four deep they are wrong.
//!
//! So the walk is Kahn's, over the reversed link direction, and the buckets
//! stay as the *reported reason* a node sits where it does — which is what a
//! reader wants and what the order alone cannot say.
//!
//! **Deterministic**: among the nodes that are ready at the same moment the
//! next one is chosen by display NAME, never by id — an id is minted in
//! authoring order, so ordering by it would make the plan a fact about the
//! sequence somebody happened to draw the graph in.
//!
//! **Total anyway.** A cycle cannot be authored, but this does not depend on
//! that: whatever the walk cannot place goes at the end in name order rather
//! than being dropped. A deployment that silently omitted a node would be worse
//! than one that ordered it badly, and a model invariant is not a reason to
//! make a second thing depend on it.
//!
//! # ★★ A node that is switched OFF is not started, and neither are its links
//!
//! [`Node::disabled`](crate::Node::disabled) means *this node produces
//! nothing* (R1682) — as against bypassing, which passes its input through and
//! leaves the graph below alive. Starting a process for it would contradict the
//! only thing the switch says, so it is not in the order at all.
//!
//! ★ **Its links go with it**, and that is not the same statement. In `a → b →
//! c` with `b` off, `a` never dialled `c` — the two links are separate and both
//! ended at `b` or began there. So with `b` gone `a` and `c` have nothing
//! between them and are ordered by name, which is exactly right: neither has to
//! wait for the other. A walk that kept the edges would invent a constraint
//! from a node that is not being started.
//!
//! What is left out is therefore **derivable and not hidden**: it is every node
//! of the tree this does not name, and a caller wanting to report the omission
//! reads `tree.nodes()` against the answer. That is a subtraction with one
//! obvious spelling, which is why there is no second call for it.
//!
//! # What it is NOT
//!
//! It is not a claim that the order is sufficient. Whether a node is *ready*
//! when the next one dials it is a race no ordering closes — the reference tool
//! this shape comes from writes a wait into the script it generates, and that
//! is the script's business. This answers only the part that is a fact about
//! the graph.

use crate::model::{Document, NodeId, NodeKind, TreeId};

/// Where a node stands in the order the graph is brought up.
///
/// Ordered, and the derive is the declaration: `First < Between < Last <
/// Alone`. A node nobody waits for is last on purpose — it is the only bucket
/// whose members can be brought up in any order at all, so putting it anywhere
/// else would suggest a constraint that is not there.
///
/// ★ Named `Bringup` and not `Standing`, which this crate already uses for
/// *whether an answer about the drawn graph can be trusted as an answer about
/// the world* (R1645). Two unrelated facts under one word is how a reader comes
/// to believe a deployment order says something about discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bringup {
    /// Dialled by others and dials nobody: it has to be up first.
    First,
    /// Dialled by others and dials others.
    Between,
    /// Dials others and is dialled by nobody.
    Last,
    /// Neither end of any link.
    Alone,
}

impl Bringup {
    /// Every arm, so a census counts against the type rather than a list.
    pub const ALL: [Self; 4] = [Self::First, Self::Between, Self::Last, Self::Alone];

    /// The word this standing goes onto a wire as.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Between => "between",
            Self::Last => "last",
            Self::Alone => "alone",
        }
    }

    /// The standing of a node that is dialled by others / dials others, as
    /// given.
    ///
    /// ★ The two words are `dialled` and `dials` rather than `reached` and
    /// `reaches`, which is the module header's own vocabulary — and the pair it
    /// replaces was near enough alike that the lint refused it, correctly: two
    /// booleans one letter apart, in that order, is a call nobody can read.
    #[must_use]
    pub const fn of(dialled: bool, dials: bool) -> Self {
        match (dialled, dials) {
            (true, false) => Self::First,
            (true, true) => Self::Between,
            (false, true) => Self::Last,
            (false, false) => Self::Alone,
        }
    }
}

/// One node's place in the bring-up order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// Which node.
    pub node: NodeId,
    /// The name it is ordered by, carried so a caller does not re-derive it.
    pub name: String,
    /// Why it sits where it does.
    pub standing: Bringup,
}

impl<K: NodeKind> Document<K> {
    /// The order this tree's nodes have to be started in, so that a node is up
    /// before anything that reaches out to it.
    ///
    /// Every node that is **started** appears exactly once, whatever the links
    /// do. A node switched off is not started and is not here — see the module
    /// header for that, for the walk, and for what the first draft got wrong.
    ///
    /// A **muted** link still counts. Muting is a semantic declaration about
    /// the value a link carries, and every structural derivation in this crate
    /// ignores it for the same reason: the wiring is still there, and a
    /// deployment is a structural question. A disabled *node* is the other
    /// case, and the difference is which of the two the switch is on.
    #[must_use]
    pub fn launch_order(&self, tree: TreeId) -> Vec<Placed> {
        let Some(held) = self.tree(tree) else {
            return Vec::new();
        };
        let started = |id: NodeId| held.node(id).is_some_and(|node| !node.disabled);
        // A link only constrains the order while BOTH of its ends are being
        // started; see the module header for why dropping the others is not the
        // same as dropping the node's own edges.
        let live = |link: &&crate::Link| started(link.from.node) && started(link.to.node);
        // Every started node, with the reason it will be reported under and the
        // count of things it still has to wait for. A node waits for whatever
        // it reaches OUT to, so the walk runs the link direction backwards.
        let mut waiting: Vec<(Placed, usize)> = held
            .nodes()
            .filter(|node| !node.disabled)
            .map(|node| {
                let id = node.id;
                let dialled = held
                    .links()
                    .iter()
                    .filter(live)
                    .any(|link| link.to.node == id);
                let waits = held
                    .links()
                    .iter()
                    .filter(live)
                    .filter(|link| link.from.node == id)
                    .count();
                (
                    Placed {
                        node: id,
                        name: node.display_name(),
                        standing: Bringup::of(dialled, waits > 0),
                    },
                    waits,
                )
            })
            .collect();

        let mut order: Vec<Placed> = Vec::with_capacity(waiting.len());
        while !waiting.is_empty() {
            // The ready ones, and among them the first by name — which is what
            // makes the same graph give the same plan twice.
            let next = waiting
                .iter()
                .enumerate()
                .filter(|(_, (_, waits))| *waits == 0)
                .min_by(|(_, (a, _)), (_, (b, _))| a.name.cmp(&b.name))
                .map(|(at, _)| at);
            let Some(at) = next else {
                // Unreachable while `connect` refuses a cycle. Kept because a
                // plan that DROPPED a node would be worse than one that ordered
                // it badly, and this does not want to depend on that invariant.
                waiting.sort_by(|(a, _), (b, _)| a.name.cmp(&b.name));
                order.extend(waiting.into_iter().map(|(placed, _)| placed));
                return order;
            };
            let (placed, _) = waiting.remove(at);
            // Whoever reached out to it has one fewer thing to wait for.
            for (other, waits) in &mut waiting {
                let feeds = held
                    .links()
                    .iter()
                    .filter(live)
                    .filter(|link| link.from.node == other.node && link.to.node == placed.node)
                    .count();
                *waits -= feeds.min(*waits);
            }
            order.push(placed);
        }
        order
    }
}
