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
//! | [`Inline`](SectionStanding::Inline) | the host paints this page itself, so this roster has no screen to ask |
//! | [`Closed`](SectionStanding::Closed) | you cannot arrive, and this is the reason |
//!
//! There is no catch-all. `Unspecified` and `Inline` are kept apart because the
//! work that closes them is different — one is *write the specification down*,
//! the other is *give the host's own page a [`Screen`](crate::Screen)*, which
//! the trait is public for.
//!
//! # The one rule that makes it worth having
//!
//! [`ApplicationConformance::conforms`] is false while **any open section is
//! unjudged**. An application must not be able to report conformance on the
//! strength of the sections somebody happened to write a specification for;
//! that is precisely the reading the measurement above found, and a report that
//! permitted it would be the defect with a type around it.
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
    /// which leaves one live case rather than two — and leaves the remainder
    /// open, because the type is unchanged and the next screen in that position
    /// would be silent in exactly the same way. Worth noting that the repair is
    /// now half built: a *surface* can say why it is not judged
    /// ([`Built::Away`](pinion_core::conformance::Built::Away)) and a *section*
    /// still cannot.
    Unspecified,
    /// The destination is open and its page is one the host paints itself, so
    /// this roster has no screen to ask.
    ///
    /// Closing it means giving that page a [`Screen`](crate::Screen) of its
    /// own, which is what the trait being public is for.
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
        matches!(self, SectionStanding::Unspecified | SectionStanding::Inline)
    }

    /// The word this arm is published under.
    #[must_use]
    pub const fn word(&self) -> &'static str {
        match self {
            SectionStanding::Judged(_) => "judged",
            SectionStanding::Unspecified => "unspecified",
            SectionStanding::Inline => "inline",
            SectionStanding::Closed(_) => "closed",
        }
    }

    /// Why this section is not judged, in one sentence, or `None` when it is.
    #[must_use]
    pub fn why(&self) -> Option<String> {
        match self {
            SectionStanding::Judged(_) => None,
            SectionStanding::Unspecified => Some(
                "a screen is here and it publishes no verdict about a specification".to_owned(),
            ),
            SectionStanding::Inline => {
                Some("the host paints this page itself, so there is no screen to ask".to_owned())
            }
            SectionStanding::Closed(why) => Some(why.sentence()),
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

    /// How many sections a reader can arrive at that nothing has judged.
    #[must_use]
    pub fn unjudged(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.standing.is_unjudged())
            .count()
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
    #[must_use]
    pub fn conforms(&self) -> bool {
        self.unjudged() == 0 && self.judged_reports().all(DocumentReport::reconciles)
    }

    /// The report of each judged section.
    fn judged_reports(&self) -> impl Iterator<Item = &DocumentReport> {
        self.rows.iter().filter_map(|row| match &row.standing {
            SectionStanding::Judged(report) => Some(report),
            SectionStanding::Unspecified | SectionStanding::Inline | SectionStanding::Closed(_) => {
                None
            }
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
            "unjudged": self.unjudged(),
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
                    });
                    if let Some(tag) = &row.tag {
                        value["tag"] = serde_json::Value::String(tag.clone());
                    }
                    if let Some(why) = row.standing.why() {
                        value["why"] = serde_json::Value::String(why);
                    }
                    if let SectionStanding::Judged(report) = &row.standing {
                        value["surfaces"] = report.to_json();
                        value["specified"] = report.specified().into();
                        value["reproduced"] = report.reproduced().into();
                    }
                    value
                })
                .collect::<Vec<_>>(),
        })
    }
}
