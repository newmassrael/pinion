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

/// Rectangular primitive — the layout-and-fill workhorse.
///
/// `fill` is the v0 minimal field: a `0x00AARRGGBB` packed colour
/// matching the softbuffer presentation format used by the §4 first
/// dogfood. Geometry (rect/insets) and `Style`-carried properties
/// (border, gradient, shadow) settle with the §5.3 DSL — see the
/// §5.11 caveat for the v0 field-schema scope.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct BoxNode {
    pub fill: u32,
}

impl BoxNode {
    #[must_use]
    pub const fn new(fill: u32) -> Self {
        Self { fill }
    }
}

/// Styled text primitive.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct TextNode {}

impl TextNode {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

/// Vector path primitive.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct PathNode {}

impl PathNode {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

/// Raster or vector image primitive.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct ImageNode {}

impl ImageNode {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

/// Child layout container.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct ContainerNode {}

impl ContainerNode {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
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
        let _ = Scene::Box(BoxNode::new(0));
        let _ = Scene::Text(TextNode::new());
        let _ = Scene::Path(PathNode::new());
        let _ = Scene::Image(ImageNode::new());
        let _ = Scene::Container(ContainerNode::new());
        let _ = Scene::Effect(EffectNode::new());
        let _ = Scene::External(ExternalNode::new(stub_handle()));
    }

    #[test]
    fn match_arm_exhaustive_within_crate() {
        // Inside the defining crate `#[non_exhaustive]` does not force a
        // wildcard arm, so this exhaustive match doubles as a guard: if
        // someone adds a Scene variant they must touch this test.
        let s = Scene::Box(BoxNode::new(0));
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
        let scene = Scene::Box(BoxNode::new(argb));
        match scene {
            Scene::Box(node) => assert_eq!(node.fill, argb),
            _ => panic!("expected Box variant"),
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
