//! R1689 — **a saved document, and a reading that says why it will not open.**
//!
//! A node system that cannot be put away is a demonstration. Saving one is not
//! interesting on its own — [`Document`] has derived `serde` since R1577, so
//! *writing* it is one call — and this module exists for the other direction,
//! which is where every editor in this tree had quietly settled for a `bool`.
//!
//! # The gap this closes, measured
//!
//! Two consumers here hand-rolled the same envelope. The node editor wrapped
//! the document in `{schema_version, graph}` and answered a load with `false`
//! for **four different reasons** — the text was not JSON, the version did not
//! match, the graph broke an invariant, or there was nothing stored — so
//! neither a person nor an agent could tell a stale file from a corrupt one.
//! The node lab had no save at all. Neither is a fault of those screens: there
//! was nothing to reach for.
//!
//! The reference toolkit's own window-state restore is the same shape and is
//! the floor this is measured against: it takes an opaque byte blob and a
//! version number and answers a **bool**. A version mismatch answers `false`
//! with the window untouched and nothing said; and an item in the blob whose
//! widget is no longer present becomes a placeholder, announced only through a
//! logging category that is off by default. It also runs its parse **twice** —
//! once in a private "testing" mode, to find out whether the blob is usable
//! before it touches anything — and that answer is not available to the caller
//! at all.
//!
//! # What is here instead
//!
//! [`Archive::read`] does that check pass and **hands you the result**:
//!
//! * [`Opening::condition`] is the whole answer as ONE value ([`Condition`]):
//!   there is no document, or there is one and its own invariants do not hold,
//!   or there is one and it is sound. Three answers, so a type rather than two
//!   `Option`s and a precedence rule each caller has to know — R1978 measured
//!   two callers working that rule out for themselves, and one of them had
//!   worked out a different answer for a state the other could not produce.
//! * [`Opening::refusal`] names which of four things stopped it
//!   ([`Unreadable`]) rather than collapsing them into one word.
//! * [`Opening::violations`] is [`Document::validate`]'s own verdict, so a
//!   structurally broken document is refused for *stated* reasons — a dangling
//!   link, a containment cycle, a value on a port that is not there.
//! * [`Opening::dropped`] names what would not survive ([`Dropped`]): a saved
//!   selection pointing at a node the document no longer holds, a camera whose
//!   zoom is not a usable number, and the application's own extras when they
//!   are the part that cannot be read.
//! * [`Opening::reason`] is the one sentence a screen can put in front of a
//!   person and a wire can answer with.
//!
//! # ★★ The document opens even when the application's extras do not
//!
//! The companion — whatever the application wants to keep beside the graph —
//! is parsed **independently of the document**. A stream that is read
//! front-to-back cannot do this: one bad field and everything after it is
//! gone, which is why the reference's restore is all-or-nothing. Here the two
//! are separate parses over one envelope, so an application that changed the
//! shape of its own extras still gets its graph back, and is *told* that the
//! extras were left behind.
//!
//! That asymmetry is deliberate and it has a direction: the document is this
//! crate's and it can be checked, the companion is the application's and it
//! cannot. Refusing a graph because a screen's saved zoom level was written by
//! a newer build would be this crate making a decision it has no standing to
//! make.
//!
//! # TWO versions, and only one of them has a migration
//!
//! ⚠ **This section said something narrower until R2006, and the narrower thing
//! was true — about a different question.** It read *there is no migration
//! hook*, full stop. That is right about [`REVISION`], the FORMAT's own version:
//! one exists, so a chain written before a second is a guess at what the change
//! will be, and the honest thing a reader can do is *name* the mismatch, which
//! [`Unreadable::Revision`] does and the reference does not. It is **not** right
//! about the application's node kinds, and R2005 cited it to rule that question
//! out — a sentence answering the wrong question is worse than no sentence,
//! because it reads like an answer.
//!
//! So there are two versions here and they are two histories with two owners:
//!
//! * [`REVISION`] moves when **this crate** changes the file's shape. No
//!   migration, for the reason above.
//! * [`NodeKind::version`] moves when the **application** changes what its node
//!   kinds mean, and [`Document::migrate`] runs every step between the version a
//!   file was written at and this build's, in order. R2006 built that, and the
//!   reference is what showed why the version has to belong to the mechanism:
//!   its own conversion hook carries none, so each implementor fetches one for
//!   itself, and of the two that implement it only one does.
//!
//! One number for both would make a framework release force an application
//! migration. An application that versions its own **extras** still does so
//! inside its companion, where it knows what changed.
//!
//! ```
//! use pinion_node_graph::{Archive, Camera, Document, NodeBody, NodeId, NodeKind, Port, ROOT};
//! # #[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
//! # struct Op;
//! # impl NodeKind for Op {
//! #     type Type = ();
//! #     type Value = i64;
//! #     type Graph = ();
//! #     fn name(&self) -> String { "Op".into() }
//! #     fn inputs(&self) -> Vec<Port<(), i64>> { vec![Port::new("In", ())] }
//! #     fn outputs(&self) -> Vec<Port<(), i64>> { vec![Port::new("Out", ())] }
//! #     fn evaluate(&self, _: &[Option<i64>]) -> Vec<Option<i64>> { vec![None] }
//! # }
//! let mut doc: Document<Op> = Document::new("root");
//! let node = doc.add_node(ROOT, NodeBody::Kind(Op), 10, 20).unwrap();
//!
//! let text = Archive::<Op, ()>::of(doc)
//!     .with_camera(Camera::new(1.5, (0.0, 0.0)))
//!     .with_selection([node])
//!     .write()
//!     .unwrap();
//!
//! let reading = Archive::<Op, ()>::read(&text);
//! assert!(reading.opens());
//! assert_eq!(reading.reason(), None);
//! let back = reading.take().unwrap();
//! assert_eq!(back.selection(), &[node]);
//!
//! // A selection naming a node the document does not hold is REPORTED, not
//! // silently ignored — the case the reference turns into a placeholder.
//! let stale = text.replace(r#""selection": ["#, r#""selection": [4242, "#);
//! let reading = Archive::<Op, ()>::read(&stale);
//! assert!(reading.opens());
//! assert_eq!(
//!     reading.dropped(),
//!     [pinion_node_graph::Dropped::Selection(NodeId(4242))]
//! );
//! ```

use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::group::Violation;
use crate::model::{Document, NodeBody, NodeId, NodeKind, Tree, TreeId};
use crate::view::Camera;

/// The revision of the archive format itself.
///
/// Stamped by [`Archive::write`] and checked by [`Archive::read`]. It is the
/// *format's*, not the application's: what a screen keeps beside its graph is
/// versioned inside the companion, by whoever knows what changed.
///
/// ★ R2006 — and the taxonomy's own version is stamped BESIDE it, by
/// [`NodeKind::version`]. Two numbers, because they are two histories with two
/// owners: this one moves when the file's shape does, and that one when the
/// application changes what its node kinds mean. One number for both would make
/// a framework release force an application migration, and the reference has
/// exactly that confusion in the other direction — its conversion hook carries
/// no version at all, so each implementor fetches one for itself.
pub const REVISION: u32 = 1;

/// ★★★★★ R2006 — what [`Document::migrate`] did: every step it ran, in order,
/// and which nodes each one rewrote.
///
/// The reference's equivalent answers `void` and writes its failures to a
/// warning log, so *nothing happened* and *four things happened* are the same
/// answer to a caller. Here a migration is a value: a screen can say what it
/// did, a test can assert it, and a document that needed nothing reports
/// `steps` empty rather than being indistinguishable from one that was never
/// asked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Migration {
    /// The version the archive was written at.
    pub from: u32,
    /// The version it is now at — this build's [`NodeKind::version`].
    pub to: u32,
    /// One entry per step from `from + 1` to `to` that changed anything, in
    /// ascending order.
    ///
    /// ⚠ Steps that changed nothing are **left out**, and steps that changed
    /// something are all here: a step is in this list exactly when it did
    /// work. That is why running is not the same as appearing.
    pub steps: Vec<Rewritten>,
}

impl Migration {
    /// Whether anything was rewritten at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Every node this migration rewrote, ascending and without repeats — a
    /// node touched by two steps appears once.
    #[must_use]
    pub fn touched(&self) -> Vec<NodeId> {
        let mut all: Vec<NodeId> = self
            .steps
            .iter()
            .flat_map(|step| step.nodes.iter().copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        all
    }
}

/// One step of a taxonomy's history, and what it rewrote (R2006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewritten {
    /// The version this step brings a document up to.
    pub step: u32,
    /// The nodes it rewrote, ascending.
    pub nodes: Vec<NodeId>,
}

/// A document on its way out of the process, with the view it was left in and
/// whatever the application keeps beside it.
///
/// The revision is not a field: a value in hand is always this build's, so
/// carrying the number would be carrying a fact that cannot be false. It is a
/// property of the *text*, which is where it can differ, and that is where
/// [`Unreadable::Revision`] reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Archive<K: NodeKind, C = ()> {
    document: Document<K>,
    camera: Option<Camera>,
    selection: Vec<NodeId>,
    companion: Option<C>,
}

impl<K: NodeKind, C> Archive<K, C> {
    /// The document alone: no camera, no selection, no companion.
    #[must_use]
    pub fn of(document: Document<K>) -> Self {
        Self {
            document,
            camera: None,
            selection: Vec::new(),
            companion: None,
        }
    }

    /// Keep where the canvas was pointed.
    #[must_use]
    pub fn with_camera(mut self, camera: Camera) -> Self {
        self.camera = Some(camera);
        self
    }

    /// Keep what was selected.
    #[must_use]
    pub fn with_selection(mut self, selection: impl IntoIterator<Item = NodeId>) -> Self {
        self.selection = selection.into_iter().collect();
        self
    }

    /// Keep the application's own state beside the graph.
    ///
    /// A companion that serialises to JSON `null` — a unit type is the case —
    /// is indistinguishable from none once written, which is exactly as much
    /// as a null carries.
    #[must_use]
    pub fn with_companion(mut self, companion: C) -> Self {
        self.companion = Some(companion);
        self
    }

    /// The graph.
    #[must_use]
    pub const fn document(&self) -> &Document<K> {
        &self.document
    }

    /// Where the canvas was pointed, if that was kept.
    #[must_use]
    pub const fn camera(&self) -> Option<Camera> {
        self.camera
    }

    /// What was selected, after any [`Dropped::Selection`] has been taken out.
    #[must_use]
    pub fn selection(&self) -> &[NodeId] {
        &self.selection
    }

    /// The application's own state, if it was kept and could be read.
    #[must_use]
    pub const fn companion(&self) -> Option<&C> {
        self.companion.as_ref()
    }

    /// The four parts, for a caller installing them.
    #[must_use]
    pub fn into_parts(self) -> (Document<K>, Option<Camera>, Vec<NodeId>, Option<C>) {
        (self.document, self.camera, self.selection, self.companion)
    }
}

impl<K, C> Archive<K, C>
where
    K: NodeKind + Serialize,
    K::Type: Serialize,
    K::Value: Serialize,
    K::Graph: Serialize,
    C: Serialize,
{
    /// The archive as text, with [`REVISION`] stamped on it.
    ///
    /// Indented, because the file is a thing a person opens and an agent
    /// diffs, and because the reference's equivalent is an opaque byte blob
    /// that is neither.
    ///
    /// # Errors
    ///
    /// [`Unwritable`] when the taxonomy's own type or value, or the
    /// companion, cannot be represented as JSON — a map with non-string keys
    /// is the usual cause. The document's own structure always can.
    pub fn write(&self) -> Result<String, Unwritable> {
        let envelope = Envelope {
            revision: REVISION,
            taxonomy: K::version(),
            document: to_value(&self.document)?,
            camera: self.camera,
            selection: self.selection.clone(),
            companion: to_value(&self.companion)?,
        };
        serde_json::to_string_pretty(&envelope).map_err(|e| Unwritable {
            message: e.to_string(),
        })
    }
}

impl<K, C> Archive<K, C>
where
    K: NodeKind + DeserializeOwned,
    K::Type: DeserializeOwned,
    K::Value: DeserializeOwned,
    K::Graph: DeserializeOwned,
    C: DeserializeOwned,
{
    /// Read `text` **without installing anything**: the plan.
    ///
    /// Everything that can be decided before a change is decided here — the
    /// envelope parses, the revision matches, the document parses, the
    /// document's own invariants hold, the selection points at nodes that are
    /// there, the camera is a usable number, the companion is readable. The
    /// answer is a [`Opening`], which the caller inspects and then either
    /// takes or does not.
    #[must_use]
    pub fn read(text: &str) -> Opening<K, C> {
        if text.trim().is_empty() {
            return Opening::refused(Unreadable::Empty);
        }
        let envelope: Envelope = match serde_json::from_str(text) {
            Ok(envelope) => envelope,
            Err(e) => {
                return Opening::refused(Unreadable::Malformed {
                    message: e.to_string(),
                });
            }
        };
        if envelope.revision != REVISION {
            return Opening::refused(Unreadable::Revision {
                found: envelope.revision,
                wanted: REVISION,
            });
        }
        let document: Document<K> = match serde_json::from_value(envelope.document) {
            Ok(document) => document,
            Err(e) => {
                return Opening::refused(Unreadable::Document {
                    message: e.to_string(),
                });
            }
        };

        let mut dropped = Vec::new();

        // ★ A saved selection is checked against the document that came with
        // it, which is the check the reference turns into a placeholder and
        // logs behind a disabled category.
        let mut selection = Vec::new();
        for id in envelope.selection {
            if document.trees().any(|tree| tree.node(id).is_some()) {
                selection.push(id);
            } else {
                dropped.push(Dropped::Selection(id));
            }
        }

        // ★ A camera is two floating-point numbers, and the values that make a
        // canvas go permanently blank are representable in JSON. A zoom of
        // zero divides in `Camera::unproject`; a NaN poisons every projection
        // it touches and never recovers.
        let camera = match envelope.camera {
            Some(camera)
                if camera.zoom.is_finite()
                    && camera.zoom > 0.0
                    && camera.pan.0.is_finite()
                    && camera.pan.1.is_finite() =>
            {
                Some(camera)
            }
            Some(camera) => {
                dropped.push(Dropped::Camera {
                    zoom: camera.zoom,
                    pan: camera.pan,
                });
                None
            }
            None => None,
        };

        let companion = if envelope.companion.is_null() {
            None
        } else {
            match serde_json::from_value(envelope.companion) {
                Ok(companion) => Some(companion),
                Err(e) => {
                    dropped.push(Dropped::Companion {
                        message: e.to_string(),
                    });
                    None
                }
            }
        };

        let taxonomy = envelope.taxonomy;
        Opening::opened(
            document.validate(),
            dropped,
            Archive {
                document,
                camera,
                selection,
                companion,
            },
            taxonomy,
        )
    }
}

/// What reading an archive found — computed **before** anything is installed.
///
/// The reference toolkit runs this pass too, in a private mode, and throws the
/// answer away. Handing it back is the whole point: a screen shows
/// [`reason`](Self::reason), a repair tool reads [`violations`](Self::violations),
/// and a migration reads [`refusal`](Self::refusal) to find out which revision
/// it is looking at.
///
/// Named for the act rather than for the reading, because [`crate::Reading`]
/// is already a value the debugger's watches answer with — one word, two
/// unrelated subjects, and the compiler said so.
#[derive(Debug, Clone, PartialEq)]
pub struct Opening<K: NodeKind, C = ()> {
    outcome: Outcome<K, C>,
}

/// ★★★★★ R1978 — the two shapes [`Archive::read`] can end in, and **nothing
/// else**.
///
/// Private, and the whole of [`Opening`]'s state: every accessor below reads
/// this, so the crate has one place where "is there a document" is decided.
///
/// Before R1978 the same facts were four independent fields — an
/// `Option<Unreadable>`, a `Vec<Violation>`, a `Vec<Dropped>` and an
/// `Option<Archive>` — which can spell states `read` never produces (a refusal
/// carrying violations; no refusal and no archive) and which every caller had
/// to know could not happen. One of them did not: the node lab's own splitter
/// answered *openable* for a refusal that carried violations, where
/// [`Opening::reason`] answered *the refusal*. Neither was reachable, and that
/// is the point — a disagreement about an unreachable state is a disagreement
/// waiting for the state to become reachable.
#[derive(Debug, Clone, PartialEq)]
enum Outcome<K: NodeKind, C> {
    /// Nothing was read; this is why.
    Unreadable(Unreadable),
    /// A document was read: what [`Document::validate`] said about it, what
    /// could not come back with it, and the archive itself.
    ///
    /// ⚠ The archive is here whatever `violations` says. A document that does
    /// not satisfy its own invariants is still a document, and looking at it is
    /// how a person repairs it — see
    /// [`take_despite_violations`](Opening::take_despite_violations).
    Read {
        violations: Vec<Violation>,
        dropped: Vec<Dropped>,
        archive: Archive<K, C>,
        /// R2006 — the taxonomy version stamped on the file, or `0` for one
        /// written before the stamp existed.
        taxonomy: u32,
    },
}

impl<K: NodeKind, C> Opening<K, C> {
    fn refused(refusal: Unreadable) -> Self {
        Self {
            outcome: Outcome::Unreadable(refusal),
        }
    }

    fn opened(
        violations: Vec<Violation>,
        dropped: Vec<Dropped>,
        archive: Archive<K, C>,
        taxonomy: u32,
    ) -> Self {
        Self {
            outcome: Outcome::Read {
                violations,
                dropped,
                archive,
                taxonomy,
            },
        }
    }

    /// ★★★★★ R2006 — **the taxonomy version this file was written at**, so a
    /// caller knows what to migrate FROM.
    ///
    /// `None` for a file that could not be read at all — the version of nothing
    /// is not zero, and answering zero there would send a caller migrating a
    /// document it does not have.
    ///
    /// ⚠ A file written before the stamp existed answers `Some(0)`, which IS
    /// the honest answer: it predates every step, so every step applies. The
    /// distinction that matters is *unreadable* against *old*, and it is the
    /// one an `Option` draws.
    #[must_use]
    pub const fn taxonomy_version(&self) -> Option<u32> {
        match &self.outcome {
            Outcome::Read { taxonomy, .. } => Some(*taxonomy),
            Outcome::Unreadable(_) => None,
        }
    }

    /// ★★★★★ R1978 — **which of three things** reading found, as one value.
    ///
    /// The question a caller actually has is not "was it refused" and then "are
    /// there violations": it is *what is in front of me*, and there are three
    /// answers, which is one more than an `Option` can carry. Handing back two
    /// `Option`s plus the rule that a refusal wins asks every caller to
    /// re-derive the same three-way — and asking is getting it wrong: measured
    /// at R1978, the two callers in this workspace had each written their own,
    /// they disagreed, and the disagreement was invisible because it lived in a
    /// state [`Archive::read`] cannot produce.
    ///
    /// [`refusal`](Self::refusal) and [`violations`](Self::violations) remain,
    /// because a migration wants the revision numbers and a repair tool wants
    /// the list. What they no longer have to carry is the *decision*.
    ///
    /// ⚠ A [`Condition`] borrows, so it cannot outlive the opening it came
    /// from; take the archive with [`take`](Self::take) or
    /// [`take_despite_violations`](Self::take_despite_violations) once the
    /// decision is made.
    #[must_use]
    pub fn condition(&self) -> Condition<'_> {
        match &self.outcome {
            Outcome::Unreadable(refusal) => Condition::Unreadable(refusal),
            Outcome::Read { violations, .. } if violations.is_empty() => Condition::Sound,
            Outcome::Read { violations, .. } => Condition::Unsound(violations),
        }
    }

    /// Why there is no document at all, or `None` when there is one.
    #[must_use]
    pub const fn refusal(&self) -> Option<&Unreadable> {
        match &self.outcome {
            Outcome::Unreadable(refusal) => Some(refusal),
            Outcome::Read { .. } => None,
        }
    }

    /// What [`Document::validate`] says about the document that was read.
    ///
    /// Non-empty means the text held a document and the document is not one
    /// this crate's own edits could ever have produced. It is kept apart from
    /// [`refusal`](Self::refusal) because the two are different questions —
    /// "is there a graph here" and "is the graph sound" — and a repair tool
    /// wants the second with the first answered yes.
    #[must_use]
    pub fn violations(&self) -> &[Violation] {
        match &self.outcome {
            Outcome::Read { violations, .. } => violations,
            Outcome::Unreadable(_) => &[],
        }
    }

    /// What would not survive the read, named.
    #[must_use]
    pub fn dropped(&self) -> &[Dropped] {
        match &self.outcome {
            Outcome::Read { dropped, .. } => dropped,
            Outcome::Unreadable(_) => &[],
        }
    }

    /// Whether the archive can be installed: a document was read and its own
    /// invariants hold.
    ///
    /// Drops do not stop it. Every one of them is a part the document does not
    /// depend on, and refusing a whole graph because a saved selection went
    /// stale is the failure mode this module was written against.
    #[must_use]
    pub fn opens(&self) -> bool {
        matches!(self.condition(), Condition::Sound)
    }

    /// The one sentence to put in front of a person, or `None` when it opens.
    ///
    /// This is the value the `bool` was standing in for.
    ///
    /// ★ R1978 — derived from [`condition`](Self::condition), so the rule that
    /// a refusal outranks a violation list is written once. It used to be
    /// spelled here and re-spelled by every caller that wanted the two apart.
    #[must_use]
    pub fn reason(&self) -> Option<String> {
        self.condition().sentence()
    }

    /// The archive, if it opens.
    #[must_use]
    pub fn take(self) -> Option<Archive<K, C>> {
        match self.outcome {
            Outcome::Read {
                violations,
                archive,
                ..
            } if violations.is_empty() => Some(archive),
            _ => None,
        }
    }

    /// The archive even though it does not open — for a tool whose job is to
    /// look at a broken document rather than to run it.
    ///
    /// Separate from [`take`](Self::take) and named for what it overrides, so
    /// a caller cannot reach it without saying so.
    #[must_use]
    pub fn take_despite_violations(self) -> Option<Archive<K, C>> {
        match self.outcome {
            Outcome::Read { archive, .. } => Some(archive),
            Outcome::Unreadable(_) => None,
        }
    }
}

/// ★★★★★ R1978 — what condition the saved graph is in: **one of three**.
///
/// The three are exclusive and exhaustive by construction — they are
/// [`Archive::read`]'s own two endings with the readable one split on whether
/// the document satisfies its own invariants — so a caller that matches on this
/// has considered every answer, and a caller written before a fourth answer
/// existed would stop compiling rather than fall through as openable.
///
/// That last property is why this is a type and not a pair of accessors. The
/// screen this crate was written for had, before R1978, worked the split out
/// for itself against the *absence* of a refusal, and its own comment recorded
/// what nothing then asserted: a future opening that reported a third kind of
/// trouble through neither channel would have been treated as fine.
///
/// # Which arm a caller wants
///
/// * [`Unreadable`](Self::Unreadable) — there is **no document**. A screen has
///   nothing to show and says why; nothing it holds should change.
/// * [`Unsound`](Self::Unsound) — there **is** a document and it breaks its own
///   invariants. Whether to show it is the caller's call and both answers are
///   defensible: a tool that runs graphs refuses, a tool whose job is to repair
///   one opens it and names the faults.
/// * [`Sound`](Self::Sound) — there is a document and it holds. Note that
///   [`dropped`](Opening::dropped) can still be non-empty: a stale selection or
///   an unreadable companion is not the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition<'a> {
    /// No document was read, for this reason.
    Unreadable(&'a Unreadable),
    /// A document was read and [`Document::validate`] refuses it, for these
    /// reasons. Never empty — an empty list is [`Sound`](Self::Sound).
    Unsound(&'a [Violation]),
    /// A document was read and its own invariants hold.
    Sound,
}

impl Condition<'_> {
    /// The one sentence for this condition, or `None` when there is nothing to
    /// report.
    ///
    /// [`Opening::reason`] is this, and this is where it lives now: a caller
    /// that has already matched the arm should not have to go back to the
    /// opening for the words, and a second spelling of them would be a second
    /// rule. `Sound` answers `None` — the sentence for "it worked" belongs to
    /// the screen, which knows what it just opened and how it names things.
    #[must_use]
    pub fn sentence(&self) -> Option<String> {
        match self {
            Self::Unreadable(refusal) => Some(refusal.to_string()),
            // ⚠ `first()?` rather than an index: the arm's own documentation
            // says the slice is never empty, and a `?` keeps that promise from
            // being enforced by a panic.
            Self::Unsound(violations) => {
                let first = violations.first()?;
                Some(match violations.len() {
                    1 => format!("the graph is not sound: {first}"),
                    n => format!("the graph is not sound in {n} ways, starting with: {first}"),
                })
            }
            Self::Sound => None,
        }
    }
}

/// Why an archive yielded no document at all.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unreadable {
    /// There was nothing to read.
    ///
    /// Its own arm because it is the answer to a *different* question — "has
    /// anything been saved yet" — and a screen that showed "the file is
    /// corrupt" the first time it was opened would be lying.
    Empty,
    /// The text is not the archive envelope.
    Malformed {
        /// What the parser said, position included.
        message: String,
    },
    /// The archive was written by a build with a different format revision.
    Revision {
        /// What the text says.
        found: u32,
        /// What this build reads.
        wanted: u32,
    },
    /// The envelope parsed and the graph inside it did not — the taxonomy has
    /// changed under the file.
    Document {
        /// What the parser said.
        message: String,
    },
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "nothing has been saved"),
            Self::Malformed { message } => write!(f, "this is not a saved graph: {message}"),
            Self::Revision { found, wanted } => write!(
                f,
                "saved by a different build: the file is revision {found}, this reads {wanted}"
            ),
            Self::Document { message } => {
                write!(f, "the saved graph does not fit this tool: {message}")
            }
        }
    }
}

impl std::error::Error for Unreadable {}

/// A part of an archive that could not come back, with the document itself
/// intact.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Dropped {
    /// The saved selection named a node this document does not hold.
    Selection(NodeId),
    /// The saved camera is not a usable one.
    Camera {
        /// The zoom it carried.
        zoom: f64,
        /// The pan it carried.
        pan: (f64, f64),
    },
    /// The document was read and the application's own extras were not.
    Companion {
        /// What the parser said.
        message: String,
    },
}

impl fmt::Display for Dropped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Selection(node) => {
                write!(f, "node {node} was selected and is no longer in the graph")
            }
            Self::Camera { zoom, pan } => write!(
                f,
                "the saved view is not a usable one (zoom {zoom}, pan {}, {})",
                pan.0, pan.1
            ),
            Self::Companion { message } => {
                write!(f, "the saved screen state could not be read: {message}")
            }
        }
    }
}

/// Why an archive could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unwritable {
    message: String,
}

impl fmt::Display for Unwritable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the graph could not be written out: {}", self.message)
    }
}

impl std::error::Error for Unwritable {}

/// The wire form: one object, whose `document` and `companion` are parsed in
/// **two independent steps** so that one failing does not take the other.
#[derive(Serialize, Deserialize)]
struct Envelope {
    revision: u32,
    /// ★ R2006 — the TAXONOMY's version, beside the format's.
    ///
    /// `#[serde(default)]` so an archive written before this field existed
    /// reads as version `0` and migrates from there — which is the honest
    /// answer for a file that predates the stamp, and is itself the first thing
    /// a migration mechanism has to get right about its own arrival.
    #[serde(default)]
    taxonomy: u32,
    document: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    camera: Option<Camera>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    selection: Vec<NodeId>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    companion: serde_json::Value,
}

impl<K: NodeKind> Document<K> {
    /// ★★★★★ R2006 — **bring this document's node kinds up to date**, one step
    /// at a time, and say what each step did.
    ///
    /// Runs [`NodeKind::at_step`] for every step from `from + 1` up to
    /// [`NodeKind::version`], **in ascending order**, over every application
    /// node in every tree. A step sees what the step before it produced, which
    /// is what lets a later step repair an earlier one's output.
    ///
    /// ⚠ **That is where the reference is wrong**, and its own source is what
    /// says so: the versioned one of its two conversion hooks is written
    /// `if (v < 21) { … } else if (v < 24) { … }`, and the declaration of step
    /// 24 carries a comment saying that documents brought up to date by step 21
    /// may end up with a wrong default for one parameter. Step 24 therefore
    /// exists to repair what step 21 produces — and a document at version 10
    /// takes step 21 and is then **excluded by the `else`** from the repair it
    /// has just earned.
    ///
    /// A document already at or past the current version is left alone and
    /// answers an empty [`Migration`] — not because nothing was asked, but
    /// because a version that is not behind has no steps between it and now.
    #[must_use]
    pub fn migrate(&mut self, from: u32) -> Migration {
        let to = K::version();
        let mut ran = Migration {
            from,
            to,
            steps: Vec::new(),
        };
        for step in (from + 1)..=to {
            let mut changed = Vec::new();
            for tree in 0..self.tree_count() {
                let tree = TreeId(u32::try_from(tree).unwrap_or(u32::MAX));
                // ★ The ids are collected before anything is written: the walk
                // reads what the previous step left, and rewriting inside it
                // would be reading a tree that is being changed.
                let wanted: Vec<(NodeId, K)> = self
                    .tree(tree)
                    .into_iter()
                    .flat_map(Tree::nodes)
                    .filter_map(|held| match &held.body {
                        NodeBody::Kind(kind) => kind.at_step(step).map(|next| (held.id, next)),
                        _ => None,
                    })
                    .collect();
                for (node, next) in wanted {
                    if let Some(held) = self.tree_mut(tree).and_then(|host| host.node_mut(node)) {
                        held.body = NodeBody::Kind(next);
                        changed.push(node);
                    }
                }
            }
            changed.sort_unstable();
            // ★ A step that changed nothing is left OUT rather than recorded
            // empty, so `steps` is the list of steps that did work and its
            // length is a count of them. `from`/`to` already say which steps
            // were offered, so nothing is lost by the omission.
            if !changed.is_empty() {
                ran.steps.push(Rewritten {
                    step,
                    nodes: changed,
                });
            }
        }
        ran
    }
}

fn to_value<T: Serialize>(value: &T) -> Result<serde_json::Value, Unwritable> {
    serde_json::to_value(value).map_err(|e| Unwritable {
        message: e.to_string(),
    })
}
