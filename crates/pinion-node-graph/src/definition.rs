//! ★★★★★ R1986 — **the three verbs a definition tree has of its own**, and the
//! one question that decides all three.
//!
//! A definition is a tree in the document that other trees instantiate. Until
//! this round a person could *make* one ([`Document::add_definition`]) and
//! *remove* one ([`Document::remove_definition`], R1944), and that was the
//! whole of it: its name was fixed at the moment it was added, and the only way
//! to copy one was [`Document::fork_definition`], which needs an **instance** to
//! rebind. This module is the missing half — rename, duplicate on its own, and
//! the permission surface all three are decided by.
//!
//! # What the reference does, measured at its header, its consumers and every
//! overrider
//!
//! Five census rows live here, and reading them together is what shaped the
//! module, because separately each one looks like a small `bool`.
//!
//! ⚠ **The counts below are over the WHOLE tree, engine and shipped plugins,
//! and that matters**: this round's first draft measured the engine source
//! alone, which understated four of the five and made one of them outright
//! false. Its own closing audit caught it. R1750's rule, on the *presence*
//! side — measure widely for what is there, not only for what is not.
//!
//! * **may this be deleted** is published on the *palette entry*, not on the
//!   graph, and answers `false` when nobody overrides it. Measured across the
//!   whole tree: **nobody overrides it** — the only two sites are the
//!   declaration and one consumer. That consumer reaches it as the last
//!   `else if` of a chain that has already handled every subject that matters,
//!   and a **definition graph** is handled by that chain's **FIRST** branch —
//!   ⚠ **six** branches earlier, re-counted at this round's close after the
//!   draft wrote *two*: the chain runs graph, delegate, variable, event, local
//!   variable, blueprint variable, category, and only then the hook. That
//!   first branch reads a stored bit on the graph. So the published permission
//!   hook is, for this subject, dead code, and the real answer is a flag.
//! * **may this be renamed** is published in the same place, answers `true` by
//!   default, and has **four** overriders: three refuse (a placeholder entry, a
//!   state-machine node, a visual-scripting event) and one answers **yes**,
//!   restating the default.
//! * **rename it** is published on the schema, answers `bool` meaning *I
//!   handled it*, and has exactly **one** overrider in the whole tree, in a
//!   plugin — with a finding of its own: on a **root** graph it performs the
//!   rename through its client host and then **falls out to `return false`**,
//!   so its caller's fallback rename runs on top of a rename that already
//!   happened. Only its non-root branch answers `true`. Every other schema
//!   falls through. And the caller gates the whole thing on *may be deleted OR
//!   may be renamed* — ⚠ **a rename permitted because deletion is**, which is
//!   what one decision spelled in three places drifts into.
//! * **may this be duplicated** is the hook here with the most overriders —
//!   **eight** — and they answer *by what the graph IS*: one reads the graph's
//!   TYPE and admits two of them, one refuses the root animation graph **by
//!   name** and by class, and **six** answer a flat no. Its verb's supplied
//!   answer is a null pointer — a duplicate that produced nothing, with no
//!   reason.
//! * **the graph is going away** is a notification, and reading all **six**
//!   overriders is what said what it is FOR: every one of them finds the node
//!   bound to the departing graph and deletes it. One does more — it also drops
//!   the graph from the recently-edited list and clears the breakpoints inside
//!   it. ⇒ the capability is *everything keyed to this definition is told,
//!   before it goes*.
//!
//! # The four measured ways this passes them
//!
//! 1. **One decision, not three.** [`Document::may_definition`] IS the
//!    decision, and the three verbs begin by asking it — R1920's shape, for the
//!    same reason: there the permission is spelled once on the entry, once as a
//!    stored bit, and once more in the rename consumer, and the three are free
//!    to disagree. They already do.
//! 2. **A refusal carries its reason and its sites.** [`DefinitionError`] names
//!    the root, the missing tree, the empty name, and — for a removal that
//!    would destroy work — **every instance that still stands for it**. Every
//!    answer over there is a `bool`.
//! 3. **What a removal will take is answerable BEFORE it takes it.**
//!    [`Document::would_remove_definition`] computes the same [`RemovedTree`]
//!    the removal reports, from the same derivation, so a caller holding
//!    anything keyed to a definition can read the departing trees while they are
//!    still there. The reference's notification is told one graph at a time and
//!    cannot see what the removal will cascade into, because its own path does
//!    not cascade at all.
//! 4. **A copy takes a name of its own.** R1985 measured this on nodes and the
//!    argument is the tree's too: [`Document::duplicate_definition`] numbers
//!    from the stem, so duplicating `Filter` gives `Filter-01` and duplicating
//!    that gives `Filter-02` rather than `Filter-01-01`.
//!
//! # ⚠ Two definitions MAY share a name, and that is load-bearing
//!
//! Not an oversight and not a rule waiting to be added: [`Document::insert`]
//! with [`Definitions::Fork`](crate::Definitions::Fork) adds a carried
//! definition **under the name it arrives with**, and the derivation that
//! decides whether a carried definition is one the destination already has
//! reads that name as one of its conjuncts. A uniqueness rule here would force
//! the fork path to rename, a renamed definition would then stop matching on
//! the next insert, and the document would grow a copy per paste. The
//! reference's own fallback rename *does* refuse a taken name; this is a
//! measured divergence, and the price is paid where it is felt:
//! [`Document::definition_named`] answers `None` when more than one definition
//! holds a name, exactly as [`Document::node_labelled`] does, so a name that
//! does not identify addresses nothing rather than addressing whichever came
//! first.

use std::fmt;

use crate::model::{Document, NodeBody, NodeId, NodeKind, ROOT, TreeId};

/// What a person may ask of a **definition tree** — as opposed to
/// [`Act`](crate::Act), which is asked of a node inside one.
///
/// Kept apart from `Act` rather than folded into it because the subject is
/// different: `may(tree, act)` asks about a node *in* `tree`, and this asks
/// about `tree` itself. One enum covering both would make the first argument
/// mean two things, which is the ambiguity R1978 spent a round making
/// unrepresentable on another axis.
///
/// Each arm carries the value the answer depends on, for R1920's reason: a
/// removal is refused for what would go with it, and a rename for the name it
/// would take, so an arm that left the value out could only answer half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionAct<'a> {
    /// Remove it, with the caller's answer about the instances that stand for
    /// it.
    Remove(Used),
    /// Give it this name.
    Rename(&'a str),
    /// Copy it, with no instance to carry the copy — the reference's
    /// *duplicate a graph AS a graph*.
    Duplicate,
}

/// ★★★★★ R1944 — what a caller wants done about the instances of a definition
/// it is removing.
///
/// Two arms and the caller must pick one, which is the whole decision: the
/// reference has no such choice — its delete-a-graph path removes the nodes
/// bound to that graph unconditionally and says nothing about having done it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Used {
    /// Refuse while anything still stands for this definition, and name what
    /// does. The safe answer, and the default a screen should offer: a
    /// reference that disappears with its target is data loss the person did
    /// not ask for.
    Refuse,
    /// Remove them too, and REPORT what went.
    TakeThemToo,
}

/// ★★★★★ R1944 — why a verb could not be done to a definition.
///
/// ★ R1986 renamed this from the removal's own error: the three definition
/// verbs are decided by [`Document::may_definition`], so a second vocabulary
/// for the other two would be the *two sources of one truth* R1920 built that
/// surface to end.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DefinitionError {
    /// No such tree.
    NoSuchTree(TreeId),
    /// The root is where a document lives; it is not one of its definitions.
    ///
    /// The refusal for all three verbs, and each for its own reason: removing
    /// it would leave nothing, renaming it is renaming the document rather than
    /// a definition, and a copy of it would BE a definition — a different act
    /// from duplicating one.
    TheRoot,
    /// Something still stands for this definition, and this says what.
    ///
    /// ⚠ The SITES, not a count. `instance_count` could already answer *how
    /// many*, and a person told "3 instances" still has to find them; the
    /// reference answers neither, because it never refuses.
    StillUsed {
        /// Every instance, as (the tree it is in, the node).
        by: Vec<(TreeId, NodeId)>,
    },
    /// A rename to a name that is empty once trimmed.
    ///
    /// The tree's spelling of `EditError::LabelEmpty`, and refused for the same
    /// reason: a nameless definition cannot be shown in a palette or addressed
    /// by anything.
    NameEmpty {
        /// The definition that would have taken it.
        tree: TreeId,
    },
}

impl fmt::Display for DefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {tree}"),
            Self::TheRoot => f.write_str("the root tree is not a definition"),
            Self::StillUsed { by } => write!(
                f,
                "{} node(s) still stand for it, the first in tree {}",
                by.len(),
                by.first().map_or(ROOT, |(tree, _)| *tree)
            ),
            Self::NameEmpty { tree } => write!(f, "a definition ({tree}) needs a name"),
        }
    }
}

impl std::error::Error for DefinitionError {}

/// ★★★★★ R1944 — what a removal took with it, and R1986 — what one *would*.
///
/// Returned rather than done silently, which is the measured difference: the
/// reference's path removes every node bound to the graph and answers `void`,
/// so a caller cannot undo, report or even count what it cost.
///
/// The three fields answer three different questions, and the union of the two
/// node lists is exactly the set of nodes that no longer exist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemovedTree {
    /// Every node that **stood for** a removed definition, as (the tree it was
    /// in, the node).
    pub instances: Vec<(TreeId, NodeId)>,
    /// Every definition that went.
    ///
    /// ⚠ More than the one asked about. A definition can hold instances of
    /// ANOTHER definition, so removing one can orphan a chain. Named rather
    /// than left: the reference's path does not look, so a nested definition
    /// simply stays in the document with nothing pointing at it.
    pub definitions: Vec<TreeId>,
    /// ★★★★★ R1986 — every node that was **inside** a removed definition, as
    /// (the tree it was in, the node).
    ///
    /// The half a report of tree ids cannot give. A side table keyed by (tree,
    /// node) — a per-card form, a breakpoint, an open tab — has entries for the
    /// cards *inside* a definition, and once the tree is gone there is nothing
    /// left to derive them from. The reference's notification hands over one
    /// graph and its listeners walk it themselves; here it is already counted.
    pub nodes: Vec<(TreeId, NodeId)>,
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1986 — **may this be done to this definition?**, asked *before*
    /// doing it.
    ///
    /// The one decision the three definition verbs are made by:
    /// [`Self::remove_definition`], [`Self::rename_definition`] and
    /// [`Self::duplicate_definition`] each begin here, so an editor that asks
    /// first and an editor that just tries cannot get different answers.
    ///
    /// # Errors
    ///
    /// [`DefinitionError`] — whatever the corresponding verb would answer.
    pub fn may_definition(
        &self,
        definition: TreeId,
        act: DefinitionAct<'_>,
    ) -> Result<(), DefinitionError> {
        // ★ The two refusals every act shares, and they are checked in this
        // order on purpose: the root EXISTS, so asking "is it there" first
        // would answer `Ok` for a subject that can never take any of these.
        if definition == ROOT {
            return Err(DefinitionError::TheRoot);
        }
        if self.tree(definition).is_none() {
            return Err(DefinitionError::NoSuchTree(definition));
        }
        match act {
            DefinitionAct::Remove(Used::Refuse) => {
                let standing = self.instances_of(definition);
                if standing.is_empty() {
                    Ok(())
                } else {
                    Err(DefinitionError::StillUsed { by: standing })
                }
            }
            // ★★★★★ The destructive arm is not a weaker check, it is a
            // DIFFERENT question: the caller has already said what happens to
            // the instances, so there is nothing left to refuse it for.
            DefinitionAct::Remove(Used::TakeThemToo) | DefinitionAct::Duplicate => Ok(()),
            DefinitionAct::Rename(name) => {
                Self::wanted_definition_name(definition, name).map(|_| ())
            }
        }
    }

    /// The name a rename would actually take, trimmed, or why it cannot.
    ///
    /// One home for the trim, because the permission and the verb both need it
    /// and two spellings of one rule is how two readers of it come to disagree
    /// (R1977). The node axis answers the same question in its own
    /// `wanted_label`; this is the tree's, and it does **not** carry that one's
    /// uniqueness half — see this module's header for why a definition name is
    /// allowed to be shared.
    fn wanted_definition_name(definition: TreeId, name: &str) -> Result<String, DefinitionError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            Err(DefinitionError::NameEmpty { tree: definition })
        } else {
            Ok(trimmed.to_owned())
        }
    }

    /// ★★★★★ R1986 — **what removing this definition WOULD take**, computed
    /// without taking it.
    ///
    /// This is the crate's answer to the reference's *the graph is going away*
    /// notification, and it is a question rather than a callback for the reason
    /// every other question here is: the model answers, and what to do about it
    /// is the caller's. A screen keeping anything keyed to a definition — a
    /// per-card form, an open tab, a breakpoint — reads this, forgets what it
    /// names, and only then removes.
    ///
    /// ⚠ **The report is the whole population**, not just the definition asked
    /// about. A definition can hold instances of another, so removing one
    /// cascades; the reference's notification is handed one graph and its own
    /// path does not cascade, so a listener there cannot be told about the
    /// chain at all.
    ///
    /// [`Self::remove_definition`] is this plus applying it, so the two cannot
    /// disagree about what went.
    ///
    /// # Errors
    ///
    /// [`DefinitionError`], the same [`Self::may_definition`] answers.
    pub fn would_remove_definition(
        &self,
        definition: TreeId,
        used: Used,
    ) -> Result<RemovedTree, DefinitionError> {
        self.may_definition(definition, DefinitionAct::Remove(used))?;
        // ⚠ The orphan rule reads the state BEFORE anything goes, because what
        // tells an orphan this removal WOULD MAKE from one already standing
        // alone is that it HAD an instance: a definition authored and not yet
        // placed is a legitimate state a removal must not sweep up.
        let mut going: Vec<TreeId> = vec![definition];
        loop {
            let orphaned: Vec<TreeId> = self
                .definitions()
                .map(|held| held.id)
                .filter(|id| !going.contains(id))
                .filter(|id| {
                    let standing = self.instances_of(*id);
                    !standing.is_empty() && standing.iter().all(|(tree, _)| going.contains(tree))
                })
                .collect();
            if orphaned.is_empty() {
                break;
            }
            going.extend(orphaned);
        }
        going.sort_unstable();
        let mut instances: Vec<(TreeId, NodeId)> =
            going.iter().flat_map(|id| self.instances_of(*id)).collect();
        instances.sort_unstable();
        instances.dedup();
        let mut nodes: Vec<(TreeId, NodeId)> = going
            .iter()
            .filter_map(|id| self.tree(*id))
            .flat_map(|held| held.nodes().map(move |node| (held.id, node.id)))
            .collect();
        nodes.sort_unstable();
        Ok(RemovedTree {
            instances,
            definitions: going,
            nodes,
        })
    }

    /// ★★★★★ R1944 — **remove a definition from the document**, saying what
    /// went with it.
    ///
    /// # What forced it, measured in the reference
    ///
    /// Its schema is asked to delete a graph, and the editor falls back to its
    /// own procedure when the schema declines. Counted: **one declaration
    /// (answering NO), ZERO overriders, one consumer** — so that extension
    /// point has never once been taken, and every deletion goes down the
    /// fallback. R1938's shape: a hook whose refusal is never exercised is a
    /// hook nobody has had to think about.
    ///
    /// The fallback is what the capability really is, and it does three things
    /// this answers differently:
    ///
    /// * **It removes every node bound to that graph, unconditionally**, and
    ///   answers `void`. A caller cannot report what it cost, and a person who
    ///   deleted a definition in use loses the nodes that used it without being
    ///   asked. Here [`Used`] makes the caller choose, and `Refuse` names the
    ///   sites.
    /// * **Whether a graph may go at all is a FLAG on the graph**, so *why not*
    ///   has no answer. Here the refusals are named ([`DefinitionError`]).
    /// * **It does not look for definitions orphaned by the removal.** A
    ///   definition can hold instances of another, so removing one can leave a
    ///   chain with nothing pointing at it; those are removed and REPORTED here.
    ///
    /// ★ R1986 — the census it reports is [`Self::would_remove_definition`]'s,
    /// asked first and then applied, so *what will go* and *what went* are one
    /// derivation rather than two that agree by care.
    ///
    /// # Errors
    ///
    /// [`DefinitionError`].
    pub fn remove_definition(
        &mut self,
        definition: TreeId,
        used: Used,
    ) -> Result<RemovedTree, DefinitionError> {
        let went = self.would_remove_definition(definition, used)?;
        // ★ Through `remove_node`, not by reaching into the tree: that verb
        // already drops the links a removed node was on and reports what it
        // orphaned, and a second removal path here would be a second set of
        // invariants free to drift from it.
        //
        // ⚠ Only the instances in trees that SURVIVE. A node inside a tree that
        // is about to be dropped goes with the tree, and asking `remove_node`
        // to take it first would be work whose result nothing can observe.
        for (tree, node) in &went.instances {
            if !went.definitions.contains(tree) {
                let _ = self.remove_node(*tree, *node);
            }
        }
        for id in &went.definitions {
            self.drop_tree(*id);
        }
        Ok(went)
    }

    /// ★★★★★ R1986 — **give a definition another name**, answering the one it
    /// had.
    ///
    /// The previous name is the answer rather than `()` for the reason
    /// [`RemovedTree`] is not `void`: a caller that cannot say what it changed
    /// cannot undo it, report it, or put it in a log. The reference answers
    /// `bool`, meaning *the schema handled this*, so a refusal and *nothing to
    /// say* arrive as the same value — and since nothing overrides it, the
    /// value is always the second one.
    ///
    /// # Errors
    ///
    /// [`DefinitionError::TheRoot`] — the root is the document, not one of its
    /// definitions; [`DefinitionError::NoSuchTree`];
    /// [`DefinitionError::NameEmpty`].
    pub fn rename_definition(
        &mut self,
        definition: TreeId,
        name: &str,
    ) -> Result<String, DefinitionError> {
        self.may_definition(definition, DefinitionAct::Rename(name))?;
        let wanted = Self::wanted_definition_name(definition, name)?;
        let held = self
            .tree_mut(definition)
            .ok_or(DefinitionError::NoSuchTree(definition))?;
        Ok(std::mem::replace(&mut held.name, wanted))
    }

    /// ★★★★★ R1986 — **copy a definition on its own**, with no instance to
    /// carry the copy, and answer the copy's id.
    ///
    /// [`Self::fork_definition`] is the other half of this axis and needs an
    /// instance: it copies the definition that instance names and rebinds *that
    /// instance* to the copy. There is no instance here, which is exactly the
    /// case the reference gates with its own *may this graph be duplicated* —
    /// a graph duplicated **as a graph**, from a palette listing the document's
    /// definitions.
    ///
    /// ★★★★★ The copy takes a **name of its own**, numbered from the stem
    /// (R1985's finding, applied to the tree): a copy under the original's name
    /// would be indistinguishable from it in a palette, and — measured —
    /// indistinguishable to the derivation that decides whether a carried
    /// definition is one this document already holds. Two definitions that are
    /// identical and identically named are the same definition to that
    /// derivation, right up until someone edits one.
    ///
    /// The copy is **shallow in the one way that matters**: a node inside it
    /// that stands for another definition still stands for that same one. The
    /// reference's clone does the same, and the alternative — copying the whole
    /// reachable chain — is what [`Definitions::Fork`](crate::Definitions::Fork)
    /// is for, on the fragment axis, where the caller says which they want.
    ///
    /// # Errors
    ///
    /// [`DefinitionError::TheRoot`] — the root is not a definition, and a copy
    /// of it would be one; [`DefinitionError::NoSuchTree`].
    pub fn duplicate_definition(&mut self, definition: TreeId) -> Result<TreeId, DefinitionError> {
        self.may_definition(definition, DefinitionAct::Duplicate)?;
        let was = self
            .tree(definition)
            .map(|held| held.name.clone())
            .ok_or(DefinitionError::NoSuchTree(definition))?;
        let fresh = self.fresh_definition_name(&was);
        let copy = self
            .copy_tree(definition)
            .ok_or(DefinitionError::NoSuchTree(definition))?;
        if let Some(held) = self.tree_mut(copy) {
            held.name = fresh;
        }
        Ok(copy)
    }

    /// Every definition answering to `name`.
    ///
    /// The tree-level peer of [`Self::nodes_labelled`], and it exists for the
    /// same reason: a name that more than one thing holds is a fact a caller
    /// has to be able to see, rather than one a lookup hides by picking the
    /// first.
    #[must_use]
    pub fn definitions_named(&self, name: &str) -> Vec<TreeId> {
        self.definitions()
            .filter(|held| held.name == name)
            .map(|held| held.id)
            .collect()
    }

    /// The one definition answering to `name`, or `None` when none does **or
    /// more than one does**.
    ///
    /// ★★★★★ The ambiguous case answers `None` deliberately, which is
    /// [`Self::node_labelled`]'s discipline one level up: a name two
    /// definitions hold does not address either of them, and a lookup that
    /// returned the first would make *which* one a person edited depend on
    /// insertion order. R1983 measured what a fallback without that discipline
    /// costs.
    #[must_use]
    pub fn definition_named(&self, name: &str) -> Option<TreeId> {
        match self.definitions_named(name).as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// The first `{stem}-NN` no definition answers to, the stem being `was`
    /// with any trailing `-NN` taken off.
    ///
    /// The tree's spelling of [`Self::numbered_label`]. Not that function
    /// itself, and the reason is measured rather than stylistic: that one is
    /// keyed on a [`NodeBody`], because a node's scope is a property of its
    /// kind, and a tree has no kind to ask — the reference decides its own
    /// duplicate permission by exactly that, and it is a census row this crate
    /// still owes. The stem derivation IS shared, one home and both readers.
    fn fresh_definition_name(&self, was: &str) -> String {
        let stem = Self::stem_of(was);
        for index in 1u32.. {
            let candidate = format!("{stem}-{index:02}");
            if self.definitions_named(&candidate).is_empty() {
                return candidate;
            }
        }
        unreachable!("the range is unbounded")
    }

    /// Every instance of `definition`, as (the tree it is in, the node).
    ///
    /// ★ Moved here at R1986 from the palette module, where R1944 left it: it
    /// is the question every one of these verbs is decided by, and it belongs
    /// beside them.
    #[must_use]
    pub fn instances_of(&self, definition: TreeId) -> Vec<(TreeId, NodeId)> {
        let mut found: Vec<(TreeId, NodeId)> = self
            .trees()
            .flat_map(|held| {
                held.nodes()
                    .filter(|node| node.body == NodeBody::Group(definition))
                    .map(move |node| (held.id, node.id))
            })
            .collect();
        found.sort_unstable();
        found
    }
}
