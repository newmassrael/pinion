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
    pub layout: LayoutStyle,
    pub tag: Option<Cow<'static, str>>,
}

impl ContainerNode {
    #[must_use]
    pub fn new(children: Vec<Scene>) -> Self {
        Self {
            children,
            rect: Rect::default(),
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
