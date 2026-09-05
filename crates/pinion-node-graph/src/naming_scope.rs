//! ★★★★★ R1932 — **what a kind requires of its own name**: where it has to be
//! unique, or that it need not be.
//!
//! # What the reference does, measured at its header, its consumers and its
//! fourteen overriders
//!
//! Its graph node publishes *make me a name validator*, supplied `NULL`, and its
//! schema publishes a second call of the same shape that is **not the same
//! mechanism**: the node's takes no arguments and answers for THAT node, the
//! schema's takes four (a blueprint, the original name, a validation scope and
//! an action type) and its one implementation is not overridden anywhere — its
//! consumers are the palette and the details panel, naming a blueprint's
//! variables and actions rather than a graph's nodes. Two names for two
//! capabilities on two subjects, and only the first is about a node.
//!
//! Reading all fourteen overriders of the first is what shaped this module, and
//! they do exactly two things:
//!
//! 1. ★★★★★ **Four of them SUPPRESS.** A comment and both reroute classes answer
//!    a dummy validator that says `Ok` to everything, carrying the same
//!    copy-pasted comment — *comments can be duplicated, etc...* That is the
//!    commonest single use, and it is the same shape R1928 measured on the pin
//!    naming hook: the capability's ordinary job is to take a rule AWAY.
//! 2. **The rest choose a SCOPE.** A composite, a timeline, a custom event, a
//!    function entry, a state machine and a cached pose each build a validator
//!    over the whole **blueprint** — not over the graph the node sits in — so
//!    what the override actually settles is *how far this name has to reach to
//!    be unique*.
//!
//! ⇒ so the axis is a scope with an off position, which is what [`Naming`] is.
//!
//! # ⚠ And the census's covering sentence was false
//!
//! It read *no name-validation surface: a label is free text*. A label has not
//! been free text since R1682:
//! [`Document::may`](Document::may)`(Act::Rename)` already refuses an empty
//! name ([`LabelEmpty`](crate::EditError::LabelEmpty)) and a name another node
//! in the tree holds ([`LabelTaken`](crate::EditError::LabelTaken), which NAMES
//! that node — the reference's
//! `AlreadyInUse` is a bare enum constant and cannot). Three of the reference's
//! seven validator results were already here.
//!
//! What was absent is that the rule was the CRATE's alone: an application could
//! neither widen the scope nor turn it off, and a frame — this crate's comment —
//! was held to the same uniqueness as a node the graph is addressed by.
//!
//! # The three measured ways this passes it
//!
//! 1. **The off position is a value, not an object you have to remember to
//!    build.** Two classes there hand back a dummy validator with an identical
//!    comment; here a kind answers [`Naming::Free`] and a frame *is* free
//!    without anybody writing anything.
//! 2. **The scope is a type rather than a constructor argument.** There the
//!    reach is whatever object was passed to the validator, so two classes
//!    wanting the same reach state it twice and can disagree; here there is one
//!    enum and [`Document::may`] reads it.
//! 3. **A refusal still names the holder.** Widening the scope did not cost the
//!    thing this crate has and the reference does not — `LabelTaken` says which
//!    node answers to the name, so a person is told what to rename rather than
//!    that they may not.

use std::collections::BTreeSet;

use crate::model::{Document, NodeBody, NodeId, NodeKind, TreeId};

/// What a kind requires of the name a person gives one of its nodes.
///
/// Three arms, measured off the reference's fourteen overriders rather than
/// invented: they either turn the rule off or choose how far the name has to
/// reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Naming {
    /// Unique among the authored names in the node's own tree. The supplied
    /// answer, and what this crate has enforced since R1682.
    #[default]
    InTree,
    /// Unique among the authored names in the whole document — every tree,
    /// including definitions this node is not in.
    ///
    /// The reference's commonest positive answer: six of its overriders build a
    /// validator over the whole blueprint rather than over the graph the node
    /// sits in.
    InDocument,
    /// Nothing is required. Two nodes of this kind may answer to one name.
    ///
    /// ⚠ A name that does not identify cannot be looked up, and
    /// [`Document::node_labelled`] says so by answering `None` when more than
    /// one node holds it. That is the trade this arm makes, and a kind takes it
    /// deliberately — the reference's comment and reroute classes do, because
    /// their name is a caption rather than an address.
    Free,
}

impl Naming {
    /// Whether a name of this kind has to identify one node.
    #[must_use]
    pub const fn is_unique(self) -> bool {
        !matches!(self, Self::Free)
    }
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1932 — **what `node` requires of its own name.**
    ///
    /// A kind answers for itself. Every other body answers what the crate knows
    /// about it, and one of those is not the default: a **frame** is
    /// [`Naming::Free`], for the reference's own reason — a frame takes no part
    /// in the graph, nothing is addressed by its caption, and holding two
    /// frames apart by name is a rule with nothing behind it. There the same
    /// decision is a dummy validator each commenting class has to remember to
    /// build.
    #[must_use]
    pub fn naming(&self, tree: TreeId, node: NodeId) -> Naming {
        match self.tree(tree).and_then(|host| host.node(node)) {
            Some(held) => self.naming_of(&held.body),
            None => Naming::InTree,
        }
    }

    /// ★★★★★ R1985 — the same question asked of a **body**, for a node that is
    /// not in a tree yet.
    ///
    /// [`Self::naming`] can only answer about a node the document already
    /// holds, and the copy path has to decide a name *before* the copy exists —
    /// see [`Document::insert`](Document::insert). Splitting the match out
    /// rather than writing a second one is R1963's rule: when one property is
    /// spelled in two places, what matters is not how many places but what is
    /// holding them together, and here it is this function.
    #[must_use]
    pub fn naming_of(&self, body: &NodeBody<K>) -> Naming {
        match body {
            NodeBody::Kind(_) => K::naming(),
            // A caption, not an address.
            //
            // ★ R1934 — a **reroute** is `Free` for the same reason and on
            // measured evidence, not by analogy. R1932's own census of all
            // fourteen overriders of the reference's name-validator hook
            // found four that suppress the rule entirely, and **both of the
            // engine's reroute classes are among them** — each handing back
            // a validator that accepts everything, carrying the same
            // copy-pasted remark about duplicates being allowed. A point on
            // a wire is not somewhere a value is addressed from.
            // ★ R1935 — an ECHO joins them, and a BEACON does not, which
            // is the whole distinction between the two halves: a beacon's
            // name is the address a value crosses the canvas to, so two of
            // them answering to one name would make that address
            // ambiguous. An echo has no name of its own at all — it shows
            // the beacon's — so there is nothing here to be unique.
            NodeBody::Frame | NodeBody::Reroute | NodeBody::Echo(_) => Naming::Free,
            // An interface end, a group instance and a delay are all
            // addressed by name somewhere — the tree they belong to is the
            // scope, which is the supplied answer. R1935's BEACON shares
            // the arm because it shares the answer, and it is named first
            // because it is the one whose name is the *whole point*: a
            // value crosses the canvas TO it.
            // ★ R2004 — a stand-in joins them, on measured evidence rather
            // than by analogy: the reference gives its alias node a rename
            // hook, a name validator, and an operator that runs that validator
            // to make `Self` unique before placing the card. A name it
            // uniquifies is a name it treats as an address. ⚠ It uniquifies by
            // appending until the name is free and tells nobody, which is the
            // silent repair R1935 declined to copy for a beacon; answering
            // `InTree` is what makes the clash a refusal here instead.
            NodeBody::Beacon
            | NodeBody::Group(_)
            | NodeBody::Interface(_)
            | NodeBody::Delay(_)
            | NodeBody::StandIn(_) => Naming::InTree,
        }
    }

    /// Every node in the WHOLE document that has authored the name `label`.
    ///
    /// The document-wide peer of
    /// [`nodes_labelled`](Document::nodes_labelled), which searches one tree.
    /// Needed because [`Naming::InDocument`] is a real answer and a scope that
    /// could not be searched would be a scope nothing enforced.
    #[must_use]
    pub fn nodes_labelled_anywhere(&self, label: &str) -> Vec<(TreeId, NodeId)> {
        (0..self.tree_count())
            .map(|index| TreeId(u32::try_from(index).unwrap_or(u32::MAX)))
            .flat_map(|tree| {
                self.nodes_labelled(tree, label)
                    .into_iter()
                    .map(move |node| (tree, node))
            })
            .collect()
    }

    /// ★★★★★ R1985 — **who already answers to `label`, in the scope this body's
    /// kind asked for.**
    ///
    /// [`Document::may`] made this decision inline and it was the only place
    /// that dispatched on [`Naming`] to FIND HOLDERS — measured over every file
    /// naming a variant of it: two only mention it in prose, and the node lab
    /// maps it to a published word (`"tree"` / `"document"` / `"free"`), which
    /// is a different use. The copy path is the second, and two inlined copies of a
    /// scope dispatch is how two consumers of one rule come to disagree
    /// (R1977). One home, both readers.
    #[must_use]
    pub fn holders_of(
        &self,
        tree: TreeId,
        body: &NodeBody<K>,
        label: &str,
    ) -> Vec<(TreeId, NodeId)> {
        match self.naming_of(body) {
            Naming::Free => Vec::new(),
            Naming::InTree => self
                .nodes_labelled(tree, label)
                .into_iter()
                .map(|node| (tree, node))
                .collect(),
            Naming::InDocument => self.nodes_labelled_anywhere(label),
        }
    }

    /// ★★★★★ R1985 — **the first `{stem}-NN` nobody in this body's scope
    /// holds.**
    ///
    /// The one derivation of *a name that is free*, which the node lab had a
    /// private copy of since R1935 and the copy path needed a second. The
    /// screen's copy asked [`Document::node_labelled`], which answers `None`
    /// when **more than one** node holds a name — so a name two cards already
    /// answered to read as free, and the fresh name it minted was a third
    /// collision. This asks [`Self::holders_of`], which counts.
    ///
    /// Always suffixed, never the bare stem: a caller asking for a fresh name
    /// has no name yet, and "part" reading as the first of the parts is what
    /// that screen has published since it existed.
    ///
    /// `avoid` is names that are not in the document **yet** — one insertion
    /// places many copies and the second one cannot be told about the first by
    /// asking the document, because neither is there while the plan is being
    /// made. Pass an empty set when there is no such batch.
    #[must_use]
    pub fn numbered_label(
        &self,
        tree: TreeId,
        body: &NodeBody<K>,
        stem: &str,
        avoid: &BTreeSet<String>,
    ) -> String {
        for index in 1u32.. {
            let candidate = format!("{stem}-{index:02}");
            if self.holders_of(tree, body, &candidate).is_empty() && !avoid.contains(&candidate) {
                return candidate;
            }
        }
        unreachable!("the range is unbounded")
    }

    /// ★★★★★ R1985 — **what this body's kind does about a name already held.**
    ///
    /// Only a [`NodeBody::Kind`] has a kind to ask. The bodies this crate owns
    /// — a group instance, a delay, a beacon, an interface end — take the
    /// supplied answer, because the policy is an application's to declare and
    /// they are not the application's nodes.
    #[must_use]
    pub fn copying_of(&self, body: &NodeBody<K>) -> Copying {
        match body {
            NodeBody::Kind(kind) => kind.copying(),
            _ => Copying::Renamed,
        }
    }

    /// ★★★★★ R1985 — **the name a COPY of this node should take here**, or
    /// `None` when it may keep the one it has.
    ///
    /// # Why a copy cannot simply keep its name
    ///
    /// Measured at this round's open, on the crate's own fixture: duplicating a
    /// node labelled `Total` left **two** nodes answering to it,
    /// [`Document::node_labelled`] then answered `None` — so the card a screen
    /// shows by name became unaddressable — and
    /// [`Document::may`]`(Act::Rename(copy, Some("Total")))` answered
    /// `LabelTaken`. The crate's own permission surface said the state may not
    /// be created, and its own copy verb created it. Two paths deriving one
    /// rule, which is R1977's class, and [`Document::validate`] reported
    /// nothing.
    ///
    /// # The rule is the references', and they disagree — so the kind chooses
    ///
    /// The DCC renames the copy (`node_unique_name` on the destination tree,
    /// separator `.`) and says nothing. The engine has TEN paste-permission
    /// overriders and exactly ONE reaches its answer this way: it **refuses
    /// the paste** when the destination already answers to the name, gathering
    /// every name in use in the blueprint and declining.
    ///
    /// ⚠ One of ten, not the norm — stated because this round's closing audit
    /// caught it claiming *commonest* in seven places without ever having
    /// measured it. It is nevertheless the right rule to reproduce, and that
    /// too is measured: it is the only one of the ten that CAN be. The hook's
    /// base answer asks whether the node suits the target GRAPH, and a tree
    /// here has no kind at all (`schema::GetGraphType` is still absent from
    /// the census), so that one is not expressible.
    ///
    /// Both are defensible and neither can express the other, so
    /// [`Copying`] is what a kind declares and this is where it is read.
    ///
    /// The stem drops a trailing `-NN` first, so a copy of a copy is
    /// `Total-02` and not `Total-01-01`. The DCC does the same thing by
    /// splitting on its separator.
    #[must_use]
    pub fn copy_label(
        &self,
        tree: TreeId,
        body: &NodeBody<K>,
        was: &str,
        avoid: &BTreeSet<String>,
    ) -> Option<String> {
        if self.holders_of(tree, body, was).is_empty() && !avoid.contains(was) {
            return None;
        }
        Some(self.numbered_label(tree, body, Self::stem_of(was), avoid))
    }

    /// `was` with a trailing `-NN` taken off, which is the stem a further copy
    /// numbers from.
    ///
    /// ★ R1986 made it crate-visible rather than writing a second one: the
    /// definition-tree copy path needs the same stem, and R1963's rule is that
    /// what matters about a property spelled in two places is what holds them
    /// together. This is what holds them together.
    pub(crate) fn stem_of(was: &str) -> &str {
        match was.rsplit_once('-') {
            Some((head, tail))
                if !head.is_empty()
                    && !tail.is_empty()
                    && tail.bytes().all(|b| b.is_ascii_digit()) =>
            {
                head
            }
            _ => was,
        }
    }
}

/// ★★★★★ R1985 — **what a copy of this node does about a name its destination
/// already holds.**
///
/// # Two references, one question, opposite answers
///
/// Measured rather than invented, at both trees' paste path:
///
/// * The DCC copies a node by giving it a name unique in the destination tree —
///   its copy helper calls its own unique-name routine unless the caller passed
///   a name or asked for duplicates. The copy lands, under a name nobody chose.
/// * The engine asks each node *may you be pasted here*, and one of its ten
///   overriding classes answers by gathering every name the destination already
///   uses and declining if this one is among them. The copy does not land, and
///   at the call site that refusal **breaks the node's links and continues** —
///   so a person is left with a node missing its wires and nothing said.
///
/// Neither can express the other: the DCC has no way to say *this one must not
/// be copied at all*, and the engine's answer is a `bool`, so *I refuse* and
/// *nothing to say* are the same value.
///
/// # How this passes both
///
/// A kind declares which, the document enforces it — the shape [`crate::Berth`]
/// took for the same reason (R1980) — and **either outcome is reported**:
/// [`crate::Inserted::renamed`] says what each copy was renamed from and to,
/// and [`crate::InsertError::NameTaken`] names the node, the name and the
/// holder. The DCC reports neither and the engine reports neither.
///
/// ⚠ This is only asked where the kind's [`Naming`] requires uniqueness. A
/// [`Naming::Free`] name is a caption, so there is no clash to have a policy
/// about, and [`Document::holders_of`] answers nothing there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Copying {
    /// Take a name of its own, derived from the original's. The DCC's answer,
    /// and the supplied one — a person who pressed duplicate asked for a
    /// second node, not for a refusal.
    #[default]
    Renamed,
    /// Refuse to land. The engine's answer, for a node whose name is the thing
    /// it IS — an event, an entry point, a graph's single result — where a
    /// second one under another name would not be what was asked for.
    Refused,
}
