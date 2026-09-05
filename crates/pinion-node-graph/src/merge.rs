//! ★★★★★ R2008 — **three versions of a document, and where their changes
//! meet.**
//!
//! [`Document::changes_from`] is what one version did to a base;
//! [`Document::merge_from`] is the three-way, and its whole job is the join:
//! which subjects BOTH sides touched, and what that meeting is.
//!
//! # The reference's shape, measured
//!
//! Its merge tool takes a base, a remote and a local, diffs each side against
//! the base, and then walks the two difference lists looking for pairs that
//! touch the same thing. The unit above that is the GRAPH: it takes the union
//! of graph paths across the three versions and records, per path, whether it
//! exists in each — so *added on one side*, *deleted on one side* and *changed
//! on both* are all one table.
//!
//! ★ That existence axis is a question this crate already answers one dimension
//! narrower. `hello-graph-diff`'s [`LinkLayer`](crate::LinkLayer) is *in both* /
//! *in the first only* / *in the second only* over two sets; a merge asks it
//! over three. Same shape, and [`Standing`] is it.
//!
//! # ★★★★★ THREE MEASURED DEFECTS IN ITS CONFLICT RULE, and its own source says
//! the first one out loud
//!
//! 1. **Two people making the SAME change is reported as a conflict.** The
//!    comment beside the pin case says it: *given the wide variety of changes
//!    that can be made to a pin it is difficult to identify the change as
//!    identical, for now I'm just flagging all changes to the same pin as a
//!    conflict*. It is difficult THERE because a difference is a description of
//!    an edit built for display. Here the two versions are still in hand, so
//!    *both sides did the same thing* is decided against them — see [`agrees`],
//!    which asks the very function that decided the changes.
//!    ⚠ **Equal [`What`]s are not enough and that is a defect this module made
//!    before it caught it**: a `What` is a KIND of change, so two people moving
//!    one card to two different places both report [`What::Moved`], and reading
//!    the kind alone would call that agreement — the reference's coarseness in
//!    a different shape.
//!
//! 2. **The harmless-change exclusion is ASYMMETRIC.** It skips a pair when the
//!    REMOTE side's difference is a move or a comment — the local side's is not
//!    tested — so *they moved a node, I retyped it* is excused and *I moved a
//!    node, they retyped it* is a conflict. Two people doing the same two
//!    things get different answers depending on which of them pushed first.
//!    Here [`Change::structural`] is a property of a change, so the rule reads
//!    the same from both sides by construction.
//!
//! 3. **A change can conflict with at most ONE.** The search `break`s at its
//!    first match and the map is keyed by a pointer to that one, so a remote
//!    edit that clashes with two local edits reports one of them and the other
//!    is silently a clean change. [`Merged::meetings`] pairs a subject with
//!    every change on it.
//!
//! ⚠ And a fourth, not reproduced because it is not a capability: the
//! difference lists are sorted by the KIND of difference, which is not an order
//! anybody navigates in.
//!
//! # What matches what
//!
//! Nodes and links are matched by their **id**, which is stable across a save
//! and a load ([`Archive`](crate::Archive) writes it and reads it back), so two
//! versions of one document agree about what a node IS without a second
//! identity to keep in step. The reference matches on a per-node GUID for the
//! same reason. ⚠ The stated limit: two documents that never shared a history
//! have unrelated ids, and merging them is not a question this answers — it
//! answers *these two both came from that one*.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{Document, Link, LinkId, Node, NodeId, NodeKind, TreeId};

/// Which of the three versions hold a thing (R2008).
///
/// ★ The same question [`LinkLayer`](crate::LinkLayer) answers over two sets,
/// asked over three — and a struct of three flags rather than an enum of eight
/// arms because the eight are not eight *kinds*: a caller reads the axis it
/// cares about, and the two readings that carry a decision are named below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Standing {
    /// It is in the version both sides started from.
    pub base: bool,
    /// It is in the version being merged in.
    pub remote: bool,
    /// It is in the version being merged into.
    pub local: bool,
}

impl Standing {
    /// Whether exactly one side added it — the case a merge takes without
    /// asking, because there is nothing to weigh it against.
    #[must_use]
    pub const fn added_by_one(self) -> bool {
        !self.base && (self.remote != self.local)
    }

    /// Whether one side removed what the other still has.
    ///
    /// ★ Its own reading because it is the one shape where a merge cannot
    /// decide alone: keeping and deleting are both losses, in opposite
    /// directions, and no rule about the CONTENT settles it.
    #[must_use]
    pub const fn removed_by_one(self) -> bool {
        self.base && (self.remote != self.local)
    }
}

/// What a change is about (R2008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Subject {
    /// One node of the tree.
    Node(NodeId),
    /// One link of the tree.
    Link(LinkId),
}

/// What one version did to one subject, against the base (R2008).
///
/// A **value**, which is the whole reason the same edit made twice can be
/// recognised: the reference's difference is a description built for display
/// and its own source says identifying two of them as identical is difficult.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    /// The base does not have it and this version does.
    Added,
    /// The base has it and this version does not.
    Removed,
    /// Both have it and **what the graph means through it** differs.
    ///
    /// For a node that is its body, whether it is bypassed or switched off,
    /// which frame holds it, the values authored on its ports and its variadic
    /// items; for a link its two ends and whether it is muted. Each of those
    /// fields says in its own documentation that a derivation or the evaluator
    /// reads it — see [`Node::bypassed`](crate::Node::bypassed), which calls
    /// itself *the one fact on a node, other than its body and its links, that
    /// changes what the graph means*.
    Rewritten,
    /// Both have it and it sits somewhere else on the canvas.
    Moved,
    /// Both have it and what a person CALLS it differs — its label, or the
    /// sentence written about it.
    Renamed,
    /// Both have it and how it is DRAWN differs — collapsed, tinted, and the
    /// rest of what a node's looks hold.
    Restyled,
}

impl What {
    /// Whether this alters what the graph MEANS, as against how it is drawn or
    /// what it is called.
    ///
    /// ★★★★★ The rule the reference writes as a hand-listed pair of difference
    /// kinds tested on ONE SIDE of a pair. Here it is a property of the change,
    /// so a meeting reads the same whichever side made which — the asymmetry
    /// that makes *they moved it, I rewrote it* and *I moved it, they rewrote
    /// it* two different answers there is unrepresentable.
    #[must_use]
    pub const fn structural(self) -> bool {
        match self {
            Self::Added | Self::Removed | Self::Rewritten => true,
            Self::Moved | Self::Renamed | Self::Restyled => false,
        }
    }
}

/// How two versions of one node differ, or `None` when they do not (R2008).
///
/// ★★★★★ **Destructured on purpose, so a field added to [`Node`]
/// cannot be silently invisible to a merge**: the compiler refuses this
/// function until the new field is named on one side of the meaning/looks split
/// or the other. A hand-listed set of fields is the same defect as a
/// hand-listed set of difference kinds — right when it is written, and nothing
/// re-performs it. This crate's first draft of this function read four of the
/// eleven, so a bypass, a switch-off, a re-parent, an authored value and an
/// item edit — every one of them meaning-bearing by its own field's
/// documentation — met as *nothing changed*.
///
/// The order is the ORDER OF CONSEQUENCE: what a node means outranks where it
/// sits, which outranks what it is called, which outranks how it is drawn. A
/// node changed two ways reports the heaviest, so a meeting on it is weighed by
/// the heaviest too.
fn node_difference<K: NodeKind>(a: &Node<K>, b: &Node<K>) -> Option<What> {
    let Node {
        // Not a difference: it is the identity the two are matched BY.
        id: _,
        body,
        x,
        y,
        label,
        description,
        bypassed,
        disabled,
        appearance,
        parent,
        values,
        items,
    } = a;
    if (body, bypassed, disabled, parent, values, items)
        != (
            &b.body,
            &b.bypassed,
            &b.disabled,
            &b.parent,
            &b.values,
            &b.items,
        )
    {
        return Some(What::Rewritten);
    }
    if (x, y) != (&b.x, &b.y) {
        return Some(What::Moved);
    }
    if (label, description) != (&b.label, &b.description) {
        return Some(What::Renamed);
    }
    if appearance != &b.appearance {
        return Some(What::Restyled);
    }
    None
}

/// How two versions of one link differ, or `None` when they do not (R2008).
///
/// Destructured for [`node_difference`]'s reason. A link has no looks of its
/// own — a derivation or the evaluator reads every field of it — so every
/// difference here is [`What::Rewritten`], and that is a fact about the type
/// rather than a simplification: `muted` stops a value reaching a port, which
/// is what the graph MEANS.
fn link_difference(a: &Link, b: &Link) -> Option<What> {
    let Link {
        id: _,
        from,
        to,
        muted,
    } = a;
    ((from, to, muted) != (&b.from, &b.to, &b.muted)).then_some(What::Rewritten)
}

/// Whether the two sides ended up with the same thing at `at` (R2008).
///
/// ★★★★★ The test that makes [`Meet::Agreed`] mean what it says. A [`What`] is
/// a KIND of change, so two people moving one card to two different places both
/// report [`What::Moved`] — equal kinds, unequal outcomes. Agreement is
/// therefore decided against the versions themselves, through the SAME
/// difference functions that decided the changes, so the two answers cannot
/// drift apart.
///
/// Both sides having removed it is agreement with nothing to compare, which the
/// `(None, None)` arm says.
fn agrees<K: NodeKind>(
    remote: &Document<K>,
    local: &Document<K>,
    tree: TreeId,
    at: Subject,
) -> bool {
    match at {
        Subject::Node(node) => match (
            remote.tree(tree).and_then(|host| host.node(node)),
            local.tree(tree).and_then(|host| host.node(node)),
        ) {
            (None, None) => true,
            (Some(one), Some(two)) => node_difference(one, two).is_none(),
            _ => false,
        },
        Subject::Link(link) => {
            let find = |doc: &Document<K>| {
                doc.tree(tree)
                    .and_then(|host| host.links().iter().find(|held| held.id == link).copied())
            };
            match (find(remote), find(local)) {
                (None, None) => true,
                (Some(one), Some(two)) => link_difference(&one, &two).is_none(),
                _ => false,
            }
        }
    }
}

/// One version's change to one subject (R2008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// Which tree it is in.
    pub tree: TreeId,
    /// What it is about.
    pub at: Subject,
    /// What was done.
    pub what: What,
}

impl Change {
    /// Whether it alters what the graph means — [`What::structural`], forwarded
    /// so a caller holding a change need not reach past it.
    #[must_use]
    pub const fn structural(&self) -> bool {
        self.what.structural()
    }
}

/// What two sides changing one subject amounts to (R2008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meet {
    /// Both sides did the SAME thing.
    ///
    /// ★★★★★ The answer the reference cannot give and says so: its own comment
    /// records that identifying two differences as identical is difficult, so
    /// it flags every pair on one subject as a conflict. A [`Change`] here is a
    /// value and equality is the whole test.
    Agreed,
    /// Both changed it and neither change alters what the graph means.
    ///
    /// Two people moving one card is not a decision anybody needs to make. ★
    /// Symmetric, unlike the reference's, which tests only the remote side.
    Harmless,
    /// Both changed it and at least one of them alters what it means.
    Conflict,
}

/// One subject both sides changed, and what that amounts to (R2008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Meeting {
    /// The tree it is in.
    pub tree: TreeId,
    /// The subject.
    pub at: Subject,
    /// What the version being merged in did.
    pub remote: What,
    /// What the version being merged into did.
    pub local: What,
    /// What the two amount to.
    pub meet: Meet,
}

/// A three-way comparison of one document against two others (R2008).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Merged {
    /// Every tree of the three, and which of them hold it, ascending.
    pub trees: Vec<(TreeId, Standing)>,
    /// What the version being merged in changed, ascending.
    pub remote: Vec<Change>,
    /// What the version being merged into changed, ascending.
    pub local: Vec<Change>,
    /// Every subject BOTH sides changed, ascending.
    ///
    /// ⚠ One entry per subject and **not** one per pair-that-happened-to-match:
    /// the reference stops at the first local difference clashing with a remote
    /// one, so a remote edit meeting two local edits reports one and the other
    /// passes as clean.
    pub meetings: Vec<Meeting>,
}

impl Merged {
    /// Every meeting that needs a person, ascending.
    #[must_use]
    pub fn conflicts(&self) -> Vec<Meeting> {
        self.meetings
            .iter()
            .copied()
            .filter(|met| met.meet == Meet::Conflict)
            .collect()
    }

    /// Whether the two sides can be joined without anyone deciding anything.
    ///
    /// ★ True for a merge whose only meetings are agreements and harmless
    /// pairs, which is a state the reference reports as conflicted.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.conflicts().is_empty() && !self.trees.iter().any(|(_, how)| how.removed_by_one())
    }
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R2008 — **what this version changed against `base`**, ascending.
    ///
    /// Nodes and links are matched by id, which survives a save and a load, so
    /// two versions of one document agree about what a node is without a second
    /// identity to keep in step.
    ///
    /// ⚠ A tree the base does not have contributes every one of its nodes and
    /// links as [`What::Added`] rather than one change about the tree: the tree
    /// itself is on [`Merged::trees`], and a caller that wants the summary
    /// reads that. Two facts, two places, so neither has to stand for the other.
    #[must_use]
    pub fn changes_from(&self, base: &Self) -> Vec<Change> {
        let mut found = Vec::new();
        let mut trees: BTreeSet<u32> = self.trees().map(|held| held.id.0).collect();
        trees.extend(base.trees().map(|held| held.id.0));
        for tree in trees {
            let tree = TreeId(tree);
            let (mine, theirs) = (self.tree(tree), base.tree(tree));
            let mut nodes: BTreeSet<NodeId> = mine
                .into_iter()
                .flat_map(|host| host.nodes().map(|held| held.id))
                .collect();
            nodes.extend(
                theirs
                    .into_iter()
                    .flat_map(|host| host.nodes().map(|held| held.id)),
            );
            for node in nodes {
                let here = mine.and_then(|host| host.node(node));
                let there = theirs.and_then(|host| host.node(node));
                // ★ ONE change per subject, weighed by `node_difference` — the
                // same function agreement is decided by, so a caller never has
                // to reconcile two spellings of *what differs*.
                let what = match (here, there) {
                    (Some(_), None) => Some(What::Added),
                    (None, Some(_)) => Some(What::Removed),
                    (Some(a), Some(b)) => node_difference(a, b),
                    (None, None) => None,
                };
                if let Some(what) = what {
                    found.push(Change {
                        tree,
                        at: Subject::Node(node),
                        what,
                    });
                }
            }
            let link_of = |host: Option<&crate::model::Tree<K>>| -> BTreeMap<LinkId, Link> {
                host.into_iter()
                    .flat_map(|host| host.links().iter().map(|link| (link.id, *link)))
                    .collect()
            };
            let (here, there) = (link_of(mine), link_of(theirs));
            let mut links: BTreeSet<LinkId> = here.keys().copied().collect();
            links.extend(there.keys().copied());
            for link in links {
                let what = match (here.get(&link), there.get(&link)) {
                    (Some(_), None) => Some(What::Added),
                    (None, Some(_)) => Some(What::Removed),
                    (Some(a), Some(b)) => link_difference(a, b),
                    (None, None) => None,
                };
                if let Some(what) = what {
                    found.push(Change {
                        tree,
                        at: Subject::Link(link),
                        what,
                    });
                }
            }
        }
        found.sort_by_key(|held| (held.tree.0, held.at));
        found
    }

    /// ★★★★★ R2008 — **the three-way**: what each side changed against this
    /// base, and where those changes MEET.
    ///
    /// `self` is the base — the version both sides started from — which is the
    /// argument order the reference's own explicit overload uses.
    ///
    /// The join is the point. Two sides can both touch a subject and mean
    /// three different things by it, and telling them apart is what saves a
    /// person from adjudicating edits nobody disagrees about:
    ///
    /// * they did the SAME thing ⇒ [`Meet::Agreed`];
    /// * they both did something that does not change what the graph means
    ///   ⇒ [`Meet::Harmless`];
    /// * otherwise ⇒ [`Meet::Conflict`].
    ///
    /// See this module's header for the three measured defects in the
    /// reference's version of that rule, one of which its own source states.
    #[must_use]
    pub fn merge_from(&self, remote: &Self, local: &Self) -> Merged {
        let mut ids: BTreeSet<u32> = self.trees().map(|held| held.id.0).collect();
        ids.extend(remote.trees().map(|held| held.id.0));
        ids.extend(local.trees().map(|held| held.id.0));
        let trees: Vec<(TreeId, Standing)> = ids
            .into_iter()
            .map(|id| {
                let tree = TreeId(id);
                (
                    tree,
                    Standing {
                        base: self.tree(tree).is_some(),
                        remote: remote.tree(tree).is_some(),
                        local: local.tree(tree).is_some(),
                    },
                )
            })
            .collect();

        let theirs = remote.changes_from(self);
        let ours = local.changes_from(self);
        // ★★★★★ Keyed by SUBJECT, so a subject both sides touched is one
        // meeting however many changes were made — where the reference walks
        // two lists and stops at the first pair, leaving a second clash to pass
        // as a clean change.
        let mine: BTreeMap<(u32, Subject), What> = ours
            .iter()
            .map(|held| ((held.tree.0, held.at), held.what))
            .collect();
        let mut meetings: Vec<Meeting> = theirs
            .iter()
            .filter_map(|held| {
                let ours = *mine.get(&(held.tree.0, held.at))?;
                // ★★★★★ Agreement is the same KIND of change AND the same
                // outcome. Two people moving one card to two different places
                // both report `Moved`, and reading the kind alone would call
                // that agreement — the reference's coarseness reproduced in a
                // different shape.
                let meet = if ours == held.what && agrees(remote, local, held.tree, held.at) {
                    Meet::Agreed
                } else if !ours.structural() && !held.what.structural() {
                    Meet::Harmless
                } else {
                    Meet::Conflict
                };
                Some(Meeting {
                    tree: held.tree,
                    at: held.at,
                    remote: held.what,
                    local: ours,
                    meet,
                })
            })
            .collect();
        meetings.sort_by_key(|held| (held.tree.0, held.at));
        Merged {
            trees,
            remote: theirs,
            local: ours,
            meetings,
        }
    }
}
