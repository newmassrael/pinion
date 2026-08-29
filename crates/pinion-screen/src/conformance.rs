//! ★★★★★ R1738 — **an application counts the sections it was judged on.**
//!
//! # What forced this module
//!
//! R1728 wrote this tool's navigation down as a reviewed artifact and made
//! something fail when the application stopped matching it. R1730 and R1731 did
//! the same one level down, for a *section's* surfaces, and
//! [`pinion_core::conformance`] is the machinery both use.
//!
//! What nothing did was add them up. Measured on the running application at
//! R1738, over the wire, standing in each section in turn:
//!
//! ```text
//! /external/conformance            -> { specified: 8, reproduced: 8, divergences: [], owed: [] }
//! /key_patterns/external/conformance -> { columns: 7/7, detail: 11/11, header: 3/3 }
//! /log_view/external/conformance     -> { columns: 5/5, detail: 6/6, header: 4/4 }
//! ...and four of the six open sections answered nothing at all.
//! ```
//!
//! The headline says `8 of 8 reproduced` and it is **true about the rail** —
//! eight navigation seats, eight reproduced. A reader has every reason to take
//! it for a statement about the tool, and as a statement about the tool it is
//! wrong: two of six sections had been compared with anything, and the other
//! four were not *failing*, they were **absent from the population**. That is
//! R1737's lesson one level up — *a gate existing and a gate's coverage being
//! deliberate are different claims* — and the repair is the same one: make the
//! framework count, and make the population derived.
//!
//! # The shape
//!
//! [`ApplicationConformance`] holds **one row per destination**, taken from the
//! roster rather than from a list an author maintains, so a section cannot be
//! left out of the report by being forgotten — only by not being in the
//! application. Each row is a [`SectionStanding`], and the four arms are four
//! genuinely different facts a reader needs to tell apart:
//!
//! | arm | what it says |
//! |---|---|
//! | [`Judged`](SectionStanding::Judged) | a screen is here and it answers a written specification |
//! | [`Unspecified`](SectionStanding::Unspecified) | a screen is here and it answers to no specification |
//! | [`Inline`](SectionStanding::Inline) | the host paints this page itself and nothing answers for it |
//! | [`Closed`](SectionStanding::Closed) | you cannot arrive, and this is the reason |
//!
//! There is no catch-all. `Unspecified` and `Inline` are kept apart because the
//! work that closes them is different — one is *write the specification down*,
//! the other is *say what compares this page with it*
//! ([`SectionJudge`], registered through
//! [`ScreenRoster::judging`](crate::ScreenRoster::judging)).
//!
//! ★★★★★ R1761 corrected the second half of that sentence, which had read
//! *give the host's own page a [`Screen`](crate::Screen), which the trait is
//! public for*. It was written from the type rather than from a measurement: a
//! host paints a section's chrome beside the page region, so a screen at that
//! destination judges the part of the section that happens to be inside one
//! rectangle. Measured on the analysis tool's dashboard — page region 1096×802,
//! layout bar 1096×46 above it, palette 292×848 beside it — the recorded route
//! would have shipped a verdict blind to a quarter of its own section.
//!
//! # The one rule that makes it worth having
//!
//! [`ApplicationConformance::conforms`] is false while **any open section is
//! unjudged**. An application must not be able to report conformance on the
//! strength of the sections somebody happened to write a specification for;
//! that is precisely the reading the measurement above found, and a report that
//! permitted it would be the defect with a type around it.
//!
//! ★★★★★ R1767 — **and this module answers about ONE FRAME, which for a
//! multi-section application means that predicate can never be true.** Not a
//! defect: one frame paints one section, the others are away, and an away
//! surface reconciles nothing. The question an assembled application can
//! actually answer is a walk's, and it lives in [`crate::journey`] — every open
//! section stood in, every verdict read from a painted frame, and each one
//! naming the step it came from. The two are peers and neither replaces the
//! other; this one is what a reader asks about the page in front of them.
//!
//! # Floor, measured by building a probe against the reference toolkit 6.11.1
//!
//! The probe assembles a paged application out of three pages, gives one page a
//! part fewer than it declares and another the specified parts in the wrong
//! order, and asks the toolkit about it.
//!
//! * Across the page-stack container, the tabbed container and a plain page,
//!   **312** members were scanned and **0** name a specification, an
//!   expectation or a divergence. There is nothing to write the statement in.
//! * The only channel the toolkit has for a page to declare what it is supposed
//!   to contain is a compile-time per-class annotation, so three pages that
//!   built **different** things all report the **same** specification — the
//!   statement is not reachable per instance.
//! * And the row that decides it: with a short page and a reordered page in it,
//!   the container still answers `count() = 3`. It has no member returning a
//!   verdict, a divergence, or a count of pages judged. Nothing failed and
//!   nothing was reported.

use pinion_core::availability::Unavailable;
use pinion_core::conformance::DocumentReport;

/// What one destination of an application can say about the specification its
/// section reproduces.
///
/// See the [module documentation](self) for why there are four arms and why
/// none of them is a catch-all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SectionStanding {
    /// A screen is mounted here and it answers a written specification.
    Judged(DocumentReport),
    /// A screen is mounted here and it publishes no verdict about a written
    /// specification, so nothing compares what it draws with anything.
    ///
    /// A *declared* absence and not silence: this is a row in the report, which
    /// is the whole difference between a section nobody judged and a section
    /// nobody noticed.
    ///
    /// ⚠ It says the screen answers nothing — **not** that nothing was written
    /// for it. Those come apart, and this tree has one of each: one section has
    /// no specification at all, and another has a specification it compares
    /// itself against inside a unit test of its own binary, where the assembled
    /// application cannot see it. A host cannot tell them apart because only
    /// the screen knows, and a screen has no way to say so yet — recorded as
    /// R1738's remainder rather than guessed at here.
    ///
    /// ★ R1742 closed the second of those two by making the screen publish,
    /// which left one live case rather than two.
    ///
    /// ★★★★★ R1888 — **and this arm carries the SCREEN's own reason now.** It
    /// was a unit variant for 150 rounds, so [`Self::why`] answered with a
    /// sentence written here — the host's inference, phrased as though the
    /// screen had said it. The two facts that sentence conflated are the ones
    /// named above: *nobody wrote a specification* and *one exists where the
    /// assembled application cannot reach it* are different, only the screen
    /// knows which, and a host that guesses points a reader at the wrong
    /// repair.
    ///
    /// Named after [`Built::Away`](pinion_core::conformance::Built::Away),
    /// which is the same statement one level down: R1742 gave a SURFACE the
    /// ability to say why it is not judged and left a SECTION without it, and
    /// this is that other half.
    ///
    /// ⚠ The string is not free of obligations. A binding that answers nothing
    /// gets [`pinion_shell::UNSTATED`], which is an admission rather than an
    /// explanation precisely so a gate can tell the two apart — see that
    /// constant for why a plausible default would have been worse than none.
    Unspecified(String),
    /// The destination is open, its page is one the host paints itself, and
    /// **nothing answers for it** — neither a screen nor a judge.
    ///
    /// ★★★★★ R1761 — this arm's own closing instructions were wrong for
    /// twenty-three rounds, and the correction is the round: it read *closing
    /// it means giving that page a [`Screen`](crate::Screen) of its own, which
    /// is what the trait being public is for*. Measured on the section that
    /// sentence had been sitting on since R1738, a host's chrome for a section
    /// is painted **beside** the page region rather than in it — a layout bar
    /// above it and a palette panel to its right, together about a quarter of
    /// the section's own area. A screen judges what it paints, so the route
    /// recorded here would have produced a verdict that could not reach the
    /// two surfaces the section is most distinctive for.
    ///
    /// Closing it means registering what answers for the page —
    /// [`ScreenRoster::judging`](crate::ScreenRoster::judging) — which is a
    /// smaller claim than being a screen and the true one for a page a host
    /// draws. Becoming a [`Screen`](crate::Screen) is still the move for a page
    /// that wants a screen's *behaviour*; it is no longer the price of saying a
    /// true sentence about a page.
    Inline,
    /// The destination is closed, so there is no section to judge — and this is
    /// the destination's own reason, not a second wording of it.
    Closed(Unavailable),
}

impl SectionStanding {
    /// Whether this section was compared with a written specification.
    #[must_use]
    pub const fn is_judged(&self) -> bool {
        matches!(self, SectionStanding::Judged(_))
    }

    /// Whether a reader can arrive here at all.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        !matches!(self, SectionStanding::Closed(_))
    }

    /// Whether this is an open section that nothing has judged.
    ///
    /// The count [`ApplicationConformance::conforms`] refuses to ignore.
    #[must_use]
    pub const fn is_unjudged(&self) -> bool {
        matches!(
            self,
            SectionStanding::Unspecified(_) | SectionStanding::Inline
        )
    }

    /// ★★★★★ R1888 — whether the thing that OWNS this section has said
    /// something about its own verdict.
    ///
    /// # The distinction, and why it is not [`is_judged`](Self::is_judged)
    ///
    /// Every row carries either a *reason* or an *admission*, and until this
    /// round nothing told them apart. A reason comes from the subject and says
    /// something only the subject knows: a verdict from the screen that painted
    /// the section, or a closure from the destination that is shut. An
    /// admission comes from the host and says that nobody answered —
    /// [`Inline`](Self::Inline)'s constant sentence, and an
    /// [`Unspecified`](Self::Unspecified) whose screen handed back
    /// [`pinion_shell::UNSTATED`].
    ///
    /// Both admissions read like sentences, which is the whole difficulty: a
    /// report full of them looks exactly like a report full of accounts, and
    /// [`why`](Self::why) hands a reader a string either way.
    ///
    /// ⚠ **This is deliberately not folded into
    /// [`ApplicationConformance::conforms`]**, which already refuses while any
    /// section is unjudged and would therefore be unmoved by it — an
    /// unaccounted section is a strict subset of an unjudged one. What it buys
    /// is the finer word: *nothing judged this* and *nothing will even say why*
    /// are different failures with different repairs, and a count that cannot
    /// separate them sends the reader to the wrong one.
    #[must_use]
    pub fn accounts(&self) -> bool {
        match self {
            SectionStanding::Judged(_) | SectionStanding::Closed(_) => true,
            SectionStanding::Unspecified(why) => why != pinion_shell::UNSTATED,
            SectionStanding::Inline => false,
        }
    }

    /// The word this arm is published under.
    #[must_use]
    pub const fn word(&self) -> &'static str {
        match self {
            SectionStanding::Judged(_) => "judged",
            SectionStanding::Unspecified(_) => "unspecified",
            SectionStanding::Inline => "inline",
            SectionStanding::Closed(_) => "closed",
        }
    }

    /// Why this section is not judged, in one sentence, or `None` when it is.
    #[must_use]
    pub fn why(&self) -> Option<String> {
        match self {
            SectionStanding::Judged(_) => None,
            // ★★★★★ R1888 — the SCREEN's sentence, not one written here. What
            // stood in this arm said "a screen is here and it publishes no
            // verdict about a specification", which is true of every such
            // section and therefore tells a reader nothing they did not have
            // from the word `unspecified` itself.
            SectionStanding::Unspecified(why) => Some(why.clone()),
            SectionStanding::Inline => {
                // ★ R1761 — *and nothing answers for it*. The sentence used to
                // stop at "no screen to ask", which read as a fact about what
                // the roster holds when it is a fact about what the host chose
                // not to say: a page the host paints can be judged now, and a
                // reason that names only the missing screen points at the wrong
                // repair.
                Some(
                    "the host paints this page itself and nothing answers for it, \
                     so nothing compares it with anything"
                        .to_owned(),
                )
            }
            SectionStanding::Closed(why) => Some(why.sentence()),
        }
    }
}

/// ★★★★★ R1761 — **what answers for a section whose page the host paints.**
///
/// # Why this is not [`Screen`](crate::Screen)
///
/// Because a screen and a verdict are two axes, and R1738 bound them together
/// by accident: the only way for a section to be judged was to be a mounted
/// binding, so two pages of this tree's analysis tool went twenty-three rounds
/// unjudged while a written specification and a comparison against it existed
/// for both, in the host's own test module.
///
/// The measurement that settles it is geometric. A host that paints a page
/// itself paints that section's chrome **beside** the page region — measured on
/// this tree's dashboard at R1761, a 1096×46 layout bar above the region and a
/// 292×848 palette to its right, against a region of 1096×802. A screen judges
/// what it paints, so making that page a screen would have produced a verdict
/// covering three quarters of its own section, with nothing anywhere saying so.
///
/// # The obligation registering one takes on
///
/// The report is not `Option`, unlike [`Screen::conformance`](crate::Screen::conformance).
/// A binding is mounted for a hundred reasons and judging is not one of them,
/// so a screen may honestly answer nothing; a judge exists for no other purpose,
/// so registering one **is** the claim that this section is compared with
/// something. A host with nothing to compare registers nothing, and the row
/// reads [`Inline`](SectionStanding::Inline) — which is the true sentence and
/// the one this trait must not make it easy to hide.
///
/// # What it is not
///
/// Not a hook a host can answer from its own tables and have believed: the
/// verdict a judge returns carries its own
/// [`Evidence`](pinion_core::conformance::Evidence), and
/// [`ApplicationConformance::conforms`] counts a verdict read from anything but
/// a painted frame as a reason to refuse. See
/// [`report_from_paint`](pinion_core::conformance::SpecDocument::report_from_paint),
/// which is what an honest judge is built on.
pub trait SectionJudge {
    /// How much of its written specification this section reproduces.
    ///
    /// `showing` is the host's own answer to *is this the page a reader is
    /// looking at*, handed in rather than looked up. See [`Showing`] for why a
    /// judge cannot work it out for itself without opening an escape hatch.
    fn conformance(&self, showing: Showing) -> DocumentReport;
}

/// ★★★★★ R1864 — **how many frames a page the HOST paints itself needs to show
/// all of what its specification describes, and how to put it in each.**
///
/// [`Screen::poses`](crate::Screen::poses) is this for a mounted screen, and
/// [`ScreenRoster::poses_of`](crate::ScreenRoster::poses_of) answered `1` for
/// every host page — with a doc line that named the gap outright: *a host that
/// knows its own page needs two frames can drive them*. It could not. The pose
/// loop lives inside [`Tour::walk`](crate::Tour::walk), between the latch that
/// takes a departing frame's verdict and the paint that makes the next one, so
/// a host driving its own poses from the paint closure would produce frames
/// **no latch ever read**.
///
/// # What forced it, measured
///
/// The analysis tool's preferences page is one the host paints itself, it
/// scrolls, and its content is taller than the region it is given — measured at
/// R1864, 946 pixels of page in an 820-pixel viewport. Its last group is below
/// the fold, so a walk that paints one frame per section reports that group
/// unreproduced, and reports it for a page a reader can read in full by
/// scrolling. The verdict was true of the frame and false of the section.
///
/// ⚠ It had been passing on a technicality: the same group straddled the fold
/// before the host reserved a status band, and a node that is partly outside a
/// viewport is still painted. Nothing had changed about what a reader could
/// see; 28 pixels moved a node from *partly visible* to *outside*, and a
/// question that should never have been about one frame started answering
/// differently.
///
/// # A fourth map, for the reason the third one is separate
///
/// A pose count is not a screen with most of it missing, and a screen that
/// exists only to carry one would judge a section it does not paint — the route
/// R1761 measured and refused. See
/// [`ScreenRoster::posing`](crate::ScreenRoster::posing).
pub trait SectionPoser {
    /// How many frames this page needs. `1` means it shows everything at once,
    /// which is what a page with no poser is taken to mean.
    fn poses(&self) -> usize;

    /// Put the page into pose `nth`, counted from zero.
    ///
    /// Called before the frame is painted, once per pose, in order. Pose `0` is
    /// the state a reader arrives in: a page whose first pose were anything
    /// else would be reporting a frame nobody opens.
    fn pose(&self, nth: usize);
}

/// ★★★★★ R1761 — whether the section a [`SectionJudge`] answers for is the one
/// on screen.
///
/// # Why a judge is told rather than left to work it out
///
/// A mounted screen needs no such fact: it paints into a surface of its own, so
/// a screen that is not showing has no recorded frame and
/// [`report_from_paint`](pinion_core::conformance::SpecDocument::report_from_paint)
/// answers away for all of it, structurally. A host's inline page has no
/// surface of its own — its marks are the host's, and the host's store is full
/// of *whatever page it is painting instead*. Measured at R1761: read from the
/// preferences page, the dashboard's four layout-bar parts are simply not among
/// the marks.
///
/// A judge could notice that by finding nothing under its own stems, and that
/// is precisely the escape hatch R1742 refused one level down: *away because I
/// found nothing* turns every absence into an excuse, so a page that stopped
/// painting half of itself would report the same as a page nobody is looking
/// at. The distinction belongs to whoever knows where the reader is, and that
/// is the host. It is the same value the row beside the verdict publishes
/// ([`SectionRow::showing`]), taken from the [`Journey`](pinion_core::widgets::destination::Journey)
/// once — so the verdict and the label on it cannot disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Showing {
    /// This section is the page the application is on.
    OnScreen,
    /// The reader is somewhere else, so this page painted no part of the last
    /// frame.
    Elsewhere,
}

impl Showing {
    /// Whether this section is the one on screen.
    #[must_use]
    pub const fn is_on_screen(self) -> bool {
        matches!(self, Showing::OnScreen)
    }

    /// The host's fact, as a judge receives it.
    #[must_use]
    pub const fn of(showing: bool) -> Self {
        if showing {
            Showing::OnScreen
        } else {
            Showing::Elsewhere
        }
    }
}

/// One destination of an application, and what it can say about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionRow {
    /// The destination's key, which is what the rail, the page and the wire all
    /// address it by.
    pub key: String,
    /// What a reader calls it.
    pub title: String,
    /// The mounted screen's paint-root tag — which is also the tag its own
    /// externals are addressed by — or `None` when no screen is mounted here.
    ///
    /// ★ Present so that a reader of this report can go and *ask the section
    /// itself*. Without it, reaching a mounted section's own published verdict
    /// means already knowing its tag, and a client that has to know a mapping
    /// nobody published is a client working from a list it maintains by hand —
    /// which is the failure this whole report exists to end.
    pub tag: Option<String>,
    /// ★★★★★ R1742 — whether this is the section the reader is looking at,
    /// which is **which frame the verdict beside it is about**.
    ///
    /// A section that derives its verdict from its own paint answers about its
    /// LAST frame, and a section that is not showing has not painted since it
    /// was left. Measured the round the first such screen was written: read
    /// from another page, the node lab's row reported surfaces standing — a
    /// true statement about a frame that is no longer in the application.
    ///
    /// The row is published either way, because withholding it would put the
    /// section back outside the population — the defect R1738 repaired — and a
    /// reader who needs a live verdict navigates there and reads again.
    pub showing: bool,
    /// What it can say.
    pub standing: SectionStanding,
}

/// ★★★★★ Every destination of an application, and how much of its own
/// specification each section reproduces.
///
/// See the [module documentation](self) for the measurement that forced this,
/// the four arms, and the reference floor.
///
/// # Examples
///
/// ```
/// use pinion_core::widgets::destination::{Destination, Destinations, Journey};
/// use pinion_core::availability::Unavailable;
/// use pinion_screen::{ScreenRoster, SectionStanding};
///
/// let destinations = Destinations::new(vec![
///     Destination::open("home", "Home"),
///     Destination::closed("later", "Later", Unavailable::reserved("requirement 12")),
/// ])
/// .expect("the fixture is a roster");
/// let journey = Journey::begin(&destinations, "home").expect("`home` is open");
/// let roster = ScreenRoster::new(destinations, Vec::new())
///     .expect("nothing is mounted, so nothing can be mounted wrongly");
///
/// let report = roster.conformance(&journey);
/// assert_eq!(report.sections(), 2);
/// assert_eq!(report.judged(), 0);
/// assert_eq!(report.unjudged(), 1); // `home` — the host paints it itself
/// assert!(matches!(report.rows()[0].standing, SectionStanding::Inline));
///
/// // ★ Every destination is a row, and each says which frame its verdict is
/// // about: the one being read is showing, the other is not.
/// assert!(report.rows()[0].showing);
/// assert!(!report.rows()[1].showing);
///
/// // And the rule: no section was judged, so the application does not conform.
/// assert!(!report.conforms());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationConformance {
    rows: Vec<SectionRow>,
}

impl ApplicationConformance {
    /// Build a report over every destination.
    ///
    /// Called by [`ScreenRoster::conformance`](crate::ScreenRoster::conformance),
    /// which is the only place that has both halves.
    pub(crate) fn new(rows: Vec<SectionRow>) -> Self {
        Self { rows }
    }

    /// Every destination, in the roster's order.
    #[must_use]
    pub fn rows(&self) -> &[SectionRow] {
        &self.rows
    }

    /// How many destinations this application has.
    ///
    /// The roster's count and not a number an author keeps, which is what makes
    /// a caller unable to check fewer sections than the application has.
    #[must_use]
    pub fn sections(&self) -> usize {
        self.rows.len()
    }

    /// How many sections were compared with a written specification.
    #[must_use]
    pub fn judged(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.standing.is_judged())
            .count()
    }

    /// ★★★★★ R1758 — how many judged sections answered from their **own
    /// tables** rather than from a painted frame.
    ///
    /// A separate count from [`unjudged`](Self::unjudged) because it is a
    /// different fact: those sections *were* compared with a specification, and
    /// the comparison could not fail for the reason comparisons exist. See
    /// [`Evidence`](pinion_core::conformance::Evidence) for what was measured,
    /// and [`conforms`](Self::conforms) for what this report does about it.
    #[must_use]
    pub fn declared(&self) -> usize {
        self.judged_reports()
            .filter(|report| !report.evidence().is_paint())
            .count()
    }

    /// How many sections a reader can arrive at that nothing has judged.
    #[must_use]
    pub fn unjudged(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.standing.is_unjudged())
            .count()
    }

    /// ★★★★★ R1888 — how many sections carry an **admission** where a reason
    /// should be: nothing answered for them, and nothing will say why.
    ///
    /// The population that makes [`unjudged`](Self::unjudged) two facts. A
    /// section whose screen published no verdict *and named its reason* is a
    /// known gap with an address; a section that answered
    /// [`pinion_shell::UNSTATED`], or one the host paints with nothing
    /// registered for it, is a gap nobody has looked at. Both count as
    /// unjudged, and the repairs are not the same.
    ///
    /// See [`SectionStanding::accounts`] for which arms are which and why this
    /// is not folded into [`conforms`](Self::conforms).
    #[must_use]
    pub fn unaccounted(&self) -> usize {
        self.unaccounted_keys().count()
    }

    /// The sections [`unaccounted`](Self::unaccounted) counts, by key.
    ///
    /// Named rather than counted, for the reason
    /// [`ScreenRoster::unsized_keys`](crate::ScreenRoster::unsized_keys) is:
    /// an assertion that reports a number leaves a reader to find out which,
    /// and the question a failing ratchet has to answer is *which one*.
    pub fn unaccounted_keys(&self) -> impl Iterator<Item = &str> {
        self.rows
            .iter()
            .filter(|row| !row.standing.accounts())
            .map(|row| row.key.as_str())
    }

    /// How many destinations cannot be arrived at.
    #[must_use]
    pub fn closed(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| !row.standing.is_open())
            .count()
    }

    /// How many parts every judged section's specification fixes, added up.
    #[must_use]
    pub fn specified(&self) -> usize {
        self.judged_reports().map(DocumentReport::specified).sum()
    }

    /// How many of them the build has where they were specified.
    #[must_use]
    pub fn reproduced(&self) -> usize {
        self.judged_reports().map(DocumentReport::reproduced).sum()
    }

    /// ★★★★★ Whether this application reproduces its specification.
    ///
    /// **False while any open section is unjudged**, and that is the point of
    /// the type. An application assembled from sections must not be able to
    /// report conformance on the strength of the ones somebody wrote a
    /// specification for — the count of judged sections is part of the verdict,
    /// not a footnote under it.
    ///
    /// ★★★★★ R1758 — **and false while any judged section answered from its own
    /// tables**, which is the same rule one level down. R1742 settled that a
    /// verdict read from the model is structurally consistent with the model
    /// and therefore proves nothing; leaving it as prose in one screen's header
    /// cost what prose costs. Measured on this tree's running application at
    /// R1747 and again at R1758: two of four judged sections reported every
    /// part of their specification reproduced while they had not painted a
    /// frame in that session, and no count anywhere distinguished them from the
    /// two that answered honestly.
    ///
    /// A section that genuinely cannot be judged from what it painted says so
    /// per surface with [`Built::Away`](pinion_core::conformance::Built::Away)
    /// and a reason. That is a narrower claim than asserting a roster, and it
    /// is the one R1742 made non-escapable — an away surface reconciles nothing.
    ///
    /// ★★★★★ R1767 — **and for an application with two open sections this is
    /// unreachable, by construction rather than by defect.** One frame paints
    /// one section, so every other section is away, and an away surface
    /// reconciles nothing; there is no build of such an application that makes
    /// this true. That is the honest reading of *a verdict is about a frame*,
    /// and it is a fact about the question rather than an answer to it, so it
    /// is recorded here where somebody trying to make this true will look.
    ///
    /// The question that HAS an answer for an assembled application is
    /// [`JourneyConformance::conforms`](crate::JourneyConformance::conforms):
    /// how much of its specification each section reproduced somewhere along a
    /// walk, with every credited verdict naming the step it was read at. This
    /// one stays, unchanged and still the right question about the frame in
    /// front of a reader.
    #[must_use]
    pub fn conforms(&self) -> bool {
        self.unjudged() == 0
            && self.declared() == 0
            && self.judged_reports().all(DocumentReport::reconciles)
    }

    /// The report of each judged section.
    fn judged_reports(&self) -> impl Iterator<Item = &DocumentReport> {
        self.rows.iter().filter_map(|row| match &row.standing {
            SectionStanding::Judged(report) => Some(report),
            SectionStanding::Unspecified(_)
            | SectionStanding::Inline
            | SectionStanding::Closed(_) => None,
        })
    }

    /// The report as the value a running application publishes.
    ///
    /// The counts ride *beside* the rows rather than being left for a client to
    /// recompute, because a client recomputing them is a client that can
    /// disagree with the application about how much of it was judged.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "sections": self.sections(),
            "judged": self.judged(),
            // ★ R1758 — beside `judged`, because "compared with a
            // specification" and "compared with a painted frame" are two
            // populations and a reader adding the first to a headline is
            // reading the second.
            "declared": self.declared(),
            "unjudged": self.unjudged(),
            // ★★★★★ R1888 — beside `unjudged` and for the reason `declared`
            // sits beside `judged`: it is a SUBSET of the count above it, and
            // the two answer different questions. `unjudged` says how many
            // sections nothing compared with a specification; this says how
            // many of those will not even say why. A reader who has only the
            // first cannot tell a known gap from an unlooked-at one.
            "unaccounted": self.unaccounted(),
            "closed": self.closed(),
            "specified": self.specified(),
            "reproduced": self.reproduced(),
            "conforms": self.conforms(),
            "rows": self
                .rows
                .iter()
                .map(|row| {
                    let mut value = serde_json::json!({
                        "key": row.key,
                        "title": row.title,
                        "showing": row.showing,
                        "standing": row.standing.word(),
                        // ★★★★★ R1888 — whether the `why` beside it (when
                        // there is one) is the SUBJECT's reason or the host's
                        // admission that nobody answered. Published per row
                        // rather than left to a client to infer from the
                        // sentence, because inferring it means recognising a
                        // constant string — which is a client keeping a copy of
                        // one of this framework's values, the failure this
                        // report exists to end.
                        "accounts": row.standing.accounts(),
                    });
                    if let Some(tag) = &row.tag {
                        value["tag"] = serde_json::Value::String(tag.clone());
                    }
                    if let Some(why) = row.standing.why() {
                        value["why"] = serde_json::Value::String(why);
                    }
                    if let SectionStanding::Judged(report) = &row.standing {
                        // ★★★★★ R1758 — the section's verdict, WHOLE and under
                        // one key. It was three of its facts spread flat here
                        // (`surfaces`, `specified`, `reproduced`), which is how
                        // a partial verdict comes to read as a complete one:
                        // the row said how much was reproduced and had no room
                        // to say what that was measured against, so the two
                        // sections answering from their own tables were
                        // indistinguishable here from the two answering from a
                        // frame. Nesting also makes the row and the section's
                        // own published slot the SAME value rather than two
                        // renderings of it — the "one word, two documents"
                        // class R1747 spent a round on.
                        value["conformance"] = report.to_json();
                    }
                    value
                })
                .collect::<Vec<_>>(),
        })
    }
}
