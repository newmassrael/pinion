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
#[non_exhaustive]
#[derive(Debug, Clone)]
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
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct BoxNode {}

impl BoxNode {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
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

/// Opaque embedded-content escape (§3 capability boundary); §5.15
/// settles the 8-item integration contract for authors.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct ExternalNode {}

impl ExternalNode {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_seven_variants_construct() {
        let _ = Scene::Box(BoxNode::new());
        let _ = Scene::Text(TextNode::new());
        let _ = Scene::Path(PathNode::new());
        let _ = Scene::Image(ImageNode::new());
        let _ = Scene::Container(ContainerNode::new());
        let _ = Scene::Effect(EffectNode::new());
        let _ = Scene::External(ExternalNode::new());
    }

    #[test]
    fn match_arm_exhaustive_within_crate() {
        // Inside the defining crate `#[non_exhaustive]` does not force a
        // wildcard arm, so this exhaustive match doubles as a guard: if
        // someone adds a Scene variant they must touch this test.
        let s = Scene::Box(BoxNode::new());
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
    fn modifier_default_constructs() {
        let _ = Modifier::new();
        let _ = Modifier::default();
    }
}
