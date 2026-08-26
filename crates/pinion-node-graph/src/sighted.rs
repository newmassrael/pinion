//! R1852 — a topology built from **nothing but what was seen**, with no
//! management API to ask.
//!
//! # Why this is not [`observed`](crate::observed)
//!
//! R1645 gave a DRAWN graph a second layer: a source reports traffic between
//! sockets that already exist, [`Document::observe`](crate::Document::observe)
//! records it, and [`Layers`](crate::Layers) says where the two layers agree.
//! That model presupposes the drawing — and says so by refusing: `observe`
//! answers [`ObserveError::NoSuchNode`](crate::ObserveError::NoSuchNode) for a
//! report about a node the document does not have.
//!
//! A capture has no drawing. It has a list of hops, each naming an endpoint that
//! sent and an endpoint that received, and **the endpoints are whatever the hops
//! mention** — there is no roster to check them against and no node to ask,
//! because asking is a management API and this capability is defined by not
//! having one. So the two layers are not two here: there is one, and it is made
//! of sightings.
//!
//! ⇒ The two modules compose rather than overlap. This one builds a topology out
//! of hops; `observed` is what compares a drawing with one.
//!
//! # ★★★★★ What this type exists to make impossible
//!
//! **A topology built from sightings can affirm and it cannot deny.** Every
//! question about a direction has THREE answers, not two
//! ([`Sighted`]): traffic was seen, traffic was not seen between two endpoints
//! that ARE known, or an endpoint is not one this capture ever mentioned. The
//! middle one is the whole point, and collapsing it into *no* is the error this
//! module is shaped against — *we did not see it* is a fact about the capture
//! and *there is no such link* is a claim about the world, and only a management
//! API could support the second.
//!
//! The same rule decides the vantage vocabulary. [`Vantage`] has THREE arms and
//! not four: an endpoint is in this topology **because** traffic was seen, so
//! *isolated* cannot occur — and rather than being documented as impossible it is
//! not expressible.
//!
//! And [`SightedTopology::standing`] is [`Standing::Partial`] always, by
//! construction. R1645 derives that verdict from a switch and from drift; here it
//! needs neither, because a drawing assembled from traffic is *definitionally*
//! not known to be whole. A `Certain` answer from this type would be a lie with a
//! type signature.
//!
//! # Where the floor stands, and the claim the probe REFUTED
//!
//! ⚠ This section's first draft said the reference toolkit's tabular model
//! cannot tell *there is nothing here* from *nothing is known here*. **Compiled
//! and run against 6.11.1, that is false**: a cell holding an empty value
//! answers a valid one and a cell nothing was ever set on answers an invalid
//! one, and a consumer distinguishes them with one call. The claim was written
//! before the probe and the probe deleted it.
//!
//! What the same probe measured that DOES stand, and it is a different and
//! better difference:
//!
//! * **That model asserts its own completeness.** Asked whether more rows may
//!   exist, it answered *no* — for a table whose content was a sample. There is
//!   no place in it to say *this is what I saw, not what there is*, so a
//!   consumer reading a topology out of one is told it has the whole graph.
//!   [`SightedTopology::standing`] cannot make that mistake, because the type
//!   has no arm for it.
//! * **The meaning of the distinction is the consumer's, not the surface's.**
//!   *Invalid* there means *this index is not the model's*; whether that stands
//!   for *unknown* is an interpretation each reader makes. [`Sighted`] names its
//!   three answers and publishes the words
//!   ([`Sighted::WIRE_NAMES`]), so a client reads a verdict instead of
//!   inferring one.
//! * **A state that cannot occur is not expressible.** A node with no edges is
//!   representable in any graph model and is a logical impossibility in a
//!   sightings-only one. [`Vantage`] has no arm for it.
//!
//! ⇒ Recorded this way round on purpose: a superiority claim that a probe
//! reverses is worth more written down than quietly replaced, because the next
//! round reaching for the same argument will find it already tested.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::observed::{Discovery, Standing};

/// One hop a capture saw: traffic from one endpoint to another.
///
/// Borrowed rather than owned because a caller reads these off rows it already
/// holds, and a builder that demanded `String` would make every consumer
/// allocate a copy of its own capture to ask a question about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sighting<'a> {
    /// The endpoint that sent.
    pub from: &'a str,
    /// The endpoint that received.
    pub to: &'a str,
}

impl<'a> Sighting<'a> {
    /// One hop.
    #[must_use]
    pub const fn new(from: &'a str, to: &'a str) -> Self {
        Self { from, to }
    }
}

/// What a sightings-only topology can say about one direction.
///
/// ★★★★★ THREE answers, and the middle one is why this type exists. See the
/// module header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sighted {
    /// Traffic was seen this way, this many times. Never zero — a zero would be
    /// [`NotSeen`](Self::NotSeen), and the two must not be one value.
    Seen(usize),
    /// Both endpoints were seen, and traffic was NOT seen this way.
    ///
    /// ⚠ This is **not** *there is no such link*. A capture is a sample; a link
    /// that carried nothing while it was recording leaves exactly this trace.
    NotSeen,
    /// At least one of the two endpoints was never mentioned by any hop, so the
    /// question is not about this topology at all.
    Unknown,
}

impl Sighted {
    /// How many times, or zero for the two answers that are not a count.
    ///
    /// A convenience for a caller totalling traffic; it is deliberately NOT the
    /// way to test whether something was seen, because that reading collapses the
    /// distinction this type carries. Use [`is_seen`](Self::is_seen).
    #[must_use]
    pub const fn count(self) -> usize {
        match self {
            Self::Seen(n) => n,
            Self::NotSeen | Self::Unknown => 0,
        }
    }

    /// Whether traffic was seen this way.
    #[must_use]
    pub const fn is_seen(self) -> bool {
        matches!(self, Self::Seen(_))
    }

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Seen(_) => "seen",
            Self::NotSeen => "not_seen",
            Self::Unknown => "unknown",
        }
    }

    /// The closed vocabulary a client enumerates instead of discovering the
    /// words from a sample.
    pub const WIRE_NAMES: [&'static str; 3] = ["seen", "not_seen", "unknown"];
}

impl fmt::Display for Sighted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Seen(n) => write!(f, "seen {n} time(s)"),
            Self::NotSeen => f.write_str("not seen between two known endpoints"),
            Self::Unknown => f.write_str("an endpoint this capture never mentioned"),
        }
    }
}

/// Where an endpoint stands in what was seen.
///
/// ★★★★★ THREE arms, not four. An endpoint is in a sightings-only topology
/// *because* traffic was seen, so *isolated* cannot occur — and it is absent
/// from the vocabulary rather than documented as impossible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Vantage {
    /// Seen sending, never seen receiving.
    Sending,
    /// Seen receiving, never seen sending.
    Receiving,
    /// Seen both ways.
    Both,
}

impl Vantage {
    /// Every arm, for a consumer that must cover the vocabulary.
    pub const ALL: [Self; 3] = [Self::Sending, Self::Receiving, Self::Both];

    /// A stable name, for a caption or a wire form.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sending => "sending",
            Self::Receiving => "receiving",
            Self::Both => "both",
        }
    }

    /// Parse a wire name back — the inverse of [`name`](Self::name).
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.name() == name)
    }

    /// The closed vocabulary, projected from [`ALL`](Self::ALL) so a new arm
    /// cannot leave this list short.
    pub const WIRE_NAMES: [&'static str; Self::ALL.len()] = {
        let mut out = [""; Self::ALL.len()];
        let mut at = 0;
        while at < Self::ALL.len() {
            out[at] = Self::ALL[at].name();
            at += 1;
        }
        out
    };
}

impl fmt::Display for Vantage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A topology assembled from sightings and nothing else.
///
/// Endpoints are whatever the hops mentioned, in sorted order; edges are the
/// distinct directions traffic was seen in, each carrying how many times. Both
/// are derived once at construction, because every question below is a lookup
/// and a consumer asking about one endpoint should not pay for a pass over the
/// capture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SightedTopology {
    endpoints: Vec<String>,
    /// Directed, by endpoint index, with the count of sightings.
    edges: BTreeMap<(usize, usize), usize>,
    sightings: usize,
}

impl SightedTopology {
    /// Build one from a sequence of hops.
    ///
    /// A hop whose two endpoints are the same is kept: an endpoint that sent to
    /// itself is a thing a capture can show, and dropping it would be this type
    /// deciding what the capture meant.
    #[must_use]
    pub fn from_sightings<'a>(seen: impl IntoIterator<Item = Sighting<'a>>) -> Self {
        let hops: Vec<(&str, &str)> = seen.into_iter().map(|one| (one.from, one.to)).collect();
        let names: BTreeSet<&str> = hops.iter().flat_map(|(a, b)| [*a, *b]).collect();
        let endpoints: Vec<String> = names.iter().map(|one| (*one).to_owned()).collect();
        let index: BTreeMap<&str, usize> = names.iter().enumerate().map(|(n, s)| (*s, n)).collect();
        let mut edges: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for (from, to) in &hops {
            let pair = (index[*from], index[*to]);
            *edges.entry(pair).or_default() += 1;
        }
        Self {
            endpoints,
            edges,
            sightings: hops.len(),
        }
    }

    /// Every endpoint any hop mentioned, sorted.
    #[must_use]
    pub fn endpoints(&self) -> &[String] {
        &self.endpoints
    }

    /// How many endpoints.
    #[must_use]
    pub fn len(&self) -> usize {
        self.endpoints.len()
    }

    /// Whether no hop was seen at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.endpoints.is_empty()
    }

    /// How many hops went into this topology.
    #[must_use]
    pub const fn sightings(&self) -> usize {
        self.sightings
    }

    /// What can be said about traffic from `from` to `to`.
    #[must_use]
    pub fn sighted(&self, from: &str, to: &str) -> Sighted {
        let (Some(a), Some(b)) = (self.at(from), self.at(to)) else {
            return Sighted::Unknown;
        };
        match self.edges.get(&(a, b)) {
            Some(&n) => Sighted::Seen(n),
            None => Sighted::NotSeen,
        }
    }

    /// Where `endpoint` stands, or `None` when this capture never mentioned it.
    #[must_use]
    pub fn vantage(&self, endpoint: &str) -> Option<Vantage> {
        let at = self.at(endpoint)?;
        let sends = self.edges.keys().any(|(a, _)| *a == at);
        let receives = self.edges.keys().any(|(_, b)| *b == at);
        // An endpoint is here because a hop named it, so at least one holds —
        // which is why there is no fourth arm to return.
        Some(match (sends, receives) {
            (true, true) => Vantage::Both,
            (true, false) => Vantage::Sending,
            _ => Vantage::Receiving,
        })
    }

    /// How many distinct endpoints `endpoint` was seen sending to, and receiving
    /// from — or `None` for an endpoint this capture never mentioned.
    ///
    /// One function for both, because they are one fact about a place in the
    /// topology and a caller that could ask for half would be able to describe a
    /// vantage the other half contradicts.
    #[must_use]
    pub fn degree(&self, endpoint: &str) -> Option<(usize, usize)> {
        let at = self.at(endpoint)?;
        let out = self.edges.keys().filter(|(a, _)| *a == at).count();
        let into = self.edges.keys().filter(|(_, b)| *b == at).count();
        Some((out, into))
    }

    /// Every endpoint traffic was seen between `endpoint` and, either way,
    /// sorted. Empty for an endpoint this capture never mentioned.
    #[must_use]
    pub fn peers(&self, endpoint: &str) -> Vec<&str> {
        let Some(at) = self.at(endpoint) else {
            return Vec::new();
        };
        let mut out: BTreeSet<usize> = BTreeSet::new();
        for (a, b) in self.edges.keys() {
            if *a == at {
                out.insert(*b);
            }
            if *b == at {
                out.insert(*a);
            }
        }
        out.into_iter()
            .map(|n| self.endpoints[n].as_str())
            .collect()
    }

    /// Every direction traffic was seen in, with its count — sorted.
    pub fn edges(&self) -> impl Iterator<Item = (&str, &str, usize)> {
        self.edges
            .iter()
            .map(|((a, b), n)| (self.endpoints[*a].as_str(), self.endpoints[*b].as_str(), *n))
    }

    /// The **undirected** endpoint pairs traffic was seen between, sorted.
    ///
    /// A conversation between two endpoints is one thing whichever way a
    /// particular hop went, and a caller counting them wants that number rather
    /// than the directed one. Derived from the edges rather than accumulated
    /// separately, so the two cannot disagree about how many there are.
    #[must_use]
    pub fn conversations(&self) -> Vec<(&str, &str)> {
        let pairs: BTreeSet<(usize, usize)> = self
            .edges
            .keys()
            .map(|(a, b)| if a <= b { (*a, *b) } else { (*b, *a) })
            .collect();
        pairs
            .into_iter()
            .map(|(a, b)| (self.endpoints[a].as_str(), self.endpoints[b].as_str()))
            .collect()
    }

    /// Whether `a` and `b` were seen talking, either way.
    #[must_use]
    pub fn converse(&self, a: &str, b: &str) -> bool {
        self.sighted(a, b).is_seen() || self.sighted(b, a).is_seen()
    }

    /// ★★★★★ Always [`Standing::Partial`], by construction.
    ///
    /// R1645's `Standing` derives this verdict from a discovery switch and from
    /// drift. Here it needs neither: a topology assembled from traffic is
    /// definitionally not known to be whole, because the only thing that could
    /// make it whole is asking every endpoint what it is connected to — and this
    /// capability is defined by having no way to ask.
    ///
    /// `drift` is every direction seen, because with no drawing at all there is
    /// nothing any of them was accounted for by; `discovery` is
    /// [`Discovery::On`], because arriving unbidden is the only way anything
    /// arrives here.
    #[must_use]
    pub fn standing(&self) -> Standing {
        Standing::Partial {
            drift: self.edges.len(),
            discovery: Discovery::On,
        }
    }

    fn at(&self, endpoint: &str) -> Option<usize> {
        self.endpoints
            .binary_search_by(|known| known.as_str().cmp(endpoint))
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{Sighted, SightedTopology, Sighting, Vantage};
    use crate::observed::{Discovery, Standing};

    /// The shape a capture of a hub-and-spoke deployment leaves: several
    /// endpoints sending to one, and that one answering some of them.
    fn star() -> SightedTopology {
        SightedTopology::from_sightings([
            Sighting::new("n1", "r1"),
            Sighting::new("n1", "r1"),
            Sighting::new("n2", "r1"),
            Sighting::new("n3", "r1"),
            Sighting::new("r1", "n2"),
        ])
    }

    #[test]
    fn the_endpoints_are_whatever_the_hops_mentioned() {
        let seen = star();
        assert_eq!(seen.endpoints(), ["n1", "n2", "n3", "r1"]);
        assert_eq!(seen.len(), 4);
        assert!(!seen.is_empty());
        assert_eq!(seen.sightings(), 5, "five hops, four endpoints");
        assert_eq!(
            seen.edges().collect::<Vec<_>>(),
            vec![
                ("n1", "r1", 2),
                ("n2", "r1", 1),
                ("n3", "r1", 1),
                ("r1", "n2", 1)
            ],
            "directions are deduplicated and carry their counts"
        );
        assert!(SightedTopology::default().is_empty());
    }

    /// ★★★★★ The claim this module exists for: *not seen* and *no such thing*
    /// are different answers, and so is *I have never heard of that endpoint*.
    #[test]
    fn a_direction_has_three_answers_and_not_two() {
        let seen = star();
        assert_eq!(seen.sighted("n1", "r1"), Sighted::Seen(2));
        // Known endpoints, and traffic was not seen this way. NOT a claim that
        // the link does not exist — the capture is a sample.
        assert_eq!(seen.sighted("r1", "n1"), Sighted::NotSeen);
        assert_eq!(seen.sighted("n1", "n2"), Sighted::NotSeen);
        // An endpoint no hop mentioned: the question is not about this topology.
        assert_eq!(seen.sighted("n9", "r1"), Sighted::Unknown);
        assert_eq!(seen.sighted("r1", "n9"), Sighted::Unknown);
        assert_eq!(seen.sighted("n9", "n8"), Sighted::Unknown);

        // ⚠ And the three are DISTINGUISHABLE by a consumer, which is the whole
        // difference from an accessor that answers one empty value for both.
        assert_ne!(seen.sighted("r1", "n1"), seen.sighted("n9", "r1"));
        assert!(!seen.sighted("r1", "n1").is_seen());
        assert!(!seen.sighted("n9", "r1").is_seen());
        // `count` collapses them and says so in its own doc; `is_seen` does not.
        assert_eq!(seen.sighted("r1", "n1").count(), 0);
        assert_eq!(seen.sighted("n9", "r1").count(), 0);
        assert_eq!(
            Sighted::WIRE_NAMES,
            ["seen", "not_seen", "unknown"],
            "the vocabulary is published, so a client enumerates it"
        );
        assert_eq!(seen.sighted("n1", "r1").name(), "seen");
        assert!(seen.sighted("n9", "r1").to_string().contains("never"));
    }

    /// ★★★★★ Three vantages, not four: *isolated* is not expressible because it
    /// cannot occur.
    #[test]
    fn a_vantage_cannot_be_isolated() {
        let seen = star();
        assert_eq!(seen.vantage("n1"), Some(Vantage::Sending));
        assert_eq!(seen.vantage("n3"), Some(Vantage::Sending));
        assert_eq!(seen.vantage("n2"), Some(Vantage::Both));
        assert_eq!(seen.vantage("r1"), Some(Vantage::Both));
        assert_eq!(seen.vantage("n9"), None, "never mentioned is not a vantage");
        // The vocabulary is three words and every one of them is reachable from
        // a real capture shape — a receiving-only endpoint needs one that never
        // sends.
        let quiet = SightedTopology::from_sightings([Sighting::new("r1", "n8")]);
        assert_eq!(quiet.vantage("n8"), Some(Vantage::Receiving));
        assert_eq!(Vantage::ALL.len(), 3);
        assert_eq!(Vantage::WIRE_NAMES, ["sending", "receiving", "both"]);
        for word in Vantage::WIRE_NAMES {
            assert!(Vantage::from_wire(word).is_some(), "{word} round trips");
        }
        assert_eq!(Vantage::from_wire("isolated"), None);
    }

    #[test]
    fn degree_and_peers_are_about_distinct_endpoints() {
        let seen = star();
        assert_eq!(seen.degree("r1"), Some((1, 3)), "answers one, hears three");
        assert_eq!(seen.degree("n1"), Some((1, 0)));
        assert_eq!(seen.degree("n9"), None);
        assert_eq!(seen.peers("r1"), ["n1", "n2", "n3"]);
        assert_eq!(seen.peers("n1"), ["r1"]);
        assert!(seen.peers("n9").is_empty());
    }

    /// A conversation is undirected, and the count differs from the directed one
    /// exactly when somebody answered.
    #[test]
    fn conversations_are_undirected_and_derived_from_the_edges() {
        let seen = star();
        assert_eq!(
            seen.conversations(),
            vec![("n1", "r1"), ("n2", "r1"), ("n3", "r1")]
        );
        assert_eq!(seen.edges().count(), 4, "four directions");
        assert_eq!(seen.conversations().len(), 3, "three conversations");
        assert!(seen.converse("r1", "n1"), "either way counts");
        assert!(seen.converse("n1", "r1"));
        assert!(!seen.converse("n1", "n2"));
        assert!(!seen.converse("n1", "n9"));
    }

    /// ★★★★★ A sightings-only topology can never be `Certain`, and that is a
    /// property of the type rather than of any particular capture.
    #[test]
    fn standing_is_partial_by_construction() {
        for seen in [
            star(),
            SightedTopology::default(),
            SightedTopology::from_sightings([Sighting::new("a", "b")]),
        ] {
            let standing = seen.standing();
            assert!(
                !standing.is_certain(),
                "a drawing made of traffic is never known to be whole: {standing}"
            );
            assert_eq!(standing.name(), "partial");
            let Standing::Partial { drift, discovery } = standing else {
                panic!("the only other arm is Certain, which is refused above");
            };
            assert_eq!(
                drift,
                seen.edges().count(),
                "nothing drawn accounts for any of it"
            );
            assert_eq!(
                discovery,
                Discovery::On,
                "arriving unbidden is the only way"
            );
        }
    }

    /// A hop from an endpoint to itself is KEPT, because a capture can show one
    /// and dropping it would be this type deciding what the capture meant.
    #[test]
    fn a_self_hop_is_kept_rather_than_interpreted() {
        let seen = SightedTopology::from_sightings([Sighting::new("r1", "r1")]);
        assert_eq!(seen.endpoints(), ["r1"]);
        assert_eq!(seen.sighted("r1", "r1"), Sighted::Seen(1));
        assert_eq!(seen.vantage("r1"), Some(Vantage::Both));
        assert_eq!(seen.conversations(), vec![("r1", "r1")]);
        assert_eq!(seen.degree("r1"), Some((1, 1)));
    }
}
