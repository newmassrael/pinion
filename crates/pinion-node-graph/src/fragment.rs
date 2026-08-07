//! R1578 — a fragment of a graph is a value.
//!
//! # Why this module exists
//!
//! Copy, paste and duplicate look like three editor gestures and are one
//! question: *can a piece of a node graph be lifted out and stand on its own?*
//! Everything hard about them is in that question rather than in the keystrokes.
//! A selection is not a list of nodes — it is nodes, the links **between** them,
//! the links that **cross** its edge, and, when one of those nodes is a group
//! instance, the whole transitive closure of definitions it depends on. Lift
//! only the nodes and you have copied a group instance that points at nothing.
//!
//! So a [`Fragment`] carries a whole [`Document`]: its root holds the copied
//! nodes and its definitions hold that closure. That is not an implementation
//! convenience — it is what makes a fragment answerable. Every question you can
//! ask a document you can ask a fragment: what is in it, what does it depend on,
//! does it satisfy the invariants, what would it evaluate to. Blender's clipboard
//! is a `.blend` file written to the temp directory (`copybuffer_nodes.blend`,
//! measured at `8cf50599`), so the only thing that can be done with it is a
//! paste.
//!
//! # The cut is named
//!
//! A fragment also records the boundary it was cut from — the values that used
//! to cross its edge, in the same producer-keyed form
//! [`pinion_graph::group::InterfaceSocket`] uses for a group interface, because
//! it is the same fact. Blender's `node_copy_local` copies a link only when both
//! of its ends are selected and does not record the others in any form; a user
//! who copies the middle of a chain is told nothing about the two wires that were
//! severed.
//!
//! Recording them is also what lets a paste **restore** them
//! ([`Crossings::KeepInbound`]). Blender has that as `keep_inputs`, a boolean on
//! `NODE_OT_duplicate` — and *only* there: `NODE_OT_clipboard_paste` declares one
//! property, `offset`, so pasting into the tree you copied from cannot re-feed
//! the copies.
//!
//! # Why only the inbound crossings come back
//!
//! Blender has `keep_inputs` and no `keep_outputs`, and says nowhere why. The
//! reason is a property of the model rather than a preference: an output may feed
//! any number of consumers, so reproducing an **inbound** crossing adds a link
//! and takes nothing away; an input takes at most one link, so reproducing an
//! **outbound** crossing would have to displace the link already feeding that
//! consumer — the copy would steal the original's connection. So the outbound
//! crossings are *published* ([`Fragment::outbound`]) rather than silently
//! absent, and a caller that wants one anyway has [`Document::connect`], which
//! reports the displacement.
//!
//! # Definitions are matched by content
//!
//! Pasting a group instance twice must not leave two definitions behind: editing
//! one would then stop affecting the other, which is what a group is *for*. So
//! [`Definitions::Share`] matches each carried definition against the
//! destination's and re-uses one when they agree.
//!
//! The match is by **content**, and it is **conservative**: it may fail to notice
//! that two definitions are the same up to a renumbering (and then adds a
//! duplicate), and it can never claim two different definitions are one. Blender
//! errs the other way — `BKE_main_merge` keys candidate matches on the datablock
//! **name**, and for two local IDs `are_ids_from_different_mains_matching`
//! returns `true` on the name alone, so pasting a group called `Fresnel` into a
//! file that already has an unrelated `Fresnel` silently rebinds the pasted
//! instance to the wrong definition.
//!
//! [`Definitions::Fork`] is the other arm — a deep copy, Blender's
//! `duplicate(linked=false)`. There it is a *user preference*
//! (`U.dupflag & USER_DUP_NTREE`), so whether an edit propagates depends on a
//! setting the person doing the edit cannot see from the gesture; here it is an
//! argument.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use pinion_graph::group as boundary;
use serde::{Deserialize, Serialize};

use crate::model::{
    Document, InterfaceSide, Link, LinkId, Node, NodeBody, NodeId, NodeKind, ROOT, Sink, Socket,
    Tree, TreeId, centroid,
};
use crate::numbering::Numbering;

/// One value that used to cross a fragment's boundary.
///
/// The same shape a derived group interface uses, and for the same reason: a
/// crossing is *one value*, identified by the socket that produces it. An
/// inbound crossing has its producer outside the fragment and its consumers
/// inside; an outbound crossing is the other way round.
///
/// The sockets inside the fragment name it in the fragment's **own** node ids,
/// which are the ids the nodes had in the tree they were cut from — a fragment
/// preserves them, so both ends of a crossing are readable in one numbering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Severed {
    producer: Socket,
    consumers: Vec<Socket>,
    #[serde(default)]
    muted: Vec<Socket>,
}

impl Severed {
    /// The socket producing the value.
    #[must_use]
    pub const fn producer(&self) -> Socket {
        self.producer
    }

    /// Every socket that consumed it, ascending and without repeats.
    #[must_use]
    pub fn consumers(&self) -> &[Socket] {
        &self.consumers
    }

    /// Those consumers whose crossing link was **muted** at the cut (R1586).
    ///
    /// Recorded because a re-attachment puts the wire back, and putting back a
    /// wire that was stopping a value as one that carries it would make a paste
    /// change what the graph computes.
    #[must_use]
    pub fn muted_consumers(&self) -> &[Socket] {
        &self.muted
    }

    /// Whether this crossing was muted at `consumer`.
    #[must_use]
    pub fn is_muted(&self, consumer: Socket) -> bool {
        self.muted.contains(&consumer)
    }
}

impl fmt::Display for Severed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> ", self.producer)?;
        for (i, consumer) in self.consumers.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{consumer}")?;
        }
        Ok(())
    }
}

/// A piece of a node graph, standing on its own.
///
/// Serializable, so this is what goes on a clipboard, down a wire or into a
/// palette of snippets. See the module docs for why it carries a whole
/// [`Document`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Serialize, K::Type: Serialize, K::Value: Serialize",
    deserialize = "K: Deserialize<'de>, K::Type: Deserialize<'de>, K::Value: Deserialize<'de>"
))]
pub struct Fragment<K: NodeKind> {
    content: Document<K>,
    inbound: Vec<Severed>,
    outbound: Vec<Severed>,
    source: TreeId,
    origin: (i32, i32),
}

impl<K: NodeKind> Fragment<K> {
    /// The fragment as a document: root holds the copied nodes, the rest are the
    /// definitions they depend on.
    ///
    /// Everything a document answers, a fragment answers — including
    /// [`Document::validate`] and [`Document::evaluate`].
    #[must_use]
    pub const fn document(&self) -> &Document<K> {
        &self.content
    }

    /// The copied nodes, ascending by id.
    pub fn nodes(&self) -> impl Iterator<Item = &Node<K>> {
        self.content.tree(ROOT).into_iter().flat_map(Tree::nodes)
    }

    /// How many nodes were copied.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.content.tree(ROOT).map_or(0, Tree::node_count)
    }

    /// The links wholly inside the selection, which came along unchanged.
    #[must_use]
    pub fn links(&self) -> &[Link] {
        self.content.tree(ROOT).map_or(&[], Tree::links)
    }

    /// The group definitions the copied nodes depend on, transitively.
    pub fn definitions(&self) -> impl Iterator<Item = &Tree<K>> {
        self.content.definitions()
    }

    /// The values that used to arrive from outside.
    ///
    /// These are the ones a [`Crossings::KeepInbound`] insertion restores.
    #[must_use]
    pub fn inbound(&self) -> &[Severed] {
        &self.inbound
    }

    /// The values the selection used to send outside.
    ///
    /// Never restored by an insertion — see the module docs — and published so
    /// the caller knows what was severed rather than discovering it by looking.
    #[must_use]
    pub fn outbound(&self) -> &[Severed] {
        &self.outbound
    }

    /// The tree this was cut from.
    ///
    /// Informational: a fragment is a value and may be inserted anywhere. A
    /// caller that wants "put it back where it came from" compares this itself.
    #[must_use]
    pub const fn source_tree(&self) -> TreeId {
        self.source
    }

    /// Where the selection sat, so an insertion at this point puts it back
    /// exactly.
    ///
    /// Node positions inside the fragment are relative to it.
    #[must_use]
    pub const fn origin(&self) -> (i32, i32) {
        self.origin
    }

    /// Every way this fragment breaks the document invariants.
    ///
    /// Empty for any fragment [`Document::extract`] produced. Worth asking of one
    /// that arrived from a file or a peer — see [`Document::insert`] for what an
    /// insertion does and does not promise about such a fragment.
    #[must_use]
    pub fn validate(&self) -> Vec<crate::group::Violation> {
        self.content.validate()
    }
}

/// What an insertion does with the crossings the fragment was cut from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crossings {
    /// Leave the copies fed only by what came with them. Blender's
    /// `keep_inputs = false`, and the only thing its paste can do.
    Drop,
    /// Re-feed each copy from the socket that used to feed its original, where
    /// that socket is still present in the destination tree and still carries
    /// the same type. Blender's `keep_inputs = true`.
    ///
    /// Only the inbound crossings: see the module docs for why the outbound ones
    /// cannot come back.
    KeepInbound,
}

/// What an insertion does with the definitions the fragment carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Definitions {
    /// Re-use a destination definition whose content agrees, so two instances of
    /// one group stay two instances of one group. Blender's
    /// `duplicate(linked=true)`, and what its paste does by name.
    Share,
    /// Add every carried definition as a new one, so the copies can be edited
    /// without touching the originals. Blender's `duplicate(linked=false)`.
    Fork,
}

/// What an insertion produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inserted {
    /// The new nodes in the destination tree, in fragment order.
    pub nodes: Vec<NodeId>,
    /// The links between them.
    pub links: Vec<LinkId>,
    /// Definitions added to the document, inner-first.
    pub definitions_added: Vec<TreeId>,
    /// Definitions already present that the insertion bound to instead.
    pub definitions_reused: Vec<TreeId>,
    /// Links restored from the fragment's inbound crossings.
    pub reattached: Vec<LinkId>,
    /// Inbound crossings that could not be restored — the producing socket is
    /// gone from the destination tree, or no longer carries the type the copy
    /// expects. Named rather than dropped, because "your paste is missing two
    /// inputs" is a thing the user has to be told.
    pub unattached: Vec<Severed>,
}

/// Why a selection could not be lifted out.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExtractError {
    /// Nothing was selected.
    Empty,
    /// No such tree.
    NoSuchTree(TreeId),
    /// A selected node is not in that tree.
    NoSuchNode(NodeId),
    /// A selected node materialises the tree's own interface, which is a
    /// projection of that tree and not a thing that can travel.
    InterfaceNodeSelected(NodeId),
    /// A link names a node the tree does not hold, so there is no graph to cut.
    /// [`Document::validate`] reports these.
    Malformed {
        /// The link.
        link: crate::model::LinkId,
    },
    /// The cut refused for a reason this crate has no arm for.
    ///
    /// `pinion_graph::group::Refusal` is `#[non_exhaustive]`; carrying the
    /// sentence is the only answer that stays true when it grows an arm.
    Boundary(String),
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("nothing is selected"),
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode(node) => write!(f, "no node {}", node.0),
            Self::InterfaceNodeSelected(node) => write!(
                f,
                "node {} is this tree's own interface and cannot be copied",
                node.0
            ),
            Self::Malformed { link } => {
                write!(f, "link {} names a node this tree does not have", link.0)
            }
            Self::Boundary(reason) => write!(f, "the selection cannot be cut: {reason}"),
        }
    }
}

impl std::error::Error for ExtractError {}

/// Why a fragment could not be put into a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InsertError {
    /// No such tree to insert into.
    NoSuchTree(TreeId),
    /// The fragment's root holds a node materialising some tree's interface. No
    /// fragment [`Document::extract`] built can, but one that arrived from
    /// elsewhere can, and inserting it would give the destination two.
    InterfaceNodeInFragment(NodeId),
    /// The insertion would make a definition contain itself, along this chain.
    ///
    /// Reached by copying a group instance and pasting it inside that very group
    /// — the one recursion an editor meets by accident.
    Recursion {
        /// The containment chain the insertion would close.
        chain: Vec<TreeId>,
    },
}

impl fmt::Display for InsertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::InterfaceNodeInFragment(node) => write!(
                f,
                "the fragment carries interface node {}, which belongs to the tree it came from",
                node.0
            ),
            Self::Recursion { chain } => {
                f.write_str("that would nest a group inside itself: ")?;
                for (i, tree) in chain.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" contains ")?;
                    }
                    write!(f, "{}", tree.0)?;
                }
                f.write_str(", which would then contain the first")
            }
        }
    }
}

impl std::error::Error for InsertError {}

impl<K: NodeKind> Document<K> {
    /// Lift `selection` out of `tree` as a standalone value, leaving the
    /// document untouched.
    ///
    /// The fragment carries the selected nodes, the links between them, the
    /// transitive closure of the group definitions they instance, and the
    /// boundary that was severed.
    ///
    /// # Errors
    ///
    /// See [`ExtractError`].
    pub fn extract(&self, tree: TreeId, selection: &[NodeId]) -> Result<Fragment<K>, ExtractError> {
        let host = self.tree(tree).ok_or(ExtractError::NoSuchTree(tree))?;
        if selection.is_empty() {
            return Err(ExtractError::Empty);
        }
        let mut chosen: Vec<NodeId> = selection.to_vec();
        chosen.sort_unstable();
        chosen.dedup();
        for &id in &chosen {
            let node = host.node(id).ok_or(ExtractError::NoSuchNode(id))?;
            if matches!(node.body, NodeBody::Interface(_)) {
                return Err(ExtractError::InterfaceNodeSelected(id));
            }
        }

        // The same 0..n mapping every `pinion-graph` algorithm states as its
        // contract. A CUT rather than a `derive`: severing the crossings cannot
        // create a cycle, so there is nothing here to refuse.
        let numbering = Numbering::of(host).map_err(|link| ExtractError::Malformed { link })?;
        let selected = numbering
            .vertices(&chosen)
            .ok_or(ExtractError::NoSuchTree(tree))?;
        let cut = boundary::Boundary::cut(numbering.order(), numbering.links(), &selected)
            .map_err(|refusal| match refusal {
                boundary::Refusal::Empty => ExtractError::Empty,
                other => ExtractError::Boundary(other.to_string()),
            })?;

        let socket_of = |s: boundary::Socket| numbering.socket(s);
        let severed = |faces: &[boundary::InterfaceSocket]| -> Vec<Severed> {
            faces
                .iter()
                .map(|face| {
                    let consumers: Vec<Socket> =
                        face.consumers().iter().map(|&c| socket_of(c)).collect();
                    Severed {
                        muted: consumers
                            .iter()
                            .copied()
                            .filter(|&c| host.link_into(c).is_some_and(|l| l.muted))
                            .collect(),
                        producer: socket_of(face.producer()),
                        consumers,
                    }
                })
                .collect()
        };

        let origin = centroid(chosen.iter().filter_map(|&id| {
            let n = host.node(id)?;
            Some((n.x, n.y))
        }));

        // The definition closure, and this fragment's own numbering for it.
        let seeds: Vec<TreeId> = chosen
            .iter()
            .filter_map(|&id| match host.node(id)?.body {
                NodeBody::Group(definition) => Some(definition),
                _ => None,
            })
            .collect();
        let carried = self.definition_closure(&seeds);
        let mut content = Document::new(host.name.clone());
        let remap: BTreeMap<TreeId, TreeId> = carried
            .iter()
            .enumerate()
            .map(|(i, &src)| (src, TreeId(u32::try_from(i + 1).unwrap_or(u32::MAX))))
            .collect();
        for &src in &carried {
            let name = self.tree(src).map(|t| t.name.clone()).unwrap_or_default();
            let fresh = content.add_definition(name);
            debug_assert_eq!(Some(&fresh), remap.get(&src), "closure order fixes the ids");
            if let Some(source) = self.tree(src) {
                copy_tree_body(&mut content, fresh, source, &remap);
            }
        }

        for &id in &chosen {
            let Some(node) = host.node(id) else { continue };
            content.put_node(
                ROOT,
                Node {
                    id: node.id,
                    body: remapped_body(&node.body, &remap),
                    x: node.x - origin.0,
                    y: node.y - origin.1,
                    label: node.label.clone(),
                    bypassed: node.bypassed,
                    appearance: node.appearance.clone(),
                },
            );
        }
        for &index in cut.internal() {
            let link = host.links()[index];
            // A cut carries the wiring as it was: a muted link is still a link,
            // and a fragment that quietly unmuted one would paste back a graph
            // that computes something the original did not (R1586).
            content.push_link(ROOT, link.from, link.to, link.muted);
        }

        Ok(Fragment {
            content,
            inbound: severed(cut.inputs()),
            outbound: severed(cut.outputs()),
            source: tree,
            origin,
        })
    }

    /// Put a fragment into `tree`, with its origin at `at`.
    ///
    /// The nodes arrive with fresh ids; the definitions the fragment carries are
    /// re-used or added per `definitions`; the severed inbound values are
    /// restored or not per `crossings`. Everything is decided before the first
    /// mutation, so a refusal leaves the document **untouched**.
    ///
    /// Blender's paste does the opposite: `node_copy_local` reports an error for
    /// a node it cannot place, skips it and its links, and carries on — so a
    /// paste of five nodes containing one illegal group leaves four nodes and a
    /// message in a report list.
    ///
    /// # Guarantee
    ///
    /// If this document satisfies [`Document::validate`] and so does the
    /// fragment, so does the result. A fragment that does not is inserted
    /// faithfully, invariant breakages and all, except for the two this can
    /// detect and must refuse — see [`InsertError`].
    ///
    /// # Errors
    ///
    /// See [`InsertError`].
    pub fn insert(
        &mut self,
        tree: TreeId,
        fragment: &Fragment<K>,
        at: (i32, i32),
        crossings: Crossings,
        definitions: Definitions,
    ) -> Result<Inserted, InsertError> {
        if self.tree(tree).is_none() {
            return Err(InsertError::NoSuchTree(tree));
        }
        for node in fragment.nodes() {
            if matches!(node.body, NodeBody::Interface(_)) {
                return Err(InsertError::InterfaceNodeInFragment(node.id));
            }
        }
        let plan = self.plan_insert(tree, fragment, crossings, definitions)?;
        Ok(self.perform_insert(tree, fragment, at, &plan))
    }

    /// Copy `selection` beside itself in the same tree.
    ///
    /// Blender's `NODE_OT_duplicate`, which is exactly a cut and a paste that
    /// never leave the tree: `offset` is `duplicate_move`'s translation,
    /// `crossings` its `keep_inputs`, and `definitions` its `linked`.
    ///
    /// # Errors
    ///
    /// [`DuplicateError`] — either half can refuse, and which one did is part of
    /// the answer.
    pub fn duplicate(
        &mut self,
        tree: TreeId,
        selection: &[NodeId],
        offset: (i32, i32),
        crossings: Crossings,
        definitions: Definitions,
    ) -> Result<Inserted, DuplicateError> {
        let fragment = self.extract(tree, selection).map_err(DuplicateError::Cut)?;
        let at = (
            fragment.origin.0.saturating_add(offset.0),
            fragment.origin.1.saturating_add(offset.1),
        );
        self.insert(tree, &fragment, at, crossings, definitions)
            .map_err(DuplicateError::Place)
    }

    /// Every definition reachable from `seeds` through containment, inner-first.
    ///
    /// Inner-first because a definition's identity depends on its children's:
    /// deciding whether a carried definition matches one already in the
    /// destination is only answerable once the definitions it instances have
    /// been decided.
    fn definition_closure(&self, seeds: &[TreeId]) -> Vec<TreeId> {
        let containment = self.containment();
        let mut order = Vec::new();
        let mut done: BTreeSet<TreeId> = BTreeSet::new();
        let mut stack: Vec<(TreeId, bool)> =
            seeds.iter().rev().map(|&seed| (seed, false)).collect();
        let mut open: BTreeSet<TreeId> = BTreeSet::new();
        while let Some((tree, expanded)) = stack.pop() {
            if done.contains(&tree) {
                continue;
            }
            if expanded {
                done.insert(tree);
                order.push(tree);
                continue;
            }
            // A document this crate built has acyclic containment; one that did
            // not must still terminate, so a tree already open is not re-entered.
            if !open.insert(tree) {
                continue;
            }
            stack.push((tree, true));
            for &(_, inner) in containment
                .iter()
                .filter(|&&(host, _)| host == tree.0 as usize)
            {
                let inner = TreeId(u32::try_from(inner).unwrap_or(u32::MAX));
                if !done.contains(&inner) {
                    stack.push((inner, false));
                }
            }
        }
        order
    }

    /// Decide, before mutating anything, where each carried definition lands,
    /// whether the placement is legal, and which severed crossings can come
    /// back.
    ///
    /// The crossings are resolved **here** rather than during the insertion, and
    /// that ordering is load-bearing: a crossing names its outside end by socket
    /// address, and the copies are allocated fresh ids in the destination tree.
    /// Resolve afterwards and a paste into an emptier tree can hand a copy the
    /// very id its own producer used to have — which wires the copy to *itself*.
    /// A crossing's outside end is by definition not one of the copies, so it is
    /// looked up against the tree as it stands before any of them exist. Found
    /// by `an_inbound_crossing_that_cannot_be_restored_is_named`, which was
    /// written to check the reporting and caught the self-link instead.
    fn plan_insert(
        &self,
        tree: TreeId,
        fragment: &Fragment<K>,
        crossings: Crossings,
        policy: Definitions,
    ) -> Result<InsertPlan, InsertError> {
        let carried: Vec<TreeId> = fragment
            .content
            .definition_closure(&fragment.instanced_definitions());
        let mut resolved: BTreeMap<TreeId, TreeId> = BTreeMap::new();
        let mut added = Vec::new();
        let mut reused = Vec::new();
        let next = self.next_tree_id().0;
        for &frag in &carried {
            let Some(source) = fragment.content.tree(frag) else {
                continue;
            };
            let existing = (policy == Definitions::Share)
                .then(|| {
                    self.definitions()
                        .find(|candidate| same_definition(candidate, source, &resolved))
                        .map(|candidate| candidate.id)
                })
                .flatten();
            if let Some(destination) = existing {
                resolved.insert(frag, destination);
                reused.push(destination);
            } else {
                let provisional =
                    TreeId(next.saturating_add(u32::try_from(added.len()).unwrap_or(u32::MAX)));
                resolved.insert(frag, provisional);
                added.push(frag);
            }
        }

        // Containment as it would stand afterwards, one prospective edge at a
        // time so the edge that closes a cycle is the one named.
        let mut relation = self.containment();
        let edge = |relation: &mut Vec<(usize, usize)>, host: TreeId, inner: TreeId| {
            let (host, inner) = (host.0 as usize, inner.0 as usize);
            if let Some(chain) = boundary::Nesting::cycle(relation, host, inner) {
                return Err(InsertError::Recursion {
                    chain: chain
                        .into_iter()
                        .map(|t| TreeId(u32::try_from(t).unwrap_or(u32::MAX)))
                        .collect(),
                });
            }
            relation.push((host, inner));
            relation.sort_unstable();
            relation.dedup();
            Ok(())
        };
        for &frag in &added {
            let Some(&host) = resolved.get(&frag) else {
                continue;
            };
            for inner in fragment.content.instances_in(frag) {
                if let Some(&inner) = resolved.get(&inner) {
                    edge(&mut relation, host, inner)?;
                }
            }
        }
        for definition in fragment.instanced_definitions() {
            if let Some(&definition) = resolved.get(&definition) {
                edge(&mut relation, tree, definition)?;
            }
        }

        // Which severed values can come back, judged against the destination as
        // it stands now — see this function's own doc for why not later.
        let mut reattach = Vec::new();
        let mut unattached = Vec::new();
        if crossings == Crossings::KeepInbound {
            let mut fed: BTreeSet<Socket> = BTreeSet::new();
            for severed in &fragment.inbound {
                let mut landed = Vec::new();
                for &consumer in &severed.consumers {
                    // A malformed fragment may name one consumer twice; an input
                    // takes at most one link, so the second is not attempted.
                    if !fed.insert(consumer) {
                        continue;
                    }
                    if self.crossing_types_agree(tree, severed.producer, fragment, consumer) {
                        landed.push(Sink {
                            socket: consumer,
                            muted: severed.is_muted(consumer),
                        });
                    }
                }
                if landed.is_empty() {
                    unattached.push(severed.clone());
                } else {
                    reattach.push((severed.producer, landed));
                }
            }
        }

        Ok(InsertPlan {
            resolved,
            added,
            reused,
            reattach,
            unattached,
        })
    }

    /// Whether the producer still sits in `tree` carrying the type the
    /// fragment's consumer expects.
    ///
    /// Both ends are read **before** the insertion: the producer from this
    /// document, the consumer from the fragment's own signature — which is the
    /// same signature the copy will have, because a carried definition's
    /// interface is either copied whole or matched to an equal one.
    fn crossing_types_agree(
        &self,
        tree: TreeId,
        producer: Socket,
        fragment: &Fragment<K>,
        consumer: Socket,
    ) -> bool {
        let (Some(source), Some(sink)) = (
            self.signature(tree, producer.node),
            fragment.content.signature(ROOT, consumer.node),
        ) else {
            return false;
        };
        let (Some(out), Some(input)) = (
            source.outputs.get(producer.port as usize),
            sink.inputs.get(consumer.port as usize),
        ) else {
            return false;
        };
        out.ty == input.ty
    }

    /// Apply a validated plan. Nothing here can fail.
    fn perform_insert(
        &mut self,
        tree: TreeId,
        fragment: &Fragment<K>,
        at: (i32, i32),
        plan: &InsertPlan,
    ) -> Inserted {
        let mut definitions_added = Vec::new();
        for &frag in &plan.added {
            let name = fragment
                .content
                .tree(frag)
                .map(|t| t.name.clone())
                .unwrap_or_default();
            let fresh = self.add_definition(name);
            debug_assert_eq!(
                fresh, plan.resolved[&frag],
                "the plan predicted this id with the allocator's own expression"
            );
            if let Some(source) = fragment.content.tree(frag) {
                copy_tree_body(self, fresh, source, &plan.resolved);
            }
            definitions_added.push(fresh);
        }

        let mut renamed: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        let mut nodes = Vec::new();
        for node in fragment.nodes() {
            let body = remapped_body(&node.body, &plan.resolved);
            let Ok(fresh) = self.add_node(
                tree,
                body,
                at.0.saturating_add(node.x),
                at.1.saturating_add(node.y),
            ) else {
                continue;
            };
            if let Some(slot) = self.tree_mut(tree).and_then(|t| t.node_mut(fresh)) {
                slot.adopt_from(node);
            }
            renamed.insert(node.id, fresh);
            nodes.push(fresh);
        }

        let mut links = Vec::new();
        for link in fragment.links() {
            let (Some(&from), Some(&to)) =
                (renamed.get(&link.from.node), renamed.get(&link.to.node))
            else {
                continue;
            };
            links.push(self.push_link(
                tree,
                Socket::new(from, link.from.port),
                Socket::new(to, link.to.port),
                link.muted,
            ));
        }

        let mut reattached = Vec::new();
        let mut unattached = plan.unattached.clone();
        for (producer, consumers) in &plan.reattach {
            let mut landed = Vec::new();
            for consumer in consumers {
                if let Some(&node) = renamed.get(&consumer.socket.node) {
                    landed.push(self.push_link(
                        tree,
                        *producer,
                        Socket::new(node, consumer.socket.port),
                        consumer.muted,
                    ));
                }
            }
            if landed.is_empty() {
                // The copy itself never landed, so there is nothing to feed.
                unattached.push(Severed {
                    producer: *producer,
                    consumers: consumers.iter().map(|c| c.socket).collect(),
                    muted: consumers
                        .iter()
                        .filter(|c| c.muted)
                        .map(|c| c.socket)
                        .collect(),
                });
            } else {
                reattached.extend(landed);
            }
        }

        Inserted {
            nodes,
            links,
            definitions_added,
            definitions_reused: plan.reused.clone(),
            reattached,
            unattached,
        }
    }

    /// The definitions instanced directly by `tree`'s nodes, ascending.
    fn instances_in(&self, tree: TreeId) -> Vec<TreeId> {
        let mut found: Vec<TreeId> = self.tree(tree).map_or_else(Vec::new, |t| {
            t.nodes()
                .filter_map(|n| match n.body {
                    NodeBody::Group(definition) => Some(definition),
                    _ => None,
                })
                .collect()
        });
        found.sort_unstable();
        found.dedup();
        found
    }
}

impl<K: NodeKind> Fragment<K> {
    /// The definitions the copied nodes instance directly.
    fn instanced_definitions(&self) -> Vec<TreeId> {
        self.content.instances_in(ROOT)
    }
}

/// Why a duplicate did not happen.
///
/// Two arms because a duplicate is two operations, and "your selection cannot be
/// copied" and "the copy cannot go there" send the user to different places.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DuplicateError {
    /// The selection could not be lifted out.
    Cut(ExtractError),
    /// The copy could not be put back.
    Place(InsertError),
}

impl fmt::Display for DuplicateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cut(error) => write!(f, "{error}"),
            Self::Place(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DuplicateError {}

/// Everything an insertion needs, decided before the first mutation.
struct InsertPlan {
    resolved: BTreeMap<TreeId, TreeId>,
    added: Vec<TreeId>,
    reused: Vec<TreeId>,
    /// Producer in the destination -> the fragment's own consumer sockets, for
    /// the crossings that survived the check.
    reattach: Vec<(Socket, Vec<Sink>)>,
    unattached: Vec<Severed>,
}

/// A node body with its group reference moved into the destination numbering.
fn remapped_body<K: NodeKind>(body: &NodeBody<K>, remap: &BTreeMap<TreeId, TreeId>) -> NodeBody<K> {
    match body {
        NodeBody::Group(definition) => {
            NodeBody::Group(remap.get(definition).copied().unwrap_or(*definition))
        }
        other => other.clone(),
    }
}

/// Fill `destination`'s tree `target` from `source`, moving every group
/// reference into the destination numbering.
///
/// Node ids are preserved — they are per tree, so nothing forces them to change,
/// and keeping them is what lets a carried definition be compared with the one it
/// was copied from. Link ids are not: they are allocation state, and the
/// comparison reads a link as the pair of sockets it joins.
fn copy_tree_body<K: NodeKind>(
    destination: &mut Document<K>,
    target: TreeId,
    source: &Tree<K>,
    remap: &BTreeMap<TreeId, TreeId>,
) {
    for port in source.interface().inputs() {
        let _ = destination.expose(target, InterfaceSide::Input, port.clone());
    }
    for port in source.interface().outputs() {
        let _ = destination.expose(target, InterfaceSide::Output, port.clone());
    }
    for node in source.nodes() {
        destination.put_node(
            target,
            Node {
                id: node.id,
                body: remapped_body(&node.body, remap),
                x: node.x,
                y: node.y,
                label: node.label.clone(),
                bypassed: node.bypassed,
                appearance: node.appearance.clone(),
            },
        );
    }
    for link in source.links() {
        destination.push_link(target, link.from, link.to, link.muted);
    }
}

/// Whether a carried definition is the definition `candidate` already is.
///
/// Conservative in one direction on purpose: two definitions that differ only by
/// a renumbering of their nodes are reported as different, and the cost of that
/// is a duplicate definition. The other error — declaring two different
/// definitions to be one — would silently rebind an instance to a graph that
/// computes something else, which is what matching by name does.
fn same_definition<K: NodeKind>(
    candidate: &Tree<K>,
    carried: &Tree<K>,
    resolved: &BTreeMap<TreeId, TreeId>,
) -> bool {
    if candidate.name != carried.name
        || candidate.interface() != carried.interface()
        || candidate.node_count() != carried.node_count()
        || candidate.links().len() != carried.links().len()
    {
        return false;
    }
    for node in carried.nodes() {
        let Some(theirs) = candidate.node(node.id) else {
            return false;
        };
        if theirs.x != node.x || theirs.y != node.y || theirs.label != node.label {
            return false;
        }
        let same_body = match (&theirs.body, &node.body) {
            (NodeBody::Group(theirs), NodeBody::Group(ours)) => resolved.get(ours) == Some(theirs),
            (theirs, ours) => theirs == ours,
        };
        if !same_body {
            return false;
        }
    }
    candidate
        .links()
        .iter()
        .zip(carried.links())
        .all(|(theirs, ours)| theirs.from == ours.from && theirs.to == ours.to)
}
