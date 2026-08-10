//! `scene/draw_profile` RPC method — R1557 R1558 §5.16 §5.18 §5.7 §2 #2 §2 #7.
//!
//! Publishes [`pinion_runtime::DrawProfile`]: which subtree of the painted
//! scene drew the frame, in the units a 2D vector renderer is charged in —
//! for the whole window, or for the one subtree the caller named.
//!
//! # Why a method of its own
//!
//! `scene/frame_timings` already answers `last.draw` — the frame's whole draw
//! census (R1556). It is one number per frame, so it says *how much* and never
//! *where*, and two frames costing four thousand glyphs in different subtrees
//! read identically while wanting opposite fixes.
//!
//! It is also an axis that costs a **walk**: attributing the frame means
//! re-encoding the retained paint scene with a profiler attached.
//! `scene/frame_timings` is answered from a ring the paint already filled and
//! costs nothing; this cannot be, and a separate method is billed only when
//! asked. The same split `scene/memory` made for the same reason.
//!
//! # `path` scopes the measurement; `depth` scopes the reply (R1558)
//!
//! They are opposite axes and the reply states which one acted. `depth` trims a
//! measurement that was taken in full: the whole window is re-encoded, the
//! ranking still reads every row, and `nodes_total` keeps the profile's real
//! size while `children_omitted` says where the tree was cut. `path` does not
//! trim anything — it re-encodes **only** the addressed subtree, so
//! `nodes_total` itself shrinks and the cost of asking falls with it.
//!
//! That is what makes the profiler usable as a bisection tool on a scene worth
//! profiling. Reading the root, then drilling into the heaviest child, then
//! into its heaviest child, costs three profiles of rapidly shrinking subtrees
//! rather than three re-encodes of the window — and the address to drill with
//! is the one the previous reply already handed back.
//!
//! Scoping is only trustworthy because a subtree's draw work is **independent
//! of the context it is drawn in** — see [`pinion_runtime::draw_profile`] for
//! why that holds of the encoder, and `paint_adapter`'s tests plus this
//! method's demo for the two places it is asserted rather than assumed.
//!
//! # Wire form
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "scene/draw_profile",
//!   "params": {"path": "/window[main]/grid", "depth": 2,
//!              "heaviest_by": "glyphs"},
//!   "id": 1
//! }
//! ```
//!
//! answers with
//!
//! ```json
//! {
//!   "root": {
//!     "path": "/window[main]/grid",
//!     "segment": "grid",
//!     "kind": "Container",
//!     "tag": "grid",
//!     "total": {"draws": 812, "paths": 400, "path_segments": 2100,
//!               "layers": 3, "glyph_runs": 120, "glyphs": 4210},
//!     "own": {"draws": 1, "paths": 1, "path_segments": 4,
//!             "layers": 0, "glyph_runs": 0, "glyphs": 0},
//!     "children": [ ... ],
//!     "children_omitted": 0
//!   },
//!   "path": "/window[main]/grid",
//!   "nodes": 37,
//!   "nodes_total": 1204,
//!   "depth": 2,
//!   "heaviest_by": "glyphs",
//!   "heaviest": [
//!     {"path": "/window[main]/grid/rows/12", "kind": "Text",
//!      "tag": null, "own": { ... }}
//!   ]
//! }
//! ```
//!
//! # Reading it
//!
//! - `total` is **inclusive** (the node and its whole subtree); `own` is
//!   **exclusive** (the node alone). Summing `own` over every node gives the
//!   root's `total` exactly — the attribution is a partition, not a set of
//!   overlapping estimates.
//! - A row's `path` is the address `scene/locate` produces and `scene/query` /
//!   `scene/invoke` resolve (all go through `Scene::lookup_path_ref`, which
//!   takes the same tag-or-index segments and is likewise transparent to a
//!   `Scroll`). "Which subtree is expensive", "profile just that subtree" and
//!   "act on it" are the same string. Since R1558 this method is itself a
//!   reader of that address; the limit that remains is that a row's address
//!   still has no *general* reader — `scene/query` and `scene/invoke` resolve
//!   one in order to reach an `External` on it, `scene/snapshot` takes a window
//!   selector with no scene tail, and `scene/layout` addresses its own nodes by
//!   bare index (its `path` filter is an accepted-and-ignored R47.7.x carry).
//! - The reply's top-level `path` echoes the scope that was asked for, verbatim
//!   and `null` when none was, so a caller can tell a scoped answer from a
//!   whole-window one without comparing addresses.
//! - `nodes` counts the rows in this reply; `nodes_total` counts what was
//!   **measured**. `depth` moves the first and not the second; `path` moves
//!   both. Every pruned node's parent says so in `children_omitted` —
//!   truncation is never silent, and neither is scoping.
//! - `heaviest` ranks by `own` in the **named** unit `heaviest_by`, never by a
//!   weight this crate invented. What a glyph or a path segment costs moves
//!   with the GPU, the driver and the resolution, so a single "heaviest" scalar
//!   would be a made-up number wearing an objective face. Omitted when the
//!   caller names no unit.
//! - `root` is `null` only for a window that has never painted.

use pinion_runtime::{DrawProfile, DrawProfileNode, DrawWork};
use serde::Serialize;

/// The default number of rows [`draw_profile`] ranks when a caller names a
/// `heaviest_by` unit but no `limit`.
///
/// Ten because a ranking is read by a human or steered by an agent, and neither
/// acts on the eleventh row before acting on the first. A caller that wants the
/// whole ordering asks for it; the tree is already the whole census.
const DEFAULT_HEAVIEST_LIMIT: usize = 10;

/// Typed errors the [`draw_profile`] dispatcher can return. The variant name
/// rides in `error.data` so an agent pattern-matches rather than parsing prose.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawProfileError {
    /// The embedder installed no profile on the dispatch context.
    ///
    /// A host with no live window — the headless single-window dispatch entry,
    /// or a `scene/draw_profile` for a window id that has never painted.
    DrawProfileUnavailable,
    /// `heaviest_by` named something that is not a [`DrawWork`] field.
    ///
    /// Carries the offending name AND the valid set, so the message names the
    /// reason, echoes what was rejected and teaches what would have worked —
    /// the R1388 recovery-from-the-message-alone shape.
    UnknownUnit {
        /// The unit the caller asked to rank by.
        requested: String,
        /// Every unit that would have worked, in wire order.
        valid: &'static [&'static str],
    },
    /// `depth` or `limit` was present but not a non-negative integer.
    MalformedParam {
        /// Which parameter.
        param: &'static str,
    },
    /// (R1558) `path`'s `/window[id]/` prefix was malformed or empty.
    ///
    /// The two SYNTAX failures only — whether the name is a window is
    /// [`Self::UnknownWindow`], judged against a registry this crate does not
    /// hold. Forwarded rather than collapsed into one blanket tag, the
    /// concrete-reason discipline every path-taking method here follows.
    Path(crate::path::PathError),
    /// (R1558) `path` parsed cleanly and reached no node in the painted
    /// scene: an out-of-range index, an unknown tag, or a descent through a
    /// leaf.
    ///
    /// Carries what was asked for, because a profile scope is typically a
    /// string the caller copied out of an earlier reply and the useful
    /// question is *which* address stopped resolving.
    UnknownPath {
        /// The address the caller sent, verbatim.
        requested: String,
    },
    /// (R1558) `path`'s prefix named no window this host has open.
    ///
    /// Judged by the embedder against its **live** window slots — the registry
    /// `{window: "<id>"}` is judged against and the one a profile actually
    /// comes from — rather than by [`crate::path::resolve`] against the SCE
    /// topology, which a binding can differ from by opening a second
    /// `WindowSpec` without a second `AppState`. Under the older rule
    /// `scene/draw_profile` published `/window[inspector]/…` rows that no
    /// path-taking method could read back.
    UnknownWindow {
        /// The window id the address named.
        requested: String,
        /// Every window this host has open, in slot order.
        valid: Vec<String>,
    },
    /// (R1558) `path` named one window and the frame's `window` scope named
    /// another.
    ///
    /// The two are not merged and neither silently wins. A request that says
    /// two different things about which window it means is a caller bug, and
    /// answering it with a profile of *either* window is how that bug survives
    /// into a dashboard.
    WindowMismatch {
        /// The window the address's prefix named.
        path_window: String,
        /// The window `params.window` named.
        scope_window: String,
    },
}

impl DrawProfileError {
    /// The machine-matchable tag carried in a failing response's `error.data`.
    #[must_use]
    pub fn wire_tag(&self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::DrawProfileUnavailable => std::borrow::Cow::Borrowed("DrawProfileUnavailable"),
            Self::UnknownUnit { requested, valid } => std::borrow::Cow::Owned(format!(
                "UnknownUnit: {requested:?} (valid: {})",
                valid.join(", ")
            )),
            Self::MalformedParam { param } => {
                std::borrow::Cow::Owned(format!("MalformedParam: {param:?}"))
            }
            Self::Path(err) => err.wire_tag(),
            Self::UnknownWindow { requested, valid } => std::borrow::Cow::Owned(format!(
                "UnknownWindow: {requested:?} (valid: {})",
                valid.join(", ")
            )),
            Self::UnknownPath { requested } => {
                std::borrow::Cow::Owned(format!("UnknownPath: {requested:?}"))
            }
            Self::WindowMismatch {
                path_window,
                scope_window,
            } => std::borrow::Cow::Owned(format!(
                "WindowMismatch: path names {path_window:?}, window names {scope_window:?}"
            )),
        }
    }
}

impl From<crate::path::PathError> for DrawProfileError {
    fn from(err: crate::path::PathError) -> Self {
        Self::Path(err)
    }
}

impl std::fmt::Display for DrawProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DrawProfileUnavailable => f.write_str("draw profile unavailable for this window"),
            Self::UnknownUnit { requested, valid } => write!(
                f,
                "heaviest_by {requested:?} is not a draw-work unit (valid: {})",
                valid.join(", ")
            ),
            Self::MalformedParam { param } => {
                write!(f, "{param} must be a non-negative integer")
            }
            // `PathError` has no `Display`; `wire_tag` is this codebase's SSOT
            // for naming its three reasons, so the prose and the machine tag
            // cannot drift apart into two descriptions of one failure.
            Self::Path(err) => write!(f, "path prefix: {}", err.wire_tag()),
            Self::UnknownWindow { requested, valid } => write!(
                f,
                "path names window {requested:?}, which is not open (open: {})",
                valid.join(", ")
            ),
            Self::UnknownPath { requested } => {
                write!(f, "path {requested:?} reaches no node in the painted scene")
            }
            Self::WindowMismatch {
                path_window,
                scope_window,
            } => write!(
                f,
                "path names window {path_window:?} but params.window names {scope_window:?}"
            ),
        }
    }
}

/// One [`DrawWork`] field, named on the wire — the units a caller may rank by.
///
/// The census of them, so `heaviest_by`'s valid set is derived from the type
/// rather than hand-listed beside it: a field added to [`DrawWork`] fails
/// [`Unit::of_wire`]'s match here, where it must be given a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, pinion_derive::VariantCensus)]
#[variant_census(all)]
pub enum Unit {
    /// [`DrawWork::draws`].
    Draws,
    /// [`DrawWork::paths`].
    Paths,
    /// [`DrawWork::path_segments`].
    PathSegments,
    /// [`DrawWork::layers`].
    Layers,
    /// [`DrawWork::glyph_runs`].
    GlyphRuns,
    /// [`DrawWork::glyphs`].
    Glyphs,
}

impl Unit {
    /// The census, in the order the fields are declared.
    pub const ALL: [Self; 6] = [
        Self::Draws,
        Self::Paths,
        Self::PathSegments,
        Self::Layers,
        Self::GlyphRuns,
        Self::Glyphs,
    ];

    /// Every unit's wire name — what `heaviest_by` accepts, and what a
    /// rejection echoes back as the valid set.
    pub const NAMES: &'static [&'static str] = &[
        "draws",
        "paths",
        "path_segments",
        "layers",
        "glyph_runs",
        "glyphs",
    ];

    /// The wire name of this unit.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Draws => "draws",
            Self::Paths => "paths",
            Self::PathSegments => "path_segments",
            Self::Layers => "layers",
            Self::GlyphRuns => "glyph_runs",
            Self::Glyphs => "glyphs",
        }
    }

    /// Parse a wire name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|u| u.name() == name)
    }

    /// Read this unit out of a census — the projection the ranking sorts on.
    ///
    /// One reader, over the WIRE census, because that is the value the ranking
    /// actually holds. A second reader over [`DrawWork`] would be a second
    /// field-to-unit mapping with nothing forcing it to agree with this one.
    #[must_use]
    pub const fn of_wire(self, work: DrawProfileWork) -> u32 {
        match self {
            Self::Draws => work.draws,
            Self::Paths => work.paths,
            Self::PathSegments => work.path_segments,
            Self::Layers => work.layers,
            Self::GlyphRuns => work.glyph_runs,
            Self::Glyphs => work.glyphs,
        }
    }
}

/// A [`DrawWork`] census on the wire.
///
/// Mirrors `crate::frame_timings::FrameTimingsDraw` field for field, and
/// deliberately does not reuse it: that type is a member of the frame-timings
/// response and freezing the two together would make a change to one a silent
/// change to the other's contract. Built by destructuring, so a field added to
/// [`DrawWork`] fails to compile until it is stated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DrawProfileWork {
    /// Draw commands encoded.
    pub draws: u32,
    /// Paths encoded — one per filled or stroked shape, plus one per clip.
    pub paths: u32,
    /// Line and curve segments across those paths — the geometric size.
    /// Disjoint from [`Self::glyphs`]: glyph outlines are resolved downstream
    /// of the encoding, so text contributes to neither this nor
    /// [`Self::paths`].
    pub path_segments: u32,
    /// Clip / blend layers pushed.
    pub layers: u32,
    /// Glyph runs issued — one per shaped run, which is one text draw.
    pub glyph_runs: u32,
    /// Positioned glyphs across those runs — the text size.
    pub glyphs: u32,
}

impl DrawProfileWork {
    fn of(w: DrawWork) -> Self {
        let DrawWork {
            draws,
            paths,
            path_segments,
            layers,
            glyph_runs,
            glyphs,
        } = w;
        Self {
            draws,
            paths,
            path_segments,
            layers,
            glyph_runs,
            glyphs,
        }
    }
}

/// One attributed node on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DrawProfileRow {
    /// The address that reaches this node — `/window[main]/a/b`, the form
    /// `scene/snapshot`, `scene/query` and `scene/invoke` accept.
    pub path: String,
    /// This node's own path segment within its parent, or `null` for a node
    /// that consumes none: the root, and a `Scroll`'s content (which is reached
    /// at the scroll's own address). Published beside `path` so a consumer can
    /// see that two rows sharing an address is the addressing rule rather than
    /// a defect.
    pub segment: Option<String>,
    /// The `Scene` variant — the same name `scene/snapshot` uses for `type`.
    pub kind: &'static str,
    /// The node's §5.20 tag, or `null`.
    pub tag: Option<String>,
    /// **Inclusive**: this node and its whole subtree.
    pub total: DrawProfileWork,
    /// **Exclusive**: this node alone.
    pub own: DrawProfileWork,
    /// Children, in paint order.
    pub children: Vec<DrawProfileRow>,
    /// Children not in `children` because `depth` cut them off. `0` on every
    /// row of an unpruned reply — a depth limit never silently shortens a tree.
    pub children_omitted: u32,
}

/// One row of the `heaviest` ranking — flat, because a ranking is an ordering
/// over the whole profile rather than a subtree of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DrawProfileRank {
    /// The address that reaches this node.
    pub path: String,
    /// The `Scene` variant.
    pub kind: &'static str,
    /// The node's §5.20 tag, or `null`.
    pub tag: Option<String>,
    /// This node's **exclusive** cost — what the ranking sorted on.
    pub own: DrawProfileWork,
}

/// Snapshot returned by [`draw_profile`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DrawProfileOutcome {
    /// The attributed tree, or `null` for a window that has never painted.
    pub root: Option<DrawProfileRow>,
    /// (R1558) The address the profile was rooted at, echoed verbatim, or
    /// `null` for a whole-window profile.
    pub path: Option<String>,
    /// Rows in this reply.
    pub nodes: u32,
    /// Rows in the **profile** — what was measured, which is why it shrinks
    /// under `path` and does not shrink under `depth`. Equal to `nodes` unless
    /// `depth` pruned.
    ///
    /// The pair is what tells the two axes apart from the outside: `path`
    /// scoped the measurement, so a smaller profile was taken; `depth` scoped
    /// the reply, so the same profile was taken and reported shorter.
    pub nodes_total: u32,
    /// The depth limit that was applied, or `null` for the whole tree.
    pub depth: Option<u32>,
    /// The unit `heaviest` was ranked by, or `null` when none was asked for.
    pub heaviest_by: Option<&'static str>,
    /// The heaviest nodes by `own` in that unit, most first. Empty when no
    /// unit was named.
    pub heaviest: Vec<DrawProfileRank>,
}

/// (R1558) The subtree a request asked to be profiled — an address, already
/// parsed into the pieces the embedder and the projection each need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileScope {
    /// The address exactly as the caller wrote it. Echoed by the reply and
    /// quoted by [`DrawProfileError::UnknownPath`], so a rejection names the
    /// string that was rejected rather than a normalisation of it.
    pub requested: String,
    /// The window an explicit `/window[id]/` prefix named, `None` when the
    /// address carried none. Never invented: an absent prefix stays absent
    /// here so [`DrawProfileParams::window`] can tell "the caller said main"
    /// from "the caller said nothing".
    ///
    /// A **name**, not a resolved window, and deliberately unvalidated at parse
    /// time — see [`crate::path::split_window_prefix`] for why. Whether it
    /// names a window is judged by the embedder against its live slots, which
    /// is the registry this method's answer actually comes from.
    pub window: Option<String>,
    /// The scene segments below the prefix — what
    /// [`Scene::lookup_path_ref`](pinion_core::Scene::lookup_path_ref) walks.
    /// Empty addresses the scene root, which is the whole-window profile
    /// spelled out.
    pub segments: Vec<String>,
}

/// Parameters [`draw_profile`] accepts, already parsed and validated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DrawProfileParams {
    /// Levels of tree to emit below the root, or `None` for all of them.
    /// `Some(0)` is the root alone.
    pub depth: Option<u32>,
    /// The unit to rank `heaviest` by, or `None` for no ranking.
    pub heaviest_by: Option<Unit>,
    /// How many rows the ranking holds.
    pub limit: usize,
    /// (R1558) The subtree to profile, or `None` for the whole window.
    ///
    /// This scopes the **measurement**, not the reply: only the named subtree
    /// is re-encoded and only its nodes are attributed. `depth` is the other
    /// axis and does the opposite — it trims what a full measurement reports.
    pub scope: Option<ProfileScope>,
}

impl DrawProfileParams {
    /// (R1558) The window this request names, given the frame's `window`
    /// scope: the address's explicit `/window[id]/` prefix when it has one,
    /// else that scope, else the primary window.
    ///
    /// ONE rule with two callers that must agree — the embedder picks which
    /// window's paint scene to re-encode, and the projection formats every
    /// row's `/window[id]/…` address. Two copies of this expression would be
    /// two answers to "which window is this?", and a profile of one window
    /// whose rows address another is worse than no profile: every address in
    /// it resolves, somewhere else.
    #[must_use]
    pub fn window<'a>(&'a self, scope: Option<&'a str>) -> &'a str {
        self.scope
            .as_ref()
            .and_then(|s| s.window.as_deref())
            .or(scope)
            .unwrap_or(pinion_runtime::DEFAULT_WINDOW)
    }

    /// (R1558) The segment chain to root the profile at — empty for the whole
    /// window, which is also what an address of `"/"` means.
    #[must_use]
    pub fn scope_segments(&self) -> &[String] {
        self.scope.as_ref().map_or(&[], |s| &s.segments)
    }

    /// Parse the method's `params` object.
    ///
    /// # Errors
    ///
    /// - [`DrawProfileError::UnknownUnit`] — `heaviest_by` is not a unit.
    /// - [`DrawProfileError::MalformedParam`] — `depth`, `limit` or `path` is
    ///   present and is not of the right JSON type.
    /// - [`DrawProfileError::Path`] — `path`'s window prefix is malformed or
    ///   names no declared window.
    /// - [`DrawProfileError::WindowMismatch`] — `path` and `window` name
    ///   different windows.
    pub fn parse(params: Option<&serde_json::Value>) -> Result<Self, DrawProfileError> {
        let mut out = Self {
            limit: DEFAULT_HEAVIEST_LIMIT,
            ..Self::default()
        };
        let Some(obj) = params.and_then(serde_json::Value::as_object) else {
            return Ok(out);
        };
        if let Some(depth) = obj.get("depth").filter(|v| !v.is_null()) {
            let n = depth
                .as_u64()
                .ok_or(DrawProfileError::MalformedParam { param: "depth" })?;
            out.depth = Some(u32::try_from(n).unwrap_or(u32::MAX));
        }
        if let Some(limit) = obj.get("limit").filter(|v| !v.is_null()) {
            let n = limit
                .as_u64()
                .ok_or(DrawProfileError::MalformedParam { param: "limit" })?;
            out.limit = usize::try_from(n).unwrap_or(usize::MAX);
        }
        if let Some(by) = obj.get("heaviest_by").filter(|v| !v.is_null()) {
            let name = by.as_str().ok_or_else(|| DrawProfileError::UnknownUnit {
                requested: by.to_string(),
                valid: Unit::NAMES,
            })?;
            out.heaviest_by =
                Some(
                    Unit::parse(name).ok_or_else(|| DrawProfileError::UnknownUnit {
                        requested: name.to_owned(),
                        valid: Unit::NAMES,
                    })?,
                );
        }
        if let Some(path) = obj.get("path").filter(|v| !v.is_null()) {
            let requested = path
                .as_str()
                .ok_or(DrawProfileError::MalformedParam { param: "path" })?;
            // Syntax only. Whether the name is a window is the embedder's
            // question, because the set that decides is its live window slots
            // and not the SCE topology `crate::path::resolve` consults — sets
            // that a binding opening a second `WindowSpec` makes differ.
            let (window, tail) = crate::path::split_window_prefix(requested)?;
            // An absent prefix must stay absent: folding in a default would
            // turn "the caller said nothing about the window" into "the caller
            // said main", which the check below would then reject for a
            // request that named one window exactly once.
            if let Some(from_path) = window
                && let Some(from_scope) = obj.get("window").and_then(serde_json::Value::as_str)
                && from_path != from_scope
            {
                return Err(DrawProfileError::WindowMismatch {
                    path_window: from_path.to_owned(),
                    scope_window: from_scope.to_owned(),
                });
            }
            out.scope = Some(ProfileScope {
                requested: requested.to_owned(),
                window: window.map(ToOwned::to_owned),
                segments: crate::path::segments(tail),
            });
        }
        Ok(out)
    }
}

/// Project a [`DrawProfile`] onto the wire-shaped [`DrawProfileOutcome`].
///
/// `window` names the dispatch scope, and is what turns each row's segment
/// chain into the `/window[<id>]/…` address every other method accepts.
///
/// `profile` is the embedder's answer to the scope this request named: `None`
/// for a host with no live window, `Some(Err(_))` for a scope the painted
/// scene could not resolve, `Some(Ok(_))` for the subtree that was measured.
///
/// # Errors
///
/// - [`DrawProfileError::DrawProfileUnavailable`] — the embedder registered no
///   profile, which is a host with no live window or a window that has never
///   painted.
/// - Whatever the embedder's scope resolution rejected —
///   [`DrawProfileError::UnknownPath`] for an address that reached no node.
pub fn draw_profile(
    profile: Option<&Result<DrawProfile, DrawProfileError>>,
    window: &str,
    params: &DrawProfileParams,
) -> Result<DrawProfileOutcome, DrawProfileError> {
    let path = params.scope.as_ref().map(|s| s.requested.clone());
    let profile = match profile {
        None => return Err(DrawProfileError::DrawProfileUnavailable),
        Some(Err(err)) => return Err(err.clone()),
        Some(Ok(profile)) => profile,
    };
    let Some(root) = profile.root.as_ref() else {
        return Ok(DrawProfileOutcome {
            root: None,
            path,
            nodes: 0,
            nodes_total: 0,
            depth: params.depth,
            heaviest_by: params.heaviest_by.map(Unit::name),
            heaviest: Vec::new(),
        });
    };
    // ONE path derivation. The tree is projected in full, then the reply's copy
    // is pruned and the ranking is read off the projected rows — where the
    // addresses already are. Deriving them twice (once per output) would be two
    // implementations of the same addressing rule, free to disagree about a
    // `Scroll`'s transparency in one of them and not the other.
    //
    // R1558 — seeded with the chain the profile was ROOTED at, so a scoped
    // profile's rows carry the same absolute addresses an unscoped profile's
    // rows do and can be fed straight back in to drill further. The seed comes
    // from the profile rather than from `params`, because the resolver accepts
    // a scene root named by its own tag and a profile rooted that way sits at
    // the empty chain — see `DrawProfile::scope`.
    let mut segments: Vec<&str> = profile.scope.iter().map(String::as_str).collect();
    let full = project(root, window, &mut segments);
    // `nodes_total` is counted on the PROFILE and `nodes` on the reply, so the
    // two are answers to two different questions rather than one number
    // computed twice. It also means an unpruned reply that lost a row would say
    // so: `nodes == nodes_total` holds only if the projection emitted
    // everything it was given.
    let nodes_total = root.node_count();
    let heaviest = params
        .heaviest_by
        .map(|unit| rank(&full, unit, params.limit))
        .unwrap_or_default();
    let mut row = full;
    prune(&mut row, params.depth);
    Ok(DrawProfileOutcome {
        nodes: row.node_count(),
        nodes_total,
        root: Some(row),
        path,
        depth: params.depth,
        heaviest_by: params.heaviest_by.map(Unit::name),
        heaviest,
    })
}

impl DrawProfileRow {
    /// Rows in this subtree, itself included — what `nodes` reports.
    fn node_count(&self) -> u32 {
        self.children
            .iter()
            .fold(1_u32, |acc, c| acc.saturating_add(c.node_count()))
    }

    /// Every row in this subtree, itself first — the ranking's input.
    fn rows(&self) -> impl Iterator<Item = &Self> {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.children.iter());
            Some(node)
        })
    }
}

/// Render one node and its whole subtree, maintaining the segment stack that
/// becomes each row's address.
fn project<'n>(
    node: &'n DrawProfileNode,
    window: &str,
    segments: &mut Vec<&'n str>,
) -> DrawProfileRow {
    // A node that consumes no segment (`None`) contributes nothing to the
    // stack, which is what makes a `Scroll`'s content report the scroll's own
    // address — the `scene/locate` rule.
    let pushed = node.segment.as_deref().is_some_and(|s| {
        segments.push(s);
        true
    });
    let path = crate::locate::format_path(window, segments);
    let children = node
        .children
        .iter()
        .map(|c| project(c, window, segments))
        .collect();
    if pushed {
        segments.pop();
    }
    DrawProfileRow {
        path,
        segment: node.segment.clone(),
        kind: node.kind.name(),
        tag: node.tag.clone(),
        total: DrawProfileWork::of(node.total),
        own: DrawProfileWork::of(node.own),
        children,
        children_omitted: 0,
    }
}

/// Cut the reply's tree to `depth` levels below its root, recording at each cut
/// how many children were dropped.
///
/// A separate pass rather than a parameter on [`project`] so the ranking above
/// can read the WHOLE tree: a caller asking for a shallow tree and a ranking is
/// asking "summarise the shape, and tell me where the cost actually is", and
/// answering the second from the first would only ever name rows already on
/// screen. `None` cuts nothing.
fn prune(row: &mut DrawProfileRow, depth: Option<u32>) {
    let Some(depth) = depth else { return };
    if depth == 0 {
        row.children_omitted = u32::try_from(row.children.len()).unwrap_or(u32::MAX);
        row.children.clear();
        return;
    }
    for child in &mut row.children {
        prune(child, Some(depth - 1));
    }
}

/// The `heaviest` ranking: every row, ordered by its exclusive cost in `unit`,
/// most first, truncated to `limit`.
///
/// Ties break on the path, so the ordering is total and two runs of the same
/// scene answer identically — a ranking whose ties fell out of walk order would
/// be reproducible only by accident.
fn rank(root: &DrawProfileRow, unit: Unit, limit: usize) -> Vec<DrawProfileRank> {
    let mut all: Vec<&DrawProfileRow> = root.rows().collect();
    all.sort_by(|a, b| {
        unit.of_wire(b.own)
            .cmp(&unit.of_wire(a.own))
            .then_with(|| a.path.cmp(&b.path))
    });
    all.truncate(limit);
    all.into_iter()
        .map(|row| DrawProfileRank {
            path: row.path.clone(),
            kind: row.kind,
            tag: row.tag.clone(),
            own: row.own,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{DrawProfileError, DrawProfileParams, DrawProfileWork, Unit, draw_profile};
    use pinion_core::scene::SceneNodeKind;
    use pinion_runtime::{DrawProfile, DrawProfileNode, DrawWork};

    fn work(glyphs: u32, paths: u32) -> DrawWork {
        DrawWork {
            glyphs,
            paths,
            ..DrawWork::default()
        }
    }

    fn leaf(segment: &str, kind: SceneNodeKind, own: DrawWork) -> DrawProfileNode {
        DrawProfileNode {
            segment: Some(segment.to_owned()),
            kind,
            tag: None,
            total: own,
            own,
            children: Vec::new(),
        }
    }

    /// A root Container with a box child and a text child, the text carrying
    /// every glyph — profiled whole, so its scope is the empty chain.
    fn profile() -> DrawProfile {
        scoped(Vec::new())
    }

    /// The same tree, profiled as the subtree `scope` addresses — which is
    /// what every row's address is then built on top of.
    fn scoped(scope: Vec<String>) -> DrawProfile {
        let a = leaf("0", SceneNodeKind::Box, work(0, 3));
        let b = leaf("label", SceneNodeKind::Text, work(40, 0));
        DrawProfile {
            root: Some(DrawProfileNode {
                segment: None,
                kind: SceneNodeKind::Container,
                tag: Some("app-root".to_owned()),
                total: work(40, 5),
                own: work(0, 2),
                children: vec![a, b],
            }),
            scope,
        }
    }

    #[test]
    fn r1557_missing_profile_errors() {
        assert_eq!(
            draw_profile(None, "main", &DrawProfileParams::default()).unwrap_err(),
            DrawProfileError::DrawProfileUnavailable,
        );
    }

    #[test]
    fn r1557_rows_carry_the_address_every_other_method_accepts() {
        let out =
            draw_profile(Some(&Ok(profile())), "main", &DrawProfileParams::default()).unwrap();
        let root = out.root.expect("root");
        assert_eq!(root.path, "/window[main]/");
        assert_eq!(root.segment, None);
        assert_eq!(root.kind, "Container");
        assert_eq!(root.tag.as_deref(), Some("app-root"));
        assert_eq!(root.children[0].path, "/window[main]/0");
        assert_eq!(root.children[1].path, "/window[main]/label");
        assert_eq!(root.children[1].kind, "Text");
        assert_eq!(out.nodes, 3);
        assert_eq!(out.nodes_total, 3);
        assert_eq!(root.children_omitted, 0);
    }

    #[test]
    fn r1557_inclusive_and_exclusive_are_both_published() {
        let out =
            draw_profile(Some(&Ok(profile())), "main", &DrawProfileParams::default()).unwrap();
        let root = out.root.expect("root");
        assert_eq!(
            root.total,
            DrawProfileWork {
                draws: 0,
                paths: 5,
                path_segments: 0,
                layers: 0,
                glyph_runs: 0,
                glyphs: 40,
            }
        );
        assert_eq!(root.own.glyphs, 0, "the container drew no glyphs");
        assert_eq!(root.children[1].own.glyphs, 40);
    }

    #[test]
    fn r1557_depth_prunes_loudly() {
        let params = DrawProfileParams {
            depth: Some(0),
            ..DrawProfileParams::default()
        };
        let out = draw_profile(Some(&Ok(profile())), "main", &params).unwrap();
        let root = out.root.expect("root");
        assert!(root.children.is_empty());
        // The pruning is stated three ways rather than left to be inferred from
        // an empty array: the count that was cut, the rows emitted, and the
        // rows that exist.
        assert_eq!(root.children_omitted, 2);
        assert_eq!(out.nodes, 1);
        assert_eq!(out.nodes_total, 3);
        assert_eq!(out.depth, Some(0));
    }

    #[test]
    fn r1557_ranking_is_by_a_named_unit_over_the_whole_profile() {
        let params = DrawProfileParams {
            depth: Some(0),
            heaviest_by: Some(Unit::Glyphs),
            limit: 2,
            scope: None,
        };
        let out = draw_profile(Some(&Ok(profile())), "main", &params).unwrap();
        assert_eq!(out.heaviest_by, Some("glyphs"));
        assert_eq!(out.heaviest.len(), 2);
        // The depth limit cut the reply's tree to one row; the ranking still
        // names the node that holds the cost.
        assert_eq!(out.heaviest[0].path, "/window[main]/label");
        assert_eq!(out.heaviest[0].own.glyphs, 40);
        // Ranking by a different unit is a different answer, which is the
        // reason the unit is the caller's to name.
        let by_paths = DrawProfileParams {
            heaviest_by: Some(Unit::Paths),
            limit: 1,
            ..DrawProfileParams::default()
        };
        let out = draw_profile(Some(&Ok(profile())), "main", &by_paths).unwrap();
        assert_eq!(out.heaviest[0].path, "/window[main]/0");
    }

    #[test]
    fn r1557_no_unit_named_means_no_ranking() {
        let out =
            draw_profile(Some(&Ok(profile())), "main", &DrawProfileParams::default()).unwrap();
        assert_eq!(out.heaviest_by, None);
        assert!(out.heaviest.is_empty());
    }

    #[test]
    fn r1557_unknown_unit_teaches_the_valid_set() {
        let params = serde_json::json!({"heaviest_by": "milliseconds"});
        let err = DrawProfileParams::parse(Some(&params)).unwrap_err();
        assert_eq!(
            err,
            DrawProfileError::UnknownUnit {
                requested: "milliseconds".to_owned(),
                valid: Unit::NAMES,
            }
        );
        let tag = err.wire_tag();
        assert!(tag.starts_with("UnknownUnit: \"milliseconds\""), "{tag}");
        assert!(
            tag.contains("glyphs"),
            "the message teaches what works: {tag}"
        );
    }

    #[test]
    fn r1557_malformed_numeric_params_are_named() {
        let depth = serde_json::json!({"depth": "deep"});
        assert_eq!(
            DrawProfileParams::parse(Some(&depth)).unwrap_err(),
            DrawProfileError::MalformedParam { param: "depth" },
        );
        let limit = serde_json::json!({"limit": -1});
        assert_eq!(
            DrawProfileParams::parse(Some(&limit)).unwrap_err(),
            DrawProfileError::MalformedParam { param: "limit" },
        );
    }

    #[test]
    fn r1557_absent_params_are_the_whole_census() {
        let p = DrawProfileParams::parse(None).unwrap();
        assert_eq!(p.depth, None, "no limit is the whole tree");
        assert_eq!(p.heaviest_by, None);
        assert_eq!(p.limit, super::DEFAULT_HEAVIEST_LIMIT);
    }

    #[test]
    fn r1557_every_unit_is_nameable_and_reads_its_own_field() {
        // The census link: a `DrawWork` field added without a `Unit` arm fails
        // `Unit::of_wire`'s match, and one added without a name fails here.
        assert_eq!(Unit::ALL.len(), Unit::NAMES.len());
        let all = DrawProfileWork::of(DrawWork {
            draws: 1,
            paths: 2,
            path_segments: 3,
            layers: 4,
            glyph_runs: 5,
            glyphs: 6,
        });
        let read: Vec<u32> = Unit::ALL.into_iter().map(|u| u.of_wire(all)).collect();
        assert_eq!(
            read,
            vec![1, 2, 3, 4, 5, 6],
            "each unit reads its own field"
        );
        for (unit, name) in Unit::ALL.into_iter().zip(Unit::NAMES) {
            assert_eq!(unit.name(), *name);
            assert_eq!(Unit::parse(name), Some(unit));
        }
        assert_eq!(Unit::parse("nope"), None);
    }

    #[test]
    fn r1557_a_profile_with_no_root_is_not_an_error() {
        // A host that painted nothing answers "nothing", not a failure — the
        // `MemoryOutcome` shape, where an arena holding nothing is still a row.
        let out = draw_profile(
            Some(&Ok(DrawProfile {
                root: None,
                scope: Vec::new(),
            })),
            "main",
            &DrawProfileParams::default(),
        )
        .unwrap();
        assert_eq!(out.root, None);
        assert_eq!(out.nodes, 0);
        assert_eq!(out.nodes_total, 0);
        assert_eq!(out.path, None);
    }

    // ----- R1558: the profile is rooted where the caller asks -----

    #[test]
    fn r1558_a_scoped_profiles_rows_carry_absolute_addresses() {
        // The scoped profile is a subtree, but its rows must address the same
        // nodes an unscoped profile's rows do — otherwise drilling down hands
        // back a string that no longer resolves anywhere.
        let params = DrawProfileParams::parse(Some(&serde_json::json!({
            "path": "/window[main]/grid/panel"
        })))
        .unwrap();
        let scope = Ok(scoped(vec!["grid".to_owned(), "panel".to_owned()]));
        let out = draw_profile(Some(&scope), "main", &params).unwrap();
        let root = out.root.expect("root");
        assert_eq!(root.path, "/window[main]/grid/panel");
        assert_eq!(root.children[0].path, "/window[main]/grid/panel/0");
        assert_eq!(root.children[1].path, "/window[main]/grid/panel/label");
        // The requested scope is echoed verbatim, so a caller reads which
        // answer it got without comparing addresses.
        assert_eq!(out.path.as_deref(), Some("/window[main]/grid/panel"));
    }

    #[test]
    fn r1558_the_prefix_comes_from_what_was_profiled_not_what_was_asked() {
        // The addressing vocabulary lets a scene root be named by its own tag,
        // and such a root sits at the EMPTY chain — so the resolver's answer
        // and the caller's question are different chains. Seeding the
        // projection from the question would address every row one segment too
        // deep, and each of those addresses would then fail to resolve.
        let params =
            DrawProfileParams::parse(Some(&serde_json::json!({"path": "/app-root"}))).unwrap();
        assert_eq!(params.scope_segments(), ["app-root"]);
        // The embedder resolved that to the root, whose chain is empty.
        let out = draw_profile(Some(&Ok(scoped(Vec::new()))), "main", &params).unwrap();
        let root = out.root.expect("root");
        assert_eq!(root.path, "/window[main]/");
        assert_eq!(root.children[1].path, "/window[main]/label");
        assert_eq!(
            out.path.as_deref(),
            Some("/app-root"),
            "still echoed as sent"
        );
    }

    #[test]
    fn r1558_path_scopes_the_measurement_and_depth_scopes_the_reply() {
        // The two axes, told apart from the outside by the pair `nodes` /
        // `nodes_total`. A `depth` limit leaves `nodes_total` at the profile's
        // real size, because the whole window was still measured…
        let pruned = DrawProfileParams {
            depth: Some(0),
            ..DrawProfileParams::default()
        };
        let out = draw_profile(Some(&Ok(profile())), "main", &pruned).unwrap();
        assert_eq!((out.nodes, out.nodes_total), (1, 3));
        assert_eq!(out.path, None);

        // …while a `path` scope shrinks `nodes_total` itself, because a
        // smaller profile was taken. The embedder hands back the subtree it
        // measured, which is what makes this a different number rather than a
        // different view of the same one.
        let scoped_params =
            DrawProfileParams::parse(Some(&serde_json::json!({"path": "/panel"}))).unwrap();
        let one_leaf = Ok(DrawProfile {
            root: Some(leaf("label", SceneNodeKind::Text, work(40, 0))),
            scope: vec!["panel".to_owned()],
        });
        let out = draw_profile(Some(&one_leaf), "main", &scoped_params).unwrap();
        assert_eq!((out.nodes, out.nodes_total), (1, 1));
        assert_eq!(out.depth, None, "nothing was pruned to get there");
    }

    #[test]
    fn r1558_an_unresolvable_scope_is_named_not_reported_as_no_profile() {
        // The embedder resolved the address against the painted scene and
        // found nothing. That is a different fact from "this window has no
        // profile", and collapsing the two would have a typo read as a host
        // with no window.
        let err = draw_profile(
            Some(&Err(DrawProfileError::UnknownPath {
                requested: "/window[main]/nope".to_owned(),
            })),
            "main",
            &DrawProfileParams::default(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            DrawProfileError::UnknownPath {
                requested: "/window[main]/nope".to_owned()
            }
        );
        assert_eq!(err.wire_tag(), "UnknownPath: \"/window[main]/nope\"");
        assert!(err.to_string().contains("reaches no node"), "{err}");
        assert_ne!(err, DrawProfileError::DrawProfileUnavailable);
    }

    #[test]
    fn r1558_a_malformed_or_unknown_window_prefix_keeps_its_own_reason() {
        let malformed = serde_json::json!({"path": "/window[main/x"});
        assert_eq!(
            DrawProfileParams::parse(Some(&malformed)).unwrap_err(),
            DrawProfileError::Path(crate::path::PathError::MalformedPrefix),
        );
        // Forwarded, not collapsed into one blanket `Path` tag — an agent
        // switches on the concrete reason.
        assert_eq!(
            DrawProfileParams::parse(Some(&malformed))
                .unwrap_err()
                .wire_tag(),
            "MalformedPrefix"
        );
        let empty = serde_json::json!({"path": "/window[]/x"});
        assert_eq!(
            DrawProfileParams::parse(Some(&empty)).unwrap_err(),
            DrawProfileError::Path(crate::path::PathError::EmptyWindowId),
        );
        let not_a_string = serde_json::json!({"path": 7});
        assert_eq!(
            DrawProfileParams::parse(Some(&not_a_string)).unwrap_err(),
            DrawProfileError::MalformedParam { param: "path" },
        );
    }

    #[test]
    fn r1558_a_window_name_is_not_judged_at_parse_time() {
        // Whether a name is a window is not a syntax question, and the set
        // that answers it is the embedder's live window slots — NOT the SCE
        // topology `crate::path::resolve` consults. The two differ whenever a
        // binding opens a second `WindowSpec` without a second `AppState`, and
        // under the older rule this method published `/window[inspector]/…`
        // rows that it then refused to read back.
        let p = DrawProfileParams::parse(Some(&serde_json::json!({
            "path": "/window[inspector]/panel"
        })))
        .expect("an unopened name parses; it is judged where the registry is");
        assert_eq!(
            p.scope.as_ref().unwrap().window.as_deref(),
            Some("inspector")
        );
        assert_eq!(p.window(Some("main")), "inspector");
        assert_eq!(p.scope_segments(), ["panel"]);

        // …and the embedder's judgment, when it goes against the caller, names
        // the registry that decided.
        let err = DrawProfileError::UnknownWindow {
            requested: "inspector".to_owned(),
            valid: vec!["main".to_owned()],
        };
        assert_eq!(err.wire_tag(), "UnknownWindow: \"inspector\" (valid: main)");
        assert!(err.to_string().contains("not open"), "{err}");
    }

    #[test]
    fn r1558_two_windows_named_in_one_request_is_refused() {
        // Neither silently wins. A request that says two different things
        // about which window it means is a caller bug, and a profile of either
        // one would carry that bug into whatever reads it.
        let conflict = serde_json::json!({"path": "/window[main]/a", "window": "other"});
        let err = DrawProfileParams::parse(Some(&conflict)).unwrap_err();
        assert_eq!(
            err,
            DrawProfileError::WindowMismatch {
                path_window: "main".to_owned(),
                scope_window: "other".to_owned(),
            }
        );
        assert!(err.wire_tag().starts_with("WindowMismatch:"), "{err:?}");
        // Agreement is not a conflict, and neither is naming the window once.
        let agrees = serde_json::json!({"path": "/window[main]/a", "window": "main"});
        assert!(DrawProfileParams::parse(Some(&agrees)).is_ok());
    }

    #[test]
    fn r1558_the_window_is_decided_by_one_rule() {
        // One expression with two callers — the embedder picking which window
        // to re-encode, and the projection formatting every row's address.
        let bare = DrawProfileParams::default();
        assert_eq!(bare.window(None), pinion_runtime::DEFAULT_WINDOW);
        assert_eq!(bare.window(Some("sidecar")), "sidecar");

        // An address with no prefix leaves the frame's scope in charge: an
        // absent prefix must not be folded into `Some(initial window)`, or a
        // request that named a window ONCE would look like two.
        let implicit =
            DrawProfileParams::parse(Some(&serde_json::json!({"path": "/a/b"}))).unwrap();
        assert_eq!(implicit.scope.as_ref().unwrap().window, None);
        assert_eq!(implicit.window(Some("sidecar")), "sidecar");
        assert_eq!(implicit.scope_segments(), ["a", "b"]);

        // An explicit prefix is the answer, and it is where the rows get
        // rendered against too.
        let explicit =
            DrawProfileParams::parse(Some(&serde_json::json!({"path": "/window[main]/a"})))
                .unwrap();
        assert_eq!(explicit.window(None), "main");
    }

    #[test]
    fn r1558_an_empty_address_is_the_whole_window_spelled_out() {
        for path in ["/", "", "/window[main]/"] {
            let p = DrawProfileParams::parse(Some(&serde_json::json!({"path": path}))).unwrap();
            assert!(
                p.scope_segments().is_empty(),
                "{path:?} addresses the scene root",
            );
        }
        // …and no scope at all is the same measurement, which is why the
        // absent case is not a special arm anywhere below this.
        assert!(DrawProfileParams::default().scope_segments().is_empty());
    }
}
