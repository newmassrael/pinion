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

// --- The specification half ---------------------------------------------------

/// ★★★★★ R1728 — **what a specification fixes about one destination.**
///
/// Not [`Standing`], and the difference is the whole reason this is a separate
/// type. A specification says *this seat is closed, and it is closed because
/// nobody has built it*; it does not say *"specified and not built yet: the
/// behaviour specification"*. The wording is the application's, the **kind** is
/// the specification's, and a checker that demanded both would fail on prose
/// and pass on a seat closed for entirely the wrong reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Required {
    /// The specification says a reader arrives here.
    Open,
    /// The specification says the seat is on the rail and not arrivable, for
    /// this reason.
    Closed(crate::availability::UnavailableKind),
}

impl Required {
    /// What this reads as in a divergence sentence.
    #[must_use]
    pub fn phrase(self) -> String {
        match self {
            Required::Open => "open".to_owned(),
            Required::Closed(kind) => format!("closed ({})", kind.name()),
        }
    }

    /// What a live destination's standing *is*, in the same vocabulary, so the
    /// two sides of a comparison are the same kind of sentence.
    #[must_use]
    pub fn of(standing: &Standing) -> Self {
        match standing {
            Standing::Open => Required::Open,
            Standing::Closed(why) => Required::Closed(why.kind()),
        }
    }
}

/// One destination as a written-down specification states it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeatSpec {
    /// The key the application must address it by.
    pub key: Cow<'static, str>,
    /// What a reader must be able to call it.
    pub title: Cow<'static, str>,
    /// Whether it must be arrivable, and if not, why not.
    pub required: Required,
}

impl SeatSpec {
    /// A seat the specification says a reader arrives at.
    #[must_use]
    pub fn open(key: impl Into<Cow<'static, str>>, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
            required: Required::Open,
        }
    }

    /// A seat the specification says is present and not arrivable, for a
    /// stated kind of reason.
    #[must_use]
    pub fn closed(
        key: impl Into<Cow<'static, str>>,
        title: impl Into<Cow<'static, str>>,
        kind: crate::availability::UnavailableKind,
    ) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
            required: Required::Closed(kind),
        }
    }
}

/// ★★★★★ R1728 — **one way an application's navigation differs from the
/// navigation that was specified for it.**
///
/// Every arm names the key it is about and both sides of the disagreement, so a
/// report is readable without the specification in the other hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Divergence {
    /// The specification has this seat and the application does not.
    Absent {
        /// The key the specification declares.
        key: String,
        /// What the specification calls it.
        title: String,
        /// Where in the specified order it belongs.
        at: usize,
    },
    /// The application has this seat and the specification does not.
    ///
    /// Reported rather than tolerated: a rail that invents a seat is a rail
    /// that has stopped being the specified one, and *which* direction a
    /// difference runs in is exactly what a one-directional check cannot say.
    Unspecified {
        /// The key the application declares.
        key: String,
        /// Where it sits in the application's order.
        at: usize,
    },
    /// Both have the seat, in different places.
    OutOfOrder {
        /// The key.
        key: String,
        /// Where the specification puts it.
        specified_at: usize,
        /// Where the application puts it.
        at: usize,
    },
    /// Both have the seat, under different names.
    Retitled {
        /// The key.
        key: String,
        /// What the specification calls it.
        specified: String,
        /// What the application calls it.
        found: String,
    },
    /// Both have the seat; one can be arrived at and the other cannot, or both
    /// are closed for different kinds of reason.
    Standing {
        /// The key.
        key: String,
        /// What the specification requires.
        specified: Required,
        /// What the application offers.
        found: Required,
    },
}

impl Divergence {
    /// The key this divergence is about.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Divergence::Absent { key, .. }
            | Divergence::Unspecified { key, .. }
            | Divergence::OutOfOrder { key, .. }
            | Divergence::Retitled { key, .. }
            | Divergence::Standing { key, .. } => key,
        }
    }

    /// The divergence as one sentence, for a report a person reads.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Divergence::Absent { key, title, at } => {
                format!(
                    "seat {at} `{key}` ({title}) is specified and the application has no such destination"
                )
            }
            Divergence::Unspecified { key, at } => {
                format!("seat {at} `{key}` is on the rail and no specification declares it")
            }
            Divergence::OutOfOrder {
                key,
                specified_at,
                at,
            } => format!("`{key}` is specified at seat {specified_at} and sits at seat {at}"),
            Divergence::Retitled {
                key,
                specified,
                found,
            } => format!("`{key}` is specified as \"{specified}\" and reads \"{found}\""),
            Divergence::Standing {
                key,
                specified,
                found,
            } => format!(
                "`{key}` is specified {} and is {}",
                specified.phrase(),
                found.phrase()
            ),
        }
    }
}

/// ★★★★★ R1728 — **a navigation, written down, that an application can be
/// checked against.**
///
/// # What was missing, measured
///
/// [`Destinations`] made a rail a value, so a screen can no longer disagree
/// with itself about what it contains. It cannot say whether that value is the
/// *right* one. Measured on the analysis tool this project assembles: its
/// behaviour reference is one application with **eight** sections, drawn in one
/// order, identical on every screen — and the shell reproducing it offered
/// seven, three of whose keys the reference does not have, with one seat
/// wearing another seat's icon. Every one of those is a difference a person had
/// to notice by opening two things side by side, and for several hundred rounds
/// nobody did.
///
/// A specification is only a specification if something fails when the product
/// stops matching it. This is that something.
///
/// # The shape
///
/// A [`RosterSpec`] is a list of [`SeatSpec`]s, and [`diff`](Self::diff)
/// compares it with a live [`Destinations`] **in both directions at once**: a
/// seat the specification has and the application lacks, and a seat the
/// application has and no specification declares, are both reported, as are
/// order, title and standing. A one-directional check passes an application
/// that has quietly grown a section, which is the drift that actually happens.
///
/// What it deliberately does *not* fix is the wording of a closed seat's
/// reason: see [`Required`].
///
/// # Against the reference toolkit at 6.11
///
/// There is no row to compare. A paged container there addresses its pages by
/// ordinal, has one bool per page and no vocabulary for why a page is inert, so
/// the *statement* this type checks cannot be written down in the first place —
/// and reordering the rail and changing where a press goes are the same edit.
///
/// # Examples
///
/// ```
/// use pinion_core::availability::{Unavailable, UnavailableKind};
/// use pinion_core::widgets::destination::{Destination, Destinations, RosterSpec, SeatSpec};
///
/// let spec = RosterSpec::new(vec![
///     SeatSpec::open("home", "Home"),
///     SeatSpec::closed("reports", "Reports", UnavailableKind::Unbuilt),
/// ])
/// .expect("a specification is a navigable roster");
///
/// let built = Destinations::new(vec![
///     Destination::open("home", "Home"),
///     Destination::closed("reports", "Reports", Unavailable::unbuilt("the plan")),
/// ])
/// .expect("the rail is navigable");
/// assert!(spec.diff(&built).is_empty());
///
/// // The application ships the page early: same seat, different standing.
/// let shipped = Destinations::new(vec![
///     Destination::open("home", "Home"),
///     Destination::open("reports", "Reports"),
/// ])
/// .expect("the rail is navigable");
/// assert_eq!(
///     spec.diff(&shipped)[0].sentence(),
///     "`reports` is specified closed (unbuilt) and is open",
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RosterSpec {
    seats: Vec<SeatSpec>,
}

impl RosterSpec {
    /// Write down a navigation, refusing one no application could reproduce.
    ///
    /// The same three defects [`Destinations::new`] refuses, checked here too
    /// rather than only on the built side: a specification that declares one
    /// key twice cannot be conformed to, and a checker that discovered that
    /// only by way of a confusing diff would blame the application for a defect
    /// in the specification.
    ///
    /// # Errors
    ///
    /// [`RosterDefect`] — empty, a blank key, two seats sharing a key, or
    /// nothing open to arrive at.
    pub fn new(seats: Vec<SeatSpec>) -> Result<Self, RosterDefect> {
        if seats.is_empty() {
            return Err(RosterDefect::NoDestinations);
        }
        for (at, seat) in seats.iter().enumerate() {
            if seat.key.is_empty() {
                return Err(RosterDefect::BlankKey { at });
            }
            if let Some(first) = seats[..at].iter().position(|s| s.key == seat.key) {
                return Err(RosterDefect::DuplicateKey {
                    key: seat.key.clone().into_owned(),
                    first,
                    again: at,
                });
            }
        }
        if !seats.iter().any(|s| s.required == Required::Open) {
            return Err(RosterDefect::NoOpenDestination);
        }
        Ok(Self { seats })
    }

    /// The seats, in the order the specification draws them.
    #[must_use]
    pub fn seats(&self) -> &[SeatSpec] {
        &self.seats
    }

    /// How many destinations the specification declares.
    #[must_use]
    pub fn len(&self) -> usize {
        self.seats.len()
    }

    /// Whether the specification declares no seats. Never true of a
    /// [`RosterSpec`] that exists — [`new`](Self::new) refuses one — and
    /// present because a length without it reads as an invitation to compare
    /// against zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.seats.is_empty()
    }

    /// Every way `roster` differs from this specification, in both directions.
    ///
    /// Ordered by the specification's seats first, then by the application's,
    /// so a report reads in the order a person looks down the rail. A seat that
    /// diverges in more than one way reports each way once: *renamed and
    /// moved* is two facts and a reader fixing one still needs the other.
    #[must_use]
    pub fn diff(&self, roster: &Destinations) -> Vec<Divergence> {
        let mut out = Vec::new();
        let built = roster.all();
        for (specified_at, seat) in self.seats.iter().enumerate() {
            let Some(at) = built.iter().position(|d| d.key == seat.key) else {
                out.push(Divergence::Absent {
                    key: seat.key.clone().into_owned(),
                    title: seat.title.clone().into_owned(),
                    at: specified_at,
                });
                continue;
            };
            let found = &built[at];
            if at != specified_at {
                out.push(Divergence::OutOfOrder {
                    key: seat.key.clone().into_owned(),
                    specified_at,
                    at,
                });
            }
            if found.title != seat.title {
                out.push(Divergence::Retitled {
                    key: seat.key.clone().into_owned(),
                    specified: seat.title.clone().into_owned(),
                    found: found.title.clone().into_owned(),
                });
            }
            let standing = Required::of(&found.standing);
            if standing != seat.required {
                out.push(Divergence::Standing {
                    key: seat.key.clone().into_owned(),
                    specified: seat.required,
                    found: standing,
                });
            }
        }
        for (at, destination) in built.iter().enumerate() {
            if !self.seats.iter().any(|s| s.key == destination.key) {
                out.push(Divergence::Unspecified {
                    key: destination.key.clone().into_owned(),
                    at,
                });
            }
        }
        out
    }

    /// Whether `roster` reproduces this specification exactly.
    #[must_use]
    pub fn conforms(&self, roster: &Destinations) -> bool {
        self.diff(roster).is_empty()
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

    /// ★★★★ R1718 — both reasons a journey can refuse to move are said, and
    /// they do not read alike.
    ///
    /// This producer exists because two screens authored their own wording for
    /// the same refusal and one of them told a reserved destination the wrong
    /// thing. That was the reason to derive it; this is the check that the
    /// derivation actually says two different things.
    #[test]
    fn r1718_both_refusals_a_journey_can_give_are_said_and_distinct() {
        use crate::availability::Unavailable;
        use crate::test_fixtures::speech::assert_speaks;

        let roster = Destinations::new(vec![
            Destination::open("packets", "Packets"),
            Destination::closed(
                "dashboard",
                "Dashboard",
                Unavailable::reserved("the second release"),
            ),
        ])
        .expect("two distinct destinations are a roster");
        let said = [
            (
                "NoSuchDestination",
                Detour::NoSuchDestination {
                    key: "nowhere".to_owned(),
                }
                .sentence(&roster),
            ),
            (
                "Closed",
                Detour::Closed {
                    key: "dashboard".to_owned(),
                    why: Unavailable::reserved("the second release"),
                }
                .sentence(&roster),
            ),
        ];
        assert_speaks("Detour", 2, &said, &[]);
    }

    // --- The specification half -----------------------------------------------

    use super::{Divergence, Required, RosterSpec, SeatSpec};
    use crate::availability::UnavailableKind;

    /// The specification the divergence tests measure against: three seats, one
    /// of each standing a rail actually has.
    fn spec() -> RosterSpec {
        RosterSpec::new(vec![
            SeatSpec::open("dashboard", "Dashboard"),
            SeatSpec::open("lab", "Node Lab"),
            SeatSpec::closed("logs", "Logs", UnavailableKind::Unbuilt),
        ])
        .expect("the specification is a navigable roster")
    }

    /// The application that reproduces it exactly.
    fn conforming() -> Destinations {
        Destinations::new(vec![
            Destination::open("dashboard", "Dashboard"),
            Destination::open("lab", "Node Lab"),
            Destination::closed(
                "logs",
                "Logs",
                Unavailable::unbuilt("the behaviour reference"),
            ),
        ])
        .expect("the rail is navigable")
    }

    #[test]
    fn a_reproduction_diverges_in_no_way() {
        let spec = spec();
        assert!(spec.conforms(&conforming()));
        assert_eq!(spec.diff(&conforming()), Vec::new());
        assert_eq!(spec.len(), 3);
        assert!(!spec.is_empty());
    }

    /// ★ The wording of a closed seat's reason is the application's and the
    /// KIND is the specification's — so the same seat closed with entirely
    /// different prose still conforms, and the same prose under a different
    /// kind does not. Both directions, because a checker that fixed the wording
    /// would fail on translation and one that ignored the kind would pass a
    /// seat closed for the wrong reason.
    #[test]
    fn a_specification_fixes_the_kind_of_a_reason_and_not_its_wording() {
        let differently_worded = Destinations::new(vec![
            Destination::open("dashboard", "Dashboard"),
            Destination::open("lab", "Node Lab"),
            Destination::closed(
                "logs",
                "Logs",
                Unavailable::unbuilt("nobody has written it"),
            ),
        ])
        .expect("the rail is navigable");
        assert!(spec().conforms(&differently_worded));

        let wrong_kind = Destinations::new(vec![
            Destination::open("dashboard", "Dashboard"),
            Destination::open("lab", "Node Lab"),
            // Same words, and it now tells a reader to wait for a release the
            // seat is not booked into.
            Destination::closed(
                "logs",
                "Logs",
                Unavailable::reserved("the behaviour reference"),
            ),
        ])
        .expect("the rail is navigable");
        assert_eq!(
            spec()
                .diff(&wrong_kind)
                .iter()
                .map(Divergence::sentence)
                .collect::<Vec<_>>(),
            ["`logs` is specified closed (unbuilt) and is closed (reserved)"],
        );
    }

    /// ★★ The direction a one-sided check cannot see: the application grew a
    /// seat. Nothing is absent, nothing moved, nothing was renamed — and the
    /// rail is no longer the specified one.
    #[test]
    fn a_seat_the_specification_does_not_declare_is_reported() {
        let grown = Destinations::new(vec![
            Destination::open("dashboard", "Dashboard"),
            Destination::open("lab", "Node Lab"),
            Destination::closed(
                "logs",
                "Logs",
                Unavailable::unbuilt("the behaviour reference"),
            ),
            Destination::open("scratch", "Scratch"),
        ])
        .expect("the rail is navigable");
        let found = spec().diff(&grown);
        assert_eq!(
            found,
            [Divergence::Unspecified {
                key: "scratch".to_owned(),
                at: 3,
            }],
        );
        assert_eq!(
            found[0].sentence(),
            "seat 3 `scratch` is on the rail and no specification declares it",
        );
        assert_eq!(found[0].key(), "scratch");
    }

    #[test]
    fn a_seat_the_application_lacks_is_reported_with_where_it_belongs() {
        let missing = Destinations::new(vec![
            Destination::open("dashboard", "Dashboard"),
            Destination::open("lab", "Node Lab"),
        ])
        .expect("the rail is navigable");
        assert_eq!(
            spec()
                .diff(&missing)
                .iter()
                .map(Divergence::sentence)
                .collect::<Vec<_>>(),
            ["seat 2 `logs` (Logs) is specified and the application has no such destination"],
        );
    }

    /// ★ Order is part of the statement. The reference draws one rail in one
    /// order on every screen, and a rail with the right seats in the wrong
    /// places is a different rail to the hand that reaches for one.
    #[test]
    fn two_seats_that_swapped_places_are_both_reported() {
        let swapped = Destinations::new(vec![
            Destination::open("lab", "Node Lab"),
            Destination::open("dashboard", "Dashboard"),
            Destination::closed(
                "logs",
                "Logs",
                Unavailable::unbuilt("the behaviour reference"),
            ),
        ])
        .expect("the rail is navigable");
        assert_eq!(
            spec()
                .diff(&swapped)
                .iter()
                .map(Divergence::sentence)
                .collect::<Vec<_>>(),
            [
                "`dashboard` is specified at seat 0 and sits at seat 1",
                "`lab` is specified at seat 1 and sits at seat 0",
            ],
        );
    }

    /// ★★★ A seat that diverges in more than one way reports each way. The
    /// fixture is built so that fixing either one alone still leaves the rail
    /// wrong — which is what a report collapsing them into one finding would
    /// hide.
    #[test]
    fn one_seat_can_diverge_in_three_ways_at_once() {
        let mangled = Destinations::new(vec![
            Destination::open("dashboard", "Dashboard"),
            Destination::open("logs", "Log Viewer"),
            Destination::open("lab", "Node Lab"),
        ])
        .expect("the rail is navigable");
        assert_eq!(
            spec()
                .diff(&mangled)
                .iter()
                .map(Divergence::sentence)
                .collect::<Vec<_>>(),
            [
                "`lab` is specified at seat 1 and sits at seat 2",
                "`logs` is specified at seat 2 and sits at seat 1",
                "`logs` is specified as \"Logs\" and reads \"Log Viewer\"",
                "`logs` is specified closed (unbuilt) and is open",
            ],
        );
    }

    /// A specification is refused for the same three defects a roster is, so a
    /// confusing diff never has to stand in for "the specification is wrong".
    #[test]
    fn a_specification_that_cannot_be_conformed_to_is_refused() {
        assert_eq!(
            RosterSpec::new(Vec::new()),
            Err(RosterDefect::NoDestinations)
        );
        assert_eq!(
            RosterSpec::new(vec![SeatSpec::open("", "Nameless")]),
            Err(RosterDefect::BlankKey { at: 0 }),
        );
        assert_eq!(
            RosterSpec::new(vec![
                SeatSpec::open("home", "Home"),
                SeatSpec::open("home", "Home again"),
            ]),
            Err(RosterDefect::DuplicateKey {
                key: "home".to_owned(),
                first: 0,
                again: 1,
            }),
        );
        assert_eq!(
            RosterSpec::new(vec![SeatSpec::closed(
                "logs",
                "Logs",
                UnavailableKind::Unbuilt
            )]),
            Err(RosterDefect::NoOpenDestination),
        );
    }

    /// ★★★★ R1728 — **every way a rail can differ from its specification is
    /// said, and no two of them read alike.**
    ///
    /// The producer this drives is the one a person reads when a conformance
    /// gate fails, so a wording that collapsed two situations into one sentence
    /// would send whoever is fixing it after the wrong thing — the exact
    /// failure R1718 built this fixture after. *Renamed* and *moved* in
    /// particular must not read alike: a seat can be both at once, and the
    /// report says so twice.
    #[test]
    fn r1728_every_way_a_rail_can_diverge_is_said_and_distinct() {
        use crate::test_fixtures::speech::assert_speaks;

        let said = [
            (
                "Absent",
                Divergence::Absent {
                    key: "logs".to_owned(),
                    title: "Logs".to_owned(),
                    at: 3,
                }
                .sentence(),
            ),
            (
                "Unspecified",
                Divergence::Unspecified {
                    key: "scratch".to_owned(),
                    at: 8,
                }
                .sentence(),
            ),
            (
                "OutOfOrder",
                Divergence::OutOfOrder {
                    key: "settings".to_owned(),
                    specified_at: 7,
                    at: 4,
                }
                .sentence(),
            ),
            (
                "Retitled",
                Divergence::Retitled {
                    key: "packets".to_owned(),
                    specified: "Packets".to_owned(),
                    found: "Stream".to_owned(),
                }
                .sentence(),
            ),
            (
                "Standing",
                Divergence::Standing {
                    key: "keys".to_owned(),
                    specified: Required::Open,
                    found: Required::Closed(UnavailableKind::Unbuilt),
                }
                .sentence(),
            ),
        ];
        assert_speaks("Divergence", 5, &said, &[]);
        // ★ And each names the seat it is about, so a report is readable
        // without the specification in the other hand.
        for (arm, sentence) in &said {
            assert!(
                sentence.contains('`'),
                "Divergence::{arm} says {sentence:?} without naming a seat",
            );
        }
    }

    /// `Required::of` is what makes the two sides of a comparison the same kind
    /// of sentence, so it is checked against every standing a rail can hold
    /// rather than the two this file's fixtures happen to use.
    #[test]
    fn a_live_standing_reads_in_the_specifications_vocabulary() {
        assert_eq!(Required::of(&Standing::Open), Required::Open);
        for kind in UnavailableKind::ALL {
            let standing = Standing::Closed(Unavailable::new(kind, "why"));
            assert_eq!(Required::of(&standing), Required::Closed(kind));
            assert_eq!(
                Required::Closed(kind).phrase(),
                format!("closed ({})", kind.name()),
            );
        }
        assert_eq!(Required::Open.phrase(), "open");
    }
}
