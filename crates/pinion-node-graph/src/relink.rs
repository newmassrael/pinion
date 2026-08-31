//! R1681 — moving one **end** of a link that is already there, and R1924 —
//! asking whether it will be taken **before** it moves.
//!
//! Half of a link's life is what happens after it is drawn: it is deleted, it
//! is re-aimed at a different node, it is moved between two ports of the *same*
//! node when that node listens in more than one place. [`Document::disconnect`]
//! answered the first. This module answers the other two, with one verb,
//! because they are one operation over a different socket.
//!
//! Named apart from [`Rewired`](crate::Rewired), which is what a *dissolve*
//! does to the links **around** a node. This is one link, re-linked.
//!
//! # Why it is not disconnect-then-connect
//!
//! Because that is a different operation with the same outcome on a good day.
//!
//! * **Identity.** [`Document::connect`] mints a [`LinkId`]. A link taken out
//!   and put back is therefore a *new* link, and everything holding the old id
//!   — a selection, a breakpoint, an undo entry, a renderer's cache — is now
//!   holding a dangling name. This session watched exactly that happen from the
//!   other direction: a reset that rebuilt its links silently changed a screen
//!   that should not have moved, because the rebuild re-minted the ids.
//! * **Atomicity.** If the destination refuses the wire, the naive pair has
//!   already destroyed the original. The caller asked to *move* a link, and a
//!   refusal that also deletes it is not a refusal.
//! * **The link must not block its own move.** Placement reads the graph, and
//!   the graph contains the link being moved. Re-aiming a link's *other* end
//!   leaves this one where it was, so the arriving link finds that port
//!   crowded — by itself — and displaces it: the same link both surviving and
//!   being replaced.
//!
//! So the link comes out and goes back under its own id at its own position —
//! or, if the vet refuses, nothing is taken out at all and the refusal is
//! returned with its reason intact.
//!
//! # ★★★★★ R1924 — the decision is ONE function, asked at two moments
//!
//! [`Document::may_relink`] answers *would this end be taken there*, moving
//! nothing; [`Document::relink`] moves it. They are not two implementations
//! that have to agree — the second **calls** the first, so agreement is not a
//! property this module maintains. That is the shape R1920 chose for
//! [`may`](Document::may) over node edits, reached again here because the
//! reference splits it the other way: a `Can…` predicate beside a `Try…` that
//! re-decides, two bodies free to drift with nothing able to notice.
//!
//! The refusal vocabulary stays [`ConnectError`] rather than the node edits'
//! `EditError`, because that is what a wire is refused *for*: a type that does
//! not cross, a port that is not there and its arity, or the cycle path that
//! would close. Two questions, two vocabularies, each the one its own verb
//! answers.
//!
//! [`Document::relink_targets`] then asks that one question of **every** socket
//! on the moving end's side and keeps the ones it admits. The reference's
//! equivalent is a boolean asked of a pin — *may relinking start here* — and
//! that boolean is this list being non-empty; what the list adds is the half a
//! hand has to have, which is *where it may go*, and it is a derivation rather
//! than a second rule.
//!
//! ## Where the link is taken out, and why that moved
//!
//! Before R1924 the link was lifted out of the tree **and then** vetted, so
//! that every read of the graph happened without it, and a refusal put it back.
//! The question cannot be asked that way — `may_relink` takes `&self` — so the
//! order is now decide, lift, place.
//!
//! That is safe, and the argument is a proof rather than a measurement. The one
//! rule in `vet` that reads links is the acyclicity walk,
//! `data_path_between(to.node, from.node)`, a **forward** search. Moving the
//! *input* end leaves `from` where it was, and the moved link **leaves**
//! `from.node` — the node the search is looking for; a search that has arrived
//! stops, so an edge out of it cannot change the answer. Moving the *output*
//! end leaves `to` where it was, and the moved link **arrives at** `to.node` —
//! where the search starts; a forward search never uses an edge into its own
//! origin. Neither end's move is visible to that walk.
//!
//! And the property the old order defended comes out stronger rather than
//! weaker: a refused relink now mutates the document **not at all**, where
//! before it mutated twice and leaned on the second to undo the first. That is
//! what `r1924_a_refused_relink_leaves_the_document_untouched` holds — an
//! assertion about the document, where the old shape could only make one about
//! its own bookkeeping.
//!
//! # What the floor does
//!
//! Measured offscreen against the reference toolkit's 6.11.1 build, whose
//! relocation verb is the same question over a different shape:
//!
//! | | reference | here |
//! |---|---|---|
//! | a relocation verb over a **flat** sequence | yes | — |
//! | a relocation verb over an **arbitrary structure** | no — the structural model inherits the base class's `false` | yes |
//! | a persistent handle survives it | yes | yes ([`LinkId`] is unchanged) |
//! | a persistent handle survives remove + re-insert | **no** | no — which is why the verb exists at all |
//! | what a refusal carries | one bit | [`ConnectError`]: the port and its arity, the two types, or the path that would close |
//!
//! The middle row is the gap: the reference offers the verb where the shape is
//! a list and declines it where the shape is a tree, and a graph is neither.
//! The last row is the one a person feels — "no" is not a reason.

use std::fmt;

use crate::model::{ConnectError, Document, Link, LinkId, NodeId, NodeKind, Side, Socket, TreeId};

/// What a relink did (R1681).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relinked {
    /// The link, under the id it had before — the point of the verb.
    pub link: LinkId,
    /// Which end moved: [`Side::Input`] is the consuming end, [`Side::Output`]
    /// the producing one.
    pub end: Side,
    /// Where that end was.
    pub was: Socket,
    /// Where it is now. Equal to [`was`](Self::was) when the move was asked for
    /// and was already true, which is a success and not a refusal.
    pub now: Socket,
    /// The link the moved end displaced on arrival, if the port it landed on
    /// takes one link and already had one.
    pub displaced: Option<Link>,
}

impl Relinked {
    /// Whether the end actually went anywhere.
    #[must_use]
    pub fn moved(&self) -> bool {
        self.was != self.now
    }
}

/// Why an end could not be moved (R1681).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RelinkError<T> {
    /// No such tree.
    NoSuchTree(TreeId),
    /// No such link in that tree.
    NoSuchLink {
        /// The tree.
        tree: TreeId,
        /// The link that is not in it.
        link: LinkId,
    },
    /// The graph will not hold the link where it was aimed.
    ///
    /// Carries the authoring refusal whole rather than flattening it to a
    /// failure: a move refused for a type mismatch and a move refused for a
    /// cycle are fixed by different actions, and this is the difference.
    Refused(ConnectError<T>),
}

impl<T: fmt::Debug> fmt::Display for RelinkError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {tree}"),
            Self::NoSuchLink { tree, link } => write!(f, "tree {tree} has no link {link}"),
            // ★★★★★ R1924 — the refusal SAYS ITSELF. This was `{why:?}` until
            // the round that first put this sentence in front of a person: a
            // hand passing a wire over a card it may not land on read
            // `SelfLink(NodeId(4))` where `ConnectError` had
            // "node 4 cannot feed itself" ready to say. The class is the one
            // R1699 and R1719 already recorded — `Utterance::refused` even
            // names a `DebugSpelling` fault for it — and it survived here
            // because until now nothing but a test ever read this string.
            Self::Refused(why) => write!(f, "the end cannot go there: {why}"),
        }
    }
}

impl<T: fmt::Debug> std::error::Error for RelinkError<T> {}

/// What a relink WOULD do, worked out without doing any of it (R1924).
///
/// Private because it is the shared decision and not a published answer:
/// [`Document::may_relink`] narrows it to *yes or why not*, and
/// [`Document::relink`] is the only thing that acts on the rest.
struct Plan {
    /// Where in the tree's order the link sits, so it can go back there.
    at: usize,
    /// The link as it stands, with its id, its mute and its two ends.
    held: Link,
    /// Where the moving end is now.
    was: Socket,
    /// The producing socket the move would leave it with.
    from: Socket,
    /// The consuming socket the move would leave it with.
    to: Socket,
    /// Which end's limit the arriving link would exceed, from the vet.
    crowded: Option<Side>,
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1924 — the one decision behind both the question and the verb.
    ///
    /// Takes `&self`: that is the whole point, and it is what forced the lift
    /// below the vet. See this module's header for why the vetting rules are
    /// blind to the link being moved, which is what makes reading the graph
    /// *with* it still there the same answer as reading it without.
    fn plan_relink(
        &self,
        tree: TreeId,
        link: LinkId,
        end: Side,
        socket: Socket,
    ) -> Result<Plan, RelinkError<K::Type>> {
        let host = self.tree(tree).ok_or(RelinkError::NoSuchTree(tree))?;
        let at = host
            .links()
            .iter()
            .position(|l| l.id == link)
            .ok_or(RelinkError::NoSuchLink { tree, link })?;
        let held = host.links()[at];
        let (was, from, to) = match end {
            Side::Input => (held.to, held.from, socket),
            Side::Output => (held.from, socket, held.to),
        };
        let crowded = self.vet(tree, from, to).map_err(RelinkError::Refused)?;
        Ok(Plan {
            at,
            held,
            was,
            from,
            to,
            crowded,
        })
    }

    /// ★★★★★ R1924 — **would this end be taken there?**, asked before moving it.
    ///
    /// The reference asks this as `can this connection be relinked to that pin`
    /// and answers with a response object carrying a sentence. Here the answer
    /// is the refusal itself — [`ConnectError`] inside a [`RelinkError`] — so a
    /// hand hovering a port that will not take the wire can be told *the two
    /// types*, or *the port and its arity*, or *the path that would close*,
    /// rather than that it will not work.
    ///
    /// This is not a prediction of [`relink`](Self::relink): it is the same
    /// call [`relink`](Self::relink) makes, so the two cannot answer
    /// differently. Asking changes nothing about the document.
    ///
    /// # Errors
    ///
    /// [`RelinkError`] — exactly what [`relink`](Self::relink) would answer.
    pub fn may_relink(
        &self,
        tree: TreeId,
        link: LinkId,
        end: Side,
        socket: Socket,
    ) -> Result<(), RelinkError<K::Type>> {
        self.plan_relink(tree, link, end, socket).map(|_| ())
    }

    /// ★★★★★ R1924 — **where else this end could go**, in ascending order.
    ///
    /// Every socket on the moving end's own side that
    /// [`may_relink`](Self::may_relink) admits, minus the one it is on: the
    /// question a hand asks is *where else*, and a list that always contained
    /// the current socket could never be empty, so it could never say the one
    /// thing worth saying.
    ///
    /// Empty means the end is stuck — there is nowhere in this tree it may be
    /// re-aimed at. That is the reference's per-pin *may relinking start here*
    /// boolean, which is `!targets.is_empty()`, and this answers the half that
    /// boolean cannot: an editor can light the ports that will take the wire
    /// instead of making the hand find out by trying each one.
    ///
    /// Derived by asking the one rule rather than by re-stating it, so a rule
    /// added to the vet reaches this list on the same commit.
    ///
    /// # Errors
    ///
    /// [`RelinkError::NoSuchTree`] or [`RelinkError::NoSuchLink`]. A socket
    /// that is merely refused is left out of the list, which is what the list
    /// means.
    pub fn relink_targets(
        &self,
        tree: TreeId,
        link: LinkId,
        end: Side,
    ) -> Result<Vec<Socket>, RelinkError<K::Type>> {
        let host = self.tree(tree).ok_or(RelinkError::NoSuchTree(tree))?;
        let held = *host
            .links()
            .iter()
            .find(|l| l.id == link)
            .ok_or(RelinkError::NoSuchLink { tree, link })?;
        let standing = match end {
            Side::Input => held.to,
            Side::Output => held.from,
        };
        let mut nodes: Vec<NodeId> = host.nodes().map(|node| node.id).collect();
        nodes.sort_unstable();
        let mut out = Vec::new();
        for node in nodes {
            let Some(signature) = self.signature(tree, node) else {
                continue;
            };
            let arity = match end {
                Side::Input => signature.inputs.len(),
                Side::Output => signature.outputs.len(),
            };
            for port in 0..u32::try_from(arity).unwrap_or(u32::MAX) {
                let candidate = Socket::new(node, port);
                if candidate == standing {
                    continue;
                }
                if self.may_relink(tree, link, end, candidate).is_ok() {
                    out.push(candidate);
                }
            }
        }
        Ok(out)
    }

    /// Move one end of an existing link to another socket (R1681).
    ///
    /// `end` says which: [`Side::Input`] moves the consuming end (the link's
    /// `to`), [`Side::Output`] the producing one (its `from`). The two words
    /// are the port sides those ends attach to rather than a second vocabulary
    /// for one fact.
    ///
    /// The link keeps its [`LinkId`], its mute and its place in the tree's
    /// order. A refusal leaves the document exactly as it was — see this
    /// module's header for why neither of those is what
    /// [`disconnect`](Self::disconnect) followed by [`connect`](Self::connect)
    /// gives.
    ///
    /// Moving an end to where it already is succeeds and reports
    /// [`Relinked::moved`] as `false`: the caller asked for a state, and the
    /// state holds.
    ///
    /// # Errors
    ///
    /// [`RelinkError`].
    pub fn relink(
        &mut self,
        tree: TreeId,
        link: LinkId,
        end: Side,
        socket: Socket,
    ) -> Result<Relinked, RelinkError<K::Type>> {
        // ★★★★★ R1924 — the same call `may_relink` makes, so a hand that asked
        // first and a hand that just tried cannot be told different things.
        // Nothing has been touched yet when this refuses, which is why the
        // refusal arm below has no undo in it.
        let Plan {
            at,
            held,
            was,
            from,
            to,
            crowded,
        } = self.plan_relink(tree, link, end, socket)?;

        // Out, so the PLACEMENT does not find the moving link crowding the port
        // its other end never left and displace it with itself.
        self.lift(tree, at);
        let displaced = self.place(
            tree,
            Link {
                id: held.id,
                from,
                to,
                muted: held.muted,
            },
            crowded,
            Some(at),
        );
        Ok(Relinked {
            link: held.id,
            end,
            was,
            now: socket,
            displaced,
        })
    }
}
