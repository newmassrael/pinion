//! ★★★★★ R1935 — **a value crosses the canvas with no edge**: the named
//! endpoint and the far end of its name.
//!
//! [`NodeBody::Beacon`] and [`NodeBody::Echo`] are the two bodies; this module
//! is the two readings that relate them and the two verbs that convert between
//! this form and the plain bend R1934 built.
//!
//! # The four things the reference does, measured rather than recalled
//!
//! Its editor ships four operators over this pair, and reading all four is
//! what showed they are not four spellings of one thing:
//!
//! 1. **from a far end to the named one** — the answer is ONE node, so it
//!    moves the selection there and fits the view to it;
//! 2. **from the named one to its far ends** — the answer is MANY, so it
//!    *clears* the selection instead and hands the list to the search-results
//!    panel;
//! 3. **plain bend → named pair**, which is a FAN-OUT: one bend with N outgoing
//!    wires becomes one named endpoint and **N** far ends, one per wire;
//! 4. **named pair → plain bend**, which is the fold, and it accepts EITHER
//!    half as the thing you started from.
//!
//! ★★★★★ Point 1 against point 2 is the finding, and the census sentence for
//! the second row — "the other direction of a named reroute" — is what hid it:
//! the two directions do not differ only in which way they walk, they differ in
//! **the shape of the answer**, and therefore in what an editor can do with it.
//! An [`Option`] and a [`Vec`] is that difference written down where a caller
//! cannot miss it. There the two return `void` and act on the editor directly,
//! so nothing but the two bodies of code says so.
//!
//! # What this crate does that the reference cannot
//!
//! * **The readings are the MODEL's.** There both live on the editor class and
//!   reach for its focused graph panel, so a second consumer — a test, an
//!   agent, a headless check — cannot ask "what does this name reach?" at all.
//!   Here [`Document::beacon_of`] and [`Document::echoes_of`] are questions
//!   about the document, and the selection and the view are the caller's
//!   business.
//! * **A name clash is refused, not silently repaired.** A private routine
//!   there walks the tree appending an index until the name is free, so a
//!   person who types a name that is taken gets a different one and is not
//!   told. Here a beacon answers [`Naming::InTree`](crate::Naming::InTree) and
//!   the existing uniqueness axis (R1932) refuses, naming the node that already
//!   answers to it.
//! * **A dangling far end is diagnosed.** There, whether the named endpoint
//!   still exists is a predicate an editor MAY call; nothing obliges anyone to.
//!   Here [`Document::validate`] reports it.
//!
//! # What is deliberately NOT reproduced
//!
//! The reference's fan-out sorts the outgoing wires by the **drawn Y of the
//! node each one feeds** and stacks the far ends around the named one at fifty
//! units apart. The sort is reproduced — it is a fact about the document, since
//! a node's position is stored — but nothing here consults a *drawn* bound, for
//! the reason R1934 gave: which curve a line crosses is the screen's question,
//! not the model's.

use std::collections::BTreeSet;

use crate::model::{Document, LinkId, NodeBody, NodeId, NodeKind, Socket, TreeId};

/// How far to one side of a bend the named endpoint that replaces it is put,
/// and how far to the other side its far ends are.
///
/// The reference's own two constants, and they are equal — which is what makes
/// the round trip put the bend back where it started rather than walking it
/// across the canvas one conversion at a time. That property is asserted, not
/// assumed.
const OFFSET: i32 = 50;

/// The vertical gap between stacked far ends, the reference's own.
const STACK: i32 = 50;

/// ★ R2005 — how far to the RIGHT of a named endpoint one more far end lands.
///
/// The reference's own `+150`, from its create-usage-from-declaration command,
/// and deliberately **not** [`OFFSET`]: that one is the fan-out's, and the two
/// are different numbers in the reference because they answer different
/// gestures. Folding them into one would make the round trip's equality
/// ([`OFFSET`]'s whole reason) accidentally depend on this.
const BESIDE: i32 = 150;

/// What [`Document::spread_reroute`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spread {
    /// The named endpoint the bend became.
    pub beacon: NodeId,
    /// The far ends made, **one per outgoing wire**, in the order those wires
    /// were stacked — which is by the drawn Y of the node each one feeds.
    ///
    /// Deliberately not sorted ascending like the rest of this crate's report
    /// vectors: the order IS the answer here, because it is the order they were
    /// placed in, and a caller drawing them or stepping through them wants that
    /// rather than their ids.
    pub echoes: Vec<NodeId>,
    /// The links **kept** and re-pointed rather than remade, ascending — so a
    /// caller holding a [`LinkId`] still holds the same link. R1934's rule, and
    /// the reference's: it re-points too.
    pub carried: Vec<LinkId>,
}

/// What [`Document::gather_beacon`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gathered {
    /// The plain bend the pair became.
    pub reroute: NodeId,
    /// The nodes removed — the named endpoint and every far end of it,
    /// ascending.
    pub gone: Vec<NodeId>,
    /// The links kept and re-pointed, ascending.
    pub carried: Vec<LinkId>,
}

/// ★★★★★ R2005 — what [`Document::echo_beacon`] made, and **what it had to
/// step past**.
///
/// The reference's command answers `void` and always places the new usage at
/// exactly `+150, same Y` from the declaration, with no regard for what is
/// already there — so running it twice puts two cards on the same point and
/// nothing says so. `past` is that fact made reportable: it is the far ends the
/// placement walked over, and it is empty exactly when the first spot was free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Echoed {
    /// The far end made.
    pub echo: NodeId,
    /// Where it landed.
    pub at: (i32, i32),
    /// The far ends of this same endpoint that were already sitting on the
    /// spots this one stepped past, ascending.
    ///
    /// Empty on the first call, one long on the second, and so on — which is
    /// what makes "they do not stack up" a measurement rather than a claim.
    pub past: Vec<NodeId>,
}

/// Why a conversion could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeaconError {
    /// The tree is not in this document.
    NoSuchTree(TreeId),
    /// That node is not in the tree.
    NoSuchNode(NodeId),
    /// [`Document::spread_reroute`] was given something that is not a plain
    /// bend.
    ///
    /// Named separately from [`Self::NotNamed`] because they are opposite
    /// repairs: this one says *convert the other way*, and that one says
    /// *convert this way*. One word for both would make the screen unable to
    /// say which.
    NotAReroute(NodeId),
    /// [`Document::gather_beacon`] was given something that is neither half of
    /// a named pair.
    NotNamed(NodeId),
    /// A far end naming an endpoint that is not there.
    ///
    /// Reachable only through [`Document::gather_beacon`] on such a node, and
    /// it is the state [`Document::validate`] reports.
    Dangling(NodeId),
    /// ★★★★★ R2005 — [`Document::echo_beacon`] was asked to make another far
    /// end of something that is itself a far END.
    ///
    /// Its own arm rather than [`Self::NotNamed`] because the repair is
    /// different **and available**: the endpoint this far end shows is one
    /// [`Document::beacon_of`] away, so the refusal carries it and a screen can
    /// offer *do it there instead* rather than only saying no. That is the same
    /// argument [`Self::NotAReroute`] and [`Self::NotNamed`] were split on.
    ///
    /// ⚠ The reference cannot reach this state at all, and not because it
    /// handles it: its context menu offers the command only on a declaration,
    /// so the question is never put. Measured — the command is bound as a bare
    /// execute action with **no** can-execute predicate, so the menu's
    /// visibility test is the only thing standing between it and a node it does
    /// nothing for.
    NotTheEndpoint {
        /// The far end that was asked.
        node: NodeId,
        /// The endpoint it shows, when it still has one — `None` for a far end
        /// whose endpoint is gone, which [`Self::Dangling`] is the name of.
        endpoint: Option<NodeId>,
    },
}

impl std::fmt::Display for BeaconError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchTree(tree) => write!(f, "no tree {}", tree.0),
            Self::NoSuchNode(node) => write!(f, "no node {}", node.0),
            Self::NotAReroute(node) => write!(f, "node {} is not a bend", node.0),
            Self::NotNamed(node) => write!(f, "node {} is not a named endpoint", node.0),
            Self::Dangling(node) => {
                write!(f, "node {} names an endpoint that is not there", node.0)
            }
            // R2005 — the sentence NAMES THE REPAIR when there is one, because
            // a refusal a person can act on is worth more than a correct no.
            Self::NotTheEndpoint {
                node,
                endpoint: Some(end),
            } => write!(
                f,
                "node {} shows a name rather than holding it — ask node {}",
                node.0, end.0
            ),
            // ★ And when the far end has nothing to redirect to, the sentence
            // says THAT rather than repeating `NotNamed`'s. Clippy asking for
            // the two arms to be merged is what showed they had been written
            // the same: a far end whose endpoint is gone and a node that was
            // never part of a pair are different states, and a screen that
            // reads one sentence for both cannot offer the repair.
            Self::NotTheEndpoint {
                node,
                endpoint: None,
            } => write!(
                f,
                "node {} shows a name whose endpoint is not there",
                node.0
            ),
        }
    }
}

impl std::error::Error for BeaconError {}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R1935 — **which named endpoint does this far end name?**
    ///
    /// `None` when `node` is not a far end, and `None` when it is one whose
    /// endpoint has been deleted — two absences that a caller distinguishes by
    /// asking what the body is, or by reading
    /// [`validate`](Document::validate), which reports the second and is silent
    /// about the first.
    ///
    /// The answer is at most ONE, and that is the whole difference from
    /// [`echoes_of`](Self::echoes_of) — see the module header.
    #[must_use]
    pub fn beacon_of(&self, tree: TreeId, node: NodeId) -> Option<NodeId> {
        let host = self.tree(tree)?;
        let NodeBody::Echo(beacon) = host.node(node)?.body else {
            return None;
        };
        host.node(beacon)
            .filter(|held| matches!(held.body, NodeBody::Beacon))
            .map(|held| held.id)
    }

    /// ★★★★★ R1935 — **which far ends name this endpoint?**, ascending.
    ///
    /// Empty for a node that is not a named endpoint, and empty for one nothing
    /// names yet — the second is an ordinary state, since a beacon is useful
    /// the moment it exists and its far ends arrive afterwards.
    ///
    /// The answer is MANY, which is why it is a [`Vec`] and not the reference's
    /// `void`: there the operator's answer goes straight into a search-results
    /// panel and no caller ever holds it.
    #[must_use]
    pub fn echoes_of(&self, tree: TreeId, node: NodeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        if !matches!(
            host.node(node).map(|held| &held.body),
            Some(NodeBody::Beacon)
        ) {
            return Vec::new();
        }
        let mut found: Vec<NodeId> = host
            .nodes()
            .filter(|held| matches!(held.body, NodeBody::Echo(named) if named == node))
            .map(|held| held.id)
            .collect();
        found.sort_unstable();
        found
    }

    /// ★★★★★ R1935 — **the name a far end shows**, which is its endpoint's.
    ///
    /// A far end has no name of its own — [`Naming::Free`](crate::Naming::Free)
    /// — so this is the reading a card needs, and it cannot live on
    /// [`Node::display_name`](crate::Node::display_name) because that method
    /// has only the node and this has to reach the document.
    ///
    /// `None` for a node that is not a far end or whose endpoint is gone: a
    /// card with no name to show should say so in its own words rather than be
    /// handed a plausible blank.
    #[must_use]
    pub fn echo_display_name(&self, tree: TreeId, node: NodeId) -> Option<String> {
        let beacon = self.beacon_of(tree, node)?;
        self.tree(tree)?.node(beacon).map(crate::Node::display_name)
    }

    /// Every far end in `tree` whose endpoint is gone, ascending.
    ///
    /// What [`validate`](Document::validate) reports, kept here beside the rest
    /// of the naming so the diagnosis and the readings cannot drift apart.
    #[must_use]
    pub fn dangling_echoes(&self, tree: TreeId) -> Vec<NodeId> {
        let Some(host) = self.tree(tree) else {
            return Vec::new();
        };
        let mut found: Vec<NodeId> = host
            .nodes()
            .filter(|held| match held.body {
                NodeBody::Echo(named) => !host
                    .node(named)
                    .is_some_and(|beacon| matches!(beacon.body, NodeBody::Beacon)),
                _ => false,
            })
            .map(|held| held.id)
            .collect();
        found.sort_unstable();
        found
    }

    /// ★★★★★ R1935 — **give this bend a name**: the plain bend becomes a named
    /// endpoint, and each wire that left it becomes a far end of that name.
    ///
    /// The reference's fan-out, measured behaviour for behaviour:
    ///
    /// * the endpoint lands one `OFFSET` to the **left** of the bend, and the
    ///   far ends one to the right;
    /// * the outgoing wires are **sorted by the Y of the node each one feeds**,
    ///   so the far ends come out in the order their consumers are drawn;
    /// * they are stacked `STACK` apart and **centred** on the bend's own Y,
    ///   which is what its index starting at `-n / 2` does;
    /// * every wire is **re-pointed, not remade**, so its identity survives.
    ///
    /// # Errors
    ///
    /// [`BeaconError::NotAReroute`] for anything that is not a plain bend —
    /// which includes a named endpoint, and that caller wants
    /// [`gather_beacon`](Self::gather_beacon) instead.
    pub fn spread_reroute(&mut self, tree: TreeId, node: NodeId) -> Result<Spread, BeaconError> {
        let host = self.tree(tree).ok_or(BeaconError::NoSuchTree(tree))?;
        let held = host.node(node).ok_or(BeaconError::NoSuchNode(node))?;
        if !matches!(held.body, NodeBody::Reroute) {
            return Err(BeaconError::NotAReroute(node));
        }
        let (x, y) = (held.x, held.y);
        let parent = held.parent;
        // Read the wires before anything moves. Incoming ones keep their sink
        // and change it to the endpoint's input; outgoing ones keep their
        // source and change it to a far end's only port.
        let incoming: Vec<LinkId> = host
            .links()
            .iter()
            .filter(|link| link.to.node == node)
            .map(|link| link.id)
            .collect();
        // The sort key is the drawn Y of the node each wire FEEDS, with the
        // link id after it so the order is total — the reference sorts on the
        // position alone and leaves ties to the container's order, which is not
        // a fact about the document.
        let mut outgoing: Vec<(i32, LinkId, Socket)> = host
            .links()
            .iter()
            .map(|link| (link.from.node, link.id, link.to))
            .filter(|&(source, _, _)| source == node)
            .map(|(_, id, to)| {
                let sink_y = host.node(to.node).map_or(0, |held| held.y);
                (sink_y, id, to)
            })
            .collect();
        outgoing.sort_by_key(|&(sink_y, id, _)| (sink_y, id));

        let beacon = self
            .add_node(tree, NodeBody::Beacon, x - OFFSET, y)
            .map_err(|_| BeaconError::NoSuchTree(tree))?;
        let mut carried = Vec::new();
        if let Some(host) = self.tree_mut(tree) {
            for id in incoming {
                if let Some(link) = host.link_mut(id) {
                    link.to = Socket::new(beacon, 0);
                    carried.push(id);
                }
            }
        }

        let count = i32::try_from(outgoing.len()).unwrap_or(0);
        let mut echoes = Vec::new();
        for (index, &(_, id, _)) in outgoing.iter().enumerate() {
            let step = i32::try_from(index).unwrap_or(0) - count / 2;
            let echo = self
                .add_node(tree, NodeBody::Echo(beacon), x + OFFSET, y + step * STACK)
                .map_err(|_| BeaconError::NoSuchTree(tree))?;
            if let Some(host) = self.tree_mut(tree) {
                if let Some(link) = host.link_mut(id) {
                    link.from = Socket::new(echo, 0);
                    carried.push(id);
                }
            }
            echoes.push(echo);
        }
        // The bend now has no wires on it, so removing it removes nothing else.
        self.remove_node(tree, node)
            .map_err(|_| BeaconError::NoSuchNode(node))?;
        if let Some(parent) = parent {
            let _ = self.set_parent(tree, beacon, Some(parent));
            for &echo in &echoes {
                let _ = self.set_parent(tree, echo, Some(parent));
            }
        }
        carried.sort_unstable();
        Ok(Spread {
            beacon,
            echoes,
            carried,
        })
    }

    /// ★★★★★ R1935 — **take the name away**: a named endpoint and all its far
    /// ends fold back into one plain bend.
    ///
    /// ★ Accepts **either half**, exactly as the reference does — handed a far
    /// end it walks to the endpoint first and makes the same edit. That is
    /// worth reproducing because it is what makes the gesture reachable from
    /// wherever the person is looking: the far ends are the halves scattered
    /// across the canvas, and requiring the endpoint would mean finding it
    /// first, which is the very thing the name exists to avoid.
    ///
    /// The bend lands one `OFFSET` to the **right** of the endpoint, so a
    /// spread followed by a gather puts it back exactly where it began.
    ///
    /// # Errors
    ///
    /// [`BeaconError::NotNamed`] for a node that is neither half, and
    /// [`BeaconError::Dangling`] for a far end whose endpoint is gone — there
    /// is nothing to fold, and answering "done" would be a lie.
    pub fn gather_beacon(&mut self, tree: TreeId, node: NodeId) -> Result<Gathered, BeaconError> {
        let host = self.tree(tree).ok_or(BeaconError::NoSuchTree(tree))?;
        let held = host.node(node).ok_or(BeaconError::NoSuchNode(node))?;
        let beacon = match held.body {
            NodeBody::Beacon => node,
            NodeBody::Echo(named) => host
                .node(named)
                .filter(|end| matches!(end.body, NodeBody::Beacon))
                .map(|end| end.id)
                .ok_or(BeaconError::Dangling(node))?,
            _ => return Err(BeaconError::NotNamed(node)),
        };
        let anchor = host.node(beacon).ok_or(BeaconError::NoSuchNode(beacon))?;
        let (x, y) = (anchor.x, anchor.y);
        let parent = anchor.parent;
        let echoes = self.echoes_of(tree, beacon);
        let folded: BTreeSet<NodeId> = echoes.iter().copied().chain([beacon]).collect();

        let host = self.tree(tree).ok_or(BeaconError::NoSuchTree(tree))?;
        // Every wire with one end on the pair, split by which end. A wire
        // between two halves of the same pair cannot exist — a far end has no
        // input — so the two lists cannot overlap.
        let incoming: Vec<LinkId> = host
            .links()
            .iter()
            .filter(|link| folded.contains(&link.to.node))
            .map(|link| link.id)
            .collect();
        let outgoing: Vec<LinkId> = host
            .links()
            .iter()
            .filter(|link| folded.contains(&link.from.node))
            .map(|link| link.id)
            .collect();

        let reroute = self
            .add_node(tree, NodeBody::Reroute, x + OFFSET, y)
            .map_err(|_| BeaconError::NoSuchTree(tree))?;
        let mut carried = Vec::new();
        if let Some(host) = self.tree_mut(tree) {
            for id in incoming {
                if let Some(link) = host.link_mut(id) {
                    link.to = Socket::new(reroute, 0);
                    carried.push(id);
                }
            }
            for id in outgoing {
                if let Some(link) = host.link_mut(id) {
                    link.from = Socket::new(reroute, 0);
                    carried.push(id);
                }
            }
        }
        let mut gone: Vec<NodeId> = folded.iter().copied().collect();
        for &id in &gone {
            self.remove_node(tree, id)
                .map_err(|_| BeaconError::NoSuchNode(id))?;
        }
        if let Some(parent) = parent {
            let _ = self.set_parent(tree, reroute, Some(parent));
        }
        gone.sort_unstable();
        carried.sort_unstable();
        Ok(Gathered {
            reroute,
            gone,
            carried,
        })
    }

    /// ★★★★★ R2005 — **may this node be asked for another far end?**
    ///
    /// The question [`Self::echo_beacon`] itself asks, so a screen that greys
    /// the gesture and one that just calls the verb cannot get different
    /// answers (R1920's rule).
    ///
    /// ⚠ **The reference has no such question.** Measured: its
    /// create-usage-from-declaration command is registered as a bare execute
    /// action with **no** can-execute predicate, twice, and the only thing
    /// deciding whether it is offered is a class test in the node's context
    /// menu builder. So the same command reached any other way runs on any node
    /// and silently does nothing — which is the shape R1888 recorded, a fact
    /// that is only true when the right person asks.
    ///
    /// # Errors
    ///
    /// [`BeaconError::NoSuchTree`], [`BeaconError::NoSuchNode`],
    /// [`BeaconError::NotTheEndpoint`] for a far end (carrying the endpoint to
    /// ask instead), and [`BeaconError::NotNamed`] for anything else.
    pub fn may_echo_beacon(&self, tree: TreeId, node: NodeId) -> Result<(), BeaconError> {
        let host = self.tree(tree).ok_or(BeaconError::NoSuchTree(tree))?;
        let held = host.node(node).ok_or(BeaconError::NoSuchNode(node))?;
        match held.body {
            NodeBody::Beacon => Ok(()),
            // ★ The one refusal that carries its own repair: `beacon_of`
            // answers where the question should have been put, and `None` there
            // is the dangling case rather than a second kind of absence.
            NodeBody::Echo(_) => Err(BeaconError::NotTheEndpoint {
                node,
                endpoint: self.beacon_of(tree, node),
            }),
            _ => Err(BeaconError::NotNamed(node)),
        }
    }

    /// ★★★★★ R2005 — **one more far end of this name**, placed where it does
    /// not sit on top of the ones already there.
    ///
    /// The reference's create-usage-from-declaration command, and the three
    /// things measured about it that this does differently:
    ///
    /// * **It is askable in advance** — see [`Self::may_echo_beacon`], which
    ///   this calls rather than repeating.
    /// * **It does not stack cards on one point.** There the position is
    ///   `+150, same Y` unconditionally, so a second call lands the new usage
    ///   exactly on the first. Here the spot steps down by the same
    ///   fifty units the fan-out stacks by — one constant, two verbs, so they
    ///   cannot disagree — and the far ends stepped past are **reported** in
    ///   [`Echoed::past`].
    /// * **It leaves the selection alone.** There the command *clears* the
    ///   selection first and does not select what it made, so a person is left
    ///   looking at a canvas with a new card on it and nothing indicating
    ///   which. Here the id comes back and what to select is the caller's
    ///   business, which is the split R1935 already drew.
    ///
    /// ★ And there is no second address. The reference writes BOTH a pointer
    /// to the declaration and a copy of its guid onto the new usage; a far end
    /// here names its endpoint by [`NodeId`] and by nothing else, so the two
    /// cannot come apart.
    ///
    /// ⚠ The stated limit: the spot is stepped past the far ends **of this
    /// endpoint**, not past everything on the canvas. A general
    /// what-is-drawn-here reading does not exist in this crate — [`Occupants`]
    /// is about a socket's links — and inventing one for this verb would be a
    /// second layout rule with no other caller. The population is the one the
    /// verb is about.
    ///
    /// [`Occupants`]: crate::Occupants
    ///
    /// # Errors
    ///
    /// Whatever [`Self::may_echo_beacon`] refuses.
    pub fn echo_beacon(&mut self, tree: TreeId, node: NodeId) -> Result<Echoed, BeaconError> {
        self.may_echo_beacon(tree, node)?;
        let held = self
            .tree(tree)
            .and_then(|host| host.node(node))
            .ok_or(BeaconError::NoSuchNode(node))?;
        let (x, mut y) = (held.x + BESIDE, held.y);
        // ★ The population is this endpoint's own far ends, read through
        // `echoes_of` rather than by walking the tree here — so the verb and
        // the reading a screen shows beside the card cannot disagree about who
        // belongs to this name.
        let taken: Vec<(NodeId, i32, i32)> = self
            .echoes_of(tree, node)
            .into_iter()
            .filter_map(|echo| {
                self.tree(tree)
                    .and_then(|host| host.node(echo))
                    .map(|far| (echo, far.x, far.y))
            })
            .collect();
        let mut past = Vec::new();
        while let Some((sitting, _, _)) = taken
            .iter()
            .find(|(_, at_x, at_y)| *at_x == x && *at_y == y)
        {
            past.push(*sitting);
            y += STACK;
        }
        past.sort_unstable();
        let echo = self
            .add_node(tree, NodeBody::Echo(node), x, y)
            .map_err(|_| BeaconError::NoSuchTree(tree))?;
        Ok(Echoed {
            echo,
            at: (x, y),
            past,
        })
    }
}
