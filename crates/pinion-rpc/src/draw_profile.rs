//! `scene/draw_profile` RPC method — R1557 §5.16 §5.18 §5.7 §2 #2 §2 #7.
//!
//! Publishes [`pinion_runtime::DrawProfile`]: which subtree of the painted
//! scene drew the frame, in the units a 2D vector renderer is charged in.
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
//! # Wire form
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "scene/draw_profile",
//!   "params": {"window": "main", "depth": 2, "heaviest_by": "glyphs"},
//!   "id": 1
//! }
//! ```
//!
//! answers with
//!
//! ```json
//! {
//!   "root": {
//!     "path": "/window[main]/",
//!     "segment": null,
//!     "kind": "Container",
//!     "tag": "app-root",
//!     "total": {"draws": 812, "paths": 400, "path_segments": 2100,
//!               "layers": 3, "glyph_runs": 120, "glyphs": 4210},
//!     "own": {"draws": 1, "paths": 1, "path_segments": 4,
//!             "layers": 0, "glyph_runs": 0, "glyphs": 0},
//!     "children": [ ... ],
//!     "children_omitted": 0
//!   },
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
//! - `path` is the address `scene/locate` produces and `scene/query` /
//!   `scene/invoke` resolve (both go through `Scene::lookup_path_ref`, which
//!   takes the same tag-or-index segments and is likewise transparent to a
//!   `Scroll`). "Which subtree is expensive" and "act on that subtree" are the
//!   same string. Note the limit this states rather than hides: those two
//!   methods resolve a path in order to reach an `External` on it, so an
//!   arbitrary leaf's address is in the right vocabulary and has no reader yet
//!   — `scene/snapshot` takes a window selector with no scene tail, and
//!   `scene/layout` addresses its own nodes by bare index (its `path` filter is
//!   an accepted-and-ignored R47.7.x carry, predating this round).
//! - `nodes` counts the rows in this reply; `nodes_total` counts the profile.
//!   They differ exactly when `depth` pruned something, and every pruned node's
//!   parent says so in `children_omitted` — truncation is never silent.
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
        }
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
        }
    }
}

/// One [`DrawWork`] field, named on the wire — the units a caller may rank by.
///
/// The census of them, so `heaviest_by`'s valid set is derived from the type
/// rather than hand-listed beside it: a field added to [`DrawWork`] fails
/// [`Unit::of_wire`]'s match here, where it must be given a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Rows in this reply.
    pub nodes: u32,
    /// Rows in the profile. Equal to `nodes` unless `depth` pruned.
    pub nodes_total: u32,
    /// The depth limit that was applied, or `null` for the whole tree.
    pub depth: Option<u32>,
    /// The unit `heaviest` was ranked by, or `null` when none was asked for.
    pub heaviest_by: Option<&'static str>,
    /// The heaviest nodes by `own` in that unit, most first. Empty when no
    /// unit was named.
    pub heaviest: Vec<DrawProfileRank>,
}

/// Parameters [`draw_profile`] accepts, already parsed and validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrawProfileParams {
    /// Levels of tree to emit below the root, or `None` for all of them.
    /// `Some(0)` is the root alone.
    pub depth: Option<u32>,
    /// The unit to rank `heaviest` by, or `None` for no ranking.
    pub heaviest_by: Option<Unit>,
    /// How many rows the ranking holds.
    pub limit: usize,
}

impl DrawProfileParams {
    /// Parse the method's `params` object.
    ///
    /// # Errors
    ///
    /// - [`DrawProfileError::UnknownUnit`] — `heaviest_by` is not a unit.
    /// - [`DrawProfileError::MalformedParam`] — `depth` or `limit` is present
    ///   and is not a non-negative integer.
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
        Ok(out)
    }
}

/// Project a [`DrawProfile`] onto the wire-shaped [`DrawProfileOutcome`].
///
/// `window` names the dispatch scope, and is what turns each row's segment
/// chain into the `/window[<id>]/…` address every other method accepts.
///
/// # Errors
///
/// - [`DrawProfileError::DrawProfileUnavailable`] — the embedder registered no
///   profile, which is a host with no live window or a window that has never
///   painted.
pub fn draw_profile(
    profile: Option<&DrawProfile>,
    window: &str,
    params: DrawProfileParams,
) -> Result<DrawProfileOutcome, DrawProfileError> {
    let Some(profile) = profile else {
        return Err(DrawProfileError::DrawProfileUnavailable);
    };
    let Some(root) = profile.root.as_ref() else {
        return Ok(DrawProfileOutcome {
            root: None,
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
    let mut segments: Vec<&str> = Vec::new();
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
    /// every glyph.
    fn profile() -> DrawProfile {
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
        }
    }

    #[test]
    fn r1557_missing_profile_errors() {
        assert_eq!(
            draw_profile(None, "main", DrawProfileParams::default()).unwrap_err(),
            DrawProfileError::DrawProfileUnavailable,
        );
    }

    #[test]
    fn r1557_rows_carry_the_address_every_other_method_accepts() {
        let out = draw_profile(Some(&profile()), "main", DrawProfileParams::default()).unwrap();
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
        let out = draw_profile(Some(&profile()), "main", DrawProfileParams::default()).unwrap();
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
        let out = draw_profile(Some(&profile()), "main", params).unwrap();
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
        };
        let out = draw_profile(Some(&profile()), "main", params).unwrap();
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
        let out = draw_profile(Some(&profile()), "main", by_paths).unwrap();
        assert_eq!(out.heaviest[0].path, "/window[main]/0");
    }

    #[test]
    fn r1557_no_unit_named_means_no_ranking() {
        let out = draw_profile(Some(&profile()), "main", DrawProfileParams::default()).unwrap();
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
            Some(&DrawProfile { root: None }),
            "main",
            DrawProfileParams::default(),
        )
        .unwrap();
        assert_eq!(out.root, None);
        assert_eq!(out.nodes, 0);
        assert_eq!(out.nodes_total, 0);
    }
}
