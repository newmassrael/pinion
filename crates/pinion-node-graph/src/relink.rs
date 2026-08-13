//! R1681 — moving one **end** of a link that is already there.
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
//! So the link comes out, the graph is read **without** it, and it goes back
//! under its own id at its own position — or, if the vet refuses, it goes back
//! exactly as it was and the refusal is returned with its reason intact.
//!
//! ★ Measured, and narrower than it first looked: taking the link out before
//! [`Document::vet`] rather than merely before the placement is **not
//! observable today**, and a counterfactual that moved the lift down between
//! the two left the whole suite green. The four vetting rules are provably
//! blind to the link being moved, in an acyclic graph: a path that closed a
//! cycle *through* the moved link would have to reach that link's producing
//! node before arriving at its consuming one, which is a cycle the document
//! already refused. It is the **placement** that can see it, and the placement
//! is what the failing counterfactual pins. The lift stays where it is because
//! the scope it names — every read of the graph happens without the link —
//! is the property, and a check added later would otherwise inherit the bug
//! silently; that this is defence rather than repair is said here rather than
//! implied by a comment nothing can falsify.
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

use crate::model::{ConnectError, Document, Link, LinkId, NodeKind, Side, Socket, TreeId};

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
            Self::Refused(why) => write!(f, "the end cannot go there: {why:?}"),
        }
    }
}

impl<T: fmt::Debug> std::error::Error for RelinkError<T> {}

impl<K: NodeKind> Document<K> {
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

        // Out first, so every read of the graph below happens without it. What
        // a counterfactual can actually reach is the PLACEMENT — a link still
        // in place is found crowding the port its other end never left, and is
        // displaced by itself. See the header for why the vet above is immune
        // today and why the lift is still here rather than one line down.
        self.lift(tree, at);
        match self.vet(tree, from, to) {
            Err(why) => {
                // Exactly as it was: same id, same mute, same position.
                self.place(tree, held, None, Some(at));
                Err(RelinkError::Refused(why))
            }
            Ok(crowded) => {
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
    }
}
