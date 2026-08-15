//! ★★★★★ R1695 §5.38 §5.39 §5.40 §2 #7 — **a destination is a place you arrive
//! at**, and a roster of them is a value.
//!
//! ## What was missing, measured
//!
//! A navigation rail is a list of a product's top-level destinations, and the
//! framework has had one since R751: a `navigation` landmark, `link` children,
//! `aria-current="page"` on the active one, and a 1-of-N selection coordinator
//! underneath. What it has never had is **the other half** — what is *at* a
//! destination, and what happens when you go there.
//!
//! So every consumer invented that half, and the analysis tool's two screens
//! invented it differently. Driven through the pointer router and measured:
//!
//! * One screen's rail offers seven destinations. Pressing four of them moved a
//!   string the rail highlights itself from — and left the painted scene at
//!   **193 tagged regions before and 193 after, nothing added and nothing
//!   removed**. The screen said *Stream* and showed the dashboard.
//! * The other screen's rail offers seven, and pressing **any** of them —
//!   including the one the screen already is — produced a message saying the
//!   destination is not this screen. Two of those seats are declared
//!   unavailable with a stated reason, and the message said something else.
//! * The two rosters share two keys out of seven. One tool, two lists of what
//!   the tool contains.
//!
//! The existing gate did not see it, and could not: it asserted that a press
//! **moved the state**, which is true of a screen that highlights a seat and
//! shows nothing new. *Moved* is not *arrived*.
//!
//! ## The shape
//!
//! [`Destinations`] is the roster — ordered, keyed, and every key distinct,
//! checked at construction. [`Journey`] is where you are. [`Journey::navigate`]
//! is the only way to move, and it **returns why it refused**, which is the
//! part every hand-rolled version dropped.
//!
//! A destination's [`Standing`] is either [`Open`](Standing::Open) or
//! [`Closed`](Standing::Closed), and a closed one carries the framework's
//! existing [`Unavailable`] rather than a second vocabulary — the same reason
//! value the disabled cascade, `scene/disabled` and the accessibility tree
//! already speak. R1695 added one arm to it,
//! [`Elsewhere`](crate::availability::UnavailableKind::Elsewhere), because a
//! product with more than one surface needs to say *built, shipping, and not
//! here* and neither *reserved* nor *unsupported* means that.
//!
//! ## Against the reference toolkit's paged container
//!
//! Measured by building a probe against 6.11.1 and running it, rather than by
//! reading about it:
//!
//! | question | there | here |
//! |---|---|---|
//! | how is a destination addressed | an ordinal, or a pointer to the page | a key |
//! | going somewhere that does not exist | returns `void`; the index silently stays | [`Detour::NoSuchDestination`] |
//! | going somewhere unavailable | **arrives anyway** — a disabled page still becomes current | [`Detour::Closed`], carrying the reason |
//! | why a destination is closed | one bool on the page, and it does not gate arrival | [`Unavailable`]: kind, detail, derived recourse |
//! | a destination with no page yet | inexpressible | a roster entry that is [`Closed`](Standing::Closed) |
//! | can a hidden page still be driven | yes — sent a press, a key and a wheel, it counted all three | it is not in the scene |
//! | which destination is current, published | the container's accessible value is empty | [`wire`](Destinations::wire) and the region's own node |
//! | a locked seat, to a reader | `focusable`, `selectable`, and **no unavailable state at all** | unavailable, with kind, detail and recourse |
//!
//! The sixth row is the one that changes how a screen is written. Because a
//! non-current page is live there, input scoping is a guard the author must
//! remember at every handler — and the reference prototype this tool is
//! modelled on does exactly that, opening its wheel handler with a test that
//! the active section is its own. Here the page that is not current is not
//! built, so there is nothing to guard.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::availability::Unavailable;

/// Whether a destination can be arrived at, and if not, why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "standing", rename_all = "snake_case")]
pub enum Standing {
    /// You arrive, and the region shows this destination's page.
    Open,
    /// You do not arrive, and this says why — in the vocabulary the disabled
    /// cascade and the accessibility tree already use, so a rail seat and its
    /// destination cannot describe the same closure two ways.
    Closed(Unavailable),
}

impl Standing {
    /// Whether arriving is possible.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        matches!(self, Standing::Open)
    }

    /// The reason arriving is impossible, when it is.
    #[must_use]
    pub const fn why(&self) -> Option<&Unavailable> {
        match self {
            Standing::Open => None,
            Standing::Closed(why) => Some(why),
        }
    }
}

/// One top-level destination of a product.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    /// The stable identity a rail seat, a page and the wire all address it by.
    ///
    /// A key rather than the ordinal the reference toolkit's paged container
    /// uses: an ordinal makes *reordering the rail* and *changing where a press
    /// goes* the same edit, and nothing catches the second one.
    pub key: Cow<'static, str>,
    /// What a reader calls it.
    pub title: Cow<'static, str>,
    /// Whether it can be arrived at.
    pub standing: Standing,
}

impl Destination {
    /// A destination you can arrive at.
    #[must_use]
    pub fn open(key: impl Into<Cow<'static, str>>, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
            standing: Standing::Open,
        }
    }

    /// A destination that is on the rail, named, and not arrivable — with the
    /// reason a reader and an agent both receive.
    #[must_use]
    pub fn closed(
        key: impl Into<Cow<'static, str>>,
        title: impl Into<Cow<'static, str>>,
        why: Unavailable,
    ) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
            standing: Standing::Closed(why),
        }
    }
}

/// What is wrong with a roster, or with the destination a journey was asked to
/// begin at.
///
/// A constructor that returns this rather than one that trusts its caller,
/// because every field of a roster is a join — a seat's tag to a key, a key to
/// a page — and a join that is silently wrong paints a rail nobody can use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RosterDefect {
    /// A roster with nothing in it. A rail with no seats is not a rail.
    NoDestinations,
    /// A destination whose key is empty, at this position.
    BlankKey {
        /// Where in the roster.
        at: usize,
    },
    /// Two destinations answer to one key, so a press is ambiguous and the
    /// second page is unreachable.
    DuplicateKey {
        /// The key both claim.
        key: String,
        /// The first claimant.
        first: usize,
        /// The second.
        again: usize,
    },
    /// Every destination is closed, so the region can never show anything.
    NoOpenDestination,
    /// The journey was asked to begin somewhere the roster does not hold.
    NoSuchOpening {
        /// The key that was asked for.
        key: String,
    },
    /// The journey was asked to begin at a destination that is closed.
    OpeningIsClosed {
        /// The key that was asked for.
        key: String,
    },
}

/// The product's destinations, in rail order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destinations {
    list: Vec<Destination>,
}

impl Destinations {
    /// Build a roster, refusing one that cannot be navigated.
    ///
    /// # Errors
    ///
    /// [`RosterDefect`] — empty, a blank key, two destinations sharing a key,
    /// or nothing open to arrive at.
    pub fn new(list: Vec<Destination>) -> Result<Self, RosterDefect> {
        if list.is_empty() {
            return Err(RosterDefect::NoDestinations);
        }
        for (at, destination) in list.iter().enumerate() {
            if destination.key.is_empty() {
                return Err(RosterDefect::BlankKey { at });
            }
            if let Some(first) = list[..at].iter().position(|d| d.key == destination.key) {
                return Err(RosterDefect::DuplicateKey {
                    key: destination.key.clone().into_owned(),
                    first,
                    again: at,
                });
            }
        }
        if !list.iter().any(|d| d.standing.is_open()) {
            return Err(RosterDefect::NoOpenDestination);
        }
        Ok(Self { list })
    }

    /// The destinations, in rail order.
    #[must_use]
    pub fn all(&self) -> &[Destination] {
        &self.list
    }

    /// How many destinations the product declares.
    #[must_use]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Never true — the constructor refuses an empty roster — and present
    /// because a `len` without it is a lint and a reader's double-take.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The destination with this key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Destination> {
        self.list.iter().find(|d| d.key == key)
    }

    /// The keys, in rail order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.list.iter().map(|d| d.key.as_ref())
    }

    /// The destinations that can be arrived at.
    pub fn open(&self) -> impl Iterator<Item = &Destination> {
        self.list.iter().filter(|d| d.standing.is_open())
    }

    /// The destinations that cannot, each with its reason.
    pub fn closed(&self) -> impl Iterator<Item = (&Destination, &Unavailable)> {
        self.list
            .iter()
            .filter_map(|d| d.standing.why().map(|why| (d, why)))
    }

    /// The roster and the current position, as one published value.
    ///
    /// Built here rather than at each screen so two screens of one product
    /// cannot publish the same fact in two shapes — which is exactly what the
    /// two analysis-tool screens were doing before this module existed.
    #[must_use]
    pub fn wire(&self, journey: &Journey) -> serde_json::Value {
        serde_json::json!({
            "at": journey.at(),
            "destinations": self
                .list
                .iter()
                .map(|d| {
                    let why = d.standing.why();
                    serde_json::json!({
                        "key": d.key,
                        "title": d.title,
                        "open": d.standing.is_open(),
                        "current": journey.at() == d.key,
                        "kind": why.map(|w| w.kind().name()),
                        "detail": why.map(Unavailable::detail),
                        "recourse": why.map(|w| w.recourse().name()),
                        "sentence": why.map(Unavailable::sentence),
                    })
                })
                .collect::<Vec<_>>(),
        })
    }
}

/// Where you are.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journey {
    at: String,
}

/// What happened when a journey was asked to move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arrival {
    /// The region now shows a different destination.
    Moved {
        /// Where the journey was.
        from: String,
        /// Where it is.
        to: String,
    },
    /// The destination asked for is the one already showing.
    ///
    /// An arm rather than an error: pressing the seat you are on is a normal
    /// thing a person does, and it is also the case a gate has to be able to
    /// tell apart from a refusal — "nothing changed" is the same observation
    /// for both, and only one of them is a fault.
    AlreadyHere,
}

/// Why a journey did not move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detour {
    /// No destination answers to that key.
    NoSuchDestination {
        /// The key that was asked for.
        key: String,
    },
    /// The destination is on the rail and cannot be arrived at.
    Closed {
        /// The key that was asked for.
        key: String,
        /// Why, in the vocabulary the seat itself is painted from.
        why: Unavailable,
    },
}

impl Detour {
    /// The refusal as one phrase, for a reader.
    ///
    /// Derived, so the message a person is shown and the reason on the wire
    /// cannot disagree. Before this module the two analysis-tool screens
    /// authored their own sentences and one of them told a reserved destination
    /// the wrong thing.
    #[must_use]
    pub fn sentence(&self, roster: &Destinations) -> String {
        match self {
            Detour::NoSuchDestination { key } => format!("there is no {key} to go to"),
            Detour::Closed { key, why } => {
                let title = roster.get(key).map_or(key.as_str(), |d| d.title.as_ref());
                format!("{title} is {}", why.sentence())
            }
        }
    }
}

impl Journey {
    /// Begin at a destination, refusing one that is missing or closed.
    ///
    /// # Errors
    ///
    /// [`RosterDefect::NoSuchOpening`] when the roster does not hold it, and
    /// [`RosterDefect::OpeningIsClosed`] when it does and it is shut — a screen
    /// that opened on a destination it refuses to navigate to would be in a
    /// state no press could have produced.
    pub fn begin(roster: &Destinations, at: &str) -> Result<Self, RosterDefect> {
        match roster.get(at) {
            None => Err(RosterDefect::NoSuchOpening { key: at.to_owned() }),
            Some(destination) if !destination.standing.is_open() => {
                Err(RosterDefect::OpeningIsClosed { key: at.to_owned() })
            }
            Some(_) => Ok(Self { at: at.to_owned() }),
        }
    }

    /// The destination the region is showing.
    #[must_use]
    pub fn at(&self) -> &str {
        &self.at
    }

    /// Go to a destination.
    ///
    /// The only way the current destination changes, so a screen cannot arrive
    /// somewhere no press could have taken it.
    ///
    /// # Errors
    ///
    /// [`Detour`] — the key is not on the roster, or its destination is closed.
    /// The reference toolkit's paged container returns nothing in either case:
    /// an out-of-range ordinal is a silent no-op, and a disabled page is
    /// arrived at regardless.
    pub fn navigate(&mut self, roster: &Destinations, key: &str) -> Result<Arrival, Detour> {
        let Some(destination) = roster.get(key) else {
            return Err(Detour::NoSuchDestination {
                key: key.to_owned(),
            });
        };
        if let Some(why) = destination.standing.why() {
            return Err(Detour::Closed {
                key: key.to_owned(),
                why: why.clone(),
            });
        }
        if self.at == key {
            return Ok(Arrival::AlreadyHere);
        }
        let from = std::mem::replace(&mut self.at, key.to_owned());
        Ok(Arrival::Moved {
            from,
            to: key.to_owned(),
        })
    }

    /// The destination that is showing, resolved against the roster.
    ///
    /// # Panics
    ///
    /// Never, for a journey built by [`begin`](Self::begin) against the roster
    /// it is asked about: both constructors check the key is present, and
    /// [`navigate`](Self::navigate) only ever moves to a key it resolved. It
    /// panics if a journey is asked about a *different* roster, which is a
    /// programming error rather than a state a screen can reach.
    #[must_use]
    pub fn here<'r>(&self, roster: &'r Destinations) -> &'r Destination {
        roster
            .get(&self.at)
            .expect("a journey's destination is on the roster it was begun against")
    }
}

#[cfg(test)]
mod tests {
    use super::{Arrival, Destination, Destinations, Detour, Journey, RosterDefect, Standing};
    use crate::availability::{Recourse, Unavailable};

    fn roster() -> Destinations {
        Destinations::new(vec![
            Destination::open("dashboard", "Dashboard"),
            Destination::open("settings", "Settings"),
            Destination::closed(
                "stream",
                "Stream",
                Unavailable::elsewhere("the packet viewer"),
            ),
            Destination::closed(
                "topology",
                "Topology",
                Unavailable::reserved("requirement 12"),
            ),
        ])
        .expect("roster")
    }

    /// R1695 — going nowhere and going somewhere shut are different answers,
    /// and both are answers.
    ///
    /// The floor this is measured against returns `void` for the first and
    /// **arrives** for the second, so a caller there cannot distinguish a
    /// refusal from a success without re-reading the index afterwards — and
    /// cannot distinguish it at all in the disabled case, because there is no
    /// refusal to distinguish.
    #[test]
    fn r1695_a_refused_journey_says_which_refusal_it_is() {
        let roster = roster();
        let mut journey = Journey::begin(&roster, "dashboard").expect("begin");

        assert_eq!(
            journey.navigate(&roster, "nowhere"),
            Err(Detour::NoSuchDestination {
                key: "nowhere".to_owned()
            })
        );
        assert_eq!(journey.at(), "dashboard", "a refusal does not move");

        let shut = journey.navigate(&roster, "topology").expect_err("closed");
        assert_eq!(
            shut,
            Detour::Closed {
                key: "topology".to_owned(),
                why: Unavailable::reserved("requirement 12"),
            }
        );
        assert_eq!(
            journey.at(),
            "dashboard",
            "a closed destination does not move"
        );
        assert_eq!(
            shut.sentence(&roster),
            "Topology is reserved for requirement 12"
        );
    }

    /// R1695 — arriving, and being told you are already there, are not the same
    /// answer even though the region looks identical after both.
    #[test]
    fn r1695_already_here_is_not_a_refusal() {
        let roster = roster();
        let mut journey = Journey::begin(&roster, "dashboard").expect("begin");

        assert_eq!(
            journey.navigate(&roster, "dashboard"),
            Ok(Arrival::AlreadyHere)
        );
        assert_eq!(
            journey.navigate(&roster, "settings"),
            Ok(Arrival::Moved {
                from: "dashboard".to_owned(),
                to: "settings".to_owned(),
            })
        );
        assert_eq!(journey.at(), "settings");
        assert_eq!(journey.here(&roster).title, "Settings");
    }

    /// R1695 — the roster refuses the joins that make a rail unusable, at
    /// construction, rather than painting a seat whose press is ambiguous.
    #[test]
    fn r1695_a_roster_refuses_a_join_it_cannot_navigate() {
        assert_eq!(
            Destinations::new(vec![]).expect_err("empty"),
            RosterDefect::NoDestinations
        );
        assert_eq!(
            Destinations::new(vec![Destination::open("", "Nameless")]).expect_err("blank"),
            RosterDefect::BlankKey { at: 0 }
        );
        assert_eq!(
            Destinations::new(vec![
                Destination::open("a", "First"),
                Destination::open("b", "Second"),
                Destination::open("a", "Third"),
            ])
            .expect_err("duplicate"),
            RosterDefect::DuplicateKey {
                key: "a".to_owned(),
                first: 0,
                again: 2,
            }
        );
        assert_eq!(
            Destinations::new(vec![Destination::closed(
                "a",
                "First",
                Unavailable::reserved("later"),
            )])
            .expect_err("all shut"),
            RosterDefect::NoOpenDestination
        );
    }

    /// R1695 — a journey cannot begin somewhere a press could not have taken it.
    #[test]
    fn r1695_a_journey_cannot_open_at_a_closed_destination() {
        let roster = roster();
        assert_eq!(
            Journey::begin(&roster, "topology").expect_err("closed"),
            RosterDefect::OpeningIsClosed {
                key: "topology".to_owned()
            }
        );
        assert_eq!(
            Journey::begin(&roster, "ghost").expect_err("missing"),
            RosterDefect::NoSuchOpening {
                key: "ghost".to_owned()
            }
        );
    }

    /// R1695 — the published shape carries the reason a closed destination is
    /// closed, and what a reader can do about it.
    ///
    /// The floor publishes neither: its container's accessible value is empty,
    /// so no client can even ask which destination is current.
    #[test]
    fn r1695_the_wire_carries_the_standing_and_its_recourse() {
        let roster = roster();
        let journey = Journey::begin(&roster, "dashboard").expect("begin");
        let wire = roster.wire(&journey);

        assert_eq!(wire["at"], "dashboard");
        let rows = wire["destinations"].as_array().expect("rows");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["current"], true);
        assert_eq!(rows[0]["kind"], serde_json::Value::Null);

        assert_eq!(rows[2]["key"], "stream");
        assert_eq!(rows[2]["open"], false);
        assert_eq!(rows[2]["kind"], "elsewhere");
        assert_eq!(rows[2]["recourse"], Recourse::OpenElsewhere.name());
        assert_eq!(rows[2]["sentence"], "in the packet viewer");

        assert_eq!(rows[3]["kind"], "reserved");
        assert_eq!(rows[3]["recourse"], Recourse::AwaitRelease.name());
    }

    /// R1695 — the partition is total: every destination is open or says why
    /// not, and the two iterators are complements.
    #[test]
    fn r1695_open_and_closed_partition_the_roster() {
        let roster = roster();
        let open: Vec<_> = roster.open().map(|d| d.key.as_ref()).collect();
        let closed: Vec<_> = roster.closed().map(|(d, _)| d.key.as_ref()).collect();
        assert_eq!(open, vec!["dashboard", "settings"]);
        assert_eq!(closed, vec!["stream", "topology"]);
        assert_eq!(open.len() + closed.len(), roster.len());
        assert!(!roster.is_empty());
        for destination in roster.all() {
            assert_eq!(
                destination.standing.is_open(),
                matches!(destination.standing, Standing::Open),
                "{} disagrees with its own arm",
                destination.key
            );
        }
    }
}
