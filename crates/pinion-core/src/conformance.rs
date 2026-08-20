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
        }
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
}

impl Unreconciled {
    /// The key this is about.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Unreconciled::Undeclared { key, .. }
            | Unreconciled::Paid { key, .. }
            | Unreconciled::Reworded { key, .. } => key,
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
            owed.push(Owed {
                key: field("key")?,
                sentence: field("sentence")?,
                since: field("since")?,
                why,
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
        let mut out = Vec::new();
        let mut matched = vec![false; self.owed.len()];
        for difference in found {
            let sentence = difference.sentence();
            let entry = self
                .owed
                .iter()
                .enumerate()
                .find(|(at, e)| !matched[*at] && e.key == difference.key());
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
        for (at, entry) in self.owed.iter().enumerate() {
            if !matched[at] {
                out.push(Unreconciled::Paid {
                    key: entry.key.clone(),
                    sentence: entry.sentence.clone(),
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
        Ok(Self { order, canon, owed })
    }

    /// The surfaces this document fixes, in the order it declares them.
    pub fn surfaces(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
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
        let (Some(canon), Some(ledger)) = (self.canon(surface), self.ledger(surface)) else {
            return vec![Unreconciled::Undeclared {
                key: surface.to_owned(),
                sentence: format!("`{surface}` is a surface no specification declares"),
            }];
        };
        ledger.judge(&canon.diff(built))
    }

    /// ★★★★★ R1738 — the whole comparison, as a value that can be **added up**.
    ///
    /// See [`DocumentReport`] for why this exists beside [`wire`](Self::wire),
    /// which now derives from it: the wire form was the only form, and a wire
    /// form is where a count goes to stop being a count.
    #[must_use]
    pub fn report(&self, built: &dyn Fn(&str) -> Vec<Part>) -> DocumentReport {
        DocumentReport {
            surfaces: self
                .surfaces()
                .map(|surface| {
                    let canon = &self.canon[surface];
                    let ledger = &self.owed[surface];
                    let divergences = canon.diff(&built(surface));
                    SurfaceStanding {
                        unreconciled: ledger.judge(&divergences),
                        surface: surface.to_owned(),
                        specified: canon.len(),
                        divergences,
                        owed: ledger.owed().to_vec(),
                    }
                })
                .collect(),
        }
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
    pub fn wire(&self, built: &dyn Fn(&str) -> Vec<Part>) -> serde_json::Value {
        self.report(built).to_json()
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
/// use pinion_core::conformance::{Part, SpecDocument};
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
/// let report = doc.report(&|_| vec![Part::new("id", "ID")]);
/// assert_eq!(report.specified(), 2);
/// assert_eq!(report.reproduced(), 1);
/// assert!(!report.reconciles());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceStanding {
    surface: String,
    specified: usize,
    divergences: Vec<PartDivergence>,
    unreconciled: Vec<Unreconciled>,
    owed: Vec<Owed>,
}

impl SurfaceStanding {
    /// The surface this is about.
    #[must_use]
    pub fn surface(&self) -> &str {
        &self.surface
    }

    /// How many parts the specification fixes.
    #[must_use]
    pub fn specified(&self) -> usize {
        self.specified
    }

    /// How many of them the build has, in the place the specification puts
    /// them.
    #[must_use]
    pub fn reproduced(&self) -> usize {
        self.specified - self.divergences.len()
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
    #[must_use]
    pub fn reconciles(&self) -> bool {
        self.unreconciled.is_empty()
    }
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
}

impl DocumentReport {
    /// Each surface, in the order the document declares them.
    #[must_use]
    pub fn surfaces(&self) -> &[SurfaceStanding] {
        &self.surfaces
    }

    /// How many parts this specification fixes across every surface.
    #[must_use]
    pub fn specified(&self) -> usize {
        self.surfaces.iter().map(SurfaceStanding::specified).sum()
    }

    /// How many of them the build has where they were specified.
    #[must_use]
    pub fn reproduced(&self) -> usize {
        self.surfaces.iter().map(SurfaceStanding::reproduced).sum()
    }

    /// Whether every surface's difference is the difference somebody wrote
    /// down.
    #[must_use]
    pub fn reconciles(&self) -> bool {
        self.surfaces.iter().all(SurfaceStanding::reconciles)
    }

    /// The report as the value a running application publishes.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for standing in &self.surfaces {
            out.insert(
                standing.surface.clone(),
                serde_json::json!({
                    "specified": standing.specified(),
                    "reproduced": standing.reproduced(),
                    "divergences": standing
                        .divergences
                        .iter()
                        .map(|d| serde_json::json!({ "key": d.key(), "says": d.sentence() }))
                        .collect::<Vec<_>>(),
                    "owed": standing
                        .owed
                        .iter()
                        .map(|entry| serde_json::json!({
                            "key": entry.key,
                            "says": entry.sentence,
                            "since": entry.since,
                            "why": entry.why,
                        }))
                        .collect::<Vec<_>>(),
                    "unreconciled": standing
                        .unreconciled
                        .iter()
                        .map(|u| serde_json::json!({ "key": u.key(), "says": u.sentence() }))
                        .collect::<Vec<_>>(),
                }),
            );
        }
        serde_json::Value::Object(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Ledger, LedgerDefect, Owed, Part, PartDivergence, SurfaceDefect, SurfaceSpec, Unreconciled,
    };

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
            "columns" => vec![Part::new("id", "ID"), Part::new("name", "Name")],
            _ => vec![Part::new("subject", "Subject")],
        };
        let wire = document().wire(&built);
        assert_eq!(wire["columns"]["specified"], 2);
        assert_eq!(wire["columns"]["reproduced"], 2);
        assert_eq!(wire["detail"]["reproduced"], 1);
        assert_eq!(wire["detail"]["owed"][0]["since"], "R1731");
        for surface in ["columns", "detail"] {
            assert!(
                wire[surface]["unreconciled"]
                    .as_array()
                    .expect("an array")
                    .is_empty(),
                "{surface} publishes a difference its ledger does not declare",
            );
        }
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
            "columns" => vec![Part::new("id", "ID"), Part::new("name", "Name")],
            "detail" => vec![Part::new("summary", "Summary")],
            other => panic!("no surface named {other}"),
        });
        assert_eq!(report.surfaces().len(), 2);
        assert_eq!(report.specified(), 3);
        assert_eq!(report.reproduced(), 3);
        assert!(report.reconciles());
    }

    /// ★ A difference **nobody accepted** is what fails, in either direction —
    /// not merely a difference. A surface with a reviewed remainder is a
    /// surface somebody looked at, and a check demanding zero divergences would
    /// make the ledger unusable the moment it held an entry.
    #[test]
    fn r1738_a_difference_nobody_wrote_down_is_what_stops_a_report_reconciling() {
        let report = two_surface_document().report(&|surface| match surface {
            "columns" => vec![Part::new("id", "ID")],
            "detail" => vec![Part::new("summary", "Summary")],
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
            "columns" => vec![Part::new("id", "ID")],
            "detail" => vec![Part::new("summary", "Summary")],
            other => panic!("no surface named {other}"),
        };
        let wire = doc.wire(&built);
        assert_eq!(wire["columns"]["specified"], 2);
        assert_eq!(wire["columns"]["reproduced"], 1);
        assert_eq!(
            wire["columns"]["divergences"][0]["says"],
            "part 1 `name` (Name) is specified and the surface has no such part",
        );
        assert_eq!(wire["columns"]["unreconciled"].as_array().unwrap().len(), 1);
        assert_eq!(wire["detail"]["specified"], 1);
        assert_eq!(wire["detail"]["reproduced"], 1);
        assert!(wire["detail"]["divergences"].as_array().unwrap().is_empty());

        // And the typed accessors answer the same build, which is what makes
        // the two halves one derivation rather than two that agree today.
        let report = doc.report(&built);
        assert_eq!(report.specified(), 3);
        assert_eq!(report.reproduced(), 2);
        assert!(!report.reconciles());
    }
}
