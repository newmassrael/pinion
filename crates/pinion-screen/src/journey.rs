//! ★★★★★ R1767 — **a walk reproduces a specification no single frame can.**
//!
//! # What forced this module, measured
//!
//! [`ApplicationConformance::conforms`](crate::ApplicationConformance::conforms)
//! is `unjudged == 0 && declared == 0 && every judged report reconciles`, and
//! by R1763 all three clauses were finally honest. Together they had also
//! become **unreachable by construction**, because a frame shows one section:
//!
//! ```text
//! one frame paints one section
//! => every other section is away
//! => an away surface reconciles nothing (R1742)
//! => an application with two open sections can never report conformance
//! ```
//!
//! Measured on this tree's analysis tool at the head of R1767, by walking every
//! open section once and returning: headline `26 of 133`, `conforms = false` —
//! the same numbers as at boot, which is R1763's repair working exactly as
//! specified. The honest answer, and no vocabulary for the question a reader
//! actually has.
//!
//! ★★★★★ **And the second half, which the debt did not know about.** The same
//! walk, standing *inside* each section rather than reading from outside:
//!
//! ```text
//! dashboard  26/28  reconciles=true      logs      15/15  reconciles=true
//! packets    25/26  reconciles=true      lab        1/15  reconciles=FALSE
//! keys       21/21  reconciles=true      settings  26/28  reconciles=true
//! ```
//!
//! `lab` fails **while the reader is standing in it**, and not because the
//! screen is wrong. Its specification names an enumeration row *with its roster
//! shut* and the roster *standing over it*, and those two states exclude each
//! other — the node lab's own judge said so in prose at R1742: *this document
//! cannot be fully judged at any one instant. A reader who wants the whole
//! verdict drives the session and reads twice.* Nothing anywhere could hold the
//! two readings, so "reads twice" meant *a person compares two printouts*.
//!
//! So the missing vocabulary is not merely per-section. It is per **surface**,
//! and the unit that carries it is the walk.
//!
//! # How this coexists with "a verdict is about one frame"
//!
//! That rule (R1742, and R1758 one level down) is **kept, not relaxed**, and it
//! is the whole design:
//!
//! * Nothing here is credited to a frame that did not paint it. A
//!   [`SurfaceVisit`] carries a verdict only from a step at which that surface
//!   was **standing**; an away surface contributes its specification and
//!   nothing else, exactly as it does one level down.
//! * Every credited verdict **names the step it was read at**. A journey report
//!   is not one verdict about many frames — it is many one-frame verdicts, each
//!   labelled with the frame it came from, added up by a type that says so.
//! * The walk is therefore part of the claim, and
//!   [`JourneyConformance::steps`] publishes how long it was. An application
//!   that conforms after a nine-stop walk and one that conforms after two are
//!   making different statements, and a reader can tell them apart.
//!
//! # What it refuses
//!
//! [`JourneyConformance::conforms`] is false while any of these holds, and the
//! list is the type's reason for existing:
//!
//! | refusal | what it stops |
//! |---|---|
//! | an open section the walk never stood in | conformance earned by the sections somebody happened to visit |
//! | an open section nothing answers for | R1738's defect, at journey scale |
//! | a verdict whose evidence is not a painted frame | R1758's defect, at journey scale |
//! | a specified surface no step ever had on a frame | crediting a surface for being unopened |
//! | a surface that stood and did not reconcile | the thing a specification is for |
//!
//! # Floor, measured against the reference toolkit 6.11.1
//!
//! The probe of R1738 scanned **312** members across that toolkit's page-stack
//! container, its tabbed container and a plain page, and **0** name a
//! specification, an expectation or a divergence — so the per-frame question
//! cannot be asked there at all, let alone accumulated over a walk.
//!
//! ★ And there is nothing to accumulate it *onto*. Measured at 6.11.1 while
//! writing this paragraph, because the first draft of it asserted that the
//! nearest thing there was a page **history**: across the page-stack container,
//! the stacked layout and the tabbed container, the count of members naming a
//! history, a visited page, a back or a forward is **0**. Those containers
//! remember which page is current and nothing about where a reader has been, so
//! a walk is not a shorter version of something they have — it is absent.

use std::collections::BTreeMap;

use pinion_core::availability::Unavailable;
use pinion_core::conformance::{DocumentReport, Evidence, SurfaceStanding};

use crate::conformance::SectionStanding;

/// What a walk saw of one surface a specification names.
///
/// Two facts, kept apart because they answer different questions and a reader
/// holding only one of them is the failure this type exists to prevent:
///
/// * **the latest** verdict any step of the walk produced for this surface,
///   whether or not it was on that frame — which is where the specification's
///   own side comes from, and the reason an away surface is still counted in
///   [`JourneyConformance::specified`];
/// * **the last step at which it was STANDING**, with the verdict that frame
///   gave — which is the only thing this type will ever credit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceVisit {
    surface: String,
    latest: SurfaceStanding,
    stood: Option<(u32, SurfaceStanding)>,
}

/// ★★★★★ R1770 — whether a credited verdict was read at a size the new
/// observation says the surface no longer has.
///
/// Both sizes must be known for this to be a claim: a verdict that named no
/// extent cannot be shown stale by one that does, and saying otherwise would
/// discard credits on every path that has no frame to take a size from. So the
/// predicate is *two known sizes that differ* — which is the same shape as
/// [`pinion_core::conformance::DocumentReport::read_where_written`], and for the
/// same reason: a missing number supports no claim in either direction.
fn stale_at(credited: &SurfaceStanding, now: &SurfaceStanding) -> bool {
    matches!((credited.at(), now.at()), (Some(was), Some(is)) if was != is)
}

impl SurfaceVisit {
    /// Begin a record for the surface this standing is about.
    ///
    /// `step` is `None` for a section no step of the walk has stood in: its
    /// verdict is read anyway, so that its **specification** is in the totals,
    /// and nothing it says is credited. That is the same distinction one level
    /// up between a section missing from the denominator (R1738's defect) and a
    /// section credited for a frame nobody saw (R1763's).
    fn begin(step: Option<u32>, standing: SurfaceStanding) -> Self {
        let mut visit = Self {
            surface: standing.surface().to_owned(),
            latest: standing.clone(),
            stood: None,
        };
        // Through `record` rather than beside it, so the rule about what a
        // standing frame credits has exactly one statement.
        if let Some(step) = step {
            visit.record(step, standing);
        }
        visit
    }

    /// Fold in what the frame at `step` said about this surface.
    ///
    /// A standing frame replaces the credited verdict; an away frame replaces
    /// only the latest one. That asymmetry **is** the rule of this module: a
    /// surface that has been on a frame does not stop having been on it because
    /// the reader closed it again, and a surface that has never been on one is
    /// not credited for the reason it gives.
    ///
    /// ★★★★★ R1770 — **with one exception, and it was found by running.** A
    /// credit is dropped when the new observation was read at a **different
    /// extent**. Measured that round on this tree's own analysis tool: walk it
    /// maximised, where it conforms; shrink the window and walk it again, where
    /// one section is given less width than it declares it lays out at and
    /// therefore declines to be judged; and the walk still reported
    /// `conforms=true` — on the strength of frames painted at a size that no
    /// longer exists.
    ///
    /// The asymmetry above is right and this does not weaken it: *the reader
    /// closed it again* leaves the frame that verdict came from intact, and
    /// *the reader resized the window* does not. It is R1763's rule — a section
    /// that leaves takes its verdict with it — applied to the other way a frame
    /// can stop being the frame it was. It could not be written before this
    /// round because a verdict did not carry the size it was read at.
    fn record(&mut self, step: u32, standing: SurfaceStanding) {
        if self
            .stood
            .as_ref()
            .is_some_and(|(_, credited)| stale_at(credited, &standing))
        {
            self.stood = None;
        }
        if standing.is_standing() {
            self.stood = Some((step, standing.clone()));
        }
        self.latest = standing;
    }

    /// The extent the credited verdict was read at, or `None` while nothing is
    /// credited or the verdict named no size.
    ///
    /// ★ R1770 — published so a reader of a walk can see that its rows were not
    /// all read at one size, which for an assembled tool they never are: a
    /// section mounted as a page is given a fraction of the window.
    #[must_use]
    pub fn at(&self) -> Option<pinion_core::painted::Extent> {
        self.stood.as_ref().and_then(|(_, standing)| standing.at())
    }

    /// The surface this is about.
    #[must_use]
    pub fn surface(&self) -> &str {
        &self.surface
    }

    /// How many parts the specification fixes here.
    ///
    /// The specification's own count, so it is the same whether or not the walk
    /// ever opened this surface — the property [`SurfaceStanding::specified`]
    /// already has, carried up unchanged.
    #[must_use]
    pub fn specified(&self) -> usize {
        self.latest.specified()
    }

    /// Whether any step of the walk had this surface on a frame.
    #[must_use]
    pub const fn stood(&self) -> bool {
        self.stood.is_some()
    }

    /// The step at which it was last standing, or `None` while no step had it.
    #[must_use]
    pub fn step(&self) -> Option<u32> {
        self.stood.as_ref().map(|(step, _)| *step)
    }

    /// How many specified parts the build had, on the frame this credits.
    ///
    /// **Zero while no step of the walk had this surface on a frame**, for the
    /// same reason the per-frame report answers zero for an away surface:
    /// crediting what nobody opened is the direction of error that inflates a
    /// report silently.
    #[must_use]
    pub fn reproduced(&self) -> usize {
        self.stood
            .as_ref()
            .map_or(0, |(_, standing)| standing.reproduced())
    }

    /// Whether the difference this surface had, on the frame this credits, is
    /// the difference somebody wrote down.
    ///
    /// False while no step had it on a frame — declining to be judged is not
    /// passing, which is [`SurfaceStanding::reconciles`]'s rule and this is the
    /// same one over a walk instead of an instant.
    #[must_use]
    pub fn reconciles(&self) -> bool {
        self.stood
            .as_ref()
            .is_some_and(|(_, standing)| standing.reconciles())
    }

    /// The verdict this credits: the last standing frame's, or the latest
    /// reading when no frame ever had it.
    #[must_use]
    pub fn standing(&self) -> &SurfaceStanding {
        self.stood
            .as_ref()
            .map_or(&self.latest, |(_, standing)| standing)
    }

    /// Why the last step that could have shown this surface did not, or `None`
    /// when the latest reading had it on the frame.
    #[must_use]
    pub fn why(&self) -> Option<&str> {
        self.latest.why()
    }

    /// This visit as the value a running application publishes.
    ///
    /// The credited verdict's own row, plus the two facts a walk adds: whether
    /// any frame ever had it, and which step that was. `why` rides through from
    /// the row when the latest reading was away — so a reader can see both *it
    /// stood at step 5* and *it is not on the frame now*.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut row = self.standing().to_json();
        row["stood"] = serde_json::Value::Bool(self.stood());
        row["step"] = self.step().map_or(serde_json::Value::Null, |step| {
            serde_json::Value::Number(step.into())
        });
        row["reconciles"] = serde_json::Value::Bool(self.reconciles());
        if let Some(why) = self.why() {
            row["why"] = serde_json::Value::String(why.to_owned());
        }
        row
    }
}

/// What one destination of an application can say about a **walk**.
///
/// The peer of [`SectionStanding`], which says the same kind of thing about one
/// frame. There is no `Unvisited` arm: whether the walk arrived is
/// [`JourneySection::arrived`], and keeping it out of here is what lets a
/// section the walk never reached still contribute its **specification** to the
/// totals — a section missing from the denominator is the R1738 defect wearing
/// a different hat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JourneyStanding {
    /// Something answers for this section, and this is what the walk saw of
    /// every surface its specification names.
    Judged {
        /// Where the built side of the credited verdicts came from.
        evidence: Evidence,
        /// One record per surface the specification names, in its order.
        surfaces: Vec<SurfaceVisit>,
    },
    /// Nothing answers for this section, and this is the reason.
    Unanswered(String),
    /// A reader cannot arrive here, and this is the destination's own reason.
    Closed(Unavailable),
}

impl JourneyStanding {
    /// Start a record from what one frame said, or — with `step` `None` — from
    /// a section no step of the walk has stood in.
    pub(crate) fn of(step: Option<u32>, standing: SectionStanding) -> Self {
        match standing {
            SectionStanding::Judged(report) => JourneyStanding::Judged {
                evidence: report.evidence(),
                surfaces: report
                    .surfaces()
                    .iter()
                    .map(|standing| SurfaceVisit::begin(step, standing.clone()))
                    .collect(),
            },
            SectionStanding::Closed(why) => JourneyStanding::Closed(why),
            other => JourneyStanding::Unanswered(
                other
                    .why()
                    .unwrap_or_else(|| "nothing answers for this section".to_owned()),
            ),
        }
    }

    /// Fold in what the frame at `step` said.
    ///
    /// Only a judged frame folded onto a judged record merges; every other
    /// pairing replaces, because the two are not accounts of the same thing —
    /// a section that stopped publishing a verdict has not kept the old one.
    fn absorb(&mut self, step: u32, standing: SectionStanding) {
        if let (JourneyStanding::Judged { evidence, surfaces }, SectionStanding::Judged(report)) =
            (&mut *self, &standing)
        {
            *evidence = report.evidence();
            Self::merge(surfaces, step, report);
            return;
        }
        *self = Self::of(Some(step), standing);
    }

    /// Fold one frame's surfaces into the records kept for them.
    fn merge(surfaces: &mut Vec<SurfaceVisit>, step: u32, report: &DocumentReport) {
        for standing in report.surfaces() {
            match surfaces
                .iter_mut()
                .find(|visit| visit.surface() == standing.surface())
            {
                Some(visit) => visit.record(step, standing.clone()),
                None => surfaces.push(SurfaceVisit::begin(Some(step), standing.clone())),
            }
        }
    }

    /// The word this arm is published under.
    #[must_use]
    pub const fn word(&self) -> &'static str {
        match self {
            JourneyStanding::Judged { .. } => "judged",
            JourneyStanding::Unanswered(_) => "unanswered",
            JourneyStanding::Closed(_) => "closed",
        }
    }

    /// Whether a reader can arrive here at all.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        !matches!(self, JourneyStanding::Closed(_))
    }

    /// Every surface record, empty for a section nothing answers for.
    #[must_use]
    pub fn surfaces(&self) -> &[SurfaceVisit] {
        match self {
            JourneyStanding::Judged { surfaces, .. } => surfaces,
            JourneyStanding::Unanswered(_) | JourneyStanding::Closed(_) => &[],
        }
    }

    /// Why this section is not judged, in one sentence, or `None` when it is.
    #[must_use]
    pub fn why(&self) -> Option<String> {
        match self {
            JourneyStanding::Judged { .. } => None,
            JourneyStanding::Unanswered(why) => Some(why.clone()),
            JourneyStanding::Closed(why) => Some(why.sentence()),
        }
    }
}

/// One destination of an application, and what a walk saw of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JourneySection {
    /// The destination's key, which is what the rail, the page and the wire all
    /// address it by.
    pub key: String,
    /// What a reader calls it.
    pub title: String,
    /// The mounted screen's paint-root tag, or `None` for a page the host
    /// paints itself.
    pub tag: Option<String>,
    /// Whether this is the section the reader is on **now**.
    ///
    /// Kept beside a walk's record for the reason R1742 put it beside a frame's:
    /// the showing section's row is the only one whose credited verdict can
    /// still be moving.
    pub showing: bool,
    /// The step at which the walk first stood here, or `None` while it never
    /// has.
    pub arrived: Option<u32>,
    /// What it can say about the walk.
    pub standing: JourneyStanding,
}

impl JourneySection {
    /// Whether the walk ever stood here.
    #[must_use]
    pub const fn is_visited(&self) -> bool {
        self.arrived.is_some()
    }

    /// Whether a reader can arrive here at all.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.standing.is_open()
    }

    /// Whether this open section reproduced its whole specification somewhere
    /// along the walk.
    ///
    /// The per-section half of [`JourneyConformance::conforms`]: the walk stood
    /// here, something answered, it answered from painted frames, and every
    /// surface the specification names was on one of those frames and
    /// reconciled there.
    #[must_use]
    pub fn reproduced_over_the_walk(&self) -> bool {
        match &self.standing {
            JourneyStanding::Judged { evidence, surfaces } => {
                self.is_visited()
                    && evidence.is_paint()
                    && !surfaces.is_empty()
                    && surfaces.iter().all(SurfaceVisit::reconciles)
            }
            JourneyStanding::Unanswered(_) => false,
            // A destination nobody can arrive at owes the walk nothing.
            JourneyStanding::Closed(_) => true,
        }
    }

    /// This row as the value a running application publishes.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "key": self.key,
            "title": self.title,
            "showing": self.showing,
            "standing": self.standing.word(),
            "visited": self.is_visited(),
            "arrived": self
                .arrived
                .map_or(serde_json::Value::Null, |step| serde_json::Value::Number(step.into())),
            "reproduced_over_the_walk": self.reproduced_over_the_walk(),
        });
        if let Some(tag) = &self.tag {
            value["tag"] = serde_json::Value::String(tag.clone());
        }
        if let Some(why) = self.standing.why() {
            value["why"] = serde_json::Value::String(why);
        }
        if let JourneyStanding::Judged { evidence, surfaces } = &self.standing {
            value["evidence"] = serde_json::Value::String(evidence.wire().to_owned());
            value["specified"] = surfaces
                .iter()
                .map(SurfaceVisit::specified)
                .sum::<usize>()
                .into();
            value["reproduced"] = surfaces
                .iter()
                .map(SurfaceVisit::reproduced)
                .sum::<usize>()
                .into();
            value["stood"] = surfaces.iter().filter(|v| v.stood()).count().into();
            let mut rows = serde_json::Map::new();
            for visit in surfaces {
                rows.insert(visit.surface().to_owned(), visit.to_json());
            }
            value["surfaces"] = serde_json::Value::Object(rows);
        }
        value
    }
}

/// ★★★★★ Every destination of an application, and how much of its own
/// specification each section reproduced **somewhere along a walk**.
///
/// See the [module documentation](self) for the measurement that forced this
/// and for how it keeps R1742's rule rather than relaxing it.
///
/// # Examples
///
/// ```
/// use pinion_core::widgets::destination::{Destination, Destinations, Journey};
/// use pinion_core::availability::Unavailable;
/// use pinion_screen::ScreenRoster;
///
/// let destinations = Destinations::new(vec![
///     Destination::open("home", "Home"),
///     Destination::open("away", "Away"),
///     Destination::closed("later", "Later", Unavailable::reserved("requirement 12")),
/// ])
/// .expect("the fixture is a roster");
/// let journey = Journey::begin(&destinations, "home").expect("`home` is open");
/// let roster = ScreenRoster::new(destinations, Vec::new())
///     .expect("nothing is mounted, so nothing can be mounted wrongly");
///
/// let walk = roster.journey_conformance(&journey);
/// assert_eq!(walk.sections(), 3);
/// assert_eq!(walk.open(), 2);
///
/// // The reader is standing on `home`, so the walk has been there; `away` is
/// // the open section it has not reached, and a report that left it out would
/// // be conformance earned by the sections somebody happened to visit.
/// assert_eq!(walk.unvisited(), 1);
///
/// // Nothing answers for either page, and nothing has painted, so there is
/// // no verdict to be had — and the report says so rather than passing.
/// assert_eq!(walk.unanswered(), 2);
/// assert_eq!(walk.stood(), 0);
/// assert!(!walk.conforms());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JourneyConformance {
    steps: u32,
    stops: u32,
    rows: Vec<JourneySection>,
}

impl JourneyConformance {
    /// Build a report over every destination.
    ///
    /// Called by
    /// [`ScreenRoster::journey_conformance`](crate::ScreenRoster::journey_conformance),
    /// which is the only place that has both the walk and the roster.
    /// ★ `steps` is **derived from the rows**, not carried in beside them. The
    /// recorder's own counter and the ordinals in the report would be two
    /// tallies of one fact, and the moment they disagreed the report would name
    /// a step it did not contain — the second-account shape this tree keeps
    /// refusing (R1739, R1766).
    pub(crate) fn new(stops: u32, rows: Vec<JourneySection>) -> Self {
        let furthest = rows
            .iter()
            .flat_map(|row| {
                row.arrived.into_iter().chain(
                    row.standing
                        .surfaces()
                        .iter()
                        .filter_map(SurfaceVisit::step),
                )
            })
            .max()
            .unwrap_or(0);
        Self {
            steps: furthest,
            stops,
            rows,
        }
    }

    /// The furthest step this report reaches.
    ///
    /// A step is **one observation that saw something different** — not one
    /// arrival, and not one frame — so it is what a credited verdict names when
    /// it says which frame it came from. A frame repeating what the last one
    /// said takes no step, which is what stops this from counting wall-clock.
    /// Two verdicts at different steps cannot be about one frame, which is the
    /// property that lets this type hold a
    /// specification whose surfaces exclude each other without ever claiming
    /// they were on screen together.
    #[must_use]
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// How many destinations the walk has arrived at.
    ///
    /// One at the destination it opened on, and one more for each arrival
    /// since. It rides beside the verdict because *conformed over three stops*
    /// and *conformed over nine* are different claims about the same
    /// application — and because it is emphatically **not**
    /// [`steps`](Self::steps): a reader who opens something and closes it again
    /// has taken two steps at one stop.
    #[must_use]
    pub const fn stops(&self) -> u32 {
        self.stops
    }

    /// Every destination, in the roster's order.
    #[must_use]
    pub fn rows(&self) -> &[JourneySection] {
        &self.rows
    }

    /// How many destinations this application has.
    #[must_use]
    pub fn sections(&self) -> usize {
        self.rows.len()
    }

    /// How many of them a reader can arrive at.
    #[must_use]
    pub fn open(&self) -> usize {
        self.rows.iter().filter(|row| row.is_open()).count()
    }

    /// How many cannot be arrived at.
    #[must_use]
    pub fn closed(&self) -> usize {
        self.rows.iter().filter(|row| !row.is_open()).count()
    }

    /// How many open sections the walk has stood in.
    #[must_use]
    pub fn visited(&self) -> usize {
        self.open_rows().filter(|row| row.is_visited()).count()
    }

    /// How many open sections the walk has never stood in.
    #[must_use]
    pub fn unvisited(&self) -> usize {
        self.open_rows().filter(|row| !row.is_visited()).count()
    }

    /// How many open sections nothing answers for.
    #[must_use]
    pub fn unanswered(&self) -> usize {
        self.open_rows()
            .filter(|row| matches!(row.standing, JourneyStanding::Unanswered(_)))
            .count()
    }

    /// How many judged sections answered from their own tables rather than from
    /// a painted frame.
    ///
    /// R1758's count, over a walk. A verdict that could not fail is not made
    /// truer by being taken nine times.
    #[must_use]
    pub fn declared(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| match &row.standing {
                JourneyStanding::Judged { evidence, .. } => !evidence.is_paint(),
                JourneyStanding::Unanswered(_) | JourneyStanding::Closed(_) => false,
            })
            .count()
    }

    /// How many surfaces the specifications of this application name, added up.
    #[must_use]
    pub fn surfaces(&self) -> usize {
        self.visits().count()
    }

    /// How many of those surfaces some step of the walk had on a frame.
    ///
    /// ★ The number that makes this report worth having: it is what separates
    /// *the reader never opened it* from *it is not there*, and one frame can
    /// never make it equal [`surfaces`](Self::surfaces) in an application whose
    /// specification names surfaces that exclude each other.
    #[must_use]
    pub fn stood(&self) -> usize {
        self.visits().filter(|visit| visit.stood()).count()
    }

    /// How many parts every section's specification fixes, added up.
    #[must_use]
    pub fn specified(&self) -> usize {
        self.visits().map(SurfaceVisit::specified).sum()
    }

    /// How many of them the build had, on the frames this credits.
    #[must_use]
    pub fn reproduced(&self) -> usize {
        self.visits().map(SurfaceVisit::reproduced).sum()
    }

    /// How many surfaces stood somewhere and did not reconcile there.
    #[must_use]
    pub fn unreconciled(&self) -> usize {
        self.visits()
            .filter(|visit| visit.stood() && !visit.reconciles())
            .count()
    }

    /// ★★★★★ Whether this application reproduced its specification over this
    /// walk.
    ///
    /// See the table in the [module documentation](self) for each refusal and
    /// what it stops. In one sentence: **every open section was stood in,
    /// something answered for each from painted frames, and every surface those
    /// specifications name was on one of those frames and reconciled there.**
    ///
    /// It is a claim about a walk and not about an instant, and
    /// [`steps`](Self::steps) is published beside it so nobody can read it as
    /// the second thing. The per-frame claim is still
    /// [`ApplicationConformance::conforms`](crate::ApplicationConformance::conforms)
    /// and is still the right question to ask about the frame in front of you.
    #[must_use]
    pub fn conforms(&self) -> bool {
        self.rows
            .iter()
            .all(JourneySection::reproduced_over_the_walk)
    }

    /// The report as the value a running application publishes.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "steps": self.steps(),
            "stops": self.stops(),
            "sections": self.sections(),
            "open": self.open(),
            "closed": self.closed(),
            "visited": self.visited(),
            "unvisited": self.unvisited(),
            "unanswered": self.unanswered(),
            "declared": self.declared(),
            "surfaces": self.surfaces(),
            "stood": self.stood(),
            "specified": self.specified(),
            "reproduced": self.reproduced(),
            "unreconciled": self.unreconciled(),
            "conforms": self.conforms(),
            "rows": self.rows.iter().map(JourneySection::to_json).collect::<Vec<_>>(),
        })
    }

    /// Every open destination's row.
    fn open_rows(&self) -> impl Iterator<Item = &JourneySection> {
        self.rows.iter().filter(|row| row.is_open())
    }

    /// Every surface record of every section.
    fn visits(&self) -> impl Iterator<Item = &SurfaceVisit> {
        self.rows.iter().flat_map(|row| row.standing.surfaces())
    }
}

/// What one section's records look like part-way through a walk.
#[derive(Clone, Debug)]
pub(crate) struct SectionWalk {
    arrived: u32,
    /// The last verdict folded in here, kept so an identical one can be
    /// recognised as **the same observation** rather than a new step.
    last: SectionStanding,
    standing: JourneyStanding,
}

impl SectionWalk {
    /// Begin a section's record at the step the walk first stood in it.
    fn begin(step: u32, standing: SectionStanding) -> Self {
        Self {
            arrived: step,
            last: standing.clone(),
            standing: JourneyStanding::of(Some(step), standing),
        }
    }

    /// Whether this frame says exactly what the last one folded in here did.
    fn unchanged(&self, standing: &SectionStanding) -> bool {
        &self.last == standing
    }

    /// Fold in what the frame at `step` said.
    fn record(&mut self, step: u32, standing: SectionStanding) {
        self.last = standing.clone();
        self.standing.absorb(step, standing);
    }

    /// The step the walk first stood here.
    pub(crate) const fn arrived(&self) -> u32 {
        self.arrived
    }

    /// What has been seen of this section so far.
    pub(crate) fn standing(&self) -> &JourneyStanding {
        &self.standing
    }
}

/// ★★★★★ The record a roster keeps of the walk a reader is taking.
///
/// # Why the roster keeps it rather than the host
///
/// Because the two things that make the report trustworthy are the roster's
/// alone. The **population** is the destination roster, so a section cannot
/// fall out of the report by being forgotten — only by not being in the
/// application; and the **moment** is
/// [`ScreenRoster::latch`](crate::ScreenRoster::latch), which is where R1763
/// discards a departed screen's marks, so this is the last instant at which the
/// frame a reader actually saw can still be read. A host accumulating its own
/// journal would be a host that can disagree with the application about what it
/// showed.
///
/// # What it observes, and why the PREVIOUS position
///
/// A latch happens *before* the frame it belongs to is painted, so at that
/// instant the paint store still holds the frame the **last** latch's section
/// drew. Recording the journey's current position there would attribute one
/// section's marks to another — measured as the first draft of this type did
/// exactly that, on the frame after every navigation. So each observation is
/// about `at`, the position the previous latch left behind, and the section a
/// reader is on *right now* is folded in live by
/// [`ScreenRoster::journey_conformance`](crate::ScreenRoster::journey_conformance)
/// instead — which is also what lets a client read a section's verdict on the
/// frame it arrives at rather than one frame later.
#[derive(Default)]
pub(crate) struct Walk {
    at: Option<String>,
    folds: u32,
    stops: u32,
    seen: BTreeMap<String, SectionWalk>,
}

impl Walk {
    /// The step a frame saying something new would be folded in at.
    ///
    /// ★★★★★ A step is **one observation that saw something different**, not
    /// one arrival and not one frame. Both of the other two were written and
    /// both were wrong, and the second was wrong loudly enough to measure:
    ///
    /// * *one arrival* cannot do the job at all — opening a roster and closing
    ///   it again are two frames of one stop, so a report crediting both to
    ///   *stop 4* would say two verdicts came from one frame when the whole
    ///   point is that they cannot have.
    /// * *one frame* is worse in the other direction. Measured on the running
    ///   analysis tool the first time this was driven end to end: a walk of
    ///   **seven stops** reported **17,385 steps**, because a window repaints
    ///   whether or not anything changed. An ordinal that counts wall-clock is
    ///   not a label a reader can use, and a gate asserting on one would be
    ///   asserting how fast the machine is.
    ///
    /// So a frame that says exactly what the last one said is the **same**
    /// observation, and takes no step. Arrivals are counted too, as
    /// [`stops`](Self::stops), because *how far did the reader walk* is a real
    /// question — it is just not this one.
    pub(crate) const fn step(&self) -> u32 {
        self.folds.saturating_add(1)
    }

    /// How many destinations the walk has arrived at, with the opening one
    /// counted before any latch has confirmed it.
    pub(crate) const fn stops(&self) -> u32 {
        if self.stops == 0 { 1 } else { self.stops }
    }

    /// The section the previous latch left showing, whose marks are the ones in
    /// the paint store now.
    pub(crate) fn showing_last(&self) -> Option<&str> {
        self.at.as_deref()
    }

    /// What the walk has recorded for `key`.
    pub(crate) fn seen(&self, key: &str) -> Option<&SectionWalk> {
        self.seen.get(key)
    }

    /// Fold one frame's verdict for `key` into the record, taking a step only
    /// if the frame said something the last one did not.
    pub(crate) fn record(&mut self, key: &str, standing: SectionStanding) {
        match self.seen.get_mut(key) {
            Some(section) if section.unchanged(&standing) => {}
            Some(section) => {
                self.folds = self.folds.saturating_add(1);
                section.record(self.folds, standing);
            }
            None => {
                self.folds = self.folds.saturating_add(1);
                self.seen
                    .insert(key.to_owned(), SectionWalk::begin(self.folds, standing));
            }
        }
    }

    /// Note where the journey is now, counting a stop when it has moved.
    pub(crate) fn arrive(&mut self, at: &str) {
        if self.at.as_deref() != Some(at) {
            self.stops = self.stops.saturating_add(1);
            self.at = Some(at.to_owned());
        }
    }

    /// A section's record with one more frame folded in, without disturbing the
    /// record itself.
    ///
    /// This is the live fold: the frame in front of the reader has been painted
    /// and not yet latched, and a report that left it out would make a client
    /// navigate away to see the section it is looking at.
    pub(crate) fn with_live(&self, key: &str, standing: SectionStanding) -> SectionWalk {
        match self.seen.get(key) {
            // Nothing new, so nothing to fold and no step to take: this is the
            // observation already recorded, and giving it a second ordinal
            // would say the application had changed because somebody looked.
            Some(section) if section.unchanged(&standing) => section.clone(),
            Some(section) => {
                let mut folded = section.clone();
                folded.record(self.step(), standing);
                folded
            }
            None => SectionWalk::begin(self.step(), standing),
        }
    }
}
