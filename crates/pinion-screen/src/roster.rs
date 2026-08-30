//! ★★★★★ R1724 — **the roster: which destination is which screen, and the one
//! rule that makes the others hold.**
//!
//! The rule: *the screen the journey is at is the only one anything reaches.*
//! Every accessor here is keyed by a [`Journey`], so there is no expression in
//! this crate that hands out a screen the application is not showing — which is
//! the difference between this and the reference toolkit's paged container,
//! where a hidden page counted a press, a key and a wheel, appeared in the
//! accessibility tree with its children, and left its floating windows on
//! screen (all four measured at 6.11.1).

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use pinion_core::chrome::{HostChrome, with_host_chrome_for};
use pinion_core::external::with_surface_extent;
use pinion_core::shrink::{ShrinkPolicy, pan};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::destination::{Destination, Destinations, Journey, Standing};
use pinion_core::{Frame, Scene};

use crate::Screen;
use crate::conformance::{
    ApplicationConformance, SectionJudge, SectionPoser, SectionRow, SectionStanding, Showing,
};
use crate::journey::{JourneyConformance, JourneySection, JourneyStanding, Walk};

/// A host's cached projection: where it is, and how far the screen it is
/// showing has moved.
///
/// This is what a host declares as its
/// [`WidgetCore::State`](pinion_core::WidgetCore::State). Neither field is read
/// as a quantity — together they are the change detector that makes the
/// framework repaint a host whose own state is constant while the screen inside
/// it is not. A host with `State = ()` mounting a screen with a text field
/// would otherwise paint the field's first frame and no other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ScreenState {
    /// The destination's position in the roster.
    ///
    /// The position and not the key, because a host's state must be `Copy`.
    /// It changes on every arrival, which is the half of the detector that
    /// notices *navigation*.
    pub at: u32,
    /// The current screen's latch revision — the half that notices the screen
    /// itself moving.
    pub revision: u64,
}

/// What is wrong with a pairing of destinations and the things that answer for
/// them.
///
/// ★ R1761 — it was `MountDefect`, and every arm was about a screen. A
/// destination's answer can now be a screen *or* a judge for a page the host
/// paints itself, and the two share every refusal but one; a type named for
/// half its arms is a type a reader stops believing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RosterDefect {
    /// Something was placed at a key the destination roster does not hold, so
    /// nothing could ever navigate to it.
    NoSuchDestination {
        /// The key it was placed at.
        key: String,
    },
    /// Something was placed at a destination that is closed, so the roster
    /// says one thing and the placement says another.
    ///
    /// The direction that matters: a seat declared
    /// [`Unavailable::elsewhere`](pinion_core::availability::Unavailable::elsewhere)
    /// — *built, shipping, and not here* — with the screen mounted right here
    /// is a sentence the application would be telling a reader while showing
    /// them the opposite. A judge at a closed destination is the same sentence
    /// one level quieter: a verdict about a section nobody can arrive at.
    DestinationIsClosed {
        /// The key it was placed at.
        key: String,
    },
    /// Two screens were mounted at one key.
    DuplicateMount {
        /// The key both claim.
        key: String,
    },
    /// Two judges were registered for one key.
    DuplicateJudge {
        /// The key both claim.
        key: String,
    },
    /// ★ R1864 — two posers were registered for one key.
    DuplicatePoser {
        /// The key both claim.
        key: String,
    },
    /// ★★★★★ R1761 — a judge was registered for a destination that already has
    /// a screen, so two things claim to answer for one section.
    ///
    /// The refusal exists because the alternative is silent: whichever the
    /// lookup happened to reach would win, and a section whose verdict depends
    /// on the order two registrations were written in is a section nobody can
    /// check. A mounted screen answers for itself — that is what
    /// [`Screen::conformance`] is — so a host with something to add about it
    /// has a screen to put it in.
    SectionAlreadyAnswers {
        /// The key with both a screen and a judge.
        key: String,
    },
    /// ★★★★★ R1784 — a size was declared for a destination that already has a
    /// screen, so two things claim to say what one section lays out in.
    ///
    /// The same rule as [`SectionAlreadyAnswers`](Self::SectionAlreadyAnswers)
    /// and for the same reason: a screen states its own policy through
    /// [`Screen::shrink_policy`], and a host adding a second one would make the
    /// answer depend on which lookup happened first. A host with something to
    /// say about a mounted screen's size has a screen to say it in.
    SectionAlreadySized {
        /// The key with both a screen and a host-side size.
        key: String,
    },
    /// ★ R1784 — two sizes declared for one destination.
    DuplicateSize {
        /// The key both claim.
        key: String,
    },
    /// ★ R1830 — two grants declared for one destination.
    DuplicateGrant {
        /// The key both claim.
        key: String,
    },
    /// ★★★★★ R1911 — a paint claim was made for a destination that already has
    /// a screen, so two things claim to say where one section's marks are.
    ///
    /// The same rule as [`SectionAlreadySized`](Self::SectionAlreadySized) and
    /// for the same reason: a mounted screen's paint root is
    /// [`Screen::tag`], which is also the address its own
    /// externals answer at, so a host-side second opinion would make "where is
    /// this section on the frame" depend on which lookup was read first.
    SectionAlreadyPaints {
        /// The key with both a screen and a host-side paint claim.
        key: String,
    },
    /// ★ R1911 — two paint claims declared for one destination.
    DuplicatePaint {
        /// The key both claim.
        key: String,
    },
    /// ★★★★★ R1911 — two sections claim the same marks.
    ///
    /// **The refusal that makes "leaving takes a section away" mean anything.**
    /// The check that property is asserted by reads a section's stems off the
    /// frame and requires every *other* section's to be absent; if one
    /// section's claim contained another's, the containing section would be
    /// found painted at every destination and the containing claim would have
    /// to be excused. An excused claim is an unchecked one, so the overlap is
    /// refused where it is declared rather than tolerated where it is read.
    PaintAlreadyClaimed {
        /// The key making the claim.
        key: String,
        /// The key that already holds marks this claim would also take.
        by: String,
        /// The stem the two claims meet at.
        stem: String,
    },
    /// ★★★★★ R1911 — a section's marks and the host's chrome are the same
    /// marks.
    ///
    /// Chrome is what the host paints at **every** destination, so a mark that
    /// is both would be found on every frame and could never be shown to go
    /// away. Reported in one variant whichever half was declared second,
    /// because the defect is the pair and not the order two builder calls
    /// happen to be written in — the asymmetry the first draft of this had, and
    /// which made the check depend on a line ordering nothing enforced.
    ChromeIsAlsoASection {
        /// The section whose claim the chrome meets.
        key: String,
        /// The stem the two meet at.
        stem: String,
    },
    /// ★★★★★ R1911 — a section claimed no marks at all.
    ///
    /// A destination a reader can arrive at paints *something*, so an empty
    /// claim is not a section that paints nothing — it is a declaration that
    /// says nothing while counting as one, which would let
    /// [`unrooted_keys`](ScreenRoster::unrooted_keys) reach empty without any
    /// section becoming locatable. The escape hatch is refused at the door.
    EmptyPaintClaim {
        /// The key that claimed nothing.
        key: String,
    },
}

impl core::fmt::Display for RosterDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RosterDefect::NoSuchDestination { key } => {
                write!(f, "no destination `{key}` to place a section's answer at")
            }
            RosterDefect::DestinationIsClosed { key } => write!(
                f,
                "destination `{key}` is closed, and a closed destination with a \
                 screen behind it tells a reader the opposite of what it shows"
            ),
            RosterDefect::DuplicateMount { key } => {
                write!(f, "two screens mounted at destination `{key}`")
            }
            RosterDefect::DuplicateJudge { key } => {
                write!(f, "two judges registered for destination `{key}`")
            }
            RosterDefect::DuplicatePoser { key } => {
                write!(f, "two posers registered for destination `{key}`")
            }
            RosterDefect::SectionAlreadyAnswers { key } => write!(
                f,
                "destination `{key}` has a screen, which answers for it; a \
                 second verdict from the host would make the section's own \
                 answer depend on which registration a lookup reached first"
            ),
            RosterDefect::SectionAlreadySized { key } => write!(
                f,
                "destination `{key}` has a screen, which states its own size \
                 policy; a second one from the host would make what the \
                 section lays out in depend on which registration was read"
            ),
            RosterDefect::DuplicateSize { key } => {
                write!(f, "two sizes declared for destination `{key}`")
            }
            RosterDefect::DuplicateGrant { key } => {
                write!(f, "two grants declared for destination `{key}`")
            }
            RosterDefect::SectionAlreadyPaints { key } => write!(
                f,
                "destination `{key}` has a screen, whose paint root is its own \
                 tag; a second claim from the host would make where the \
                 section's marks are depend on which registration was read"
            ),
            RosterDefect::DuplicatePaint { key } => {
                write!(f, "two paint claims declared for destination `{key}`")
            }
            RosterDefect::PaintAlreadyClaimed { key, by, stem } => write!(
                f,
                "destination `{key}` claims marks under `{stem}`, which `{by}` \
                 already holds; two sections claiming one mark makes \
                 \"leaving takes a section away\" uncheckable, because the \
                 containing claim is found painted everywhere"
            ),
            RosterDefect::ChromeIsAlsoASection { key, stem } => write!(
                f,
                "`{stem}` is claimed both as the host's chrome and as \
                 `{key}`'s marks; chrome is painted at every destination, so a \
                 mark that is both could never be shown to go away"
            ),
            RosterDefect::EmptyPaintClaim { key } => write!(
                f,
                "destination `{key}` claimed no marks at all; a section a \
                 reader can arrive at paints something, so an empty claim is a \
                 declaration that says nothing while counting as one"
            ),
        }
    }
}

/// Whether two paint stems reach any of the same marks.
///
/// A stem names a tag and everything addressed beneath it, which is the shape
/// R1729's check already read off the frame by hand
/// (`tag == root || tag.starts_with("{root}.")`) — derived here so the
/// declaration and the reading cannot drift apart. Two stems meet when they are
/// equal or one is an ancestor of the other; `shell.palette` and
/// `shell.settings` do not meet, and `shell` meets both.
fn stems_meet(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{b}.")) || b.starts_with(&format!("{a}."))
}

impl std::error::Error for RosterDefect {}

/// The destinations of an application, and the screens behind the ones that
/// have one.
///
/// Not every destination needs a screen: a host that paints some of its own
/// pages inline (a dashboard whose cards are the host's) leaves those keys
/// unmounted, and every accessor here answers `None` for them. That is what
/// lets an application be assembled from screens *and* pages without a second
/// vocabulary for the difference.
pub struct ScreenRoster {
    destinations: Destinations,
    screens: BTreeMap<String, Box<dyn Screen>>,
    /// ★★★★★ R1761 — what answers for the destinations this host paints
    /// itself.
    ///
    /// A second map rather than a second arm of `screens`, because a judge is
    /// not a screen with most of it missing: it has no paint, no hit test and
    /// no externals, and every accessor in this file that hands out a screen
    /// would have to learn to refuse it. Keeping them apart means those
    /// accessors are unchanged and cannot hand one out by accident, and the one
    /// place both are read — [`conformance`](Self::conformance) — refuses a key
    /// that is in both.
    judges: BTreeMap<String, Box<dyn SectionJudge>>,
    /// ★★★★★ R1784 — what a page the host paints itself lays out in.
    ///
    /// A third map for the reason `judges` is a second one: a size is not a
    /// screen with most of it missing either, and the alternative — a screen
    /// that exists only to carry a number — is the route R1761 measured and
    /// refused, since such a screen would judge a section it does not paint.
    sizes: BTreeMap<String, ShrinkPolicy>,
    /// ★★★★★ R1830 — **what the host draws BESIDE each section**, horizontally,
    /// from which what the section is granted is derived.
    ///
    /// `sizes` is what a section *wants*; this is the half that says what it
    /// *gets*. R1784 built the first and left the second in the host: a gate
    /// compared `shrink_policy_of(key)` against the shell's own
    /// `page_rect(key)`, which is one host's function — another host has its
    /// own, and the roster could not check that the two agreed about a single
    /// section.
    ///
    /// ★★★★★ **An INSET and not a width, and that is a measurement rather than
    /// a preference.** The first shape of this field held the granted width
    /// directly, and it could not be built: a host computes that width from the
    /// window, the window is reactive state read through `Owner::cache`, and
    /// this roster is itself constructed inside such a factory — so every
    /// screen-owning test died on `Owner::cache factory closures must not call
    /// Owner::cache`. The width is per-frame; what the host draws beside a page
    /// is not. Holding the static half and deriving the rest is what the debt
    /// that opened this asked for in as many words, and the re-entrancy panic
    /// is what made the difference impossible to ignore.
    ///
    /// A fourth map, for the reason `sizes` is a third one: an inset is not a
    /// size with a different meaning, and folding it into `sizes` would put two
    /// accounts on one fact — the failure this crate is shaped to make
    /// unrepresentable, and the one [`laying_out`](Self::laying_out)'s own
    /// documentation warns about.
    grants: BTreeMap<String, u32>,
    /// ★★★★★ R1864 — how many frames a page the host paints itself needs, and
    /// how to put it in each.
    ///
    /// A fifth map, for the reason `grants` is a fourth one. See
    /// [`SectionPoser`] for the measurement that forced it: a scrolling host
    /// page whose content is taller than its region has parts a walk of one
    /// frame per section cannot see, and reporting those as unreproduced is a
    /// verdict about the frame rather than about the section.
    posers: BTreeMap<String, Box<dyn SectionPoser>>,
    /// ★★★★★ R1911 — where on the frame a page the host paints itself puts its
    /// marks.
    ///
    /// A sixth map, for the reason `posers` is a fifth one. A *set* of stems
    /// rather than the single tag a screen has, because a host page's marks are
    /// not under one address: its chrome is painted beside its region, which is
    /// the same measurement that produced [`judging`](ScreenRoster::judging).
    stems: BTreeMap<String, Vec<String>>,
    /// ★★★★★ R1911 — the stems the host paints at every destination.
    ///
    /// Not per-key, because that is exactly what makes it chrome: it is on the
    /// frame whichever section the reader is at, so it can belong to none of
    /// them. Empty until declared, which leaves
    /// [`unclaimed_marks`](ScreenRoster::unclaimed_marks) reporting the host's
    /// own frame — loud rather than silent, which is the direction an
    /// undeclared population should fail in.
    chrome_stems: Vec<String>,
    /// ★★★★★ R1725 — what this host already provides, which every screen it
    /// shows is told before it builds anything.
    ///
    /// A field on the roster and not an argument at each call site: it is a
    /// fact about the application, the roster is the only path to a screen, and
    /// an argument would let the paint and the hit test be told different
    /// things about the same frame — the failure this whole crate is shaped to
    /// make unrepresentable.
    chrome: HostChrome,
    /// The extent the page region was last laid out at, so every delegated
    /// call reads the rectangle the screen is actually in.
    ///
    /// `(0, 0)` before the first paint — "the region has no extent yet", which
    /// is the one reading [`with_surface_extent`] refuses and this type must
    /// therefore not make.
    placed_extent: Cell<(u32, u32)>,
    /// ★★★★★ R1767 — **what the walk a reader is taking has seen.**
    ///
    /// Kept here rather than by the host for the two reasons in [`Walk`]'s own
    /// documentation: the population is this roster's, and the moment is
    /// [`latch`](Self::latch) — the last instant at which the frame a departing
    /// section actually painted can still be read, since R1763 discards it
    /// immediately after.
    walk: RefCell<Walk>,
}

impl ScreenRoster {
    /// Pair a destination roster with the screens behind its keys.
    ///
    /// # Errors
    ///
    /// [`RosterDefect`] — a screen at a key the roster does not hold, at a key
    /// the roster declares closed, or two screens at one key.
    pub fn new(
        destinations: Destinations,
        mounts: Vec<(&str, Box<dyn Screen>)>,
    ) -> Result<Self, RosterDefect> {
        let mut screens: BTreeMap<String, Box<dyn Screen>> = BTreeMap::new();
        for (key, screen) in mounts {
            Self::placeable(&destinations, key)?;
            if screens.insert(key.to_owned(), screen).is_some() {
                return Err(RosterDefect::DuplicateMount {
                    key: key.to_owned(),
                });
            }
        }
        Ok(Self {
            destinations,
            screens,
            judges: BTreeMap::new(),
            sizes: BTreeMap::new(),
            grants: BTreeMap::new(),
            posers: BTreeMap::new(),
            stems: BTreeMap::new(),
            chrome_stems: Vec::new(),
            chrome: HostChrome::NONE,
            placed_extent: Cell::new((0, 0)),
            walk: RefCell::new(Walk::default()),
        })
    }

    /// The two refusals a screen and a judge share: the destination must exist
    /// and must be one a reader can arrive at.
    ///
    /// Derived rather than written twice, for the reason this crate keeps
    /// finding: two copies of one rule are two rules the moment somebody edits
    /// one of them.
    fn placeable(destinations: &Destinations, key: &str) -> Result<(), RosterDefect> {
        match destinations.get(key) {
            None => Err(RosterDefect::NoSuchDestination {
                key: key.to_owned(),
            }),
            Some(destination) if !destination.standing.is_open() => {
                Err(RosterDefect::DestinationIsClosed {
                    key: key.to_owned(),
                })
            }
            Some(_) => Ok(()),
        }
    }

    /// ★★★★★ R1761 — **register what answers for a page this host paints
    /// itself.**
    ///
    /// # What forced it, measured
    ///
    /// [`SectionStanding::Inline`] said the closing move was to give the page a
    /// [`Screen`] of its own, and this crate's own documentation said the trait
    /// is public for exactly that. Measured on the analysis tool at R1761,
    /// standing on the section that entry had been open for since R1738: the
    /// page region a screen would be granted is 1096×802 at (52, 98), while the
    /// section's layout bar is 1096×46 at (52, 52) and its palette is 292×848
    /// at (1148, 52) — **both outside the page**, because a host that paints
    /// chrome *for* a section paints it beside the region rather than in it.
    ///
    /// A screen judges what it paints. So the recorded route would have
    /// produced a verdict that structurally could not cover the section it was
    /// about — quieter than the silence it replaced, and harder to notice.
    ///
    /// # Why this is not a way out of mounting
    ///
    /// It answers one question and grants nothing else: a judge has no paint,
    /// no hit test, no keys and no accessibility tree, so a page that wants to
    /// *be* a screen still has to become one. What it stops being is the price
    /// of saying a true sentence about a page the host draws.
    ///
    /// # Errors
    ///
    /// [`RosterDefect`] — a judge at a key the roster does not hold, at a key
    /// it declares closed, at a key that already has a screen, or two judges at
    /// one key.
    pub fn judging(
        mut self,
        key: &str,
        judge: Box<dyn SectionJudge>,
    ) -> Result<Self, RosterDefect> {
        Self::placeable(&self.destinations, key)?;
        if self.screens.contains_key(key) {
            return Err(RosterDefect::SectionAlreadyAnswers {
                key: key.to_owned(),
            });
        }
        if self.judges.insert(key.to_owned(), judge).is_some() {
            return Err(RosterDefect::DuplicateJudge {
                key: key.to_owned(),
            });
        }
        Ok(self)
    }

    /// ★★★★★ R1864 — **declare how many frames a page this host paints itself
    /// needs to show all of its specification, and how to put it in each.**
    ///
    /// [`Screen::poses`] is this for a mounted screen.
    /// Without it a host page answered `1` whatever it was, so a scrolling page
    /// taller than its region reported the parts below its fold as
    /// unreproduced — a verdict true of the frame and false of the section. See
    /// [`SectionPoser`] for the measurement.
    ///
    /// Like [`judging`](Self::judging), it grants nothing else: a poser has no
    /// paint, no hit test and no verdict, and a page that wants to *be* a
    /// screen still has to become one.
    ///
    /// # Errors
    ///
    /// [`RosterDefect`] — a poser at a key the roster does not hold, at a key it
    /// declares closed, at a key that already has a screen (a screen answers
    /// `poses` itself, and two accounts of one fact is what this crate is
    /// shaped to make unrepresentable), or two posers at one key.
    pub fn posing(mut self, key: &str, poser: Box<dyn SectionPoser>) -> Result<Self, RosterDefect> {
        Self::placeable(&self.destinations, key)?;
        if self.screens.contains_key(key) {
            return Err(RosterDefect::SectionAlreadyAnswers {
                key: key.to_owned(),
            });
        }
        if self.posers.insert(key.to_owned(), poser).is_some() {
            return Err(RosterDefect::DuplicatePoser {
                key: key.to_owned(),
            });
        }
        Ok(self)
    }

    /// ★★★★★ R1784 — **declare what a page this host paints itself lays out
    /// in**, so the size question reaches every section a reader can arrive at.
    ///
    /// # What forced it, measured
    ///
    /// R1781 gave a host [`shrink_policy_of`](Self::shrink_policy_of) so it
    /// could ask what its guests need without navigating to each one, and the
    /// analysis tool's gate walked [`mounted_keys`](Self::mounted_keys) and
    /// asserted it had asked at least four. Measured at R1784: that
    /// application opens **six** sections and four are mounted screens, so the
    /// two the host paints itself — its dashboard and its settings page — were
    /// not failing the check, they **were not in it**. The assertion read as
    /// though it covered the application.
    ///
    /// That is R1738's finding one property over. There it was conformance: an
    /// application counted what it was judged on, and sections that published
    /// nothing were absent rather than short. Here it is layout, and the
    /// remedy is the same shape — the population comes from the roster, and a
    /// destination that cannot answer is nameable rather than silent (see
    /// [`unsized_keys`](Self::unsized_keys)).
    ///
    /// # Why not a screen
    ///
    /// The same measurement that produced [`judging`](Self::judging): a host
    /// paints a page's chrome *beside* the page region rather than in it, so a
    /// screen mounted there would state a size for a rectangle that is not the
    /// section. This grants nothing else — no paint, no hit test, no verdict —
    /// which is what keeps it from being a way out of mounting.
    ///
    /// # Errors
    ///
    /// [`RosterDefect`] — a size at a key the roster does not hold, at a key it
    /// declares closed, at a key that already has a screen (which states its
    /// own), or two sizes at one key.
    pub fn laying_out(mut self, key: &str, policy: ShrinkPolicy) -> Result<Self, RosterDefect> {
        Self::placeable(&self.destinations, key)?;
        if self.screens.contains_key(key) {
            return Err(RosterDefect::SectionAlreadySized {
                key: key.to_owned(),
            });
        }
        if self.sizes.insert(key.to_owned(), policy).is_some() {
            return Err(RosterDefect::DuplicateSize {
                key: key.to_owned(),
            });
        }
        Ok(self)
    }

    /// ★★★★★ R1911 — **declare where on the frame a page this host paints
    /// itself puts its marks**, so "which section is this mark part of" reaches
    /// every section a reader can arrive at.
    ///
    /// [`Screen::tag`] is this for a mounted screen, and
    /// one stem is enough there because a screen paints into a surface of its
    /// own. A host page has no such surface: measured on the analysis tool at
    /// R1911, its dashboard's marks sit under the stems its cards, its palette
    /// and its layout bar are addressed by — more than one, and not derivable
    /// from the destination key. So this takes a *set*.
    ///
    /// # What forced it, measured
    ///
    /// R1729's check — arriving paints a section, leaving takes it away, and
    /// the host's chrome survives — walks [`mounted_keys`](Self::mounted_keys).
    /// Measured at R1911 on an application with six open sections, four of them
    /// mounted: the two the host paints itself were not failing that check,
    /// they **were not in it**, so nothing anywhere asserted that leaving the
    /// dashboard stops the dashboard being painted.
    ///
    /// The verdict half does not cover it either, and deliberately.
    /// [`Showing`] *hands* a judge the fact that
    /// its page is away, because R1761 refused "away because I found nothing"
    /// as an excuse a page that stopped painting itself would also pass.
    /// Refusing that inference at **runtime** is right, and it leaves the
    /// handed-over claim untested. This is what a gate tests it against.
    ///
    /// # Why this is not a way out of mounting
    ///
    /// It says where a section's marks are — the first of the four a mount
    /// gives, and the one the other three can only be asked *about*. It hands
    /// out no paint, no hit test, no keys and no accessibility subtree: a page
    /// that wants those still has to become a [`Screen`].
    ///
    /// # Errors
    ///
    /// [`RosterDefect`] — a claim at a key the roster does not hold, at a key
    /// it declares closed, at a key that already has a screen (which names its
    /// own root), two claims at one key, an **empty** claim, or a claim
    /// overlapping one another section already holds.
    pub fn painting(mut self, key: &str, stems: &[&str]) -> Result<Self, RosterDefect> {
        Self::placeable(&self.destinations, key)?;
        if self.screens.contains_key(key) {
            return Err(RosterDefect::SectionAlreadyPaints {
                key: key.to_owned(),
            });
        }
        if stems.is_empty() {
            return Err(RosterDefect::EmptyPaintClaim {
                key: key.to_owned(),
            });
        }
        for stem in stems {
            // The host's chrome first, so the pair is refused whichever half
            // was declared second -- the order-dependence the first draft had.
            if let Some(met) = self
                .chrome_stems
                .iter()
                .find(|chrome| stems_meet(chrome, stem))
            {
                return Err(RosterDefect::ChromeIsAlsoASection {
                    key: key.to_owned(),
                    stem: met.clone(),
                });
            }
            for (other, held) in self.claims() {
                if other == key {
                    continue;
                }
                if held.iter().any(|held| stems_meet(held, stem)) {
                    return Err(RosterDefect::PaintAlreadyClaimed {
                        key: key.to_owned(),
                        by: other,
                        stem: (*stem).to_owned(),
                    });
                }
            }
        }
        let claimed: Vec<String> = stems.iter().map(|s| (*s).to_owned()).collect();
        if self.stems.insert(key.to_owned(), claimed).is_some() {
            return Err(RosterDefect::DuplicatePaint {
                key: key.to_owned(),
            });
        }
        Ok(self)
    }

    /// Every section's paint claim, from both sources, for the overlap check.
    ///
    /// A mounted screen's claim is its own root tag, and it is in here so a
    /// host page cannot claim marks a guest already paints — the direction a
    /// check reading only `stems` would miss entirely.
    fn claims(&self) -> Vec<(String, Vec<String>)> {
        self.screens
            .iter()
            .map(|(key, screen)| {
                (
                    key.clone(),
                    screen
                        .paint_stems()
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                )
            })
            .chain(
                self.stems
                    .iter()
                    .map(|(key, stems)| (key.clone(), stems.clone())),
            )
            .collect()
    }

    /// ★★★★★ R1911 — the stems the section at `key` paints its marks under, or
    /// `None` when the roster cannot say where that section is on a frame.
    ///
    /// One stem for a mounted screen — its [`tag`](crate::Screen::tag) — and
    /// whatever [`painting`](Self::painting) declared for a page the host draws
    /// itself. Answering from both sources is what lets a caller ask the
    /// question once for the whole application, which is
    /// [`poses_of`](Self::poses_of)'s shape and for its reason: a caller that
    /// has to know which sections are mounted is a caller keeping by hand the
    /// list this roster exists to publish.
    #[must_use]
    pub fn paint_stems_of(&self, key: &str) -> Option<Vec<&str>> {
        if let Some(screen) = self.screens.get(key) {
            return Some(screen.paint_stems());
        }
        self.stems
            .get(key)
            .map(|stems| stems.iter().map(String::as_str).collect())
    }

    /// ★★★★★ R1911 — whether `tag` is one of the marks the section at `key`
    /// paints.
    ///
    /// Derived from [`paint_stems_of`](Self::paint_stems_of) so that the
    /// declaration and every reading of it are one expression. R1729's check
    /// spelled this rule out by hand at three sites; a rule spelled twice is
    /// two rules the moment one is edited, which is the failure this crate is
    /// shaped to make unrepresentable.
    #[must_use]
    pub fn paints(&self, key: &str, tag: &str) -> bool {
        self.paint_stems_of(key).is_some_and(|stems| {
            stems
                .iter()
                .any(|stem| tag == *stem || tag.starts_with(&format!("{stem}.")))
        })
    }

    /// ★★★★★ R1911 — **whose mark is this**, or `None` for the host's own
    /// chrome and for a mark no section claims.
    ///
    /// The inverse of [`paints`](Self::paints), and the reason
    /// [`PaintAlreadyClaimed`](RosterDefect::PaintAlreadyClaimed) is refused
    /// rather than tolerated: **this function is single-valued exactly because
    /// no two sections may claim one mark.** A refusal with no consumer is a
    /// rule nothing depends on; this is the consumer, so the refusal is now
    /// load-bearing rather than tidy.
    ///
    /// # What it is for, measured
    ///
    /// R1784 wrote the honest remainder of a host-painted page as *its own
    /// paint root, hit testing, keys and an accessibility subtree*, and asked
    /// which of the four such a page actually owes. Measured at R1911 on the
    /// analysis tool, three of the four were **present and unattributable**
    /// rather than absent: pressing inside the dashboard reaches the dashboard,
    /// its keyboard stops are gated per destination, and 222 of its regions are
    /// announced — but nothing could say that a given mark, press or announced
    /// node *was the dashboard's*. Attribution is what a paint root gives, and
    /// this is attribution.
    ///
    /// Answers over every section, mounted or host-painted, from the same
    /// declarations — a caller that had to know which sections are screens
    /// would be keeping by hand the list this roster exists to publish.
    #[must_use]
    pub fn section_at(&self, tag: &str) -> Option<&str> {
        self.destinations.keys().find(|key| self.paints(key, tag))
    }

    /// ★★★★★ R1911 — **declare the stems the host paints at every
    /// destination**, which is what makes "this mark belongs to nobody"
    /// answerable.
    ///
    /// A section's claim says where that section is; without the host's, a mark
    /// belonging to no section is indistinguishable from a mark belonging to
    /// the frame itself — so a gate could only ask "is the section present" and
    /// never "is anything here unaccounted for". The second question is the one
    /// that keeps [`Screen::paint_stems`]'s default honest: an undeclared mark
    /// family is red rather than invisible.
    ///
    /// # Errors
    ///
    /// [`RosterDefect::PaintAlreadyClaimed`] — chrome overlapping a section's
    /// claim, which would make that section's marks the host's and never go
    /// away.
    pub fn painting_chrome(mut self, stems: &[&str]) -> Result<Self, RosterDefect> {
        for stem in stems {
            for (key, held) in self.claims() {
                if let Some(met) = held.iter().find(|held| stems_meet(held, stem)) {
                    return Err(RosterDefect::ChromeIsAlsoASection {
                        key,
                        stem: met.clone(),
                    });
                }
            }
        }
        self.chrome_stems = stems.iter().map(|s| (*s).to_owned()).collect();
        Ok(self)
    }

    /// ★★★★★ R1911 — the marks on this frame that belong to no section and to
    /// no declared host chrome.
    ///
    /// The population a gate asserts is empty, and the reason
    /// [`Screen::paint_stems`] can carry a default at all: a screen that leaves
    /// its real mark family undeclared does not quietly pass a thinner check —
    /// its marks turn up here, named, at every destination it paints.
    ///
    /// `tags` is whatever the caller read off the frame; this crate does not
    /// paint, so the frame has to come from the host.
    #[must_use]
    pub fn unclaimed_marks<'t>(&self, tags: impl IntoIterator<Item = &'t str>) -> Vec<&'t str> {
        let chrome: Vec<&str> = self.chrome_stems.iter().map(String::as_str).collect();
        let sections: Vec<Vec<&str>> = self
            .destinations
            .keys()
            .filter_map(|key| self.paint_stems_of(key))
            .collect();
        tags.into_iter()
            .filter(|tag| {
                !chrome
                    .iter()
                    .chain(sections.iter().flatten())
                    .any(|stem| *tag == *stem || tag.starts_with(&format!("{stem}.")))
            })
            .collect()
    }

    /// ★★★★★ R1911 — the open destinations whose marks the roster cannot
    /// locate on a frame, in roster order.
    ///
    /// The count a gate should assert on, for [`unsized_keys`](Self::unsized_keys)'s
    /// reason: "four sections were checked for going away" is true of an
    /// application with four sections and of one with forty, and only this says
    /// which sections the question never reached. Empty is the state that lets
    /// a host claim every section a reader can open is one a frame can be asked
    /// about.
    pub fn unrooted_keys(&self) -> impl Iterator<Item = &str> {
        self.destinations
            .keys()
            .filter(|key| {
                self.destinations
                    .get(key)
                    .is_some_and(|d| d.standing.is_open())
            })
            .filter(|key| self.paint_stems_of(key).is_none())
    }

    /// ★★★★★ R1784 — the open destinations that cannot say what they lay out
    /// in, in roster order.
    ///
    /// The count a gate should assert on, rather than on how many answered:
    /// "four screens declared a size" is true of an application with four
    /// sections and of one with forty, and only this says which sections the
    /// question never reached. Empty is the state that lets a host claim the
    /// window it ships in was checked against everything a reader can open.
    pub fn unsized_keys(&self) -> impl Iterator<Item = &str> {
        self.destinations
            .keys()
            .filter(|key| {
                self.destinations
                    .get(key)
                    .is_some_and(|d| d.standing.is_open())
            })
            .filter(|key| self.shrink_policy_of(key).is_none())
    }

    /// ★★★★★ R1830 — **declare what a section is GRANTED**: the width the host
    /// actually hands that destination.
    ///
    /// The other half of [`laying_out`](Self::laying_out), and deliberately not
    /// an extension of it. That one records what a section *wants*; this
    /// records what it *receives*, and the two are different facts about
    /// different actors — a section states its want, a host makes its grant.
    /// Folding them into one registration would give one fact two accounts,
    /// which is the failure mode `laying_out`'s own docs already name.
    ///
    /// # Why the roster has to hold it
    ///
    /// Because otherwise nothing can check the pair. R1784 built the want half
    /// and left the grant in the host, so the gate that compared them read the
    /// shell's own `page_rect(key)` — one host's function. Another host has its
    /// own, and neither the roster nor anything portable could ask whether a
    /// section's want and its grant agreed. Measured then and still true now:
    /// what a section is granted is **per-destination**, because a page the
    /// host paints itself has the host's chrome *inside* its section while a
    /// mounted screen does not, so one figure for the whole application is
    /// wrong for at least one of them.
    ///
    /// `beside` is the width this host paints BESIDE that section — its rail,
    /// and any per-page chrome that sits outside the page region, like a
    /// palette. Declared per key rather than derived, because the roster cannot
    /// know it: a host may paint a palette beside one page and nothing beside
    /// another, and that is a fact about the host's own layout.
    ///
    /// ★ An inset rather than the granted width itself, because the width is
    /// per-frame and this registration is not — see the `grants` field for the
    /// measurement that settled it. What the roster CAN then do — and now does
    /// — is derive the width ([`granted_of`](Self::granted_of)), refuse to let
    /// the fact go unstated ([`ungranted_keys`](Self::ungranted_keys)) and hold
    /// it against the want
    /// ([`sections_short_of_their_grant`](Self::sections_short_of_their_grant)).
    ///
    /// # Errors
    ///
    /// [`RosterDefect`] — a grant at a key the roster does not hold, at a key
    /// it declares closed, or two grants at one key. A grant at a MOUNTED key
    /// is legal and is the point: a host puts chrome beside a guest exactly as
    /// it puts chrome beside a page it paints itself, and a screen stating its
    /// own want does not state what it is given.
    pub fn granting(mut self, key: &str, beside: u32) -> Result<Self, RosterDefect> {
        Self::placeable(&self.destinations, key)?;
        if self.grants.insert(key.to_owned(), beside).is_some() {
            return Err(RosterDefect::DuplicateGrant {
                key: key.to_owned(),
            });
        }
        Ok(self)
    }

    /// The width `key` is granted in a window `window_w` wide, or `None` when
    /// the host never declared what it draws beside that section.
    ///
    /// `saturating_sub`, so a window narrower than the host's own chrome
    /// reports a grant of zero rather than wrapping to something enormous — a
    /// section granted nothing is a true and checkable statement, and the
    /// number a wrap would produce is neither.
    #[must_use]
    pub fn granted_of(&self, key: &str, window_w: u32) -> Option<u32> {
        self.grants
            .get(key)
            .map(|beside| window_w.saturating_sub(*beside))
    }

    /// ★★★★★ R1830 — the open destinations the host never granted a width, in
    /// roster order. The peer of [`unsized_keys`](Self::unsized_keys), and it
    /// exists for that method's reason rather than for symmetry: a count of how
    /// many grants were declared is true of an application that granted every
    /// section and of one that granted two, and only this names the sections
    /// the question never reached.
    pub fn ungranted_keys(&self) -> impl Iterator<Item = &str> {
        self.destinations
            .keys()
            .filter(|key| {
                self.destinations
                    .get(key)
                    .is_some_and(|d| d.standing.is_open())
            })
            // Asks the map directly rather than `granted_of`, because the
            // question here is whether the host SAID anything — which needs no
            // window, and would be a different question if it took one.
            .filter(|key| !self.grants.contains_key(*key))
    }

    /// ★★★★★ R1830 — **every open section whose want exceeds its grant**, as
    /// `(key, wants, granted)`, in roster order.
    ///
    /// This is the check that used to live in one host's test file reading that
    /// host's own function. It is here so that ANY host is held to it, and so
    /// that the two halves it compares are both facts the roster holds.
    ///
    /// A section that declares no size, or that was granted nothing, is not
    /// reported here — it is reported by [`unsized_keys`](Self::unsized_keys)
    /// and [`ungranted_keys`](Self::ungranted_keys), which name what could not
    /// be asked instead of counting what answered. Silently treating an absent
    /// half as satisfied is the shape that makes a gate green over a question
    /// nobody put.
    #[must_use]
    pub fn sections_short_of_their_grant(&self, window_w: u32) -> Vec<(&str, u32, u32)> {
        self.destinations
            .keys()
            .filter(|key| {
                self.destinations
                    .get(key)
                    .is_some_and(|d| d.standing.is_open())
            })
            .filter_map(|key| {
                let wants = self.shrink_policy_of(key)?.comfortable().0;
                let granted = self.granted_of(key, window_w)?;
                (wants > granted).then_some((key, wants, granted))
            })
            .collect()
    }

    /// ★★★★★ R1725 — declare what this host draws for every screen it shows,
    /// so a guest can leave its own out.
    ///
    /// Measured on the first mount, before this existed: at the mounted
    /// destination the shell's navigation ran x=0..52 and the guest painted its
    /// own at x=52..106, and the accessibility tree published **both** as
    /// `role=navigation`. The guest was not wrong to have one — it runs
    /// standalone too — and neither was the host. What was missing is that
    /// "there is already a navigation here" is a fact about **the place**, and
    /// nothing carried it.
    ///
    /// Default [`HostChrome::NONE`], so a host that declares nothing gets what
    /// it got before: every guest draws everything it has.
    #[must_use]
    pub fn providing(mut self, chrome: HostChrome) -> Self {
        self.chrome = chrome;
        self
    }

    /// What this host declared it provides.
    #[must_use]
    pub fn chrome(&self) -> HostChrome {
        self.chrome
    }

    /// The destinations, which are what the rail is painted from and what
    /// [`Journey::navigate`] refuses against.
    #[must_use]
    pub fn destinations(&self) -> &Destinations {
        &self.destinations
    }

    /// Whether this destination's page is a mounted screen rather than one the
    /// host paints itself.
    #[must_use]
    pub fn is_mounted(&self, key: &str) -> bool {
        self.screens.contains_key(key)
    }

    /// ★★★★★ R1738 — how much of its own specification each section of this
    /// application reproduces, one row per destination.
    ///
    /// **The population is this roster's**, walked here rather than taken from
    /// a list a caller passes in, which is the whole reason the report is worth
    /// having: a section cannot be missing from it by being forgotten, only by
    /// not being in the application. See
    /// [`conformance`](crate::conformance) for the measurement that forced it
    /// and for what each arm means.
    ///
    /// The population is **not** keyed by the [`Journey`], unlike every
    /// accessor that hands out a *screen*. The rule this crate is built on —
    /// *the screen the journey is at is the only one anything reaches* — is
    /// about reaching a screen's behaviour: its paint, its keys, its windows. A
    /// verdict about what a section is specified to contain is not behaviour
    /// and is not reached through the page region; it is a fact about the
    /// assembled application, and an application that could only report on the
    /// section somebody happens to be standing in would be the defect this
    /// exists to repair.
    ///
    /// ★★★★★ R1742 — **but each row now says whether that section was SHOWING
    /// when the verdict was taken**, and the journey is passed in for exactly
    /// that. Measured the round a screen first derived its verdict from its own
    /// paint: read from the dashboard, the node lab's row reported surfaces
    /// *standing* — a verdict about a frame that had scrolled out of the
    /// application entirely, because the paint store keeps a surface's LAST
    /// frame and a section that is not showing has not painted since.
    ///
    /// Both halves are kept rather than one: withholding the row would put the
    /// section back outside the population, which is the defect R1738 repaired,
    /// and publishing it unlabelled says a section reproduces something it is
    /// not currently drawing. So the verdict is published and the reader is
    /// told which frame it is about. A section whose verdict does not depend on
    /// a frame — one built from its own tables — reads the same either way, and
    /// `showing` is how a client tells those two kinds apart without knowing
    /// how any section computes itself.
    #[must_use]
    pub fn conformance(&self, journey: &Journey) -> ApplicationConformance {
        ApplicationConformance::new(
            self.destinations
                .all()
                .iter()
                .map(|destination| {
                    let mounted = self.screens.get(&*destination.key);
                    let showing = journey.at() == destination.key.as_ref();
                    let standing = self.standing_of(destination, showing);
                    SectionRow {
                        key: destination.key.to_string(),
                        title: destination.title.to_string(),
                        // The journey's own answer, so "showing" here and the
                        // page the reader is looking at cannot disagree — and
                        // since R1761 it is the SAME value a judge was handed,
                        // so a verdict and the label on it cannot either.
                        showing,
                        // A closed destination cannot have a screen mounted at
                        // it — `new` refuses that pairing — so this is `None`
                        // there for the same reason it is `None` for a page the
                        // host paints itself: there is nothing to address.
                        tag: mounted.map(|screen| screen.tag().to_owned()),
                        standing,
                    }
                })
                .collect(),
        )
    }

    /// What one destination can say about the frame in the paint store, told
    /// whether it is the section a reader is looking at.
    ///
    /// ★ R1767 — extracted so there is **one** definition of *what a section
    /// says*. It had been inline in [`conformance`](Self::conformance) and this
    /// round needed the same answer at two more moments — once per latch, to
    /// record what a walk saw, and once per row of a journey report, to fold in
    /// the frame the reader is on. A second spelling of this match is exactly
    /// the second account this tree keeps refusing.
    fn standing_of(&self, destination: &Destination, showing: bool) -> SectionStanding {
        match (&destination.standing, self.screens.get(&*destination.key)) {
            (Standing::Closed(why), _) => SectionStanding::Closed(why.clone()),
            // ★★★★★ R1761 — a page the host paints is `Inline` only while
            // nothing answers for it. A host that registered a judge has said
            // what this section is compared against, and the row says so.
            (Standing::Open, None) => self
                .judges
                .get(&*destination.key)
                .map_or(SectionStanding::Inline, |judge| {
                    SectionStanding::Judged(judge.conformance(Showing::of(showing)))
                }),
            // ★★★★★ R1888 — and when it publishes nothing, the row carries the
            // SCREEN's reason. `map_or_else` rather than `map_or` deliberately:
            // asking for the reason must not happen on the judged path, because
            // a screen that answers both would then have both read and the row
            // would have to choose. It is asked exactly when it is the answer.
            (Standing::Open, Some(screen)) => screen.conformance().map_or_else(
                || SectionStanding::Unspecified(screen.unjudged_because()),
                SectionStanding::Judged,
            ),
        }
    }

    /// ★★★★★ R1767 — **how much of its specification each section reproduced
    /// somewhere along the walk a reader is taking.**
    ///
    /// The peer of [`conformance`](Self::conformance), which answers the same
    /// question about the frame in front of you.
    /// [`JourneyConformance`] is the type, and its module documentation carries
    /// the measurement that forced it: with one section per frame and — in this
    /// tree's own analysis tool — one section whose specified surfaces exclude
    /// each other, the per-frame verdict is **unreachable by construction** for
    /// any application with two open sections.
    ///
    /// # What it reads and what it does not
    ///
    /// Everything but one row comes from the walk this roster records at
    /// [`latch`](Self::latch), which therefore only ever holds verdicts taken
    /// from frames this application really painted. The exception is the
    /// section the reader is **on**: its newest frame has been painted and not
    /// yet latched, so it is folded in here, live, by the same derivation the
    /// recorder uses. Nothing is stored by reading — call it twice between
    /// frames and it answers the same thing.
    ///
    /// A destination the walk has never stood in is still asked, while away, so
    /// that its **specification** is in the totals. A section absent from the
    /// denominator is R1738's defect with a different hat on; a section
    /// credited for a frame nobody saw is R1763's.
    #[must_use]
    pub fn journey_conformance(&self, journey: &Journey) -> JourneyConformance {
        let walk = self.walk.borrow();
        let rows = self
            .destinations
            .all()
            .iter()
            .map(|destination| {
                let key = destination.key.as_ref();
                let showing = journey.at() == key;
                let seen = if showing {
                    Some(walk.with_live(key, self.standing_of(destination, true)))
                } else {
                    walk.seen(key).cloned()
                };
                let (arrived, standing) = match seen {
                    Some(section) => (Some(section.arrived()), section.standing().clone()),
                    None => (
                        None,
                        JourneyStanding::of(None, self.standing_of(destination, false)),
                    ),
                };
                JourneySection {
                    key: key.to_owned(),
                    title: destination.title.to_string(),
                    tag: self.screens.get(key).map(|screen| screen.tag().to_owned()),
                    showing,
                    arrived,
                    standing,
                }
            })
            .collect();
        JourneyConformance::new(walk.stops(), rows)
    }

    /// Fold the frame now in the paint store into the walk.
    ///
    /// Called from [`latch`](Self::latch), before R1763's forgetting, because
    /// that is the last instant a departing section's frame can be read. See
    /// [`Walk`] for why the observation is about the position the **previous**
    /// latch left behind rather than the journey's current one.
    ///
    /// # What it costs, measured, and the limit that measurement does not cover
    ///
    /// This runs every frame, and it derives **one** section's verdict — the
    /// one whose marks are in the store. It is deliberately not sampled:
    /// sampling would make which frames a walk saw depend on how fast the
    /// machine is, and a report that under-credits differently on every run is
    /// worse than one that costs a little.
    ///
    /// Measured at R1767 on the analysis tool, the same binary with this call
    /// behind a switch, three sections at ninety frames each: `mean_build_us`
    /// 2158/2883/1466 with it against 2004/3384/1444 without and 1991/2972/1521
    /// with it again — **below that instrument's noise**, which is a different
    /// claim from zero and is the one those numbers support.
    ///
    /// ⚠ The limit, stated rather than left to be discovered: that is one
    /// application's [`Screen::conformance`]. A
    /// section whose verdict is expensive to derive pays that cost per frame
    /// here, and nothing in this crate bounds it. An application whose sections
    /// publish no verdict pays a map lookup, which is the property that makes
    /// this affordable by default.
    fn observe(&self, journey: &Journey) {
        let mut walk = self.walk.borrow_mut();
        if let Some(last) = walk.showing_last().map(ToOwned::to_owned)
            && let Some(destination) = self.destinations.get(&last)
        {
            let standing = self.standing_of(destination, true);
            walk.record(&last, standing);
        }
        walk.arrive(journey.at());
    }

    /// The keys with a screen behind them, in roster order.
    pub fn mounted_keys(&self) -> impl Iterator<Item = &str> {
        self.destinations
            .keys()
            .filter(|key| self.screens.contains_key(*key))
    }

    /// The paint-root tag of the screen mounted at `key`, if one is.
    ///
    /// ★ R1729 — the asymmetry this closes: the roster could name the tag of
    /// the screen you are *at* and not of any other, while
    /// [`wire`](Self::wire) had been publishing all of them since R1724. So a
    /// caller asking "is some other screen painted right now" — which is how
    /// you check that leaving a page takes it away — had to navigate there to
    /// find out what to look for, and navigating is the thing under test.
    #[must_use]
    pub fn tag_of(&self, key: &str) -> Option<&'static str> {
        self.screens.get(key).map(|s| s.tag())
    }

    /// ★ R1808 — how many frames the section at `key` needs to show all of what
    /// its specification describes.
    ///
    /// ★★★★★ R1864 — a page the host paints itself answers here too, through
    /// [`posing`](Self::posing). It used to answer `1` unconditionally, under a
    /// doc line that named the gap and left it open: *a host that knows its own
    /// page needs two frames can drive them.* It could not — the pose loop is
    /// inside [`Tour::walk`](crate::Tour::walk), between the latch that reads a
    /// departing frame and the paint that makes the next one, so frames a host
    /// drove itself would be frames no latch ever read.
    ///
    /// `1` for a section with neither a screen nor a poser, which is what a
    /// page that shows everything at once means.
    #[must_use]
    pub fn poses_of(&self, key: &str) -> usize {
        self.screens.get(key).map_or_else(
            || self.posers.get(key).map_or(1, |p| p.poses().max(1)),
            |s| s.poses().max(1),
        )
    }

    /// Put the section at `key` into pose `nth`.
    ///
    /// A section with neither a screen nor a poser has nothing to pose and this
    /// does nothing. The two sources cannot both hold one key: [`posing`]
    /// refuses a destination with a screen, so this lookup has no precedence to
    /// get wrong — the property [`shrink_policy_of`](Self::shrink_policy_of)
    /// records for the same reason.
    ///
    /// [`posing`]: Self::posing
    pub fn pose(&self, key: &str, nth: usize) {
        if let Some(screen) = self.screens.get(key) {
            screen.pose(nth);
        } else if let Some(poser) = self.posers.get(key) {
            poser.pose(nth);
        }
    }

    /// What the screen mounted at `key` declares it needs to lay out in.
    ///
    /// ★ R1781 — the same asymmetry `tag_of` closed, one property over: a
    /// screen's size policy reached `page_scene` (which applies its recourse)
    /// and nothing else, so a host could not ask what its guests need without
    /// navigating to each one. That made an ordinary question — does the window
    /// this application ships in satisfy every screen it mounts — answerable
    /// only by driving the running binary, when it is two declarations sitting
    /// side by side.
    ///
    /// `None` for a screen that declares no policy — a screen that concedes
    /// nothing is not a screen that asked for something and was refused — and
    /// for a destination that has neither a screen nor a declaration.
    ///
    /// ★ R1784 — a page the host paints itself answers here too, through
    /// [`laying_out`](Self::laying_out). The two sources cannot both hold one
    /// key: that registration refuses a destination with a screen, so this
    /// lookup has no precedence to get wrong.
    #[must_use]
    pub fn shrink_policy_of(&self, key: &str) -> Option<ShrinkPolicy> {
        self.screens
            .get(key)
            .and_then(|s| s.shrink_policy())
            .or_else(|| self.sizes.get(key).copied())
    }

    /// ★ R1861 — the part of `region` the screen at `key` has content in that a
    /// floating overlay must not cover.
    ///
    /// `None` for a page the host paints itself: a host has no screen to ask,
    /// and a host that puts an overlay over its own page can see that without
    /// being told. Asked of the roster for [`shrink_policy_of`](Self::shrink_policy_of)'s
    /// reason — a host placing an overlay should not have to navigate to a
    /// destination to learn what is under it.
    ///
    /// ★★★★★ **Asked inside the screen's own extent, and R1825's defect is why.**
    /// A screen answers this from the same geometry it paints with, and that
    /// geometry reads [`layout_size`](pinion_core::external::layout_size) — which
    /// outside a grant falls back to the screen's DESIGN size. Measured on the
    /// first run of this method without the grant: the capture viewer put its
    /// strip 52 pixels below where it paints it and the node lab reported a hint
    /// that cleared the overlay when it did not. So the same wrapper
    /// [`with_current`](Self::with_current) uses for the paint is used here, and
    /// the declaration and the painting read one rectangle.
    #[must_use]
    pub fn keeps_clear_of(
        &self,
        key: &str,
        region: pinion_core::scene::Rect,
    ) -> Option<pinion_core::scene::Rect> {
        let screen = self.screens.get(key)?;
        if region.w == 0 || region.h == 0 {
            return screen.keeps_clear(region);
        }
        with_surface_extent(screen.tag(), (region.w, region.h), || {
            screen.keeps_clear(region)
        })
    }

    /// The current screen's paint-root tag, when the journey is at a mounted
    /// destination.
    #[must_use]
    pub fn current_tag(&self, journey: &Journey) -> Option<&'static str> {
        self.tag_of(journey.at())
    }

    /// The current screen's title — what the host publishes as the window's
    /// title while this screen is showing.
    ///
    /// The reference toolkit keeps a mounted window's title and shows it
    /// nowhere; measured at 6.11.1, the host window went on announcing its own
    /// name while a whole other application filled it.
    #[must_use]
    pub fn current_title(&self, journey: &Journey) -> Option<&'static str> {
        self.screens.get(journey.at()).map(|s| s.title())
    }

    /// ★★★★★ R1724 §2 #2 — the roster an agent reads, saying which of an
    /// application's destinations are **whole screens**.
    ///
    /// [`Destinations::wire`] answers what the rail contains and where the
    /// journey is; it cannot answer this, because a destination's page being
    /// another binding is a fact about the *pairing* rather than about the
    /// destination. Published rather than left to be inferred for the reason
    /// §2 #2 exists: an agent that has to guess whether a section is a screen
    /// guesses from the tag prefixes it happens to see, which is a rule nobody
    /// wrote down.
    ///
    /// Additive over [`Destinations::wire`]'s shape — each row gains `mounted`,
    /// and a mounted row gains the screen's own `tag`, `title` and (R1890)
    /// `address`.
    ///
    /// ★★★★★ R1890 — **`tag` was not an address, and the difference cost a
    /// round.**
    ///
    /// This doc said `tag` is "what lets a client address that screen's
    /// surfaces at all", and it was half true: a client also had to know that a
    /// surface is reached at `/<tag>/external/<path>`, a rule that lived as a
    /// `const` inside the transport's parser and appeared in no published
    /// value. R1889 asked the assembled analysis tool for a mounted screen's
    /// paths at `/external/<path>` — the root short-circuit, which in an
    /// assembled application is the **host's** surface — got
    /// `UnknownIntrospectPath` seven times, and concluded that a screen's wire
    /// surface does not survive mounting. Re-measured at R1890 the same build
    /// answers all seven, at the address nobody was publishing.
    ///
    /// So the row now carries the address itself, composed through
    /// [`pinion_core::wire_address::surface_at`] — the same expression the
    /// transport's splitter reads its separator from. A client appends an
    /// introspect path and never learns the grammar.
    ///
    /// An unmounted destination publishes no address, because it has no surface
    /// to answer one: `screen` is `null` and so is anything derived from it.
    /// That asymmetry is the useful half — "this page is the host's own" and
    /// "this page is a screen you can interrogate" become different values
    /// rather than the same silence.
    #[must_use]
    pub fn wire(&self, journey: &Journey) -> serde_json::Value {
        let mut value = self.destinations.wire(journey);
        if let Some(rows) = value
            .get_mut("destinations")
            .and_then(serde_json::Value::as_array_mut)
        {
            for row in rows {
                let key = row
                    .get("key")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let screen = self.screens.get(&key);
                row["mounted"] = serde_json::Value::Bool(screen.is_some());
                row["screen"] = screen.map_or(serde_json::Value::Null, |s| {
                    serde_json::json!({
                        "tag": s.tag(),
                        "title": s.title(),
                        "address": pinion_core::wire_address::surface_at(s.tag()),
                    })
                });
                // ★★★★★ R1911 — **where this section's marks are**, for every
                // row rather than only the mounted ones. A client asking "is
                // the dashboard still painted" had to guess the stems from the
                // tags it happened to see, which is the rule §2 #2 exists to
                // stop being unwritten — and the guess held only while every
                // section that had an answer was a screen.
                row["paints"] =
                    self.paint_stems_of(&key)
                        .map_or(serde_json::Value::Null, |stems| {
                            serde_json::Value::Array(
                                stems
                                    .into_iter()
                                    .map(|s| serde_json::Value::String(s.to_owned()))
                                    .collect(),
                            )
                        });
            }
        }
        // ★★★★★ R1911 — and what the HOST paints at every destination, beside
        // the rows rather than in one, because painting at every destination is
        // what makes a mark chrome. A client checking "is anything here
        // unaccounted for" needs both halves; with only the rows it would keep
        // the host's list by hand, which is the guess §2 #2 exists to end.
        value["chrome_paints"] = serde_json::Value::Array(
            self.chrome_stems
                .iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect(),
        );
        value
    }

    /// Run `body` against the screen the journey is at, inside that screen's
    /// extent grant.
    ///
    /// **The only way to reach a screen.** Two properties come from that and
    /// neither is a convention:
    ///
    /// * a screen the journey is not at cannot be reached at all, so it cannot
    ///   take a press, answer a key, or appear in a tree;
    /// * every hook runs inside
    ///   [`with_surface_extent`], so a screen that hit-tests its own rectangles
    ///   reads the region it was placed in rather than the window it is inside
    ///   — including from the hooks the shell wraps in an owner scope, where
    ///   [`layout_size`](pinion_core::external::layout_size) would otherwise
    ///   answer the whole window.
    ///
    /// Answers `None` at an unmounted destination, which is a host's own page.
    pub fn with_current<R>(
        &self,
        journey: &Journey,
        body: impl FnOnce(&dyn Screen) -> R,
    ) -> Option<R> {
        let Some(screen) = self.screens.get(journey.at()) else {
            // ★★★★★ R1825 — a host that is not placing a screen must not leave
            // one reading a place. The declaration now OUTLIVES the build (see
            // `with_host_chrome_for`), so dropping it is the host's job, and
            // the honest moment is the one where it finds nothing to place.
            for tag in self.screens.values().map(|s| s.tag()) {
                pinion_core::chrome::forget_host_chrome(tag);
            }
            return None;
        };
        let extent = self.placed_extent.get();
        // ★ R1725 — the chrome declaration wraps EVERY hook and not only the
        // view, for the reason the extent grant does: a guest that omits its
        // rail while painting must omit it from its accessibility tree and its
        // keyboard too, and those are different calls.
        // ★★★★★ R1825 — the `_for` spelling, which RECORDS the declaration
        // against this screen's surface as well as scoping it. The scope covers
        // the hooks this roster calls; the record covers the ones the FRAMEWORK
        // calls on the screen's `External` afterwards — `target_at`, the press
        // path, `wheel`, the drag hooks — which run inside no scope at all and,
        // before this, read `NONE` and behaved as if the screen were standalone.
        // That is not a hypothetical: measured on the analysis tool, 41 of the
        // node lab's painted regions addressed a different region at their own
        // centre while mounted, and none did standalone at the same size.
        Some(with_host_chrome_for(screen.tag(), self.chrome, || {
            if extent.0 == 0 || extent.1 == 0 {
                // Nothing has placed the region yet, so there is no rectangle
                // to grant and the pre-R1724 reading is the honest one.
                return body(screen.as_ref());
            }
            with_surface_extent(screen.tag(), extent, || body(screen.as_ref()))
        }))
    }

    /// The current screen's scene, laid out in `extent` and given the recourse
    /// it declared for not fitting.
    ///
    /// Records `extent` as the region the screen is placed in, which is what
    /// every later [`Self::with_current`] grants — so the paint and the gesture
    /// halves of a mounted screen read one rectangle by construction.
    ///
    /// # ★★★★★ The region owes the screen its recourse
    ///
    /// A screen lays out at `max(extent, its own comfortable size)` — that is
    /// [`layout_size`](pinion_core::external::layout_size)'s rule and it does
    /// not change because the screen is a page. So a region smaller than the
    /// screen's layout minimum has content it cannot show, and what happens to
    /// that content is the screen's own declaration:
    /// [`pan`] is applied here for exactly the reason
    /// the shell applies it to a window.
    ///
    /// Measured on the first real mount, before this existed: the node lab,
    /// whose layout stops reflowing at 1625 wide, placed in a 1388-wide region,
    /// painted **51 of its regions outside that rectangle** — its inspector ran
    /// from x=1365 to x=1677 in a window that ends at 1440, so the pane a
    /// person configures a node with was off the screen with no way to reach
    /// it. The screen had declared `Recourse::Pan` since R1714 and the region
    /// was not listening.
    ///
    /// `pan` is the identity for a screen that fits, one that declares no
    /// policy, and one that clips — so this costs nothing where there is
    /// nothing to pan over.
    ///
    /// `frame` is passed through to the screen unchanged.
    #[must_use]
    pub fn page_scene(
        &self,
        journey: &Journey,
        extent: (u32, u32),
        frame: &Frame,
    ) -> Option<Scene> {
        if extent.0 > 0 && extent.1 > 0 {
            self.placed_extent.set(extent);
        }
        self.with_current(journey, |screen| {
            pan(
                screen.shrink_policy(),
                screen.tag(),
                extent,
                screen.view(frame),
            )
        })
    }

    /// The current screen's scene for a window of its own.
    #[must_use]
    pub fn window_scene(&self, journey: &Journey, window_id: &str, frame: &Frame) -> Option<Scene> {
        self.with_current(journey, |screen| screen.view_for_window(window_id, frame))
    }

    /// Read the current screen's projection out of the state scene, and report
    /// where the application is as one `Copy` value.
    ///
    /// This is a host's
    /// [`WidgetCore::read_state`](pinion_core::WidgetCore::read_state), and the
    /// only place [`Screen::latch`] is called from.
    ///
    /// ★★★★★ R1763 — **and leaving a screen takes its painted marks with it.**
    ///
    /// # What forced it, measured
    ///
    /// Leaving a screen already takes its externals, its windows and its
    /// accessibility tree with it — that is this crate's central rule, *the
    /// screen the journey is at is the only one anything reaches*. Its marks
    /// were the one thing it left behind, and a verdict read from those is a
    /// statement about a frame that has left the application.
    ///
    /// Measured on the assembled analysis tool at R1763, walking every section
    /// and returning to the first:
    ///
    /// ```text
    /// packets  showing=false  25 of 26  away=0  reconciles=true
    /// keys     showing=false  21 of 21  away=0  reconciles=true
    /// logs     showing=false  15 of 15  away=0  reconciles=true
    /// ```
    ///
    /// Three sections reporting a reproduced specification about frames nobody
    /// could see. R1742 published `showing` beside each row so a reader could
    /// TELL, which was the honest half; this is the other half.
    ///
    /// ⚠ It matters more than a stale number, because
    /// [`ApplicationConformance::conforms`](crate::ApplicationConformance::conforms)
    /// is `unjudged == 0 && declared == 0 && every judged report reconciles` —
    /// and R1762 brought the first two to zero. Without this, an application
    /// could report conformance earned entirely by frames that had left it.
    ///
    /// # Why here, and what it does not reach
    ///
    /// Here because this is the per-frame moment that already names the current
    /// screen, so the fact and its consequence are one expression. The window
    /// path forgets a surface that is in the state scene and painted nothing
    /// (`announce_external_sizes`, R1737) — since R1826, one that THIS window
    /// had itself announced, because that function runs once per window against
    /// the shared state scene and every window was answering for every surface;
    /// a screen the journey has left has no externals in the state scene at all,
    /// so that loop never reaches it either way — which is why the roster is the
    /// only thing that can.
    ///
    /// A screen's own nested surfaces (a text field that owns focus is an
    /// `External` of its own) keep their marks. Nothing reads them for a
    /// verdict — a section's judge reads its ROOT's store, which is what R1758
    /// made sufficient — and stating the limit is cheaper than a reader
    /// assuming it was covered.
    #[must_use]
    pub fn latch(&self, journey: &Journey, state_scene: &Scene) -> ScreenState {
        // ★★★★★ R1767 — and the walk takes the departing frame's verdict WITH
        // it, on the way past. This line is before the forgetting below for the
        // same reason the forgetting is here at all: this is the last instant
        // at which the frame a reader actually saw can still be read, and a
        // journey verdict that could not read it would be a verdict about
        // whichever section happened to be last.
        self.observe(journey);
        for (key, screen) in &self.screens {
            if key.as_str() != journey.at() {
                pinion_core::painted::forget_painted_regions(screen.tag());
            }
        }
        let at = self
            .destinations
            .keys()
            .position(|key| key == journey.at())
            .unwrap_or(0);
        let revision = self
            .with_current(journey, |screen| screen.latch(state_scene))
            .unwrap_or(0);
        ScreenState {
            at: u32::try_from(at).unwrap_or(u32::MAX),
            revision,
        }
    }

    /// The externals that are live while the journey is where it is: the
    /// current screen's, and nobody else's.
    ///
    /// This is the whole of "a screen you are not at is not routable". There is
    /// no filtering step and no visibility flag — an external that is not in
    /// the returned list is not in the state scene, so the §5.35 router has
    /// nothing to resolve a press to and the wire has no slot to address.
    #[must_use]
    pub fn externals(&self, journey: &Journey) -> Vec<ExtraExternal> {
        self.with_current(journey, |screen| screen.externals())
            .unwrap_or_default()
    }
}
