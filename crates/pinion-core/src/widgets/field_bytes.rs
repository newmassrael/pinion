//! R1663 §5.27 §5.40 §5.41 — **a decoded field says which bytes it came from.**
//!
//! ## The relation nothing held
//!
//! Two halves of a binary-inspection screen already exist here and they do not
//! touch. [`row_dissect`](super::row_dissect) explodes a structured row into a
//! tree of named fields with a stable path per node; [`hex_dump`](super::hex_dump)
//! maps a byte to its two hex cells and its one ascii cell and back again. Both
//! are bidirectional *within themselves* — and neither says which bytes a field
//! was decoded from.
//!
//! Measured on the wire before this module existed: the dissection External
//! publishes thirteen paths and **not one names a byte**; the hex External
//! publishes seventeen and **not one names a field**. So "select a field, watch
//! its bytes light up — click a byte, watch the field light up" was, on both
//! surfaces, application code with a hand-kept second map. A hand-kept second
//! map is the defect this module removes, because two maps of one fact drift
//! and nothing says when they have.
//!
//! ## Why the marks this tree already has are not this
//!
//! R1615's [`crate::marks::MarkSet`] is a named run over a content
//! index space and it *is* bidirectional — `get(name)` forward,
//! `names_at(index)` inverse, from one declaration list. It is the right thing
//! for what it is for, which is saying **why a painted position looks the way
//! it does**, and this module's [`ByteMap::marks`] produces one rather than
//! competing with it.
//!
//! What it cannot carry is a *dissection*, and each gap is asserted in this
//! module's tests against a real `MarkSet` rather than claimed here:
//!
//! * **One index space.** A set states one `domain`, so a payload reassembled
//!   into a second buffer has no address at all.
//! * **Overlap is a paint rule, not an invariant.** Declaration order decides
//!   who wins, by design — so a child escaping its parent, or two siblings
//!   sharing bytes, is *accepted* and paints something plausible, and the
//!   stack `names_at` reports is the order the caller wrote rather than the
//!   containment it meant.
//! * **Two answers where there are three.** `names_at` is empty for a byte
//!   nobody claimed and equally empty for a byte past the end of the buffer.
//! * **No arm for a field with no bytes.** A derived value is expressed by
//!   *omitting* it, which makes "computed, so nothing lights" and "nobody
//!   declared this" the same answer.
//!
//! So the dissection is the model and the marks are derived from it, which is
//! the direction that cannot drift.
//!
//! ## One declaration, two directions
//!
//! A [`ByteMap`] is built from **one** list of [`FieldSpan`]s. Everything else
//! is derived from that list:
//!
//! * forward — [`extent_of`](ByteMap::extent_of) / [`selection_for`](ByteMap::selection_for),
//!   the bytes a field occupies, already shaped as the
//!   [`ByteSelection`] the hex view highlights;
//! * inverse — [`layers_at`](ByteMap::layers_at), the chain of fields covering
//!   one byte, outermost first, and [`owner_at`](ByteMap::owner_at), which is
//!   *defined as* the last link of that chain rather than computed a second
//!   way.
//!
//! The two directions are inverse by a law the module asserts rather than by
//! inspection: for every field that has bytes, `owner_at(selection_for(p))` is
//! `p` again. Deriving `owner_at` from `layers_at` is the same discipline
//! `hex_dump` used for cell↔byte — a second traversal is a second chance to
//! disagree.
//!
//! ## Why a source, and not just an offset
//!
//! A field's bytes are not always in the buffer on screen. A payload
//! reassembled from three fragments is a real field with a real length whose
//! bytes are in a *different* buffer from the frame that carried the last
//! fragment. A map that could address only one buffer would push exactly that
//! case back into the hand-kept second map, so a [`ByteSource`] is named and a
//! [`FieldOrigin::Bytes`] carries which one. Nesting is checked **within** a
//! source: a child in another source is not a violation, it is how reassembly
//! is expressed.
//!
//! A field with no bytes at all — a value resolved from a declaration seen in
//! an earlier message, a count the decoder computed — is [`FieldOrigin::Derived`].
//! It is a first-class arm, not a missing entry, because "this field has no
//! bytes" and "nobody declared this field" are different answers and a screen
//! that folds them lies about one of them.
//!
//! ## The invariants are checked once, at build
//!
//! [`ByteMap::build`] refuses a declaration that cannot be a dissection:
//! a duplicate path, an extent past the end of its source, a child escaping
//! its parent, or two unrelated fields sharing a byte. The refusals are typed
//! ([`MapDefect`]) and name the paths involved, because a decoder is
//! application code and the useful answer to a bad dissection is which two
//! fields disagree — not a panic, and not a silently wrong highlight.
//!
//! Checking is `O(n log n)`: one sort per source and a stack walk, plus a
//! nearest-declared-ancestor lookup per field.
//!
//! ## The AI-first witness (§2 #7)
//!
//! [`ByteMapExternal`] publishes both directions on the §5.12 `scene/query`
//! path: `field_paths`, `origin.<path>`, `extent.<path>`, `selection.<path>`
//! forward, and `coverage.<src>.<byte>`, `owner.<src>.<byte>`,
//! `layers.<src>.<byte>` inverse. An agent can ask "which field owns byte 12"
//! and "which bytes is `l1.sn`" without a pixel — see
//! `tools/demos/r1663_a_field_says_which_bytes.py`.
//!
//! Measured on the reference toolkit at 6.11.1: its mechanism for two views
//! sharing one selection **refuses** two views over different models
//! ("Trying to set a selection model, which works on a different model than the
//! view"), leaving the byte view with zero selected rows when a field is
//! picked; and zero of its 29 selection-model methods and zero of its 76
//! item-model methods name a byte extent. Relating a decoded field to its
//! bytes is entirely the application's there.

use std::collections::BTreeMap;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError, SchemaArg,
    SchemaField, query_proxy_external_impl,
};
use crate::marks::{Mark, MarkSet, domain};
use crate::reactive::{Owner, Signal};
use crate::widgets::hex_dump::ByteSelection;

/// R1663 §5.41 — a half-open run of bytes `[at, at + len)` inside one
/// [`ByteSource`].
///
/// Half-open because that is the shape every downstream reader wants: an empty
/// run is `len == 0` rather than an inverted pair, and adjacency is `a.end() ==
/// b.at()` with no off-by-one at the seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteExtent {
    at: usize,
    len: usize,
}

impl ByteExtent {
    /// The run of `len` bytes starting at `at`.
    #[must_use]
    pub const fn new(at: usize, len: usize) -> Self {
        Self { at, len }
    }

    /// The first byte of the run.
    #[must_use]
    pub const fn at(&self) -> usize {
        self.at
    }

    /// How many bytes the run holds. Zero is legal — a field can be declared
    /// present and empty (a zero-length option, a truncated tail) and that is
    /// different from having no bytes at all, which is
    /// [`FieldOrigin::Derived`].
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the run holds no bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// One past the last byte of the run.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.at + self.len
    }

    /// Whether `byte` is in the run. An empty run contains nothing.
    #[must_use]
    pub const fn contains(&self, byte: usize) -> bool {
        self.at <= byte && byte < self.end()
    }

    /// Whether this run covers all of `other`. An empty run is contained by any
    /// run whose bounds enclose its position, which is what lets a zero-length
    /// field sit inside its parent.
    #[must_use]
    pub const fn covers(&self, other: Self) -> bool {
        self.at <= other.at && other.end() <= self.end()
    }

    /// Whether the two runs share at least one byte.
    #[must_use]
    pub const fn intersects(&self, other: Self) -> bool {
        self.at < other.end() && other.at < self.end()
    }

    /// The run as the [`ByteSelection`] a hex view highlights, or `None` when
    /// the run is empty — a selection is never zero bytes, so an empty field
    /// honestly lights nothing.
    #[must_use]
    pub fn selection(&self) -> Option<ByteSelection> {
        (self.len > 0).then(|| ByteSelection::drag(self.at, self.end() - 1))
    }
}

/// R1663 §5.41 — which buffer a field's bytes are in.
///
/// An index into [`ByteMap::sources`]. Named rather than bare so a consumer
/// cannot pass a byte offset where a buffer was meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceId(u16);

impl SourceId {
    /// The source at `index`.
    #[must_use]
    pub const fn new(index: u16) -> Self {
        Self(index)
    }

    /// The index this id addresses.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// R1663 §5.41 — one named byte buffer a dissection can address.
///
/// The captured frame is one; a payload reassembled from several frames is
/// another. Both are addressable at once, which is the whole reason the id
/// exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByteSource {
    name: String,
    len: usize,
}

impl ByteSource {
    /// A source called `name` holding `len` bytes.
    #[must_use]
    pub fn new(name: impl Into<String>, len: usize) -> Self {
        Self {
            name: name.into(),
            len,
        }
    }

    /// The buffer's name, as a screen or an agent should say it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How many bytes the buffer holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the buffer is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// R1663 §5.41 — where a decoded field's value came from.
///
/// Two arms and not an `Option`, because the absent case has a meaning worth a
/// name: a field the decoder *computed* is not a field somebody forgot to map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldOrigin {
    /// The field was read from `extent` of `source`.
    Bytes {
        /// Which buffer.
        source: SourceId,
        /// Which run of it.
        extent: ByteExtent,
    },
    /// The field has no bytes of its own — it was derived (an id resolved
    /// against a declaration from an earlier message, a length the decoder
    /// summed, a verdict). Selecting it truthfully highlights nothing.
    Derived,
}

impl FieldOrigin {
    /// The run this field occupies, or `None` when it is [`Derived`](Self::Derived).
    #[must_use]
    pub const fn extent(&self) -> Option<ByteExtent> {
        match self {
            Self::Bytes { extent, .. } => Some(*extent),
            Self::Derived => None,
        }
    }

    /// Which buffer this field is in, or `None` when it is
    /// [`Derived`](Self::Derived).
    #[must_use]
    pub const fn source(&self) -> Option<SourceId> {
        match self {
            Self::Bytes { source, .. } => Some(*source),
            Self::Derived => None,
        }
    }

    /// The wire word for this arm — `"bytes"` or `"derived"`.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Bytes { .. } => "bytes",
            Self::Derived => "derived",
        }
    }
}

/// R1663 §5.27 §5.41 — one field of a dissection and where its value came
/// from.
///
/// `path` is the *same* stable path
/// [`dissect_row`](super::row_dissect::dissect_row) gives a tree node, and that
/// is deliberate: the path is the join key between the tree a person reads and
/// the bytes underneath it, so neither side needs an id the other has to keep
/// in step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSpan {
    path: String,
    origin: FieldOrigin,
}

impl FieldSpan {
    /// A field at `path` read from `extent` of `source`.
    #[must_use]
    pub fn bytes(path: impl Into<String>, source: SourceId, extent: ByteExtent) -> Self {
        Self {
            path: path.into(),
            origin: FieldOrigin::Bytes { source, extent },
        }
    }

    /// A field at `path` the decoder computed rather than read.
    #[must_use]
    pub fn derived(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            origin: FieldOrigin::Derived,
        }
    }

    /// The field's stable path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Where the field's value came from.
    #[must_use]
    pub const fn origin(&self) -> FieldOrigin {
        self.origin
    }

    /// How deep the path is — the number of segments. Used to order a covering
    /// chain, and reported so a consumer can indent without re-parsing.
    #[must_use]
    pub fn depth(&self) -> usize {
        path_depth(&self.path)
    }
}

/// R1663 §5.41 — what is at one byte address.
///
/// Three arms, because three things can be true and folding any two of them
/// makes a screen say something false: the address is outside the buffer, the
/// address is inside the buffer but no field claims it (padding, an
/// undissected tail), or a field owns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage<'a> {
    /// No such source, or the byte is past the end of that source.
    OutOfBuffer,
    /// Inside the buffer, claimed by no field.
    Unmapped,
    /// Owned by this field — the innermost of the chain covering the byte.
    Field(&'a FieldSpan),
}

impl Coverage<'_> {
    /// The wire word for this arm — `"out-of-buffer"`, `"unmapped"` or
    /// `"field"`.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::OutOfBuffer => "out-of-buffer",
            Self::Unmapped => "unmapped",
            Self::Field(_) => "field",
        }
    }

    /// The owning field's path, when a field owns the byte.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Field(span) => Some(span.path()),
            Self::OutOfBuffer | Self::Unmapped => None,
        }
    }
}

/// R1663 §5.41 — why a declaration is not a dissection.
///
/// Every arm names the paths involved. A decoder is application code, so the
/// useful answer to a bad dissection is *which two fields disagree*; a panic
/// says nothing and a silent accept paints a wrong highlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapDefect {
    /// Two spans declare the same path.
    Duplicate {
        /// The repeated path.
        path: String,
    },
    /// A span names a source the map does not have.
    UnknownSource {
        /// The offending field.
        path: String,
        /// The index it named.
        source: usize,
        /// How many sources the map has.
        sources: usize,
    },
    /// A span runs past the end of its source.
    PastEnd {
        /// The offending field.
        path: String,
        /// Where its run ends.
        end: usize,
        /// How long the source is.
        source_len: usize,
    },
    /// A child field's bytes are not inside its nearest declared ancestor's.
    Escapes {
        /// The child.
        path: String,
        /// The ancestor it escaped.
        parent: String,
    },
    /// Two fields that are not ancestor and descendant share a byte.
    Overlaps {
        /// One field.
        path: String,
        /// The other.
        other: String,
    },
}

impl core::fmt::Display for MapDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Duplicate { path } => write!(f, "duplicate field path `{path}`"),
            Self::UnknownSource {
                path,
                source,
                sources,
            } => write!(
                f,
                "field `{path}` names source {source} but the map has {sources}"
            ),
            Self::PastEnd {
                path,
                end,
                source_len,
            } => write!(
                f,
                "field `{path}` ends at {end}, past its {source_len}-byte source"
            ),
            Self::Escapes { path, parent } => {
                write!(f, "field `{path}` is not inside its parent `{parent}`")
            }
            Self::Overlaps { path, other } => write!(
                f,
                "fields `{path}` and `{other}` share bytes but neither contains the other"
            ),
        }
    }
}

impl std::error::Error for MapDefect {}

/// R1663 §5.41 — why a field cannot be turned into a byte highlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectDefect {
    /// No field is declared at that path.
    Undeclared,
    /// The field is [`FieldOrigin::Derived`] — it has no bytes to light.
    Derived,
    /// The field's run is empty, so there is no byte to select.
    Empty,
}

impl core::fmt::Display for SelectDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let what = match self {
            Self::Undeclared => "no field is declared at that path",
            Self::Derived => "the field was derived and has no bytes",
            Self::Empty => "the field's run is empty",
        };
        f.write_str(what)
    }
}

impl std::error::Error for SelectDefect {}

/// The nearest enclosing path — `a.b[0].c` -> `a.b[0]` -> `a.b` -> `a` -> None.
fn parent_path(path: &str) -> Option<&str> {
    if path.ends_with(']') {
        return path.rfind('[').map(|i| &path[..i]);
    }
    path.rfind('.').map(|i| &path[..i])
}

/// How many segments a path has. The root is depth 1.
fn path_depth(path: &str) -> usize {
    if path.is_empty() {
        return 0;
    }
    let mut depth = 1;
    let mut rest = path;
    while let Some(head) = parent_path(rest) {
        depth += 1;
        rest = head;
    }
    depth
}

/// R1663 §5.27 §5.41 — the relation between a dissection's fields and the
/// bytes they were decoded from.
///
/// Built once from one list of [`FieldSpan`]s; both directions are derived
/// from that list, so they cannot disagree. See the module documentation for
/// why the invariants are checked here rather than trusted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByteMap {
    sources: Vec<ByteSource>,
    spans: Vec<FieldSpan>,
    /// Per source, the indices of the spans in it, sorted so a covering chain
    /// is a stack walk: `at` ascending, then the longer run first.
    ordered: Vec<Vec<usize>>,
    /// Path -> index into `spans`, for the forward direction and for the
    /// nearest-declared-ancestor walk.
    by_path: BTreeMap<String, usize>,
}

impl ByteMap {
    /// Build the map, or refuse with the first [`MapDefect`] found.
    ///
    /// Refusals are ordered by how local they are — a duplicate path, then a
    /// bad source, then a run past the end, then containment, then overlap —
    /// so the first thing reported is the smallest thing to fix.
    ///
    /// # Errors
    ///
    /// Returns the defect that makes `spans` not a dissection of `sources`.
    pub fn build(sources: Vec<ByteSource>, spans: Vec<FieldSpan>) -> Result<Self, MapDefect> {
        let mut by_path: BTreeMap<String, usize> = BTreeMap::new();
        for (i, span) in spans.iter().enumerate() {
            if by_path.insert(span.path.clone(), i).is_some() {
                return Err(MapDefect::Duplicate {
                    path: span.path.clone(),
                });
            }
        }

        // Bounds: every run names a source it has and stays inside it.
        for span in &spans {
            let FieldOrigin::Bytes { source, extent } = span.origin else {
                continue;
            };
            let Some(buffer) = sources.get(source.index()) else {
                return Err(MapDefect::UnknownSource {
                    path: span.path.clone(),
                    source: source.index(),
                    sources: sources.len(),
                });
            };
            if extent.end() > buffer.len() {
                return Err(MapDefect::PastEnd {
                    path: span.path.clone(),
                    end: extent.end(),
                    source_len: buffer.len(),
                });
            }
        }

        // Containment: a child's bytes are inside its nearest declared
        // ancestor's — but only when both are in the same source, because a
        // field in another buffer is reassembly, not a violation.
        for span in &spans {
            let FieldOrigin::Bytes { source, extent } = span.origin else {
                continue;
            };
            let mut walk = parent_path(&span.path);
            while let Some(candidate) = walk {
                if let Some(&pi) = by_path.get(candidate) {
                    let parent = &spans[pi];
                    if let FieldOrigin::Bytes {
                        source: psource,
                        extent: pextent,
                    } = parent.origin
                    {
                        if psource == source && !pextent.covers(extent) {
                            return Err(MapDefect::Escapes {
                                path: span.path.clone(),
                                parent: parent.path.clone(),
                            });
                        }
                        if psource == source {
                            break;
                        }
                    }
                }
                walk = parent_path(candidate);
            }
        }

        // Laminarity: sorted by start with the longer run first, a covering
        // chain is a stack. Anything that neither nests nor separates overlaps,
        // and anything that nests without being kin overlaps too.
        let mut ordered: Vec<Vec<usize>> = vec![Vec::new(); sources.len()];
        for (i, span) in spans.iter().enumerate() {
            if let FieldOrigin::Bytes { source, .. } = span.origin {
                ordered[source.index()].push(i);
            }
        }
        for bucket in &mut ordered {
            bucket.sort_by_key(|&i| {
                let extent = span_extent(&spans[i]);
                (extent.at(), core::cmp::Reverse(extent.len()))
            });
            let mut stack: Vec<usize> = Vec::new();
            for &i in bucket.iter() {
                let extent = span_extent(&spans[i]);
                while let Some(&top) = stack.last() {
                    if span_extent(&spans[top]).end() <= extent.at() {
                        stack.pop();
                    } else {
                        break;
                    }
                }
                if let Some(&top) = stack.last() {
                    let outer = span_extent(&spans[top]);
                    let kin = is_ancestor(&spans[top].path, &spans[i].path);
                    if !outer.covers(extent) || !kin {
                        return Err(MapDefect::Overlaps {
                            path: spans[i].path.clone(),
                            other: spans[top].path.clone(),
                        });
                    }
                }
                stack.push(i);
            }
        }

        Ok(Self {
            sources,
            spans,
            ordered,
            by_path,
        })
    }

    /// The named buffers this dissection addresses.
    #[must_use]
    pub fn sources(&self) -> &[ByteSource] {
        &self.sources
    }

    /// Every declared field, in declaration order.
    #[must_use]
    pub fn fields(&self) -> &[FieldSpan] {
        &self.spans
    }

    /// The field declared at `path`, if any.
    #[must_use]
    pub fn field(&self, path: &str) -> Option<&FieldSpan> {
        self.by_path.get(path).map(|&i| &self.spans[i])
    }

    /// Where the field at `path` came from — `None` when nothing is declared
    /// there. [`FieldOrigin::Derived`] is the *answer* for a computed field,
    /// not the absence of one.
    #[must_use]
    pub fn origin_of(&self, path: &str) -> Option<FieldOrigin> {
        self.field(path).map(FieldSpan::origin)
    }

    /// The run of bytes the field at `path` occupies, with its source.
    #[must_use]
    pub fn extent_of(&self, path: &str) -> Option<(SourceId, ByteExtent)> {
        match self.origin_of(path)? {
            FieldOrigin::Bytes { source, extent } => Some((source, extent)),
            FieldOrigin::Derived => None,
        }
    }

    /// **Forward direction** — the highlight a hex view should show when the
    /// field at `path` is picked.
    ///
    /// # Errors
    ///
    /// [`SelectDefect`] when the path is undeclared, derived, or empty. Each
    /// is a different thing for a screen to say, which is why they are not one
    /// `None`.
    pub fn selection_for(&self, path: &str) -> Result<(SourceId, ByteSelection), SelectDefect> {
        let origin = self.origin_of(path).ok_or(SelectDefect::Undeclared)?;
        let FieldOrigin::Bytes { source, extent } = origin else {
            return Err(SelectDefect::Derived);
        };
        extent
            .selection()
            .map(|sel| (source, sel))
            .ok_or(SelectDefect::Empty)
    }

    /// **Inverse direction** — the chain of fields covering `byte` of `source`,
    /// outermost first.
    ///
    /// This is the layer stack a decode pane shows: the frame, then the
    /// transport header, then the field inside it. Empty when the byte is
    /// unmapped or out of the buffer — [`coverage_at`](Self::coverage_at) is
    /// what distinguishes those two.
    #[must_use]
    pub fn layers_at(&self, source: SourceId, byte: usize) -> Vec<&FieldSpan> {
        let Some(bucket) = self.ordered.get(source.index()) else {
            return Vec::new();
        };
        bucket
            .iter()
            .filter(|&&i| span_extent(&self.spans[i]).contains(byte))
            .map(|&i| &self.spans[i])
            .collect()
    }

    /// **Inverse direction** — the innermost field covering `byte` of
    /// `source`, *defined as* the last link of [`layers_at`](Self::layers_at)
    /// rather than found a second way.
    #[must_use]
    pub fn owner_at(&self, source: SourceId, byte: usize) -> Option<&FieldSpan> {
        self.layers_at(source, byte).pop()
    }

    /// What is at one byte address: outside the buffer, inside but unclaimed,
    /// or owned by a field.
    #[must_use]
    pub fn coverage_at(&self, source: SourceId, byte: usize) -> Coverage<'_> {
        match self.sources.get(source.index()) {
            Some(buffer) if byte < buffer.len() => self
                .owner_at(source, byte)
                .map_or(Coverage::Unmapped, Coverage::Field),
            _ => Coverage::OutOfBuffer,
        }
    }

    /// Every field whose bytes meet `selection` in `source`, in declaration
    /// order — the tree rows a drag across the hex pane should light.
    #[must_use]
    pub fn fields_touching(&self, source: SourceId, selection: ByteSelection) -> Vec<&FieldSpan> {
        let run = ByteExtent::new(selection.start(), selection.len());
        self.spans
            .iter()
            .filter(|span| match span.origin {
                FieldOrigin::Bytes {
                    source: s,
                    extent: e,
                } => s == source && e.intersects(run),
                FieldOrigin::Derived => false,
            })
            .collect()
    }

    /// Paths this map declares that the dissection tree does not have.
    ///
    /// The join key is the path, so a map entry with no node is a field a
    /// person can never select and an agent can never reach. It is reported
    /// rather than refused at build because the tree is rebuilt per row while
    /// the map is declared per decode, and a transient mismatch during a
    /// re-dissection is not a defect in either.
    #[must_use]
    pub fn unmatched<'a>(&self, tree_paths: impl IntoIterator<Item = &'a str>) -> Vec<&str> {
        let have: std::collections::BTreeSet<&str> = tree_paths.into_iter().collect();
        self.spans
            .iter()
            .map(FieldSpan::path)
            .filter(|p| !have.contains(p))
            .collect()
    }

    /// The paint decoration for one source, **derived** from the dissection.
    ///
    /// Every field in `source` becomes a [`Mark`] named by its path, declared
    /// outermost first so R1615's last-wins overlap rule puts the innermost
    /// field on top — which is the same order [`layers_at`](Self::layers_at)
    /// reports, from the same sort. A consumer that declared its own marks
    /// beside this map would be keeping the second copy this module exists to
    /// remove, so the marks are not something a caller writes.
    #[must_use]
    pub fn marks(&self, source: SourceId) -> MarkSet {
        let Some(bucket) = self.ordered.get(source.index()) else {
            return MarkSet::over(domain::BYTE);
        };
        MarkSet::from_marks(
            domain::BYTE,
            bucket.iter().filter_map(|&i| {
                let span = &self.spans[i];
                let extent = span.origin.extent()?;
                (!extent.is_empty()).then(|| Mark::new(span.path(), extent.at(), extent.end()))
            }),
        )
    }

    /// How many bytes of `source` no field claims.
    #[must_use]
    pub fn unmapped_bytes(&self, source: SourceId) -> usize {
        let Some(buffer) = self.sources.get(source.index()) else {
            return 0;
        };
        (0..buffer.len())
            .filter(|&b| self.owner_at(source, b).is_none())
            .count()
    }
}

/// The run of a span known to have one — only called after the `Derived` arm
/// has been filtered out.
fn span_extent(span: &FieldSpan) -> ByteExtent {
    span.origin.extent().unwrap_or(ByteExtent::new(0, 0))
}

/// Whether `outer` is a strict ancestor path of `inner`.
fn is_ancestor(outer: &str, inner: &str) -> bool {
    let mut walk = parent_path(inner);
    while let Some(candidate) = walk {
        if candidate == outer {
            return true;
        }
        walk = parent_path(candidate);
    }
    false
}

/// R1663 §5.41 — the map for the row currently being read, held so a new
/// decode swaps the whole relation at once.
///
/// **The map itself is not in the `Signal`, and that is deliberate.** A
/// [`ByteMap`] carries derived indices beside its declaration; a `Signal`
/// requires its payload to round-trip through serde, and a map that could be
/// *deserialized* could arrive with indices that do not match its own spans —
/// a second encoding of one state, which is the class of defect this module
/// exists to remove. So the map lives behind a `RefCell` and a `revision`
/// `Signal` is what a reactive reader subscribes to (the R1651 shape: a model
/// behind a `RefCell` is invisible to reactivity unless something published
/// alongside it changes).
///
/// One revision and not one per direction: the two directions are two reads of
/// one value, and a screen that could hold a new forward map beside an old
/// inverse one is exactly the drift being prevented.
pub struct ByteMapState {
    map: std::cell::RefCell<Rc<ByteMap>>,
    revision: Signal<u64>,
}

impl ByteMapState {
    /// Hold `map` as the current dissection.
    #[must_use]
    pub fn new(map: ByteMap) -> Self {
        Self {
            map: std::cell::RefCell::new(Rc::new(map)),
            revision: Signal::new(0),
        }
    }

    /// The current map. Reading subscribes to [`revision`](Self::revision), so
    /// a view that reads the map repaints when a new decode lands.
    #[must_use]
    pub fn map(&self) -> Rc<ByteMap> {
        let _ = self.revision.get();
        Rc::clone(&self.map.borrow())
    }

    /// How many times the dissection has been replaced.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    /// Replace the dissection — a different row is being read.
    pub fn set(&self, map: ByteMap) {
        *self.map.borrow_mut() = Rc::new(map);
        self.revision.set_with(|r| r.wrapping_add(1));
    }
}

impl core::fmt::Debug for ByteMapState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let map = self.map.borrow();
        f.debug_struct("ByteMapState")
            .field("fields", &map.fields().len())
            .field("sources", &map.sources().len())
            .finish_non_exhaustive()
    }
}

/// R1663 §5.41 — cache a [`ByteMapState`] on the current [`Owner`] under
/// `key`, so the view function and the External reach one instance.
///
/// # Panics
///
/// Panics when called outside an `Owner` scope, like every other `use_*` hook.
#[must_use]
pub fn use_byte_map(key: &'static str, build: impl FnOnce() -> ByteMap) -> Rc<ByteMapState> {
    Owner::current()
        .expect("use_byte_map requires an active Owner scope")
        .cache(key, || ByteMapState::new(build()))
}

/// R1663 §5.12 §5.41 — the field↔bytes relation on the wire.
///
/// Read-only by construction: the map is *derived* from a decode, and a wire
/// client that could rewrite it could make the screen disagree with the bytes
/// it is showing. What a client drives is the selection on the panes; what it
/// reads here is which selection goes with which field.
#[derive(Clone)]
pub struct ByteMapExternal {
    state: Rc<ByteMapState>,
}

impl core::fmt::Debug for ByteMapExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ByteMapExternal")
            .field("fields", &self.state.map().fields().len())
            .finish_non_exhaustive()
    }
}

impl ByteMapExternal {
    /// Publish the shared [`ByteMapState`] (from [`use_byte_map`]).
    #[must_use]
    pub fn new(state: Rc<ByteMapState>) -> Self {
        Self { state }
    }

    /// The shared state handle.
    #[must_use]
    pub fn state(&self) -> &Rc<ByteMapState> {
        &self.state
    }

    /// Split a `<src>.<byte>` tail into its two numbers.
    fn address(rest: &str) -> Option<(SourceId, usize)> {
        let (src, byte) = rest.split_once('.')?;
        Some((SourceId::new(src.parse().ok()?), byte.parse().ok()?))
    }
}

query_proxy_external_impl!(ByteMapExternal);

impl ExternalIntrospect for ByteMapExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("source_count", "int"),
                    SchemaField::new("field_count", "int"),
                    SchemaField::new("field_paths", "json"),
                    SchemaField::parametric(
                        "source_name.<src>",
                        "string",
                        const { &[SchemaArg::index("src", "source_count")] },
                    ),
                    SchemaField::parametric(
                        "source_len.<src>",
                        "int",
                        const { &[SchemaArg::index("src", "source_count")] },
                    ),
                    SchemaField::parametric(
                        "origin.<path>",
                        "string",
                        const { &[SchemaArg::key("path", "string", "field_paths")] },
                    ),
                    SchemaField::parametric(
                        "extent.<path>",
                        "json",
                        const { &[SchemaArg::key("path", "string", "field_paths")] },
                    ),
                    SchemaField::parametric(
                        "selection.<path>",
                        "json",
                        const { &[SchemaArg::key("path", "string", "field_paths")] },
                    ),
                    SchemaField::parametric(
                        "coverage.<src>.<byte>",
                        "string",
                        const {
                            &[
                                SchemaArg::index("src", "source_count"),
                                SchemaArg::open("byte", "int"),
                            ]
                        },
                    ),
                    SchemaField::parametric(
                        "owner.<src>.<byte>",
                        "string",
                        const {
                            &[
                                SchemaArg::index("src", "source_count"),
                                SchemaArg::open("byte", "int"),
                            ]
                        },
                    ),
                    SchemaField::parametric(
                        "layers.<src>.<byte>",
                        "json",
                        const {
                            &[
                                SchemaArg::index("src", "source_count"),
                                SchemaArg::open("byte", "int"),
                            ]
                        },
                    ),
                    SchemaField::parametric(
                        "unmapped_bytes.<src>",
                        "int",
                        const { &[SchemaArg::index("src", "source_count")] },
                    ),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let map = self.state.map();
        if let Some(rest) = path.strip_prefix("source_name.") {
            return Some(
                rest.parse::<usize>()
                    .ok()
                    .and_then(|i| map.sources().get(i))
                    .map_or(IntrospectValue::Null, |s| {
                        IntrospectValue::Text(s.name().to_owned())
                    }),
            );
        }
        if let Some(rest) = path.strip_prefix("source_len.") {
            return Some(
                rest.parse::<usize>()
                    .ok()
                    .and_then(|i| map.sources().get(i))
                    .map_or(IntrospectValue::Null, |s| {
                        IntrospectValue::Int(i64::try_from(s.len()).unwrap_or(i64::MAX))
                    }),
            );
        }
        if let Some(rest) = path.strip_prefix("unmapped_bytes.") {
            return Some(
                rest.parse::<u16>()
                    .ok()
                    .map_or(IntrospectValue::Null, |src| {
                        IntrospectValue::Int(
                            i64::try_from(map.unmapped_bytes(SourceId::new(src)))
                                .unwrap_or(i64::MAX),
                        )
                    }),
            );
        }
        if let Some(rest) = path.strip_prefix("origin.") {
            return Some(map.origin_of(rest).map_or(IntrospectValue::Null, |o| {
                IntrospectValue::Text(o.as_str().to_owned())
            }));
        }
        if let Some(rest) = path.strip_prefix("extent.") {
            return Some(map.extent_of(rest).map_or(IntrospectValue::Null, |(s, e)| {
                IntrospectValue::Json(json!({
                    "source": s.index(),
                    "at": e.at(),
                    "len": e.len(),
                }))
            }));
        }
        if let Some(rest) = path.strip_prefix("selection.") {
            return Some(
                map.selection_for(rest)
                    .map_or(IntrospectValue::Null, |(s, sel)| {
                        IntrospectValue::Json(json!({
                            "source": s.index(),
                            "start": sel.start(),
                            "end": sel.end(),
                        }))
                    }),
            );
        }
        if let Some(rest) = path.strip_prefix("coverage.") {
            return Some(Self::address(rest).map_or(IntrospectValue::Null, |(s, b)| {
                IntrospectValue::Text(map.coverage_at(s, b).as_str().to_owned())
            }));
        }
        if let Some(rest) = path.strip_prefix("owner.") {
            return Some(
                Self::address(rest)
                    .and_then(|(s, b)| map.owner_at(s, b))
                    .map_or(IntrospectValue::Null, |span| {
                        IntrospectValue::Text(span.path().to_owned())
                    }),
            );
        }
        if let Some(rest) = path.strip_prefix("layers.") {
            return Some(Self::address(rest).map_or(IntrospectValue::Null, |(s, b)| {
                IntrospectValue::Json(serde_json::Value::Array(
                    map.layers_at(s, b)
                        .into_iter()
                        .map(|span| serde_json::Value::String(span.path().to_owned()))
                        .collect(),
                ))
            }));
        }
        match path {
            "source_count" => Some(IntrospectValue::Int(
                i64::try_from(map.sources().len()).unwrap_or(i64::MAX),
            )),
            "field_count" => Some(IntrospectValue::Int(
                i64::try_from(map.fields().len()).unwrap_or(i64::MAX),
            )),
            "field_paths" => Some(IntrospectValue::Json(serde_json::Value::Array(
                map.fields()
                    .iter()
                    .map(|s| serde_json::Value::String(s.path().to_owned()))
                    .collect(),
            ))),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        // Everything here is derived from a decode. A client that could write
        // it could make the screen disagree with the bytes it is showing.
        if self.query(path).is_some() {
            Err(InterveneError::ReadOnly)
        } else {
            Err(InterveneError::UnknownPath)
        }
    }

    fn invoke(
        &mut self,
        _path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        Err(InvokeError::UnknownPath)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dissection the reference screen shows: a frame with three layers,
    /// a field inside the transport header, and a payload reassembled into a
    /// second buffer.
    fn sample() -> ByteMap {
        let frame = SourceId::new(0);
        let reassembled = SourceId::new(1);
        ByteMap::build(
            vec![
                ByteSource::new("frame", 0x48),
                ByteSource::new("reassembled payload", 3144),
            ],
            vec![
                FieldSpan::bytes("l0", frame, ByteExtent::new(0, 0x0c)),
                FieldSpan::bytes("l1", frame, ByteExtent::new(0x0c, 0x08)),
                FieldSpan::bytes("l1.sn", frame, ByteExtent::new(0x0c, 2)),
                FieldSpan::bytes("l1.frag", frame, ByteExtent::new(0x0e, 2)),
                FieldSpan::bytes("l3", frame, ByteExtent::new(0x14, 0x14)),
                FieldSpan::bytes("l3.key", frame, ByteExtent::new(0x14, 8)),
                FieldSpan::derived("l3.resolved"),
                FieldSpan::bytes("l3.payload", reassembled, ByteExtent::new(0, 3144)),
            ],
        )
        .expect("the sample dissection is well formed")
    }

    #[test]
    fn a_field_says_which_bytes_it_came_from() {
        let map = sample();
        let (source, extent) = map.extent_of("l1.sn").expect("sn has bytes");
        assert_eq!(source, SourceId::new(0));
        assert_eq!((extent.at(), extent.len()), (0x0c, 2));
        assert_eq!(extent.end(), 0x0e);
    }

    #[test]
    fn a_byte_says_which_field_owns_it() {
        let map = sample();
        let frame = SourceId::new(0);
        assert_eq!(
            map.owner_at(frame, 0x0c).map(FieldSpan::path),
            Some("l1.sn")
        );
        assert_eq!(
            map.owner_at(frame, 0x0d).map(FieldSpan::path),
            Some("l1.sn")
        );
        // One past the run is the sibling, not the same field.
        assert_eq!(
            map.owner_at(frame, 0x0e).map(FieldSpan::path),
            Some("l1.frag")
        );
    }

    /// The law the module exists for: the two directions are inverse.
    #[test]
    fn the_two_directions_are_inverse_for_every_field_with_bytes() {
        let map = sample();
        let mut checked = 0;
        for span in map.fields() {
            let Ok((source, selection)) = map.selection_for(span.path()) else {
                continue;
            };
            for byte in selection.start()..selection.end() {
                let owner = map
                    .owner_at(source, byte)
                    .expect("a selected byte has an owner");
                // The owner is the field itself or one of its descendants —
                // a parent's run covers its children's.
                assert!(
                    owner.path() == span.path() || is_ancestor(span.path(), owner.path()),
                    "byte {byte} of `{}` answered `{}`",
                    span.path(),
                    owner.path()
                );
            }
            // The innermost fields round-trip exactly.
            if map
                .fields()
                .iter()
                .all(|other| !is_ancestor(span.path(), other.path()))
            {
                assert_eq!(
                    map.owner_at(source, selection.focus()).map(FieldSpan::path),
                    Some(span.path()),
                    "leaf `{}` did not round-trip",
                    span.path()
                );
            }
            checked += 1;
        }
        assert_eq!(checked, 7, "every field with bytes was checked");
    }

    #[test]
    fn owner_is_the_last_link_of_the_layer_chain() {
        let map = sample();
        let frame = SourceId::new(0);
        for byte in 0..0x48 {
            let chain = map.layers_at(frame, byte);
            assert_eq!(
                chain.last().map(|s| s.path()),
                map.owner_at(frame, byte).map(FieldSpan::path),
                "byte {byte}"
            );
        }
    }

    #[test]
    fn the_layer_chain_runs_outermost_first() {
        let map = sample();
        let chain: Vec<&str> = map
            .layers_at(SourceId::new(0), 0x0c)
            .into_iter()
            .map(FieldSpan::path)
            .collect();
        assert_eq!(chain, vec!["l1", "l1.sn"]);
        // Each link covers the next.
        let spans: Vec<ByteExtent> = chain
            .iter()
            .map(|p| map.extent_of(p).expect("chain links have bytes").1)
            .collect();
        assert!(spans[0].covers(spans[1]));
    }

    #[test]
    fn three_answers_at_a_byte_address_stay_three() {
        let map = sample();
        let frame = SourceId::new(0);
        assert_eq!(map.coverage_at(frame, 0x0c).as_str(), "field");
        // 0x28..0x48 is inside the frame and no field claims it.
        assert_eq!(map.coverage_at(frame, 0x30).as_str(), "unmapped");
        assert_eq!(map.coverage_at(frame, 0x48).as_str(), "out-of-buffer");
        assert_eq!(
            map.coverage_at(SourceId::new(9), 0).as_str(),
            "out-of-buffer"
        );
    }

    #[test]
    fn a_derived_field_is_not_a_missing_one() {
        let map = sample();
        assert_eq!(map.origin_of("l3.resolved"), Some(FieldOrigin::Derived));
        assert_eq!(map.origin_of("l3.nothing"), None);
        assert_eq!(
            map.selection_for("l3.resolved"),
            Err(SelectDefect::Derived),
            "a derived field refuses with its own word"
        );
        assert_eq!(
            map.selection_for("l3.nothing"),
            Err(SelectDefect::Undeclared)
        );
    }

    #[test]
    fn a_field_in_another_source_is_reassembly_not_a_violation() {
        let map = sample();
        let (source, extent) = map.extent_of("l3.payload").expect("payload has bytes");
        assert_eq!(source, SourceId::new(1));
        assert_eq!(extent.len(), 3144);
        // Its parent `l3` is 0x14 bytes of the frame, which does not cover
        // 3,144 bytes — and that is accepted because they are different
        // buffers.
        assert_eq!(map.extent_of("l3").expect("l3 has bytes").1.len(), 0x14);
        assert_eq!(
            map.owner_at(SourceId::new(1), 3143).map(FieldSpan::path),
            Some("l3.payload")
        );
    }

    #[test]
    fn a_selection_over_the_hex_pane_names_every_field_it_meets() {
        let map = sample();
        let touched: Vec<&str> = map
            .fields_touching(SourceId::new(0), ByteSelection::drag(0x0c, 0x15))
            .into_iter()
            .map(FieldSpan::path)
            .collect();
        assert_eq!(touched, vec!["l1", "l1.sn", "l1.frag", "l3", "l3.key"]);
    }

    #[test]
    fn an_empty_run_lights_nothing_and_says_so() {
        let map = ByteMap::build(
            vec![ByteSource::new("frame", 8)],
            vec![
                FieldSpan::bytes("head", SourceId::new(0), ByteExtent::new(0, 4)),
                FieldSpan::bytes("head.opt", SourceId::new(0), ByteExtent::new(2, 0)),
            ],
        )
        .expect("a zero-length option is well formed");
        assert!(map.extent_of("head.opt").expect("declared").1.is_empty());
        assert_eq!(map.selection_for("head.opt"), Err(SelectDefect::Empty));
        // No byte belongs to it — its parent still owns byte 2.
        assert_eq!(
            map.owner_at(SourceId::new(0), 2).map(FieldSpan::path),
            Some("head")
        );
    }

    /// The marks a painter obeys come **out of** the dissection, so the ink and
    /// the model cannot disagree.
    #[test]
    fn the_paint_marks_are_derived_from_the_dissection() {
        let map = sample();
        let marks = map.marks(SourceId::new(0));
        assert_eq!(marks.domain(), domain::BYTE);
        // Only the frame's fields, and the derived one contributes nothing.
        let names: Vec<&str> = marks.iter().map(Mark::name).collect();
        assert_eq!(names, vec!["l0", "l1", "l1.sn", "l1.frag", "l3", "l3.key"]);
        // Declared outermost first, so last-wins paint puts the leaf on top —
        // the SAME order the layer chain reports, from the same sort.
        assert_eq!(marks.top_at(0x0c).map(Mark::name), Some("l1.sn"));
        assert_eq!(
            marks.names_at(0x0c),
            map.layers_at(SourceId::new(0), 0x0c)
                .into_iter()
                .map(FieldSpan::path)
                .collect::<Vec<_>>()
        );
        // The second source gets its own set, which is the thing one MarkSet
        // cannot hold.
        let payload = map.marks(SourceId::new(1));
        assert_eq!(
            payload.iter().map(Mark::name).collect::<Vec<_>>(),
            vec!["l3.payload"]
        );
    }

    /// ★ Why this module exists beside [`MarkSet`] — each gap measured against
    /// a real one rather than asserted in prose.
    #[test]
    fn a_markset_alone_cannot_express_a_dissection() {
        // (1) A malformed dissection is ACCEPTED by a mark set: `b` escapes
        // `a` and the set paints it happily.
        let loose = MarkSet::over(domain::BYTE)
            .marking("a", 0, 4)
            .marking("a.b", 3, 8);
        assert_eq!(loose.names_at(5), vec!["a.b"]);
        assert_eq!(
            ByteMap::build(
                vec![ByteSource::new("f", 16)],
                vec![
                    FieldSpan::bytes("a", SourceId::new(0), ByteExtent::new(0, 4)),
                    FieldSpan::bytes("a.b", SourceId::new(0), ByteExtent::new(3, 5)),
                ],
            ),
            Err(MapDefect::Escapes {
                path: "a.b".to_owned(),
                parent: "a".to_owned(),
            }),
            "the same declaration is refused as a dissection"
        );

        // (2) The reported stack is DECLARATION order, so declaring the child
        // first reports the parent as innermost — a lie about containment that
        // the sorted chain cannot tell.
        let misordered = MarkSet::over(domain::BYTE)
            .marking("a.b", 0, 2)
            .marking("a", 0, 8);
        assert_eq!(misordered.top_at(1).map(Mark::name), Some("a"));
        let sorted = ByteMap::build(
            vec![ByteSource::new("f", 16)],
            vec![
                FieldSpan::bytes("a.b", SourceId::new(0), ByteExtent::new(0, 2)),
                FieldSpan::bytes("a", SourceId::new(0), ByteExtent::new(0, 8)),
            ],
        )
        .expect("declaration order is not the model's business");
        assert_eq!(
            sorted.owner_at(SourceId::new(0), 1).map(FieldSpan::path),
            Some("a.b"),
            "containment decides the innermost, not the order somebody wrote"
        );

        // (3) Two answers where there are three: unmapped and past-the-end are
        // the same empty list.
        let short = MarkSet::over(domain::BYTE).marking("a", 0, 2);
        assert!(short.names_at(5).is_empty());
        assert!(short.names_at(9_999).is_empty());
        let map = sample();
        assert_ne!(
            map.coverage_at(SourceId::new(0), 0x30).as_str(),
            map.coverage_at(SourceId::new(0), 0x9999).as_str()
        );

        // (4) A derived field can only be expressed by omission, which makes it
        // indistinguishable from an undeclared one.
        assert!(short.get("computed").is_none());
        assert!(short.get("never-declared").is_none());
        assert_ne!(map.origin_of("l3.resolved"), map.origin_of("l3.nothing"));
    }

    #[test]
    fn a_duplicate_path_is_refused_by_name() {
        let err = ByteMap::build(
            vec![ByteSource::new("f", 8)],
            vec![
                FieldSpan::bytes("a", SourceId::new(0), ByteExtent::new(0, 2)),
                FieldSpan::bytes("a", SourceId::new(0), ByteExtent::new(4, 2)),
            ],
        )
        .expect_err("two fields cannot share a path");
        assert_eq!(
            err,
            MapDefect::Duplicate {
                path: "a".to_owned()
            }
        );
    }

    #[test]
    fn a_run_past_the_end_is_refused_with_both_numbers() {
        let err = ByteMap::build(
            vec![ByteSource::new("f", 8)],
            vec![FieldSpan::bytes(
                "a",
                SourceId::new(0),
                ByteExtent::new(6, 4),
            )],
        )
        .expect_err("a run cannot leave its source");
        assert_eq!(
            err,
            MapDefect::PastEnd {
                path: "a".to_owned(),
                end: 10,
                source_len: 8,
            }
        );
        assert!(err.to_string().contains("past its 8-byte source"));
    }

    #[test]
    fn an_unknown_source_is_refused_with_the_count() {
        let err = ByteMap::build(
            vec![ByteSource::new("f", 8)],
            vec![FieldSpan::bytes(
                "a",
                SourceId::new(3),
                ByteExtent::new(0, 1),
            )],
        )
        .expect_err("a field cannot name a source that is not there");
        assert_eq!(
            err,
            MapDefect::UnknownSource {
                path: "a".to_owned(),
                source: 3,
                sources: 1,
            }
        );
    }

    #[test]
    fn a_child_escaping_its_parent_names_both() {
        let err = ByteMap::build(
            vec![ByteSource::new("f", 16)],
            vec![
                FieldSpan::bytes("hdr", SourceId::new(0), ByteExtent::new(0, 4)),
                FieldSpan::bytes("hdr.sn", SourceId::new(0), ByteExtent::new(3, 4)),
            ],
        )
        .expect_err("a child cannot leave its parent");
        assert_eq!(
            err,
            MapDefect::Escapes {
                path: "hdr.sn".to_owned(),
                parent: "hdr".to_owned(),
            }
        );
    }

    #[test]
    fn two_unrelated_fields_sharing_a_byte_are_refused() {
        let err = ByteMap::build(
            vec![ByteSource::new("f", 16)],
            vec![
                FieldSpan::bytes("a", SourceId::new(0), ByteExtent::new(0, 6)),
                FieldSpan::bytes("b", SourceId::new(0), ByteExtent::new(4, 6)),
            ],
        )
        .expect_err("siblings cannot share bytes");
        assert_eq!(
            err,
            MapDefect::Overlaps {
                path: "b".to_owned(),
                other: "a".to_owned(),
            }
        );
    }

    /// Nesting without kinship is an overlap too — the stack walk would accept
    /// it as a chain if the ancestry were not also checked, and then
    /// `layers_at` would report a "layer stack" of two unrelated fields.
    #[test]
    fn a_field_nested_inside_a_stranger_is_refused() {
        let err = ByteMap::build(
            vec![ByteSource::new("f", 16)],
            vec![
                FieldSpan::bytes("a", SourceId::new(0), ByteExtent::new(0, 8)),
                FieldSpan::bytes("b", SourceId::new(0), ByteExtent::new(2, 2)),
            ],
        )
        .expect_err("`b` is not a child of `a`");
        assert_eq!(
            err,
            MapDefect::Overlaps {
                path: "b".to_owned(),
                other: "a".to_owned(),
            }
        );
    }

    #[test]
    fn paths_nest_through_array_indices() {
        assert_eq!(parent_path("a.b[0].c"), Some("a.b[0]"));
        assert_eq!(parent_path("a.b[0]"), Some("a.b"));
        assert_eq!(parent_path("a.b"), Some("a"));
        assert_eq!(parent_path("a"), None);
        assert_eq!(path_depth("a.b[0].c"), 4);
        assert_eq!(path_depth("a"), 1);
        assert!(is_ancestor("a.b", "a.b[0].c"));
        assert!(!is_ancestor("a.b", "a.bc"));
        assert!(!is_ancestor("a.b", "a.b"));
    }

    #[test]
    fn an_index_element_nests_inside_its_array() {
        let map = ByteMap::build(
            vec![ByteSource::new("f", 16)],
            vec![
                FieldSpan::bytes("opts", SourceId::new(0), ByteExtent::new(0, 8)),
                FieldSpan::bytes("opts[0]", SourceId::new(0), ByteExtent::new(0, 4)),
                FieldSpan::bytes("opts[1]", SourceId::new(0), ByteExtent::new(4, 4)),
            ],
        )
        .expect("bracket paths nest");
        assert_eq!(
            map.owner_at(SourceId::new(0), 5).map(FieldSpan::path),
            Some("opts[1]")
        );
    }

    #[test]
    fn a_map_entry_with_no_tree_node_is_reported() {
        let map = sample();
        let tree = [
            "l0",
            "l1",
            "l1.sn",
            "l1.frag",
            "l3",
            "l3.key",
            "l3.resolved",
        ];
        assert_eq!(map.unmatched(tree), vec!["l3.payload"]);
        // Everything matched is the empty answer, not a missing one.
        let all: Vec<&str> = map.fields().iter().map(FieldSpan::path).collect();
        assert!(map.unmatched(all).is_empty());
    }

    #[test]
    fn unmapped_bytes_counts_what_no_field_claims() {
        let map = sample();
        // 0x00..0x28 is claimed by l0 / l1 / l3; the rest of the 0x48 frame is
        // not.
        assert_eq!(map.unmapped_bytes(SourceId::new(0)), 0x48 - 0x28);
        assert_eq!(map.unmapped_bytes(SourceId::new(1)), 0);
    }

    #[test]
    fn the_external_answers_both_directions() {
        let ext = ByteMapExternal::new(Rc::new(ByteMapState::new(sample())));
        assert_eq!(ext.query("field_count").and_then(|v| v.as_i64()), Some(8));
        assert_eq!(ext.query("source_count").and_then(|v| v.as_i64()), Some(2));
        assert_eq!(
            ext.query("source_name.1")
                .and_then(|v| v.as_str().map(str::to_owned)),
            Some("reassembled payload".to_owned())
        );
        // Forward.
        assert_eq!(
            ext.query("origin.l1.sn")
                .and_then(|v| v.as_str().map(str::to_owned)),
            Some("bytes".to_owned())
        );
        assert_eq!(
            ext.query("origin.l3.resolved")
                .and_then(|v| v.as_str().map(str::to_owned)),
            Some("derived".to_owned())
        );
        let extent = ext.query("extent.l1.sn").expect("declared");
        assert_eq!(
            extent,
            IntrospectValue::Json(json!({"source": 0, "at": 12, "len": 2}))
        );
        let selection = ext.query("selection.l1.sn").expect("declared");
        assert_eq!(
            selection,
            IntrospectValue::Json(json!({"source": 0, "start": 12, "end": 14}))
        );
        // Inverse.
        assert_eq!(
            ext.query("owner.0.13")
                .and_then(|v| v.as_str().map(str::to_owned)),
            Some("l1.sn".to_owned())
        );
        // The wire speaks decimal: the frame is 0x48 == 72 bytes, so 72 is the
        // first address past it and 48 is still inside (and unmapped).
        assert_eq!(
            ext.query("coverage.0.72")
                .and_then(|v| v.as_str().map(str::to_owned)),
            Some("out-of-buffer".to_owned())
        );
        assert_eq!(
            ext.query("coverage.0.48")
                .and_then(|v| v.as_str().map(str::to_owned)),
            Some("unmapped".to_owned())
        );
        assert_eq!(
            ext.query("layers.0.12"),
            Some(IntrospectValue::Json(json!(["l1", "l1.sn"])))
        );
        assert_eq!(ext.query("no_such_path"), None);
    }

    /// Every declared path answers, and every answering path is declared —
    /// the census both ways, so a reader added without a declaration and a
    /// declaration without a reader are both build failures for the demo.
    #[test]
    fn every_declared_path_answers_and_every_answer_is_declared() {
        let ext = ByteMapExternal::new(Rc::new(ByteMapState::new(sample())));
        let schema = ext.schema();
        let mut probed = 0;
        for field in schema.fields {
            let probe = match field.path {
                "source_name.<src>" | "source_len.<src>" | "unmapped_bytes.<src>" => {
                    field.path.replace("<src>", "0")
                }
                "origin.<path>" | "extent.<path>" | "selection.<path>" => {
                    field.path.replace("<path>", "l1.sn")
                }
                "coverage.<src>.<byte>" | "owner.<src>.<byte>" | "layers.<src>.<byte>" => {
                    field.path.replace("<src>", "0").replace("<byte>", "12")
                }
                plain => plain.to_owned(),
            };
            assert!(
                ext.query(&probe).is_some(),
                "declared path `{}` did not answer at `{probe}`",
                field.path
            );
            probed += 1;
        }
        assert_eq!(probed, 12, "every declared path was probed");
    }

    #[test]
    fn the_relation_is_read_only_on_the_wire() {
        let mut ext = ByteMapExternal::new(Rc::new(ByteMapState::new(sample())));
        assert_eq!(
            ext.intervene("field_count", IntrospectValue::Int(3)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            ext.intervene("no_such_path", IntrospectValue::Int(3)),
            Err(InterveneError::UnknownPath)
        );
        assert_eq!(
            ext.invoke("select", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn a_new_decode_swaps_the_whole_relation() {
        let state = Rc::new(ByteMapState::new(sample()));
        assert_eq!(state.map().fields().len(), 8);
        state.set(
            ByteMap::build(
                vec![ByteSource::new("frame", 4)],
                vec![FieldSpan::bytes(
                    "only",
                    SourceId::new(0),
                    ByteExtent::new(0, 4),
                )],
            )
            .expect("well formed"),
        );
        assert_eq!(state.map().fields().len(), 1);
        assert_eq!(
            state
                .map()
                .owner_at(SourceId::new(0), 0)
                .map(FieldSpan::path),
            Some("only")
        );
    }
}
