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
//! `#[non_exhaustive]` propagates the R14 forward-compat hedge (§5.2
//! caveat): future variants like `Mesh`/`Camera`/`Light` (game-engine
//! evolution) are addable without a `SemVer` major bump.

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

/// Composition modifier (§5.11 layered shape). Layout/transform
/// adjustments that wrap any [`Scene`] variant. The §5.3 DSL settles the
/// concrete operations; this skeleton anchors the type.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct Modifier {}

impl Modifier {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
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
/// `fill` is the v0 ARGB colour (`0x00AARRGGBB`, softbuffer-native);
/// `rect` is the v0 absolute pixel geometry. `Style`-carried
/// properties (border, gradient, shadow) and taffy-driven relative
/// layout settle with the §5.3 DSL — see the §5.11 caveats for the
/// v0 field-schema scope.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct BoxNode {
    pub fill: u32,
    pub rect: Rect,
}

impl BoxNode {
    #[must_use]
    pub const fn new(fill: u32, rect: Rect) -> Self {
        Self { fill, rect }
    }
}

/// Styled text primitive.
///
/// v0 §5.11 shape: `content: String` carries the raw string payload
/// (cosmic-text rasterizer integration deferred); `rect: Rect` gives
/// absolute bounds in the same u32 coordinate space as `BoxNode`.
/// Font / size / colour and the layout-relative positioning come
/// with the §5.3 DSL alongside the rasterizer slice.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct TextNode {
    pub content: String,
    pub rect: Rect,
}

impl TextNode {
    #[must_use]
    pub fn new(content: impl Into<String>, rect: Rect) -> Self {
        Self {
            content: content.into(),
            rect,
        }
    }
}

/// Vector path primitive.
///
/// v0 §5.11 shape: `data: String` carries an opaque path payload
/// (SVG path-d notation is the natural carrier today, but the
/// framework does not parse it — the consumer rasterizer does);
/// `rect: Rect` gives the absolute bounding box for layout/hit
/// purposes. A structured command enum (`MoveTo`/`LineTo`/`CurveTo`/
/// `Close`) plus stroke/fill `Style` lands with the §5.3 DSL.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct PathNode {
    pub data: String,
    pub rect: Rect,
}

impl PathNode {
    #[must_use]
    pub fn new(data: impl Into<String>, rect: Rect) -> Self {
        Self {
            data: data.into(),
            rect,
        }
    }
}

/// Raster or vector image primitive.
///
/// v0 §5.11 shape: `source: String` carries an opaque locator
/// (`file://`, `https://`, `memory://0xABCD`, etc.) that the
/// consumer loader resolves; `rect: Rect` gives the destination
/// bounds. The codec / decoded-buffer cache and `Style`-level
/// fit/cover/tile policy come with the §5.3 DSL.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct ImageNode {
    pub source: String,
    pub rect: Rect,
}

impl ImageNode {
    #[must_use]
    pub fn new(source: impl Into<String>, rect: Rect) -> Self {
        Self {
            source: source.into(),
            rect,
        }
    }
}

/// Child layout container.
///
/// v0 §5.11 shape: holds `children: Vec<Scene>` for structural
/// grouping; taffy-driven flexbox/grid layout (§5.11 decision)
/// arrives with the §5.3 DSL. `Clone` is intentionally *not* derived
/// — `Scene` carries `ExternalNode` (`Box<dyn External>`) which has
/// no general clone strategy, see [`Scene`] doc.
#[non_exhaustive]
#[derive(Debug, Default)]
pub struct ContainerNode {
    pub children: Vec<Scene>,
}

impl ContainerNode {
    #[must_use]
    pub fn new(children: Vec<Scene>) -> Self {
        Self { children }
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
/// Not `Clone` — `Box<dyn External>` has no generic clone strategy,
/// see [`Scene`] doc for the introspection-based alternative.
#[non_exhaustive]
#[derive(Debug)]
pub struct ExternalNode {
    pub handle: Box<dyn crate::external::External>,
}

impl ExternalNode {
    #[must_use]
    pub fn new(handle: Box<dyn crate::external::External>) -> Self {
        Self { handle }
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
        let _ = Scene::Box(BoxNode::new(0, Rect::default()));
        let _ = Scene::Text(TextNode::new("", Rect::default()));
        let _ = Scene::Path(PathNode::new("", Rect::default()));
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
        let s = Scene::Box(BoxNode::new(0, Rect::default()));
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
        let scene = Scene::Box(BoxNode::new(argb, Rect::default()));
        match scene {
            Scene::Box(node) => assert_eq!(node.fill, argb),
            _ => panic!("expected Box variant"),
        }
    }

    #[test]
    fn box_node_rect_round_trips_through_scene() {
        // v0 §5.11 geometry: Rect carries x/y/w/h as u32, lossless
        // round-trip through Scene::Box.
        let rect = Rect::new(10, 20, 160, 80);
        let scene = Scene::Box(BoxNode::new(0, rect));
        match scene {
            Scene::Box(node) => assert_eq!(node.rect, rect),
            _ => panic!("expected Box variant"),
        }
    }

    #[test]
    fn path_node_data_and_rect_round_trip_through_scene() {
        // v0 §5.11 Path shape: opaque `data` string + `rect`. The
        // framework treats `data` as bytes-on-the-wire; the §5.3 DSL
        // settles whether SVG path-d, a typed command enum, or both
        // are the canonical input form.
        let node = PathNode::new("M10 10 L20 20 Z", Rect::new(0, 0, 32, 32));
        let scene = Scene::Path(node);
        match scene {
            Scene::Path(p) => {
                assert_eq!(p.data, "M10 10 L20 20 Z");
                assert_eq!(p.rect, Rect::new(0, 0, 32, 32));
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
            Scene::Box(BoxNode::new(0x00ff_0000, Rect::new(0, 0, 10, 10))),
            Scene::Box(BoxNode::new(0x0000_ff00, Rect::new(20, 20, 5, 5))),
        ];
        let scene = Scene::Container(ContainerNode::new(children));
        match scene {
            Scene::Container(node) => {
                assert_eq!(node.children.len(), 2);
                match &node.children[0] {
                    Scene::Box(b) => assert_eq!(b.fill, 0x00ff_0000),
                    _ => panic!("child 0 not Box"),
                }
            }
            _ => panic!("expected Container variant"),
        }
    }

    #[test]
    fn modifier_default_constructs() {
        let _ = Modifier::new();
        let _ = Modifier::default();
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
