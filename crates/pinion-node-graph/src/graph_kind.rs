//! ★★★★★ R1999 — **a tree says what kind of graph it is**, and one declaration
//! per node kind says which of those it is at home in.
//!
//! # What the reference does, measured at its own header rather than summarised
//!
//! Its graph schema publishes a hook answering *what type of graph is this*,
//! and the measurement changed three things about what is built here.
//!
//! * **The vocabulary is a fixed five-member enumeration** declared beside the
//!   schema base class, and the comment written directly above the hook says
//!   in its own words that this is too specific to one editor to be there and
//!   should be refactored. Every application in that engine — a material graph,
//!   a sound cue, a behaviour tree, a state machine — is answered out of one
//!   visual-scripting vocabulary, of which it uses at most one member.
//! * **The supplied answer ignores its argument.** The base body is a single
//!   `return` of the first member, so the hook answers *function* for a graph
//!   that does not exist as readily as for one that does. Measured across the
//!   whole editor source, **three** schemas override it: two answer a constant
//!   apiece, and the visual-scripting one walks the graph's owner chain and
//!   reports which of the owning document's three lists holds it —
//!   falling through to the base when it is in none of them. So *this is a
//!   function graph* and *I could not classify this* are one value there, and a
//!   caller cannot tell them apart.
//! * **Its consumers are hand-written comparisons.** Measured at R1999: **53**
//!   call expressions in the engine source read the kind (60 occurrences less
//!   the 7 signatures), and grouped by the function that makes them, the
//!   largest single group by a factor of four — **sixteen** calls in
//!   **fifteen** node classes, against **four** for the next — sits inside that
//!   class's own *are you compatible with this graph* body, each re-writing the
//!   same comparison. A node type added afterwards is compatible with every
//!   graph until somebody remembers to edit one more of them.
//!
//! # What is built, and the four measured ways it is better
//!
//! 1. **The vocabulary is the taxonomy's** ([`NodeKind::Graph`]), like
//!    [`NodeKind::Type`] and [`NodeKind::Value`] already are. The reference's
//!    own comment asks for exactly this and has not had it.
//! 2. **The kind is stored, not re-derived from where the document keeps the
//!    tree.** A tree with no classification is not silently reported as the
//!    first member of the enumeration; there is no such tree, because
//!    [`Document::add_definition_of`] takes the kind and
//!    [`Document::add_definition`] uses the one the taxonomy declares unchosen.
//!    The only `None` here is *no such tree*, which is a different fact.
//! 3. **Compatibility is one declaration, read by the refusal and by the
//!    offer.** [`NodeKind::at_home`] answers an [`Admitted`](crate::Admitted)
//!    over the taxonomy's graph kinds; [`Document::admits`] refuses against it
//!    (so every verb that goes through it does), and [`Document::at_home`] is
//!    the same predicate a chooser filters with. A palette that offered a kind
//!    the edit would refuse is unrepresentable — the arrangement R1933 built
//!    for socket types, and the two-oracle defect R1884 recorded the cost of.
//! 4. **A re-classification says what it left behind.**
//!    [`Document::not_at_home`] lists the nodes a tree now holds that its kind
//!    does not admit, and [`Document::validate`] reports them. Nothing in the
//!    reference asks that question at all: changing which list holds a graph
//!    changes its type there with no pass over what is already in it.
//!
//! # What this deliberately does NOT do
//!
//! [`Document::set_graph_kind`] does not re-check the nodes already placed, for
//! [`Document::set_admitted`]'s reason (R1933): narrowing what a tree already
//! holds is a judgement about existing content, and an edit that silently
//! deleted nodes would take their links with them. It is **reported** rather
//! than prevented, by the two readers named above.
//!
//! ⚠ **The carry from R1998, applied here.** A hook whose return type is wider
//! than the taxonomy's vocabulary has parts no fixture built out of that
//! vocabulary can reach (R1845's eighth). [`NodeKind::at_home`] answers
//! `Admitted<Self::Graph>`, which is the vocabulary plus two shapes:
//! [`Admitted::Anything`](crate::Admitted::Anything) and an **empty**
//! `These`, the kind at home in no graph at all. Naming graph kinds cannot
//! produce the empty list, so the census test constructs it directly rather
//! than assuming a taxonomy will wander into it.

use crate::model::{Document, EditError, NodeBody, NodeId, NodeKind, TreeId};

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1999 — **what kind of graph `tree` is**, or `None` when there is
    /// no such tree.
    ///
    /// The reference's hook has no way to say the second thing: its supplied
    /// body ignores the graph it is handed and answers the first member of its
    /// enumeration, so a caller holding nothing at all is told *function*.
    #[must_use]
    pub fn graph_kind(&self, tree: TreeId) -> Option<&K::Graph> {
        self.tree(tree).map(|host| &host.kind)
    }

    /// The identity token for `tree`'s kind, for a refusal that has to name it.
    ///
    /// Crate-private and `Debug`-derived on purpose: [`EditError`] is not
    /// generic over the taxonomy, and a sentence in an application's own words
    /// is that application's to write — the crate names *which* graph kind
    /// refused, and [`graph_kind`](Self::graph_kind) hands over the value
    /// itself for anyone who wants to phrase it.
    pub(crate) fn graph_kind_token(&self, tree: TreeId) -> String {
        self.graph_kind(tree)
            .map_or_else(|| "no such".to_owned(), |kind| format!("{kind:?}"))
    }

    /// ★★★★★ R1999 — **re-classify `tree`.**
    ///
    /// ⚠ Does not re-check the nodes already in it. See the module header: the
    /// nodes a narrowing left behind are [`not_at_home`](Self::not_at_home)'s
    /// answer and a [`Violation`](crate::Violation) of
    /// [`validate`](Document::validate), not a silent deletion.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchTree`] when `tree` is not in the document.
    pub fn set_graph_kind(&mut self, tree: TreeId, kind: K::Graph) -> Result<(), EditError> {
        let host = self.tree_mut(tree).ok_or(EditError::NoSuchTree(tree))?;
        host.kind = kind;
        Ok(())
    }

    /// ★★★★★ R1999 — **whether `kind` is at home in `tree`**: the predicate a
    /// chooser filters its offers with.
    ///
    /// ★ THE SAME question [`admits`](Document::admits) refuses on, so an offer
    /// and a refusal cannot disagree. In the reference the offer is a palette
    /// filter and the refusal is a per-node-type virtual, and nothing relates
    /// them.
    ///
    /// `true` for a tree that is not there — the same answer as a graph kind
    /// that restricts nothing, because both mean *this asks nothing of a kind*
    /// and [`Document::admits_type`] already draws the line there.
    #[must_use]
    pub fn at_home(&self, tree: TreeId, kind: &K) -> bool {
        self.graph_kind(tree)
            .is_none_or(|graph| kind.at_home().admits(graph))
    }

    /// ★★★★★ R1999 — the nodes in `tree` whose kind its graph kind does not
    /// admit, ascending by id.
    ///
    /// What a re-classification left behind, answered rather than prevented —
    /// [`Document::unadmitted_ports`]'s shape, and for its reason.
    ///
    /// ⚠ Only [`NodeBody::Kind`] nodes can be in this list. The bodies this
    /// crate owns — a frame, a group instance, an interface end, a delay — are
    /// editor and structural affordances rather than application subject
    /// matter, so a taxonomy has nothing to declare about them and the question
    /// is not asked.
    #[must_use]
    pub fn not_at_home(&self, tree: TreeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        host.nodes()
            .filter(|node| match &node.body {
                NodeBody::Kind(kind) => !kind.at_home().admits(&host.kind),
                _ => false,
            })
            .map(|node| node.id)
            .collect()
    }
}
