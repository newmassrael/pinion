//! Walking a whole application past its own specification — the **populations**
//! a walk needs, derived from the roster rather than written out beside it.
//!
//! # What was here already, and what was not
//!
//! [`JourneyConformance`](crate::journey::JourneyConformance) answers *did this
//! application reproduce its specification over the walk a reader took*, and it
//! is complete: it folds the live section in, counts a section nobody visited as
//! unreproduced, and refuses the application while any section is unanswered.
//!
//! What it does not do is **walk**. Driving one takes two lists, and until this
//! module every caller wrote both by hand:
//!
//! 1. **Where to go.** The open destinations, in order.
//! 2. **Which surfaces the frame must record.** A verdict about a section is
//!    read out of the paint store under that section's own paint-root tag, so a
//!    frame that recorded the wrong tags produces verdicts about a store it
//!    never filled.
//!
//! The first list is self-correcting and the second is not, and the difference
//! is worth stating because it decides which one this module exists for.
//! Forget a destination and `conforms()` goes **false** — the section is still
//! in the report, unvisited, and unreproduced. Forget a *surface tag* and the
//! section is painted, judged, and reported as not reproducing anything, for a
//! reason that has nothing to do with the screen. One mistake fails loudly; the
//! other fails while looking like a defect somewhere else entirely.
//!
//! So [`Tour::surfaces`] derives the tag list from the roster's own mounted
//! screens, and [`Tour::itinerary`] derives the walk from its own open
//! destinations. Neither can be shorter than the application is.
//!
//! # The frame protocol
//!
//! Per stop: **navigate, paint, record the surfaces, hand the stop back.** This
//! module latches it. Latching records the *departing* section's verdict — the
//! frame a reader actually saw, read at the last instant it is still in the
//! store — so the recording must already have happened when a stop is returned.
//! [`Tour::walk`]'s closure is where those three steps go, and its documentation
//! is the contract.
//!
//! This crate cannot perform the paint itself: recording a scene's surfaces
//! lives in the render crate, which depends on this one. That boundary is why
//! the closure exists rather than a `Tour::run(&app)`.

use std::collections::BTreeSet;

use pinion_core::Scene;

use pinion_core::widgets::destination::Journey;

use crate::ScreenRoster;
use crate::journey::JourneyConformance;

/// A walk over every open destination an application declares.
///
/// Built from the roster so that neither of its two populations can be written
/// shorter than the application is. See the module documentation for why the
/// surface list is the one that matters.
pub struct Tour<'a> {
    roster: &'a ScreenRoster,
    host_surfaces: Vec<String>,
}

impl<'a> Tour<'a> {
    /// A tour of `roster`.
    #[must_use]
    pub fn of(roster: &'a ScreenRoster) -> Self {
        Self {
            roster,
            host_surfaces: Vec::new(),
        }
    }

    /// Also record `tag` at every stop — the paint-root of a page the **host**
    /// paints itself rather than mounting.
    ///
    /// A host page has no `Screen` and therefore no `tag_of`, so the roster
    /// cannot derive it. It is the one part of the surface list a caller must
    /// supply, and it is declared once here instead of being spelled at each
    /// stop.
    #[must_use]
    pub fn also_recording(mut self, tag: impl Into<String>) -> Self {
        self.host_surfaces.push(tag.into());
        self
    }

    /// **Where to go** — every open destination, in roster order.
    ///
    /// Derived, so a section added to the rail joins the walk without anybody
    /// editing a list.
    #[must_use]
    pub fn itinerary(&self) -> Vec<String> {
        self.roster
            .destinations()
            .open()
            .map(|destination| destination.key.to_string())
            .collect()
    }

    /// **Which surfaces a frame must record** for this roster's verdicts to be
    /// about frames that were really painted: every mounted screen's paint-root
    /// tag, plus whatever [`also_recording`](Self::also_recording) declared.
    ///
    /// This is the list that fails quietly when it is wrong, which is why it is
    /// computed here and not spelled at the call site.
    #[must_use]
    pub fn surfaces(&self) -> Vec<String> {
        let mut out: Vec<String> = self.host_surfaces.clone();
        for key in self.roster.mounted_keys() {
            if let Some(tag) = self.roster.tag_of(key)
                && !out.iter().any(|held| held == tag)
            {
                out.push(tag.to_owned());
            }
        }
        out
    }

    /// Walk the [`itinerary`](Self::itinerary), latching each stop, and report.
    ///
    /// Two closures, because the order between them is the contract:
    ///
    /// * `arrive` navigates to the key and returns the resulting journey. It
    ///   must **not** paint.
    /// * `paint` is called with the key and a pose ordinal, paints that
    ///   frame — view, layout — **records** the surfaces
    ///   [`surfaces`](Self::surfaces) names, and returns the scene.
    ///
    /// **How many times `paint` is called is the section's own answer**, not
    /// the caller's: this asks [`ScreenRoster::poses_of`] and calls
    /// [`ScreenRoster::pose`] before each frame, so a walk never has to know
    /// that one particular screen's surfaces exclude each other. That is the
    /// third population this module takes off the roster instead of a call site.
    ///
    /// ★★★★★ **Some sections need more than one frame.** Measured on this
    /// tree's own analysis tool at R1808: the
    /// node lab specifies an `enum_roster` surface and an `enum_row` surface,
    /// and the roster is *the row's open state* — so the two are MUTUALLY
    /// EXCLUSIVE and no single frame can show both. A one-frame-per-section
    /// walk reports that section as never reproducing its specification, and
    /// the report is right: the fault is the walk's, for looking once. This is
    /// the same fact that forced the walk over the frame in the first place,
    /// arriving one level up.
    ///
    /// Extra frames at one section are cheap and safe: a stop is counted only
    /// when the journey MOVES, and a step only when a frame says something the
    /// last one did not, so a section driven through five frames that change
    /// nothing reads exactly like one driven through one.
    ///
    /// This latches **between** them, and that is the whole reason they are
    /// separate. Latching records the *departing* section's verdict by reading
    /// the paint store, and the store holds the departing frame only until the
    /// next one is recorded — recording a frame also forgets the surfaces that
    /// frame did not paint. So a walk that paints before it latches destroys
    /// the evidence it is about to ask for.
    ///
    /// ★ Written the other way round first, in the round that built this, and
    /// the application reported **every** section as never having painted a
    /// frame. That failure is why the order lives here instead of in each
    /// caller: it is invisible in the signature, unambiguous in the result, and
    /// wrong in a way that looks like a broken application rather than a broken
    /// harness.
    ///
    /// The scene handed to the latch is the **previously** painted one (an
    /// empty scene at the first stop), for the same reason: latching belongs to
    /// the frame that is leaving. What that scene feeds is a mounted screen's
    /// state-revision change detector, which this report does not read.
    ///
    /// An empty itinerary returns a report that does not conform and says so —
    /// an application with no open destination has nothing to reproduce, and
    /// answering `true` there would make a mis-built roster look compliant.
    pub fn walk<A, P>(&self, mut arrive: A, mut paint: P) -> TourReport
    where
        A: FnMut(&str) -> Journey,
        P: FnMut(&str, usize) -> Scene,
    {
        let itinerary = self.itinerary();
        let mut visited: Vec<String> = Vec::new();
        let mut last: Option<Journey> = None;
        let mut departing = Scene::Container(pinion_core::scene::ContainerNode::new(Vec::new()));
        for key in &itinerary {
            let journey = arrive(key);
            for nth in 0..self.roster.poses_of(key) {
                // Before every paint, never after — see the contract above.
                let _ = self.roster.latch(&journey, &departing);
                // The section puts ITSELF into the state its specification
                // describes; the walk never knows which screen needed it.
                self.roster.pose(key, nth);
                departing = paint(key, nth);
            }
            visited.push(journey.at().to_owned());
            last = Some(journey);
        }
        let walk = last
            .as_ref()
            .map(|journey| self.roster.journey_conformance(journey));
        TourReport {
            itinerary,
            visited,
            walk,
        }
    }
}

/// What a [`Tour::walk`] found.
///
/// Two questions, kept apart: did the walk **cover** the application, and did
/// the application **reproduce** its specification over it. A tour that skipped
/// a section could otherwise report the verdict of the sections it did reach
/// and read as a pass.
pub struct TourReport {
    itinerary: Vec<String>,
    visited: Vec<String>,
    walk: Option<JourneyConformance>,
}

impl TourReport {
    /// The destinations the roster declared open.
    #[must_use]
    pub fn itinerary(&self) -> &[String] {
        &self.itinerary
    }

    /// The destinations the walk actually stood in, in the order it did.
    #[must_use]
    pub fn visited(&self) -> &[String] {
        &self.visited
    }

    /// Declared open but never stood in — empty when the walk covered the
    /// application.
    #[must_use]
    pub fn missed(&self) -> BTreeSet<&str> {
        let stood: BTreeSet<&str> = self.visited.iter().map(String::as_str).collect();
        self.itinerary
            .iter()
            .map(String::as_str)
            .filter(|key| !stood.contains(key))
            .collect()
    }

    /// Stood in but not on the itinerary — a stop whose navigation did not
    /// arrive where it was sent, which is a defect in the application's routing
    /// rather than in the walk.
    #[must_use]
    pub fn strayed(&self) -> BTreeSet<&str> {
        let planned: BTreeSet<&str> = self.itinerary.iter().map(String::as_str).collect();
        self.visited
            .iter()
            .map(String::as_str)
            .filter(|key| !planned.contains(key))
            .collect()
    }

    /// Whether the walk stood in every open destination and strayed nowhere.
    #[must_use]
    pub fn covered(&self) -> bool {
        !self.itinerary.is_empty() && self.missed().is_empty() && self.strayed().is_empty()
    }

    /// The specification verdict accumulated over the walk, or `None` when
    /// nothing was walked.
    #[must_use]
    pub const fn walk(&self) -> Option<&JourneyConformance> {
        self.walk.as_ref()
    }

    /// Whether the application both **was** walked in full and **reproduced**
    /// its specification over that walk.
    #[must_use]
    pub fn conforms(&self) -> bool {
        self.covered() && self.walk.as_ref().is_some_and(JourneyConformance::conforms)
    }

    /// Why it does not conform, as a sentence — `None` when it does.
    #[must_use]
    pub fn why(&self) -> Option<String> {
        if self.conforms() {
            return None;
        }
        let mut parts = Vec::new();
        if self.itinerary.is_empty() {
            parts.push("the roster declares no open destination to walk".to_owned());
        }
        let missed = self.missed();
        if !missed.is_empty() {
            parts.push(format!(
                "never stood in: {}",
                missed.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
        let strayed = self.strayed();
        if !strayed.is_empty() {
            parts.push(format!(
                "arrived somewhere it was not sent: {}",
                strayed.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
        // The specification half, section by section. `JourneyConformance` has
        // no one-line `why` — a walk fails per section and a single sentence
        // would have to pick one — so the rows that did not reproduce name
        // themselves, each with its own standing's reason.
        if let Some(walk) = &self.walk {
            for row in walk.rows() {
                if !row.is_open() || row.reproduced_over_the_walk() {
                    continue;
                }
                // A section fails for one of two reasons and they need
                // different words. Either nothing answered for it — the
                // standing says so — or something did and some SURFACE of its
                // specification never reconciled on any frame of the walk. The
                // first draft of this printed only the first case's sentence
                // and fell back to "did not reproduce its specification" for
                // the second, which is the failure restated rather than
                // explained: every section reported the same six words and none
                // of them said which surface.
                if let Some(why) = row.standing.why() {
                    parts.push(format!("{}: {why}", row.key));
                    continue;
                }
                let mut unreconciled: Vec<String> = row
                    .standing
                    .surfaces()
                    .iter()
                    .filter(|visit| !visit.reconciles())
                    .map(|visit| match visit.why() {
                        Some(why) => format!("{} ({why})", visit.surface()),
                        None => format!(
                            "{} ({} of {} reproduced)",
                            visit.surface(),
                            visit.reproduced(),
                            visit.specified()
                        ),
                    })
                    .collect();
                if unreconciled.is_empty() {
                    unreconciled.push("no surface of its specification was seen".to_owned());
                }
                parts.push(format!("{}: {}", row.key, unreconciled.join(", ")));
            }
        }
        Some(parts.join("; "))
    }
}
