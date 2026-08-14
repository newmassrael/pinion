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
//! # One revision, and no migration invented before there are two
//!
//! [`REVISION`] is the format's own, stamped on write and checked on read.
//! There is no migration hook: a chain written before a second revision exists
//! is a guess at what the change will be, and the honest thing a reader can do
//! today is *name* the mismatch — [`Unreadable::Revision`] carries both numbers
//! — which is already the half the reference does not do. An application that
//! versions its **own** extras does so inside its companion, where it knows
//! what changed.
//!
//! ```
//! use pinion_node_graph::{Archive, Camera, Document, NodeBody, NodeId, NodeKind, Port, ROOT};
//! # #[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
//! # struct Op;
//! # impl NodeKind for Op {
//! #     type Type = ();
//! #     type Value = i64;
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
use crate::model::{Document, NodeId, NodeKind};
use crate::view::Camera;

/// The revision of the archive format itself.
///
/// Stamped by [`Archive::write`] and checked by [`Archive::read`]. It is the
/// *format's*, not the application's: what a screen keeps beside its graph is
/// versioned inside the companion, by whoever knows what changed.
pub const REVISION: u32 = 1;

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

        Opening {
            refusal: None,
            violations: document.validate(),
            dropped,
            archive: Some(Archive {
                document,
                camera,
                selection,
                companion,
            }),
        }
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
    refusal: Option<Unreadable>,
    violations: Vec<Violation>,
    dropped: Vec<Dropped>,
    archive: Option<Archive<K, C>>,
}

impl<K: NodeKind, C> Opening<K, C> {
    fn refused(refusal: Unreadable) -> Self {
        Self {
            refusal: Some(refusal),
            violations: Vec::new(),
            dropped: Vec::new(),
            archive: None,
        }
    }

    /// Why there is no document at all, or `None` when there is one.
    #[must_use]
    pub const fn refusal(&self) -> Option<&Unreadable> {
        self.refusal.as_ref()
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
        &self.violations
    }

    /// What would not survive the read, named.
    #[must_use]
    pub fn dropped(&self) -> &[Dropped] {
        &self.dropped
    }

    /// Whether the archive can be installed: a document was read and its own
    /// invariants hold.
    ///
    /// Drops do not stop it. Every one of them is a part the document does not
    /// depend on, and refusing a whole graph because a saved selection went
    /// stale is the failure mode this module was written against.
    #[must_use]
    pub fn opens(&self) -> bool {
        self.archive.is_some() && self.violations.is_empty()
    }

    /// The one sentence to put in front of a person, or `None` when it opens.
    ///
    /// This is the value the `bool` was standing in for.
    #[must_use]
    pub fn reason(&self) -> Option<String> {
        if let Some(refusal) = &self.refusal {
            return Some(refusal.to_string());
        }
        let first = self.violations.first()?;
        Some(match self.violations.len() {
            1 => format!("the graph is not sound: {first}"),
            n => format!("the graph is not sound in {n} ways, starting with: {first}"),
        })
    }

    /// The archive, if it opens.
    #[must_use]
    pub fn take(self) -> Option<Archive<K, C>> {
        self.opens().then_some(self.archive).flatten()
    }

    /// The archive even though it does not open — for a tool whose job is to
    /// look at a broken document rather than to run it.
    ///
    /// Separate from [`take`](Self::take) and named for what it overrides, so
    /// a caller cannot reach it without saying so.
    #[must_use]
    pub fn take_despite_violations(self) -> Option<Archive<K, C>> {
        self.archive
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
    document: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    camera: Option<Camera>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    selection: Vec<NodeId>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    companion: serde_json::Value,
}

fn to_value<T: Serialize>(value: &T) -> Result<serde_json::Value, Unwritable> {
    serde_json::to_value(value).map_err(|e| Unwritable {
        message: e.to_string(),
    })
}
