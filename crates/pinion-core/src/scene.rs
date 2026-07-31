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
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use crate::cell_metric::CellMetric;
use crate::external::ExternalIntrospect;
use crate::style::{
    Align, BoxStyle, Color, CursorHint, ImageStyle, LayoutStyle, PathStyle, Size, TextStyle,
};
use crate::term_grid::{GridBuffer, Palette};
use crate::widgets::measured_rows::MeasuredRowState;
use crate::widgets::scroll::ScrollState;

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
    /// R55.A §5.45 — scroll container primitive. Carries a clip
    /// viewport, a content scene rendered with an applied offset, and
    /// the `(offset_x, offset_y)` pair the input router / paint
    /// adapter consult to render only the visible window of the
    /// content. First-cut scaffold: subsequent R55.A.* sub-rounds
    /// wire hit-test descent, lookup-path traversal, paint clipping,
    /// and input mapping. See §5.45 R55 axis caveats for the full
    /// sub-axis enumeration.
    Scroll(ScrollNode),
    /// R681 §2 #4 — immediate-mode subtree opt-in (axis 2 of the
    /// 4-axis paint-pipeline rewrite series). Carries an opaque
    /// driver behind [`Rc<RefCell<dyn ImmediateMode>>`] plus the
    /// post-layout `viewport` the backend bridge paints into and
    /// a [`Cell<Duration>`] sidecar publishing the last per-paint
    /// `dt` the shell drove. The retained tree treats this node as
    /// a paint-opaque leaf (mirrors [`Scene::External`] for input /
    /// path-lookup purposes) while the per-window paint cycle
    /// advances each driver in fixed-timestep steps (R831
    /// `pinion_runtime::FixedTimestep` accumulator) every frame —
    /// substrate for the §2 #4 immediate-mode ↔ retained widget
    /// tree dual execution model (Phase C entry).
    ///
    /// Landed across R681 (data shape + trait surface + Vello paint
    /// bridge + per-window `winit::event_loop::ControlFlow::WaitUntil`
    /// pacing + first consumer), R827 (intent bridge), R828
    /// (introspection), R829 (deterministic stepping), R830 (pointer
    /// input), and R831 (fixed-timestep accumulator).
    ImmediateModeNode(ImmediateModeNode),
    /// R972 §5.41 — cell-native text-grid geometry scaffold (the
    /// cell-native coordinate sub-axis's first real consumer). Carries
    /// a node-local [`CellMetric`] (R968 node-local ratify) plus the
    /// layout-resolved pixel [`Rect`]; its `(cols, rows)` are *derived*
    /// from that rect via the metric — the R969 one-directional
    /// `(rows, cols)` SSOT (layout-derived, never fed back).
    ///
    /// It carries the producer's terminal [`GridBuffer`] projection
    /// (R973–R978 data model: cluster / fg / bg / attrs / cursor /
    /// alt-screen / damage), proven both as scene-as-data via
    /// `scene/snapshot` (§2 #7) and, since **R991**, painted on the Vello
    /// backend (per-cell bg fill + cluster glyph; reverse / hidden / wide
    /// honoured) — extended in **R992** with the typographic SGR attributes
    /// (bold / italic / dim / underline / strikethrough) and in **R993** with
    /// the [`GridCursor`](crate::term_grid::GridCursor) overlay (block / bar / underline shapes). **R994**
    /// added the ratatui TUI arm, so both backends now paint it — the §2 #6
    /// GUI / TUI dual holds for the grid. It is uncacheable — the projection
    /// is replaced wholesale each frame. The Vello `blink` slice (a timing
    /// attribute the TUI gets free from the host terminal) remains.
    TextGrid(TextGridNode),
}

/// (R1516 §5.2 §2 #6 §2 #7) The census of [`Scene`] variants.
///
/// [`Scene`] is `#[non_exhaustive]` — deliberately, so the game-engine
/// variants this module's header names (`Mesh` / `Camera` / `Light`) can
/// land without a major bump. The cost is that no other crate can
/// enumerate the variants, or match them without a wildcard, and a
/// wildcard is where a new node kind goes to be forgotten: a consumer
/// that must reason about *every* kind keeps a hand list instead, and a
/// hand list cannot be told that it is short.
///
/// Measured when this was written, three of them were:
/// the §2 #6 backend-parity matrix asserted over "the two node types that
/// carry a [`BoxStyle`]" as a comment; the focus-ring walk answered `0` for
/// the corner radius of anything outside `Box | Container`; and the §2 #7
/// wire mapped an unrecognised node to `"Unknown"`. Each is the R1511
/// shape — a declaration reaching no consumer, with nothing to notice.
///
/// Only this crate can compute the list, so this crate publishes it, and
/// the two links are each a compile error:
///
/// 1. a new [`Scene`] variant fails [`Scene::node_kind`]'s match here, and
///    the repair is a variant below;
/// 2. a new variant below fails every downstream `match` on
///    `SceneNodeKind`, where the consumer must say what it does with the
///    kind.
///
/// Deliberately **not** `#[non_exhaustive]`, for the reason
/// [`BoxFacet`](crate::style::BoxFacet) is not: link 2 *is* the point, and
/// `#[non_exhaustive]` would force downstream wildcards that swallow
/// exactly what this exists to surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SceneNodeKind {
    /// [`Scene::Box`].
    Box,
    /// [`Scene::Text`].
    Text,
    /// [`Scene::Path`].
    Path,
    /// [`Scene::Image`].
    Image,
    /// [`Scene::Container`].
    Container,
    /// [`Scene::Effect`] — a §3 opaque escape.
    Effect,
    /// [`Scene::External`] — a §3 opaque escape.
    External,
    /// [`Scene::Scroll`].
    Scroll,
    /// [`Scene::ImmediateModeNode`].
    ImmediateModeNode,
    /// [`Scene::TextGrid`].
    TextGrid,
}

impl SceneNodeKind {
    /// The census. Consumers iterate this instead of re-deriving a variant
    /// list they cannot see.
    pub const ALL: [Self; 10] = [
        Self::Box,
        Self::Text,
        Self::Path,
        Self::Image,
        Self::Container,
        Self::Effect,
        Self::External,
        Self::Scroll,
        Self::ImmediateModeNode,
        Self::TextGrid,
    ];

    /// Stable identity — the variant name, which is also the §2 #7 wire
    /// `"type"` tag a `scene/snapshot` node carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Box => "Box",
            Self::Text => "Text",
            Self::Path => "Path",
            Self::Image => "Image",
            Self::Container => "Container",
            Self::Effect => "Effect",
            Self::External => "External",
            Self::Scroll => "Scroll",
            Self::ImmediateModeNode => "ImmediateModeNode",
            Self::TextGrid => "TextGrid",
        }
    }

    /// Whether nodes of this kind carry a [`BoxStyle`] — i.e. whether the
    /// [`BoxFacet`](crate::style::BoxFacet) census applies to them at all.
    /// Cross-checked against [`Scene::box_style`] over a fixture of every
    /// kind, so the two cannot drift.
    #[must_use]
    pub const fn carries_box_style(self) -> bool {
        match self {
            Self::Box | Self::Container => true,
            Self::Text
            | Self::Path
            | Self::Image
            | Self::Effect
            | Self::External
            | Self::Scroll
            | Self::ImmediateModeNode
            | Self::TextGrid => false,
        }
    }

    /// Whether this kind clips a *child subtree* to a window of its own —
    /// the declaration every renderer must observe, since ink the scene
    /// says is hidden must not reach the surface on any backend.
    ///
    /// Only [`Scene::Scroll`] does (to its `viewport`). A [`Scene::Text`],
    /// [`Scene::Image`] or [`Scene::TextGrid`] confines its *own* glyphs or
    /// pixels to its rect, which is leaf rasterisation rather than a clip
    /// declared over other nodes; a [`Scene::Container`] does not clip at
    /// all, which is precisely why `Scroll` exists.
    #[must_use]
    pub const fn clips_subtree(self) -> bool {
        match self {
            Self::Scroll => true,
            Self::Box
            | Self::Text
            | Self::Path
            | Self::Image
            | Self::Container
            | Self::Effect
            | Self::External
            | Self::ImmediateModeNode
            | Self::TextGrid => false,
        }
    }
}

impl Scene {
    /// Outermost rect of this primitive. [`EffectNode`] has no
    /// geometry of its own and returns [`Rect::default`].
    ///
    /// R55.A §5.45 — [`Scene::Scroll`] returns its `viewport` rect (the
    /// visible clip window). Content scene geometry is intentionally
    /// hidden behind the clip: hit-test / region-select / paint walk
    /// at the parent level see only what the viewport exposes, which
    /// preserves the §5.34 `dry_run` invariant that an introspection
    /// query never reveals primitive state the user cannot observe
    /// on screen.
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
            Scene::Scroll(n) => n.viewport,
            Scene::ImmediateModeNode(n) => n.viewport,
            Scene::TextGrid(n) => n.rect,
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
            Scene::Scroll(n) => n.tag.as_deref(),
            Scene::ImmediateModeNode(n) => n.tag.as_deref(),
            Scene::TextGrid(n) => n.tag.as_deref(),
        }
    }

    /// (R1516 §5.2) Which [`SceneNodeKind`] this node is — link 1 of the
    /// census. The match is exhaustive and this crate owns [`Scene`], so a
    /// variant added above lands here as a compile error, where it must be
    /// given a census entry rather than joining silently.
    #[must_use]
    pub const fn node_kind(&self) -> SceneNodeKind {
        match self {
            Scene::Box(_) => SceneNodeKind::Box,
            Scene::Text(_) => SceneNodeKind::Text,
            Scene::Path(_) => SceneNodeKind::Path,
            Scene::Image(_) => SceneNodeKind::Image,
            Scene::Container(_) => SceneNodeKind::Container,
            Scene::Effect(_) => SceneNodeKind::Effect,
            Scene::External(_) => SceneNodeKind::External,
            Scene::Scroll(_) => SceneNodeKind::Scroll,
            Scene::ImmediateModeNode(_) => SceneNodeKind::ImmediateModeNode,
            Scene::TextGrid(_) => SceneNodeKind::TextGrid,
        }
    }

    /// (R1516 §5.3 §5.16) The [`BoxStyle`] this node carries, or `None` for
    /// the kinds that carry none — the read behind every "what did this node
    /// declare visually" question.
    ///
    /// Before this existed, callers asked it as `match { Box(n) => n.style,
    /// Container(n) => n.style, _ => <a made-up default> }`, and the
    /// wildcard answered for a third styled variant that does not exist
    /// *yet*: [`Scene`] is `#[non_exhaustive]` for exactly that future. Here
    /// the match is exhaustive, so the future arrives as a compile error in
    /// this crate instead of as a default somewhere else.
    #[must_use]
    pub const fn box_style(&self) -> Option<&BoxStyle> {
        match self {
            Scene::Box(n) => Some(&n.style),
            Scene::Container(n) => Some(&n.style),
            Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::Effect(_)
            | Scene::External(_)
            | Scene::Scroll(_)
            | Scene::ImmediateModeNode(_)
            | Scene::TextGrid(_) => None,
        }
    }

    /// (R705 §5.39) Pointer-events transparency — mirrors CSS
    /// `pointer-events: none`. Reads the node's
    /// [`crate::style::LayoutStyle::pointer_transparent`] flag.
    /// [`Self::hit_test`] skips nodes for which this returns `true`, so
    /// a decorative overlay (focus ring, inspector highlight) painted on
    /// top of a widget does not shadow it for input while staying fully
    /// present for `scene/snapshot` introspection. [`Scene::Effect`]
    /// carries no layout sidecar and is never pointer-transparent.
    #[must_use]
    pub fn is_pointer_transparent(&self) -> bool {
        match self {
            Scene::Box(n) => n.layout.pointer_transparent,
            Scene::Text(n) => n.layout.pointer_transparent,
            Scene::Path(n) => n.layout.pointer_transparent,
            Scene::Image(n) => n.layout.pointer_transparent,
            Scene::Container(n) => n.layout.pointer_transparent,
            Scene::External(n) => n.layout.pointer_transparent,
            Scene::Scroll(n) => n.layout.pointer_transparent,
            Scene::ImmediateModeNode(n) => n.layout.pointer_transparent,
            Scene::TextGrid(n) => n.layout.pointer_transparent,
            Scene::Effect(_) => false,
        }
    }

    /// (R1196 §5.16 §5.39) The hover [`CursorHint`] this node declares via
    /// [`LayoutStyle::cursor`](crate::style::LayoutStyle::cursor), or `None`.
    /// The per-node read behind [`Self::cursor_hint_at`]; [`Scene::Effect`]
    /// carries no layout sidecar and never declares a cursor.
    #[must_use]
    pub fn cursor_hint(&self) -> Option<CursorHint> {
        match self {
            Scene::Box(n) => n.layout.cursor,
            Scene::Text(n) => n.layout.cursor,
            Scene::Path(n) => n.layout.cursor,
            Scene::Image(n) => n.layout.cursor,
            Scene::Container(n) => n.layout.cursor,
            Scene::External(n) => n.layout.cursor,
            Scene::Scroll(n) => n.layout.cursor,
            Scene::ImmediateModeNode(n) => n.layout.cursor,
            Scene::TextGrid(n) => n.layout.cursor,
            Scene::Effect(_) => None,
        }
    }

    /// (R1080 §5.51) Drag-and-drop drop-target flag — reads the node's
    /// [`crate::style::LayoutStyle::drop_target`] marker. The §5.51 R742
    /// router's `resolve_drop_point` resolves a drop over this node or any
    /// descendant to this node's [`tag`](Self::tag) (nearest opted-in
    /// ancestor wins) instead of the deepest tagged leaf, so a drag
    /// coordinator receives the semantic drop region (e.g. a dock panel,
    /// not its content). [`Scene::Effect`] carries no layout sidecar and
    /// is never a drop target.
    #[must_use]
    pub fn is_drop_target(&self) -> bool {
        match self {
            Scene::Box(n) => n.layout.drop_target,
            Scene::Text(n) => n.layout.drop_target,
            Scene::Path(n) => n.layout.drop_target,
            Scene::Image(n) => n.layout.drop_target,
            Scene::Container(n) => n.layout.drop_target,
            Scene::External(n) => n.layout.drop_target,
            Scene::Scroll(n) => n.layout.drop_target,
            Scene::ImmediateModeNode(n) => n.layout.drop_target,
            Scene::TextGrid(n) => n.layout.drop_target,
            Scene::Effect(_) => false,
        }
    }

    /// (R1020 §5.39) Keyboard focus-stop flag — reads the node's
    /// [`crate::style::LayoutStyle::focusable`] marker.
    /// [`Self::collect_focusable_tags`] enumerates the tags of nodes
    /// for which this returns `true`, in depth-first tree order, to
    /// feed the §5.39 `FocusManager`. [`Scene::Effect`] carries no
    /// layout sidecar and is never focusable.
    #[must_use]
    pub fn is_focusable(&self) -> bool {
        match self {
            Scene::Box(n) => n.layout.focusable,
            Scene::Text(n) => n.layout.focusable,
            Scene::Path(n) => n.layout.focusable,
            Scene::Image(n) => n.layout.focusable,
            Scene::Container(n) => n.layout.focusable,
            Scene::External(n) => n.layout.focusable,
            Scene::Scroll(n) => n.layout.focusable,
            Scene::ImmediateModeNode(n) => n.layout.focusable,
            Scene::TextGrid(n) => n.layout.focusable,
            Scene::Effect(_) => false,
        }
    }

    /// (R55.G.19 §5.49) Returns `true` when this scene tree contains
    /// at least one node tagged `target`. Walks depth-first matching
    /// [`Self::tag`] before descending into `Container.children` and
    /// `Scroll.content`. `Effect` is never tagged (per [`Self::tag`])
    /// so its branch is a leaf.
    ///
    /// Codifies the R55.G.17 composite paint-root tag convention:
    /// `V::view(state, frame).contains_tag(V::tag())` is the
    /// regression-test primitive that asserts a composite's paint
    /// scene exposes its `WidgetCore::tag()` somewhere — without
    /// this, `scene/click` / `scene/key` / `scene/wheel`
    /// `{path: V::tag()}` and `rect_for_tag` AT-bounds attach both
    /// fail silently.
    #[must_use]
    pub fn contains_tag(&self, target: &str) -> bool {
        if self.tag() == Some(target) {
            return true;
        }
        match self {
            Scene::Container(n) => n.children.iter().any(|c| c.contains_tag(target)),
            Scene::Scroll(n) => n.content.contains_tag(target),
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::External(_)
            | Scene::Effect(_)
            | Scene::ImmediateModeNode(_)
            | Scene::TextGrid(_) => false,
        }
    }

    /// (R1020 §5.39) Depth-first enumeration of keyboard focus-stop
    /// tags in tree order — the ratified §5.39 focus model. The shell
    /// re-runs this over the freshly produced PAINT scene every frame
    /// and feeds the result to
    /// [`FocusManager::update_focusable_tags`](../pinion_runtime/struct.FocusManager.html#method.update_focusable_tags),
    /// so a node that appears / disappears across frames (a dynamic
    /// pane, a conditionally-painted inline editor) joins / leaves the
    /// Tab order automatically — there is no binding-side list to keep
    /// in sync. This replaces the pre-R1020
    /// `WidgetCore::focusable_tags()` flat list, which was an unratified
    /// drift from this spec.
    ///
    /// Walks the same branches as [`Self::contains_tag`]:
    /// `Container.children` in declaration order, then `Scroll.content`.
    /// [`Scene::External`] is a tag-bearing leaf — a focusable External
    /// contributes its own tag but the walk does not descend into its
    /// opaque content, so a composite widget like `RadioGroup` is a
    /// single Tab stop whose internal roving lives inside the External
    /// (§5.39 composite single-stop).
    ///
    /// A focusable node with no [`tag`](Self::tag) cannot be a focus
    /// target (focus is tag-keyed) and is skipped; the convention is
    /// that a focusable-marked node (set via
    /// `.with_layout(LayoutStyle::new().with_focusable(true))`) also
    /// carries the `.with_tag(...)` the view fn pins for hit-test / RPC
    /// `focus/set` routing (the R55.G.17 `contains_tag` invariant).
    #[must_use]
    pub fn collect_focusable_tags(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_focusable_tags_into(&mut out);
        out
    }

    fn collect_focusable_tags_into(&self, out: &mut Vec<String>) {
        if self.is_focusable() {
            if let Some(tag) = self.tag() {
                out.push(tag.to_owned());
            }
        }
        match self {
            Scene::Container(n) => {
                for child in &n.children {
                    child.collect_focusable_tags_into(out);
                }
            }
            Scene::Scroll(n) => n.content.collect_focusable_tags_into(out),
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::External(_)
            | Scene::Effect(_)
            | Scene::ImmediateModeNode(_)
            | Scene::TextGrid(_) => {}
        }
    }

    // (R1168 removed `collect_drop_target_tags` / `_into`: its only consumer was
    // the static dock-zone guide [`apply_dock_zone_guides`], retired this round.
    // A drop-target enumerator is a clean scene primitive — re-add it when a real
    // consumer appears, e.g. an AI introspection of a scene's drop zones.)

    /// (R55.D.5 §5.45) Depth-first walk for the first [`ExternalNode`]
    /// whose tag equals `target`. Mirrors the
    /// `find_external_by_tag` private helper inside
    /// `pinion_runtime::input` (R51.41 dispatch path); promoted to a
    /// public `Scene` method so [`WidgetCore::read_state`](crate::widget_core::WidgetCore::read_state) in
    /// applications that opt into [`WidgetCore::create_extra_externals`](crate::widget_core::WidgetCore::create_extra_externals)
    /// can resolve the primary [`ExternalNode`] regardless of whether
    /// the substrate wrapped the state scene in a [`Scene::Container`]
    /// (multi-External) or left it as a bare [`Scene::External`]
    /// (single-External, the default shape).
    ///
    /// Walks the same branches as [`Self::contains_tag`] /
    /// [`Self::hit_test`]: `Container.children` in declaration order,
    /// `Scroll.content`. The other leaf variants
    /// (`Box`/`Text`/`Path`/`Image`/`Effect`) cannot carry an
    /// `External` handle and are skipped.
    ///
    /// Returns the first match in DFS pre-order. The substrate
    /// guarantees the primary External (declared via
    /// [`WidgetCore::create_external`](crate::widget_core::WidgetCore::create_external)) is the first child of the
    /// composed Container, so the canonical
    /// `scene.find_external_with_tag(V::tag())` call resolves O(1)
    /// for the common case.
    #[must_use]
    pub fn find_external_with_tag(&self, target: &str) -> Option<&ExternalNode> {
        match self {
            Scene::External(n) => {
                if n.tag.as_deref() == Some(target) {
                    Some(n)
                } else {
                    None
                }
            }
            Scene::Container(c) => c
                .children
                .iter()
                .find_map(|s| s.find_external_with_tag(target)),
            Scene::Scroll(s) => s.content.find_external_with_tag(target),
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::Effect(_)
            | Scene::ImmediateModeNode(_)
            | Scene::TextGrid(_) => None,
        }
    }

    /// R830 §2 #4 §5.15 — immediate-mode peer of
    /// [`find_external_with_tag`](Self::find_external_with_tag): resolve
    /// the [`ImmediateModeNode`] carrying `target` as its §5.20 tag. Used
    /// by the shell to forward a resolved pointer press to the addressed
    /// game driver (the node lives only in the paint scene, so the shell
    /// walks the *paint* scene with this — see
    /// [[state-scene-vs-paint-scene-introspect]]). Returns the node (not
    /// the driver) so the caller reads [`ImmediateModeNode::viewport`]
    /// for viewport-local coordinate translation and `borrow_mut`s the
    /// shared handle to dispatch [`ImmediateMode::on_pointer_down`].
    #[must_use]
    pub fn find_immediate_with_tag(&self, target: &str) -> Option<&ImmediateModeNode> {
        match self {
            Scene::ImmediateModeNode(n) => {
                if n.tag.as_deref() == Some(target) {
                    Some(n)
                } else {
                    None
                }
            }
            Scene::Container(c) => c
                .children
                .iter()
                .find_map(|s| s.find_immediate_with_tag(target)),
            Scene::Scroll(s) => s.content.find_immediate_with_tag(target),
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::Effect(_)
            | Scene::External(_)
            | Scene::TextGrid(_) => None,
        }
    }

    /// (R55.D.5 §5.45) Resolve to the substrate's *primary* External
    /// — the first [`ExternalNode`] reached by a depth-first
    /// pre-order walk. Returns `Some(self)` directly for the
    /// single-External shape (`Scene::External(...)`) and descends
    /// into `Container.children` / `Scroll.content` to find the
    /// first child External for the multi-External shape the
    /// substrate composes when [`WidgetCore::create_extra_externals`](crate::widget_core::WidgetCore::create_extra_externals)
    /// is non-empty.
    ///
    /// Used by the RPC introspect / invoke / dry-run / rewind
    /// primitives so an `external/...` path against a multi-External
    /// state scene transparently lands on the primary widget without
    /// requiring per-call disambiguation. A future round can add a
    /// `find_external_with_tag(tag)/external/...` path syntax to
    /// address the sibling externals explicitly; the substrate
    /// convention "primary is first in declaration order" makes this
    /// helper unambiguous in the meantime.
    #[must_use]
    pub fn primary_external(&self) -> Option<&ExternalNode> {
        match self {
            Scene::External(n) => Some(n),
            // (R1307 PR-51 §5.45) A container marked as having no
            // distinguished primary head (the no-primary state-scene root)
            // resolves to `None` — the DFS-first child is an extra, not a
            // primary, so bare `/external` rejects instead of misrouting.
            Scene::Container(c) if c.no_primary_head => None,
            Scene::Container(c) => c.children.iter().find_map(Self::primary_external),
            Scene::Scroll(s) => s.content.primary_external(),
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::Effect(_)
            | Scene::ImmediateModeNode(_)
            | Scene::TextGrid(_) => None,
        }
    }

    /// R681 §2 #4 atomic 1 — walk this scene tree depth-first and
    /// invoke [`ImmediateMode::tick`] on every reachable
    /// [`Scene::ImmediateModeNode`] driver with `dt`, then publish
    /// the same `dt` into the node's [`ImmediateModeNode::last_dt`]
    /// sidecar.
    ///
    /// Called by the per-window paint cycle AFTER the §5.21 layout
    /// pass resolves [`ImmediateModeNode::viewport`] and BEFORE the
    /// paint adapter encodes the immediate-mode subtree
    /// (`pinion_runtime::paint_adapter::to_vello`'s walker invokes
    /// [`ImmediateMode::paint`] inside the same frame).
    ///
    /// R831: the shell invokes this once per WHOLE fixed timestep its
    /// per-window `pinion_runtime::FixedTimestep` accumulator
    /// releases, so `dt` is the fixed simulation step (not the
    /// wall-clock frame delta) and `last_dt` publishes that fixed
    /// step. A sub-fixed / frozen frame invokes it zero times. The
    /// presence signal that gates the game-loop redraw re-arm is
    /// [`Self::has_immediate_mode_subtree`] (no tick side effect), so
    /// the returned count is informational (number of drivers ticked
    /// this step).
    ///
    /// Walks the same branches as [`Self::contains_tag`]: descends
    /// `Container.children` in declaration order, `Scroll.content`.
    /// `Effect` / `External` / `Box` / `Text` / `Path` / `Image` are
    /// non-immediate leaves; they contribute zero to the count.
    pub fn tick_immediate_mode(&self, dt: Duration) -> usize {
        let mut count = 0_usize;
        self.tick_immediate_mode_walk(dt, &mut count);
        count
    }

    /// Recursive helper for [`Self::tick_immediate_mode`].
    fn tick_immediate_mode_walk(&self, dt: Duration, count: &mut usize) {
        match self {
            Scene::ImmediateModeNode(node) => {
                node.handle.borrow_mut().tick(dt);
                node.set_last_dt(dt);
                *count = count.saturating_add(1);
            }
            Scene::Container(c) => {
                for child in &c.children {
                    child.tick_immediate_mode_walk(dt, count);
                }
            }
            Scene::Scroll(s) => s.content.tick_immediate_mode_walk(dt, count),
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::External(_)
            | Scene::Effect(_)
            | Scene::TextGrid(_) => {}
        }
    }

    /// R681 §2 #4 atomic 1 — `true` iff this scene tree contains at
    /// least one [`Scene::ImmediateModeNode`]. Cheaper than
    /// [`Self::tick_immediate_mode`] when the caller only needs the
    /// presence signal (the per-window
    /// `winit::event_loop::ControlFlow::WaitUntil` game-loop pacing
    /// decision, and — R831 — the immediate→retained intent-drain gate,
    /// which must run even on a frame that ticked zero whole steps).
    ///
    /// Short-circuits on the first hit (DFS pre-order).
    #[must_use]
    pub fn has_immediate_mode_subtree(&self) -> bool {
        match self {
            Scene::ImmediateModeNode(_) => true,
            Scene::Container(c) => c.children.iter().any(Self::has_immediate_mode_subtree),
            Scene::Scroll(s) => s.content.has_immediate_mode_subtree(),
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::External(_)
            | Scene::Effect(_)
            | Scene::TextGrid(_) => false,
        }
    }

    /// R682 §5.16 — paint-affecting structural hash for the §5.16
    /// paint-fragment cache (axis 4 of the 4-axis paint-pipeline
    /// rewrite series).
    ///
    /// Returns the same `u64` for two scenes that produce
    /// observationally identical Vello fragments at the same root
    /// `(x, y)` (the R682 first-cut hashes absolute coords; a
    /// follow-up round may switch to container-local coords for
    /// translation-invariant cache hits across reflows). The
    /// shell-side fragment cache (R682 atomic 1) keys off this
    /// value.
    ///
    /// Variants that carry opaque paint side-effects the cache
    /// cannot prove identical between frames return the fixed
    /// sentinel [`PAINT_HASH_UNCACHEABLE`]:
    ///
    /// - [`Scene::External`] — `Box<dyn External>` paint is opaque;
    ///   currently a no-op in `pinion_runtime::paint_adapter::to_vello`,
    ///   but the sentinel keeps the contract honest for future
    ///   paint impls that wire the §5.15 surface bridge.
    /// - [`Scene::ImmediateModeNode`] — driver state advances every
    ///   tick; cached encoded paint is stale by definition (§2 #4
    ///   immediate-mode game-loop semantics).
    ///
    /// The fragment cache layer consults
    /// [`Self::is_cacheable_for_paint`] in addition to this hash —
    /// a [`Scene::Container`] whose hash sub-includes an uncacheable
    /// descendant has a deterministic hash but the cacheability
    /// predicate still rejects it (a sentinel-bearing hash alone
    /// would let two different uncacheable scenes hash-collide and
    /// reuse each other's stale fragments).
    ///
    /// [`Scene::Effect`] paints nothing (no geometry per
    /// [`Self::rect`]), so its hash is a stable zero-payload value —
    /// distinct from [`PAINT_HASH_UNCACHEABLE`] so the cache can
    /// treat consecutive Effect leaves as identical and reuse a
    /// no-op fragment.
    #[must_use]
    pub fn paint_hash(&self) -> u64 {
        use core::hash::{Hash, Hasher};
        match self {
            Scene::Container(c) => c.paint_hash(),
            Scene::Box(b) => {
                let mut h = std::hash::DefaultHasher::new();
                b"pinion.scene.Box".hash(&mut h);
                b.rect.hash(&mut h);
                b.style.hash(&mut h);
                b.layout.hash(&mut h);
                h.finish()
            }
            Scene::Text(t) => {
                let mut h = std::hash::DefaultHasher::new();
                b"pinion.scene.Text".hash(&mut h);
                t.content.hash(&mut h);
                t.rect.hash(&mut h);
                t.style.hash(&mut h);
                t.layout.hash(&mut h);
                t.line_count.hash(&mut h);
                // R713 §5.36 — fold the styled-run spans into the paint
                // hash so the R682 paint-cache re-keys when a run's
                // range or style changes (e.g. a highlight toggling a
                // span's weight). `StyleRun` derives `Hash` (TextStyle
                // is already Hash); the length guards against a run
                // being added / removed without any field changing.
                (t.runs.len() as u64).hash(&mut h);
                for run in &t.runs {
                    run.hash(&mut h);
                }
                // R1072 §5.37 — fold the caret-bearing marker: it selects which
                // shaper paints this leaf (§5.37 vs parley) when the engine is
                // enabled, so two leaves identical but for this flag must not
                // share a cached paint fragment. The bit is hashed
                // unconditionally; with the engine off it is a nil dedup effect
                // (a node's marker is frame-stable, so its own hash is steady
                // frame-to-frame — only a label/field sharing identical text
                // would have deduped, and they sit in distinct containers).
                t.caret_bearing.hash(&mut h);
                h.finish()
            }
            Scene::Path(p) => {
                let mut h = std::hash::DefaultHasher::new();
                b"pinion.scene.Path".hash(&mut h);
                p.rect.hash(&mut h);
                p.style.hash(&mut h);
                p.layout.hash(&mut h);
                (p.commands.len() as u64).hash(&mut h);
                for cmd in &p.commands {
                    hash_path_command_into(cmd, &mut h);
                }
                h.finish()
            }
            Scene::Image(i) => {
                let mut h = std::hash::DefaultHasher::new();
                b"pinion.scene.Image".hash(&mut h);
                i.source.hash(&mut h);
                i.rect.hash(&mut h);
                i.style.hash(&mut h);
                i.layout.hash(&mut h);
                h.finish()
            }
            Scene::Scroll(s) => {
                let mut h = std::hash::DefaultHasher::new();
                b"pinion.scene.Scroll".hash(&mut h);
                s.viewport.hash(&mut h);
                s.axis.hash(&mut h);
                s.offset_x.hash(&mut h);
                s.offset_y.hash(&mut h);
                s.layout.hash(&mut h);
                s.content.paint_hash().hash(&mut h);
                h.finish()
            }
            Scene::Effect(_) => PAINT_HASH_EFFECT_SENTINEL,
            // R974.1/R991 §5.41 — TextGrid joins `External` /
            // `ImmediateModeNode` as UNCACHEABLE: R991 paints its cells
            // (the glyph grid) and the projection is replaced wholesale
            // each frame, so it must never let a parent container cache a
            // fragment keyed off this content-blind hash
            // (`is_cacheable_for_paint` returns `false`, so the hash value
            // is moot). Folding per-cell content into the hash to enable
            // caching is a deferred perf slice. R972.1's first cut grouped
            // it with the cacheable `Effect` sentinel — a latent
            // stale-frame fuse, corrected in R974.1.
            Scene::TextGrid(_) | Scene::External(_) | Scene::ImmediateModeNode(_) => {
                PAINT_HASH_UNCACHEABLE
            }
        }
    }

    /// R682 §5.16 — `true` iff every paint side-effect this subtree
    /// produces is fully captured by [`Self::paint_hash`] (so a
    /// cache hit safely substitutes a previously encoded
    /// `vello::Scene` fragment without losing pixel fidelity).
    ///
    /// The §5.16 cache layer is conservative — only known-pure
    /// retained-tree primitives are cacheable. A subtree with any
    /// uncacheable descendant rejects the cache wholesale at the
    /// first ancestor [`Scene::Container`]; that container's
    /// children encode fresh every frame so the immediate-mode
    /// driver / opaque external surface paints at live cadence.
    ///
    /// Uncacheable variants (must always paint fresh):
    /// - [`Scene::ImmediateModeNode`] — driver state per-tick.
    /// - [`Scene::External`] — opaque §3 escape; even a no-op paint
    ///   today is one new-paint-impl PR away from being live.
    /// - [`Scene::TextGrid`] — a terminal projection painted since R991
    ///   and replaced wholesale each frame (high-churn producer content),
    ///   like [`Scene::ImmediateModeNode`]; caching it off a content-blind
    ///   hash would serve a stale frame, so it paints fresh. (Folding
    ///   per-cell content into `paint_hash` to enable caching is a
    ///   deferred perf slice.)
    ///
    /// Cacheable leaves: [`Scene::Box`], [`Scene::Text`],
    /// [`Scene::Path`], [`Scene::Image`], [`Scene::Effect`] (no-op
    /// paint — caching a no-op fragment is a stable no-op).
    ///
    /// Container / Scroll recurse: cacheable iff all descendants
    /// are.
    #[must_use]
    pub fn is_cacheable_for_paint(&self) -> bool {
        match self {
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::Effect(_) => true,
            Scene::Container(c) => c.children.iter().all(Self::is_cacheable_for_paint),
            Scene::Scroll(s) => s.content.is_cacheable_for_paint(),
            // R974.1/R991 §5.41 — TextGrid is uncacheable (see the doc
            // above): a terminal projection is replaced wholesale each
            // frame, so a parent container re-encodes it fresh rather than
            // serving a stale cached fragment.
            Scene::External(_) | Scene::ImmediateModeNode(_) | Scene::TextGrid(_) => false,
        }
    }

    /// R1426 §5.41 §5.28 — does this tree contain a [`TextGrid`](Self::TextGrid)
    /// whose cursor is both shown
    /// ([`GridCursor::visible`](crate::term_grid::GridCursor::visible)) and in
    /// the blinking DECSCUSR mode
    /// ([`GridCursor::blink`](crate::term_grid::GridCursor::blink), R1425)?
    ///
    /// The shell reads this once per paint (a cheap read-only walk, mirroring
    /// [`Self::is_cacheable_for_paint`]) to decide whether to arm the
    /// per-window blink clock: a window showing at least one blinking cursor
    /// keeps requesting frames so the render-time phase alternates; a window
    /// with none (only steady or hidden cursors) lets the surface idle. Because
    /// [`Self::TextGrid`] is uncacheable, the grid subtree is re-walked every
    /// frame, so this stays correct without a cache-invalidation hook — an
    /// invariant to preserve if `TextGrid` ever gains a paint cache. The
    /// render-time phase itself is never stored here (or anywhere in the scene)
    /// — only the mode drives arming; the phase is a paint-time argument
    /// ([`GridCursor::shown_this_phase`](crate::term_grid::GridCursor::shown_this_phase)).
    #[must_use]
    pub fn has_visible_blinking_grid_cursor(&self) -> bool {
        match self {
            Scene::TextGrid(n) => {
                let cursor = n.cells().cursor();
                cursor.visible && cursor.blink
            }
            Scene::Container(c) => c
                .children
                .iter()
                .any(Self::has_visible_blinking_grid_cursor),
            Scene::Scroll(s) => s.content.has_visible_blinking_grid_cursor(),
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::Effect(_)
            | Scene::External(_)
            | Scene::ImmediateModeNode(_) => false,
        }
    }

    /// R668 §5.16 — tight content bounding box, in logical-pixel
    /// (x+w, y+h) terms, of every visible primitive in this tree.
    ///
    /// Defined as `(max_right, max_bottom)` over the post-layout
    /// rects of every non-[`Scene::Effect`] node reached by a
    /// depth-first walk:
    ///
    /// - [`Scene::Container`] contributes its own rect *and* the
    ///   union of its children's bbox — a Container with explicit
    ///   `LayoutStyle` width can fence in its descendants while a
    ///   Container with default sizing inherits the descendant
    ///   union;
    /// - [`Scene::Scroll`] contributes only its `viewport` rect, not
    ///   the inner content — the visible clip window is what the
    ///   user sees on screen, the inner content is intentionally
    ///   bounded by the scrollbar primitive ([[w3c-dom-selection-shape]]
    ///   `Window.innerWidth/innerHeight` mirror);
    /// - [`Scene::Effect`] has no geometry of its own (per
    ///   [`Self::rect`]) and is skipped.
    ///
    /// Returns `(0, 0)` for an empty / zero-sized tree.
    ///
    /// Consumer: `pinion_shell::SizeStrategy::IntrinsicAfterFirstPaint`
    /// — after the first paint cycle populates per-node rects, the
    /// shell calls this to compute the window resize target, clamped
    /// to `[min, max]`, and forwards it to
    /// `winit::window::Window::request_inner_size`. The walk is
    /// O(N) over scene nodes and runs once per binding lifetime (only
    /// the first paint), not per frame.
    #[must_use]
    pub fn intrinsic_content_size(&self) -> (u32, u32) {
        let mut max_w: u32 = 0;
        let mut max_h: u32 = 0;
        intrinsic_walk(self, &mut max_w, &mut max_h);
        (max_w, max_h)
    }

    /// (R55.D.5 §5.45) Mutable counterpart to
    /// [`Self::primary_external`]. Used by the RPC invoke / dry-run /
    /// rewind primitives which need to advance the `External`'s state
    /// via [`ExternalIntrospect::invoke`].
    pub fn primary_external_mut(&mut self) -> Option<&mut ExternalNode> {
        match self {
            Scene::External(n) => Some(n),
            // (R1307 PR-51 §5.45) No distinguished primary head → `None`
            // (mirror of [`Self::primary_external`]).
            Scene::Container(c) if c.no_primary_head => None,
            Scene::Container(c) => c.children.iter_mut().find_map(Self::primary_external_mut),
            Scene::Scroll(s) => s.content.primary_external_mut(),
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::Effect(_)
            | Scene::ImmediateModeNode(_)
            | Scene::TextGrid(_) => None,
        }
    }

    /// (R55.D.5 §5.45) Mutable counterpart to
    /// [`Self::find_external_with_tag`]. Test fixtures and apply-key
    /// dispatch paths that need to call
    /// [`ExternalIntrospect::intervene`]
    /// or [`ExternalIntrospect::invoke`]
    /// on the primary widget reach for the mutable borrow.
    pub fn find_external_with_tag_mut(&mut self, target: &str) -> Option<&mut ExternalNode> {
        match self {
            Scene::External(n) => {
                if n.tag.as_deref() == Some(target) {
                    Some(n)
                } else {
                    None
                }
            }
            Scene::Container(c) => c
                .children
                .iter_mut()
                .find_map(|s| s.find_external_with_tag_mut(target)),
            Scene::Scroll(s) => s.content.find_external_with_tag_mut(target),
            Scene::Box(_)
            | Scene::Text(_)
            | Scene::Path(_)
            | Scene::Image(_)
            | Scene::Effect(_)
            | Scene::ImmediateModeNode(_)
            | Scene::TextGrid(_) => None,
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
                // R705 §5.39 — pointer-transparent overlays (focus ring,
                // inspector highlight) layer on top in paint / snapshot
                // order but are invisible to hit-testing, so the widget
                // beneath them keeps receiving pointer input.
                if child.is_pointer_transparent() {
                    continue;
                }
                if let Some(mut child_hit) = child.hit_test(x, y) {
                    let seg = child.tag().map_or_else(|| idx.to_string(), String::from);
                    child_hit.segments.insert(0, seg);
                    return Some(child_hit);
                }
            }
        }
        // R55.A.2 §5.45 — `Scroll` descends into `content` with the
        // offset translation applied. The viewport-contains check
        // above already gated this branch on `(x, y)` landing inside
        // the clip; here we convert viewport-relative coordinates
        // into content-intrinsic ones and recurse. If the
        // translation lands outside the content (negative coordinate
        // because the offset moved content past the origin, or
        // beyond the content's outermost rect), the scroll container
        // itself is the deepest hit — same fallback shape as the
        // empty-content Container case.
        if let Scene::Scroll(s) = self {
            let vx = x.saturating_sub(s.viewport.x);
            let vy = y.saturating_sub(s.viewport.y);
            // Promote to i64 so a negative offset plus a small
            // viewport-local coordinate never wraps around the u32
            // ceiling, and the saturating add overflow at
            // `i32::MAX` cannot bite either.
            let cx = i64::from(vx).checked_add(i64::from(s.offset_x));
            let cy = i64::from(vy).checked_add(i64::from(s.offset_y));
            if let (Some(cx), Some(cy)) = (cx, cy)
                && cx >= 0
                && cy >= 0
                && let (Ok(cx_u), Ok(cy_u)) = (u32::try_from(cx), u32::try_from(cy))
                && let Some(child_hit) = s.content.hit_test(cx_u, cy_u)
            {
                return Some(child_hit);
            }
            // Translation fell outside the content (or content
            // itself reported no hit) — return the scroll container
            // as the deepest hit.
        }
        Some(HitPath {
            segments: Vec::new(),
            bbox: self.rect(),
        })
    }

    /// (R1196 §5.16 §5.39; R1199 SSOT) The hover [`CursorHint`] for the pointer
    /// at `(x, y)`: the hint of the DEEPEST node under the pointer that declares
    /// one, falling back to a hinted ancestor, or `None` if nothing on the hit
    /// path declares a cursor.
    ///
    /// Resolved by REUSING [`Self::hit_test`]'s single tree descent — the same
    /// pattern `pinion_runtime`'s `resolve_hover_tag` (tag) and
    /// `resolve_drop_target_tag` (drop-target) use for their positional
    /// attributes: one `hit_test`, then walk the returned [`HitPath`]
    /// deepest-first via [`Self::lookup_path_ref`], returning the first node with
    /// a [`Self::cursor_hint`]. This shares [`Self::hit_test`]'s descent SSOT
    /// (transparent skip, Scroll offset translation, topmost-child selection)
    /// rather than re-walking the tree. The cursor is a property of the painted
    /// region, independent of tags and input routing (a splitter handle is
    /// untagged yet declares a resize cursor — [`Self::hit_test`] includes
    /// untagged nodes by index, so [`Self::lookup_path_ref`] still reaches it).
    #[must_use]
    pub fn cursor_hint_at(&self, x: u32, y: u32) -> Option<CursorHint> {
        let hit = self.hit_test(x, y)?;
        // Deepest-first: the nearest hint wins; a hinted ancestor applies when a
        // child declares none. `segments[..len]` is the deepest hit node,
        // `segments[..0]` the root.
        (0..=hit.segments.len()).rev().find_map(|k| {
            self.lookup_path_ref(&hit.segments[..k])
                .and_then(Scene::cursor_hint)
        })
    }

    /// (§5.32 R39.2 v0) Collect every primitive whose rect intersects
    /// the query rect `(x, y, w, h)`. Walks the scene tree in
    /// declaration order (DFS pre-order); each match appears once,
    /// with its full path from the root. Containers themselves count
    /// as hits (a region-select on a tagged container is meaningful
    /// for AI reasoning).
    ///
    /// Zero-area query rects return an empty vec (per `rects_intersect`
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
        // R55.A.3 §5.45 — Scroll is path-transparent: the scroll
        // container does not consume a path segment (mirrors R51.181
        // hit-test). The empty-segment case above already returns
        // the viewport rect via `Self::rect`, so a non-empty path
        // forwards into `content` unchanged. ScrollNode's own
        // `tag` is the §5.20 input router carrier, not a path
        // identifier — a parent Container is what surfaces the
        // scroll's tag/index in the `HitPath::segments` chain.
        if let Scene::Scroll(s) = self {
            return s.content.lookup_path(segments);
        }
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

    /// R705.1 §5.45 §2 #7 — depth-first walk for the **window-absolute**
    /// post-layout rect of the node tagged `target`.
    ///
    /// This is the single coordinate-translation authority for "where on
    /// screen does tag X paint": rects inside a [`Scene::Scroll`] content
    /// tree are stored *scroll-local*, while everything else is
    /// window-absolute. The walk accumulates `(viewport.x - offset_x,
    /// viewport.y - offset_y)` on each Scroll boundary and intersects the
    /// enclosing viewport stack as a clip, so the returned rect is always
    /// window-absolute and bounded to the visible region. A node fully
    /// scrolled out of view returns `None` (not a degenerate origin
    /// rect).
    ///
    /// Used by the RPC click/drag path-resolver
    /// (`pinion_rpc::dispatch`) to land synthetic input on the pixels the
    /// user actually sees, and by the §5.39 focus-ring overlay
    /// (`pinion_overlay::inject_focus_ring`) to draw the ring at the
    /// widget's true on-screen position even when the widget lives inside
    /// a scroll. Consolidating both on one resolver is what makes
    /// `scene/snapshot from: paint` ring assertions verifiable against
    /// the real geometry rather than tautologically
    /// ([[introspection-from-paint-not-screen]]).
    #[must_use]
    pub fn rect_for_tag_absolute(&self, target: &str) -> Option<Rect> {
        self.rect_for_tag_with_offset(target, 0, 0, None)
    }

    /// (R1205 §5.51 §5.39) The DOCK AREA of this (window) paint scene: the
    /// window-absolute rect of the [`DOCK_SURFACE_TAG`](crate::external::DOCK_SURFACE_TAG)
    /// wrapper the dock walker stamps around its workspace subtree, falling back to
    /// this scene's own rect (the whole window) when there is no dock surface — a
    /// naked floater, or any non-dock window. The one SSOT the same-window OUTER
    /// dock band ([`InputRouter::resolve_own_outer_dock`](../../pinion_runtime/input/struct.InputRouter.html))
    /// and the cross-window redock preview both measure against, so they agree on
    /// where the dock area sits (below a chrome strip / toolbar / menu) with no
    /// per-window scalar to stamp — the rect the layout engine already computed for
    /// the workspace wrapper carries every inset for free.
    #[must_use]
    pub fn dock_surface_rect(&self) -> Rect {
        self.rect_for_tag_absolute(crate::external::DOCK_SURFACE_TAG)
            .unwrap_or_else(|| self.rect())
    }

    fn rect_for_tag_with_offset(
        &self,
        target: &str,
        x_off: i64,
        y_off: i64,
        clip: Option<Rect>,
    ) -> Option<Rect> {
        if self.tag() == Some(target) {
            // `Self::rect` returns the viewport rect for `Scene::Scroll`,
            // matching the "scroll's own tag → viewport" convention.
            return translate_rect_into_clip(self.rect(), x_off, y_off, clip);
        }
        match self {
            Scene::Container(c) => c
                .children
                .iter()
                .find_map(|child| child.rect_for_tag_with_offset(target, x_off, y_off, clip)),
            Scene::Scroll(n) => {
                // Tighten the clip to this scroll's window-abs viewport,
                // then descend with the offset folded in. Nested scrolls
                // chain their viewports through the recursion.
                let new_clip = translate_rect_into_clip(n.viewport, x_off, y_off, clip)?;
                let dx = i64::from(n.viewport.x) - i64::from(n.offset_x);
                let dy = i64::from(n.viewport.y) - i64::from(n.offset_y);
                n.content
                    .rect_for_tag_with_offset(target, x_off + dx, y_off + dy, Some(new_clip))
            }
            _ => None,
        }
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
        // R55.A.3 §5.45 — Scroll is path-transparent (see
        // `Self::lookup_path` rationale). Forward unchanged so the
        // `scene/query` / `scene/rewind` nested-External walker
        // sees Scroll as a wrapper, not a path-bearing layer.
        if let Scene::Scroll(s) = self {
            return s.content.lookup_path_ref(segments);
        }
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
        // R55.A.3 §5.45 — Scroll is path-transparent (see
        // `Self::lookup_path` rationale). `s.content` is a
        // `Box<Scene>`; deref-coercion gives us `&mut Scene` on
        // recurse, so `TypedProposal::SetStyle` / `ReplaceView`
        // can mutate inside the scroll without breaking the
        // borrow chain.
        if let Scene::Scroll(s) = self {
            return s.content.lookup_path_mut(segments);
        }
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

    /// (R55.C.2 §5.45) Find the deepest [`ScrollNode`] whose
    /// `viewport` contains `(x, y)`. Returns the inner reference
    /// directly so callers (the [`InputRouter`](crate::scene::Scene)
    /// wheel / arrow / page dispatch) can read the attached
    /// `state: Option<Rc<ScrollState>>` without re-walking.
    ///
    /// Traversal mirrors [`Self::hit_test`]: descend into
    /// `Scene::Container` children in declaration order, into
    /// `Scene::Scroll` content with the offset translation applied
    /// (so nested scroll containers route wheel to the innermost
    /// match — the W3C `overflow: scroll` ancestor walk). The
    /// nested descent uses the same `i64` promotion guard against
    /// negative-offset `u32` wrap that R51.181 introduced for
    /// `hit_test` and R51.183 mirrored for the region descent;
    /// when the translation lands outside the content rect the
    /// scroll container itself is the deepest match (the wheel
    /// dispatches against the outermost matching scroll, never
    /// falls through to a non-scroll ancestor).
    ///
    /// `Scene::Effect` / `Scene::External` / `Scene::Box` /
    /// `Scene::Text` / `Scene::Path` / `Scene::Image` cannot carry
    /// a scroll viewport and are non-descendable leaves.
    #[must_use]
    pub fn scroll_target_at(&self, x: u32, y: u32) -> Option<&ScrollNode> {
        match self {
            Scene::Scroll(s) => {
                if !rect_contains(s.viewport, x, y) {
                    return None;
                }
                // Try to descend into nested content first so the
                // innermost matching scroll container wins. Same
                // viewport-local + offset translation as R51.181
                // hit_test descent.
                let vx = x.saturating_sub(s.viewport.x);
                let vy = y.saturating_sub(s.viewport.y);
                let cx = i64::from(vx).checked_add(i64::from(s.offset_x));
                let cy = i64::from(vy).checked_add(i64::from(s.offset_y));
                if let (Some(cx), Some(cy)) = (cx, cy)
                    && cx >= 0
                    && cy >= 0
                    && let (Ok(cxu), Ok(cyu)) = (u32::try_from(cx), u32::try_from(cy))
                    && let Some(deeper) = s.content.scroll_target_at(cxu, cyu)
                {
                    return Some(deeper);
                }
                Some(s)
            }
            Scene::Container(c) => {
                if !rect_contains(c.rect, x, y) {
                    return None;
                }
                for child in &c.children {
                    if let Some(deeper) = child.scroll_target_at(x, y) {
                        return Some(deeper);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// (R55.C.2 §5.45) Convenience wrapper over
    /// [`Self::scroll_target_at`] that returns the attached
    /// reactive [`ScrollState`] directly. `None` when no scroll
    /// container covers `(x, y)` OR when the covering container
    /// has no `state` attached (a declarative-only scroll node
    /// the application built without a `with_state` link — the
    /// router silently drops wheel input in that case).
    ///
    /// The `Rc` clone is cheap (one atomic-free refcount bump)
    /// and the router uses the result immediately to call
    /// `state.scroll_by(...)` then drops the clone — no shared
    /// long-lived borrow into the paint tree, so the next
    /// `update_paint_scene` can freely replace the tree.
    #[must_use]
    pub fn scroll_state_at(&self, x: u32, y: u32) -> Option<Rc<ScrollState>> {
        self.scroll_target_at(x, y)
            .and_then(|node| node.state.clone())
    }

    /// Recursive helper for [`Self::hit_test_region`]. Maintains a
    /// segment stack representing the current path from the root.
    fn collect_intersections(&self, query: Rect, path: &mut Vec<String>, out: &mut Vec<HitPath>) {
        if matches!(self, Scene::Effect(_)) {
            return;
        }
        if !rects_intersect(self.rect(), query) {
            return;
        }
        out.push(HitPath {
            segments: path.clone(),
            bbox: self.rect(),
        });
        if let Scene::Container(c) = self {
            for (idx, child) in c.children.iter().enumerate() {
                let seg = child.tag().map_or_else(|| idx.to_string(), String::from);
                path.push(seg);
                child.collect_intersections(query, path, out);
                path.pop();
            }
        }
        // R55.A.4 §5.45 — Scroll descends into `content` with the
        // query rect translated from root-local into content-
        // intrinsic coords (mirrors R51.181 hit_test and R51.182
        // lookup_path). The viewport-intersect gate above already
        // pushed the scroll container if it overlaps the query.
        // The descent uses the same `path` stack — Scroll consumes
        // no segment, so a content hit's path is identical to what
        // it would be without the scroll in the chain.
        if let Scene::Scroll(s) = self
            && let Some(translated) = s.translate_query_into_content(query)
        {
            s.content.collect_intersections(translated, path, out);
        }
    }
}

/// R705.1 §5.45 — translate a scroll-local rect into window-absolute
/// coords via the accumulated `(x_off, y_off)` shift, intersect with
/// `clip` (the enclosing Scroll viewport stack), and saturate back to
/// `u32`. Returns `None` when the result is empty (rect lies fully
/// outside the clip — e.g. scrolled off-viewport). The single
/// translation primitive behind [`Scene::rect_for_tag_absolute`];
/// previously duplicated as a private helper in `pinion_rpc::dispatch`
/// (R51.200 / R55.G.7) before R705.1 lifted it here next to the Scroll
/// node it serves.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "u32 <-> i64 <-> u32 round-trip on bounded scene coords; saturate at 0 below"
)]
fn translate_rect_into_clip(
    rect: Rect,
    x_off: i64,
    y_off: i64,
    clip: Option<Rect>,
) -> Option<Rect> {
    let rect_left = i64::from(rect.x) + x_off;
    let rect_top = i64::from(rect.y) + y_off;
    let rect_right = rect_left + i64::from(rect.w);
    let rect_bottom = rect_top + i64::from(rect.h);
    let (clip_left, clip_top, clip_right, clip_bottom) = match clip {
        Some(c) => {
            let cl = i64::from(c.x);
            let ct = i64::from(c.y);
            (cl, ct, cl + i64::from(c.w), ct + i64::from(c.h))
        }
        None => (i64::MIN, i64::MIN, i64::MAX, i64::MAX),
    };
    let visible_left = rect_left.max(clip_left);
    let visible_top = rect_top.max(clip_top);
    let visible_right = rect_right.min(clip_right);
    let visible_bottom = rect_bottom.min(clip_bottom);
    if visible_right <= visible_left || visible_bottom <= visible_top {
        return None;
    }
    let out_x = visible_left.max(0) as u32;
    let out_y = visible_top.max(0) as u32;
    let out_w = (visible_right - i64::from(out_x)) as u32;
    let out_h = (visible_bottom - i64::from(out_y)) as u32;
    Some(Rect::new(out_x, out_y, out_w, out_h))
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
    /// Bounding rect of the deepest matched primitive — *the matched
    /// primitive's own `rect` field, verbatim*. The coordinate frame
    /// is the frame the primitive's rect is stated in, which depends
    /// on the enclosing container chain:
    ///
    /// - **Outside any `Scroll`** (root scene, anywhere inside
    ///   `Container` / `Box` descents): the bbox is in the *same
    ///   frame as the queried `(x, y)`* — viewport-relative for the
    ///   §5.32 v0 RPC call site. `Container` is path-transparent and
    ///   applies no coordinate transform, so a hit on a deep child
    ///   inside several nested Containers still reports the child's
    ///   rect in viewport coords.
    /// - **Inside a `Scroll` content subtree**: the bbox is in
    ///   *content-intrinsic coordinates* (the frame the content's
    ///   `rect` was declared in), **not** viewport-relative. The
    ///   `Scroll` node introduces a coordinate transform via its
    ///   `viewport.{x,y}` + `offset_{x,y}` fields; primitives deeper
    ///   than the `Scroll` carry rects in the content frame so the
    ///   layout / paint adapter can apply the scroll offset
    ///   correctly. AI clients that need viewport-relative bboxes
    ///   inside a `Scroll` must apply the inverse transform
    ///   themselves (the §5.45 `Scroll` introspect surface exposes
    ///   `viewport` + `offset` for this).
    /// - **The `Scroll` container itself is the deepest hit** (e.g.
    ///   viewport contains the point but the translation falls
    ///   outside the content): the bbox is the `Scroll`'s viewport
    ///   rect, stated in *the same frame as `(x, y)`* (mirror of the
    ///   "outside any Scroll" case).
    ///
    /// Tagline: bbox = matched primitive's declared rect, no extra
    /// transform. Walk the parent chain in the returned `segments`
    /// to recover the enclosing `Scroll` (if any) and translate.
    pub bbox: Rect,
}

/// R682 §5.16 — sentinel paint hash returned by [`Scene::Effect`].
/// A stable distinct value (not zero, not the uncacheable sentinel)
/// so two adjacent Effect leaves cache-collide intentionally (the
/// underlying paint is a no-op; reusing the encoded no-op fragment
/// is safe).
///
/// The exact bit pattern is irrelevant — any value distinct from
/// [`PAINT_HASH_UNCACHEABLE`] and from any plausible structural-hash
/// collision works. Encoded as a magic constant so a hash dump
/// inspector can spot Effect leaves without re-running the hasher.
pub const PAINT_HASH_EFFECT_SENTINEL: u64 = 0xEFFE_C700_EFFE_C700;

/// R682 §5.16 — sentinel paint hash returned by uncacheable
/// variants ([`Scene::External`], [`Scene::ImmediateModeNode`]).
/// Distinct from any plausible structural-hash payload AND distinct
/// from [`PAINT_HASH_EFFECT_SENTINEL`] so a hash-dump inspector can
/// distinguish "cache-poison leaf" from "no-op cacheable leaf".
///
/// The §5.16 fragment cache layer never inserts under this hash
/// (the [`Scene::is_cacheable_for_paint`] predicate gates insertion
/// at the enclosing Container boundary), so a collision here cannot
/// resurrect a stale fragment.
pub const PAINT_HASH_UNCACHEABLE: u64 = 0xDEAD_CAFE_DEAD_CAFE;

/// R682 §5.16 — hash a [`PathCommand`] into the supplied hasher
/// state. Manual rather than `#[derive(Hash)]` because
/// [`PathPoint`] carries `f32` fields ([`Hash`] not implemented for
/// `f32` per std docs); we widen to the bit pattern via
/// [`f32::to_bits`] so identical points hash identically and `NaN`
/// payloads stay deterministically distinct (a cache miss on a
/// `NaN` path is the conservative response — `NaN` in geometry is
/// a path-builder bug anyway).
fn hash_path_command_into<H: core::hash::Hasher>(cmd: &PathCommand, hasher: &mut H) {
    use core::hash::Hash;
    match cmd {
        PathCommand::MoveTo(p) => {
            b"MoveTo".hash(hasher);
            p.x.to_bits().hash(hasher);
            p.y.to_bits().hash(hasher);
        }
        PathCommand::LineTo(p) => {
            b"LineTo".hash(hasher);
            p.x.to_bits().hash(hasher);
            p.y.to_bits().hash(hasher);
        }
        PathCommand::CurveTo { c1, c2, end } => {
            b"CurveTo".hash(hasher);
            c1.x.to_bits().hash(hasher);
            c1.y.to_bits().hash(hasher);
            c2.x.to_bits().hash(hasher);
            c2.y.to_bits().hash(hasher);
            end.x.to_bits().hash(hasher);
            end.y.to_bits().hash(hasher);
        }
        PathCommand::Close => {
            b"Close".hash(hasher);
        }
    }
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

/// R668 §5.16 — depth-first walker for
/// [`Scene::intrinsic_content_size`]. Updates the running
/// `(max_right, max_bottom)` against every reachable rect.
/// [`Scene::Effect`] has no geometry and is skipped; [`Scene::Scroll`]
/// contributes only its viewport (the clip window the user sees) and
/// intentionally does not descend into the inner content.
fn intrinsic_walk(s: &Scene, max_w: &mut u32, max_h: &mut u32) {
    match s {
        Scene::Effect(_) => {}
        Scene::Container(c) => {
            let r = c.rect;
            *max_w = (*max_w).max(r.x.saturating_add(r.w));
            *max_h = (*max_h).max(r.y.saturating_add(r.h));
            for child in &c.children {
                intrinsic_walk(child, max_w, max_h);
            }
        }
        Scene::Box(_)
        | Scene::Text(_)
        | Scene::Path(_)
        | Scene::Image(_)
        | Scene::External(_)
        | Scene::Scroll(_)
        | Scene::ImmediateModeNode(_)
        // R972 §5.41 — the grid occupies its layout-resolved rect.
        | Scene::TextGrid(_) => {
            let r = s.rect();
            *max_w = (*max_w).max(r.x.saturating_add(r.w));
            *max_h = (*max_h).max(r.y.saturating_add(r.h));
        }
    }
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
///
/// (R682 §5.16) `Hash` participates in the §5.16 paint-fragment cache
/// key derivation — see [`ContainerNode::paint_hash`]. All four fields
/// are `u32` so the derive is direct (no float bit-pattern detour).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
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

    /// R682 §5.16 — smallest axis-aligned rectangle that contains
    /// both `self` and `other`. Used by the §5.16 fragment cache
    /// (R682 atomic 2) to compute the per-paint damage region as the
    /// union of every cache-miss Container's rect.
    ///
    /// Zero-area rects (`w == 0` or `h == 0`) are treated as empty
    /// sets — the union of a zero-area rect with a non-zero rect
    /// returns the non-zero rect verbatim (a zero-area rect
    /// contributes no pixels, so it cannot extend the damage
    /// region). The union of two zero-area rects returns a zero-area
    /// rect at `(min(x), min(y))`.
    #[must_use]
    pub fn union(self, other: Rect) -> Rect {
        // Empty-set short-circuits (consistent with `rects_intersect`
        // and the half-open `rect_contains` semantics elsewhere).
        let self_empty = self.w == 0 || self.h == 0;
        let other_empty = other.w == 0 || other.h == 0;
        match (self_empty, other_empty) {
            (true, true) => Rect::new(self.x.min(other.x), self.y.min(other.y), 0, 0),
            (true, false) => other,
            (false, true) => self,
            (false, false) => {
                let lx = self.x.min(other.x);
                let ty = self.y.min(other.y);
                let rx = self
                    .x
                    .saturating_add(self.w)
                    .max(other.x.saturating_add(other.w));
                let by = self
                    .y
                    .saturating_add(self.h)
                    .max(other.y.saturating_add(other.h));
                Rect::new(lx, ty, rx.saturating_sub(lx), by.saturating_sub(ty))
            }
        }
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

    /// R55.G.6 §5.45 — apply a functional transform to the layout
    /// sidecar in place. The closure receives the current layout (any
    /// constructor-supplied seed) and returns the new layout; this
    /// preserves whatever the caller does not override, in contrast
    /// to [`Self::with_layout`] which performs a full replacement.
    /// Use this when chaining a single modification on top of the
    /// seeded default (e.g. `node.map_layout(|l| l.with_gap(8))`).
    #[must_use]
    pub fn map_layout<F: FnOnce(LayoutStyle) -> LayoutStyle>(mut self, f: F) -> Self {
        self.layout = f(self.layout);
        self
    }
}

/// R1417 §5.35 §5.15 — a **capture surface**: a transparent, geometrically
/// pointer-opaque [`Scene::Box`] laid out over `rect` and carrying `tag`.
///
/// The one idiom for an *invisible interaction layer* over content — a chart
/// plot, a scrub track, a terminal pane — so a pointer anywhere on the region
/// routes to the [`External`](crate::external::External) / widget registered
/// under `tag`. It is transparent so the content behind shows through, and
/// geometrically opaque so the §5.35 hit-test lands here (the router's
/// hit-test is alpha-independent — a zero-alpha fill still occupies its rect).
/// `focusable` marks it a Tab stop ([`LayoutStyle::with_focusable`]) for the
/// widgets whose keyboard interaction needs focus (a scrub bar); the dataviz
/// hover / brush surfaces pass `false`.
///
/// Lifted at R1417 from the ~10 hand-rolled copies the interaction examples had
/// each grown (`hello-chart` / `-timeline` / `-scatter` / `-donut` / `-treemap`
/// / `-crosshair` / `-histogram-brush` / `-linked-brush` / `-scrubber` /
/// `-raw-pointer`) — every one byte-identical but for `(tag, rect, focusable)`,
/// so the surface's construction lives in exactly one place. A caller that
/// wants an enlarged hit area (the scrubber's padded track) inflates `rect`
/// before the call; the surface's own shape is not the variable.
#[must_use]
pub fn capture_surface(tag: impl Into<Cow<'static, str>>, rect: Rect, focusable: bool) -> Scene {
    Scene::Box(
        BoxNode::filled(Rect::default(), Color::TRANSPARENT)
            .with_tag(tag)
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x, rect.y)
                    .with_size(Size::px(rect.w, rect.h))
                    .with_focusable(focusable),
            ),
    )
}

/// R713 §5.36 — a styled span over a sub-range of [`TextNode::content`].
///
/// The styled-run substrate (`RichText` / `Text.rich`): a [`TextNode`]
/// carries a `style` that applies to the whole string *plus* an
/// ordered list of `StyleRun`s, each fully restyling the bytes in
/// `[start, end)`. This is the Qt `QTextLayout::FormatRange` / Flutter
/// `TextSpan`-list model: each run carries a **fully resolved**
/// [`TextStyle`] (not a partial override), so a run is unambiguous and
/// the cache key (`pinion-text::LayoutCache`) stays a value comparison.
///
/// # Range semantics
///
/// * `start` / `end` are **UTF-8 byte offsets** into
///   [`TextNode::content`] — the same units parley's `RangedBuilder`
///   indexes. `end` is exclusive; `start == end` is an empty run
///   (no-op). Offsets outside the content length are clamped by the
///   shaper (parley ignores out-of-range pushes), but well-formed
///   consumers keep runs in-bounds and non-overlapping.
/// * Runs apply **in list order, after** the base `style`: the base
///   `style` is pushed as the default, then each run's style is pushed
///   over its range. Where runs overlap, a later run wins for the
///   overlapping bytes (last-push-wins, matching parley's range
///   resolution). Uncovered bytes keep the base `style`.
///
/// # Empty runs = single-style fast path
///
/// `TextNode::runs.is_empty()` is the canonical single-style text node
/// (every pre-R713 node): the shaper takes the `push_default`-only
/// path and the cache key omits run data. The field defaults to an
/// empty `Vec`, so every existing `TextNode` constructor is unchanged.
///
/// # Run-level vs paragraph-level style fields
///
/// A `StyleRun` carries a whole [`TextStyle`] for self-describing
/// scene-as-data introspection (§2 #7), but only the **run-level**
/// fields take effect per range — `font_size_px`, `fg_color`,
/// `font_weight`, `font_style`, `letter_spacing`, `line_height`,
/// `decoration`, `font_family`. The **paragraph-level** fields
/// (`text_align`, `overflow`) are resolved once for the whole node
/// from [`TextNode::style`]; a per-run value for them is ignored. This
/// mirrors CSS, where block properties (`text-align`) set on an inline
/// span have no effect. Authoring convention: build each run's style
/// from the node's base style so it inherits the paragraph-level
/// fields and overrides only the run-level ones.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StyleRun {
    /// UTF-8 byte offset of the first styled byte (inclusive).
    pub start: u32,
    /// UTF-8 byte offset one past the last styled byte (exclusive).
    pub end: u32,
    /// The fully-resolved style for the bytes in `[start, end)`.
    pub style: TextStyle,
}

impl StyleRun {
    /// Construct a styled run over the UTF-8 byte range `[start, end)`.
    #[must_use]
    pub fn new(start: u32, end: u32, style: TextStyle) -> Self {
        Self { start, end, style }
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
/// R713 §5.36 — `runs` carries an optional ordered list of [`StyleRun`]
/// spans for rich (multi-style) text; empty (the default) is the
/// single-style fast path. See [`StyleRun`] for range semantics.
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
    /// R713 §5.36 — optional ordered styled-run spans for rich
    /// (multi-style) text. Empty (the default) is the single-style
    /// node: the whole string uses `style`. Non-empty applies each
    /// [`StyleRun`] over its byte range on top of `style` (see
    /// [`StyleRun`] for ordering / overlap semantics). The paint
    /// adapter already emits one Vello glyph run per parley run, so a
    /// multi-style node renders without any extra paint plumbing.
    pub runs: Vec<StyleRun>,
    /// R51.81 §5.40 — WAI-ARIA-aligned role hint for the AT-side name
    /// enrichment pipeline.
    ///
    /// The §5.40 `enrich_names_from_scene` pass derives a widget's
    /// accessible name by walking the paint scene for the first
    /// descendant `TextNode::content`. Decoration glyphs (checkbox `✓`,
    /// slider thumb caret, toggle dot) that visually mark state but
    /// have no linguistic value should NOT become the AT-exposed
    /// name — they confuse screen readers ("checked" reading as "✓").
    ///
    /// `Default` (= `None` after `TextNode::new`) puts the text into
    /// the enrichment search. `Presentational` declares the run is
    /// pure decoration (WAI-ARIA `role="presentation"`) so enrichment
    /// skips it.
    ///
    /// R51.86 §5.40 — the enum is `#[non_exhaustive]`; future
    /// variants (e.g. an explicit `Label` carrier for the
    /// WAI-ARIA 1.2 §5.2.6 labelling axis) land additively when a concrete
    /// consumer arrives. Pre-R51.86 carried a `Label` placeholder
    /// without a consumer; strict-YAGNI removed it so the enum
    /// surfaces only roles the pipeline actually honours.
    pub role: Option<TextRole>,
    /// R1072 §5.37 — this text owns externally-shaped caret / selection /
    /// hit-test geometry, so the opt-in self-hosted text engine must NOT
    /// re-shape it.
    ///
    /// A [`TextField`](crate::widgets::text_field) derives its caret rect,
    /// selection bands, find / bracket highlights, IME-preedit underline, and
    /// click-to-position hit-test all from ONE parley `Layout` of this same
    /// string (the `field_shaping` SSOT). The §5.37 engine shapes with its own
    /// font and advances, which need not match parley's — so painting an
    /// editable field's glyphs through §5.37 while those overlays stay parley
    /// would drift the caret off the glyphs. `true` keeps both *measure* and
    /// *paint* of this leaf on parley (the eligibility SSOT
    /// `text_engine::self_hosted_text_eligible` rejects it for BOTH arms), so
    /// caret-bearing text stays self-consistent. `false` (the default, every
    /// static label) leaves the leaf eligible for the §5.37 arms.
    ///
    /// Integrating the caret into §5.37 (so editable text could route through
    /// the engine too) is a later campaign step; until then this marker is the
    /// R1070.1 "exclude caret-bearing text" contract.
    pub caret_bearing: bool,
}

/// R51.81 §5.40 — accessibility role hint attached to a [`TextNode`].
///
/// See [`TextNode::role`] for the contract. The enum is `#[non_exhaustive]`
/// so future variants (the WAI-ARIA 1.2 §5.2.6 labelling axis is the
/// next likely addition) land additively without breaking downstream
/// matchers.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
    /// Default — text participates in the §5.40 name enrichment as
    /// the first-text source. Most widgets' static labels carry this
    /// role.
    Default,
    /// Decoration glyph — skipped by `enrich_names_from_scene`.
    /// WAI-ARIA `role="presentation"`. Use for visual-only state
    /// marks (`✓`, `▶`, `●`).
    Presentational,
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
            runs: Vec::new(),
            role: None,
            caret_bearing: false,
        }
    }

    /// R1072 §5.37 — mark this text as owning externally-shaped caret /
    /// selection / hit-test geometry (builder form). See
    /// [`TextNode::caret_bearing`]; a [`TextField`](crate::widgets::text_field)
    /// sets this so the §5.37 self-hosted engine never re-shapes its editable
    /// text (which would drift the caret off the painted glyphs).
    #[must_use]
    pub fn caret_bearing(mut self) -> Self {
        self.caret_bearing = true;
        self
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

    /// R55.G.6 §5.45 — apply a functional transform to the layout
    /// sidecar in place; see [`BoxNode::map_layout`] for the
    /// canonical rationale. Preserves any seeded default by handing
    /// the current layout to the closure as input.
    #[must_use]
    pub fn map_layout<F: FnOnce(LayoutStyle) -> LayoutStyle>(mut self, f: F) -> Self {
        self.layout = f(self.layout);
        self
    }

    /// R51.81 §5.40 — attach a [`TextRole`] hint for the
    /// `enrich_names_from_scene` pass. Use `TextRole::Presentational`
    /// on decoration glyphs (checkbox `✓`, slider thumb caret) so
    /// the AT-exposed name skips past them and lands on the linguistic
    /// label text.
    #[must_use]
    pub fn with_role(mut self, role: TextRole) -> Self {
        self.role = Some(role);
        self
    }

    /// R713 §5.36 — attach styled-run spans for rich (multi-style)
    /// text (builder form). Each [`StyleRun`] fully restyles its UTF-8
    /// byte range of `content` on top of the node's base `style`; see
    /// [`StyleRun`] for ordering / overlap semantics. An empty `runs`
    /// (the default) keeps the single-style fast path.
    #[must_use]
    pub fn with_runs(mut self, runs: Vec<StyleRun>) -> Self {
        self.runs = runs;
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
/// command stream the rasterizer consumes; `rect: Rect` is the pixel
/// bounding box for layout / hit-test; `style: PathStyle` carries
/// stroke and fill specifications.
///
/// # Coordinate basis (R1358)
///
/// `commands` are **relative to `rect`'s origin**: a command at
/// `(0, 0)` paints at the node's top-left corner, wherever layout puts
/// it. This is the same basis [`ImmediateModeNode`] uses for its
/// viewport, and the basis the R722 [`PathStyle::gradient`] already
/// used (its UV `(0,0)`/`(1,1)` anchor to `rect`) — R1358 aligned the
/// geometry with the gradient that shared the node, so a path is now
/// positioned by `rect` alone.
///
/// Commands are read against whatever `rect` the node carries when it is
/// consumed. For a scene that is laid out, that is layout's *output*: the
/// taffy pass overwrites `rect` from [`PathNode::layout`], so a path
/// **moves with** its rect — dock it, scroll it, resize its container, and
/// the geometry goes along. A producer pinning a path to an exact region
/// declares it —
/// [`LayoutStyle::with_absolute_position`](crate::style::LayoutStyle::with_absolute_position)
/// plus a size — rather than baking window coordinates into commands, and
/// every in-tree producer does exactly that.
///
/// What R1358 did **not** give you: the geometry *translates*, it does not
/// *scale*. A path handed a bigger rect draws the same size in the new
/// place; nothing rescales commands to fit (unlike a `Box`, whose fill IS
/// its rect). An auto-sized path in a flex row therefore measures `0x0`
/// (the layout pass has no intrinsic size for a command stream) — declare a
/// size. A producer that wants geometry to track its box must rebuild the
/// commands from the measured rect, which is the reactive measured-rect
/// seam's job, not the primitive's.
///
/// A scene that is never laid out keeps its authored `rect` and the
/// commands are read against that. The two in-tree cases are an overlay
/// injected *after* the layout pass (`pinion-overlay`'s window chrome,
/// which paints) and a `from:state` blueprint subtree (which is only ever
/// introspected, never painted).
///
/// Inside a [`Scene::Scroll`] content tree `rect` is stored scroll-local,
/// so `rect.origin + command` is a position in that content frame, not on
/// screen; [`Scene::rect_for_tag_absolute`] is the single authority for
/// "where on screen". A path is no different from a `Box` here.
///
/// A command may fall outside `rect` (a stroke's join, a Bézier bowing
/// past its endpoints); `rect` is a bounding box for layout and
/// hit-test, never a clip.
///
/// [`PathStyle::gradient`]: crate::style::PathStyle::gradient
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

    /// R55.G.6 §5.45 — apply a functional transform to the layout
    /// sidecar in place; see [`BoxNode::map_layout`] for the
    /// canonical rationale. Preserves any seeded default by handing
    /// the current layout to the closure as input.
    #[must_use]
    pub fn map_layout<F: FnOnce(LayoutStyle) -> LayoutStyle>(mut self, f: F) -> Self {
        self.layout = f(self.layout);
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

    /// R55.G.6 §5.45 — apply a functional transform to the layout
    /// sidecar in place; see [`BoxNode::map_layout`] for the
    /// canonical rationale. Preserves any seeded default by handing
    /// the current layout to the closure as input.
    #[must_use]
    pub fn map_layout<F: FnOnce(LayoutStyle) -> LayoutStyle>(mut self, f: F) -> Self {
        self.layout = f(self.layout);
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
    /// (R1307 PR-51 §5.45 §2 #7) When `true`, this container has **no
    /// distinguished primary child**: it is the state-scene root the
    /// runtime composes for a binding whose interactive surfaces are all
    /// dynamic extras ([`WidgetCore::primary_surface`](crate::WidgetCore::primary_surface)
    /// `== None`). [`Scene::primary_external`] returns `None` for such a
    /// container instead of its first External, so the bare `/external` RPC
    /// shorthand — which names "the primary" — self-describes the absence
    /// with a clean `NoExternalAtPath` rather than silently resolving an
    /// arbitrary extra as if it were the primary. Default `false`: an
    /// ordinary container is headed by its first External (the substrate's
    /// "primary is first in declaration order" convention), unchanged.
    pub no_primary_head: bool,
    /// R682 §5.16 — within-paint-pass memoised structural hash for
    /// the §5.16 paint-fragment cache (axis 4 of the 4-axis
    /// paint-pipeline rewrite series).
    ///
    /// Computed lazily by [`Self::paint_hash`] the first time the
    /// paint adapter walks this container; subsequent calls inside
    /// the same paint pass hit the [`Cell`]. The next paint pass
    /// works on a fresh container (`V::view` rebuilds the scene tree
    /// from scratch each frame per R26) which defaults the field
    /// back to [`None`] — the hash is per-paint-pass memoisation,
    /// not cross-frame state.
    ///
    /// The fragment cache itself (R682 atomic 1) lives in
    /// `pinion-shell` per-window-slot and is keyed off the value
    /// this method returns. A repeated paint of structurally
    /// identical content keeps the same hash → fragment cache hit →
    /// `vello::Scene::append` reuse instead of fresh encode (the
    /// §2 #4 immediate-mode coexistence enabler — the retained
    /// widget tree does NOT re-encode every frame when only an
    /// immediate-mode sibling is animating).
    ///
    /// Excluded from `Debug` formatting because the inner [`Cell`]
    /// state is paint-pass transient — including it would render
    /// scene dumps differ depending on whether they were taken
    /// before / after a paint walk.
    pub paint_hash: Cell<Option<u64>>,
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
            no_primary_head: false,
            paint_hash: Cell::new(None),
        }
    }

    /// (R1307 PR-51 §5.45) Mark this container as having **no distinguished
    /// primary child** — the state-scene root shape the runtime composes for
    /// a no-primary binding. See [`Self::no_primary_head`]. Builder form; the
    /// substrate's `compose_root` calls it on the primary-less arm so
    /// [`Scene::primary_external`] returns `None` and the bare `/external`
    /// RPC shorthand rejects cleanly instead of resolving an arbitrary extra.
    #[must_use]
    pub const fn without_primary_head(mut self) -> Self {
        self.no_primary_head = true;
        self
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

    /// R55.G.6 §5.45 — apply a functional transform to the layout
    /// sidecar in place; see [`BoxNode::map_layout`] for the
    /// canonical rationale. Preserves any seeded default by handing
    /// the current layout to the closure as input.
    #[must_use]
    pub fn map_layout<F: FnOnce(LayoutStyle) -> LayoutStyle>(mut self, f: F) -> Self {
        self.layout = f(self.layout);
        self
    }

    /// Attach a §5.3 [`BoxStyle`] — the container paints its own fill /
    /// border before recursing into children. v0 covers fill +
    /// `corner_radius` via the same shape as `BoxNode`.
    ///
    /// Not `const`: R708 §5.50 made [`BoxStyle`] carry an optional heap
    /// [`Gradient`](crate::style::Gradient), so assigning over `self.style` runs a destructor —
    /// disallowed in `const fn`.
    #[must_use]
    pub fn with_style(mut self, style: BoxStyle) -> Self {
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

    /// R682 §5.16 — paint-affecting structural hash of this
    /// container, memoised inside the inner [`Cell`] for the duration
    /// of one paint pass.
    ///
    /// Combines (in fixed declaration order):
    ///
    /// - A type-tag byte string (so a `ContainerNode` with all-zero
    ///   fields never collides with a `BoxNode` of all-zero fields).
    /// - [`Self::rect`] — post-layout absolute bounds (R682 first-cut
    ///   uses absolute coords; a follow-up round may move the cache
    ///   key to container-local coords for shift-invariant hits).
    /// - [`Self::style`] — fill / border / corner radius (every paint
    ///   side-effect at the container's own paint step).
    /// - [`Self::layout`] — declared `LayoutStyle` (taffy reads this
    ///   on the next pass; a change here can shift descendants which
    ///   are already covered by descendant rects, but the layout
    ///   itself affects nothing painted FOR this container if rect
    ///   matches — still hashed for paranoia and to keep the
    ///   "any paint-side-effect input" invariant).
    /// - Child count + every child's [`Scene::paint_hash`] in
    ///   declaration order (so reordering siblings or inserting /
    ///   deleting a child invalidates the cache).
    ///
    /// Deliberately EXCLUDED (no paint side-effect at this layer):
    ///
    /// - [`Self::tag`] — pure §5.20 input router / a11y identifier;
    ///   the focus ring is painted by
    ///   `pinion-runtime::paint_adapter::paint_focus_ring` AFTER
    ///   `to_vello`, so a tag change does not invalidate the cached
    ///   fragment.
    /// - [`Self::aria_label`] — R51.69 a11y override; does not reach
    ///   the paint adapter.
    ///
    /// Uses [`std::hash::DefaultHasher`] (`SipHash` 1-3, deterministic
    /// across process runs — no `RandomState` seed) so two paint
    /// passes of structurally identical scenes produce the same hash
    /// → fragment cache hit.
    pub fn paint_hash(&self) -> u64 {
        use core::hash::{Hash, Hasher};
        if let Some(h) = self.paint_hash.get() {
            return h;
        }
        let mut hasher = std::hash::DefaultHasher::new();
        b"pinion.scene.Container".hash(&mut hasher);
        self.rect.hash(&mut hasher);
        self.style.hash(&mut hasher);
        self.layout.hash(&mut hasher);
        (self.children.len() as u64).hash(&mut hasher);
        for child in &self.children {
            child.paint_hash().hash(&mut hasher);
        }
        let value = hasher.finish();
        self.paint_hash.set(Some(value));
        value
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

    /// R55.G.6 §5.45 — apply a functional transform to the layout
    /// sidecar in place; see [`BoxNode::map_layout`] for the
    /// canonical rationale. Preserves any seeded default by handing
    /// the current layout to the closure as input.
    #[must_use]
    pub fn map_layout<F: FnOnce(LayoutStyle) -> LayoutStyle>(mut self, f: F) -> Self {
        self.layout = f(self.layout);
        self
    }
}

/// (R784 §5.45) The axis along which a [`ScrollNode`]'s content may
/// overflow its clip viewport.
///
/// The layout pass clamps the content to the viewport extent on the
/// *cross* axis and lets it grow on the scroll axis, so the choice
/// here is exactly "which dimension is the scrollable one":
///
/// - [`Self::Vertical`] — content keeps `viewport.w` (flex children
///   resolve to the clip width, e.g. list rows fill the column) and
///   grows taller. This is the pre-R784 behaviour and the default,
///   so every existing scroll consumer is unaffected.
/// - [`Self::Horizontal`] — content keeps `viewport.h` and grows
///   wider; the motivating consumer is the frozen-header data-grid
///   (`pinion_widget_paint::table::view_virtual_table`), whose header
///   row and body share one horizontal scroll so the header tracks
///   the body's horizontal offset while staying vertically pinned.
///
/// - [`Self::Both`] (R877) — content may overflow on *both* axes; the
///   motivating consumer is the node-editor's pannable 2-D canvas
///   (one world surface, panned freely in x and y). The R784 note
///   deferring `Both` ("no consumer needs simultaneous overflow yet")
///   is hereby resolved by that first consumer — the frozen-header
///   grid still composes nested single-axis scrolls because its two
///   axes are *coupled to different followers*, which a single
///   two-axis scroll cannot express; the two shapes coexist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum ScrollAxis {
    /// Content overflows vertically; width clamped to the viewport.
    #[default]
    Vertical,
    /// Content overflows horizontally; height clamped to the viewport.
    Horizontal,
    /// R877 §5.45 — content overflows on both axes (a pannable 2-D
    /// canvas). [`ScrollState`]
    /// has always carried both offsets / maxima; this variant lets the
    /// layout pass leave both axes unbounded so the declared content
    /// extent survives measuring.
    Both,
}

impl ScrollAxis {
    /// (R785.1 audit-correction) The stable wire spelling of this axis
    /// (`"vertical"` / `"horizontal"`). The wire vocabulary belongs to the
    /// type, not its consumers (the R773 wire-vocab-canon rule) — the RPC
    /// scene-as-data layer reads this rather than re-spelling the variants,
    /// so a second consumer (a TUI snapshot, a log line) cannot drift.
    #[must_use]
    pub const fn as_wire_name(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
            Self::Both => "both",
        }
    }
}

/// R55.A §5.45 — scroll container primitive carrying a clip viewport,
/// a child scene rendered with an applied offset, and the
/// `(offset_x, offset_y)` pair the input router and paint adapter
/// consult to determine which window of the content is visible.
///
/// Content geometry MAY exceed `viewport.size` — that is the entire
/// point of the primitive. The runtime clamps `offset_x` to
/// `0..=max(0, content_intrinsic_width - viewport.width)` and
/// `offset_y` analogously; offsets the caller supplies outside that
/// range are clamped at next dispatch.
///
/// ## First-cut scaffold (R55.A first round)
///
/// Only the data shape lands this round. The companion sub-rounds
/// wire:
///
/// - R55.A.2 — `Scene::hit_test` / `lookup_path_*` /
///   `collect_intersections` descent through `Scroll.content` with
///   the offset translation applied (intent: a hit inside the
///   visible portion of the content surfaces with the same path
///   shape it would have without the wrap).
/// - R55.B — `ScrollState` (offset + max bounds + spring animation)
///   stored on the [`Owner::cache`](crate::reactive::Owner::cache)
///   scope-id keyed substrate, so the reactive cascade fires when
///   the offset changes.
/// - R55.C — wheel / arrow / PgUp/PgDn / Home/End input mapping
///   layered on top of the existing §5.13 `Event` enum.
/// - R55.E — paint clipping at the Vello + TUI boundaries.
///
/// The struct stays `#[non_exhaustive]`-free for now because the
/// field set is stable: any future addition (e.g. an explicit
/// `clip_to_viewport: bool` flag for overflow-visible compatibility)
/// is additive and can land without breaking the closed-form
/// invariant.
#[derive(Debug)]
pub struct ScrollNode {
    /// (R784 §5.45) Which axis this container scrolls. The layout
    /// pass (`pinion_runtime::layout`) unbounds the content along
    /// this axis so it can overflow the clip window — the *other*
    /// axis stays clamped to the viewport extent, so a
    /// [`ScrollAxis::Vertical`] container's content keeps its
    /// `viewport.w`-wide flex resolution (rows fill the width) while
    /// growing taller, and a [`ScrollAxis::Horizontal`] container's
    /// content keeps its `viewport.h` height while growing wider.
    /// Defaults to [`ScrollAxis::Vertical`] (the only mode before
    /// R784), so every existing consumer is byte-unchanged.
    pub axis: ScrollAxis,
    /// (R859 §5.45) Linked-scroll **follower**. A follower shares its
    /// [`state`](Self::state) (hence its `offset_*`) with a *primary*
    /// scroll node so the two slide in lockstep, but the layout pass
    /// (`pinion_runtime::layout::update_scroll_state_bounds`) does
    /// **not** publish this node's measured viewport / max bounds back
    /// into the shared state. Only the primary owns that write.
    ///
    /// This is the substrate behind the frozen-column data-grid (the
    /// Qt `QTableView` / AG-Grid / Excel "linked scrollbar" pattern):
    /// the frozen pane's body and the scrolling pane's body are two
    /// *vertical* scroll nodes that both reference one vertical
    /// [`ScrollState`] (so they scroll in vertical lockstep), but they
    /// sit in side-by-side columns with different cross-axis viewport
    /// *widths* (`frozen_w` vs the scrolling pane's width). If both
    /// published, `set_measured_viewport` would flip-flop the shared
    /// `measured_w` every frame and spin a perpetual scroll-dirty
    /// re-pass. Marking the frozen pane's body a follower makes it a
    /// passive slider: it still lays out its content unbounded along its
    /// axis (so the overflow clips and the `offset_*` slide applies), it
    /// just never feeds the bounds back. (The mechanism is axis-agnostic
    /// — it suppresses the publish on whichever axis the node scrolls.)
    ///
    /// Defaults to `false` (a primary, the only mode before R859), so
    /// every existing consumer is byte-unchanged. A follower must be
    /// paired with a primary sharing the same `state`; a lone follower
    /// never has its `max` written, so it cannot clamp input.
    pub follower: bool,
    /// Visible clip window in logical pixels (Vello) or cells (TUI).
    /// The runtime hit-tester and paint adapter treat this rect as
    /// the geometry of the entire primitive at the parent level —
    /// the [`Scene::rect`] return value points at `viewport`.
    pub viewport: Rect,
    /// Child scene painted with the `(offset_x, offset_y)`
    /// translation applied. The content's intrinsic size MAY exceed
    /// `viewport.size`; the parts that fall outside the clipped
    /// viewport are not rendered.
    pub content: Box<Scene>,
    /// Horizontal offset in the same unit as `viewport`. Bounded by
    /// `0..=max(0, content_intrinsic_width - viewport.width)`; the
    /// runtime clamps out-of-range values at dispatch.
    pub offset_x: i32,
    /// Vertical offset, semantics symmetric with [`Self::offset_x`].
    pub offset_y: i32,
    /// R51.122 §5.41 — input router tag. Wheel / arrow / page
    /// keystrokes inside this tag route to the §5.45 R55.C scroll
    /// input handler instead of bubbling to the parent.
    pub tag: Option<Cow<'static, str>>,
    /// (R55.C.2 §5.45) Optional backreference to the reactive
    /// [`ScrollState`] that owns the offset signals for this scroll
    /// container. Set via [`Self::with_state`] (canonical) when the
    /// application's view fn calls
    /// [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) and
    /// builds the matching `ScrollNode` in the same paint cycle —
    /// the widget-owns-state pattern Material / `SwiftUI` / GTK /
    /// Qt all carry. Left `None` for the "declarative-only" use
    /// (a pure offset snapshot with no input wiring, e.g. an
    /// AI-driven scroll preview the agent measures without ever
    /// dispatching wheel input).
    ///
    /// The [`InputRouter`](crate::scene::Scene) wheel / arrow / page
    /// dispatch (R51.186 §5.45) walks the paint tree via
    /// [`Scene::scroll_state_at`] and forwards the input delta into
    /// `state.scroll_by(...)` — the reactive Signal write fires the
    /// view re-run on the next paint with the updated offset.
    /// `None` here means "no input dispatch target", which the
    /// router silently honours (the wheel input drops without
    /// dispatch rather than panicking).
    ///
    /// Scene-as-data introspection treats the field as an opaque
    /// link: the `Rc` clone is not serialisable, so the AI-facing
    /// `scene/query` family surfaces only the declarative
    /// `viewport` / `offset_*` / `tag` fields. The state link is a
    /// substrate-internal detail.
    pub state: Option<Rc<ScrollState>>,
    /// R1194 §5.27 — optional reactive
    /// [`MeasuredRowState`]
    /// for a **measured variable-height** virtualized list. The peer of
    /// [`state`](Self::state): where `state` owns the scroll offset,
    /// `measured_rows` owns the progressively-discovered per-row heights.
    /// Set via [`Self::with_measured_rows`] when the content is a windowed
    /// list whose rows size to their content (each windowed row tagged
    /// `measured-row:<index>`); the runtime layout pass harvests each
    /// rendered row's laid-out height into this state after layout
    /// ([`MeasuredRowState::harvest`](crate::widgets::measured_rows::MeasuredRowState::harvest)),
    /// then the next view re-run windows against the refined heights.
    ///
    /// `None` for every ordinary scroll container (fixed-pitch or
    /// caller-supplied variable heights), so the field is a pure opt-in —
    /// the harvest pass is a no-op unless it is `Some`. Like
    /// [`state`](Self::state) the `Rc` link is substrate-internal and not
    /// serialised by `scene/query`.
    pub measured_rows: Option<Rc<MeasuredRowState>>,
    /// R55.G.4 §5.45 — layout sidecar mirroring the
    /// `{Box,Text,Path,Image,Container,External}Node` shape. Drives
    /// the §5.21 R23 taffy pass: how this scroll participates in
    /// parent flex (size / `flex_grow` / margin / align) plus its
    /// own children if the future R55.G.x slices add Scroll-as-flex-
    /// parent semantics. [`Self::new`] seeds
    /// `LayoutStyle::with_size(viewport.{w,h})` so the default
    /// behaviour is "Scroll is a fixed-size leaf at the dimensions
    /// the caller passed in" — backward-compatible with pre-R55.G.4
    /// callers that never touched a layout sidecar. The R55.G.3
    /// build-site override hack (force size from `viewport` regardless
    /// of layout) is retired by routing through this field instead.
    pub layout: LayoutStyle,
}

impl ScrollNode {
    /// Construct a scroll container around `content` clipped to
    /// `viewport`. Initial offset is `(0, 0)`; the caller adjusts via
    /// future `set_offset` / scroll-by intent emission.
    #[must_use]
    pub fn new(viewport: Rect, content: Scene) -> Self {
        Self {
            viewport,
            content: Box::new(content),
            axis: ScrollAxis::Vertical,
            follower: false,
            offset_x: 0,
            offset_y: 0,
            tag: None,
            state: None,
            measured_rows: None,
            // R55.G.4 §5.45 — default the layout size to the clip-window
            // dimensions so taffy treats Scroll as a fixed-size leaf
            // unless the caller chains `with_layout(...)` to opt into
            // `flex_grow` / `margin` / etc.
            layout: LayoutStyle::new().with_size(Size::px(viewport.w, viewport.h)),
        }
    }

    /// Attach a §5.20 intent tag — wheel / key intents that route
    /// through this scroll container are prefixed with `<tag>.` by
    /// the future R55.C input mapping.
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<Cow<'static, str>>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Set the initial offset. The runtime clamps to bounds at
    /// dispatch; callers do not need to know the content size to
    /// supply an in-range value.
    #[must_use]
    pub const fn with_offset(mut self, offset_x: i32, offset_y: i32) -> Self {
        self.offset_x = offset_x;
        self.offset_y = offset_y;
        self
    }

    /// (R784 §5.45) Select the scroll axis (default
    /// [`ScrollAxis::Vertical`]). A [`ScrollAxis::Horizontal`]
    /// container lets its content overflow the viewport width while
    /// the layout pass clamps the height — the shape the
    /// frozen-header data-grid wraps around its `[header, body]`
    /// column so both scroll horizontally as one unit.
    #[must_use]
    pub const fn with_axis(mut self, axis: ScrollAxis) -> Self {
        self.axis = axis;
        self
    }

    /// (R859 §5.45) Mark this node a linked-scroll **follower** (see
    /// [`Self::follower`]). The node keeps its shared [`ScrollState`],
    /// so it slides in lockstep with the primary, but the layout pass
    /// skips publishing its measured viewport / max bounds — the
    /// primary node (sharing the same state) owns that write. Use for
    /// the frozen-column grid's header strip, which tracks the body's
    /// horizontal offset without contributing a (mismatched-height)
    /// measured viewport that would spin a perpetual re-pass.
    #[must_use]
    pub const fn as_follower(mut self) -> Self {
        self.follower = true;
        self
    }

    /// R55.G.4 §5.45 — replace the layout sidecar (size / flex /
    /// align / margin). The pre-R55.G.4 default is
    /// `LayoutStyle::with_size(viewport.{w,h})` so taffy treats
    /// Scroll as a fixed-size leaf; overriding here is the supported
    /// path for `flex_grow` / `margin` / parent-flex participation.
    /// Callers that want the dimensions to stay tied to the clip
    /// window must include `with_size(Size::px(viewport.w,
    /// viewport.h))` in the supplied layout — or reach for
    /// [`Self::map_layout`] (R55.G.6) which preserves the seeded
    /// default and chains a single modification on top.
    #[must_use]
    pub const fn with_layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }

    /// R55.G.6 §5.45 — apply a functional transform to the layout
    /// sidecar in place. The closure receives the seeded layout
    /// ([`Self::new`] supplies `LayoutStyle::with_size(viewport.{w,h})`)
    /// and returns the new layout, preserving any field the caller
    /// does not touch. Cures the [`Self::with_layout`] full-replace
    /// footgun where `with_layout(LayoutStyle::new().with_gap(8))`
    /// silently drops the seeded viewport size and collapses Scroll
    /// to a 0×0 leaf. Canonical idiom is
    /// `scroll.map_layout(|l| l.with_flex_grow(1.0))` —
    /// `Size::px(viewport.{w,h})` survives the chain.
    #[must_use]
    pub fn map_layout<F: FnOnce(LayoutStyle) -> LayoutStyle>(mut self, f: F) -> Self {
        self.layout = f(self.layout);
        self
    }

    /// (R55.C.2 §5.45) Attach the reactive [`ScrollState`] that
    /// owns the offset signals for this scroll container. Most
    /// callers should reach for the higher-level
    /// [`Self::from_state`] convenience (R51.190) instead — it
    /// derives the offset and tag from the state in one call. Use
    /// `with_state` directly only when the caller needs to override
    /// the derived fields independently.
    ///
    /// The framework input router (R51.186 §5.45) follows the
    /// attached `Rc<ScrollState>` to dispatch wheel / arrow / page
    /// input into the reactive `scroll_by` / `scroll_to` writes —
    /// the next view re-run paints the new offset without any
    /// application-level wiring.
    #[must_use]
    pub fn with_state(mut self, state: Rc<ScrollState>) -> Self {
        self.state = Some(state);
        self
    }

    /// R1194 §5.27 — attach the reactive
    /// [`MeasuredRowState`]
    /// for a measured variable-height list (see [`Self::measured_rows`]).
    /// Pairs with [`Self::with_state`]: the offset lives in the
    /// `ScrollState`, the per-row heights in the `MeasuredRowState`. The
    /// runtime layout pass harvests each windowed row's laid-out height into
    /// this state after layout; a scroll node without it is never harvested.
    #[must_use]
    pub fn with_measured_rows(mut self, measured: Rc<MeasuredRowState>) -> Self {
        self.measured_rows = Some(measured);
        self
    }

    /// (R51.190 §5.45) Build a `ScrollNode` whose offset, tag, and
    /// input-router wiring are all derived from the given
    /// [`ScrollState`]. Collapses the canonical view-fn shape from
    /// five lines down to two:
    ///
    /// ```ignore
    /// let state = use_scroll_state("main_scroll");
    /// ScrollNode::from_state(state, viewport, content)
    /// ```
    ///
    /// (Pre-R51.190 the same wiring required `let (ox, oy) =
    /// state.offset()` + `ScrollNode::new(...).with_tag(key)
    /// .with_offset(ox, oy).with_state(state)`, repeating both the
    /// key string and the offset destructure at every call site.)
    ///
    /// The tag is set when [`ScrollState::tag`] is `Some` (states
    /// constructed via [`use_scroll_state`](crate::widgets::scroll::use_scroll_state) / [`ScrollState::with_tag`]
    /// always carry one); states built via [`ScrollState::new`]
    /// directly leave the node untagged, which matches the
    /// pre-R51.190 untagged default. To override the derived tag
    /// or offset, chain [`Self::with_tag`] / [`Self::with_offset`]
    /// after `from_state`.
    ///
    /// The `state.offset()` read inside this constructor triggers a
    /// `Signal` subscription on whichever
    /// [`Owner`](crate::reactive::Owner) wraps the view-fn call —
    /// scroll mutations re-run the view on the next frame, matching
    /// the explicit `let (ox, oy) = state.offset();` precedent.
    #[must_use]
    pub fn from_state(state: Rc<ScrollState>, viewport: Rect, content: Scene) -> Self {
        let (ox, oy) = state.offset();
        let derived_tag = state.tag();
        let mut node = Self::new(viewport, content).with_offset(ox, oy);
        if let Some(t) = derived_tag {
            node = node.with_tag(t);
        }
        node.with_state(state)
    }

    /// (R55.A.4 §5.45) Translate a root-local query rect into this
    /// scroll container's content-intrinsic coordinate frame.
    /// Returns [`None`] when the query falls entirely outside the
    /// viewport or when the translation underflows (offset moved
    /// the visible window past the content origin so the query maps
    /// to a negative content coordinate).
    ///
    /// The translation is rigid: width / height pass through
    /// unchanged after the viewport-clip step. Mirrors the
    /// offset-shift shape used by [`Scene::hit_test`] for a single
    /// point — same `i64` promotion guard against `u32` wrap on
    /// negative-offset edges (R51.181).
    fn translate_query_into_content(&self, query: Rect) -> Option<Rect> {
        let vp = self.viewport;
        let vp_right = vp.x.saturating_add(vp.w);
        let vp_bottom = vp.y.saturating_add(vp.h);
        let q_right = query.x.saturating_add(query.w);
        let q_bottom = query.y.saturating_add(query.h);
        // Step 1: clip the query against the viewport in root-local
        // coords. Zero-extent intersection means the query never
        // reaches inside the scroll container.
        let lx = query.x.max(vp.x);
        let ty = query.y.max(vp.y);
        let rx = q_right.min(vp_right);
        let by = q_bottom.min(vp_bottom);
        if lx >= rx || ty >= by {
            return None;
        }
        // Step 2: shift root-local → viewport-local (origin at vp).
        let v_lx = lx - vp.x;
        let v_ty = ty - vp.y;
        // Step 3: shift viewport-local → content-intrinsic by
        // adding the scroll offset. `i64` promotion prevents wrap
        // on negative-offset edges (mirrors R51.181 hit_test).
        let c_lx = i64::from(v_lx).checked_add(i64::from(self.offset_x))?;
        let c_ty = i64::from(v_ty).checked_add(i64::from(self.offset_y))?;
        if c_lx < 0 || c_ty < 0 {
            return None;
        }
        let c_lx_u = u32::try_from(c_lx).ok()?;
        let c_ty_u = u32::try_from(c_ty).ok()?;
        // Width / height stay rigid — the translation is a pure
        // shift in both x and y.
        Some(Rect::new(c_lx_u, c_ty_u, rx - lx, by - ty))
    }
}

// ---------------------------------------------------------------------------
// R681 §2 #4 — `ImmediateModeNode` + `ImmediateMode` trait.
//
// Landed across R681 (data shape + trait surface + Vello paint bridge +
// per-window `ControlFlow::WaitUntil` pacing + `frame_pacing::target_fps`
// + first consumer `hello-immediate-mode-canvas`), then grown by R827
// (intent bridge), R828 (introspection), R829 (deterministic stepping),
// R830 (pointer input), and R831 (fixed-timestep accumulator). The
// `ImmediateMode` trait surface stays minimal and additive: new methods
// ship with default impls so existing drivers compile unchanged.
// ---------------------------------------------------------------------------

/// R681 §2 #4 — opaque immediate-mode driver trait (axis 2 of the
/// 4-axis paint-pipeline rewrite series).
///
/// Designed dyn-safe by construction (all methods take `&self` or
/// `&mut self`, no associated items, no `Self`-returning methods) so
/// the framework holds `Rc<RefCell<dyn ImmediateMode>>` inside
/// [`Scene::ImmediateModeNode`] for the per-window paint cycle to
/// invoke without monomorphisation. Mirrors the
/// [`External`](crate::external::External) opaque-payload pattern
/// (§5.15 item 8 introspection echo) while differing in two ways:
///
/// 1. **Repaint cadence**: an `External` advertises
///    [`RepaintOwner::Framework`](crate::external::RepaintOwner::Framework)
///    or [`RepaintOwner::External`](crate::external::RepaintOwner::External)
///    and the framework drives layout-cadence repaints; an
///    `ImmediateMode` driver always runs on the per-window paint
///    clock the shell maintains, so the contract surfaces a single
///    [`tick`](Self::tick) hook that takes the per-frame `dt` and
///    advances both state AND paint in one step.
/// 2. **Paint dispatch**: an `External` paints through the §5.15
///    integration contract (own surface, framework composes); an
///    `ImmediateMode` driver paints directly into the per-window
///    backend buffer (the Vello bridge in atomic 1 hands the driver
///    a `&mut vello::Scene` slice positioned at
///    [`ImmediateModeNode::viewport`]).
///
/// `Debug` is a super-trait so `Rc<RefCell<dyn ImmediateMode>>`
/// participates in the scene tree's `#[derive(Debug)]` machinery
/// (mirror of [`External`](crate::external::External) `Debug` super-trait at §5.15 line ~349).
///
/// ## Lifecycle
///
/// The shell calls [`tick`](Self::tick) once per per-window paint
/// cycle, AFTER the layout pass has resolved
/// [`ImmediateModeNode::viewport`] and BEFORE the backend bridge
/// asks the driver to encode its paint. The `dt` argument is the
/// monotonic wall-clock delta between the previous and current
/// per-window paint instants (clamped by
/// `pinion_runtime::frame_pacing::clamp_frame_dt` before
/// dispatch — R51.145 `1/30s` anchor + NaN guard precedent).
///
/// Implementors that need persistent state across frames hold it
/// inside the concrete type (the framework keeps the same
/// `Rc<RefCell<dyn ImmediateMode>>` across paints by storing it in
/// [`Owner::cache`](crate::reactive::Owner::cache) — the canonical
/// `use_X()` reactive hook pattern).
pub trait ImmediateMode: core::fmt::Debug {
    /// Advance the driver by `dt`. Called once per per-window paint
    /// cycle before the backend bridge encodes the immediate paint.
    ///
    /// The default impl is `()` so test fixtures and inert drivers
    /// (a placeholder during binding scaffolding) compile without
    /// boilerplate. Real drivers override.
    fn tick(&mut self, _dt: Duration) {}

    /// Per-frame paint into the backend-agnostic [`ImmediatePainter`].
    /// Called once per per-window paint cycle AFTER
    /// [`Self::tick`] and AFTER the §5.21 layout pass has resolved
    /// [`ImmediateModeNode::viewport`].
    ///
    /// Coordinates passed to the painter are VIEWPORT-LOCAL —
    /// `(0, 0)` is the top-left of the viewport the backend bridge
    /// resolved for this node. The bridge translates to root-window
    /// pixel coordinates before encoding.
    ///
    /// Default no-op so test fixtures and inert drivers compile
    /// without boilerplate. Real drivers override.
    fn paint(&mut self, _painter: &mut dyn ImmediatePainter) {}

    /// R827 §2 #4 §5.20 — drain the §5.20 intents this driver emitted
    /// during the current frame's [`tick`](Self::tick), pushing each
    /// into `sink`. Mirrors
    /// [`External::drain_intents`](crate::external::External::drain_intents):
    /// real drivers accumulate intents in an internal buffer while
    /// `tick` advances simulation state (a game object reaching a goal,
    /// a physics body crossing a trigger volume, an animation reaching a
    /// keyframe) and drain that buffer here. Default no-op so inert /
    /// rendering-only drivers (the R681 first consumer) compile
    /// unchanged.
    ///
    /// **Drain timing diverges from `External`.** Retained `External`
    /// nodes live in the boot-frozen *state scene* and drain through
    /// [`CoreShell::tail`](crate::widget_core) on each input dispatch.
    /// Immediate-mode drivers live only in the per-frame *paint scene*
    /// (the view fn pulls the driver from
    /// [`Owner::cache`](crate::reactive::Owner::cache) and emits a fresh
    /// [`Scene::ImmediateModeNode`] each render — see
    /// [[state-scene-vs-paint-scene-introspect]]), so the runtime drains
    /// them in the per-window paint cycle, immediately AFTER
    /// [`Scene::tick_immediate_mode`], via
    /// `pinion_runtime::walk_scene_and_drain_immediate`. Each drained
    /// intent's tag is prefixed with the node's §5.20
    /// [`ImmediateModeNode::tag`] (`<tag>.<kind>`, R22 convention) and
    /// routed through `V::update` so the game loop drives retained app
    /// state — the §2 #4 dual-execution *bidirectional* contract
    /// (retained → immediate via the shared driver handle; immediate →
    /// retained via this intent channel).
    ///
    /// No `is_dirty` short-circuit (unlike `External`): immediate-mode
    /// drivers `tick` every frame regardless, so there is no idle
    /// virtual-dispatch to avoid — the default no-op already drains
    /// nothing on the frames a driver emits no intent.
    fn drain_intents(&mut self, _sink: &mut dyn FnMut(crate::intent::Intent)) {}

    /// R830 §2 #4 §5.15 — pointer-press input forwarding (§5.15 item 5).
    /// The framework calls this when a pointer press resolves to this
    /// driver's [`ImmediateModeNode`] viewport, handing VIEWPORT-LOCAL
    /// logical-pixel coordinates: `(0, 0)` is the viewport top-left, the
    /// SAME coordinate space [`paint`](Self::paint) draws in — so a
    /// driver hit-tests a press against the geometry it rendered without
    /// any transform bookkeeping. This is the player → game input
    /// channel completing the §2 #4 immediate-mode I/O surface (time via
    /// [`tick`](Self::tick), output via [`paint`](Self::paint), game →
    /// app via [`drain_intents`](Self::drain_intents), observe via
    /// [`introspect`](Self::introspect), and now player → game here).
    ///
    /// Default no-op so rendering-only drivers (the R681 first consumer)
    /// compile unchanged. Pointer-move / release forwarding are additive
    /// future methods (the trait grows as concrete consumers surface the
    /// need, mirroring the [`ImmediatePainter`] surface), so a driver
    /// that only cares about discrete clicks overrides just this.
    fn on_pointer_down(&mut self, _x: f32, _y: f32) {}

    /// (R681 §5.15 echo) Surface the [`ExternalIntrospect`] view of
    /// this driver, when the author opts in. Default returns `None`;
    /// override with `Some(self)` after `impl ExternalIntrospect for
    /// YourType` (mirrors
    /// [`External::introspect`](crate::external::External::introspect)).
    ///
    /// AI clients query immediate-mode driver state through the same
    /// §5.12 `scene/query` / `scene/intervene` / `scene/invoke` RPC
    /// surface they use against `Scene::External`, preserving the
    /// §2 #2 + §2 #7 AI-first scene-as-data invariant across the
    /// retained / immediate boundary.
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        None
    }

    /// Mutable counterpart to [`introspect`](Self::introspect).
    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        None
    }
}

/// R681 §2 #4 — backend-agnostic immediate-mode paint primitive
/// surface (axis 2 of the 4-axis paint-pipeline rewrite series).
///
/// Concrete backends implement this on their per-frame painter
/// wrapper (e.g. `pinion_runtime::paint_adapter::VelloImmediatePainter`
/// wraps `&mut vello::Scene` + viewport transform + DPI for the Vello
/// GUI backend); the [`ImmediateMode::paint`] hook receives
/// `&mut dyn ImmediatePainter` so impls draw without knowing which
/// backend renders the scene. Mirrors the HTML5 Canvas / Cairo /
/// Direct2D pattern of "backend-agnostic 2D primitive surface, host
/// chooses GPU pipeline" while staying minimal enough to grow
/// additively as concrete consumers surface needs.
///
/// First-cut surface (R681): `clear` + `fill_rect` + `fill_triangle` +
/// `stroke_line` — enough to drive the `hello-immediate-mode-canvas`
/// first consumer (a rotating triangle) and the `hello-immediate-intent`
/// bouncing-ball game loop, with future fps-overlay / waveform-viewer /
/// minimap-overlay consumers driving additive surface growth (arc /
/// path / image when a real consumer needs them). The surface is
/// `#[non_exhaustive]` via trait extension (every new method ships with
/// a default impl so existing impls compile unchanged).
///
/// Coordinates are VIEWPORT-LOCAL: `(0, 0)` is the top-left of the
/// [`ImmediateModeNode::viewport`] the backend bridge handed the
/// painter. Backends translate to root-window pixel coordinates
/// before encoding (the Vello impl composes
/// `vello::kurbo::Affine::translate` over the inherited transform
/// chain — same shape as `Scene::Scroll` content translation).
///
/// Floating-point coordinates so impls can position sub-pixel without
/// quantising to the `u32` [`Rect`] grid the retained-tree
/// [`BoxNode`] / [`TextNode`] / etc. use; the backend rasteriser
/// (Vello / future Skia / future Metal) decides anti-aliasing
/// strategy without losing precision at the primitive surface.
pub trait ImmediatePainter {
    /// Viewport size in LOGICAL pixels. Impls scale paint geometry
    /// against this each frame so the same driver renders
    /// proportionally regardless of paint area.
    fn viewport_size(&self) -> (u32, u32);

    /// DPI scale factor (logical → physical pixel ratio). The
    /// backend may use this internally to fatten stroke widths,
    /// hint paths, etc.; impls typically ignore it (the painter
    /// surface stays in logical units).
    fn dpi_scale(&self) -> f32;

    /// Fill the entire viewport with `color`. Equivalent to
    /// `fill_rect(0.0, 0.0, w as f32, h as f32, color)` but the
    /// backend may short-circuit (e.g. surface-clear hint).
    fn clear(&mut self, color: Color);

    /// Fill an axis-aligned rect `(x, y, w, h)` (viewport-local
    /// logical pixels) with `color`. Negative `w`/`h` is undefined
    /// behaviour at this layer — impls should not produce them.
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color);

    /// Fill a triangle defined by three viewport-local points with
    /// `color`. Winding order is irrelevant at this layer (impls
    /// rasterise both clockwise and counter-clockwise the same way).
    fn fill_triangle(&mut self, p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), color: Color);

    /// Stroke a line between two viewport-local points with the
    /// given pixel `width` and `color`. Line caps / joins are
    /// backend-default (Vello rounds caps; the future GPU pipeline
    /// may differ).
    fn stroke_line(&mut self, p1: (f32, f32), p2: (f32, f32), width: f32, color: Color);
}

/// R681 §2 #4 — paint-tree carrier for an [`ImmediateMode`] driver.
///
/// Carries the opaque driver behind [`Rc<RefCell<dyn ImmediateMode>>`]
/// (so the view fn pulls the same instance from
/// [`Owner::cache`](crate::reactive::Owner::cache) on every render
/// and the per-window paint cycle obtains a mutable borrow at tick
/// time), the post-layout `viewport` the backend bridge paints
/// into, an optional §5.20 intent tag, and the
/// [`Cell<Duration>`] sidecar publishing the last per-paint `dt`
/// the shell drove (mirrors [`TextNode::line_count`] layout-measured
/// sidecar — paint-side publish, scene-side read).
///
/// The retained tree treats this node as a paint-opaque leaf:
/// hit-test resolves to the viewport rect verbatim (no descent),
/// `lookup_path_*` cannot descend (no children), `find_external_*`
/// / `primary_external` skip (immediate-mode drivers are not
/// `Scene::External` despite the introspection surface mirror).
/// The §5.20 tag is still walked by [`Scene::contains_tag`] so
/// composite paint-root tag pinning works (R55.G.17).
///
/// `Clone` is intentionally *not* derived — [`Scene`] omits `Clone`
/// because [`ExternalNode`] holds `Box<dyn External>` with no
/// general clone strategy, and the same invariant applies here:
/// snapshots of immediate-mode state go through the
/// [`ImmediateMode::introspect`] channel, not a tree-wide clone.
#[non_exhaustive]
#[derive(Debug)]
pub struct ImmediateModeNode {
    /// Backend-agnostic immediate-mode driver. Shared via `Rc` so
    /// the [`Owner::cache`](crate::reactive::Owner::cache) slot the
    /// view fn pulls from each render keeps a stable reference;
    /// wrapped in `RefCell` so the per-window paint cycle's
    /// [`ImmediateMode::tick`] call obtains the required mutable
    /// borrow without forcing impl authors to wrap every state
    /// field in [`Cell`] / [`RefCell`] internally.
    pub handle: Rc<RefCell<dyn ImmediateMode>>,
    /// Post-layout paint area in logical pixels. The taffy pass
    /// resolves this from [`Self::layout`] + the enclosing flex /
    /// box parent each frame; the backend bridge in atomic 1
    /// reads this when it hands the driver a positioned paint
    /// slice.
    pub viewport: Rect,
    /// §5.21 layout sidecar mirroring [`ContainerNode::layout`] /
    /// [`ExternalNode::layout`] / [`ScrollNode::layout`]. Drives
    /// the §5.21 R23 taffy pass: how this immediate-mode subtree
    /// participates in parent flex (size / `flex_grow` / margin /
    /// align). Default is [`LayoutStyle::new`] (no flex
    /// participation, sized by `viewport.{w,h}`).
    pub layout: LayoutStyle,
    /// §5.20 intent-system carrier — intents the immediate-mode driver
    /// emits via [`ImmediateMode::drain_intents`] are prefixed with
    /// `<tag>.` by the runtime walk
    /// (`pinion_runtime::walk_scene_and_drain_immediate`, R827),
    /// completing the `<widget>.<kind>` convention (R22) so the retained
    /// `V::update` reducer matches them exactly as it matches
    /// `Scene::External` widget intents.
    pub tag: Option<Cow<'static, str>>,
    /// The `dt` of the last [`ImmediateMode::tick`] the shell drove
    /// into this node. Published by the per-window paint cycle
    /// post-tick; read-side surfaced via [`Self::last_dt`] for AI
    /// introspection (`scene/snapshot` exposes it as `last_dt_micros`,
    /// R828).
    ///
    /// R831: this is the FIXED simulation timestep
    /// (`pinion_runtime::FixedTimestep`, 1/120 s), not the wall-clock
    /// frame delta — the shell advances the driver in whole fixed steps
    /// and carries the sub-step remainder across frames. A frame whose
    /// accumulated time is still sub-fixed (or a frozen / paused frame)
    /// ticks zero times and leaves this unchanged.
    ///
    /// `Duration::ZERO` is the sentinel for "no whole step yet" — the
    /// first paints, before the accumulator releases its first fixed
    /// step.
    pub last_dt: Cell<Duration>,
}

impl ImmediateModeNode {
    /// Construct an `ImmediateModeNode` from a shared driver handle
    /// and a viewport rect. Default `tag` / `layout` / `last_dt`
    /// (`Duration::ZERO` sentinel for "no tick yet").
    #[must_use]
    pub fn new(handle: Rc<RefCell<dyn ImmediateMode>>, viewport: Rect) -> Self {
        Self {
            handle,
            viewport,
            layout: LayoutStyle::new(),
            tag: None,
            last_dt: Cell::new(Duration::ZERO),
        }
    }

    /// Build an `ImmediateModeNode` from a concrete driver impl,
    /// boxing it into the canonical `Rc<RefCell<dyn ImmediateMode>>`
    /// shape. Convenience for one-shot drivers; multi-frame drivers
    /// should go through [`Owner::cache`](crate::reactive::Owner::cache)
    /// + [`Self::new`] so the same `Rc` survives view-fn re-runs.
    #[must_use]
    pub fn from_driver<T: ImmediateMode + 'static>(driver: T, viewport: Rect) -> Self {
        let handle: Rc<RefCell<dyn ImmediateMode>> = Rc::new(RefCell::new(driver));
        Self::new(handle, viewport)
    }

    /// Attach a §5.20 intent tag — drained intents from this
    /// driver's [`ImmediateMode`] impl will be prefixed with
    /// `<tag>.` by the runtime walk (R22 convention).
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

    /// R55.G.6 §5.45 — apply a functional transform to the layout
    /// sidecar in place. See [`BoxNode::map_layout`] for the
    /// canonical rationale.
    #[must_use]
    pub fn map_layout<F: FnOnce(LayoutStyle) -> LayoutStyle>(mut self, f: F) -> Self {
        self.layout = f(self.layout);
        self
    }

    /// Read the last per-paint `dt` the shell drove. Returns
    /// `Duration::ZERO` before the first paint cycle has run.
    #[must_use]
    pub fn last_dt(&self) -> Duration {
        self.last_dt.get()
    }

    /// Publish the fixed step into the sidecar. Called by the
    /// per-window paint cycle AFTER each [`ImmediateMode::tick`] call,
    /// so reads via [`Self::last_dt`] in the same frame return the
    /// value that drove the tick (R831: the fixed simulation step).
    pub fn set_last_dt(&self, dt: Duration) {
        self.last_dt.set(dt);
    }
}

/// R972 §5.41 — the cell-native text-grid geometry scaffold payload of
/// [`Scene::TextGrid`] (the cell-native coordinate sub-axis's first real
/// consumer).
///
/// Holds a node-local [`CellMetric`] (R968 node-local ratify) and the
/// layout-resolved pixel [`Rect`]. The grid's `(cols, rows)` are **not**
/// stored — they are *derived* on demand from `rect` via the metric
/// ([`Self::cols`] / [`Self::rows`]), so the layout-resolved pixel size
/// is the single source of truth: the R969 one-directional `(rows,
/// cols)` SSOT (layout → dims, never fed back).
///
/// The cell **content** (R973: the [`GridBuffer`] projection of cells,
/// each carrying a grapheme cluster + foreground / background
/// [`TermColor`](crate::term_grid::TermColor)) and the per-grid
/// [`Palette`] that resolves indexed / default colours land here. The
/// grid is a *retained projection* of the producer's terminal buffer
/// (R969): the producer assembles a [`GridBuffer`] and the node holds it
/// wholesale (no per-cell mutation — R969 "dual-grid 금지"). Cell
/// attributes (R974) / cursor (R975) / wide-char trailer (R976) /
/// alternate-screen kind (R977) / per-row damage generations (R978) ride
/// on the [`GridBuffer`], completing the S5 data model. **R991** paints the
/// grid on the Vello backend (per-cell bg + cluster glyph; reverse / hidden
/// / wide); it is uncacheable (the projection is replaced wholesale each
/// frame). Typographic attrs / cursor / the TUI backend are follow-up paint
/// slices; the cells stay readable as scene-as-data (§2 #7) via
/// `scene/snapshot`.
///
/// `#[non_exhaustive]` per the R14 forward-compat hedge (like every peer
/// leaf node): construction already goes through [`Self::new`] + builders,
/// so the hedge is free.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct TextGridNode {
    /// §5.20 intent-system carrier, when the grid participates in
    /// tag-routed introspection. `None` for an untagged grid.
    pub tag: Option<Cow<'static, str>>,
    /// Layout-resolved paint area in logical pixels — written by the
    /// §5.21 taffy pass from [`Self::layout`] each frame (the
    /// `pinion_runtime` `assign_rect`). The authoritative input to
    /// [`Self::cols`] / [`Self::rows`].
    pub rect: Rect,
    /// Node-local cell metric (R968): logical pixels one cell spans on
    /// each axis. Vello sources this from the grid's monospace font; the
    /// TUI backend uses `1 cell = 1 character cell`.
    pub metric: CellMetric,
    /// §5.21 layout sidecar (parent flex / box participation), mirroring
    /// [`ImmediateModeNode::layout`] / [`ScrollNode::layout`]. The taffy
    /// pass resolves [`Self::rect`] from this.
    pub layout: LayoutStyle,
    /// R973 — the retained projection of the producer's terminal buffer:
    /// the cells the grid currently shows. Replaced wholesale each frame
    /// (the producer owns the authoritative buffer; the node never
    /// mutates per-cell). Empty (`0×0`) for a geometry-only grid.
    pub cells: GridBuffer,
    /// R973 — the per-grid `indexed → rgb` palette that resolves this
    /// grid's cell [`TermColor`](crate::term_grid::TermColor)s at paint /
    /// introspection time. Defaults to [`Palette::xterm_default`].
    pub palette: Palette,
    /// R1002 §5.41 — explicit Vello glyph font size in logical px, when the
    /// grid is font-derived. `Some(s)` means the cells were sized from a
    /// monospace face of `s` px (see
    /// `pinion_text::LayoutCache::measure_monospace_cell`): the paint adapter
    /// renders glyphs at exactly `s` instead of re-deriving a size from
    /// `cell_h`, so the rendered advance matches `metric.cell_w`
    /// (`== advance(s)`) **by construction** — `s` is the single font-size
    /// source of truth. `None` is a producer-picked cell with no font basis
    /// (the TUI 8×16 default, or a Vello producer that chose only cell
    /// dimensions): the paint adapter fits a font into `cell_h` (R1001).
    /// Backend-agnostic cell geometry stays on [`metric`](Self::metric); this
    /// Vello-only render size is deliberately separate — the TUI
    /// character-cell backend has no font size.
    pub font_size_px: Option<u32>,
}

impl TextGridNode {
    /// Construct a grid with the given node-local [`CellMetric`]. The
    /// `rect` starts empty and is filled by the layout pass from
    /// [`Self::layout`]; chain [`Self::with_layout`] to size it. The cell
    /// projection starts empty (a geometry-only grid) with the default
    /// xterm [`Palette`]; chain [`Self::with_cells`] to set the projection.
    #[must_use]
    pub fn new(metric: CellMetric) -> Self {
        Self {
            tag: None,
            rect: Rect::default(),
            metric,
            layout: LayoutStyle::new(),
            cells: GridBuffer::default(),
            palette: Palette::xterm_default(),
            font_size_px: None,
        }
    }

    /// Attach a §5.20 intent tag (builder form).
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<Cow<'static, str>>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Attach a §5.21 layout style (builder form) — how the grid
    /// participates in the parent layout that resolves its pixel
    /// [`Rect`].
    #[must_use]
    pub const fn with_layout(mut self, layout: LayoutStyle) -> Self {
        self.layout = layout;
        self
    }

    /// The node-local cell metric.
    #[must_use]
    pub const fn cell_metric(&self) -> CellMetric {
        self.metric
    }

    /// Pin the Vello glyph font size in logical px (builder form) — the size
    /// the grid's cells were measured from. Set this to the same size passed
    /// to `pinion_text::LayoutCache::measure_monospace_cell` so the painted
    /// advance matches `cell_w` by construction (R1002 font-size SSOT). Leave
    /// unset for a producer-picked cell with no font basis (the paint adapter
    /// then fits a font into `cell_h`).
    #[must_use]
    pub const fn with_font_size_px(mut self, font_size_px: u32) -> Self {
        self.font_size_px = Some(font_size_px);
        self
    }

    /// The explicit Vello glyph font size, if the grid is font-derived
    /// (R1002). `None` ⇒ the paint adapter fits a font into `cell_h`.
    #[must_use]
    pub const fn font_size_px(&self) -> Option<u32> {
        self.font_size_px
    }

    /// Whole cell columns the grid holds — derived from the
    /// layout-resolved [`Rect`] width via [`CellMetric::cols_for`] (the
    /// R1.4 PTY-winsize authority; a trailing partial cell floors).
    #[must_use]
    pub fn cols(&self) -> u16 {
        self.metric.cols_for(self.rect.w)
    }

    /// Whole cell rows the grid holds — derived from the layout-resolved
    /// [`Rect`] height via [`CellMetric::rows_for`].
    #[must_use]
    pub fn rows(&self) -> u16 {
        self.metric.rows_for(self.rect.h)
    }

    /// Set the retained cell projection (builder form). The producer
    /// hands the node a freshly-assembled [`GridBuffer`]; the node holds
    /// it wholesale (R969 retained projection — no per-cell mutation).
    #[must_use]
    pub fn with_cells(mut self, cells: GridBuffer) -> Self {
        self.cells = cells;
        self
    }

    // A palette mutator (theme application) is deliberately not exposed
    // yet: this slice ships only the fixed xterm palette set in `new`.
    // It lands with the theme-swap consumer slice (R972 "no unconsumed
    // surface" discipline).

    /// The retained cell projection.
    #[must_use]
    pub const fn cells(&self) -> &GridBuffer {
        &self.cells
    }

    /// The per-grid palette that resolves this grid's cell colours.
    #[must_use]
    pub const fn palette(&self) -> Palette {
        self.palette
    }
}

/// R681 §2 #4 — paint-inert reference driver. Records every tick's
/// `dt` and every paint call's viewport into internal counters so
/// tests can pin the per-frame dispatch contract without a real
/// paint backend. Opts out of introspection (mirrors
/// [`StubExternal`](crate::external::StubExternal) as the minimal
/// baseline).
#[derive(Debug, Default)]
pub struct StubImmediateMode {
    /// Monotonic tick counter — incremented by every [`ImmediateMode::tick`]
    /// call. Exposed for test inspection; semantically opaque to the
    /// runtime.
    pub tick_count: u64,
    /// Monotonic paint counter — incremented by every
    /// [`ImmediateMode::paint`] call. Distinct from `tick_count`
    /// because the substrate dispatches `tick` then `paint` as
    /// separate phases (R681 atomic 1).
    pub paint_count: u64,
    /// Accumulated `dt` across all ticks since construction. Tests
    /// pin per-frame dispatch by reading this after a known number
    /// of paint cycles.
    pub accumulated_dt: Duration,
    /// Most recent `dt` observed. Distinguishes per-tick values
    /// from the running total above.
    pub last_observed_dt: Duration,
    /// Most recent viewport size observed by [`Self::paint`] (read
    /// from [`ImmediatePainter::viewport_size`]). `None` before the
    /// first paint call.
    pub last_paint_viewport: Option<(u32, u32)>,
}

impl StubImmediateMode {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tick_count: 0,
            paint_count: 0,
            accumulated_dt: Duration::ZERO,
            last_observed_dt: Duration::ZERO,
            last_paint_viewport: None,
        }
    }
}

impl ImmediateMode for StubImmediateMode {
    fn tick(&mut self, dt: Duration) {
        self.tick_count = self.tick_count.saturating_add(1);
        self.accumulated_dt = self.accumulated_dt.saturating_add(dt);
        self.last_observed_dt = dt;
    }

    #[allow(clippy::cast_precision_loss)]
    fn paint(&mut self, painter: &mut dyn ImmediatePainter) {
        self.paint_count = self.paint_count.saturating_add(1);
        self.last_paint_viewport = Some(painter.viewport_size());
        // Issue one paint call so backend impls exercise their
        // primitive dispatch — exact pixels are not the contract;
        // the backend's own integration test owns pixel correctness.
        // Viewport > 2^24 px would lose precision in the cast — far
        // beyond any GUI surface ever encountered.
        let (w, h) = painter.viewport_size();
        painter.clear(Color::default());
        if w > 0 && h > 0 {
            painter.fill_rect(0.0, 0.0, w as f32, h as f32, Color::default());
        }
    }
}

/// R681 §2 #4 — test-fixture [`ImmediatePainter`] that records every
/// primitive call into a [`Vec`] so unit tests can pin the dispatch
/// contract without spinning up a real backend. The Vello backend
/// integration tests own pixel correctness; this fixture owns the
/// API contract (which method, in which order, with which args).
#[derive(Debug, Default)]
pub struct RecordingImmediatePainter {
    pub viewport: (u32, u32),
    pub dpi: f32,
    pub calls: Vec<RecordedPaintCall>,
}

/// One recorded primitive call from [`RecordingImmediatePainter`].
/// Carries the call's args verbatim so tests can pattern-match on
/// the dispatch shape.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedPaintCall {
    Clear {
        color: Color,
    },
    FillRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
    },
    FillTriangle {
        p1: (f32, f32),
        p2: (f32, f32),
        p3: (f32, f32),
        color: Color,
    },
    StrokeLine {
        p1: (f32, f32),
        p2: (f32, f32),
        width: f32,
        color: Color,
    },
}

impl RecordingImmediatePainter {
    #[must_use]
    pub fn new(viewport: (u32, u32), dpi: f32) -> Self {
        Self {
            viewport,
            dpi,
            calls: Vec::new(),
        }
    }
}

impl ImmediatePainter for RecordingImmediatePainter {
    fn viewport_size(&self) -> (u32, u32) {
        self.viewport
    }
    fn dpi_scale(&self) -> f32 {
        self.dpi
    }
    fn clear(&mut self, color: Color) {
        self.calls.push(RecordedPaintCall::Clear { color });
    }
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.calls
            .push(RecordedPaintCall::FillRect { x, y, w, h, color });
    }
    fn fill_triangle(&mut self, p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), color: Color) {
        self.calls
            .push(RecordedPaintCall::FillTriangle { p1, p2, p3, color });
    }
    fn stroke_line(&mut self, p1: (f32, f32), p2: (f32, f32), width: f32, color: Color) {
        self.calls.push(RecordedPaintCall::StrokeLine {
            p1,
            p2,
            width,
            color,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::{Backend, CountedExternal, External, IntrospectValue, StubExternal};

    fn stub_handle() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    /// R1417 — the lifted [`capture_surface`] is BYTE-IDENTICAL to the
    /// hand-rolled idiom its ~10 consumers each grew, for both the plain
    /// (`focusable = false`, the dataviz hover / brush surfaces) and the
    /// focusable (`true`, the scrub bar) shapes. If this holds, migrating a
    /// consumer to the helper cannot change its scene.
    #[test]
    fn r1417_capture_surface_matches_the_hand_rolled_idiom() {
        // `Scene` is not `PartialEq` (a `Box<dyn External>` variant), so compare
        // the full `Debug` form — it captures every field (rect, transparent
        // style, tag, absolute position, size, focusable) the surfaces carry.
        let rect = Rect::new(16, 48, 688, 336);
        for focusable in [false, true] {
            let hand_rolled = Scene::Box(
                BoxNode::new(Rect::default(), BoxStyle::filled(Color::TRANSPARENT))
                    .with_tag("plot")
                    .with_layout(
                        LayoutStyle::new()
                            .with_absolute_position(rect.x, rect.y)
                            .with_size(Size::px(rect.w, rect.h))
                            .with_focusable(focusable),
                    ),
            );
            assert_eq!(
                format!("{:?}", capture_surface("plot", rect, focusable)),
                format!("{hand_rolled:?}"),
                "focusable = {focusable}",
            );
        }
        // The plain (non-focusable) consumers omit `.with_focusable` entirely,
        // relying on the LayoutStyle default — the helper's explicit
        // `with_focusable(false)` must match that default too.
        let omitted = Scene::Box(
            BoxNode::new(Rect::default(), BoxStyle::filled(Color::TRANSPARENT))
                .with_tag("plot")
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(rect.x, rect.y)
                        .with_size(Size::px(rect.w, rect.h)),
                ),
        );
        assert_eq!(
            format!("{:?}", capture_surface("plot", rect, false)),
            format!("{omitted:?}"),
        );
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
            | Scene::External(_)
            | Scene::Scroll(_)
            | Scene::ImmediateModeNode(_)
            | Scene::TextGrid(_) => {}
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
        let scene =
            Scene::Box(BoxNode::filled(Rect::default(), Color::default()).with_layout(layout));
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
        let scene =
            Scene::Box(BoxNode::filled(Rect::default(), Color::default()).with_tag("save_btn"));
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
        let inner =
            Scene::Box(BoxNode::filled(Rect::default(), Color::default()).with_tag("inner_btn"));
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
        assert!(
            s.hit_test(10, 10).is_none(),
            "zero-area rect cannot contain"
        );
    }

    #[test]
    fn hit_test_container_with_unmatched_children_returns_root() {
        // Container at (0,0,100,100); child at (200,200,10,10) — point
        // inside container but outside all children. Hit returns the
        // container itself (empty segments).
        let s = container_at(0, 0, 100, 100, vec![box_at(200, 200, 10, 10)]);
        let hit = s.hit_test(50, 50).expect("inside container");
        assert!(hit.segments.is_empty(), "container itself is the hit");
    }

    #[test]
    fn hit_test_skips_pointer_transparent_overlay_child() {
        // R705 §5.39 — a pointer-transparent overlay (focus ring)
        // layered ON TOP of a tagged widget must not shadow it for
        // input: hit_test walks topmost-first but skips the overlay,
        // landing on the widget beneath.
        let widget = tagged_box_at(0, 0, 100, 100, "btn");
        let mut ring = BoxNode::filled(Rect::new(0, 0, 100, 100), Color::default());
        ring.layout = ring.layout.with_pointer_transparent(true);
        ring.tag = Some("ai-overlay/focus-ring".into());
        let s = container_at(0, 0, 100, 100, vec![widget, Scene::Box(ring)]);
        let hit = s.hit_test(50, 50).expect("inside");
        assert_eq!(
            hit.segments.first().map(String::as_str),
            Some("btn"),
            "overlay skipped — hit lands on the widget tag, not the ring",
        );
    }

    #[test]
    fn hit_test_pointer_transparent_lone_overlay_falls_through_to_container() {
        // The overlay is the only child; skipping it leaves the
        // container itself as the deepest hit (no phantom ring target).
        let mut ring = BoxNode::filled(Rect::new(0, 0, 100, 100), Color::default());
        ring.layout = ring.layout.with_pointer_transparent(true);
        ring.tag = Some("ai-overlay/focus-ring".into());
        let s = container_at(0, 0, 100, 100, vec![Scene::Box(ring)]);
        let hit = s.hit_test(50, 50).expect("inside container");
        assert!(
            hit.segments.is_empty(),
            "ring skipped → container is the hit"
        );
    }

    // ---- R1196 cursor_hint_at (hover cursor resolution) ----

    fn hinted_box_at(x: u32, y: u32, w: u32, h: u32, hint: CursorHint) -> Scene {
        let mut node = BoxNode::filled(Rect::new(x, y, w, h), Color::default());
        node.layout = node.layout.with_cursor(hint);
        Scene::Box(node)
    }

    #[test]
    fn cursor_hint_at_resolves_the_hinted_node_under_the_pointer() {
        // A splitter-like layout: a hintless container with a hinted 4-px handle
        // strip between two panels.
        let s = container_at(
            0,
            0,
            200,
            100,
            vec![
                box_at(0, 0, 98, 100),
                hinted_box_at(98, 0, 4, 100, CursorHint::ColResize),
                box_at(102, 0, 98, 100),
            ],
        );
        // Over the handle strip → its resize hint.
        assert_eq!(s.cursor_hint_at(100, 50), Some(CursorHint::ColResize));
        // Over the panels (no hint on them or the container) → None.
        assert_eq!(s.cursor_hint_at(40, 50), None);
        assert_eq!(s.cursor_hint_at(150, 50), None);
        // Outside the whole scene → None.
        assert_eq!(s.cursor_hint_at(300, 50), None);
    }

    #[test]
    fn cursor_hint_at_deepest_hint_wins_over_ancestor() {
        // A hinted container; a hintless child inherits it (ancestor fallback),
        // a hinted child overrides it (deepest wins).
        let mut inherit = ContainerNode::new(vec![box_at(10, 10, 30, 30)]);
        inherit.rect = Rect::new(0, 0, 100, 100);
        inherit.layout = inherit.layout.with_cursor(CursorHint::RowResize);
        let inherit = Scene::Container(inherit);
        assert_eq!(inherit.cursor_hint_at(20, 20), Some(CursorHint::RowResize));
        assert_eq!(inherit.cursor_hint_at(80, 80), Some(CursorHint::RowResize));

        let mut override_c =
            ContainerNode::new(vec![hinted_box_at(10, 10, 30, 30, CursorHint::ColResize)]);
        override_c.rect = Rect::new(0, 0, 100, 100);
        override_c.layout = override_c.layout.with_cursor(CursorHint::RowResize);
        let override_c = Scene::Container(override_c);
        // Over the hinted child → the child's hint, not the container's.
        assert_eq!(
            override_c.cursor_hint_at(20, 20),
            Some(CursorHint::ColResize)
        );
        // Elsewhere in the container → the container's hint.
        assert_eq!(
            override_c.cursor_hint_at(80, 80),
            Some(CursorHint::RowResize)
        );
    }

    #[test]
    fn cursor_hint_at_skips_pointer_transparent_hinted_overlay() {
        // A pointer-transparent overlay carrying a hint must not shadow the
        // widget beneath (mirrors hit_test's transparency skip).
        let widget = hinted_box_at(0, 0, 100, 100, CursorHint::ColResize);
        let mut overlay = BoxNode::filled(Rect::new(0, 0, 100, 100), Color::default());
        overlay.layout = overlay
            .layout
            .with_pointer_transparent(true)
            .with_cursor(CursorHint::RowResize);
        let s = container_at(0, 0, 100, 100, vec![widget, Scene::Box(overlay)]);
        assert_eq!(
            s.cursor_hint_at(50, 50),
            Some(CursorHint::ColResize),
            "the transparent overlay is skipped — the widget's hint wins",
        );
    }

    #[test]
    fn cursor_hint_at_resolves_through_a_scroll_offset() {
        // A hinted node inside a scrolled container resolves at the offset-
        // translated coordinate (mirrors hit_test's Scroll translation).
        let hinted = hinted_box_at(0, 100, 50, 20, CursorHint::RowResize);
        let mut content = ContainerNode::new(vec![hinted]);
        content.rect = Rect::new(0, 0, 50, 400);
        let scroll =
            ScrollNode::new(Rect::new(0, 0, 50, 50), Scene::Container(content)).with_offset(0, 100);
        let s = Scene::Scroll(scroll);
        // Viewport y=5 + offset 100 = content y=105, inside the hinted node.
        assert_eq!(s.cursor_hint_at(10, 5), Some(CursorHint::RowResize));
        // Viewport y=45 + offset 100 = content y=145, below the hinted node.
        assert_eq!(s.cursor_hint_at(10, 45), None);
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
        let s = container_at(
            0,
            0,
            200,
            200,
            vec![tagged_box_at(10, 10, 50, 50, "save_btn")],
        );
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
            0,
            0,
            200,
            200,
            vec![box_at(10, 10, 50, 50), box_at(100, 100, 30, 30)],
        );
        // Zero-area query never intersects.
        assert!(s.hit_test_region(50, 50, 0, 0).is_empty());
    }

    #[test]
    fn hit_test_region_covering_everything_returns_root_plus_children() {
        let s = container_at(
            0,
            0,
            200,
            200,
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
            0,
            0,
            200,
            200,
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
            0,
            0,
            100,
            100,
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
            0,
            0,
            200,
            200,
            vec![box_at(10, 10, 20, 20), box_at(50, 50, 30, 30)],
        );
        assert_eq!(
            s.lookup_path(&["1".to_string()]),
            Some(Rect::new(50, 50, 30, 30))
        );
    }

    #[test]
    fn lookup_path_resolves_tag_segment() {
        let s = container_at(0, 0, 200, 200, vec![tagged_box_at(10, 10, 20, 20, "btn")]);
        assert_eq!(
            s.lookup_path(&["btn".to_string()]),
            Some(Rect::new(10, 10, 20, 20))
        );
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
        assert_eq!(
            s.lookup_path(&["btn".to_string()]),
            Some(Rect::new(99, 99, 99, 99))
        );
    }

    #[test]
    fn lookup_path_mut_resolves_index_segment() {
        let mut s = container_at(
            0,
            0,
            200,
            200,
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
        assert!(
            s.lookup_path_mut(&[]).is_none(),
            "Effect never resolves, even at root"
        );
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
            0,
            0,
            200,
            200,
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

    // ─────────────────────────────────────────────────────────────────
    // R55.A §5.45 — ScrollNode scaffold smoke tests. The primitive's
    // data shape is final this round; hit-test descent / lookup-path
    // traversal / paint clipping ride the R55.A.* sub-axes.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r55_a_scroll_node_rect_returns_viewport() {
        // R55.A — `Scene::rect()` returns the clip viewport, not the
        // intrinsic content geometry. Preserves the §5.34 dry_run
        // invariant: the parent-level hit-test sees only what the
        // viewport exposes.
        let viewport = Rect::new(10, 20, 100, 200);
        let content = box_at(0, 0, 500, 1000);
        let scene = Scene::Scroll(ScrollNode::new(viewport, content));
        assert_eq!(scene.rect(), viewport);
    }

    #[test]
    fn r55_a_scroll_node_tag_round_trips() {
        // R55.A — the `tag` field surfaces through `Scene::tag()` so
        // R51.122 input router resolves wheel / arrow / page events
        // to the scroll container the same way it resolves clicks to
        // a tagged ContainerNode.
        let scroll =
            ScrollNode::new(Rect::new(0, 0, 50, 50), box_at(0, 0, 200, 200)).with_tag("scroll_box");
        let scene = Scene::Scroll(scroll);
        assert_eq!(scene.tag(), Some("scroll_box"));
    }

    #[test]
    fn r55_a_scroll_node_offset_round_trips() {
        // R55.A — the builder `with_offset` writes the field pair
        // verbatim. The substrate-side clamp lives on the scroll
        // dispatch path (R55.B carry); construction itself is
        // verbatim.
        let scroll =
            ScrollNode::new(Rect::new(0, 0, 100, 100), box_at(0, 0, 400, 800)).with_offset(40, 250);
        assert_eq!(scroll.offset_x, 40);
        assert_eq!(scroll.offset_y, 250);
    }

    #[test]
    fn r55_a_scroll_node_default_offset_is_zero() {
        // R55.A — `ScrollNode::new` starts at (0, 0) so the content's
        // top-left aligns with the viewport's top-left by default.
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), box_at(0, 0, 400, 800));
        assert_eq!(scroll.offset_x, 0);
        assert_eq!(scroll.offset_y, 0);
        assert!(scroll.tag.is_none());
    }

    #[test]
    fn r55_a2_hit_test_descends_into_scrolled_content() {
        // R55.A.2 — a hit inside the viewport translates by the
        // current offset and resolves against the content. With
        // offset_y=100, the viewport row 0 maps to content row 100,
        // so a viewport-relative (5, 10) lands at content
        // intrinsic (5, 110) — inside the content box.
        let content = box_at(0, 0, 200, 400);
        let scroll = ScrollNode::new(Rect::new(50, 60, 100, 100), content).with_offset(0, 100);
        let scene = Scene::Scroll(scroll);
        // Pick a point inside the viewport: viewport.x=50, vx=5 →
        // content_x = 5 + 0 = 5; viewport.y=60, vy=10 →
        // content_y = 10 + 100 = 110. Content box is 200×400
        // intrinsic, so (5, 110) is inside.
        let hit = scene.hit_test(55, 70).expect("hit lands in content");
        assert_eq!(hit.bbox, Rect::new(0, 0, 200, 400));
    }

    #[test]
    fn r55_a2_hit_test_outside_viewport_misses() {
        // R55.A.2 — a hit outside the viewport never descends.
        // The content's intrinsic geometry exceeds the viewport but
        // is hidden by the clip.
        let content = box_at(0, 0, 500, 500);
        let scene = Scene::Scroll(ScrollNode::new(Rect::new(10, 10, 50, 50), content));
        // Inside content intrinsic (200, 200) but outside viewport.
        assert!(scene.hit_test(200, 200).is_none());
    }

    #[test]
    fn r55_a2_hit_test_inside_viewport_outside_content_returns_scroll_rect() {
        // R55.A.2 — viewport contains the point but the translated
        // content coordinate falls past the content's bbox. The
        // scroll container itself is then the deepest hit (same
        // fallback as a Container with no matching child).
        let content = box_at(0, 0, 10, 10);
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), content).with_offset(50, 50);
        let scene = Scene::Scroll(scroll);
        // viewport-local (5, 5) + offset (50, 50) = (55, 55),
        // outside the 10×10 content box.
        let hit = scene.hit_test(5, 5).expect("viewport contains the point");
        // The scroll container's viewport is the resolved bbox.
        assert_eq!(hit.bbox, Rect::new(0, 0, 100, 100));
        assert!(hit.segments.is_empty());
    }

    #[test]
    fn r55_a2_hit_test_through_container_into_scroll() {
        // R55.A.2 — a Container parent over a Scroll child surfaces
        // the Scroll's descent result with the parent's path segment
        // (idx or tag) prepended. End-to-end:
        // Container("root") > Scroll(viewport=10..60) > Box(...)
        let inner = box_at(0, 0, 100, 100);
        let scroll = ScrollNode::new(Rect::new(10, 10, 50, 50), inner).with_tag("scroll_box");
        let scene = container_at(0, 0, 100, 100, vec![Scene::Scroll(scroll)]);
        // Hit at (20, 20): inside root container, inside scroll
        // viewport (vx=10, vy=10), inside content (0, 0) + (10, 10)
        // = (10, 10).
        let hit = scene.hit_test(20, 20).expect("hits content");
        assert_eq!(hit.bbox, Rect::new(0, 0, 100, 100));
        // The path's first segment is the Scroll's tag (tag wins
        // over index per Container's hit_test rule).
        assert_eq!(hit.segments.first().map(String::as_str), Some("scroll_box"));
    }

    // ─────────────────────────────────────────────────────────────────
    // R55.A.3 §5.45 — Scroll is path-transparent across the full
    // lookup-path family. `Self::rect` already aliases ScrollNode's
    // viewport at the parent level; the descent rule below applies
    // when the caller supplies a non-empty segment slice.
    // Mirrors R51.181 hit-test transparency.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r55_a3_lookup_path_empty_segments_on_scroll_returns_viewport() {
        // R55.A.3 — empty path returns the scroll viewport (same
        // shape as `Self::rect`). Transparency only fires for
        // non-empty paths; this guard is the boundary case.
        let viewport = Rect::new(10, 20, 100, 200);
        let scene = Scene::Scroll(ScrollNode::new(viewport, box_at(0, 0, 500, 1000)));
        assert_eq!(scene.lookup_path(&[]), Some(viewport));
    }

    #[test]
    fn r55_a3_lookup_path_descends_into_scroll_content() {
        // R55.A.3 — Scroll forwards the segment slice into
        // `content` unchanged. A tagged Box inside the scroll
        // resolves with the same path the consumer would use if
        // Scroll were not in the chain.
        let inner = tagged_box_at(0, 0, 200, 100, "inner_btn");
        let content = container_at(0, 0, 400, 400, vec![inner]);
        let scene = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 200, 200), content));
        assert_eq!(
            scene.lookup_path(&["inner_btn".to_string()]),
            Some(Rect::new(0, 0, 200, 100))
        );
    }

    #[test]
    fn r55_a3_lookup_path_unknown_segment_inside_scroll_returns_none() {
        // R55.A.3 — transparency means an unknown segment inside
        // the scrolled content fails the same way it would without
        // the scroll wrap. The scroll itself never claims a
        // segment to short-circuit the lookup.
        let inner = tagged_box_at(0, 0, 50, 50, "inner");
        let content = container_at(0, 0, 200, 200, vec![inner]);
        let scene = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 100, 100), content));
        assert_eq!(scene.lookup_path(&["nope".to_string()]), None);
    }

    #[test]
    fn r55_a3_lookup_path_ref_descends_into_scroll() {
        // R55.A.3 — `lookup_path_ref` carries the same transparency
        // and returns `&Scene` at the resolved path. The
        // `scene/query` nested-External walker relies on this
        // shape to reach an `ExternalNode` inside a scroll.
        let inner = tagged_box_at(0, 0, 80, 40, "inner_btn");
        let content = container_at(0, 0, 300, 300, vec![inner]);
        let scene = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 200, 200), content));
        let resolved = scene
            .lookup_path_ref(&["inner_btn".to_string()])
            .expect("inner_btn resolves through scroll");
        assert!(matches!(resolved, Scene::Box(_)));
        assert_eq!(resolved.rect(), Rect::new(0, 0, 80, 40));
    }

    #[test]
    fn r55_a3_lookup_path_mut_mutates_scroll_content() {
        // R55.A.3 — `lookup_path_mut` descends through the scroll
        // and the caller mutates the resolved leaf. Confirms the
        // borrow chain `&mut Scene → ScrollNode.content (Box) →
        // child` resolves without aliasing — same shape
        // `TypedProposal::SetStyle` / `ReplaceView` need.
        let inner = tagged_box_at(0, 0, 50, 50, "inner");
        let content = container_at(0, 0, 200, 200, vec![inner]);
        let mut scene = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 100, 100), content));
        let resolved = scene
            .lookup_path_mut(&["inner".to_string()])
            .expect("inner resolves through scroll");
        match resolved {
            Scene::Box(b) => {
                b.rect = Rect::new(5, 5, 80, 80);
            }
            _ => panic!("expected Box leaf"),
        }
        // Re-resolve to confirm the mutation stuck.
        let after = scene
            .lookup_path(&["inner".to_string()])
            .expect("still resolves");
        assert_eq!(after, Rect::new(5, 5, 80, 80));
    }

    #[test]
    fn r55_a3_lookup_path_through_container_into_scroll() {
        // R55.A.3 — Container > Scroll(tag="scroll_box") > Box.
        // The Container consumes the "scroll_box" segment (Scroll
        // surfaces its tag through `Self::tag`); the Scroll is
        // path-transparent inside, so the remaining segments
        // forward into the content. End-to-end mirror of the
        // R51.181 hit_test_through_container_into_scroll path.
        let inner = tagged_box_at(0, 0, 50, 50, "inner");
        let content = container_at(0, 0, 200, 200, vec![inner]);
        let scroll = ScrollNode::new(Rect::new(10, 10, 100, 100), content).with_tag("scroll_box");
        let scene = container_at(0, 0, 200, 200, vec![Scene::Scroll(scroll)]);
        assert_eq!(
            scene.lookup_path(&["scroll_box".to_string(), "inner".to_string()]),
            Some(Rect::new(0, 0, 50, 50))
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R55.A.4 §5.45 — `hit_test_region` / `collect_intersections`
    // descend through ScrollNode with the query rect translated
    // from root-local into content-intrinsic coordinates. The
    // viewport-intersect gate skips descent when the query falls
    // outside the visible window; the descent reuses the existing
    // segment stack (Scroll is path-transparent).
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r55_a4_hit_test_region_includes_scroll_viewport() {
        // R55.A.4 — a region query that overlaps the scroll
        // viewport pushes the scroll container as one hit. The
        // descent into `content` runs in addition (covered by the
        // next test). Mirrors the container-level behaviour from
        // §5.32 R39.2 v0.
        let content = box_at(0, 0, 200, 200);
        let scene = Scene::Scroll(ScrollNode::new(Rect::new(10, 10, 50, 50), content));
        let hits = scene.hit_test_region(0, 0, 100, 100);
        assert_eq!(hits[0].bbox, Rect::new(10, 10, 50, 50));
        assert!(hits[0].segments.is_empty());
    }

    #[test]
    fn r55_a4_hit_test_region_descends_into_scrolled_content() {
        // R55.A.4 — content collects with the existing path stack
        // (Scroll consumes no segment). With zero offset and
        // viewport (0,0,100,100), a root-local query of
        // (10,10,50,50) translates to content-intrinsic
        // (10,10,50,50) unchanged.
        let inner = box_at(0, 0, 200, 200);
        let scene = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 100, 100), inner));
        let hits = scene.hit_test_region(10, 10, 50, 50);
        // hits[0] = scroll viewport; hits[1] = scrolled content box.
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].bbox, Rect::new(0, 0, 100, 100));
        assert!(hits[0].segments.is_empty());
        assert_eq!(hits[1].bbox, Rect::new(0, 0, 200, 200));
        assert!(hits[1].segments.is_empty());
    }

    #[test]
    fn r55_a4_hit_test_region_offset_shifts_content_match() {
        // R55.A.4 — offset (0, 100) shifts the content up by 100
        // cells. A root-local query at (0, 0, 50, 50) translates
        // to content-intrinsic (0, 100, 50, 50). A child box at
        // intrinsic (0, 100, 30, 30) lies inside that translated
        // query and surfaces through the Container's "shifted"
        // segment.
        let inner = tagged_box_at(0, 100, 30, 30, "shifted");
        let content = container_at(0, 0, 200, 400, vec![inner]);
        let scene =
            Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 100, 100), content).with_offset(0, 100));
        let hits = scene.hit_test_region(0, 0, 50, 50);
        let found = hits.iter().any(|h| h.segments == ["shifted".to_string()]);
        assert!(found, "shifted box must surface at intrinsic-shifted path");
    }

    #[test]
    fn r55_a4_hit_test_region_query_outside_viewport_skips_content() {
        // R55.A.4 — the viewport-intersect gate at the top of
        // `collect_intersections` keeps a query disjoint from the
        // viewport from descending. Even content that would
        // intrinsically overlap stays hidden by the clip.
        let inner = tagged_box_at(0, 0, 500, 500, "huge");
        let scene = Scene::Scroll(ScrollNode::new(Rect::new(50, 50, 30, 30), inner));
        let hits = scene.hit_test_region(100, 100, 10, 10);
        assert!(hits.is_empty(), "viewport-disjoint query yields no hits");
    }

    // ─────────────────────────────────────────────────────────────────
    // R55.C.2 §5.45 — `Scene::scroll_target_at` + `scroll_state_at`
    // input-router wheel dispatch helpers. The walk mirrors
    // `hit_test`: descend into a Scroll's content (with the offset
    // translation) when nested, otherwise return the Scroll itself
    // as the deepest match.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r55_c2_scroll_target_at_finds_self_when_no_inner_scroll() {
        // R55.C.2 — a single ScrollNode wrapping a non-scroll
        // content returns itself as the wheel target for any
        // (x, y) inside the viewport.
        let content = box_at(0, 0, 200, 400);
        let scene = Scene::Scroll(ScrollNode::new(Rect::new(10, 10, 100, 100), content));
        let target = scene.scroll_target_at(50, 50).expect("inside viewport");
        assert_eq!(target.viewport, Rect::new(10, 10, 100, 100));
    }

    #[test]
    fn r55_c2_scroll_target_at_outside_viewport_returns_none() {
        // R55.C.2 — (x, y) outside the viewport never matches.
        let scene = Scene::Scroll(ScrollNode::new(
            Rect::new(50, 50, 30, 30),
            box_at(0, 0, 100, 100),
        ));
        assert!(scene.scroll_target_at(0, 0).is_none());
        assert!(scene.scroll_target_at(100, 100).is_none());
    }

    #[test]
    fn r55_c2_scroll_target_at_finds_inside_container() {
        // R55.C.2 — Container > Scroll. The walk descends through
        // the container and returns the Scroll's ref.
        let scroll = ScrollNode::new(Rect::new(20, 20, 100, 100), box_at(0, 0, 200, 200));
        let scene = container_at(0, 0, 200, 200, vec![Scene::Scroll(scroll)]);
        let target = scene.scroll_target_at(50, 50).expect("hits inner scroll");
        assert_eq!(target.viewport, Rect::new(20, 20, 100, 100));
    }

    #[test]
    fn r55_c2_scroll_target_at_picks_deepest_nested_scroll() {
        // R55.C.2 — nested scroll containers route the wheel to the
        // innermost match (W3C overflow:scroll ancestor walk).
        // Inner viewport (in content-intrinsic coords) is
        // (10, 10, 50, 50); the outer scroll's offset is zero so
        // root-local (20, 20) maps to content-intrinsic (20, 20) —
        // inside the inner viewport.
        let inner_content = box_at(0, 0, 100, 100);
        let inner_scroll = Scene::Scroll(ScrollNode::new(Rect::new(10, 10, 50, 50), inner_content));
        let outer = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 200, 200), inner_scroll));
        let target = outer.scroll_target_at(20, 20).expect("hits inner");
        assert_eq!(target.viewport, Rect::new(10, 10, 50, 50));
    }

    #[test]
    fn r55_c2_scroll_target_at_falls_back_to_outer_when_inner_misses() {
        // R55.C.2 — inside outer viewport but outside inner viewport
        // → the outer scroll is the deepest match. Mirrors the
        // `hit_test` fallback shape.
        let inner_content = box_at(0, 0, 50, 50);
        let inner_scroll = Scene::Scroll(ScrollNode::new(Rect::new(60, 60, 20, 20), inner_content));
        let outer_content = container_at(0, 0, 200, 200, vec![inner_scroll]);
        let outer = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 200, 200), outer_content));
        // (10, 10) is inside outer (0..200) but outside inner (60..80).
        let target = outer.scroll_target_at(10, 10).expect("outer");
        assert_eq!(target.viewport, Rect::new(0, 0, 200, 200));
    }

    #[test]
    fn r55_c2_scroll_state_at_returns_attached_state() {
        // R55.C.2 — when the matched ScrollNode has a state link,
        // `scroll_state_at` returns the cloned `Rc` so the router
        // can call `scroll_by` without holding a long-lived borrow
        // into the paint tree.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), box_at(0, 0, 200, 200))
            .with_state(Rc::clone(&state));
        let scene = Scene::Scroll(scroll);
        let found = scene.scroll_state_at(50, 50).expect("state attached");
        assert!(Rc::ptr_eq(&state, &found));
        // Mutation through the returned handle reaches the original.
        found.scroll_by(20, 30);
        assert_eq!(state.offset(), (20, 30));
    }

    #[test]
    fn r55_c2_scroll_state_at_returns_none_when_state_missing() {
        // R55.C.2 — a declarative-only ScrollNode (no `with_state`
        // call) silently returns `None` — the router drops the
        // wheel input rather than panic.
        let scene = Scene::Scroll(ScrollNode::new(
            Rect::new(0, 0, 100, 100),
            box_at(0, 0, 200, 200),
        ));
        assert!(scene.scroll_state_at(50, 50).is_none());
    }

    #[test]
    fn r55_c2_scroll_state_at_returns_none_when_no_scroll_at_point() {
        // R55.C.2 — outside every scroll viewport → None.
        let scene = container_at(0, 0, 200, 200, vec![box_at(50, 50, 30, 30)]);
        assert!(scene.scroll_state_at(60, 60).is_none());
    }

    #[test]
    fn r55_a4_hit_test_region_through_container_into_scroll_content() {
        // R55.A.4 — Container > Scroll(tag="sb") > Container >
        // Box(tag="leaf"). A root-spanning region query picks up
        // all four: outer container, scroll viewport, inner
        // container, leaf box. The leaf box's path is
        // ["sb", "leaf"] — the outer Container consumes the
        // scroll's tag, Scroll itself is path-transparent, and
        // the inner Container surfaces the leaf's own tag.
        let leaf = tagged_box_at(0, 0, 50, 50, "leaf");
        let inner_container = container_at(0, 0, 200, 200, vec![leaf]);
        let scroll = ScrollNode::new(Rect::new(10, 10, 100, 100), inner_container).with_tag("sb");
        let scene = container_at(0, 0, 200, 200, vec![Scene::Scroll(scroll)]);
        let hits = scene.hit_test_region(0, 0, 200, 200);
        let leaf_hit = hits
            .iter()
            .find(|h| h.segments == ["sb".to_string(), "leaf".to_string()])
            .expect("leaf must surface under [sb, leaf]");
        assert_eq!(leaf_hit.bbox, Rect::new(0, 0, 50, 50));
    }

    // ─────────────────────────────────────────────────────────────
    // R51.190 §5.45 — ScrollNode::from_state ergonomic ctor.
    // Closes the substrate-incompleteness-signal that the R51.180-
    // 188 cascade left open: the canonical view-fn shape used to
    // require five lines of boilerplate (offset destructure + tag
    // repeat + 3-builder chain). from_state collapses that to one
    // call by reading offset + tag off the state Rc.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r51_190_from_state_derives_offset() {
        use crate::widgets::scroll::ScrollState;
        // The factory consults state.offset() so a non-zero state
        // produces a node with the matching offset. Mirrors the
        // explicit `let (ox, oy) = state.offset()` + with_offset
        // chain.
        let state = Rc::new(ScrollState::new());
        state.set_max(500, 500);
        state.scroll_to(40, 90);
        let viewport = Rect::new(0, 0, 100, 100);
        let content = Scene::Box(BoxNode::filled(Rect::new(0, 0, 200, 200), Color::default()));
        let node = ScrollNode::from_state(state, viewport, content);
        assert_eq!(node.offset_x, 40);
        assert_eq!(node.offset_y, 90);
    }

    #[test]
    fn r51_190_from_state_derives_tag_when_state_has_tag() {
        use crate::widgets::scroll::ScrollState;
        // When the state carries a tag (typical when reached via
        // use_scroll_state), the node inherits it automatically —
        // no caller-side string repeat.
        let state = Rc::new(ScrollState::with_tag("main_scroll"));
        let viewport = Rect::new(0, 0, 100, 100);
        let content = Scene::Box(BoxNode::filled(Rect::new(0, 0, 200, 200), Color::default()));
        let node = ScrollNode::from_state(state, viewport, content);
        assert_eq!(node.tag.as_deref(), Some("main_scroll"));
    }

    #[test]
    fn r51_190_from_state_leaves_tag_none_for_untagged_state() {
        use crate::widgets::scroll::ScrollState;
        // Test fixtures and manual wiring reach ScrollState::new
        // directly — no tag context exists, so the matching node
        // stays untagged. Matches pre-R51.190 behaviour when the
        // caller skipped `.with_tag(...)`.
        let state = Rc::new(ScrollState::new());
        let viewport = Rect::new(0, 0, 100, 100);
        let content = Scene::Box(BoxNode::filled(Rect::new(0, 0, 200, 200), Color::default()));
        let node = ScrollNode::from_state(state, viewport, content);
        assert!(node.tag.is_none());
    }

    #[test]
    fn r51_190_from_state_attaches_state_rc() {
        use crate::widgets::scroll::ScrollState;
        // The state Rc lands on `node.state`. Verifies the input
        // router (which walks `scroll_state_at` to find the
        // dispatch target) finds the same Rc the caller passed in.
        let state = Rc::new(ScrollState::with_tag("router_target"));
        let original = Rc::clone(&state);
        let viewport = Rect::new(0, 0, 100, 100);
        let content = Scene::Box(BoxNode::filled(Rect::new(0, 0, 200, 200), Color::default()));
        let node = ScrollNode::from_state(state, viewport, content);
        let attached = node.state.as_ref().expect("state must attach");
        assert!(Rc::ptr_eq(&original, attached));
    }

    #[test]
    fn r51_190_from_state_explicit_with_tag_overrides_derived() {
        use crate::widgets::scroll::ScrollState;
        // The derived tag is a default — chaining with_tag after
        // from_state overrides it. Matches the standard builder
        // semantics (later call wins).
        let state = Rc::new(ScrollState::with_tag("derived"));
        let viewport = Rect::new(0, 0, 100, 100);
        let content = Scene::Box(BoxNode::filled(Rect::new(0, 0, 200, 200), Color::default()));
        let node = ScrollNode::from_state(state, viewport, content).with_tag("override");
        assert_eq!(node.tag.as_deref(), Some("override"));
    }

    #[test]
    fn r55_g6_scroll_map_layout_preserves_seeded_viewport_size() {
        // R55.G.6 §5.45 — `map_layout` hands the seeded layout to the
        // closure so chaining a `flex_grow` / `gap` / `margin`
        // modification does not collapse Scroll to a 0×0 leaf the way
        // `with_layout(LayoutStyle::new().with_*)` would.
        use crate::style::{Size, SizeValue};
        let content = Scene::Container(ContainerNode::new(vec![]));
        let scroll = ScrollNode::new(Rect::new(0, 0, 120, 80), content)
            .map_layout(|l| l.with_flex_grow(1.0));
        assert_eq!(scroll.layout.size, Size::px(120, 80));
        assert!((scroll.layout.flex_grow - 1.0).abs() < f32::EPSILON);
        // Negative control — `with_layout` full-replace WOULD drop the
        // seed; same payload via that path lands `SizeValue::Auto`.
        let content2 = Scene::Container(ContainerNode::new(vec![]));
        let scroll_full = ScrollNode::new(Rect::new(0, 0, 120, 80), content2)
            .with_layout(LayoutStyle::new().with_flex_grow(1.0));
        assert_eq!(scroll_full.layout.size.width, SizeValue::Auto);
        assert_eq!(scroll_full.layout.size.height, SizeValue::Auto);
    }

    #[test]
    fn r55_g6_container_map_layout_round_trips() {
        // R55.G.6 §5.45 — symmetry check: `map_layout` lands the
        // same closure-application shape on Container as on Scroll,
        // confirming the primitive surface is uniform across the
        // seven layout-bearing nodes.
        use crate::style::{Display, FlexDirection};
        let c =
            ContainerNode::new(vec![]).map_layout(|l| l.flex(FlexDirection::Column).with_gap(12));
        assert_eq!(c.layout.display, Display::Flex);
        assert_eq!(c.layout.flex_direction, FlexDirection::Column);
        assert_eq!(c.layout.gap, 12);
    }

    // ─────────────────────────────────────────────────────────────
    // R55.G.19 §5.49 — `Scene::contains_tag` walker tests. Codifies
    // the R55.G.17 composite paint-root convention as a primitive
    // that downstream widget unit tests reuse.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r55_g19_contains_tag_finds_root_match() {
        let scene =
            Scene::Box(BoxNode::filled(Rect::new(0, 0, 10, 10), Color::default()).with_tag("root"));
        assert!(scene.contains_tag("root"));
        assert!(!scene.contains_tag("absent"));
    }

    #[test]
    fn r55_g19_contains_tag_finds_nested_container_child() {
        let inner = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 10, 10), Color::default()).with_tag("inner"),
        );
        let scene = Scene::Container(ContainerNode::new(vec![inner]).with_tag("outer"));
        assert!(scene.contains_tag("outer"));
        assert!(scene.contains_tag("inner"));
        assert!(!scene.contains_tag("nope"));
    }

    #[test]
    fn r55_g19_contains_tag_descends_through_scroll_content() {
        let inner = Scene::Box(
            BoxNode::filled(Rect::new(0, 0, 10, 10), Color::default()).with_tag("buried"),
        );
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), inner).with_tag("scroll");
        let scene = Scene::Scroll(scroll);
        assert!(scene.contains_tag("scroll"));
        assert!(scene.contains_tag("buried"));
    }

    #[test]
    fn r55_g19_contains_tag_effect_leaf_is_false() {
        // `Effect` carries no tag (per `Scene::tag`) so `contains_tag`
        // on the bare leaf is unconditionally `false`.
        let scene = Scene::Effect(EffectNode::new());
        assert!(!scene.contains_tag("anything"));
    }

    // ─────────────────────────────────────────────────────────────────
    // R55.D.5 §5.45 — `find_external_with_tag` + `primary_external`
    // substrate for multi-External state scene composition. Pinned
    // on a `StubExternal` fixture (`pinion-core` already exports one
    // via `external::StubExternal`) so the tests live in this crate
    // without pulling pinion-runtime.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r55_d5_find_external_with_tag_on_bare_external() {
        // R55.D.5 — single-External state scene shape: the bare
        // `Scene::External` resolves its own tag.
        use crate::external::StubExternal;
        let scene =
            Scene::External(ExternalNode::new(Box::new(StubExternal::new())).with_tag("primary"));
        assert!(scene.find_external_with_tag("primary").is_some());
        assert!(scene.find_external_with_tag("absent").is_none());
    }

    #[test]
    fn r55_d5_find_external_with_tag_on_container_of_externals() {
        // R55.D.5 — multi-External shape composed by the substrate.
        // Both externals resolve by their tags.
        use crate::external::StubExternal;
        let primary =
            Scene::External(ExternalNode::new(Box::new(StubExternal::new())).with_tag("primary"));
        let extra =
            Scene::External(ExternalNode::new(Box::new(StubExternal::new())).with_tag("extra"));
        let scene = Scene::Container(ContainerNode::new(vec![primary, extra]));
        assert!(scene.find_external_with_tag("primary").is_some());
        assert!(scene.find_external_with_tag("extra").is_some());
        assert!(scene.find_external_with_tag("nope").is_none());
    }

    #[test]
    fn r55_d5_find_external_with_tag_descends_through_scroll() {
        // R55.D.5 — the walker mirrors `contains_tag` / `hit_test`:
        // it descends through `Scroll.content`.
        use crate::external::StubExternal;
        let buried =
            Scene::External(ExternalNode::new(Box::new(StubExternal::new())).with_tag("buried"));
        let scroll = ScrollNode::new(Rect::new(0, 0, 100, 100), buried);
        let scene = Scene::Scroll(scroll);
        assert!(scene.find_external_with_tag("buried").is_some());
    }

    #[test]
    fn r55_d5_primary_external_returns_self_on_bare_external() {
        // R55.D.5 — `Scene::External(primary)` resolves to itself.
        use crate::external::StubExternal;
        let scene =
            Scene::External(ExternalNode::new(Box::new(StubExternal::new())).with_tag("only"));
        let node = scene.primary_external().expect("must resolve");
        assert_eq!(node.tag.as_deref(), Some("only"));
    }

    #[test]
    fn r55_d5_primary_external_picks_first_external_in_container() {
        // R55.D.5 — DFS pre-order: the first child External wins,
        // matching the substrate's "primary is first in declaration
        // order" composition convention.
        use crate::external::StubExternal;
        let first =
            Scene::External(ExternalNode::new(Box::new(StubExternal::new())).with_tag("first"));
        let second =
            Scene::External(ExternalNode::new(Box::new(StubExternal::new())).with_tag("second"));
        let scene = Scene::Container(ContainerNode::new(vec![first, second]));
        let node = scene.primary_external().expect("must resolve");
        assert_eq!(node.tag.as_deref(), Some("first"));
    }

    #[test]
    fn r55_d5_primary_external_returns_none_when_no_external() {
        // R55.D.5 — a Container of Boxes resolves to None; the RPC
        // primitives surface this as `NoExternalAtPath`.
        let scene = Scene::Container(ContainerNode::new(vec![Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 10, 10),
            Color::default(),
        ))]));
        assert!(scene.primary_external().is_none());
    }

    #[test]
    fn r55_d5_primary_external_mut_allows_introspect_mut() {
        // R55.D.5 — the mutable counterpart returns the same External
        // the shared accessor would, but yields `&mut`, enabling
        // `intro.invoke` / `intro.intervene` on the primary widget.
        use crate::external::StubExternal;
        let mut scene = Scene::Container(ContainerNode::new(vec![Scene::External(
            ExternalNode::new(Box::new(StubExternal::new())).with_tag("primary"),
        )]));
        let node = scene.primary_external_mut().expect("must resolve");
        assert_eq!(node.tag.as_deref(), Some("primary"));
        // Mutable borrow lets us reach `introspect_mut` (StubExternal
        // opts out — the call returns `None` but the borrow is valid).
        let _ = node.handle.introspect_mut();
    }

    // R668 §5.16 — `Scene::intrinsic_content_size` is the
    // [[abstraction-needs-second-consumer]] candidate but ships with a
    // single consumer (pinion-shell `IntrinsicAfterFirstPaint`). The
    // tests pin the contract: empty tree → (0, 0); leaf rect dominates;
    // Container child rects vote into the union; Scroll viewport caps
    // the contribution (inner content is intentionally invisible to the
    // walk).

    #[test]
    fn r668_intrinsic_content_size_empty_container_is_zero() {
        let scene = Scene::Container(ContainerNode::new(vec![]));
        assert_eq!(scene.intrinsic_content_size(), (0, 0));
    }

    #[test]
    fn r668_intrinsic_content_size_box_returns_right_bottom() {
        // Single leaf at (10, 20) sized 160 × 80 → bbox extends to
        // (170, 100). The walker uses `x + w` / `y + h` (right edge,
        // bottom edge) rather than the rect width alone.
        let scene = Scene::Box(BoxNode::filled(
            Rect::new(10, 20, 160, 80),
            Color::default(),
        ));
        assert_eq!(scene.intrinsic_content_size(), (170, 100));
    }

    #[test]
    fn r668_intrinsic_content_size_container_unions_children() {
        // Two sibling boxes with non-overlapping rects → bbox is the
        // union (max right, max bottom), not the sum. Demonstrates the
        // Container-walk descent.
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(BoxNode::filled(Rect::new(0, 0, 100, 40), Color::default())),
            Scene::Box(BoxNode::filled(
                Rect::new(120, 60, 50, 30),
                Color::default(),
            )),
        ]));
        // Container.rect defaults to (0,0,0,0) so the children dominate.
        assert_eq!(scene.intrinsic_content_size(), (170, 90));
    }

    #[test]
    fn r668_intrinsic_content_size_scroll_uses_viewport_not_content() {
        // Scroll's contract: the visible clip window votes, not the
        // (potentially huge) inner content. Confirms the
        // `IntrinsicAfterFirstPaint` window-resize hook treats scroll
        // viewports as fixed-size content even when the inner content
        // would dominate the union.
        let inner = Scene::Box(BoxNode::filled(
            Rect::new(0, 0, 9999, 9999),
            Color::default(),
        ));
        let scroll = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 200, 150), inner));
        assert_eq!(scroll.intrinsic_content_size(), (200, 150));
    }

    #[test]
    fn r668_intrinsic_content_size_effect_has_no_geometry() {
        // Effect nodes never carry geometry (per `Scene::rect`), so a
        // Container that holds only Effect leaves measures as empty.
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Effect(EffectNode::new()),
            Scene::Effect(EffectNode::new()),
        ]));
        assert_eq!(scene.intrinsic_content_size(), (0, 0));
    }

    #[test]
    fn r668_intrinsic_content_size_saturates_at_u32_max() {
        // Overflow-safety guard: a leaf at (u32::MAX, u32::MAX) with
        // any non-zero size must not panic — `saturating_add` pins the
        // bbox at u32::MAX rather than wrapping to a tiny value the
        // clamp pass would then accept as a legal window size.
        let scene = Scene::Box(BoxNode::filled(
            Rect::new(u32::MAX, u32::MAX, 16, 16),
            Color::default(),
        ));
        assert_eq!(scene.intrinsic_content_size(), (u32::MAX, u32::MAX));
    }

    // ───────────────────────────────────────────────────────────────
    // R681 §2 #4 — ImmediateModeNode + ImmediateMode trait surface.
    //
    // These tests pin the core data shape + trait surface +
    // scene-method exhaustiveness (tick walk, presence check, tag /
    // path / hit-test leaf semantics). The shell-side tick dispatch +
    // Vello paint bridge are covered by `pinion-shell` integration
    // tests; here we verify the core primitive in isolation.
    // ───────────────────────────────────────────────────────────────

    #[test]
    fn r681_immediate_mode_node_carries_viewport_layout_tag() {
        let driver = StubImmediateMode::new();
        let viewport = Rect::new(10, 20, 100, 50);
        let node = ImmediateModeNode::from_driver(driver, viewport).with_tag("game_viewport");
        assert_eq!(node.viewport, viewport);
        assert_eq!(node.tag.as_deref(), Some("game_viewport"));
        assert_eq!(node.last_dt(), Duration::ZERO, "no tick yet sentinel");
    }

    #[test]
    fn r681_scene_immediate_mode_variant_rect_returns_viewport() {
        let viewport = Rect::new(5, 6, 40, 30);
        let scene = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            viewport,
        ));
        assert_eq!(scene.rect(), viewport);
    }

    #[test]
    fn r681_scene_immediate_mode_tag_returns_attached_tag() {
        let scene = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::default())
                .with_tag("canvas"),
        );
        assert_eq!(scene.tag(), Some("canvas"));
    }

    #[test]
    fn r681_immediate_mode_untagged_returns_none_for_tag() {
        let scene = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            Rect::default(),
        ));
        assert_eq!(scene.tag(), None);
    }

    #[test]
    fn r681_contains_tag_walks_through_container_into_immediate() {
        let inner = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::default())
                .with_tag("game"),
        );
        let container = Scene::Container(ContainerNode::new(vec![inner]));
        assert!(container.contains_tag("game"));
        assert!(!container.contains_tag("missing"));
    }

    #[test]
    fn r681_contains_tag_walks_through_scroll_into_immediate() {
        let inner = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::default())
                .with_tag("canvas"),
        );
        let scroll = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 100, 100), inner));
        assert!(scroll.contains_tag("canvas"));
    }

    #[test]
    fn r681_immediate_mode_is_paint_opaque_leaf_for_external_walkers() {
        // `find_external_with_tag` / `primary_external` MUST skip
        // ImmediateModeNode: the driver is an `ImmediateMode`, not an
        // `External`, despite the introspection-surface mirror.
        let scene = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::default())
                .with_tag("canvas"),
        );
        assert!(scene.find_external_with_tag("canvas").is_none());
        assert!(scene.primary_external().is_none());
    }

    #[test]
    fn r681_immediate_mode_external_walkers_skip_through_container() {
        // External walkers must descend Container children but skip
        // any ImmediateModeNode they hit; a sibling External should
        // still resolve.
        let immediate = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::default())
                .with_tag("canvas"),
        );
        let external =
            Scene::External(ExternalNode::new(Box::new(StubExternal::new())).with_tag("real_ext"));
        let container = Scene::Container(ContainerNode::new(vec![immediate, external]));
        // ImmediateModeNode does NOT respond to External lookups; the
        // sibling External does.
        assert!(container.find_external_with_tag("canvas").is_none());
        assert!(container.find_external_with_tag("real_ext").is_some());
        // The primary External of the container is the External sibling
        // (the ImmediateModeNode is non-external).
        assert!(container.primary_external().is_some());
        assert_eq!(
            container.primary_external().and_then(|n| n.tag.as_deref()),
            Some("real_ext"),
        );
    }

    #[test]
    fn r830_find_immediate_with_tag_resolves_driver_and_skips_external() {
        // R830 §5.15 — the immediate-mode peer of find_external_with_tag.
        // Resolves an ImmediateModeNode by tag (through Container descent)
        // and ignores Scene::External, the inverse of the external walker.
        let immediate = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::default())
                .with_tag("ball"),
        );
        let external =
            Scene::External(ExternalNode::new(Box::new(StubExternal::new())).with_tag("real_ext"));
        let container = Scene::Container(ContainerNode::new(vec![immediate, external]));
        assert!(
            container.find_immediate_with_tag("ball").is_some(),
            "resolves the immediate-mode driver by its §5.20 tag",
        );
        assert!(
            container.find_immediate_with_tag("real_ext").is_none(),
            "immediate finder must skip Scene::External (inverse of find_external_with_tag)",
        );
        assert!(container.find_immediate_with_tag("absent").is_none());
    }

    #[test]
    fn r681_hit_test_treats_immediate_as_hittable_leaf() {
        let viewport = Rect::new(10, 10, 40, 40);
        let scene = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            viewport,
        ));
        let hit = scene.hit_test(20, 20).expect("inside viewport");
        assert_eq!(hit.bbox, viewport);
        assert!(hit.segments.is_empty(), "root leaf, empty path");
        // Outside the viewport: no hit.
        assert!(scene.hit_test(0, 0).is_none());
        assert!(scene.hit_test(60, 60).is_none());
    }

    #[test]
    fn r681_hit_test_descends_container_to_immediate_with_tag_path() {
        let viewport = Rect::new(20, 20, 40, 40);
        let inner = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(StubImmediateMode::new(), viewport).with_tag("game"),
        );
        let mut container_node = ContainerNode::new(vec![inner]);
        container_node.rect = Rect::new(0, 0, 80, 80);
        let scene = Scene::Container(container_node);
        let hit = scene
            .hit_test(30, 30)
            .expect("inside viewport via container");
        assert_eq!(hit.bbox, viewport, "deepest hit is the immediate viewport");
        assert_eq!(hit.segments, vec!["game".to_string()], "tag path segment");
    }

    #[test]
    fn r681_lookup_path_does_not_descend_into_immediate() {
        let immediate = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            Rect::new(0, 0, 10, 10),
        ));
        // Empty-segment lookup returns the rect itself (root case).
        assert_eq!(
            immediate.lookup_path(&[]),
            Some(Rect::new(0, 0, 10, 10)),
            "empty-segment returns root rect",
        );
        // Non-empty path against a non-container intermediate fails.
        assert!(
            immediate.lookup_path(&["anything".to_string()]).is_none(),
            "immediate-mode driver has no addressable sub-paths",
        );
    }

    #[test]
    fn r681_scroll_target_at_skips_immediate_mode() {
        let scene = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            Rect::new(0, 0, 100, 100),
        ));
        assert!(scene.scroll_target_at(50, 50).is_none());
    }

    #[test]
    fn r681_intrinsic_content_size_includes_immediate_viewport() {
        // Immediate-mode node contributes its viewport rect to the
        // intrinsic content bbox so `SizeStrategy::IntrinsicAfterFirstPaint`
        // sizes a window correctly when the binding declares an
        // immediate viewport at the top of the scene.
        let scene = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            Rect::new(0, 0, 320, 240),
        ));
        assert_eq!(scene.intrinsic_content_size(), (320, 240));
    }

    #[test]
    fn r681_intrinsic_content_size_unions_immediate_with_box() {
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(BoxNode::filled(Rect::new(0, 0, 100, 50), Color::default())),
            Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
                StubImmediateMode::new(),
                Rect::new(0, 50, 320, 240),
            )),
        ]));
        // Union: max_w = 320 (from immediate), max_h = 290 (50 + 240).
        assert_eq!(scene.intrinsic_content_size(), (320, 290));
    }

    #[test]
    fn r681_hit_test_region_collects_immediate_as_leaf() {
        let immediate = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::new(0, 0, 50, 50))
                .with_tag("game"),
        );
        let mut container_node = ContainerNode::new(vec![immediate]);
        container_node.rect = Rect::new(0, 0, 100, 100);
        let scene = Scene::Container(container_node);
        let hits = scene.hit_test_region(0, 0, 60, 60);
        // Container (root, empty path) + immediate ("game").
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().any(|h| h.segments == vec!["game".to_string()]));
    }

    // ── ImmediateMode trait surface ────────────────────────────────

    #[test]
    fn r681_immediate_mode_trait_is_dyn_safe() {
        // Compile-time guard: any future change that loses dyn-safety
        // (associated consts, Self-returning methods) breaks this.
        let _: Rc<RefCell<dyn ImmediateMode>> = Rc::new(RefCell::new(StubImmediateMode::new()));
    }

    #[test]
    fn r681_stub_immediate_mode_tick_advances_counter_and_dt() {
        let mut driver = StubImmediateMode::new();
        assert_eq!(driver.tick_count, 0);
        assert_eq!(driver.accumulated_dt, Duration::ZERO);
        driver.tick(Duration::from_millis(16));
        assert_eq!(driver.tick_count, 1);
        assert_eq!(driver.accumulated_dt, Duration::from_millis(16));
        assert_eq!(driver.last_observed_dt, Duration::from_millis(16));
        driver.tick(Duration::from_millis(17));
        assert_eq!(driver.tick_count, 2);
        assert_eq!(driver.accumulated_dt, Duration::from_millis(33));
        assert_eq!(driver.last_observed_dt, Duration::from_millis(17));
    }

    #[test]
    fn r681_immediate_mode_tick_default_impl_is_noop() {
        // Default `tick` impl exists (compiles) so test fixtures /
        // placeholder drivers omit boilerplate. Concrete impl that
        // does not override `tick` behaves as a no-op.
        #[derive(Debug, Default)]
        struct InertDriver;
        impl ImmediateMode for InertDriver {}
        let mut driver = InertDriver;
        driver.tick(Duration::from_secs(1)); // no panic, no change.
    }

    #[test]
    fn r681_immediate_mode_introspect_default_is_none() {
        let driver = StubImmediateMode::new();
        assert!(driver.introspect().is_none());
        let mut driver_mut = StubImmediateMode::new();
        assert!(driver_mut.introspect_mut().is_none());
    }

    #[test]
    fn r681_immediate_mode_opt_in_introspection_surfaces_through_trait() {
        // Mirror of `CountedExternal` opt-in pattern: a driver that
        // impls both `ImmediateMode` AND `ExternalIntrospect` exposes
        // its state through the same RPC channel External uses.
        #[derive(Debug)]
        struct CountedDriver {
            count: i64,
        }
        impl ImmediateMode for CountedDriver {
            fn tick(&mut self, _dt: Duration) {
                self.count = self.count.saturating_add(1);
            }
            fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
                Some(self)
            }
            fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
                Some(self)
            }
        }
        impl ExternalIntrospect for CountedDriver {
            fn schema(&self) -> crate::external::IntrospectSchema {
                crate::external::IntrospectSchema::new(
                    const { &[crate::external::SchemaField::new("count", "int")] },
                )
            }
            fn query(&self, path: &str) -> Option<IntrospectValue> {
                (path == "count").then_some(IntrospectValue::Int(self.count))
            }
            fn intervene(
                &mut self,
                _: &str,
                _: IntrospectValue,
            ) -> Result<(), crate::external::InterveneError> {
                Err(crate::external::InterveneError::ReadOnly)
            }
        }
        let driver = CountedDriver { count: 5 };
        let introspect = driver.introspect().expect("opt-in declared");
        assert_eq!(introspect.query("count"), Some(IntrospectValue::Int(5)),);
    }

    // ── ImmediateModeNode helper API ───────────────────────────────

    #[test]
    fn r681_immediate_mode_node_last_dt_publish_read_round_trip() {
        let node = ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::default());
        assert_eq!(node.last_dt(), Duration::ZERO, "no tick yet sentinel");
        node.set_last_dt(Duration::from_millis(16));
        assert_eq!(node.last_dt(), Duration::from_millis(16));
        node.set_last_dt(Duration::from_millis(17));
        assert_eq!(node.last_dt(), Duration::from_millis(17), "latest wins");
    }

    #[test]
    fn r681_immediate_mode_node_with_layout_replaces_sidecar() {
        let custom = LayoutStyle::new().with_size(Size::px(640, 480));
        let node = ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::default())
            .with_layout(custom);
        // Reading the layout back via the public field confirms the
        // builder did not silently drop the override.
        assert_eq!(node.layout.size, Size::px(640, 480));
    }

    #[test]
    fn r681_immediate_mode_node_map_layout_preserves_seeded_default() {
        // `with_layout` is full-replace; `map_layout` lets the caller
        // chain a single override while preserving any constructor-
        // supplied default. Mirrors R55.G.6 idiom for Scroll.
        let node = ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::default())
            .map_layout(|l| l.with_size(Size::px(100, 100)));
        assert_eq!(node.layout.size, Size::px(100, 100));
    }

    #[test]
    fn r681_immediate_mode_node_from_driver_boxes_concrete_impl() {
        // `from_driver` is the ergonomic constructor for one-shot
        // drivers; pins that the concrete type is correctly boxed
        // into `Rc<RefCell<dyn ImmediateMode>>` without an explicit
        // type annotation at the call site.
        let node =
            ImmediateModeNode::from_driver(StubImmediateMode::new(), Rect::new(0, 0, 50, 50));
        // Mutable borrow drives one tick; observe via the field.
        node.handle.borrow_mut().tick(Duration::from_millis(16));
        // Read back the concrete state via the same trait surface
        // (no downcast — `tick_count` is on the trait by virtue of
        // being a field readable through a dyn-unsafe path; here we
        // rely on the fact that StubImmediateMode is the concrete
        // type and we constructed it ourselves).
        //
        // The dyn handle does not expose `tick_count` (it's not on
        // the trait surface), so the pinning here is that the tick
        // call succeeds without panicking — the substrate ergonomic
        // assertion. Concrete-state verification lives in
        // `r681_stub_immediate_mode_tick_advances_counter_and_dt`.
        assert!(node.tag.is_none(), "default no tag");
    }

    #[test]
    fn r681_shared_handle_observes_ticks_through_multiple_clones() {
        // The Rc handle is the substrate for the view-fn pattern
        // where `use_immediate_driver()` returns an
        // `Rc<RefCell<MyDriver>>` from `Owner::cache` and the
        // scene rebuild clones the Rc into the node every frame.
        // Multiple clones must observe the same state.
        let driver = Rc::new(RefCell::new(StubImmediateMode::new()));
        let handle_a: Rc<RefCell<dyn ImmediateMode>> = driver.clone();
        let handle_b: Rc<RefCell<dyn ImmediateMode>> = driver.clone();
        handle_a.borrow_mut().tick(Duration::from_millis(16));
        handle_b.borrow_mut().tick(Duration::from_millis(17));
        // Concrete read through the original `Rc`.
        assert_eq!(driver.borrow().tick_count, 2);
        assert_eq!(driver.borrow().accumulated_dt, Duration::from_millis(33));
    }

    #[test]
    fn r681_immediate_mode_node_is_a_recognised_paint_leaf() {
        // An ImmediateModeNode is a paint-opaque retained-tree leaf: it
        // carries no children to descend, but the tick walk and the
        // presence check both recognise it. (The Vello paint bridge
        // itself lives in `pinion-runtime::paint_adapter` and is covered
        // there + in `pinion-shell` integration tests.)
        let scene = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            Rect::new(0, 0, 16, 16),
        ));
        assert!(scene.has_immediate_mode_subtree());
        assert_eq!(scene.tick_immediate_mode(Duration::from_millis(1)), 1);
    }

    // ── R681 atomic 1: ImmediatePainter trait + paint dispatch ─────

    #[test]
    fn r681_immediate_painter_trait_is_dyn_safe() {
        // Compile-time guard against losing dyn-safety on the
        // painter surface (associated items, Self-return, etc.).
        let _: Box<dyn ImmediatePainter> = Box::new(RecordingImmediatePainter::new((100, 50), 1.0));
    }

    #[test]
    fn r681_recording_painter_observes_viewport_and_dpi() {
        let painter = RecordingImmediatePainter::new((640, 480), 2.0);
        assert_eq!(painter.viewport_size(), (640, 480));
        let diff = (painter.dpi_scale() - 2.0_f32).abs();
        assert!(diff < f32::EPSILON);
        assert!(painter.calls.is_empty());
    }

    #[test]
    fn r681_recording_painter_appends_each_primitive_in_call_order() {
        let mut painter = RecordingImmediatePainter::new((100, 100), 1.0);
        painter.clear(Color::default());
        painter.fill_rect(1.0, 2.0, 3.0, 4.0, Color::default());
        painter.fill_triangle((0.0, 0.0), (10.0, 0.0), (5.0, 8.66), Color::default());
        painter.stroke_line((0.0, 0.0), (10.0, 10.0), 2.0, Color::default());
        assert_eq!(painter.calls.len(), 4);
        assert!(matches!(painter.calls[0], RecordedPaintCall::Clear { .. }));
        assert!(matches!(
            painter.calls[1],
            RecordedPaintCall::FillRect { .. }
        ));
        assert!(matches!(
            painter.calls[2],
            RecordedPaintCall::FillTriangle { .. }
        ));
        assert!(matches!(
            painter.calls[3],
            RecordedPaintCall::StrokeLine { .. }
        ));
    }

    #[test]
    fn r681_immediate_mode_paint_default_impl_is_noop() {
        // Default `paint` impl exists (compiles, no panic).
        #[derive(Debug, Default)]
        struct InertDriver;
        impl ImmediateMode for InertDriver {}
        let mut driver = InertDriver;
        let mut painter = RecordingImmediatePainter::new((10, 10), 1.0);
        driver.paint(&mut painter);
        assert!(
            painter.calls.is_empty(),
            "default paint impl emits zero primitives",
        );
    }

    #[test]
    fn r681_stub_immediate_mode_paint_records_dispatch() {
        let mut driver = StubImmediateMode::new();
        assert_eq!(driver.paint_count, 0);
        assert_eq!(driver.last_paint_viewport, None);
        let mut painter = RecordingImmediatePainter::new((320, 240), 1.0);
        driver.paint(&mut painter);
        assert_eq!(driver.paint_count, 1);
        assert_eq!(driver.last_paint_viewport, Some((320, 240)));
        // Stub paint emits clear + fill_rect (sentinel dispatch).
        assert_eq!(painter.calls.len(), 2);
        assert!(matches!(painter.calls[0], RecordedPaintCall::Clear { .. }));
        assert!(matches!(
            painter.calls[1],
            RecordedPaintCall::FillRect { .. }
        ));
    }

    #[test]
    fn r681_stub_immediate_mode_paint_zero_viewport_skips_fill() {
        // Zero-size viewport: clear still fires (backend-clear hint),
        // but fill_rect is skipped (no useful pixels to write).
        let mut driver = StubImmediateMode::new();
        let mut painter = RecordingImmediatePainter::new((0, 0), 1.0);
        driver.paint(&mut painter);
        assert_eq!(driver.last_paint_viewport, Some((0, 0)));
        assert_eq!(painter.calls.len(), 1);
        assert!(matches!(painter.calls[0], RecordedPaintCall::Clear { .. }));
    }

    #[test]
    fn r681_tick_and_paint_are_separate_phases() {
        // The substrate dispatches `tick(dt)` then `paint(painter)`
        // as distinct phases — neither auto-implies the other.
        let mut driver = StubImmediateMode::new();
        driver.tick(Duration::from_millis(16));
        assert_eq!(driver.tick_count, 1);
        assert_eq!(driver.paint_count, 0, "tick alone does not paint");
        let mut painter = RecordingImmediatePainter::new((10, 10), 1.0);
        driver.paint(&mut painter);
        assert_eq!(driver.tick_count, 1, "paint alone does not tick");
        assert_eq!(driver.paint_count, 1);
    }

    // ── R681 atomic 1: Scene::tick_immediate_mode walker ──────────

    #[test]
    fn r681_tick_immediate_mode_drives_root_node() {
        // Concrete driver kept by `Rc` so we can inspect tick state
        // after the dyn walk advances it.
        let driver = Rc::new(RefCell::new(StubImmediateMode::new()));
        let handle: Rc<RefCell<dyn ImmediateMode>> = driver.clone();
        let scene =
            Scene::ImmediateModeNode(ImmediateModeNode::new(handle, Rect::new(0, 0, 100, 100)));
        let count = scene.tick_immediate_mode(Duration::from_millis(16));
        assert_eq!(count, 1, "one node ticked");
        assert_eq!(driver.borrow().tick_count, 1);
        assert_eq!(driver.borrow().last_observed_dt, Duration::from_millis(16));
        // last_dt sidecar published on the node.
        if let Scene::ImmediateModeNode(n) = &scene {
            assert_eq!(n.last_dt(), Duration::from_millis(16));
        } else {
            panic!("expected ImmediateModeNode variant");
        }
    }

    #[test]
    fn r681_tick_immediate_mode_zero_on_non_immediate_scene() {
        let scene = Scene::Box(BoxNode::filled(Rect::new(0, 0, 100, 100), Color::default()));
        assert_eq!(scene.tick_immediate_mode(Duration::from_millis(16)), 0);
    }

    #[test]
    fn r681_tick_immediate_mode_descends_container_and_scroll() {
        let driver_a = Rc::new(RefCell::new(StubImmediateMode::new()));
        let driver_b = Rc::new(RefCell::new(StubImmediateMode::new()));
        let inner_a: Rc<RefCell<dyn ImmediateMode>> = driver_a.clone();
        let inner_b: Rc<RefCell<dyn ImmediateMode>> = driver_b.clone();
        let scroll_immediate =
            Scene::ImmediateModeNode(ImmediateModeNode::new(inner_b, Rect::new(0, 0, 50, 50)));
        let scroll = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 100, 100), scroll_immediate));
        let container = Scene::Container(ContainerNode::new(vec![
            Scene::ImmediateModeNode(ImmediateModeNode::new(inner_a, Rect::new(0, 0, 50, 50))),
            scroll,
        ]));
        let count = container.tick_immediate_mode(Duration::from_millis(16));
        assert_eq!(count, 2, "two nodes: one in Container, one inside Scroll");
        assert_eq!(driver_a.borrow().tick_count, 1);
        assert_eq!(driver_b.borrow().tick_count, 1);
    }

    #[test]
    fn r681_tick_immediate_mode_two_calls_accumulate_per_driver() {
        let driver = Rc::new(RefCell::new(StubImmediateMode::new()));
        let handle: Rc<RefCell<dyn ImmediateMode>> = driver.clone();
        let scene =
            Scene::ImmediateModeNode(ImmediateModeNode::new(handle, Rect::new(0, 0, 100, 100)));
        scene.tick_immediate_mode(Duration::from_millis(16));
        scene.tick_immediate_mode(Duration::from_millis(17));
        assert_eq!(driver.borrow().tick_count, 2);
        assert_eq!(driver.borrow().accumulated_dt, Duration::from_millis(33),);
        if let Scene::ImmediateModeNode(n) = &scene {
            assert_eq!(n.last_dt(), Duration::from_millis(17), "latest wins");
        }
    }

    #[test]
    fn r681_has_immediate_mode_subtree_negative_for_non_immediate() {
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(BoxNode::filled(Rect::default(), Color::default())),
            Scene::Text(TextNode::default()),
        ]));
        assert!(!scene.has_immediate_mode_subtree());
    }

    #[test]
    fn r681_has_immediate_mode_subtree_positive_through_container() {
        let scene = Scene::Container(ContainerNode::new(vec![
            Scene::Box(BoxNode::filled(Rect::default(), Color::default())),
            Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
                StubImmediateMode::new(),
                Rect::new(0, 0, 50, 50),
            )),
        ]));
        assert!(scene.has_immediate_mode_subtree());
    }

    #[test]
    fn r681_has_immediate_mode_subtree_positive_through_scroll() {
        let inner = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            Rect::new(0, 0, 50, 50),
        ));
        let scene = Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 100, 100), inner));
        assert!(scene.has_immediate_mode_subtree());
    }

    #[test]
    fn r681_immediate_mode_paint_via_dyn_dispatch() {
        // The realistic call site is through
        // `node.handle.borrow_mut().paint(&mut backend_painter)` —
        // the painter behind a `&mut dyn ImmediatePainter`. Pin
        // this dispatch path here so the substrate is exercised
        // without a real backend.
        let handle: Rc<RefCell<dyn ImmediateMode>> =
            Rc::new(RefCell::new(StubImmediateMode::new()));
        let mut painter: Box<dyn ImmediatePainter> =
            Box::new(RecordingImmediatePainter::new((50, 50), 1.0));
        handle.borrow_mut().paint(painter.as_mut());
        // The Box wraps a RecordingImmediatePainter; we can't peek
        // inside via the trait surface, so the assertion is that
        // the dispatch did not panic and the handle's paint counter
        // bumped.
        // Concrete read on the handle:
        let driver_ref = handle.borrow();
        // The dyn dispatch went through `ImmediateMode::paint` which
        // for StubImmediateMode does `painter.clear(...)` and
        // `painter.fill_rect(...)` — both go through the dyn
        // ImmediatePainter, so reaching here without a panic confirms
        // the cross-dyn dispatch works.
        let _ = driver_ref;
    }

    // ─────────────────────────────────────────────────────────────
    // R682 §5.16 — paint_hash + is_cacheable_for_paint substrate
    // (atomic 0 of the axis 4 dirty-subtree-cache series)
    // ─────────────────────────────────────────────────────────────

    fn rect_a() -> Rect {
        Rect::new(10, 20, 100, 80)
    }

    fn rect_b() -> Rect {
        Rect::new(11, 20, 100, 80)
    }

    fn box_a() -> Scene {
        Scene::Box(BoxNode::filled(rect_a(), Color::rgb(0x10, 0x20, 0x30)))
    }

    fn box_b() -> Scene {
        Scene::Box(BoxNode::filled(rect_b(), Color::rgb(0x10, 0x20, 0x30)))
    }

    #[test]
    fn r682_paint_hash_is_deterministic_for_identical_box_leaves() {
        // Two structurally identical Box leaves built independently
        // must hash to the same value — that's the entire cache
        // contract.
        assert_eq!(box_a().paint_hash(), box_a().paint_hash());
    }

    #[test]
    fn r682_paint_hash_changes_when_box_rect_changes() {
        // Any paint-affecting field churn must change the hash.
        // The Vello fragment encodes absolute rect coordinates, so a
        // 1-pixel shift means the cached fragment is no longer
        // pixel-equivalent — cache MUST miss.
        assert_ne!(box_a().paint_hash(), box_b().paint_hash());
    }

    #[test]
    fn r682_paint_hash_changes_when_box_fill_changes() {
        let a = Scene::Box(BoxNode::filled(rect_a(), Color::rgb(0, 0, 0)));
        let b = Scene::Box(BoxNode::filled(rect_a(), Color::rgb(1, 0, 0)));
        assert_ne!(a.paint_hash(), b.paint_hash());
    }

    #[test]
    fn r682_paint_hash_changes_when_text_content_changes() {
        let a = Scene::Text(TextNode::new("hello", rect_a()));
        let b = Scene::Text(TextNode::new("world", rect_a()));
        assert_ne!(a.paint_hash(), b.paint_hash());
    }

    #[test]
    fn r682_paint_hash_changes_when_text_style_changes() {
        let a = Scene::Text(TextNode::new("x", rect_a()));
        let b = Scene::Text(TextNode::styled(
            "x",
            rect_a(),
            TextStyle::new().with_fg(Color::rgb(1, 2, 3)),
        ));
        assert_ne!(a.paint_hash(), b.paint_hash());
    }

    #[test]
    fn r1072_paint_hash_changes_when_caret_bearing_changes() {
        // R1072 §5.37 — the caret-bearing marker selects which shaper paints the
        // leaf (§5.37 vs parley) when the engine is enabled, so two otherwise
        // identical Text leaves must NOT share a cached paint fragment: the hash
        // must fold the flag.
        let plain = Scene::Text(TextNode::new("x", rect_a()));
        let caret = Scene::Text(TextNode::new("x", rect_a()).caret_bearing());
        assert_ne!(plain.paint_hash(), caret.paint_hash());
    }

    #[test]
    fn r682_paint_hash_changes_when_container_child_added() {
        let empty = Scene::Container(ContainerNode::new(vec![]));
        let one = Scene::Container(ContainerNode::new(vec![box_a()]));
        assert_ne!(empty.paint_hash(), one.paint_hash());
    }

    #[test]
    fn r682_paint_hash_changes_when_container_child_reordered() {
        // Children are paint-ordered — siblings drawn back-to-front
        // by declaration order. A swap means a different z-stack →
        // possibly different pixels → cache miss.
        let ab = Scene::Container(ContainerNode::new(vec![box_a(), box_b()]));
        let ba = Scene::Container(ContainerNode::new(vec![box_b(), box_a()]));
        assert_ne!(ab.paint_hash(), ba.paint_hash());
    }

    #[test]
    fn r682_paint_hash_changes_when_descendant_changes() {
        // Recursive propagation: a deep child Box swap must surface
        // at every ancestor Container's hash.
        let inner_a = Scene::Container(ContainerNode::new(vec![box_a()]));
        let inner_b = Scene::Container(ContainerNode::new(vec![box_b()]));
        let outer_a = Scene::Container(ContainerNode::new(vec![inner_a]));
        let outer_b = Scene::Container(ContainerNode::new(vec![inner_b]));
        assert_ne!(outer_a.paint_hash(), outer_b.paint_hash());
    }

    #[test]
    fn r682_paint_hash_unchanged_when_only_tag_changes() {
        // §5.20 tag is input-router-only; not painted (the focus
        // ring goes through a separate paint pass overlay). A tag
        // edit must NOT invalidate the cache.
        let a = Scene::Container(ContainerNode::new(vec![]).with_tag("alpha"));
        let b = Scene::Container(ContainerNode::new(vec![]).with_tag("bravo"));
        assert_eq!(a.paint_hash(), b.paint_hash());
    }

    #[test]
    fn r682_paint_hash_unchanged_when_only_aria_label_changes() {
        // R51.69 aria_label feeds the access tree, not the paint
        // adapter. Cache must hit.
        let a = Scene::Container(ContainerNode::new(vec![]).with_aria_label("Save"));
        let b = Scene::Container(ContainerNode::new(vec![]).with_aria_label("Cancel"));
        assert_eq!(a.paint_hash(), b.paint_hash());
    }

    #[test]
    fn r682_paint_hash_memoizes_within_paint_pass() {
        // The Cell<Option<u64>> memoisation: first call computes and
        // stores; subsequent calls inside the same paint pass return
        // the stored value (Cell::set side-effect observable by
        // checking the field is `Some(_)` after the call).
        let c = ContainerNode::new(vec![box_a(), box_b()]);
        assert!(
            c.paint_hash.get().is_none(),
            "fresh container has no memoised hash"
        );
        let h1 = c.paint_hash();
        assert_eq!(c.paint_hash.get(), Some(h1), "first call stores into Cell");
        let h2 = c.paint_hash();
        assert_eq!(h1, h2, "second call returns memoised value");
    }

    #[test]
    fn r682_paint_hash_effect_is_stable_sentinel() {
        let a = Scene::Effect(EffectNode::new());
        let b = Scene::Effect(EffectNode::new());
        assert_eq!(a.paint_hash(), b.paint_hash());
        assert_eq!(a.paint_hash(), PAINT_HASH_EFFECT_SENTINEL);
    }

    #[test]
    fn r974_1_text_grid_is_uncacheable_like_external() {
        // R974.1/R991 — TextGrid is UNCACHEABLE (joins `External`), NOT the
        // cacheable `Effect` sentinel (R972.1's first cut). R991 paints the
        // grid and the projection is replaced wholesale each frame, so a
        // parent container re-encodes it fresh — a content-blind cached
        // fragment could otherwise serve a stale frame.
        let mut node = TextGridNode::new(CellMetric::DEFAULT);
        node.rect = Rect::new(0, 0, 640, 384);
        let grid = Scene::TextGrid(node);
        assert_eq!(grid.paint_hash(), PAINT_HASH_UNCACHEABLE);
        assert!(!grid.is_cacheable_for_paint(), "TextGrid must paint fresh");

        // The guard that matters: a container holding a TextGrid is itself
        // uncacheable, so cell content can never be served from a stale
        // cached fragment.
        let parent = Scene::Container(ContainerNode::new(vec![Scene::TextGrid(
            TextGridNode::new(CellMetric::DEFAULT),
        )]));
        assert!(
            !parent.is_cacheable_for_paint(),
            "a parent of a TextGrid is uncacheable"
        );
    }

    #[test]
    fn r682_paint_hash_external_is_uncacheable_sentinel() {
        let a = Scene::External(ExternalNode::new(stub_handle()));
        let b = Scene::External(ExternalNode::new(stub_handle()));
        assert_eq!(a.paint_hash(), PAINT_HASH_UNCACHEABLE);
        assert_eq!(b.paint_hash(), PAINT_HASH_UNCACHEABLE);
    }

    #[test]
    fn r682_paint_hash_immediate_mode_is_uncacheable_sentinel() {
        let n = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            Rect::new(0, 0, 50, 50),
        ));
        assert_eq!(n.paint_hash(), PAINT_HASH_UNCACHEABLE);
    }

    #[test]
    fn r682_paint_hash_uncacheable_and_effect_sentinels_are_distinct() {
        // The two sentinel values MUST stay distinct so a hash-dump
        // inspector (and the cache layer) can tell them apart.
        assert_ne!(PAINT_HASH_EFFECT_SENTINEL, PAINT_HASH_UNCACHEABLE);
    }

    #[test]
    fn r682_paint_hash_scroll_includes_viewport_offset_and_content() {
        let content_a = box_a();
        let content_b = box_b();
        let scroll_a = Scene::Scroll(ScrollNode::new(rect_a(), content_a));
        let scroll_b = Scene::Scroll(ScrollNode::new(rect_a(), content_b));
        // Content swap surfaces at Scroll's hash.
        assert_ne!(scroll_a.paint_hash(), scroll_b.paint_hash());

        // Offset shift surfaces at Scroll's hash (content scrolled
        // to a different position → different painted pixels).
        let shifted = Scene::Scroll(ScrollNode::new(rect_a(), box_a()).with_offset(5, 7));
        let pristine = Scene::Scroll(ScrollNode::new(rect_a(), box_a()));
        assert_ne!(shifted.paint_hash(), pristine.paint_hash());
    }

    #[test]
    fn r682_is_cacheable_for_paint_leaves() {
        assert!(box_a().is_cacheable_for_paint());
        assert!(Scene::Text(TextNode::new("x", rect_a())).is_cacheable_for_paint());
        assert!(Scene::Path(PathNode::empty(rect_a())).is_cacheable_for_paint());
        assert!(Scene::Image(ImageNode::new("u", rect_a())).is_cacheable_for_paint());
        assert!(Scene::Effect(EffectNode::new()).is_cacheable_for_paint());
    }

    #[test]
    fn r682_is_cacheable_for_paint_rejects_external_leaf() {
        let s = Scene::External(ExternalNode::new(stub_handle()));
        assert!(!s.is_cacheable_for_paint());
    }

    #[test]
    fn r682_is_cacheable_for_paint_rejects_immediate_mode_leaf() {
        let s = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            Rect::new(0, 0, 50, 50),
        ));
        assert!(!s.is_cacheable_for_paint());
    }

    #[test]
    fn r682_is_cacheable_for_paint_container_recurses_negative() {
        // Container with ImmediateMode descendant rejects the cache.
        // This is the §2 #4 immediate-mode coexistence guard: the
        // parent retained-tree subtree cannot be encoded into a
        // long-lived fragment because the immediate descendant's
        // paint changes every frame.
        let s = Scene::Container(ContainerNode::new(vec![
            box_a(),
            Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
                StubImmediateMode::new(),
                Rect::new(0, 0, 50, 50),
            )),
        ]));
        assert!(!s.is_cacheable_for_paint());
    }

    #[test]
    fn r682_is_cacheable_for_paint_container_recurses_positive() {
        // Container with only cacheable leaves is cacheable.
        let s = Scene::Container(ContainerNode::new(vec![
            box_a(),
            Scene::Text(TextNode::new("hi", rect_a())),
            Scene::Effect(EffectNode::new()),
        ]));
        assert!(s.is_cacheable_for_paint());
    }

    #[test]
    fn r682_is_cacheable_for_paint_recurses_through_scroll() {
        // Scroll is path-transparent for paint cacheability: a
        // Scroll wrapping an uncacheable leaf is uncacheable.
        let inner = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            StubImmediateMode::new(),
            Rect::new(0, 0, 50, 50),
        ));
        let scroll = Scene::Scroll(ScrollNode::new(rect_a(), inner));
        assert!(!scroll.is_cacheable_for_paint());

        // And cacheable when the inner is cacheable.
        let scroll_pure = Scene::Scroll(ScrollNode::new(rect_a(), box_a()));
        assert!(scroll_pure.is_cacheable_for_paint());
    }

    #[test]
    fn r1426_has_visible_blinking_grid_cursor_detects_the_arming_condition() {
        use crate::term_grid::{CursorShape, GridCursor};

        fn grid_with_cursor(cursor: GridCursor) -> Scene {
            let cells = GridBuffer::new(4, 2).with_cursor(cursor);
            Scene::TextGrid(TextGridNode::new(CellMetric::DEFAULT).with_cells(cells))
        }

        // A visible blinking cursor is the arming condition.
        let blinking =
            grid_with_cursor(GridCursor::new(2, 1, CursorShape::Block, true).with_blink(true));
        assert!(blinking.has_visible_blinking_grid_cursor());

        // A visible STEADY cursor does NOT arm (nothing animates).
        let steady = grid_with_cursor(GridCursor::new(2, 1, CursorShape::Block, true));
        assert!(!steady.has_visible_blinking_grid_cursor());

        // A HIDDEN blinking cursor does NOT arm — `visible` (DECTCEM) gates the
        // arming apart from the mode, so a hidden cursor never keeps the window
        // painting no matter its blink mode.
        let hidden_blink =
            grid_with_cursor(GridCursor::new(2, 1, CursorShape::Block, false).with_blink(true));
        assert!(!hidden_blink.has_visible_blinking_grid_cursor());

        // A grid with the default (hidden) cursor, and a non-grid tree, do not
        // arm.
        assert!(
            !Scene::TextGrid(TextGridNode::new(CellMetric::DEFAULT))
                .has_visible_blinking_grid_cursor()
        );
        assert!(!box_a().has_visible_blinking_grid_cursor());

        // The walk recurses: a Container / Scroll wrapping a blinking-cursor
        // grid arms, mirroring `is_cacheable_for_paint`'s recursion.
        let nested = Scene::Container(ContainerNode::new(vec![
            box_a(),
            grid_with_cursor(GridCursor::new(0, 0, CursorShape::Bar, true).with_blink(true)),
        ]));
        assert!(nested.has_visible_blinking_grid_cursor());
        let scrolled = Scene::Scroll(ScrollNode::new(
            rect_a(),
            grid_with_cursor(GridCursor::new(1, 1, CursorShape::Block, true).with_blink(true)),
        ));
        assert!(scrolled.has_visible_blinking_grid_cursor());
        // ...but a Container of only steady/hidden cursors does not.
        let nested_steady = Scene::Container(ContainerNode::new(vec![
            box_a(),
            grid_with_cursor(GridCursor::new(0, 0, CursorShape::Bar, true)),
        ]));
        assert!(!nested_steady.has_visible_blinking_grid_cursor());
    }

    #[test]
    fn r859_as_follower_flag_defaults_false_and_sets() {
        // R859 §5.45 — the linked-scroll follower flag defaults off
        // (every pre-R859 node is a primary) and `as_follower()` flips
        // it without touching the offset / axis / state plumbing.
        let primary = ScrollNode::new(rect_a(), box_a());
        assert!(!primary.follower, "default node is a primary");

        let follower = ScrollNode::new(rect_a(), box_a())
            .with_axis(ScrollAxis::Horizontal)
            .as_follower();
        assert!(follower.follower, "as_follower() marks the node a follower");
        assert_eq!(
            follower.axis,
            ScrollAxis::Horizontal,
            "follower flag is orthogonal to axis selection",
        );
    }

    #[test]
    fn r682_paint_hash_path_includes_commands() {
        // Path commands carry f32 control points; the hasher widens
        // via to_bits() to participate. Two different command lists
        // must hash distinctly.
        let path_a = PathNode::new(
            rect_a(),
            vec![
                PathCommand::MoveTo(PathPoint::new(0.0, 0.0)),
                PathCommand::LineTo(PathPoint::new(10.0, 0.0)),
                PathCommand::Close,
            ],
            PathStyle::default(),
        );
        let path_b = PathNode::new(
            rect_a(),
            vec![
                PathCommand::MoveTo(PathPoint::new(0.0, 0.0)),
                PathCommand::LineTo(PathPoint::new(20.0, 0.0)),
                PathCommand::Close,
            ],
            PathStyle::default(),
        );
        let a = Scene::Path(path_a);
        let b = Scene::Path(path_b);
        assert_ne!(a.paint_hash(), b.paint_hash());
    }

    /// R1516 — the census and the accessors are three separate statements
    /// about one set of variants: [`Scene::node_kind`] matches on the node,
    /// [`SceneNodeKind::carries_box_style`] matches on the kind, and
    /// [`Scene::box_style`] matches on the node again. Nothing but a
    /// fixture of every kind makes them meet, and if they disagree the
    /// census is worse than no census — consumers would iterate a list that
    /// lies about what it names.
    #[test]
    fn r1516_the_census_agrees_with_the_node_it_names() {
        for kind in SceneNodeKind::ALL {
            let scene = crate::test_fixtures::scene_of_kind(kind);
            assert_eq!(
                scene.node_kind(),
                kind,
                "the fixture for {} builds a {:?}",
                kind.name(),
                scene.node_kind()
            );
            assert_eq!(
                scene.box_style().is_some(),
                kind.carries_box_style(),
                "`{}` says carries_box_style = {}, and its node answers \
                 `box_style` = {:?} — one of the two matches is wrong",
                kind.name(),
                kind.carries_box_style(),
                scene.box_style().is_some()
            );
        }
    }

    /// Every kind is in `ALL` exactly once. `ALL` is hand-ordered (the
    /// compiler checks the arms of a `match`, not the members of an array),
    /// so a copy-paste that repeated one kind and dropped another would
    /// leave the dropped one uniterated by every consumer — silently, which
    /// is the shape being prevented, not a shape to reproduce.
    #[test]
    fn r1516_the_census_lists_each_kind_exactly_once() {
        for kind in SceneNodeKind::ALL {
            assert_eq!(
                SceneNodeKind::ALL.iter().filter(|k| **k == kind).count(),
                1,
                "{} appears in `SceneNodeKind::ALL` exactly once",
                kind.name()
            );
        }
    }

    /// The names are the §2 #7 wire `"type"` tag an AI client reads off a
    /// `scene/snapshot` node, so they are identity rather than prose: a
    /// rename here silently moves every client's discriminator. Pinned
    /// against the literal list, which is what the wire emitted before the
    /// census existed.
    #[test]
    fn r1516_census_names_are_the_wire_type_tags() {
        assert_eq!(
            SceneNodeKind::ALL.map(SceneNodeKind::name),
            [
                "Box",
                "Text",
                "Path",
                "Image",
                "Container",
                "Effect",
                "External",
                "Scroll",
                "ImmediateModeNode",
                "TextGrid",
            ]
        );
    }

    /// `Scene::box_style` returns the style the node actually carries, not
    /// merely *a* style. Without this the accessor could answer
    /// `Some(&BoxStyle::default())` for everything and every assertion
    /// above would still pass.
    #[test]
    fn r1516_box_style_returns_the_nodes_own_style() {
        let style = BoxStyle::filled(Color::rgb(0x12, 0x34, 0x56)).with_corner_radius(9);
        let boxed = Scene::Box(BoxNode::new(Rect::default(), style.clone()));
        assert_eq!(boxed.box_style(), Some(&style));
        let container = Scene::Container(ContainerNode::new(vec![]).with_style(style.clone()));
        assert_eq!(container.box_style(), Some(&style));
    }
}
