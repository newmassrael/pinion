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

use pinion_core::chrome::{HostChrome, with_host_chrome};
use pinion_core::external::with_surface_extent;
use pinion_core::shrink::{ShrinkPolicy, pan};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::destination::{Destination, Destinations, Journey, Standing};
use pinion_core::{Frame, Scene};

use crate::Screen;
use crate::conformance::{
    ApplicationConformance, SectionJudge, SectionRow, SectionStanding, Showing,
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
            RosterDefect::SectionAlreadyAnswers { key } => write!(
                f,
                "destination `{key}` has a screen, which answers for it; a \
                 second verdict from the host would make the section's own \
                 answer depend on which registration a lookup reached first"
            ),
        }
    }
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
            (Standing::Open, Some(screen)) => screen
                .conformance()
                .map_or(SectionStanding::Unspecified, SectionStanding::Judged),
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
    /// `None` for a destination with no screen, and for a screen that declares
    /// no policy: a screen that concedes nothing is not a screen that asked for
    /// something and was refused.
    #[must_use]
    pub fn shrink_policy_of(&self, key: &str) -> Option<ShrinkPolicy> {
        self.screens.get(key).and_then(|s| s.shrink_policy())
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
    /// and a mounted row gains the screen's own `tag` and `title`, which is
    /// what lets a client address that screen's surfaces at all.
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
                row["screen"] = screen.map_or(
                    serde_json::Value::Null,
                    |s| serde_json::json!({ "tag": s.tag(), "title": s.title() }),
                );
            }
        }
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
        let screen = self.screens.get(journey.at())?;
        let extent = self.placed_extent.get();
        // ★ R1725 — the chrome declaration wraps EVERY hook and not only the
        // view, for the reason the extent grant does: a guest that omits its
        // rail while painting must omit it from its accessibility tree and its
        // keyboard too, and those are different calls.
        Some(with_host_chrome(self.chrome, || {
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
    /// (`announce_external_sizes`, R1737); a screen the journey has left has no
    /// externals in the state scene at all, so that loop never reaches it —
    /// which is why the roster is the only thing that can.
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
