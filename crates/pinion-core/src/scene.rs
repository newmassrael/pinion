//! Closed-form scene primitive type set (§5.2 §5.11, R16 slice 5).
//!
//! Seven ratified variants per §5.2: `Box`, `Text`, `Path`, `Image`,
//! `Container`, `Effect`, `External`. The two opaque escapes (`Effect`,
//! `External`) are the only sanctioned exits per §3 capability boundary;
//! all other rendering goes through the closed set.
//!
//! Per §5.11 the variant shape is *layered*: each `XxxNode` carries the
//! primitive payload, [`Style`] supplies stylistic properties, and
//! [`Modifier`] composes layout/transform adjustments. The §5.3 DSL surface
//! settles the per-variant field set in a later round; the skeleton here
//! anchors only the closed enum + extension points.
//!
//! Each introspectable node (Box/Text/Path/Image/Container) carries an
//! optional `tag: Option<Cow<'static, str>>` field per §5.20: the
//! intent-system carrier that lets a widget identify which emitted
//! intent a given scene node belongs to (e.g. `"save_btn"` on the box
//! that paints the button). Tags live on data, not callbacks, so
//! view-fn purity (§6.3) and `dry_run` (§2 #3) stay intact.
//!
//! `#[non_exhaustive]` propagates the R14 forward-compat hedge (§5.2
//! caveat): future variants like `Mesh`/`Camera`/`Light` (game-engine
//! evolution) are addable without a `SemVer` major bump.

use std::borrow::Cow;

use crate::style::{Align, BoxStyle, Color, ImageStyle, LayoutStyle, PathStyle, TextStyle};

/// Closed scene primitive set (§5.2). Two opaque escape variants
/// (`Effect`, `External`) per §3; the other five are introspectable.
///
/// `Clone` is deliberately *not* derived: `ExternalNode` owns a
/// `Box<dyn External>` (§5.15) which has no general clone strategy.
/// Snapshot/`dry_run` over External state goes through the §5.15 item
/// 8 introspection surface (`ExternalIntrospect`), not a tree-wide
/// clone.
#[non_exhaustive]
#[derive(Debug)]
pub enum Scene {
    Box(BoxNode),
    Text(TextNode),
    Path(PathNode),
    Image(ImageNode),
    Container(ContainerNode),
    Effect(EffectNode),
    External(ExternalNode),
}

impl Scene {
    /// Outermost rect of this primitive. [`EffectNode`] has no
    /// geometry of its own and returns [`Rect::default`].
    #[must_use]
    pub fn rect(&self) -> Rect {
        match self {
            Scene::Box(n) => n.rect,
            Scene::Text(n) => n.rect,
            Scene::Path(n) => n.rect,
            Scene::Image(n) => n.rect,
            Scene::Container(n) => n.rect,
            Scene::External(n) => n.rect,
            Scene::Effect(_) => Rect::default(),
        }
    }

    /// §5.20 intent tag carried by this primitive, when present.
    /// [`EffectNode`] never carries a tag.
    #[must_use]
    pub fn tag(&self) -> Option<&str> {
        match self {
            Scene::Box(n) => n.tag.as_deref(),
            Scene::Text(n) => n.tag.as_deref(),
            Scene::Path(n) => n.tag.as_deref(),
            Scene::Image(n) => n.tag.as_deref(),
            Scene::Container(n) => n.tag.as_deref(),
            Scene::External(n) => n.tag.as_deref(),
            Scene::Effect(_) => None,
        }
    }

    /// (§5.32 R39 v0) Find the deepest primitive whose rect contains
    /// `(x, y)`. Returns [`None`] when the point falls outside this
    /// scene's outermost rect — including the case where the rect has
    /// zero area.
    ///
    /// Tie-breaking on overlapping siblings: among a `Container`'s
    /// children, the last entry wins (drawn last, on top). [`EffectNode`]
    /// is not hit-testable and is skipped — its parent receives the
    /// hit if no other sibling claims it.
    ///
    /// Path segments are container-relative — either a child index
    /// (`"3"`) or the child's §5.20 tag (`"save_btn"`) when present.
    /// An untagged hit at the scene root returns an empty `segments`
    /// vec, signalling the root primitive itself.
    #[must_use]
    pub fn hit_test(&self, x: u32, y: u32) -> Option<HitPath> {
        // Effect has no introspectable geometry — skip entirely.
        if matches!(self, Scene::Effect(_)) {
            return None;
        }
        if !rect_contains(self.rect(), x, y) {
            return None;
        }
        // Container: descend into the topmost (last) child that
        // contains the point. If no child hits, the container itself
        // is the deepest hit.
        if let Scene::Container(c) = self {
            for (idx, child) in c.children.iter().enumerate().rev() {
                if let Some(mut child_hit) = child.hit_test(x, y) {
                    let seg = child
                        .tag()
                        .map_or_else(|| idx.to_string(), String::from);
                    child_hit.segments.insert(0, seg);
                    return Some(child_hit);
                }
            }
        }
        Some(HitPath { segments: Vec::new(), bbox: self.rect() })
    }

    /// (§5.32 R39.2 v0) Collect every primitive whose rect intersects
    /// the query rect `(x, y, w, h)`. Walks the scene tree in
    /// declaration order (DFS pre-order); each match appears once,
    /// with its full path from the root. Containers themselves count
    /// as hits (a region-select on a tagged container is meaningful
    /// for AI reasoning).
    ///
    /// Zero-area query rects return an empty vec (per [`rects_intersect`]
    /// semantics). [`EffectNode`] is skipped — both at the leaf level
    /// and as a child during traversal.
    ///
    /// Path segments follow [`Self::hit_test`]: tag wins over index.
    #[must_use]
    pub fn hit_test_region(&self, x: u32, y: u32, w: u32, h: u32) -> Vec<HitPath> {
        let query = Rect::new(x, y, w, h);
        let mut acc = Vec::new();
        self.collect_intersections(query, &mut Vec::new(), &mut acc);
        acc
    }

    /// (§5.32 R39.3 v0) Reverse lookup: walk the scene tree following
    /// `segments` and return the matched primitive's bounding rect.
    /// Returns [`None`] when the path does not resolve — either an
    /// out-of-range index, an unknown tag, or a non-container
    /// intermediate that cannot be descended.
    ///
    /// An empty segment slice returns the root primitive's rect (the
    /// caller asked for `/window[...]/` itself).
    #[must_use]
    pub fn lookup_path(&self, segments: &[String]) -> Option<Rect> {
        if matches!(self, Scene::Effect(_)) {
            return None;
        }
        let Some((head, tail)) = segments.split_first() else {
            return Some(self.rect());
        };
        let Scene::Container(c) = self else {
            return None;
        };
        // Tag match first (a tagged child wins over an index that
        // happens to share its name). Among ties, declaration order.
        let target = c.children.iter().enumerate().find_map(|(idx, child)| {
            let tag_match = child.tag().is_some_and(|t| t == head);
            let index_match = idx.to_string() == *head;
            (tag_match || index_match).then_some(child)
        })?;
        target.lookup_path(tail)
    }

    /// (§5.34 R42) Immutable counterpart that returns `&Scene` at the
    /// matched path. [`lookup_path`](Self::lookup_path) only exposes
    /// the matched primitive's [`Rect`] (because §5.32 R39.3 `bbox`
    /// only needs geometry); the by-reference shape is what the
    /// `scene/query` / `scene/rewind` nested-External walker needs
    /// to descend through Container/Box before reaching an
    /// `ExternalNode`.
    ///
    /// Resolution rules identical to [`lookup_path`](Self::lookup_path)
    /// and [`lookup_path_mut`](Self::lookup_path_mut): tag wins over
    /// index, declaration order on ties, `Scene::Effect` never
    /// resolves, non-container intermediates fail. Empty segment
    /// slice returns `Some(self)`.
    #[must_use]
    pub fn lookup_path_ref(&self, segments: &[String]) -> Option<&Scene> {
        if matches!(self, Scene::Effect(_)) {
            return None;
        }
        let Some((head, tail)) = segments.split_first() else {
            return Some(self);
        };
        let Scene::Container(c) = self else {
            return None;
        };
        let target = c.children.iter().enumerate().find_map(|(idx, child)| {
            let tag_match = child.tag().is_some_and(|t| t == head);
            let index_match = idx.to_string() == *head;
            (tag_match || index_match).then_some(child)
        })?;
        target.lookup_path_ref(tail)
    }

    /// (§5.34 R40.10) Mutable counterpart to
    /// [`lookup_path`](Self::lookup_path). Walks the scene tree
    /// following `segments` and returns a `&mut Scene` to the matched
    /// primitive — the addressing substrate the
    /// `TypedProposal::SetStyle` / `TypedProposal::ReplaceView`
    /// variants need to mutate a non-root node.
    ///
    /// Resolution rules match `lookup_path` exactly: tag wins over
    /// index, declaration order on ties, `Scene::Effect` never
    /// resolves, non-container intermediates fail.
    ///
    /// An empty segment slice returns `Some(self)` so callers can
    /// uniformly handle the root case (e.g. `SetStyle` on
    /// `/window[main]/` mutates the root).
    pub fn lookup_path_mut(&mut self, segments: &[String]) -> Option<&mut Scene> {
        if matches!(self, Scene::Effect(_)) {
            return None;
        }
        let Some((head, tail)) = segments.split_first() else {
            return Some(self);
        };
        let Scene::Container(c) = self else {
            return None;
        };
        // Two-phase to satisfy the borrow checker: pick the matching
        // index immutably, then re-borrow that slot mutably.
        let target_idx = c.children.iter().enumerate().find_map(|(idx, child)| {
            let tag_match = child.tag().is_some_and(|t| t == head);
            let index_match = idx.to_string() == *head;
            (tag_match || index_match).then_some(idx)
        })?;
        c.children[target_idx].lookup_path_mut(tail)
    }

    /// Recursive helper for [`Self::hit_test_region`]. Maintains a
    /// segment stack representing the current path from the root.
    fn collect_intersections(
        &self,
        query: Rect,
        path: &mut Vec<String>,
        out: &mut Vec<HitPath>,
    ) {
        if matches!(self, Scene::Effect(_)) {
            return;
        }
        if !rects_intersect(self.rect(), query) {
            return;
        }
        out.push(HitPath { segments: path.clone(), bbox: self.rect() });
        if let Scene::Container(c) = self {
            for (idx, child) in c.children.iter().enumerate() {
                let seg = child
                    .tag()
                    .map_or_else(|| idx.to_string(), String::from);
                path.push(seg);
                child.collect_intersections(query, path, out);
                path.pop();
            }
        }
    }
}

/// Result of a successful [`Scene::hit_test`] (§5.32 R39 v0). Carries
/// the container-relative path from the root of the queried scene plus
/// the bounding rect of the matched primitive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitPath {
    /// Path segments from the queried root toward the deepest match.
    /// Each segment is either a positional index (`"3"`) or the
    /// child's §5.20 tag when present (`"save_btn"`). Empty when the
    /// root primitive itself is the deepest hit.
    pub segments: Vec<String>,
    /// Bounding rect of the deepest matched primitive. Same coordinate
    /// frame as the queried `(x, y)` (viewport-relative for the v0
    /// RPC method, see §5.32).
    pub bbox: Rect,
}

/// Half-open containment check: `(x, y)` lies inside `r` when both
/// coordinates fall in `[r.x, r.x + r.w)` × `[r.y, r.y + r.h)`. Uses
/// saturating add so zero-area rects (`w == 0` or `h == 0`) never
/// contain anything, and arithmetic overflow at the `u32` ceiling is
/// pinned at saturation rather than wrapping.
fn rect_contains(r: Rect, x: u32, y: u32) -> bool {
    let right = r.x.saturating_add(r.w);
    let bottom = r.y.saturating_add(r.h);
    x >= r.x && y >= r.y && x < right && y < bottom
}

/// Half-open rectangle intersection: returns `true` when `a` and `b`
/// share at least one pixel. Two zero-area rects never intersect, and
/// saturating-add prevents overflow at the `u32` ceiling.
fn rects_intersect(a: Rect, b: Rect) -> bool {
    let a_right = a.x.saturating_add(a.w);
    let a_bottom = a.y.saturating_add(a.h);
    let b_right = b.x.saturating_add(b.w);
    let b_bottom = b.y.saturating_add(b.h);
    if a.w == 0 || a.h == 0 || b.w == 0 || b.h == 0 {
        return false;
    }
    a.x < b_right && b.x < a_right && a.y < b_bottom && b.y < a_bottom
}

/// Stylistic trait carried by [`Scene`] variants (§5.11 layered shape).
/// The §5.3 DSL settles the actual surface (colors, fonts, borders); this
/// trait is the agreed extension point.
pub trait Style {}

/// Composition modifier (§5.11 layered shape, §5.3 R20 expansion).
///
/// Layout adjustments that wrap any [`Scene`] variant. v0 covers
/// margin / padding / alignment; transforms (translate / rotate /
/// scale) and full taffy flex/grid integration are carry-forward
/// per the §5.3 R20 caveat.
///
/// `margin` and `padding` reuse the [`Rect`] shape as a four-tuple of
/// `u32` insets — field mapping:
///
/// | `Rect` field | Inset side |
/// |---|---|
/// | `x` | left |
/// | `y` | top |
/// | `w` | right |
/// | `h` | bottom |
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
pub struct Modifier {
    pub margin: Rect,
    pub padding: Rect,
    pub align: Align,
}

impl Modifier {
    /// Identity modifier: zero margin / padding, `Align::TopLeft`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            margin: Rect::new(0, 0, 0, 0),
            padding: Rect::new(0, 0, 0, 0),
            align: Align::TopLeft,
        }
    }

    /// Builder: set margin insets.
    #[must_use]
    pub const fn with_margin(mut self, insets: Rect) -> Self {
        self.margin = insets;
        self
    }

    /// Builder: set padding insets.
    #[must_use]
    pub const fn with_padding(mut self, insets: Rect) -> Self {
        self.padding = insets;
        self
    }

    /// Builder: set the alignment anchor.
    #[must_use]
    pub const fn with_align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }
}

/// Axis-aligned rectangle in top-left-origin pixel coordinates.
///
/// v0 §5.11 geometry primitive: `u32` fields only. Negative offsets
/// and sub-pixel positioning are §5.3 DSL territory and intentionally
/// excluded from this minimal schema — taffy-driven flexbox/grid
/// (§5.11 decision) supersedes absolute geometry as that surface
/// lands.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }
}

/// Rectangular primitive — the layout-and-fill workhorse.
///
/// `rect` is the v0 absolute pixel geometry; `style` is the §5.3 R20
/// [`BoxStyle`] sidecar (fill / border / corner radius). Gradient /
/// shadow fills + taffy-driven layout are §5.3 carry-forward.
///
/// `tag` is the §5.20 intent-system carrier. `None` means "no
/// symbolic identifier"; an attached tag lets a widget identify which
/// emitted intent this node belongs to (e.g. `"save_btn"`).
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct BoxNode {
    pub rect: Rect,
    pub style: BoxStyle,
    pub layout: LayoutStyle,
    pub tag: Option<Cow<'static, str>>,
}

impl BoxNode {
    /// Construct a `BoxNode` from a rect and a fully-specified style.
    #[must_use]
    pub const fn new(rect: Rect, style: BoxStyle) -> Self {
        Self {
            rect,
            style,
            layout: LayoutStyle::new(),
            tag: None,
        }
    }

    /// Solid-fill shorthand: `rect` + a fill `Color`, no border, no
    /// rounding. Equivalent to `BoxNode::new(rect, BoxStyle::filled(fill))`
    /// and minimizes churn at the dozens of call sites that just want
    /// "a coloured rectangle".
    #[must_use]
    pub const fn filled(rect: Rect, fill: Color) -> Self {
        Self::new(rect, BoxStyle::filled(fill))
    }

    /// Attach a §5.20 intent tag to this node (builder form).
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<Cow<'static, str>>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Attach a §5.21 layout style (builder form).
    #[must_use]
    pub const fn with_layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }
}

/// Styled text primitive.
///
/// v0 §5.11 shape: `content: String` carries the raw string payload;
/// `rect: Rect` gives absolute bounds in the same u32 coordinate
/// space as `BoxNode`; `style: TextStyle` carries font + colour per
/// §5.3 R20. The cosmic-text rasterizer lands in a later R21 slice
/// and consumes `style` directly.
///
/// `tag` is the §5.20 intent-system carrier (see [`BoxNode::tag`]).
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct TextNode {
    pub content: String,
    pub rect: Rect,
    pub style: TextStyle,
    pub layout: LayoutStyle,
    pub tag: Option<Cow<'static, str>>,
    /// Number of *visual* lines (UAX #14 line break opportunities the
    /// shaper actually broke at) this content resolved into against
    /// `rect.w`. R51.1 §5.12 — measured-result sidecar populated by
    /// `pinion-runtime::compute_layout`'s measure pass.
    ///
    /// **Semantic** (locked to UAX #14 visual-line counting so the
    /// backend swap from parley to the §5.37.7 self-hosted line break
    /// engine is observationally stable):
    /// * **Counts visual lines, not logical paragraphs** — soft line
    ///   breaks induced by `rect.w` constraints count; hard line
    ///   breaks (`U+000A` etc.) within `content` count.
    /// * **BIDI is irrelevant to the count** — mixed LTR / RTL runs
    ///   resolved by UBA (§5.37.4) occupy the same visual line iff
    ///   they sit between the same pair of break opportunities. A
    ///   single bidi-mixed line is `1`, not `2`.
    /// * **`content.is_empty()` → `1`** — UAX #14 treats the empty
    ///   string as a single zero-width line; the §5.37.7 engine and
    ///   parley both report this consistently.
    /// * **`0` is a sentinel** — no shape pass has run yet
    ///   (`TextNode::new` / `TextNode::styled` defaults). Distinct
    ///   from any valid measured count.
    ///
    /// The §5.12 `scene/layout` RPC surfaces this as
    /// `LayoutNode.line_count` so AI clients verify single-line text
    /// without pixel inspection (Scene-as-data invariant §2 #7).
    pub line_count: u32,
}

impl TextNode {
    /// Construct a text node with the default [`TextStyle`] (system
    /// font, 16px, opaque black). Use [`TextNode::styled`] when an
    /// explicit style is needed.
    #[must_use]
    pub fn new(content: impl Into<String>, rect: Rect) -> Self {
        Self::styled(content, rect, TextStyle::new())
    }

    /// Construct with a fully-specified [`TextStyle`].
    #[must_use]
    pub fn styled(content: impl Into<String>, rect: Rect, style: TextStyle) -> Self {
        Self {
            content: content.into(),
            rect,
            style,
            layout: LayoutStyle::new(),
            tag: None,
            line_count: 0,
        }
    }

    /// Attach a §5.20 intent tag to this node (builder form).
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<Cow<'static, str>>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Attach a §5.21 layout style (builder form).
    #[must_use]
    pub fn with_layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }
}

/// Path control point in f32 sub-pixel space (§5.3 R20).
///
/// Path geometry uses floating-point coordinates because curve
/// rasterizers (vello, lyon, cosmic-text glyph outlines) all operate
/// in sub-pixel space; the integer-pixel [`Rect`] still serves as
/// the layout / hit-test bounding box.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PathPoint {
    pub x: f32,
    pub y: f32,
}

impl PathPoint {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Structured path command per §5.3 R20.
///
/// Replaces the previous R17 opaque `data: String` (SVG-d payload).
/// Curve commands use a single cubic Bézier; quadratic / arc / etc.
/// are carry-forward.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCommand {
    MoveTo(PathPoint),
    LineTo(PathPoint),
    CurveTo {
        c1: PathPoint,
        c2: PathPoint,
        end: PathPoint,
    },
    Close,
}

/// Vector path primitive.
///
/// v0 §5.3 R20 shape: `commands: Vec<PathCommand>` is the structured
/// command stream the rasterizer consumes; `rect: Rect` is the
/// absolute pixel bounding box for layout / hit-test; `style:
/// PathStyle` carries stroke and fill specifications.
///
/// `tag` is the §5.20 intent-system carrier (see [`BoxNode::tag`]).
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct PathNode {
    pub commands: Vec<PathCommand>,
    pub rect: Rect,
    pub style: PathStyle,
    pub layout: LayoutStyle,
    pub tag: Option<Cow<'static, str>>,
}

impl PathNode {
    /// Construct a path node from its rect, command stream, and style.
    #[must_use]
    pub fn new(rect: Rect, commands: Vec<PathCommand>, style: PathStyle) -> Self {
        Self {
            commands,
            rect,
            style,
            layout: LayoutStyle::new(),
            tag: None,
        }
    }

    /// Empty path with a bounding box only — primarily a fixture for
    /// tests that need a `PathNode` without specifying commands.
    #[must_use]
    pub fn empty(rect: Rect) -> Self {
        Self::new(rect, Vec::new(), PathStyle::default())
    }

    /// Attach a §5.20 intent tag to this node (builder form).
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<Cow<'static, str>>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Attach a §5.21 layout style (builder form).
    #[must_use]
    pub fn with_layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }
}

/// Raster or vector image primitive.
///
/// v0 §5.3 R20 shape: `source: String` is the opaque locator
/// (`file://`, `https://`, `memory://0xABCD`, etc.); `rect: Rect`
/// gives the destination bounds; `style: ImageStyle` carries the fit
/// policy and optional tint. The codec / decoded-buffer cache is
/// carry-forward and resolved by the consumer rasterizer.
///
/// `tag` is the §5.20 intent-system carrier (see [`BoxNode::tag`]).
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct ImageNode {
    pub source: String,
    pub rect: Rect,
    pub style: ImageStyle,
    pub layout: LayoutStyle,
    pub tag: Option<Cow<'static, str>>,
}

impl ImageNode {
    /// Construct an image node with the default [`ImageStyle`]
    /// (`Fit::Fill`, no tint). Use [`ImageNode::styled`] when an
    /// explicit style is needed.
    #[must_use]
    pub fn new(source: impl Into<String>, rect: Rect) -> Self {
        Self::styled(source, rect, ImageStyle::default())
    }

    /// Construct with a fully-specified [`ImageStyle`].
    #[must_use]
    pub fn styled(source: impl Into<String>, rect: Rect, style: ImageStyle) -> Self {
        Self {
            source: source.into(),
            rect,
            style,
            layout: LayoutStyle::new(),
            tag: None,
        }
    }

    /// Attach a §5.20 intent tag to this node (builder form).
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<Cow<'static, str>>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Attach a §5.21 layout style (builder form).
    #[must_use]
    pub fn with_layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }
}

/// Child layout container.
///
/// v0 §5.11 shape: holds `children: Vec<Scene>` for structural
/// grouping; taffy-driven flexbox/grid layout (§5.11 decision)
/// arrives with the §5.3 DSL. `Clone` is intentionally *not* derived
/// — `Scene` carries `ExternalNode` (`Box<dyn External>`) which has
/// no general clone strategy, see [`Scene`] doc.
///
/// `tag` is the §5.20 intent-system carrier (see [`BoxNode::tag`]).
#[non_exhaustive]
#[derive(Debug, Default)]
pub struct ContainerNode {
    pub children: Vec<Scene>,
    pub rect: Rect,
    /// R24 slice 5: containers can carry their own visual style
    /// (fill / border / corner radius) so they double as "div" —
    /// the natural carrier for backgrounds without absolute
    /// positioning.
    pub style: BoxStyle,
    pub layout: LayoutStyle,
    pub tag: Option<Cow<'static, str>>,
    /// R51.69 §5.40 — explicit accessible-name override. When `None`,
    /// the §5.40 access-tree builder derives the AT-exposed name from
    /// the container's first descendant [`TextNode::content`] (matches
    /// WAI-ARIA's "name from contents" derivation for `button`,
    /// `checkbox`, etc.). When `Some`, this string wins, mirroring
    /// HTML's `aria-label` attribute precedence over inner text.
    ///
    /// Lives on `ContainerNode` rather than `Modifier` because every
    /// widget instance currently roots in a tagged `Container` and the
    /// accessible name is a per-node attribute in WAI-ARIA (1.2 §5.2),
    /// not a layout/transform adjustment.
    pub aria_label: Option<Cow<'static, str>>,
}

impl ContainerNode {
    #[must_use]
    pub fn new(children: Vec<Scene>) -> Self {
        Self {
            children,
            rect: Rect::default(),
            style: BoxStyle::default(),
            layout: LayoutStyle::new(),
            tag: None,
            aria_label: None,
        }
    }

    /// Attach a §5.20 intent tag to this node (builder form).
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<Cow<'static, str>>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Attach a §5.21 layout style (builder form).
    #[must_use]
    pub const fn with_layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }

    /// Attach a §5.3 [`BoxStyle`] — the container paints its own fill /
    /// border before recursing into children. v0 covers fill +
    /// `corner_radius` via the same shape as `BoxNode`.
    #[must_use]
    pub const fn with_style(mut self, style: BoxStyle) -> Self {
        self.style = style;
        self
    }

    /// R51.69 §5.40 — attach an explicit accessible-name override.
    /// Wins over the default "first descendant `TextNode` content"
    /// derivation that the access-tree builder performs (WAI-ARIA 1.2
    /// `aria-label` precedence). Use when the visible label and the
    /// AT-exposed name should diverge (icon-only buttons, localized
    /// names, etc.). Empty string is preserved verbatim — callers that
    /// want "no override" should not set the field.
    #[must_use]
    pub fn with_aria_label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.aria_label = Some(label.into());
        self
    }
}

/// Opaque shader/GPU effect escape (§3 capability boundary).
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct EffectNode {}

impl EffectNode {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

/// Opaque embedded-content escape (§3 capability boundary). Owns the
/// `External` author's handle behind a `Box<dyn External>`; the §5.15
/// 8-item contract governs the integration surface.
///
/// `tag` is the §5.20 intent-system carrier. When set, the runtime
/// [`walk_scene_and_drain`](../../pinion_runtime/fn.walk_scene_and_drain.html)
/// prefixes every drained intent's tag with `<tag>.` — completing
/// the `<widget>.<kind>` convention (R22).
///
/// Not `Clone` — `Box<dyn External>` has no generic clone strategy,
/// see [`Scene`] doc for the introspection-based alternative.
#[non_exhaustive]
#[derive(Debug)]
pub struct ExternalNode {
    pub handle: Box<dyn crate::external::External>,
    pub rect: Rect,
    pub layout: LayoutStyle,
    pub tag: Option<Cow<'static, str>>,
}

impl ExternalNode {
    #[must_use]
    pub fn new(handle: Box<dyn crate::external::External>) -> Self {
        Self {
            handle,
            rect: Rect::default(),
            layout: LayoutStyle::new(),
            tag: None,
        }
    }

    /// Attach a §5.20 intent tag — drained intents from this node
    /// will be prefixed with `<tag>.` by the runtime walk.
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<Cow<'static, str>>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Attach a §5.21 layout style (builder form).
    #[must_use]
    pub fn with_layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::{
        Backend, CountedExternal, External, IntrospectValue, StubExternal,
    };

    fn stub_handle() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    #[test]
    fn all_seven_variants_construct() {
        let _ = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        let _ = Scene::Text(TextNode::new("", Rect::default()));
        let _ = Scene::Path(PathNode::empty(Rect::default()));
        let _ = Scene::Image(ImageNode::new("", Rect::default()));
        let _ = Scene::Container(ContainerNode::new(vec![]));
        let _ = Scene::Effect(EffectNode::new());
        let _ = Scene::External(ExternalNode::new(stub_handle()));
    }

    #[test]
    fn match_arm_exhaustive_within_crate() {
        // Inside the defining crate `#[non_exhaustive]` does not force a
        // wildcard arm, so this exhaustive match doubles as a guard: if
        // someone adds a Scene variant they must touch this test.
        let s = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        match s {
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::Container(_)
            | Scene::Effect(_)
            | Scene::External(_) => {}
        }
    }

    #[test]
    fn box_node_fill_round_trips_through_scene() {
        // Construction stores the packed ARGB fill; pattern-match
        // extracts it bit-for-bit. Guards the v0 §5.11 field schema
        // before §5.3 DSL settles geometry/style.
        let argb = 0x00ab_cdef;
        let scene = Scene::Box(BoxNode::filled(Rect::default(), Color::from_argb(argb)));
        match scene {
            Scene::Box(node) => assert_eq!(node.style.fill.to_argb(), argb),
            _ => panic!("expected Box variant"),
        }
    }

    #[test]
    fn box_node_rect_round_trips_through_scene() {
        // v0 §5.11 geometry: Rect carries x/y/w/h as u32, lossless
        // round-trip through Scene::Box.
        let rect = Rect::new(10, 20, 160, 80);
        let scene = Scene::Box(BoxNode::filled(rect, Color::default()));
        match scene {
            Scene::Box(node) => assert_eq!(node.rect, rect),
            _ => panic!("expected Box variant"),
        }
    }

    #[test]
    fn path_node_commands_and_rect_round_trip_through_scene() {
        // R20 §5.3 lock: PathNode carries a typed `Vec<PathCommand>`
        // (replacing the prior opaque SVG-d `data: String`) plus
        // `rect` for layout/hit and `style: PathStyle` for the
        // stroke/fill spec the rasterizer consumes.
        let commands = vec![
            PathCommand::MoveTo(PathPoint::new(10.0, 10.0)),
            PathCommand::LineTo(PathPoint::new(20.0, 20.0)),
            PathCommand::Close,
        ];
        let node = PathNode::new(
            Rect::new(0, 0, 32, 32),
            commands.clone(),
            PathStyle::filled(Color::from_argb(0x00ff_ffff)),
        );
        let scene = Scene::Path(node);
        match scene {
            Scene::Path(p) => {
                assert_eq!(p.commands, commands);
                assert_eq!(p.rect, Rect::new(0, 0, 32, 32));
                assert_eq!(p.style.fill, Some(Color::from_argb(0x00ff_ffff)));
            }
            _ => panic!("expected Path variant"),
        }
    }

    #[test]
    fn image_node_source_and_rect_round_trip_through_scene() {
        // v0 §5.11 Image shape: opaque `source` locator + `rect`.
        // The framework does not interpret the URI scheme; the
        // consumer loader does (file://, https://, memory:// …).
        let node = ImageNode::new("file:///tmp/icon.png", Rect::new(8, 8, 24, 24));
        let scene = Scene::Image(node);
        match scene {
            Scene::Image(i) => {
                assert_eq!(i.source, "file:///tmp/icon.png");
                assert_eq!(i.rect, Rect::new(8, 8, 24, 24));
            }
            _ => panic!("expected Image variant"),
        }
    }

    #[test]
    fn text_node_content_and_rect_round_trip_through_scene() {
        // v0 §5.11 Text shape: content (String) + rect (Rect) survive
        // round-trip through Scene::Text. Locks the minimal schema
        // before the cosmic-text rasterizer slice fills in style.
        let node = TextNode::new("Click me!", Rect::new(96, 84, 128, 32));
        let scene = Scene::Text(node);
        match scene {
            Scene::Text(t) => {
                assert_eq!(t.content, "Click me!");
                assert_eq!(t.rect, Rect::new(96, 84, 128, 32));
            }
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn container_node_children_round_trip_through_scene() {
        // v0 §5.11 Container shape: Vec<Scene> children preserve
        // order and variant identity through pattern-match.
        let children = vec![
            Scene::Box(BoxNode::filled(
                Rect::new(0, 0, 10, 10),
                Color::from_argb(0x00ff_0000),
            )),
            Scene::Box(BoxNode::filled(
                Rect::new(20, 20, 5, 5),
                Color::from_argb(0x0000_ff00),
            )),
        ];
        let scene = Scene::Container(ContainerNode::new(children));
        match scene {
            Scene::Container(node) => {
                assert_eq!(node.children.len(), 2);
                match &node.children[0] {
                    Scene::Box(b) => assert_eq!(b.style.fill, Color::from_argb(0x00ff_0000)),
                    _ => panic!("child 0 not Box"),
                }
            }
            _ => panic!("expected Container variant"),
        }
    }

    #[test]
    fn modifier_default_is_identity() {
        let m = Modifier::new();
        assert_eq!(m.margin, Rect::new(0, 0, 0, 0));
        assert_eq!(m.padding, Rect::new(0, 0, 0, 0));
        assert_eq!(m.align, Align::TopLeft);
        let d = Modifier::default();
        assert_eq!(m.margin, d.margin);
    }

    #[test]
    fn box_with_layout_round_trips_through_scene() {
        // R24 slice 2 §5.21: introspectable variants carry a
        // LayoutStyle sidecar. Default is `Display::Block`; opt-in
        // builders switch to flex.
        use crate::style::{Display, FlexDirection};
        let layout = LayoutStyle::new().flex(FlexDirection::Column);
        let scene = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default()).with_layout(layout),
        );
        match scene {
            Scene::Box(node) => {
                assert_eq!(node.layout.display, Display::Flex);
                assert_eq!(node.layout.flex_direction, FlexDirection::Column);
            }
            _ => panic!("expected Box"),
        }
    }

    #[test]
    fn container_layout_defaults_to_block() {
        use crate::style::Display;
        let c = ContainerNode::new(vec![]);
        assert_eq!(c.layout.display, Display::Block);
    }

    #[test]
    fn modifier_with_margin_padding_align_builders() {
        // R20 §5.3: Rect field reused as 4-tuple inset (x=left,
        // y=top, w=right, h=bottom).
        let m = Modifier::new()
            .with_margin(Rect::new(4, 8, 4, 8))
            .with_padding(Rect::new(2, 2, 2, 2))
            .with_align(Align::Center);
        assert_eq!(m.margin, Rect::new(4, 8, 4, 8));
        assert_eq!(m.padding, Rect::new(2, 2, 2, 2));
        assert_eq!(m.align, Align::Center);
    }

    #[test]
    fn external_handle_dispatches_through_scene() {
        // Pattern-match the External variant and dispatch a contract
        // method through the trait object — proves Box<dyn External>
        // round-trips through the scene tree.
        let scene = Scene::External(ExternalNode::new(stub_handle()));
        match scene {
            Scene::External(node) => {
                let support = node.handle.backends();
                assert!(support.supports(Backend::Gui));
                assert!(!support.supports(Backend::Tui));
            }
            _ => panic!("expected External variant"),
        }
    }

    #[test]
    fn box_node_tag_defaults_to_none() {
        // v0 §5.20: a freshly constructed introspectable node carries
        // no intent tag. `with_tag` is the opt-in carrier — guards
        // against accidental default-tagging.
        let node = BoxNode::filled(Rect::default(), Color::default());
        assert!(node.tag.is_none());
    }

    #[test]
    fn box_node_with_tag_round_trips_through_scene() {
        // §5.20 intent tag persistence: attaching `"save_btn"` on a
        // BoxNode survives the Scene::Box wrap and pattern-match.
        let scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()).with_tag("save_btn"));
        match scene {
            Scene::Box(node) => assert_eq!(node.tag.as_deref(), Some("save_btn")),
            _ => panic!("expected Box variant"),
        }
    }

    #[test]
    fn text_path_image_with_tag_round_trip() {
        let t = TextNode::new("hi", Rect::default()).with_tag("title");
        assert_eq!(t.tag.as_deref(), Some("title"));
        let p = PathNode::empty(Rect::default()).with_tag("logo");
        assert_eq!(p.tag.as_deref(), Some("logo"));
        let i = ImageNode::new("file://x", Rect::default()).with_tag("avatar");
        assert_eq!(i.tag.as_deref(), Some("avatar"));
    }

    #[test]
    fn container_tag_persists_with_tagged_box_child() {
        // §5.20 nesting: a tagged Box inside a tagged Container
        // round-trips both tags through pattern-match.
        let inner = Scene::Box(BoxNode::filled(Rect::default(), Color::default()).with_tag("inner_btn"));
        let scene = Scene::Container(ContainerNode::new(vec![inner]).with_tag("toolbar"));
        match scene {
            Scene::Container(c) => {
                assert_eq!(c.tag.as_deref(), Some("toolbar"));
                match &c.children[0] {
                    Scene::Box(b) => assert_eq!(b.tag.as_deref(), Some("inner_btn")),
                    _ => panic!("child not Box"),
                }
            }
            _ => panic!("expected Container variant"),
        }
    }

    // ---- §5.32 R39: Scene::hit_test ----

    fn box_at(x: u32, y: u32, w: u32, h: u32) -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(x, y, w, h), Color::default()))
    }

    fn tagged_box_at(x: u32, y: u32, w: u32, h: u32, tag: &'static str) -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(x, y, w, h), Color::default()).with_tag(tag))
    }

    fn container_at(x: u32, y: u32, w: u32, h: u32, children: Vec<Scene>) -> Scene {
        let mut node = ContainerNode::new(children);
        node.rect = Rect::new(x, y, w, h);
        Scene::Container(node)
    }

    #[test]
    fn hit_test_inside_lone_box_returns_empty_segments() {
        let s = box_at(10, 10, 50, 30);
        let hit = s.hit_test(20, 15).expect("inside");
        assert!(hit.segments.is_empty(), "root hit = empty segments");
        assert_eq!(hit.bbox, Rect::new(10, 10, 50, 30));
    }

    #[test]
    fn hit_test_outside_lone_box_returns_none() {
        let s = box_at(10, 10, 50, 30);
        assert!(s.hit_test(5, 5).is_none(), "left of rect");
        assert!(s.hit_test(60, 40).is_none(), "right/below rect");
        // Right edge is exclusive (half-open) — x = rect.x + rect.w is OUT.
        assert!(s.hit_test(60, 20).is_none(), "right edge exclusive");
    }

    #[test]
    fn hit_test_zero_area_rect_never_hits() {
        let s = Scene::Box(BoxNode::filled(Rect::new(10, 10, 0, 0), Color::default()));
        assert!(s.hit_test(10, 10).is_none(), "zero-area rect cannot contain");
    }

    #[test]
    fn hit_test_container_with_unmatched_children_returns_root() {
        // Container at (0,0,100,100); child at (200,200,10,10) — point
        // inside container but outside all children. Hit returns the
        // container itself (empty segments).
        let s = container_at(
            0,
            0,
            100,
            100,
            vec![box_at(200, 200, 10, 10)],
        );
        let hit = s.hit_test(50, 50).expect("inside container");
        assert!(hit.segments.is_empty(), "container itself is the hit");
    }

    #[test]
    fn hit_test_container_picks_matching_child_with_index_segment() {
        // Container at (0,0,200,200); two untagged children. Hit lands
        // on the second child → segment "1".
        let s = container_at(
            0,
            0,
            200,
            200,
            vec![box_at(0, 0, 50, 50), box_at(100, 100, 50, 50)],
        );
        let hit = s.hit_test(120, 120).expect("on child 1");
        assert_eq!(hit.segments, vec!["1".to_string()]);
        assert_eq!(hit.bbox, Rect::new(100, 100, 50, 50));
    }

    #[test]
    fn hit_test_tagged_child_uses_tag_in_segment() {
        let s = container_at(0, 0, 200, 200, vec![tagged_box_at(10, 10, 50, 50, "save_btn")]);
        let hit = s.hit_test(20, 20).expect("on tagged child");
        assert_eq!(hit.segments, vec!["save_btn".to_string()]);
    }

    #[test]
    fn hit_test_overlapping_siblings_topmost_last_wins() {
        // Two boxes covering the same point; the later index wins
        // (drawn last = topmost in §5.2 paint order).
        let s = container_at(
            0,
            0,
            200,
            200,
            vec![box_at(50, 50, 100, 100), box_at(50, 50, 100, 100)],
        );
        let hit = s.hit_test(75, 75).expect("on overlap");
        assert_eq!(hit.segments, vec!["1".to_string()]);
    }

    #[test]
    fn hit_test_nested_containers_build_segment_chain() {
        // Outer at (0,0,200,200) → inner at (0,0,100,100) → box at (10,10,50,50).
        let inner = container_at(0, 0, 100, 100, vec![box_at(10, 10, 50, 50)]);
        let outer = container_at(0, 0, 200, 200, vec![inner]);
        let hit = outer.hit_test(20, 20).expect("deep nested");
        assert_eq!(hit.segments, vec!["0".to_string(), "0".to_string()]);
        assert_eq!(hit.bbox, Rect::new(10, 10, 50, 50));
    }

    #[test]
    fn hit_test_effect_variant_is_skipped() {
        // EffectNode has no geometry — never hit-testable. Wrap it in
        // a container and verify the container itself catches the hit.
        let s = container_at(0, 0, 100, 100, vec![Scene::Effect(EffectNode::new())]);
        let hit = s.hit_test(50, 50).expect("container takes hit");
        // Effect was skipped — container is the deepest match.
        assert!(hit.segments.is_empty());
    }

    #[test]
    fn scene_rect_accessor_returns_per_variant_rect() {
        let r = Rect::new(5, 7, 11, 13);
        assert_eq!(Scene::Box(BoxNode::filled(r, Color::default())).rect(), r);
        assert_eq!(Scene::Text(TextNode::new("x", r)).rect(), r);
        assert_eq!(Scene::Path(PathNode::empty(r)).rect(), r);
        assert_eq!(Scene::Image(ImageNode::new("file://", r)).rect(), r);
        // Effect has no rect — default
        assert_eq!(Scene::Effect(EffectNode::new()).rect(), Rect::default());
    }

    // ---- §5.32 R39.2: Scene::hit_test_region ----

    #[test]
    fn hit_test_region_empty_query_returns_empty() {
        let s = container_at(
            0, 0, 200, 200,
            vec![box_at(10, 10, 50, 50), box_at(100, 100, 30, 30)],
        );
        // Zero-area query never intersects.
        assert!(s.hit_test_region(50, 50, 0, 0).is_empty());
    }

    #[test]
    fn hit_test_region_covering_everything_returns_root_plus_children() {
        let s = container_at(
            0, 0, 200, 200,
            vec![box_at(10, 10, 50, 50), box_at(100, 100, 30, 30)],
        );
        let hits = s.hit_test_region(0, 0, 200, 200);
        // 1 container + 2 boxes = 3 entries
        assert_eq!(hits.len(), 3);
        // First entry is the container itself (empty segments)
        assert!(hits[0].segments.is_empty());
        // Children follow in declaration order
        assert_eq!(hits[1].segments, vec!["0".to_string()]);
        assert_eq!(hits[2].segments, vec!["1".to_string()]);
    }

    #[test]
    fn hit_test_region_partial_overlap_returns_only_intersecting() {
        let s = container_at(
            0, 0, 200, 200,
            vec![box_at(10, 10, 50, 50), box_at(100, 100, 30, 30)],
        );
        // Region covers only the second child + container.
        let hits = s.hit_test_region(90, 90, 50, 50);
        assert_eq!(hits.len(), 2, "container + box1, not box0");
        assert!(hits[0].segments.is_empty());
        assert_eq!(hits[1].segments, vec!["1".to_string()]);
    }

    #[test]
    fn hit_test_region_skips_effect_variant() {
        let s = container_at(
            0, 0, 100, 100,
            vec![Scene::Effect(EffectNode::new()), box_at(10, 10, 20, 20)],
        );
        let hits = s.hit_test_region(0, 0, 100, 100);
        // Container + box; Effect (index 0) skipped, so box's index is still 1.
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[1].segments, vec!["1".to_string()]);
    }

    // ---- §5.32 R39.3: Scene::lookup_path ----

    #[test]
    fn lookup_path_empty_segments_returns_root_rect() {
        let s = box_at(5, 7, 11, 13);
        assert_eq!(s.lookup_path(&[]), Some(Rect::new(5, 7, 11, 13)));
    }

    #[test]
    fn lookup_path_resolves_index_segment() {
        let s = container_at(
            0, 0, 200, 200,
            vec![box_at(10, 10, 20, 20), box_at(50, 50, 30, 30)],
        );
        assert_eq!(s.lookup_path(&["1".to_string()]), Some(Rect::new(50, 50, 30, 30)));
    }

    #[test]
    fn lookup_path_resolves_tag_segment() {
        let s = container_at(0, 0, 200, 200, vec![tagged_box_at(10, 10, 20, 20, "btn")]);
        assert_eq!(s.lookup_path(&["btn".to_string()]), Some(Rect::new(10, 10, 20, 20)));
    }

    #[test]
    fn lookup_path_nested_chain() {
        let inner = container_at(0, 0, 100, 100, vec![box_at(10, 10, 50, 50)]);
        let outer = container_at(0, 0, 200, 200, vec![inner]);
        assert_eq!(
            outer.lookup_path(&["0".to_string(), "0".to_string()]),
            Some(Rect::new(10, 10, 50, 50)),
        );
    }

    #[test]
    fn lookup_path_unknown_segment_returns_none() {
        let s = container_at(0, 0, 100, 100, vec![box_at(0, 0, 10, 10)]);
        assert!(s.lookup_path(&["ghost".to_string()]).is_none());
        assert!(s.lookup_path(&["99".to_string()]).is_none());
    }

    #[test]
    fn lookup_path_through_non_container_returns_none() {
        // Box at root + segment "0" — Box has no children, lookup fails.
        let s = box_at(0, 0, 10, 10);
        assert!(s.lookup_path(&["0".to_string()]).is_none());
    }

    // ---- §5.34 R40.10: Scene::lookup_path_mut ----

    #[test]
    fn lookup_path_mut_empty_segments_returns_root() {
        let mut s = box_at(1, 2, 3, 4);
        let node = s.lookup_path_mut(&[]).expect("root resolves");
        assert!(matches!(node, Scene::Box(_)));
    }

    #[test]
    fn lookup_path_mut_resolves_tag_segment_and_allows_mutation() {
        // Walking by tag returns &mut to the matched child; mutating
        // it through the returned reference must persist after the
        // borrow ends.
        let mut s = container_at(0, 0, 200, 200, vec![tagged_box_at(0, 0, 10, 10, "btn")]);
        {
            let node = s
                .lookup_path_mut(&["btn".to_string()])
                .expect("tag resolves");
            if let Scene::Box(b) = node {
                b.rect = Rect::new(99, 99, 99, 99);
            }
        }
        // Confirm the mutation landed via the immutable counterpart.
        assert_eq!(s.lookup_path(&["btn".to_string()]), Some(Rect::new(99, 99, 99, 99)));
    }

    #[test]
    fn lookup_path_mut_resolves_index_segment() {
        let mut s = container_at(
            0, 0, 200, 200,
            vec![box_at(10, 10, 20, 20), box_at(50, 50, 30, 30)],
        );
        let node = s
            .lookup_path_mut(&["1".to_string()])
            .expect("index 1 resolves");
        assert!(matches!(node, Scene::Box(_)));
    }

    #[test]
    fn lookup_path_mut_nested_chain() {
        let inner = container_at(0, 0, 100, 100, vec![box_at(10, 10, 50, 50)]);
        let mut outer = container_at(0, 0, 200, 200, vec![inner]);
        let node = outer
            .lookup_path_mut(&["0".to_string(), "0".to_string()])
            .expect("nested resolves");
        assert!(matches!(node, Scene::Box(_)));
    }

    #[test]
    fn lookup_path_mut_unknown_segment_returns_none() {
        let mut s = container_at(0, 0, 100, 100, vec![box_at(0, 0, 10, 10)]);
        assert!(s.lookup_path_mut(&["ghost".to_string()]).is_none());
        assert!(s.lookup_path_mut(&["99".to_string()]).is_none());
    }

    #[test]
    fn lookup_path_mut_through_non_container_returns_none() {
        let mut s = box_at(0, 0, 10, 10);
        assert!(s.lookup_path_mut(&["0".to_string()]).is_none());
    }

    #[test]
    fn lookup_path_mut_skips_effect_variant() {
        let mut s = Scene::Effect(EffectNode::new());
        assert!(s.lookup_path_mut(&[]).is_none(), "Effect never resolves, even at root");
    }

    // ---- §5.34 R42: Scene::lookup_path_ref ----

    #[test]
    fn lookup_path_ref_empty_segments_returns_root() {
        let s = box_at(1, 2, 3, 4);
        let node = s.lookup_path_ref(&[]).expect("root resolves");
        assert!(matches!(node, Scene::Box(_)));
    }

    #[test]
    fn lookup_path_ref_resolves_tag_segment() {
        let s = container_at(0, 0, 200, 200, vec![tagged_box_at(0, 0, 10, 10, "btn")]);
        let node = s.lookup_path_ref(&["btn".to_string()]).expect("tag");
        assert!(matches!(node, Scene::Box(_)));
        assert_eq!(node.tag(), Some("btn"));
    }

    #[test]
    fn lookup_path_ref_nested_chain() {
        let inner = container_at(0, 0, 100, 100, vec![box_at(10, 10, 50, 50)]);
        let outer = container_at(0, 0, 200, 200, vec![inner]);
        let node = outer
            .lookup_path_ref(&["0".to_string(), "0".to_string()])
            .expect("nested");
        assert_eq!(node.rect(), Rect::new(10, 10, 50, 50));
    }

    #[test]
    fn lookup_path_ref_unknown_returns_none() {
        let s = container_at(0, 0, 100, 100, vec![box_at(0, 0, 10, 10)]);
        assert!(s.lookup_path_ref(&["ghost".to_string()]).is_none());
    }

    #[test]
    fn lookup_path_ref_skips_effect_variant() {
        let s = Scene::Effect(EffectNode::new());
        assert!(s.lookup_path_ref(&[]).is_none());
    }

    #[test]
    fn hit_test_region_uses_tag_in_path() {
        let s = container_at(
            0, 0, 200, 200,
            vec![tagged_box_at(10, 10, 50, 50, "save_btn")],
        );
        let hits = s.hit_test_region(0, 0, 200, 200);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[1].segments, vec!["save_btn".to_string()]);
    }

    #[test]
    fn hit_test_region_disjoint_returns_empty() {
        let s = container_at(0, 0, 100, 100, vec![box_at(10, 10, 50, 50)]);
        // Query far away from any rect.
        let hits = s.hit_test_region(500, 500, 50, 50);
        assert!(hits.is_empty());
    }

    #[test]
    fn scene_tag_accessor_round_trips() {
        let tagged = tagged_box_at(0, 0, 10, 10, "x");
        assert_eq!(tagged.tag(), Some("x"));
        let untagged = box_at(0, 0, 10, 10);
        assert_eq!(untagged.tag(), None);
        assert_eq!(Scene::Effect(EffectNode::new()).tag(), None);
    }

    #[test]
    fn introspection_reaches_through_scene_with_counted() {
        // CountedExternal opts in to §5.15 item 8. Embed it in the
        // scene tree, then traverse to its introspect surface.
        let scene = Scene::External(ExternalNode::new(Box::new(CountedExternal::new(5))));
        match scene {
            Scene::External(node) => {
                let intro = node
                    .handle
                    .introspect()
                    .expect("CountedExternal opts in to introspection");
                assert_eq!(intro.query("count"), Some(IntrospectValue::Int(5)));
            }
            _ => panic!("expected External variant"),
        }
    }
}
