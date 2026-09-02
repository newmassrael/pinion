//! ★★★★★ R1980 — **what is standing on a socket**, derived once and read by
//! everything that has to know.
//!
//! # What forced it, DRIVEN rather than read
//!
//! Four places spelled *is anything on this socket* and they did not agree.
//! Three of them are on the node lab's screen — the slot it drops when a wire
//! leaves, the spare slot it reuses, and the endpoints it offers a dialler —
//! and all three count a **drawn link** and a **reported connection**. The
//! fourth is this crate's own [`Document::may_land`](crate::Document::may_land)
//! planner, and it counted only the drawn half.
//!
//! Measured at R1980 by driving the lab's opening graph, where one accept slot
//! is held by a report and nothing else:
//!
//! ```text
//! before   port 0: links=["link#4"]  obs=[]
//!          port 1: links=[]          obs=["obs<-P-01"]
//! may_land(link from S-01, Input, P-02)  ->  Takes(Socket { port: 1 })
//! after    port 1: links=["link#3"]  obs=["obs<-P-01"]
//! ```
//!
//! An unrelated wire, re-aimed at that card, took the slot a reported
//! connection was sitting on — and the screen then wrote its own address over
//! it. Nobody had asked for that slot.
//!
//! # Why this is a derivation and not a fix in the planner
//!
//! Adding the missing half to the planner would have made four spellings agree
//! **today**. This repository has measured what that is worth twice (R1963,
//! R1966): when one property is spelled in several places, what has to be
//! counted is not how many places but **what is holding them together** — and
//! the honest answer here was *nothing*. So the property gets one home, and the
//! four readers ask it.
//!
//! # ⚠ A reported connection occupies a socket; it does not FORBID one
//!
//! [`Document::adopt`](crate::Document::adopt) deliberately draws a link onto
//! the socket a report is standing on — that is the whole of what adopting a
//! reported connection means, and R1681's test asserts it. The difference this
//! module draws is between a socket a person **named** and a socket something
//! **chose for them**: adoption names it, and the automatic search must not
//! choose it. Erase that distinction and adoption breaks.
//!
//! # What it is NOT
//!
//! Not a multiplicity check. *Is anything standing here* and *may another end
//! stand here too* are different questions —
//! [`Multiplicity`](crate::Multiplicity) answers the second and a `Many` port
//! is occupied and open at once. This module answers the first, and
//! [`Document::may_land`](crate::Document::may_land) is where the two meet.

use crate::model::{Document, LinkId, NodeKind, Side, Socket, TreeId};
use crate::observed::Observation;

/// What is standing on one socket: the links a person **drew** and the
/// connections the world **reported**.
///
/// Two lists rather than a count, because the two layers are answerable to
/// different questions — a drawn link has an id a screen can address and a
/// refusal can name, a report has a source and no id at all — and a caller that
/// wants only *is this free* asks [`is_free`](Self::is_free) without having to
/// know either.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Occupants {
    drawn: Vec<LinkId>,
    reported: Vec<Observation>,
}

impl Occupants {
    /// Whether nothing at all is standing on the socket.
    ///
    /// ★ The name says *free*, not *empty*, because a caller asking this is
    /// asking whether it may put something here — and both layers are reasons
    /// it may not.
    #[must_use]
    pub fn is_free(&self) -> bool {
        self.drawn.is_empty() && self.reported.is_empty()
    }

    /// The drawn links standing on it, in the order the tree holds them.
    #[must_use]
    pub fn drawn(&self) -> &[LinkId] {
        &self.drawn
    }

    /// The reported connections standing on it, ascending.
    #[must_use]
    pub fn reported(&self) -> &[Observation] {
        &self.reported
    }

    /// The same answer with one drawn link left out.
    ///
    /// What a wire being **re-aimed** needs: the end that is already standing
    /// here is not a reason it may not stand here. A separate combinator rather
    /// than an argument to the question, so the question stays *what is on
    /// this socket* — one fact, asked one way — and the exception belongs to
    /// the caller that has one.
    #[must_use]
    pub fn without(mut self, link: LinkId) -> Self {
        self.drawn.retain(|held| *held != link);
        self
    }
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1980 — **what is standing on `socket`**, counting both layers.
    ///
    /// `end` says which end of a connection the socket would be: a socket is
    /// the consuming end of the links whose `to` it is, and the producing end
    /// of the links whose `from` it is. The same socket can be neither, and
    /// asking about the wrong side answers [`Occupants::is_free`] rather than
    /// lying — the sockets on an input port and an output port are different
    /// sockets even when the node and the ordinal coincide.
    ///
    /// Answers an empty [`Occupants`] for a tree that is not here, for the
    /// reason [`Document::observations`] does: *nothing is standing on a socket
    /// in a tree that does not exist* is true, and an error arm would make
    /// every caller handle a case none of them can act on.
    #[must_use]
    pub fn occupants(&self, tree: TreeId, socket: Socket, end: Side) -> Occupants {
        let drawn = self
            .tree(tree)
            .map(|host| {
                host.links()
                    .iter()
                    .filter(|held| match end {
                        Side::Input => held.to == socket,
                        Side::Output => held.from == socket,
                    })
                    .map(|held| held.id)
                    .collect()
            })
            .unwrap_or_default();
        let reported = self
            .reports()
            .filter(|one| one.tree == tree)
            .filter(|one| match end {
                Side::Input => one.to == socket,
                Side::Output => one.from == socket,
            })
            .copied()
            .collect();
        Occupants { drawn, reported }
    }
}
