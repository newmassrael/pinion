//! ★★★★★ R1730 §5.16 §5.40 §2 #7 — **a surface, written down, that a built one
//! can be checked against — and a reviewed remainder that must be exactly
//! right.**
//!
//! # What was missing, measured
//!
//! R1728 gave a *navigation* this treatment:
//! [`RosterSpec`](crate::widgets::destination::RosterSpec) compares a written
//! roster of destinations with the one an application runs on, in both
//! directions, and the application it was built for asserts the difference
//! **equals** a declared remainder. The mechanism found three defects in its
//! first three runs, which is the argument for it.
//!
//! What that round could not reach is everything a screen is made of that is
//! not a navigation. A table's columns are an ordered roster of named parts. So
//! are a detail pane's sections, a toolbar's groups, a form's field rows. None
//! of them is a destination — they have no standing, nobody arrives at them —
//! and so none of them could be checked at all, while the *reason* a rail was
//! wrong for several hundred rounds (nothing compared it with anything) applies
//! to every one of them unchanged.
//!
//! This module is the part of R1728 that was never about navigating: an ordered
//! roster of keyed, titled parts; a difference in both directions; and the
//! ledger that turns "we know about that one" from a comment into a value the
//! gate reads.
//!
//! # The two halves, and why they are separate types
//!
//! [`SurfaceSpec`] is what a surface must be. [`Ledger`] is where a build
//! knowingly is not that, each entry carrying the sentence verbatim, the round
//! that accepted it and why. [`Ledger::judge`] compares the two **as an
//! equality**, so three things fail rather than one:
//!
//! * a divergence nobody declared — the direction any check catches;
//! * a declared divergence that is no longer there — *paid off and not
//!   recorded*, which a containment check cannot see and which is how an
//!   accepted-remainder list silently becomes a list of everything;
//! * a divergence whose wording changed — the same key, a different sentence,
//!   which is a surface drifting from one kind of wrong to another.
//!
//! # Against the reference toolkit at 6.11.1
//!
//! Measured by building a probe against it and running it, not by reading about
//! it. The subject is its table view, its header and its item model — the three
//! classes a column roster is made of there.
//!
//! | question | there | here |
//! |---|---|---|
//! | a part of a surface has a stable key | **no.** Six members across the header and the model take a name at all, and all six name the *object* — its object name, its window title, its style sheet. A column is an ordinal and a title | [`Part::key`] |
//! | a surface's parts, written down as a specification | absent — there is nothing to write it as | [`SurfaceSpec`] |
//! | any member naming a specification, an expectation or a divergence | **0**, across all three classes' methods and properties | [`SurfaceSpec::diff`] |
//! | a reader reorders the columns; a check written against the model | **passes** — the model still answers the specified order while what is drawn has changed. Measured by moving section 0 to 3 and asking both | [`PartDivergence::OutOfOrder`], which names the key and both places |
//! | a part that is present and inert says why | one bool, and the count still includes it | the reason is [`Owed::why`], and a navigation's is [`Unavailable`](crate::availability::Unavailable) |
//!
//! The fourth row is the one worth the module. A conformance check is only
//! worth writing if it fails when the product stops matching — and there, the
//! most natural place to write one is against the model, where it cannot see
//! the difference a person is looking at.
//!
//! # Examples
//!
//! ```
//! use pinion_core::conformance::{Ledger, Owed, Part, SurfaceSpec};
//!
//! let spec = SurfaceSpec::new(vec![
//!     Part::new("time", "Time"),
//!     Part::new("severity", "Sev"),
//!     Part::new("message", "Message"),
//! ])
//! .expect("a specification is a roster of named parts");
//!
//! // The build has two of the three columns, and says so.
//! let built = [Part::new("time", "Time"), Part::new("severity", "Sev")];
//! let found = spec.diff(&built);
//! assert_eq!(
//!     found[0].sentence(),
//!     "part 2 `message` (Message) is specified and the surface has no such part",
//! );
//!
//! let ledger = Ledger::new(vec![Owed::new(
//!     "message",
//!     "part 2 `message` (Message) is specified and the surface has no such part",
//!     "R1730",
//!     "The reference wraps this column and this build has no measured row height yet.",
//! )])
//! .expect("every entry names its part, its round and its reason");
//! assert!(ledger.reconciles(&found));
//! ```

use std::borrow::Cow;
use std::collections::BTreeMap;

/// One named part of a surface — a column, a section, a group.
///
/// The same type on both sides of a comparison, because a part *is* a key and a
/// title and a place, and a specification that could describe something a build
/// cannot be is a specification nothing can conform to. Which side a value is
/// on is carried by the container: a [`SurfaceSpec`] is what must be, a
/// `&[Part]` is what is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// What the surface addresses it by — stable across renaming and
    /// reordering, which is the property the reference floor has no room for.
    pub key: Cow<'static, str>,
    /// What a reader calls it.
    pub title: Cow<'static, str>,
}

impl Part {
    /// A part with a key and a title.
    #[must_use]
    pub fn new(key: impl Into<Cow<'static, str>>, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            key: key.into(),
            title: title.into(),
        }
    }
}

/// ★★★★★ R1742 — what a build can show of ONE specified surface **right now**.
///
/// # Why a comparison needs a third answer
///
/// [`SpecDocument::report`] used to ask a screen for a surface's parts and take
/// a `Vec<Part>` back, so a screen had exactly two things it could say: *here
/// are the parts*, or *here are no parts*. Those are not the only two facts a
/// screen has.
///
/// A surface can be **specified and not on screen**. This tree's node lab is
/// the case that forced it: the inspector's rows exist once a card is selected,
/// and the roster one of them collapses exists once that row is opened — so a
/// lab nobody has touched draws none of them. Handing back an empty roster
/// there says *the build reproduces none of its specification*, which is false
/// and is the loudest kind of false: a screen that is working reports as
/// broken, and a reader learns to ignore the report. Handing back nothing at
/// all — the alternative the tree actually had — says nothing, which is how the
/// lab came to be the one section of this application that answers to a written
/// specification and never published a verdict about it.
///
/// So a screen says which of the two it is, and [`Away`](Self::Away) carries
/// **the screen's own sentence** for why. Only the screen knows whether its
/// surface is absent because a session has not opened it, because the pane it
/// lives in is collapsed, or because it is drawn for a mode nobody selected.
///
/// # It is not an escape hatch, by construction
///
/// A surface that is away **does not reconcile**
/// ([`SurfaceStanding::reconciles`]), and it counts as **0 reproduced** rather
/// than as its full specification. A screen therefore cannot report conformance
/// by keeping its surfaces off screen; it can only report that its verdict is
/// about a session nobody has put it into. That is R1738's rule one level down:
/// *the count of what was judged is part of the verdict, not a footnote under
/// it.*
///
/// # Examples
///
/// ```
/// use pinion_core::conformance::{Built, Part, SpecDocument};
///
/// let doc = SpecDocument::parse(
///     r#"{ "roster": { "canon": [ { "key": "one", "title": "One" } ], "owed": [] } }"#,
/// )
/// .expect("the fixture is a specification");
///
/// // Nobody opened it, so nothing is compared — and the report says so rather
/// // than crediting the surface or accusing it.
/// let shut = doc.report(&|_| Built::away("the roster is shut, so it has no parts"));
/// assert_eq!(shut.specified(), 1);
/// assert_eq!(shut.reproduced(), 0);
/// assert_eq!(shut.away(), 1);
/// assert!(!shut.reconciles());
///
/// let open = doc.report(&|_| Built::Standing(vec![Part::new("one", "One")]));
/// assert_eq!(open.reproduced(), 1);
/// assert!(open.reconciles());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Built {
    /// The surface is on screen, and these are the parts it is made of, in
    /// reading order.
    Standing(Vec<Part>),
    /// The surface is not on screen, so there is nothing to compare its
    /// specification with — and this is the screen's own reason.
    Away(String),
}

impl Built {
    /// A surface that is not on screen, and why.
    #[must_use]
    pub fn away(why: impl Into<String>) -> Self {
        Built::Away(why.into())
    }

    /// The parts, when the surface is standing.
    #[must_use]
    pub fn parts(&self) -> Option<&[Part]> {
        match self {
            Built::Standing(parts) => Some(parts),
            Built::Away(_) => None,
        }
    }
}

/// ★★★★★ R1758 — one specified surface's parts, **as the frame that painted it
/// has them**: everything under `stem`, in reading order, each titled by a
/// table.
///
/// # Why the framework owns this
///
/// It is the four lines every screen that judges its own paint writes. Counted
/// at the commit before this one, by the sentence a key with no table entry
/// gets: **three files** hold it — this crate's scene-side fixture, the node
/// lab's `judge` and the capture viewer's `judge` (which has two, one for each
/// titling rule) — and the round that lifted it was about to add **two more
/// screens**. Already identical down to that sentence, which is the point at
/// which a helper stops being any screen's own; the next screen writing it
/// slightly differently would make two verdicts about one build disagree for a
/// reason nobody could see from either.
///
/// `title` answers what a key is called. It is a table rather than the paint
/// because most parts of a record pane carry no label a reader sees: a
/// specification fixes that they are THERE and in what order, and how a build
/// draws them is the build's. Where the title IS drawn — a column header, a
/// roster's words — [`parts_as_read`] is the reading to take instead.
///
/// A key the paint has and the table does not gets a title saying so rather
/// than an empty string, so the difference reports as a rename with a readable
/// right-hand side instead of as a blank.
#[must_use]
pub fn parts_titled(
    regions: &crate::painted::PaintedRegions,
    stem: &str,
    title: &dyn Fn(&str) -> Option<String>,
) -> Vec<Part> {
    crate::painted::in_reading_order(regions.parts_under(stem))
        .into_iter()
        .map(|(key, _)| {
            let said =
                title(&key).unwrap_or_else(|| format!("<{key} is painted and no table names it>"));
            Part::new(key, said)
        })
        .collect()
}

/// ★ R1758 — the `title` a [`parts_titled`] call needs, from a table of
/// `(key, title)` pairs.
///
/// Four lines of closure that say nothing about any screen: find the key, hand
/// back an owned title. Lifted in the round that would otherwise have written
/// it a second and a third time — a screen's tables are its own, and looking a
/// key up in them is not.
pub fn titles_from(
    table: Vec<(&'static str, &'static str)>,
) -> impl Fn(&str) -> Option<String> + use<> {
    move |key: &str| {
        table
            .iter()
            .find(|(named, _)| *named == key)
            .map(|(_, title)| (*title).to_owned())
    }
}

/// ★★★★★ R1758 — one specified surface's parts, each titled by **the words it
/// drew**.
///
/// The reading for a surface the reference titles by what a reader sees: a
/// column header, a roster's options, a decoded field's heading. Judging those
/// by a table would let a painter label the fourth column anything at all and
/// leave the difference invisible.
///
/// Where the frame drew nothing under a part's own name the reading is said to
/// be missing rather than guessed at, because a part that draws nothing and a
/// part that draws the wrong thing are different defects and a blank would make
/// them read alike.
#[must_use]
pub fn parts_as_read(regions: &crate::painted::PaintedRegions, stem: &str) -> Vec<Part> {
    crate::painted::in_reading_order(regions.parts_under(stem))
        .into_iter()
        .map(|(key, _)| {
            let said = regions
                .reads(&format!("{stem}{key}"))
                .unwrap_or("<this part is painted and draws no words>");
            Part::new(key, said.to_owned())
        })
        .collect()
}

/// ★★★★★ R1758 — **what the built side of a verdict was read from.**
///
/// # Why a count needs this beside it
///
/// [`DocumentReport::reproduced`] answers *how many of the specified parts this
/// build has*. That number means two different things depending on where the
/// parts came from, and a reader acts on the two differently:
///
/// * read back out of the frame the screen painted, it is a statement about
///   what a person can see;
/// * taken from the screen's own tables, it is the screen agreeing with itself
///   — the specification and the answer travel together, so the comparison
///   cannot fail for the reason it exists.
///
/// R1742 wrote the second of those down as a rule (*judge from the paint; a
/// verdict read from the model is structurally consistent with the model and
/// proves nothing*) and left it as prose in one screen's header. Measured on
/// the running application at R1747 and again at R1758, standing on a page that
/// is not theirs: two of this tree's four judged sections reported **21 of 21**
/// and **15 of 15** reproduced while they had not painted a frame in that
/// session at all, beside two siblings correctly reporting `0 of 26` and
/// `0 of 15` away. Nothing was failing. The two numbers were not about pixels.
///
/// This is the sixth case of the convention R1752 named — *a number carries its
/// own qualifier* (`render_us` + `captured`, `gpu_us` + `gpu_timing_supported`,
/// `mean_render_us` + `captured_frames`, a frame timing + its adapter) — and
/// the qualifier is the half that was missing.
///
/// # What it is a claim about, exactly
///
/// **What the framework handed the screen**, not what the screen chose to look
/// at. [`SpecDocument::report_from_paint`] fetches the surface's marks out of
/// the paint store and hands them in, so [`Paint`](Self::Paint) says *this
/// verdict was computed against a recorded frame*; [`SpecDocument::report`]
/// hands the screen nothing, so [`Declaration`](Self::Declaration) says *this
/// verdict was computed against whatever the screen had*.
///
/// That distinction is worth having because it is not gameable in the direction
/// that matters. `report_from_paint` substitutes [`Built::Away`] for every
/// surface when the store holds no frame, so a report carrying `Paint` **cannot**
/// claim reproduction without a frame behind it. `Declaration` makes no such
/// promise, which is precisely why an assembled application refuses to count it
/// as conformance — see `pinion_screen`'s application report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// The parts were read back out of the frame the screen painted.
    Paint,
    /// The parts are the screen's own account of what it builds, with no frame
    /// behind them.
    Declaration,
}

impl Evidence {
    /// The word this arm is published under.
    #[must_use]
    pub const fn wire(self) -> &'static str {
        match self {
            Evidence::Paint => "paint",
            Evidence::Declaration => "declaration",
        }
    }

    /// Whether the verdict was computed against a painted frame.
    #[must_use]
    pub const fn is_paint(self) -> bool {
        matches!(self, Evidence::Paint)
    }
}

impl core::fmt::Display for Evidence {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.wire())
    }
}

/// Why a roster of parts could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceDefect {
    /// A surface with no parts. Nothing can conform to it and nothing can fail
    /// it, so accepting one would make an empty specification read as success —
    /// the failure mode a truncated or misparsed pin actually has.
    NoParts,
    /// A part whose key is empty, at this position.
    BlankKey {
        /// Where in the roster.
        at: usize,
    },
    /// Two parts answer to one key, so a difference about that key cannot be
    /// attributed to either.
    DuplicateKey {
        /// The key both claim.
        key: String,
        /// The first claimant.
        first: usize,
        /// The second.
        again: usize,
    },
}

/// One way a built surface differs from the one that was specified.
///
/// Every arm names the key it is about and both sides of the disagreement, so a
/// report reads without the specification in the other hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartDivergence {
    /// The specification has this part and the surface does not.
    Absent {
        /// The key the specification declares.
        key: String,
        /// What the specification calls it.
        title: String,
        /// Where in the specified order it belongs.
        at: usize,
    },
    /// The surface has this part and the specification does not.
    ///
    /// Reported rather than tolerated: which *direction* a difference runs in
    /// is exactly what a one-directional check cannot say, and a surface that
    /// has quietly grown a part is the drift that happens.
    Unspecified {
        /// The key the surface declares.
        key: String,
        /// Where it sits in the surface's order.
        at: usize,
    },
    /// Both have the part, in different places.
    OutOfOrder {
        /// The key.
        key: String,
        /// Where the specification puts it.
        specified_at: usize,
        /// Where the surface puts it.
        at: usize,
    },
    /// Both have the part, under different names.
    Retitled {
        /// The key.
        key: String,
        /// What the specification calls it.
        specified: String,
        /// What the surface calls it.
        found: String,
    },
}

impl PartDivergence {
    /// The key this divergence is about.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            PartDivergence::Absent { key, .. }
            | PartDivergence::Unspecified { key, .. }
            | PartDivergence::OutOfOrder { key, .. }
            | PartDivergence::Retitled { key, .. } => key,
        }
    }

    /// The divergence as one sentence, for a report a person reads.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            PartDivergence::Absent { key, title, at } => {
                format!("part {at} `{key}` ({title}) is specified and the surface has no such part")
            }
            PartDivergence::Unspecified { key, at } => {
                format!("part {at} `{key}` is on the surface and no specification declares it")
            }
            PartDivergence::OutOfOrder {
                key,
                specified_at,
                at,
            } => format!("`{key}` is specified at part {specified_at} and sits at part {at}"),
            PartDivergence::Retitled {
                key,
                specified,
                found,
            } => format!("`{key}` is specified as \"{specified}\" and reads \"{found}\""),
        }
    }
}

/// ★★★★★ R1730 — **a surface's parts, written down, in order.**
///
/// See the module documentation for what this exists for and what the reference
/// floor answers. The short of it: the parts of a surface are an ordered roster
/// of keyed, titled things, and until this type there was no way to say what
/// that roster is supposed to be — so no way for anything to fail when it stops
/// being that.
///
/// # Examples
///
/// ```
/// use pinion_core::conformance::{Part, SurfaceSpec};
///
/// let spec = SurfaceSpec::new(vec![Part::new("id", "ID"), Part::new("name", "Name")])
///     .expect("a specification is a roster of named parts");
///
/// // Reordered by the reader: the keys are both still there, and the order is not.
/// let moved = [Part::new("name", "Name"), Part::new("id", "ID")];
/// let sentences: Vec<String> = spec.diff(&moved).iter().map(PartDivergence::sentence).collect();
/// assert_eq!(
///     sentences,
///     [
///         "`id` is specified at part 0 and sits at part 1",
///         "`name` is specified at part 1 and sits at part 0",
///     ],
/// );
/// # use pinion_core::conformance::PartDivergence;
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceSpec {
    parts: Vec<Part>,
}

impl SurfaceSpec {
    /// Write down a surface, refusing one no build could reproduce.
    ///
    /// The defects are checked on the *specification* side as well as the built
    /// side, because a specification that declares one key twice cannot be
    /// conformed to and a reader who discovered that only through a confusing
    /// difference would blame the build.
    ///
    /// # Errors
    ///
    /// [`SurfaceDefect`] — no parts, a blank key, or two parts sharing a key.
    pub fn new(parts: Vec<Part>) -> Result<Self, SurfaceDefect> {
        if parts.is_empty() {
            return Err(SurfaceDefect::NoParts);
        }
        for (at, part) in parts.iter().enumerate() {
            if part.key.is_empty() {
                return Err(SurfaceDefect::BlankKey { at });
            }
            if let Some(first) = parts[..at].iter().position(|p| p.key == part.key) {
                return Err(SurfaceDefect::DuplicateKey {
                    key: part.key.clone().into_owned(),
                    first,
                    again: at,
                });
            }
        }
        Ok(Self { parts })
    }

    /// The parts, in the order the specification draws them.
    #[must_use]
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// How many parts the specification declares.
    #[must_use]
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    /// Whether the specification declares no parts. Never true of a
    /// [`SurfaceSpec`] that exists — [`new`](Self::new) refuses one — and
    /// present because a length without it reads as an invitation to compare
    /// against zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Every way `built` differs from this specification, in both directions.
    ///
    /// Ordered by the specification's parts first, then by the surface's, so a
    /// report reads in the order a person looks along the surface. A part that
    /// diverges in more than one way reports each way once: *renamed and moved*
    /// is two facts and a reader fixing one still needs the other.
    #[must_use]
    pub fn diff(&self, built: &[Part]) -> Vec<PartDivergence> {
        let mut out = Vec::new();
        for (specified_at, part) in self.parts.iter().enumerate() {
            let Some(at) = built.iter().position(|b| b.key == part.key) else {
                out.push(PartDivergence::Absent {
                    key: part.key.clone().into_owned(),
                    title: part.title.clone().into_owned(),
                    at: specified_at,
                });
                continue;
            };
            if at != specified_at {
                out.push(PartDivergence::OutOfOrder {
                    key: part.key.clone().into_owned(),
                    specified_at,
                    at,
                });
            }
            if built[at].title != part.title {
                out.push(PartDivergence::Retitled {
                    key: part.key.clone().into_owned(),
                    specified: part.title.clone().into_owned(),
                    found: built[at].title.clone().into_owned(),
                });
            }
        }
        for (at, part) in built.iter().enumerate() {
            if !self.parts.iter().any(|p| p.key == part.key) {
                out.push(PartDivergence::Unspecified {
                    key: part.key.clone().into_owned(),
                    at,
                });
            }
        }
        out
    }

    /// Whether `built` reproduces this specification exactly.
    #[must_use]
    pub fn conforms(&self, built: &[Part]) -> bool {
        self.diff(built).is_empty()
    }
}

/// ★★★★★ R1730 — **what a difference has to be able to say to be judged.**
///
/// One trait over the divergence types of the axes that have a written
/// specification, so the [`Ledger`] below is written once rather than once per
/// axis. It is deliberately two methods: a key, so an entry can be attributed,
/// and a sentence, so the ledger's own entries can be read by a person who does
/// not have the type in front of them.
pub trait Divergent {
    /// The key the difference is about.
    fn key(&self) -> &str;
    /// The difference as one sentence.
    fn sentence(&self) -> String;
}

impl Divergent for PartDivergence {
    fn key(&self) -> &str {
        PartDivergence::key(self)
    }

    fn sentence(&self) -> String {
        PartDivergence::sentence(self)
    }
}

impl Divergent for crate::widgets::destination::Divergence {
    fn key(&self) -> &str {
        crate::widgets::destination::Divergence::key(self)
    }

    fn sentence(&self) -> String {
        crate::widgets::destination::Divergence::sentence(self)
    }
}

/// One accepted difference between what was specified and what was built.
///
/// The sentence is stored **verbatim**, and that is the ratchet rather than an
/// inconvenience: any change to the built surface, and any change to how the
/// framework words a difference, makes the judgement fail until somebody reads
/// the ledger and decides what the new sentence should be. A looser match — the
/// key alone, or a substring — would let a part drift from *absent* to
/// *renamed* without a word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owed {
    /// The part or seat it is about.
    pub key: String,
    /// The divergence verbatim.
    pub sentence: String,
    /// The round that accepted it.
    pub since: String,
    /// Why it is accepted.
    pub why: String,
    /// ★★★★★ R1770 — the surface extents this entry is a claim about, or empty
    /// for a difference that is not a function of the surface's size.
    ///
    /// # Why a list of measured sizes and not a band
    ///
    /// The obvious shape is *holds at every extent no taller than H*, and it is
    /// the wrong one: it is a claim about infinitely many sizes derived from a
    /// measurement at one, which is precisely the error R1656 and R1764 each
    /// paid for. Every extent in this list was *stood at and read*.
    ///
    /// It is strict in the safe direction. At an extent this list does not
    /// name, the entry excuses nothing — so a build that still diverges there
    /// reports the difference as undeclared and the gate goes red, and the only
    /// way to quiet it is to go and measure that size too. Narrowing the list
    /// can never hide a divergence; it can only stop the entry claiming a size
    /// nobody read it at.
    pub at: Vec<crate::painted::Extent>,
}

impl Owed {
    /// Write down an accepted difference.
    #[must_use]
    pub fn new(
        key: impl Into<String>,
        sentence: impl Into<String>,
        since: impl Into<String>,
        why: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            sentence: sentence.into(),
            since: since.into(),
            why: why.into(),
            at: Vec::new(),
        }
    }

    /// ★ R1770 — narrow this entry to the extents it was measured at.
    ///
    /// See [`at`](Self::at) for why the list is measurements rather than a
    /// band, and why narrowing it can only make a gate stricter.
    #[must_use]
    pub fn only_at(mut self, at: Vec<crate::painted::Extent>) -> Self {
        self.at = at;
        self
    }

    /// Whether this entry is a claim about a verdict read at `at`.
    ///
    /// An entry that names no extent claims every one of them, which is what
    /// every entry written before R1770 means and what an entry about a
    /// difference the window cannot move should keep meaning.
    #[must_use]
    pub fn in_force_at(&self, at: Option<crate::painted::Extent>) -> bool {
        self.at.is_empty() || at.is_some_and(|read| self.at.contains(&read))
    }
}

/// Why a ledger could not be built.
///
/// Every arm is a defect in the written pin rather than a state a running
/// screen can reach, which is why [`Ledger::new`] refuses rather than warns: a
/// ledger that failed to parse and came out empty would read as *this build
/// diverges nowhere*, the most flattering possible lie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerDefect {
    /// An entry whose sentence does not name its own key, so the entry and the
    /// difference it is supposed to excuse cannot be shown to be about the same
    /// thing.
    SentenceDoesNotNameKey {
        /// The key the entry claims.
        key: String,
        /// The sentence it carries.
        sentence: String,
    },
    /// An entry that names no round, so nobody can find the decision.
    NoRound {
        /// The key the entry claims.
        key: String,
        /// What it gave instead.
        since: String,
    },
    /// An entry that states no reason. An exception list nobody has to justify
    /// becomes a list of everything.
    NoReason {
        /// The key the entry claims.
        key: String,
    },
    /// Two entries about one key, so which one excuses a difference is
    /// ambiguous.
    DuplicateKey {
        /// The key both claim.
        key: String,
    },
    /// The pin is not shaped like a ledger — a missing array, an entry missing
    /// a field, a field of the wrong type.
    Malformed {
        /// What was expected, and where.
        what: String,
    },
}

/// One way the difference a build actually has is not the difference its ledger
/// declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unreconciled {
    /// A difference nobody wrote down.
    Undeclared {
        /// The key it is about.
        key: String,
        /// The difference verbatim.
        sentence: String,
    },
    /// ★★★★★ A difference the ledger declares and the build no longer has.
    ///
    /// The arm a containment check cannot have, and the reason
    /// [`Ledger::judge`] is an equality. Somebody fixed it and did not say so,
    /// and the ledger now describes a worse build than the one that exists —
    /// which is how a remainder list stops being read.
    Paid {
        /// The key it is about.
        key: String,
        /// What the ledger still claims.
        sentence: String,
    },
    /// Both have a difference about this key and they are not the same
    /// difference.
    Reworded {
        /// The key.
        key: String,
        /// What the ledger declares.
        declared: String,
        /// What the build actually produces.
        found: String,
    },
    /// ★★★★★ R1770 — the entry holds only at extents it names, and the verdict
    /// being judged does not say what extent it was read at.
    ///
    /// The arm that keeps [`Owed::at`] from being an escape hatch. Without it,
    /// a reader that hands no extent would find every sized entry out of force
    /// and every declared difference quietly excused — the most flattering
    /// possible reading of *I did not say where I was standing*. It is the same
    /// one-directional rule [`Evidence`] carries: a claim that cannot name its
    /// own basis is refused rather than believed.
    Unsized {
        /// The key the entry claims.
        key: String,
        /// The extents the entry was measured at, as the pin writes them.
        declared_at: Vec<crate::painted::Extent>,
    },
}

impl Unreconciled {
    /// The key this is about.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Unreconciled::Undeclared { key, .. }
            | Unreconciled::Paid { key, .. }
            | Unreconciled::Reworded { key, .. }
            | Unreconciled::Unsized { key, .. } => key,
        }
    }

    /// The mismatch as one sentence, ending in what to do about it.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Unreconciled::Undeclared { sentence, .. } => {
                format!("{sentence} — and no entry declares it")
            }
            Unreconciled::Paid { key, sentence } => format!(
                "`{key}` is declared as \"{sentence}\" and the build no longer diverges there \
                 — record it as paid"
            ),
            Unreconciled::Reworded {
                key,
                declared,
                found,
            } => format!("`{key}` is declared as \"{declared}\" and now reads \"{found}\""),
            Unreconciled::Unsized { key, declared_at } => {
                let sizes = declared_at
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "`{key}` is declared only at {sizes} and this verdict does not say what \
                     extent it was read at"
                )
            }
        }
    }
}

/// ★★★★★ R1730 — **the declared, reviewed remainder, as a value a gate reads.**
///
/// A build that reproduces a specification completely needs no ledger. Every
/// real one has a remainder, and the question is whether that remainder is a
/// list somebody keeps or a feeling somebody has. This is the list, and
/// [`judge`](Self::judge) is what makes keeping it compulsory in both
/// directions.
///
/// # Where it is written
///
/// Not beside the code it judges. A specification and a remainder written in
/// the same file, in the same edit, by the same hand as the surface they are
/// about is a gate asking the subject for the answer — so the expected home is
/// a tracked document loaded through [`from_json`](Self::from_json), reviewed
/// as a claim rather than as code.
///
/// # Examples
///
/// ```
/// use pinion_core::conformance::{Ledger, Owed, Part, SurfaceSpec, Unreconciled};
///
/// let spec = SurfaceSpec::new(vec![Part::new("id", "ID"), Part::new("rate", "Msg/s")])
///     .expect("a specification is a roster of named parts");
/// let ledger = Ledger::new(vec![Owed::new(
///     "rate",
///     "part 1 `rate` (Msg/s) is specified and the surface has no such part",
///     "R1730",
///     "The rate needs a sampling window this build does not keep.",
/// )])
/// .expect("the entry names its part, its round and its reason");
///
/// // Still missing: the ledger and the build agree.
/// assert!(ledger.reconciles(&spec.diff(&[Part::new("id", "ID")])));
///
/// // Built, and nobody told the ledger.
/// let now = spec.diff(&[Part::new("id", "ID"), Part::new("rate", "Msg/s")]);
/// assert!(now.is_empty());
/// assert!(matches!(ledger.judge(&now)[0], Unreconciled::Paid { .. }));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    owed: Vec<Owed>,
}

impl Ledger {
    /// Write down a remainder, refusing entries that cannot be checked.
    ///
    /// The three per-entry conditions were assertions inside one application's
    /// test before this type existed. They are here so that every specification
    /// written from now on gets them without remembering to: an entry whose
    /// sentence does not name its key cannot be attributed, an entry with no
    /// round cannot be traced to a decision, and an entry with no reason is an
    /// exception nobody had to justify.
    ///
    /// # Errors
    ///
    /// [`LedgerDefect`] — a sentence that does not name its key, a missing
    /// round, a missing reason, or two entries about one key.
    pub fn new(owed: Vec<Owed>) -> Result<Self, LedgerDefect> {
        for (at, entry) in owed.iter().enumerate() {
            if !entry.sentence.contains(&format!("`{}`", entry.key)) {
                return Err(LedgerDefect::SentenceDoesNotNameKey {
                    key: entry.key.clone(),
                    sentence: entry.sentence.clone(),
                });
            }
            if !(entry.since.starts_with('R') && entry.since.len() >= 3) {
                return Err(LedgerDefect::NoRound {
                    key: entry.key.clone(),
                    since: entry.since.clone(),
                });
            }
            if entry.why.trim().len() <= 40 {
                return Err(LedgerDefect::NoReason {
                    key: entry.key.clone(),
                });
            }
            if owed[..at].iter().any(|e| e.key == entry.key) {
                return Err(LedgerDefect::DuplicateKey {
                    key: entry.key.clone(),
                });
            }
        }
        Ok(Self { owed })
    }

    /// Read a remainder out of the `owed` array of a specification document.
    ///
    /// The shape is an array of objects carrying `key`, `sentence`, `since` and
    /// `why`, where `why` is either a string or an array of lines — the second
    /// because a reason worth writing rarely fits on one line and a document a
    /// person reviews should not have to.
    ///
    /// # Errors
    ///
    /// [`LedgerDefect::Malformed`] for a document that is not that shape, and
    /// the rest as [`new`](Self::new).
    pub fn from_json(doc: &serde_json::Value) -> Result<Self, LedgerDefect> {
        let Some(entries) = doc.get("owed").and_then(serde_json::Value::as_array) else {
            return Err(LedgerDefect::Malformed {
                what: "the document declares an `owed` array".to_owned(),
            });
        };
        let mut owed = Vec::with_capacity(entries.len());
        for (at, entry) in entries.iter().enumerate() {
            let field = |name: &str| -> Result<String, LedgerDefect> {
                entry
                    .get(name)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| LedgerDefect::Malformed {
                        what: format!("owed entry {at} carries a string `{name}`"),
                    })
            };
            let why = match entry.get("why") {
                Some(serde_json::Value::String(text)) => text.clone(),
                Some(serde_json::Value::Array(lines)) => lines
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => {
                    return Err(LedgerDefect::Malformed {
                        what: format!("owed entry {at} carries `why` as a string or an array"),
                    });
                }
            };
            // ★ R1770 — the extents this entry was measured at. Absent means
            // *every* extent, which is what every entry written before that
            // round means; present and empty is refused, because an entry that
            // claims no size at all excuses nothing anywhere and is a mistake
            // rather than a statement.
            let at = match entry.get("at") {
                None => Vec::new(),
                Some(serde_json::Value::Array(sizes)) if !sizes.is_empty() => {
                    let mut read = Vec::with_capacity(sizes.len());
                    for size in sizes {
                        let side = |name: &str| {
                            size.get(name)
                                .and_then(serde_json::Value::as_u64)
                                .and_then(|n| u32::try_from(n).ok())
                        };
                        let (Some(width), Some(height)) = (side("width"), side("height")) else {
                            return Err(LedgerDefect::Malformed {
                                what: format!(
                                    "owed entry {at} carries `at` as sizes of `width` and `height`"
                                ),
                            });
                        };
                        read.push(crate::painted::Extent::new(width, height));
                    }
                    read
                }
                Some(_) => {
                    return Err(LedgerDefect::Malformed {
                        what: format!(
                            "owed entry {at} carries `at` as a non-empty array of sizes, or omits it"
                        ),
                    });
                }
            };
            owed.push(Owed {
                key: field("key")?,
                sentence: field("sentence")?,
                since: field("since")?,
                why,
                at,
            });
        }
        Self::new(owed)
    }

    /// The entries, in the order the document writes them.
    #[must_use]
    pub fn owed(&self) -> &[Owed] {
        &self.owed
    }

    /// How many differences the ledger accepts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.owed.len()
    }

    /// Whether the ledger accepts none — the state a fully reproduced surface
    /// is in.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.owed.is_empty()
    }

    /// ★★★★★ Every way `found` is not what this ledger declares, in **both**
    /// directions.
    ///
    /// Reported per key rather than as a whole-list inequality, because "the
    /// difference is not the declared difference" is not something a person can
    /// act on and "`rate` is declared and the build no longer diverges there"
    /// is.
    ///
    /// A key the build diverges on more than once is compared against the
    /// ledger's entry for that key in order: the ledger holds one entry per key
    /// ([`new`](Self::new) refuses two), so the second and later differences on
    /// one key are undeclared, which is the honest reading — the entry excuses
    /// the difference it quotes and not a family of them.
    #[must_use]
    pub fn judge<D: Divergent>(&self, found: &[D]) -> Vec<Unreconciled> {
        self.judge_at(None, found)
    }

    /// ★★★★★ R1770 — the same judgement, told **what extent the verdict was
    /// read at**.
    ///
    /// An entry that names extents ([`Owed::at`]) is a claim about those and
    /// nowhere else, so at any other extent it neither excuses a difference nor
    /// is owed one. That is the whole repair: measured at R1767, one entry of
    /// this tree's analysis tool declares a row that falls below the fold, and
    /// the fold is a function of how tall the surface is — so at a taller
    /// window the difference is gone and the ledger, judged without an extent,
    /// reported it [`Paid`](Unreconciled::Paid) and demanded the entry be
    /// deleted. Deleting it would have made the same tool fail at the smaller
    /// size instead. The entry was never wrong; it was never told where it
    /// applies.
    ///
    /// # What this cannot be used to hide
    ///
    /// A verdict of unknown extent against a sized entry is
    /// [`Unsized`](Unreconciled::Unsized) rather than silence, and a difference
    /// found at an extent no entry names is
    /// [`Undeclared`](Unreconciled::Undeclared) as it always was. So narrowing
    /// an entry's extents can only ever make more gates fail, never fewer.
    #[must_use]
    pub fn judge_at<D: Divergent>(
        &self,
        at: Option<crate::painted::Extent>,
        found: &[D],
    ) -> Vec<Unreconciled> {
        let in_force: Vec<bool> = self.owed.iter().map(|e| e.in_force_at(at)).collect();
        let mut out = Vec::new();
        let mut matched = vec![false; self.owed.len()];
        for difference in found {
            let sentence = difference.sentence();
            let entry = self
                .owed
                .iter()
                .enumerate()
                .find(|(at, e)| !matched[*at] && in_force[*at] && e.key == difference.key());
            match entry {
                Some((at, e)) if e.sentence == sentence => matched[at] = true,
                Some((at, e)) => {
                    matched[at] = true;
                    out.push(Unreconciled::Reworded {
                        key: e.key.clone(),
                        declared: e.sentence.clone(),
                        found: sentence,
                    });
                }
                None => out.push(Unreconciled::Undeclared {
                    key: difference.key().to_owned(),
                    sentence,
                }),
            }
        }
        for (index, entry) in self.owed.iter().enumerate() {
            if matched[index] {
                continue;
            }
            if in_force[index] {
                out.push(Unreconciled::Paid {
                    key: entry.key.clone(),
                    sentence: entry.sentence.clone(),
                });
            } else if at.is_none() {
                // Out of force only because the reader said nothing. Refusing
                // is what keeps `at` from being a way to be excused everywhere.
                out.push(Unreconciled::Unsized {
                    key: entry.key.clone(),
                    declared_at: entry.at.clone(),
                });
            }
        }
        out
    }

    /// Whether `found` is exactly what this ledger declares.
    #[must_use]
    pub fn reconciles<D: Divergent>(&self, found: &[D]) -> bool {
        self.judge(found).is_empty()
    }

    /// ★ R1770 — whether `found`, read at `at`, is exactly what this ledger
    /// declares for that extent.
    #[must_use]
    pub fn reconciles_at<D: Divergent>(
        &self,
        at: Option<crate::painted::Extent>,
        found: &[D],
    ) -> bool {
        self.judge_at(at, found).is_empty()
    }
}

// --- A whole written specification -------------------------------------------

/// Why a specification document could not be read.
///
/// Every arm is a defect in the written pin. They stop the build rather than
/// warning, for the reason [`Ledger::new`] refuses rather than warns: a document
/// that failed to parse and came out empty would read as *this build reproduces
/// everything*.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecDefect {
    /// The text is not JSON.
    NotJson {
        /// What the parser said.
        why: String,
    },
    /// The document declares no surface at all, so nothing can fail against it.
    NoSurfaces,
    /// One surface's `canon` is not a roster this vocabulary can hold.
    Surface {
        /// The surface it is about.
        surface: String,
        /// What is wrong with it.
        defect: SurfaceDefect,
    },
    /// One surface's `owed` is not a ledger.
    Ledger {
        /// The surface it is about.
        surface: String,
        /// What is wrong with it.
        defect: LedgerDefect,
    },
    /// The document is not shaped like one — a surface that is not an object, a
    /// missing `canon`, a part missing a key or a title.
    Malformed {
        /// What was expected, and where.
        what: String,
    },
}

/// ★★★★★ R1731 — **a whole written specification: several surfaces, each with
/// its canon and its declared remainder.**
///
/// # What forced this
///
/// R1730 wrote the loader, the per-surface lookup and the wire report inside the
/// first screen that needed them. The second screen needed the same four
/// functions over a different document, and two mechanical copies of a rule is
/// this project's lift trigger — with the usual sharper reason behind it: two
/// screens loading a specification differently would disagree about the same
/// build, and the one nobody ran would be the one that was wrong.
///
/// # The shape on disk
///
/// One object per surface, keyed by the name the specification gives it, each
/// carrying `canon` (an ordered array of `{key, title}`) and `owed` (a
/// [`Ledger`]). Keys beginning with `$` are commentary and are skipped, so a
/// document can explain itself to the person reviewing it — which is the whole
/// reason the specification is a document rather than a constant.
///
/// # Examples
///
/// ```
/// use pinion_core::conformance::{Part, SpecDocument};
///
/// let doc = SpecDocument::parse(r#"{
///   "$comment": "what the reference draws",
///   "columns": {
///     "canon": [{"key": "id", "title": "ID"}, {"key": "name", "title": "Name"}],
///     "owed": []
///   }
/// }"#)
/// .expect("the document is a specification");
///
/// assert_eq!(doc.surfaces().collect::<Vec<_>>(), ["columns"]);
/// let built = [Part::new("id", "ID"), Part::new("name", "Name")];
/// assert!(doc.unreconciled("columns", &built).is_empty());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecDocument {
    order: Vec<String>,
    canon: BTreeMap<String, SurfaceSpec>,
    owed: BTreeMap<String, Ledger>,
    /// ★★★★★ R1770 — the surface extent this document's canon was written
    /// against, from the pin's top-level `$at`.
    written_at: Option<crate::painted::Extent>,
}

impl SpecDocument {
    /// Read a specification document.
    ///
    /// # Errors
    ///
    /// [`SpecDefect`] — unreadable JSON, no surfaces, or a surface whose canon
    /// or remainder this vocabulary refuses.
    pub fn parse(text: &str) -> Result<Self, SpecDefect> {
        let doc: serde_json::Value =
            serde_json::from_str(text).map_err(|why| SpecDefect::NotJson {
                why: why.to_string(),
            })?;
        let object = doc.as_object().ok_or_else(|| SpecDefect::Malformed {
            what: "the document is an object of surfaces".to_owned(),
        })?;
        let mut order = Vec::new();
        let mut canon = BTreeMap::new();
        let mut owed = BTreeMap::new();
        for (name, body) in object {
            if name.starts_with('$') {
                continue;
            }
            let parts = body
                .get("canon")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| SpecDefect::Malformed {
                    what: format!("surface `{name}` declares a `canon` array"),
                })?
                .iter()
                .map(|part| {
                    let field = |key: &str| {
                        part.get(key)
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                            .ok_or_else(|| SpecDefect::Malformed {
                                what: format!("every part of `{name}` carries a string `{key}`"),
                            })
                    };
                    Ok(Part::new(field("key")?, field("title")?))
                })
                .collect::<Result<Vec<_>, SpecDefect>>()?;
            canon.insert(
                name.clone(),
                SurfaceSpec::new(parts).map_err(|defect| SpecDefect::Surface {
                    surface: name.clone(),
                    defect,
                })?,
            );
            owed.insert(
                name.clone(),
                Ledger::from_json(body).map_err(|defect| SpecDefect::Ledger {
                    surface: name.clone(),
                    defect,
                })?,
            );
            order.push(name.clone());
        }
        if order.is_empty() {
            return Err(SpecDefect::NoSurfaces);
        }
        // ★ R1770 — the extent the whole document's canon was written against.
        // Under `$at` rather than a surface name because it is a fact about the
        // reading that produced the pin, not about any one surface, and the `$`
        // prefix is already this format's word for *not a surface*.
        let written_at = match object.get("$at") {
            None => None,
            Some(declared) => {
                let side = |name: &str| {
                    declared
                        .get(name)
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|n| u32::try_from(n).ok())
                };
                let (Some(width), Some(height)) = (side("width"), side("height")) else {
                    return Err(SpecDefect::Malformed {
                        what: "`$at` is a size of `width` and `height`".to_owned(),
                    });
                };
                Some(crate::painted::Extent::new(width, height))
            }
        };
        Ok(Self {
            order,
            canon,
            owed,
            written_at,
        })
    }

    /// ★★★★★ R1761 — a pinned document, or a panic naming **both** the file and
    /// what is wrong with it.
    ///
    /// # Why this is not each screen's three lines
    ///
    /// Found by the self-grep this project's rule about mechanical duplication
    /// demands, and it was not merely duplication: six sites read a pin this
    /// way and they had drifted into **two** idioms, each losing a different
    /// half of the same sentence. Three said `.expect("docs/<file> is a
    /// specification document")` — which names the file and throws the
    /// [`SpecDefect`] away — and three said `.unwrap_or_else(|e| panic!("the
    /// section specification is readable: {e:?}"))`, which keeps the defect and
    /// does not say WHICH pin. A person reading either panic is missing exactly
    /// the half the other one has.
    ///
    /// # Panics
    ///
    /// If `text` is not a specification document. That is a defect in a
    /// reviewed artifact rather than a state a running screen can reach, and it
    /// must stop the build: a pin that failed to parse and came out empty would
    /// read as *this build reproduces everything*.
    #[must_use]
    pub fn pinned(text: &str, path: &str) -> Self {
        Self::parse(text)
            .unwrap_or_else(|defect| panic!("{path} is a specification document: {defect:?}"))
    }

    /// The surfaces this document fixes, in the order it declares them.
    pub fn surfaces(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
    }

    /// ★★★★★ R1770 — the surface extent this document's canon was **written
    /// against**, or `None` for a pin that does not say.
    ///
    /// # What was measured, and why this is the root of it
    ///
    /// Counted at R1770 over this tree's twelve analyzer pins: **none** of them
    /// named a size. Meanwhile the node lab's own gate held the size it judges
    /// itself at as a private constant inside one screen's test module —
    /// `2494x1531` — while the assembled tool gives that same section a page of
    /// `1096x802` and judges it against the same pin. So two gates disagreed by
    /// a factor of five in area about what the specification means, and neither
    /// artifact said so, because the number was not in either of them.
    ///
    /// Reading it off the pin is what makes the two gates one claim. A screen
    /// that paints its own conformance frame takes the size from here rather
    /// than declaring one, so moving the pin moves the gate.
    ///
    /// # What it does NOT do
    ///
    /// It does not excuse a verdict read at another size. A report knows both
    /// numbers — this one and [`DocumentReport::at`] — and publishing the pair
    /// is the whole of it: *judged at 1096x802 against a canon written at
    /// 2494x1531* is a sentence a reader can act on, and *conforms: false* is
    /// not. Turning the difference into an away would be the escape hatch
    /// R1742 refused.
    #[must_use]
    pub const fn written_at(&self) -> Option<crate::painted::Extent> {
        self.written_at
    }

    /// One surface's canon, or `None` for a name this document does not fix.
    #[must_use]
    pub fn canon(&self, surface: &str) -> Option<&SurfaceSpec> {
        self.canon.get(surface)
    }

    /// One surface's declared remainder.
    #[must_use]
    pub fn ledger(&self, surface: &str) -> Option<&Ledger> {
        self.owed.get(surface)
    }

    /// Every way `built` is not what this document declares for `surface`, in
    /// both directions and against the remainder.
    ///
    /// An unknown surface answers a single [`Unreconciled::Undeclared`] rather
    /// than an empty vector, because "nothing is wrong" and "nobody specified
    /// this" must not read the same.
    #[must_use]
    pub fn unreconciled(&self, surface: &str, built: &[Part]) -> Vec<Unreconciled> {
        self.unreconciled_at(surface, None, built)
    }

    /// ★★★★★ R1770 — the same comparison, told **what extent the built side was
    /// read at**.
    ///
    /// The entry point a gate that has a frame should take: an accepted
    /// difference that only holds at certain surface extents
    /// ([`Owed::at`]) can only be judged by a caller that says where it stood,
    /// and one that does not is refused rather than excused. See
    /// [`Ledger::judge_at`] for the measurement.
    #[must_use]
    pub fn unreconciled_at(
        &self,
        surface: &str,
        at: Option<crate::painted::Extent>,
        built: &[Part],
    ) -> Vec<Unreconciled> {
        let (Some(canon), Some(ledger)) = (self.canon(surface), self.ledger(surface)) else {
            return vec![Unreconciled::Undeclared {
                key: surface.to_owned(),
                sentence: format!("`{surface}` is a surface no specification declares"),
            }];
        };
        ledger.judge_at(at, &canon.diff(built))
    }

    /// ★★★★★ R1738 — the whole comparison, as a value that can be **added up**.
    ///
    /// See [`DocumentReport`] for why this exists beside [`wire`](Self::wire),
    /// which now derives from it: the wire form was the only form, and a wire
    /// form is where a count goes to stop being a count.
    ///
    /// ★ R1742 — `built` answers with a [`Built`], so a screen whose surface is
    /// not on screen says *that* instead of handing back an empty roster the
    /// comparison would read as total failure. See [`Built`] for why the third
    /// answer is not an escape hatch.
    ///
    /// ★★★★★ R1758 — the report it produces is stamped
    /// [`Evidence::Declaration`], because this entry point hands the screen
    /// **nothing**: whatever the closure answers with, it did not come from the
    /// framework's record of what was painted. See [`Evidence`] for the
    /// measurement that forced the stamp, and
    /// [`report_from_paint`](Self::report_from_paint) for the entry point that
    /// earns the other one.
    #[must_use]
    pub fn report(&self, built: &dyn Fn(&str) -> Built) -> DocumentReport {
        self.report_with(Evidence::Declaration, None, built)
    }

    /// [`report`](Self::report), told where the built side came from.
    ///
    /// Private because the stamp is not a caller's to choose: it is a fact
    /// about which entry point was taken, and a public parameter would make it
    /// a claim a screen could simply assert.
    fn report_with(
        &self,
        evidence: Evidence,
        at: Option<crate::painted::Extent>,
        built: &dyn Fn(&str) -> Built,
    ) -> DocumentReport {
        DocumentReport {
            evidence,
            at,
            written_at: self.written_at,
            surfaces: self
                .surfaces()
                .map(|surface| {
                    let canon = &self.canon[surface];
                    let ledger = &self.owed[surface];
                    match built(surface) {
                        Built::Standing(parts) => {
                            let divergences = canon.diff(&parts);
                            SurfaceStanding {
                                unreconciled: ledger.judge_at(at, &divergences),
                                surface: surface.to_owned(),
                                canon: canon.parts().to_vec(),
                                away: None,
                                divergences,
                                owed: ledger.owed().to_vec(),
                                at,
                            }
                        }
                        // ★ Nothing is judged, so nothing is recorded as
                        // judged: no divergences, no ledger verdict. A surface
                        // that is away has not been compared with its ledger
                        // either, and a report that quietly reconciled one
                        // would let a screen retire a declared remainder by
                        // never drawing the surface it is about.
                        Built::Away(why) => SurfaceStanding {
                            unreconciled: Vec::new(),
                            surface: surface.to_owned(),
                            canon: canon.parts().to_vec(),
                            away: Some(why),
                            divergences: Vec::new(),
                            owed: ledger.owed().to_vec(),
                            at,
                        },
                    }
                })
                .collect(),
        }
    }

    /// ★★★★★ R1747 — the report a screen answers **from its own last painted
    /// frame**, which is the only evidence a `conformance` hook has.
    ///
    /// # Why the framework owns this and not each screen
    ///
    /// A screen judging itself has no scene: the hook is a question a host asks
    /// between frames. What it does have is
    /// [`painted_regions`](crate::painted::painted_regions), so every such
    /// screen writes the same four lines — fetch the surface's marks, and if
    /// the frame store has none, answer away.
    ///
    /// The second screen to write them wrote the away sentence **byte for byte
    /// identical** to the first, which is the point at which a sentence stops
    /// being a screen's own words. It is not one: *this surface has not painted*
    /// is a fact about the framework's store, and a screen has no business
    /// having an opinion about how to say it. A third screen phrasing it
    /// slightly differently is the drift this lift exists to prevent, and the
    /// distinction it would blur is load-bearing — **a screen that has not
    /// painted has not been asked to draw anything yet, which is a different
    /// fact from a screen that drew none of what it should**, and a reader acts
    /// on the two differently.
    ///
    /// `built` is handed the marks, so a screen writes only its own readings.
    #[must_use]
    pub fn report_from_paint(
        &self,
        surface_tag: &str,
        built: &dyn Fn(&crate::painted::PaintedRegions, &str) -> Built,
    ) -> DocumentReport {
        let regions = crate::painted::painted_regions(surface_tag);
        // ★ R1770 — the extent comes off the STORE, never off the caller. A
        // screen cannot claim to have been read at a size it did not paint at,
        // for the same reason it cannot claim `Evidence::Paint` without a
        // frame: the fact and its qualifier come from one place.
        let at = regions
            .as_deref()
            .and_then(crate::painted::PaintedRegions::extent);
        self.report_with(Evidence::Paint, at, &|surface| match regions.as_deref() {
            Some(regions) => built(regions, surface),
            None => {
                Built::away("this screen has not painted a frame yet, so none of it is on screen")
            }
        })
    }

    /// The whole comparison, as the value a running application publishes.
    ///
    /// One shape rather than one per screen, because an agent asking two
    /// screens how much of their specification they are should not have to read
    /// two answers.
    ///
    /// ★ R1738 — derived from [`report`](Self::report) rather than written a
    /// second time. The two were one expression from the round this shape was
    /// introduced; keeping them so is what stops the typed count and the
    /// published count from disagreeing about the same build.
    #[must_use]
    pub fn wire(&self, built: &dyn Fn(&str) -> Built) -> serde_json::Value {
        self.report(built).to_json()
    }

    /// ★★★★★ R1747 — **the document itself, as a section publishes it**: each
    /// surface's canon and the remainder this build declares against it.
    ///
    /// Not to be confused with [`wire`](Self::wire), which publishes what a
    /// BUILD reproduces. This publishes what the SPECIFICATION declares, so a
    /// client can ask a running section *what is your verdict about* without
    /// reading this repository — which is the question a report of counts
    /// cannot answer, because a `SurfaceStanding` carries how many parts were
    /// specified and not which.
    ///
    /// # Why the framework owns the shape
    ///
    /// The node lab wrote this by hand at R1732 and the capture viewer needed
    /// it verbatim at R1747. Two sections publishing one document in two
    /// shapes is precisely the defect R1738 exists to prevent one level up: a
    /// client that walks a section's published specification must not have to
    /// know which section it is talking to. It also removes the reason the
    /// second one would have been written slightly differently — the `owed`
    /// half travels with the canon, because *this part is not there and here
    /// is why* is the thing a reader cannot get any other way, and it is easy
    /// to leave out when copying.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for surface in self.surfaces() {
            let canon = self.canon[surface].parts();
            let ledger = &self.owed[surface];
            out.insert(
                surface.to_owned(),
                serde_json::json!({
                    "canon": canon
                        .iter()
                        .map(|part| serde_json::json!({ "key": part.key, "title": part.title }))
                        .collect::<Vec<_>>(),
                    "owed": ledger
                        .owed()
                        .iter()
                        .map(|entry| serde_json::json!({
                            "key": entry.key,
                            "says": entry.sentence,
                            "since": entry.since,
                            "why": entry.why,
                            "at": extents_json(&entry.at),
                        }))
                        .collect::<Vec<_>>(),
                }),
            );
        }
        serde_json::Value::Object(out)
    }
}

/// ★★★★★ R1738 — how much of ONE named surface a build reproduces.
///
/// The typed half of what [`SpecDocument::wire`] publishes, and it exists
/// because the wire form used to be the *only* form. An application assembled
/// out of several specified sections could serialise each section's comparison
/// and could not add two of them together, so the question a reader actually
/// has — *how much of this application has been judged at all* — had nowhere to
/// be computed and therefore was not.
///
/// Measured on this tree's own analysis tool the round this type was written:
/// six open sections, **two** of them publishing a verdict about their own
/// surfaces, and the application's headline reading `specified 8, reproduced 8`
/// — a count of navigation seats that a reader had every reason to take for a
/// count of the tool.
///
/// # Examples
///
/// ```
/// use pinion_core::conformance::{Built, Part, SpecDocument};
///
/// let doc = SpecDocument::parse(
///     r#"{ "columns": {
///           "canon": [
///             { "key": "id", "title": "ID" },
///             { "key": "name", "title": "Name" }
///           ],
///           "owed": []
///        } }"#,
/// )
/// .expect("the fixture is a specification");
///
/// let report = doc.report(&|_| Built::Standing(vec![Part::new("id", "ID")]));
/// assert_eq!(report.specified(), 2);
/// assert_eq!(report.reproduced(), 1);
/// assert!(!report.reconciles());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceStanding {
    surface: String,
    /// ★★★★★ R1758 — the parts the specification fixes, and not merely how
    /// many of them there are.
    ///
    /// A verdict saying *seven were specified* cannot be checked by anybody: a
    /// reader holding it has no way to ask **which** seven, so a build that
    /// renamed one and a build that reordered two are indistinguishable from
    /// outside. Measured at R1758 — R1738's own gate could not read a section's
    /// canon out of the verdict, so it went looking for it in whatever the
    /// section published *beside* the verdict and had to identify the right
    /// document by a rule ("the one holding every surface") that R1747 had
    /// already had to repair once, after a screen's own table and its
    /// specification collided on the word `context`.
    ///
    /// Carrying it here removes the search. It is the specification's side of
    /// the comparison, so it is the same whether or not the surface is on
    /// screen.
    canon: Vec<Part>,
    /// `None` while the surface is on screen; the screen's own reason for it
    /// not being, otherwise.
    ///
    /// The invariant that makes the arms readable: while this is `Some`,
    /// `divergences` and `unreconciled` are **empty** — nothing was compared,
    /// so nothing can have diverged. Only [`SpecDocument::report`] constructs
    /// this type, which is what enforces it.
    away: Option<String>,
    divergences: Vec<PartDivergence>,
    unreconciled: Vec<Unreconciled>,
    owed: Vec<Owed>,
    /// ★ R1770 — the extent this verdict was read at, or `None` for one read
    /// from a declaration rather than a frame.
    at: Option<crate::painted::Extent>,
}

impl SurfaceStanding {
    /// The surface this is about.
    #[must_use]
    pub fn surface(&self) -> &str {
        &self.surface
    }

    /// ★★★★★ R1770 — the extent this verdict was read at.
    ///
    /// `None` for a verdict read from a declaration, which has no frame and
    /// therefore no size. See [`crate::painted::Extent`] for the measurement
    /// that made this compulsory: the same binary, walked twice with only the
    /// window moved, failing at two disjoint sets of surfaces.
    #[must_use]
    pub const fn at(&self) -> Option<crate::painted::Extent> {
        self.at
    }

    /// How many parts the specification fixes.
    ///
    /// The specification's count, so it is the same whether or not the surface
    /// is on screen: what a surface is *supposed* to be made of does not depend
    /// on whether anybody opened it.
    #[must_use]
    pub fn specified(&self) -> usize {
        self.canon.len()
    }

    /// ★★★★★ R1758 — **which** parts the specification fixes, in its order.
    ///
    /// The half [`specified`](Self::specified) cannot carry. See the field for
    /// the gate that had to go looking for this elsewhere, and what it had to
    /// guess to find it.
    #[must_use]
    pub fn canon(&self) -> &[Part] {
        &self.canon
    }

    /// ★ R1742 — whether the surface was on screen to be compared at all.
    #[must_use]
    pub const fn is_standing(&self) -> bool {
        self.away.is_none()
    }

    /// Why the surface was not on screen, in the screen's own words, or `None`
    /// when it was.
    #[must_use]
    pub fn why(&self) -> Option<&str> {
        self.away.as_deref()
    }

    /// How many of the specified parts the build has, in the place the
    /// specification puts them.
    ///
    /// ★ **Zero while the surface is away**, and that is the decision rather
    /// than an omission: crediting a surface nobody opened is the direction of
    /// error that inflates a report silently.
    ///
    /// ★★★★★ R1742 — **the specified parts that diverge, counted once each.**
    /// It was `specified - divergences.len()`, and that is wrong in two
    /// directions that had never met a build unequal enough to show either:
    ///
    /// * A part the surface has and the specification does not
    ///   ([`PartDivergence::Unspecified`]) is not a specified part failing to
    ///   be there, so subtracting it makes a build that grew a part report as
    ///   having *lost* one.
    /// * One part can diverge twice — *renamed and moved* is two facts
    ///   [`SurfaceSpec::diff`] reports separately, on purpose — so it was
    ///   subtracted twice.
    ///
    /// Together they can take the count below zero, and did: measured the first
    /// time a screen answered this from a live frame, on a surface fixing
    /// **five** parts whose session produced **six** divergences — `5 - 6`
    /// panicked in a debug build. The value a running application publishes must
    /// not be able to do that, and the arithmetic that could was the same
    /// arithmetic that quietly under-reported every surface with a declared
    /// extra.
    #[must_use]
    pub fn reproduced(&self) -> usize {
        if self.away.is_some() {
            return 0;
        }
        let troubled: std::collections::BTreeSet<&str> = self
            .divergences
            .iter()
            .filter(|d| !matches!(d, PartDivergence::Unspecified { .. }))
            .map(PartDivergence::key)
            .collect();
        self.canon.len() - troubled.len()
    }

    /// Every way the build is not what was specified.
    #[must_use]
    pub fn divergences(&self) -> &[PartDivergence] {
        &self.divergences
    }

    /// Every way the difference the build *has* is not the difference its
    /// ledger *declares*.
    #[must_use]
    pub fn unreconciled(&self) -> &[Unreconciled] {
        &self.unreconciled
    }

    /// The differences this surface's ledger accepts.
    #[must_use]
    pub fn owed(&self) -> &[Owed] {
        &self.owed
    }

    /// Whether the difference this build has is exactly the difference somebody
    /// wrote down.
    ///
    /// Not *whether it diverges nowhere*: a surface with a declared remainder
    /// is a surface somebody reviewed, and a check that demanded zero
    /// divergences would make the ledger unusable the moment it held an entry.
    /// The failing condition is a difference **nobody accepted**, in either
    /// direction — which is [`Ledger::judge`]'s equality.
    ///
    /// ★★★★★ R1742 — **and a surface that is away does not reconcile.** It is
    /// what stops [`Built::Away`] from being a way out: a screen may decline to
    /// be judged, and declining is not passing.
    #[must_use]
    pub fn reconciles(&self) -> bool {
        self.away.is_none() && self.unreconciled.is_empty()
    }

    /// This surface's row, as the value a running application publishes.
    ///
    /// ★ R1767 — public, and [`DocumentReport::surfaces_json`] is built out of
    /// it rather than the other way round. A second reader of these facts
    /// arrived that round — a walk keeps the frame each surface was last
    /// **standing** on, which is a different question from what the last frame
    /// said — and re-spelling the row there would have been two renderings of
    /// one verdict, the "one word, two documents" class R1747 spent a round on.
    /// A caller that needs to say more about a row adds keys to this value; it
    /// does not rebuild it.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut row = serde_json::json!({
            "specified": self.specified(),
            "reproduced": self.reproduced(),
            "standing": self.is_standing(),
            "canon": self
                .canon()
                .iter()
                .map(|part| serde_json::json!({ "key": part.key, "title": part.title }))
                .collect::<Vec<_>>(),
            "divergences": self
                .divergences
                .iter()
                .map(|d| serde_json::json!({ "key": d.key(), "says": d.sentence() }))
                .collect::<Vec<_>>(),
            "owed": self
                .owed
                .iter()
                .map(|entry| serde_json::json!({
                    "key": entry.key,
                    "says": entry.sentence,
                    "since": entry.since,
                    "why": entry.why,
                    // ★ R1770 — beside the reason, because a reader who cannot
                    // see WHERE an entry applies cannot tell an exception that
                    // is still true from one the window has already repaired.
                    "at": extents_json(&entry.at),
                }))
                .collect::<Vec<_>>(),
            "unreconciled": self
                .unreconciled
                .iter()
                .map(|u| serde_json::json!({ "key": u.key(), "says": u.sentence() }))
                .collect::<Vec<_>>(),
            // ★ R1770 — the size this row was read at, null for a verdict read
            // from a declaration.
            "at": self.at.map(|e| e.to_string()),
        });
        if let Some(why) = self.why() {
            row["why"] = serde_json::Value::String(why.to_owned());
        }
        row
    }
}

/// The extents an owed entry names, as the wire and the pin both spell them.
///
/// One writer for both directions of the same list, so a size read off a
/// report can be pasted into a pin.
fn extents_json(at: &[crate::painted::Extent]) -> serde_json::Value {
    serde_json::Value::Array(
        at.iter()
            .map(|e| serde_json::json!({ "width": e.width(), "height": e.height() }))
            .collect(),
    )
}

/// ★★★★★ R1738 — every surface one specification names, and how much of each
/// the build reproduces.
///
/// See [`SurfaceStanding`] for what forced a typed report. This is the value a
/// screen hands its host so an application can say how much of *itself* is
/// judged, rather than each section answering separately to whoever thought to
/// ask it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentReport {
    surfaces: Vec<SurfaceStanding>,
    evidence: Evidence,
    /// ★ R1770 — the extent every row of this report was read at.
    at: Option<crate::painted::Extent>,
    /// ★ R1770 — the extent the specification's canon was written against.
    written_at: Option<crate::painted::Extent>,
}

impl DocumentReport {
    /// Each surface, in the order the document declares them.
    #[must_use]
    pub fn surfaces(&self) -> &[SurfaceStanding] {
        &self.surfaces
    }

    /// ★★★★★ R1770 — the extent this whole report was read at.
    ///
    /// One size for every row, because one frame painted them all. `None` for
    /// a report built from a declaration, which has no frame — the same
    /// distinction [`evidence`](Self::evidence) draws, one question further
    /// along: not only *where did this come from* but *how big was it there*.
    #[must_use]
    pub const fn at(&self) -> Option<crate::painted::Extent> {
        self.at
    }

    /// ★★★★★ R1770 — the extent the specification's canon was written against.
    ///
    /// See [`SpecDocument::written_at`]. Carried on the report so the two
    /// numbers a reader must compare arrive together: a verdict read at one
    /// size against a canon written at another is not wrong, but it is not the
    /// same claim, and before this round nothing published either half.
    #[must_use]
    pub const fn written_at(&self) -> Option<crate::painted::Extent> {
        self.written_at
    }

    /// ★ R1770 — whether this verdict was read at the extent its specification
    /// was written against.
    ///
    /// `false` when either is unknown, because *the sizes agree* is a claim and
    /// a missing number cannot support one.
    #[must_use]
    pub fn read_where_written(&self) -> bool {
        matches!((self.at, self.written_at), (Some(read), Some(written)) if read == written)
    }

    /// ★★★★★ R1758 — where the built side of this verdict came from.
    ///
    /// The qualifier every count below needs beside it. See [`Evidence`].
    #[must_use]
    pub const fn evidence(&self) -> Evidence {
        self.evidence
    }

    /// How many parts this specification fixes across every surface.
    #[must_use]
    pub fn specified(&self) -> usize {
        self.surfaces.iter().map(SurfaceStanding::specified).sum()
    }

    /// How many of them the build has where they were specified.
    ///
    /// A surface that was not on screen contributes **0**, so this and
    /// [`specified`](Self::specified) come apart exactly when
    /// [`away`](Self::away) is not zero.
    #[must_use]
    pub fn reproduced(&self) -> usize {
        self.surfaces.iter().map(SurfaceStanding::reproduced).sum()
    }

    /// ★ R1742 — how many of this specification's surfaces were on screen to be
    /// compared.
    #[must_use]
    pub fn standing(&self) -> usize {
        self.surfaces.iter().filter(|s| s.is_standing()).count()
    }

    /// ★ R1742 — how many were not, and so were not judged.
    ///
    /// The number a reader needs beside [`reproduced`](Self::reproduced) to
    /// know what a low count means: a build that draws its surfaces wrongly and
    /// a session nobody opened produce the same shortfall, and only this tells
    /// them apart.
    #[must_use]
    pub fn away(&self) -> usize {
        self.surfaces.iter().filter(|s| !s.is_standing()).count()
    }

    /// Whether every surface's difference is the difference somebody wrote
    /// down.
    ///
    /// ★ R1742 — **false while any surface is away**, because an unjudged
    /// surface is not a reconciled one. See [`Built`].
    #[must_use]
    pub fn reconciles(&self) -> bool {
        self.surfaces.iter().all(SurfaceStanding::reconciles)
    }

    /// The report as the value a running application publishes.
    ///
    /// ★ R1742 — every row carries `standing`, and a row that is not standing
    /// carries `why`. The same shape a section row has one level up, for the
    /// same reason: a reader who cannot tell *not reproduced* from *not on
    /// screen* is reading two facts as one.
    ///
    /// ★★★★★ R1758 — **the whole verdict, not only its surfaces.** It was the
    /// surface map alone, which meant the one place a section published its own
    /// judgment carried neither a total nor the qualifier that says what the
    /// total is about, and a client wanting either had to be a *host* reading
    /// the application report one level up. `evidence` leads because it governs
    /// how the rest is read; `surfaces` is nested rather than merged, because a
    /// map keyed by surface name and a map of the report's own facts share a
    /// namespace only until a specification names a surface `reproduced`.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "evidence": self.evidence.wire(),
            // ★ R1770 — beside `evidence` and for its reason: it governs how
            // the rest is read. A reader comparing two of these reports without
            // it is comparing two builds that may only differ by a window.
            "at": self.at.map(|e| e.to_string()),
            "written_at": self.written_at.map(|e| e.to_string()),
            "read_where_written": self.read_where_written(),
            "specified": self.specified(),
            "reproduced": self.reproduced(),
            "standing": self.standing(),
            "away": self.away(),
            "reconciles": self.reconciles(),
            "surfaces": self.surfaces_json(),
        })
    }

    /// Each surface's own row, keyed by the name the specification gives it.
    #[must_use]
    pub fn surfaces_json(&self) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for standing in &self.surfaces {
            out.insert(standing.surface.clone(), standing.to_json());
        }
        serde_json::Value::Object(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Built, Ledger, LedgerDefect, Owed, Part, PartDivergence, SpecDefect, SpecDocument,
        SurfaceDefect, SurfaceSpec, Unreconciled,
    };
    use crate::Scene;

    fn spec() -> SurfaceSpec {
        SurfaceSpec::new(vec![
            Part::new("id", "ID"),
            Part::new("pattern", "Pattern"),
            Part::new("status", "Status"),
        ])
        .expect("the fixture is a roster of named parts")
    }

    fn sentences(found: &[PartDivergence]) -> Vec<String> {
        found.iter().map(PartDivergence::sentence).collect()
    }

    #[test]
    fn a_surface_that_reproduces_the_specification_diverges_nowhere() {
        let built = [
            Part::new("id", "ID"),
            Part::new("pattern", "Pattern"),
            Part::new("status", "Status"),
        ];
        assert!(spec().conforms(&built));
        assert!(spec().diff(&built).is_empty());
    }

    /// ★ The direction a one-sided check has: the specification has a part the
    /// surface does not.
    #[test]
    fn a_part_the_surface_lacks_is_reported_with_its_place() {
        let built = [Part::new("id", "ID"), Part::new("status", "Status")];
        assert_eq!(
            sentences(&spec().diff(&built)),
            [
                "part 1 `pattern` (Pattern) is specified and the surface has no such part",
                "`status` is specified at part 2 and sits at part 1",
            ],
        );
    }

    /// ★★★★★ The direction it does not: the surface has a part nobody specified.
    ///
    /// This is the arm that catches a screen quietly growing a column, and it
    /// is the arm the analysis tool's rail needed for several hundred rounds
    /// before its navigation peer had it.
    #[test]
    fn a_part_no_specification_declares_is_reported_too() {
        let built = [
            Part::new("id", "ID"),
            Part::new("pattern", "Pattern"),
            Part::new("status", "Status"),
            Part::new("owner", "Owner"),
        ];
        assert_eq!(
            sentences(&spec().diff(&built)),
            ["part 3 `owner` is on the surface and no specification declares it"],
        );
    }

    /// ★★★★★ The row the reference floor cannot answer: the parts are all
    /// present, and the reader is looking at a different surface.
    #[test]
    fn a_reordered_surface_diverges_even_though_every_part_is_present() {
        let built = [
            Part::new("status", "Status"),
            Part::new("pattern", "Pattern"),
            Part::new("id", "ID"),
        ];
        let found = spec().diff(&built);
        assert_eq!(
            sentences(&found),
            [
                "`id` is specified at part 0 and sits at part 2",
                "`status` is specified at part 2 and sits at part 0",
            ],
        );
        // The middle part did not move and is not reported, so a reader is not
        // handed the whole roster to re-check.
        assert!(found.iter().all(|d| d.key() != "pattern"));
    }

    #[test]
    fn a_renamed_part_is_reported_with_both_names() {
        let built = [
            Part::new("id", "ID"),
            Part::new("pattern", "Key"),
            Part::new("status", "Status"),
        ];
        assert_eq!(
            sentences(&spec().diff(&built)),
            ["`pattern` is specified as \"Pattern\" and reads \"Key\""],
        );
    }

    /// A part that is both renamed and moved reports both, because fixing one
    /// still leaves the other.
    #[test]
    fn two_differences_about_one_part_are_two_facts() {
        let built = [
            Part::new("pattern", "Key"),
            Part::new("id", "ID"),
            Part::new("status", "Status"),
        ];
        assert_eq!(
            sentences(&spec().diff(&built)),
            [
                "`id` is specified at part 0 and sits at part 1",
                "`pattern` is specified at part 1 and sits at part 0",
                "`pattern` is specified as \"Pattern\" and reads \"Key\"",
            ],
        );
    }

    #[test]
    fn a_specification_that_cannot_be_conformed_to_is_refused() {
        assert_eq!(SurfaceSpec::new(Vec::new()), Err(SurfaceDefect::NoParts));
        assert_eq!(
            SurfaceSpec::new(vec![Part::new("", "Nameless")]),
            Err(SurfaceDefect::BlankKey { at: 0 }),
        );
        assert_eq!(
            SurfaceSpec::new(vec![Part::new("id", "ID"), Part::new("id", "ID again")]),
            Err(SurfaceDefect::DuplicateKey {
                key: "id".to_owned(),
                first: 0,
                again: 1,
            }),
        );
    }

    fn ledger() -> Ledger {
        Ledger::new(vec![Owed::new(
            "pattern",
            "part 1 `pattern` (Pattern) is specified and the surface has no such part",
            "R1730",
            "The reference draws this column and this build has not built it yet.",
        )])
        .expect("the fixture entry names its part, its round and its reason")
    }

    #[test]
    fn a_declared_difference_reconciles() {
        let built = [Part::new("id", "ID"), Part::new("status", "Status")];
        let found = spec().diff(&built);
        // The reorder that follows from the absence is NOT declared, so this
        // fixture must not reconcile — the ledger declares one difference and
        // the build has two.
        assert!(!ledger().reconciles(&found));
        assert_eq!(
            ledger()
                .judge(&found)
                .iter()
                .map(Unreconciled::key)
                .collect::<Vec<_>>(),
            ["status"],
        );
    }

    /// ★★★★★ The arm a containment check cannot have.
    #[test]
    fn a_difference_that_was_paid_off_and_not_recorded_fails() {
        let built = [
            Part::new("id", "ID"),
            Part::new("pattern", "Pattern"),
            Part::new("status", "Status"),
        ];
        let found = spec().diff(&built);
        assert!(found.is_empty(), "the surface now reproduces it exactly");
        let judged = ledger().judge(&found);
        assert_eq!(
            judged
                .iter()
                .map(Unreconciled::sentence)
                .collect::<Vec<_>>(),
            [
                "`pattern` is declared as \"part 1 `pattern` (Pattern) is specified and the \
                 surface has no such part\" and the build no longer diverges there — record it \
                 as paid"
            ],
        );
    }

    /// The same key, a different difference: drifting from one kind of wrong to
    /// another must not pass on the strength of the key alone.
    #[test]
    fn a_difference_that_changed_its_wording_fails() {
        let built = [
            Part::new("id", "ID"),
            Part::new("pattern", "Key"),
            Part::new("status", "Status"),
        ];
        let judged = ledger().judge(&spec().diff(&built));
        assert!(matches!(judged[0], Unreconciled::Reworded { .. }));
        assert_eq!(judged[0].key(), "pattern");
    }

    #[test]
    fn an_entry_that_cannot_be_checked_is_refused() {
        assert_eq!(
            Ledger::new(vec![Owed::new(
                "pattern",
                "something is wrong somewhere",
                "R1730",
                "A reason long enough to be a reason rather than a shrug at it.",
            )]),
            Err(LedgerDefect::SentenceDoesNotNameKey {
                key: "pattern".to_owned(),
                sentence: "something is wrong somewhere".to_owned(),
            }),
        );
        assert_eq!(
            Ledger::new(vec![Owed::new(
                "pattern",
                "`pattern` is absent",
                "later",
                "A reason long enough to be a reason rather than a shrug at it.",
            )]),
            Err(LedgerDefect::NoRound {
                key: "pattern".to_owned(),
                since: "later".to_owned(),
            }),
        );
        assert_eq!(
            Ledger::new(vec![Owed::new(
                "pattern",
                "`pattern` is absent",
                "R1730",
                "no time",
            )]),
            Err(LedgerDefect::NoReason {
                key: "pattern".to_owned(),
            }),
        );
        assert_eq!(
            Ledger::new(vec![
                Owed::new(
                    "pattern",
                    "`pattern` is absent",
                    "R1730",
                    "A reason long enough to be a reason rather than a shrug at it.",
                ),
                Owed::new(
                    "pattern",
                    "`pattern` is also renamed",
                    "R1730",
                    "A reason long enough to be a reason rather than a shrug at it.",
                ),
            ]),
            Err(LedgerDefect::DuplicateKey {
                key: "pattern".to_owned(),
            }),
        );
    }

    /// ★ A document that failed to parse must not read as *this build diverges
    /// nowhere*, which is the most flattering possible lie.
    #[test]
    fn a_document_without_an_owed_array_is_refused() {
        let doc = serde_json::json!({ "canon": [] });
        assert!(matches!(
            Ledger::from_json(&doc),
            Err(LedgerDefect::Malformed { .. }),
        ));
    }

    #[test]
    fn a_reason_may_be_written_as_lines() {
        let doc = serde_json::json!({
            "owed": [{
                "key": "pattern",
                "sentence": "part 1 `pattern` (Pattern) is specified and the surface has no such part",
                "since": "R1730",
                "why": [
                    "The reference draws this column,",
                    "and this build has not built it yet.",
                ],
            }],
        });
        let ledger = Ledger::from_json(&doc).expect("the document is a ledger");
        assert_eq!(ledger.len(), 1);
        assert_eq!(
            ledger.owed()[0].why,
            "The reference draws this column, and this build has not built it yet.",
        );
    }

    /// ★★★★★ R1718's gate, applied to this module's two vocabularies: every
    /// arm that can put a sentence in front of a person is driven, and no two
    /// of them read the same.
    ///
    /// The second half is the one worth the test. These sentences are what a
    /// reader is handed when a surface stops matching its specification, and
    /// four ways of being wrong that all read alike would be four ways of being
    /// told nothing.
    #[test]
    fn r1730_every_way_a_surface_can_diverge_is_said_and_distinct() {
        use crate::test_fixtures::speech::assert_speaks;

        let said = [
            (
                "Absent",
                PartDivergence::Absent {
                    key: "rate".to_owned(),
                    title: "Msg/s".to_owned(),
                    at: 5,
                }
                .sentence(),
            ),
            (
                "Unspecified",
                PartDivergence::Unspecified {
                    key: "owner".to_owned(),
                    at: 7,
                }
                .sentence(),
            ),
            (
                "OutOfOrder",
                PartDivergence::OutOfOrder {
                    key: "status".to_owned(),
                    specified_at: 6,
                    at: 2,
                }
                .sentence(),
            ),
            (
                "Retitled",
                PartDivergence::Retitled {
                    key: "pattern".to_owned(),
                    specified: "Pattern".to_owned(),
                    found: "Key".to_owned(),
                }
                .sentence(),
            ),
        ];
        assert_speaks("PartDivergence", 4, &said, &[]);
        let mut distinct: Vec<&str> = said.iter().map(|(_, s)| s.as_str()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), said.len(), "two arms read the same");
    }

    /// The peer for the ledger's own vocabulary. Its three arms are three
    /// different things to do about the same key, so a reader who cannot tell
    /// them apart cannot act.
    #[test]
    fn r1730_every_way_a_ledger_can_be_wrong_is_said_and_distinct() {
        use crate::test_fixtures::speech::assert_speaks;

        let said = [
            (
                "Undeclared",
                Unreconciled::Undeclared {
                    key: "rate".to_owned(),
                    sentence: "`rate` is specified and the surface has no such part".to_owned(),
                }
                .sentence(),
            ),
            (
                "Paid",
                Unreconciled::Paid {
                    key: "rate".to_owned(),
                    sentence: "`rate` is specified and the surface has no such part".to_owned(),
                }
                .sentence(),
            ),
            (
                "Reworded",
                Unreconciled::Reworded {
                    key: "rate".to_owned(),
                    declared: "`rate` is absent".to_owned(),
                    found: "`rate` reads \"Rate\"".to_owned(),
                }
                .sentence(),
            ),
        ];
        assert_speaks("Unreconciled", 3, &said, &[]);
        let mut distinct: Vec<&str> = said.iter().map(|(_, s)| s.as_str()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), said.len(), "two arms read the same");
    }

    // --- A whole written specification ---------------------------------------

    fn document() -> super::SpecDocument {
        super::SpecDocument::parse(
            r#"{
              "$comment": "what the reference draws",
              "columns": {
                "canon": [
                  {"ordinal": 1, "key": "id", "title": "ID"},
                  {"ordinal": 2, "key": "name", "title": "Name"}
                ],
                "owed": []
              },
              "detail": {
                "canon": [
                  {"ordinal": 1, "key": "subject", "title": "Subject"},
                  {"ordinal": 2, "key": "bytes", "title": "Wire bytes"}
                ],
                "owed": [{
                  "key": "bytes",
                  "sentence": "part 1 `bytes` (Wire bytes) is specified and the surface has no such part",
                  "since": "R1731",
                  "why": ["The reference shows the frame and this build has not decoded one."]
                }]
              }
            }"#,
        )
        .expect("the fixture is a specification")
    }

    #[test]
    fn a_document_answers_its_surfaces_in_the_order_it_declares_them() {
        assert_eq!(
            document().surfaces().collect::<Vec<_>>(),
            ["columns", "detail"],
        );
        assert_eq!(document().canon("columns").expect("a surface").len(), 2);
        assert_eq!(document().ledger("detail").expect("a surface").len(), 1);
        assert!(document().canon("nothing").is_none());
    }

    /// ★ Commentary keys are skipped, because a specification a person reviews
    /// has to be able to explain itself.
    #[test]
    fn a_commentary_key_is_not_a_surface() {
        assert!(!document().surfaces().any(|s| s.starts_with('$')));
    }

    #[test]
    fn a_document_judges_a_surface_against_its_own_remainder() {
        let doc = document();
        assert!(
            doc.unreconciled(
                "columns",
                &[Part::new("id", "ID"), Part::new("name", "Name")]
            )
            .is_empty()
        );
        assert!(
            doc.unreconciled("detail", &[Part::new("subject", "Subject")])
                .is_empty(),
            "the missing part is the one the remainder declares",
        );
        // Built, and nobody told the ledger.
        assert!(matches!(
            doc.unreconciled(
                "detail",
                &[
                    Part::new("subject", "Subject"),
                    Part::new("bytes", "Wire bytes")
                ]
            )[0],
            Unreconciled::Paid { .. },
        ));
    }

    /// ★★ A surface nobody specified answers a NAMED difference rather than an
    /// empty vector — "nothing is wrong" and "nobody declared this" must not
    /// read the same.
    #[test]
    fn an_unspecified_surface_is_reported_rather_than_passing() {
        let found = document().unreconciled("invented", &[Part::new("x", "X")]);
        assert_eq!(found.len(), 1);
        assert!(found[0].sentence().contains("no specification declares"));
    }

    #[test]
    fn a_document_that_is_not_one_is_refused() {
        assert!(matches!(
            super::SpecDocument::parse("not json"),
            Err(super::SpecDefect::NotJson { .. }),
        ));
        assert_eq!(
            super::SpecDocument::parse(r#"{"$comment": "only prose"}"#),
            Err(super::SpecDefect::NoSurfaces),
        );
        assert!(matches!(
            super::SpecDocument::parse(r#"{"columns": {"owed": []}}"#),
            Err(super::SpecDefect::Malformed { .. }),
        ));
        assert!(matches!(
            super::SpecDocument::parse(
                r#"{"columns": {"canon": [{"key": "id", "title": "ID"},
                    {"key": "id", "title": "Again"}], "owed": []}}"#
            ),
            Err(super::SpecDefect::Surface { .. }),
        ));
        assert!(matches!(
            super::SpecDocument::parse(
                r#"{"columns": {"canon": [{"key": "id", "title": "ID"}],
                    "owed": [{"key": "id", "sentence": "nothing names it",
                              "since": "R1", "why": "short"}]}}"#
            ),
            Err(super::SpecDefect::Ledger { .. }),
        ));
    }

    /// The wire shape is the framework's, so an agent asking two screens how
    /// much of their specification they are does not read two answers.
    #[test]
    fn a_document_publishes_one_shape_for_every_surface() {
        let built = |surface: &str| match surface {
            "columns" => Built::Standing(vec![Part::new("id", "ID"), Part::new("name", "Name")]),
            _ => Built::Standing(vec![Part::new("subject", "Subject")]),
        };
        let wire = document().wire(&built);
        // ★ R1758 — the verdict's own facts lead, and the surfaces are nested
        // under one key rather than sharing a namespace with them.
        assert_eq!(wire["evidence"], "declaration");
        assert_eq!(wire["specified"], 4);
        assert_eq!(wire["reproduced"], 3);
        let surfaces = &wire["surfaces"];
        assert_eq!(surfaces["columns"]["specified"], 2);
        assert_eq!(surfaces["columns"]["reproduced"], 2);
        assert_eq!(surfaces["detail"]["reproduced"], 1);
        assert_eq!(surfaces["detail"]["owed"][0]["since"], "R1731");
        // ★ R1758 — and each surface names WHICH parts it is judged on, so a
        // reader holding only this can check the count.
        assert_eq!(
            surfaces["columns"]["canon"]
                .as_array()
                .expect("an array")
                .iter()
                .map(|part| part["key"].as_str().expect("a key"))
                .collect::<Vec<_>>(),
            ["id", "name"],
        );
        for surface in ["columns", "detail"] {
            assert!(
                surfaces[surface]["unreconciled"]
                    .as_array()
                    .expect("an array")
                    .is_empty(),
                "{surface} publishes a difference its ledger does not declare",
            );
        }
    }

    /// ★★★★★ R1758 — the two entry points stamp two different qualifiers, and
    /// that is the whole mechanism.
    ///
    /// A verdict from [`SpecDocument::report`] says `declaration`: the caller
    /// was handed nothing, so whatever it answered with did not come from a
    /// recorded frame. A verdict from `report_from_paint` says `paint` — and
    /// **cannot** claim reproduction without a frame, because the framework
    /// substitutes away for every surface when the store is empty. That is why
    /// the qualifier is not gameable in the direction that matters.
    #[test]
    fn r1758_a_verdict_says_what_it_was_read_from() {
        use super::Evidence;

        let tabled = document().report(&|_| Built::Standing(vec![Part::new("id", "ID")]));
        assert_eq!(tabled.evidence(), Evidence::Declaration);
        assert_eq!(tabled.evidence().wire(), "declaration");
        assert!(tabled.standing() > 0, "the roster was accepted as given");

        let unpainted = document().report_from_paint("r1758-nothing-has-painted-this", &|_, _| {
            Built::Standing(vec![Part::new("id", "ID")])
        });
        assert_eq!(unpainted.evidence(), Evidence::Paint);
        assert_eq!(
            unpainted.reproduced(),
            0,
            "★ a verdict stamped `paint` with no frame behind it reproduces nothing, \
             whatever the screen would have answered",
        );
        assert_eq!(unpainted.standing(), 0);
        assert!(!unpainted.reconciles());
    }

    /// ★★★★★ The trait's reason for existing: the navigation axis's own
    /// difference type is judged by the same ledger, so the mechanism is
    /// written once.
    #[test]
    fn a_navigations_difference_is_judged_by_the_same_ledger() {
        use crate::availability::{Unavailable, UnavailableKind};
        use crate::widgets::destination::{Destination, Destinations, RosterSpec, SeatSpec};

        let spec = RosterSpec::new(vec![
            SeatSpec::open("dashboard", "Dashboard"),
            SeatSpec::open("keys", "Key Patterns"),
        ])
        .expect("the fixture is a navigable roster");
        let built = Destinations::new(vec![
            Destination::open("dashboard", "Dashboard"),
            Destination::closed(
                "keys",
                "Key Patterns",
                Unavailable::new(UnavailableKind::Unbuilt, "the behaviour reference"),
            ),
        ])
        .expect("the fixture rail is navigable");

        let ledger = Ledger::new(vec![Owed::new(
            "keys",
            "`keys` is specified open and is closed (unbuilt)",
            "R1728",
            "The reference implements this section and this build has not.",
        )])
        .expect("the entry names its seat, its round and its reason");
        assert!(ledger.reconciles(&spec.diff(&built)));
    }

    // --- R1738: the comparison as a value that can be added up ---------------

    fn two_surface_document() -> super::SpecDocument {
        super::SpecDocument::parse(
            r#"{
                 "columns": {
                   "canon": [
                     { "key": "id", "title": "ID" },
                     { "key": "name", "title": "Name" }
                   ],
                   "owed": []
                 },
                 "detail": {
                   "canon": [
                     { "key": "summary", "title": "Summary" }
                   ],
                   "owed": []
                 }
               }"#,
        )
        .expect("the fixture is a document of two surfaces")
    }

    /// A report adds its surfaces up, which is the whole reason it is typed:
    /// the wire form could be serialised per surface and never summed.
    #[test]
    fn r1738_a_report_is_every_surface_added_up() {
        let report = two_surface_document().report(&|surface| match surface {
            "columns" => Built::Standing(vec![Part::new("id", "ID"), Part::new("name", "Name")]),
            "detail" => Built::Standing(vec![Part::new("summary", "Summary")]),
            other => panic!("no surface named {other}"),
        });
        assert_eq!(report.surfaces().len(), 2);
        assert_eq!(report.specified(), 3);
        assert_eq!(report.reproduced(), 3);
        assert_eq!(report.standing(), 2);
        assert_eq!(report.away(), 0);
        assert!(report.reconciles());
    }

    /// ★★★★★ R1742 — a surface that is not on screen is neither reproduced nor
    /// diverged, and a report holding one does not reconcile.
    ///
    /// The two directions are asserted together on purpose. Crediting an
    /// unopened surface would inflate the count silently, and accusing it would
    /// make a working screen report as broken; the fixture is one document with
    /// one of each so neither repair can be made without the other showing.
    #[test]
    fn r1742_a_surface_that_is_not_on_screen_is_not_judged_either_way() {
        let report = two_surface_document().report(&|surface| match surface {
            "columns" => Built::Standing(vec![Part::new("id", "ID"), Part::new("name", "Name")]),
            "detail" => Built::away("nobody selected a row, so there is no detail to read"),
            other => panic!("no surface named {other}"),
        });
        assert_eq!(report.specified(), 3, "what it is supposed to be is fixed");
        assert_eq!(report.reproduced(), 2, "and the away surface counts for 0");
        assert_eq!(report.standing(), 1);
        assert_eq!(report.away(), 1);
        assert!(
            !report.reconciles(),
            "declining to be judged is not passing"
        );

        let detail = &report.surfaces()[1];
        assert!(!detail.is_standing());
        assert_eq!(
            detail.why(),
            Some("nobody selected a row, so there is no detail to read"),
        );
        assert!(
            detail.divergences().is_empty() && detail.unreconciled().is_empty(),
            "★ nothing was compared, so nothing can have diverged",
        );
        assert_eq!(
            detail.specified(),
            1,
            "★ and what it is specified to be does not depend on being opened",
        );

        // The published form carries the distinction, which is what a client
        // reads: `reproduced: 0` beside `standing: false` is a different fact
        // from `reproduced: 0` beside `standing: true`.
        let wire = report.to_json();
        let surfaces = &wire["surfaces"];
        assert_eq!(surfaces["detail"]["standing"], serde_json::json!(false));
        assert_eq!(surfaces["columns"]["standing"], serde_json::json!(true));
        assert_eq!(
            surfaces["detail"]["why"],
            serde_json::json!("nobody selected a row, so there is no detail to read"),
        );
        assert!(surfaces["columns"].get("why").is_none());
        assert_eq!(wire["away"], 1);
        assert_eq!(wire["standing"], 1);
    }

    /// ★★★★★ R1742 — **how many parts a build reproduces is a count of
    /// SPECIFIED parts**, and neither an extra one nor a part that diverges
    /// twice may be subtracted from it.
    ///
    /// Found by running: the node lab publishes a surface with a declared
    /// extra, and the first frame with several parts absent underflowed
    /// `specified - divergences.len()` and panicked. Both directions are
    /// asserted here, and the panicking case is one of them, because a repair
    /// that only clamped the subtraction would leave the under-report in place.
    #[test]
    fn r1742_reproduced_counts_specified_parts_and_not_divergences() {
        let doc = SpecDocument::parse(
            r#"{ "row": {
                   "canon": [
                     { "key": "a", "title": "A" },
                     { "key": "b", "title": "B" },
                     { "key": "c", "title": "C" }
                   ],
                   "owed": []
                 } }"#,
        )
        .expect("the fixture is a specification");

        // A build that GREW two parts and kept all three specified ones. The
        // old arithmetic answered 1 of 3 for a surface that has everything.
        let grown = doc.report(&|_| {
            Built::Standing(vec![
                Part::new("a", "A"),
                Part::new("b", "B"),
                Part::new("c", "C"),
                Part::new("d", "D"),
                Part::new("e", "E"),
            ])
        });
        assert_eq!(
            grown.reproduced(),
            3,
            "★ an extra part is not a missing one"
        );
        assert_eq!(grown.surfaces()[0].divergences().len(), 2);
        assert!(!grown.reconciles(), "and the extras are still reported");

        // One part, two divergences: moved AND renamed. It is one part failing
        // to be reproduced, reported as two facts a reader needs both of.
        let moved = doc.report(&|_| {
            Built::Standing(vec![
                Part::new("b", "B"),
                Part::new("a", "renamed"),
                Part::new("c", "C"),
            ])
        });
        assert_eq!(
            moved.surfaces()[0].divergences().len(),
            3,
            "a and b both moved, and a was renamed as well",
        );
        assert_eq!(
            moved.reproduced(),
            1,
            "★ two parts diverge — `c` is the one still where it was specified",
        );

        // And the case that panicked: more divergences than the surface has
        // specified parts.
        let ruined = doc.report(&|_| {
            Built::Standing(vec![
                Part::new("x", "X"),
                Part::new("y", "Y"),
                Part::new("z", "Z"),
            ])
        });
        assert_eq!(ruined.surfaces()[0].divergences().len(), 6);
        assert_eq!(
            ruined.reproduced(),
            0,
            "★★ nothing specified is there, and the count says 0 rather than \
             refusing to be a number",
        );
    }

    /// ★ And a declared remainder cannot be retired by never drawing the
    /// surface it is about: an away surface keeps its ledger and reconciles
    /// nothing.
    #[test]
    fn r1742_an_away_surface_does_not_reconcile_its_ledger() {
        let doc = SpecDocument::parse(
            r#"{ "columns": {
                   "canon": [ { "key": "id", "title": "ID" } ],
                   "owed": [ {
                     "key": "id",
                     "sentence": "`id` is specified and the surface has no such part",
                     "since": "R1742",
                     "why": "a fixture entry, so the ledger is not empty"
                   } ]
                 } }"#,
        )
        .expect("the fixture is a specification");
        let report = doc.report(&|_| Built::away("the pane holding it is collapsed"));
        let columns = &report.surfaces()[0];
        assert_eq!(columns.owed().len(), 1, "the entry is still published");
        assert!(
            columns.unreconciled().is_empty(),
            "and nothing was judged against it",
        );
        assert!(!report.reconciles());
    }

    /// ★ A difference **nobody accepted** is what fails, in either direction —
    /// not merely a difference. A surface with a reviewed remainder is a
    /// surface somebody looked at, and a check demanding zero divergences would
    /// make the ledger unusable the moment it held an entry.
    #[test]
    fn r1738_a_difference_nobody_wrote_down_is_what_stops_a_report_reconciling() {
        let report = two_surface_document().report(&|surface| match surface {
            "columns" => Built::Standing(vec![Part::new("id", "ID")]),
            "detail" => Built::Standing(vec![Part::new("summary", "Summary")]),
            other => panic!("no surface named {other}"),
        });
        assert_eq!(report.specified(), 3);
        assert_eq!(report.reproduced(), 2, "`name` is absent");
        assert!(!report.reconciles(), "and no entry declares it");
        let columns = &report.surfaces()[0];
        assert_eq!(columns.surface(), "columns");
        assert_eq!(columns.divergences().len(), 1);
        assert_eq!(columns.unreconciled().len(), 1);
        assert!(
            report.surfaces()[1].reconciles(),
            "the other surface is fine"
        );
    }

    /// The published form carries what the typed form counts.
    ///
    /// ★ Written the second time. The first draft asserted
    /// `wire(built) == report(built).to_json()`, which cannot fail: `wire` *is*
    /// that expression since this round made it one. A test whose two sides are
    /// the same expression is a test that would pass while the shape it is
    /// about changed underneath it — the class this workspace keeps finding, and
    /// the closing audit found it here. So the wire form is checked against
    /// values written out by hand, and the delegation is proved by the typed
    /// accessors agreeing with them.
    #[test]
    fn r1738_the_wire_form_publishes_what_the_report_counts() {
        let doc = two_surface_document();
        let built = |surface: &str| match surface {
            "columns" => Built::Standing(vec![Part::new("id", "ID")]),
            "detail" => Built::Standing(vec![Part::new("summary", "Summary")]),
            other => panic!("no surface named {other}"),
        };
        let wire = doc.wire(&built);
        assert_eq!(wire["specified"], 3);
        assert_eq!(wire["reproduced"], 2);
        assert_eq!(wire["reconciles"], false);
        let surfaces = &wire["surfaces"];
        assert_eq!(surfaces["columns"]["specified"], 2);
        assert_eq!(surfaces["columns"]["reproduced"], 1);
        assert_eq!(
            surfaces["columns"]["divergences"][0]["says"],
            "part 1 `name` (Name) is specified and the surface has no such part",
        );
        assert_eq!(
            surfaces["columns"]["unreconciled"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(surfaces["detail"]["specified"], 1);
        assert_eq!(surfaces["detail"]["reproduced"], 1);
        assert!(
            surfaces["detail"]["divergences"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        // And the typed accessors answer the same build, which is what makes
        // the two halves one derivation rather than two that agree today.
        let report = doc.report(&built);
        assert_eq!(report.specified(), 3);
        assert_eq!(report.reproduced(), 2);
        assert!(!report.reconciles());
    }

    /// ★★★★★ R1747 — a screen that has not painted has not been ASKED to draw
    /// anything, and a screen that drew none of its specification has.
    ///
    /// The two are different facts a reader acts on differently, and this is
    /// the assertion that keeps them apart: the report a screen answers from
    /// its own last frame is `away` when the frame store has nothing for that
    /// surface, and a comparison when it has. Written when the second screen to
    /// need those four lines copied the first one's away sentence byte for
    /// byte — at which point the sentence had stopped being either screen's
    /// own words.
    #[test]
    fn r1747_a_surface_that_never_painted_is_away_rather_than_reproducing_nothing() {
        use crate::painted::{PaintedRegions, forget_painted_regions, record_painted_regions};
        use crate::scene::Rect;

        let doc = two_surface_document();
        let built = |regions: &PaintedRegions, surface: &str| match surface {
            "columns" => Built::Standing(
                regions
                    .parts_under("row.")
                    .into_iter()
                    .map(|(key, _)| {
                        let title = if key == "id" { "ID" } else { "Name" };
                        Part::new(key, title)
                    })
                    .collect(),
            ),
            "detail" => Built::Standing(vec![Part::new("summary", "Summary")]),
            other => panic!("no surface named {other}"),
        };

        forget_painted_regions("nothing-has-painted-this");
        let unpainted = doc.report_from_paint("nothing-has-painted-this", &built);
        assert_eq!(unpainted.reproduced(), 0);
        assert_eq!(unpainted.away(), 2, "every surface, not only the first");
        assert!(
            !unpainted.reconciles(),
            "declining to be judged is not passing"
        );
        assert_eq!(
            unpainted.surfaces()[0].why(),
            Some("this screen has not painted a frame yet, so none of it is on screen"),
            "and the reason is the framework's own words rather than a screen's",
        );

        // The same document and the same closure, over a frame that DID paint.
        record_painted_regions(
            "a-surface-that-painted",
            PaintedRegions::from_marks(vec![
                ("row.id".to_owned(), Rect::new(0, 0, 10, 10)),
                ("row.name".to_owned(), Rect::new(10, 0, 10, 10)),
            ]),
        );
        let painted = doc.report_from_paint("a-surface-that-painted", &built);
        assert_eq!(painted.away(), 0);
        assert_eq!(painted.reproduced(), 3);
        assert!(
            painted.reconciles(),
            "the fixture is built to reproduce the document exactly, so the two \
             arms are told apart by more than one of them failing",
        );
        forget_painted_regions("a-surface-that-painted");
    }

    // ── R1770: a verdict says what SIZE it was read at ──────────────────────

    /// A document whose one accepted difference holds only at one measured
    /// extent, plus the two readings that extent tells apart.
    fn sized_ledger() -> Ledger {
        Ledger::new(vec![
            Owed::new(
                "status",
                "`status` is specified as \"Status of the run\" and reads \"Status\"",
                "R1770",
                "The box is too narrow at this size to hold the whole heading, and \
                 the reference truncates there too.",
            )
            .only_at(vec![crate::painted::Extent::new(400, 300)]),
        ])
        .expect("the entry names its part, its round and its reason")
    }

    /// ★★★★★ The repair, in one assertion pair: the same divergence list is
    /// reconciled at the extent the entry names and **undeclared** at one it
    /// does not.
    ///
    /// The second half is what makes the first safe. If an entry out of force
    /// merely went quiet, narrowing `at` would be a way to be excused
    /// everywhere; instead the difference reappears with nobody to own it.
    #[test]
    fn r1770_an_entry_excuses_a_difference_only_at_the_extent_it_names() {
        let ledger = sized_ledger();
        let spec = SurfaceSpec::new(vec![Part::new("status", "Status of the run")])
            .expect("the fixture is a roster of named parts");
        let found = spec.diff(&[Part::new("status", "Status")]);
        assert_eq!(found.len(), 1, "the fixture diverges exactly once");

        let named = crate::painted::Extent::new(400, 300);
        assert!(
            ledger.reconciles_at(Some(named), &found),
            "at the extent the entry was measured at, it is the declared difference",
        );

        let wider = crate::painted::Extent::new(1200, 300);
        let judged = ledger.judge_at(Some(wider), &found);
        assert!(
            matches!(judged.as_slice(), [Unreconciled::Undeclared { key, .. }] if key == "status"),
            "★ at an extent nobody measured, the SAME difference is undeclared \
             rather than excused: {judged:?}",
        );
    }

    /// The other direction, which is the one R1767 measured on the running
    /// tool: the build stops diverging because the surface grew, and the entry
    /// must not be reported paid for a size it never claimed.
    #[test]
    fn r1770_an_entry_out_of_force_is_not_owed_a_difference() {
        let ledger = sized_ledger();
        let spec = SurfaceSpec::new(vec![Part::new("status", "Status of the run")])
            .expect("the fixture is a roster of named parts");
        let reproduced = spec.diff(&[Part::new("status", "Status of the run")]);
        assert!(reproduced.is_empty(), "the wider build reproduces the part");

        let named = crate::painted::Extent::new(400, 300);
        assert!(
            matches!(
                ledger.judge_at(Some(named), &reproduced).as_slice(),
                [Unreconciled::Paid { .. }]
            ),
            "★ at the size it WAS measured at, an entry the build no longer needs \
             is still reported paid — the ratchet is unchanged",
        );

        let wider = crate::painted::Extent::new(1200, 300);
        assert!(
            ledger.reconciles_at(Some(wider), &reproduced),
            "★★★★★ and at a size it never claimed it is silent, which is the whole \
             repair: before this, a taller window made the tool demand the \
             deletion of an entry that a shorter one still needs",
        );
    }

    /// ★★★★★ The refusal that stops `at` being an escape hatch: a reader that
    /// does not say where it stood is told, per entry, that it cannot be
    /// judged — never quietly excused.
    #[test]
    fn r1770_a_verdict_that_names_no_extent_is_refused_by_a_sized_entry() {
        let ledger = sized_ledger();
        let spec = SurfaceSpec::new(vec![Part::new("status", "Status of the run")])
            .expect("the fixture is a roster of named parts");
        let reproduced = spec.diff(&[Part::new("status", "Status of the run")]);

        let judged = ledger.judge_at(None, &reproduced);
        assert!(
            matches!(judged.as_slice(), [Unreconciled::Unsized { key, .. }] if key == "status"),
            "an unsized verdict is refused rather than believed: {judged:?}",
        );
        assert!(
            judged[0].sentence().contains("400x300"),
            "and the refusal names the extents the entry WAS measured at: {}",
            judged[0].sentence(),
        );
        assert_eq!(
            ledger.judge(&reproduced),
            judged,
            "★ the plain `judge` is the unsized one, so an existing caller cannot \
             silently pass a sized ledger",
        );
    }

    /// An entry that names no extent means every extent — which is what every
    /// entry written before this round means, and what an entry about a
    /// difference the window cannot move should keep meaning.
    #[test]
    fn r1770_an_entry_naming_no_extent_holds_everywhere() {
        let ledger = Ledger::new(vec![Owed::new(
            "status",
            "part 0 `status` (Status) is specified and the surface has no such part",
            "R1770",
            "This build has no such part at any size, and the reason has nothing \
             to do with how much room it is given.",
        )])
        .expect("the entry names its part, its round and its reason");
        let spec = SurfaceSpec::new(vec![Part::new("status", "Status")])
            .expect("the fixture is a roster of named parts");
        let found = spec.diff(&[]);

        assert!(ledger.reconciles(&found), "with no extent handed to it");
        for extent in [(1, 1), (400, 300), (4000, 3000)] {
            assert!(
                ledger.reconciles_at(
                    Some(crate::painted::Extent::new(extent.0, extent.1)),
                    &found
                ),
                "and at {extent:?}",
            );
        }
    }

    /// The pin's two halves, parsed: `$at` for the document and `at` for one
    /// entry, with the refusals that keep a malformed size from reading as an
    /// absent one.
    #[test]
    fn r1770_a_document_declares_the_extent_its_canon_was_written_at() {
        let doc = SpecDocument::parse(
            r#"{
              "$at": { "width": 2494, "height": 1531 },
              "columns": {
                "canon": [{ "key": "id", "title": "ID" }],
                "owed": [{
                  "key": "id",
                  "sentence": "`id` is specified as \"ID\" and reads \"I\"",
                  "since": "R1770",
                  "why": "The column is too narrow at this size for the whole word.",
                  "at": [{ "width": 2494, "height": 1531 }]
                }]
              }
            }"#,
        )
        .expect("the fixture is a specification");
        assert_eq!(
            doc.written_at(),
            Some(crate::painted::Extent::new(2494, 1531)),
        );
        assert_eq!(
            doc.ledger("columns")
                .expect("the surface is declared")
                .owed()[0]
                .at,
            vec![crate::painted::Extent::new(2494, 1531)],
        );

        // A pin that says nothing says nothing — the state every pin in this
        // tree was in before this round, and the one this type must keep
        // reading as "no claim" rather than as a size of zero.
        assert_eq!(
            two_surface_document().written_at(),
            None,
            "a document with no `$at` claims no extent",
        );

        for malformed in [
            r#"{ "$at": { "width": 100 }, "s": { "canon": [{"key":"a","title":"A"}], "owed": [] } }"#,
            r#"{ "$at": 100, "s": { "canon": [{"key":"a","title":"A"}], "owed": [] } }"#,
        ] {
            assert!(
                matches!(
                    SpecDocument::parse(malformed),
                    Err(SpecDefect::Malformed { .. })
                ),
                "a size that is not a size is refused rather than dropped: {malformed}",
            );
        }
        assert!(
            matches!(
                SpecDocument::parse(
                    r#"{ "s": { "canon": [{"key":"a","title":"A"}],
                         "owed": [{ "key": "a", "sentence": "`a` is specified as \"A\" and reads \"\"",
                                    "since": "R1770",
                                    "why": "A reason long enough for the ledger to accept it here.",
                                    "at": [] }] } }"#
                ),
                Err(SpecDefect::Ledger { .. })
            ),
            "★ an empty `at` is refused: an entry that claims NO size excuses \
             nothing anywhere, which is a mistake rather than a statement",
        );
    }

    /// ★★★★★ The whole rule, end to end on the paint path: the extent comes
    /// off the store, not off the caller, and the report publishes it beside
    /// the size the document was written at.
    #[test]
    fn r1770_a_report_read_from_paint_says_what_extent_it_was_read_at() {
        use crate::painted::{
            Extent, PaintedRegions, forget_painted_regions, record_painted_regions,
        };
        use crate::scene::Rect;

        let doc = SpecDocument::parse(
            r#"{
              "$at": { "width": 800, "height": 600 },
              "columns": {
                "canon": [{ "key": "id", "title": "ID" }],
                "owed": []
              }
            }"#,
        )
        .expect("the fixture is a specification");
        let built = |regions: &PaintedRegions, _: &str| {
            Built::Standing(
                regions
                    .parts_under("row.")
                    .into_iter()
                    .map(|(key, _)| Part::new(key, "ID"))
                    .collect(),
            )
        };

        let tag = "r1770-a-surface-with-an-extent";
        record_painted_regions(
            tag,
            PaintedRegions::from_marks(vec![("row.id".to_owned(), Rect::new(0, 0, 10, 10))])
                .with_extent(Extent::new(800, 600)),
        );
        let read = doc.report_from_paint(tag, &built);
        assert_eq!(read.at(), Some(Extent::new(800, 600)));
        assert_eq!(read.written_at(), Some(Extent::new(800, 600)));
        assert!(read.read_where_written(), "the two sizes agree here");
        assert_eq!(read.to_json()["at"], "800x600");
        assert_eq!(read.to_json()["written_at"], "800x600");

        // The same document, the same build, a smaller surface — which is the
        // assembled tool's case, and the sentence it could not say before.
        record_painted_regions(
            tag,
            PaintedRegions::from_marks(vec![("row.id".to_owned(), Rect::new(0, 0, 10, 10))])
                .with_extent(Extent::new(400, 300)),
        );
        let elsewhere = doc.report_from_paint(tag, &built);
        assert_eq!(elsewhere.at(), Some(Extent::new(400, 300)));
        assert!(
            !elsewhere.read_where_written(),
            "★ judged at 400x300 against a canon written at 800x600 — a reader can \
             now tell this apart from the reading above, which they could not",
        );

        // A store with no extent cannot claim one, and a report built from a
        // declaration has no frame to take one from.
        record_painted_regions(
            tag,
            PaintedRegions::from_marks(vec![("row.id".to_owned(), Rect::new(0, 0, 10, 10))]),
        );
        assert_eq!(doc.report_from_paint(tag, &built).at(), None);
        assert_eq!(
            doc.report(&|_| Built::Standing(vec![Part::new("id", "ID")]))
                .at(),
            None,
            "a declaration has no size, and saying so is the point of the option",
        );
        forget_painted_regions(tag);
    }

    /// A scene-built store takes its extent from the root it walked, so a
    /// fixture cannot hand in a size the scene does not have.
    #[test]
    fn r1770_a_store_built_from_a_scene_takes_the_roots_extent() {
        use crate::painted::{Extent, PaintedRegions};
        use crate::scene::{ContainerNode, Rect};
        use crate::style::{LayoutStyle, Size};

        let mut root = ContainerNode::new(vec![]);
        root.rect = Rect::new(0, 0, 640, 480);
        root.layout = LayoutStyle::new().with_size(Size::px(640, 480));
        let regions = PaintedRegions::of_scene(&Scene::Container(root));
        assert_eq!(regions.extent(), Some(Extent::new(640, 480)));
    }
}
