//! Scene highlight injection / clearing — pure functional transforms.
//!
//! Two operations form §5.33's v0 visible-value surface:
//!
//!   1. [`inject_highlight`] — given a target path, append a sibling
//!      `Scene::Box` carrying the [`HIGHLIGHT_TAG_PREFIX`] tag and the
//!      target's bounding rect. The original scene is consumed and a
//!      new scene is returned (immutable transform).
//!
//!   2. [`clear_highlights`] — strip every existing overlay box,
//!      identified by tag prefix. Returns a new scene; idempotent on
//!      scenes with no overlays.
//!
//! ## Why a Box, not an Effect?
//!
//! `Scene::Effect` is opaque (§3 capability boundary) — an AI agent
//! cannot later query "which highlights are currently active" via
//! [`scene/query`](pinion_rpc::query). A tagged `Scene::Box` is
//! introspectable: a follow-up `scene/locate_region` over the whole
//! viewport surfaces every highlight by tag, and the §5.20 intent
//! system already drains tagged nodes through the runtime.
//!
//! ## Where do highlights live in the tree?
//!
//! v0 inserts them as siblings of the root primitive when the root is
//! a `Scene::Container`. If the root is *not* a container, the entire
//! scene is wrapped in a freshly-constructed container so the
//! highlight has somewhere to live alongside the original content.
//! This keeps z-order trivial — highlights are drawn last (topmost)
//! by §5.2 painting order.

use pinion_core::Scene;
use pinion_core::scene::{BoxNode, ContainerNode, Rect};
use pinion_core::style::{Border, BoxStyle, Color};

/// Tag prefix that identifies overlay-managed boxes. Every highlight
/// node carries `ai-overlay/<target-suffix>` as its [§5.20] tag so
/// [`clear_highlights`] can find and remove them without touching
/// author-owned nodes.
pub const HIGHLIGHT_TAG_PREFIX: &str = "ai-overlay/";

/// Visual style of an injected highlight. v0 keeps this minimal —
/// a stroke colour and stroke width. Future variants (dashed, glow,
/// pulse animation) are carry-forward; expanding this struct stays
/// additive thanks to `#[non_exhaustive]`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightStyle {
    /// Stroke colour for the highlight border. ARGB.
    pub stroke: Color,
    /// Stroke width in logical pixels.
    pub stroke_width: u32,
}

impl HighlightStyle {
    /// Default v0 style: 2-pixel opaque-red border. Picked for high
    /// contrast against arbitrary scene content; consumers can supply
    /// their own [`HighlightStyle`] when theming matters.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stroke: Color::from_argb(0x00ff_0040),
            stroke_width: 2,
        }
    }

    /// Builder: override the stroke colour. Const-friendly so callers
    /// can declare semantic styles (`PENDING_PREVIEW`, `REGION_SELECT`,
    /// …) as compile-time constants alongside their own theme palette.
    /// `#[non_exhaustive]` blocks struct-literal construction from
    /// outside `pinion-overlay`; these builders are how downstream
    /// crates and examples supply non-default values.
    #[must_use]
    pub const fn with_stroke(mut self, stroke: Color) -> Self {
        self.stroke = stroke;
        self
    }

    /// Builder: override the stroke width.
    #[must_use]
    pub const fn with_stroke_width(mut self, width: u32) -> Self {
        self.stroke_width = width;
        self
    }
}

impl Default for HighlightStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Inject a highlight around the primitive at `path_suffix` inside
/// `scene`. The suffix is the path component *after* the `/window[id]/`
/// prefix — typically the same string [`pinion_rpc::locate`] returned
/// in its `path` field, with the prefix stripped, OR a raw
/// container-relative path such as `"save_btn"` or `"0/1"`.
///
/// Idempotency: re-injecting the *same* (`path_suffix`, style) is a
/// no-op — the existing highlight box is detected by its tag and
/// left in place. Adjusting style with the same path replaces the
/// stroke; bbox is recomputed in case the target moved.
///
/// Returns the unchanged scene when the path does not resolve (no
/// error path — the AI agent's responsibility to issue a meaningful
/// path; silent miss matches the "highlight nothing" semantics).
#[must_use]
pub fn inject_highlight(scene: Scene, path_suffix: &str, style: HighlightStyle) -> Scene {
    let segments: Vec<String> = path_suffix
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();

    let Some(bbox) = scene.lookup_path(&segments) else {
        return scene;
    };

    let tag = format!("{HIGHLIGHT_TAG_PREFIX}{path_suffix}");
    let highlight = build_highlight_box(bbox, &tag, style);

    let mut wrapped = wrap_into_container(scene);
    // Strip any pre-existing highlight with the same tag so a repeat
    // call updates rather than duplicates.
    strip_tag(&mut wrapped, &tag);
    push_top_level(&mut wrapped, Scene::Box(highlight));
    wrapped
}

/// (R1125 §5.51 §2 #7 PR-33) Inject a CALLER-built overlay node as a top-level,
/// on-top sibling, replacing any prior overlay carrying `tag` (idempotent — a
/// repeat call updates rather than duplicates). `node` is `None` to just clear a
/// previously-injected overlay (the drop preview vanishes when the drag leaves
/// the window). Domain-agnostic on purpose: the caller (the shell) builds the
/// node from a higher layer it can see but this crate cannot — e.g.
/// `pinion_widget_paint::dock_drop_preview_overlay` for the cross-window dock
/// drop-zone preview — and shares the same wrap / strip / push discipline as
/// [`inject_highlight`] and [`crate::focus_ring`]. The caller is responsible for
/// the node's own absolute positioning + `pointer_transparent`.
#[must_use]
pub fn inject_overlay_node(scene: Scene, tag: &str, node: Option<Scene>) -> Scene {
    let mut wrapped = wrap_into_container(scene);
    strip_tag(&mut wrapped, tag);
    if let Some(node) = node {
        push_top_level(&mut wrapped, node);
    }
    wrapped
}

/// Strip every overlay-injected node (tag starts with
/// [`HIGHLIGHT_TAG_PREFIX`]) from `scene`. The original author-owned
/// nodes are preserved verbatim. Idempotent: clearing twice yields
/// the same scene the second time.
#[must_use]
pub fn clear_highlights(scene: Scene) -> Scene {
    // R1123.1 — one retain-by-prefix SSOT: delegate to the shared
    // [`strip_children_with_prefix`] (the R1122 resize-border idempotency also
    // uses it) rather than re-implementing the `starts_with` retain here.
    let mut s = scene;
    strip_children_with_prefix(&mut s, HIGHLIGHT_TAG_PREFIX);
    s
}

fn build_highlight_box(bbox: Rect, tag: &str, style: HighlightStyle) -> BoxNode {
    let border = Border::new(style.stroke, style.stroke_width);
    let bstyle = BoxStyle::filled(Color::TRANSPARENT).with_border(border);
    // R807 §5.16 — like the §5.39 focus ring, a highlight box flush at the
    // window top would have its 2px border flooded ~16px thick by the vello
    // top-tile bug. Funnel the bbox through the shared overlay SSOT
    // ([`crate::edge`]) so the workaround lives in one place; transparent
    // fill means the 1px top nudge moves only the border, not a fill edge.
    let mut node = BoxNode::new(crate::edge::clamp_top_off_flood_row(bbox), bstyle);
    node.tag = Some(tag.to_owned().into());
    // R705 §5.39 — overlays are decorative: pointer-transparent so the
    // highlighted widget keeps receiving input even when an overlay box
    // is layered on top of it in the live (hit-tested) paint scene.
    node.layout = node.layout.with_pointer_transparent(true);
    node
}

/// Ensure the scene is a `Container`, wrapping it if not. Returns the
/// container variant ready for sibling injection. `pub(crate)` so the
/// §5.39 [`crate::focus_ring`] overlay (R705, the 2nd consumer of the
/// inject-overlay-box pattern) shares the same wrap discipline.
pub(crate) fn wrap_into_container(scene: Scene) -> Scene {
    if matches!(scene, Scene::Container(_)) {
        return scene;
    }
    let original_rect = scene.rect();
    let mut c = ContainerNode::new(vec![scene]);
    c.rect = original_rect;
    Scene::Container(c)
}

pub(crate) fn push_top_level(scene: &mut Scene, child: Scene) {
    if let Scene::Container(c) = scene {
        c.children.push(child);
    }
}

pub(crate) fn strip_tag(scene: &mut Scene, tag: &str) {
    if let Scene::Container(c) = scene {
        c.children.retain(|child| child.tag() != Some(tag));
    }
}

/// Whether `scene` is a container with a direct (top-level) child tagged
/// `tag`. Used to gate an idempotent re-layer on the presence of the node it
/// would move (so the operation is a pure no-op when the node is absent).
pub(crate) fn has_top_level_tag(scene: &Scene, tag: &str) -> bool {
    matches!(scene, Scene::Container(c) if c.children.iter().any(|ch| ch.tag() == Some(tag)))
}

/// Strip every top-level child whose tag starts with `prefix`. The
/// [`crate::window_chrome`] resize border (R1122) injects its eight edge /
/// corner hit regions as FLAT siblings of the content rather than in one
/// bounding sub-container (a full-window container would absorb every
/// center click via [`Scene::hit_test`]'s "no child hit ⇒ the container is
/// the hit" rule). Flat siblings have no single container tag to strip, so
/// idempotent re-injection strips them by their shared tag prefix instead.
pub(crate) fn strip_children_with_prefix(scene: &mut Scene, prefix: &str) {
    if let Scene::Container(c) = scene {
        c.children
            .retain(|child| !child.tag().is_some_and(|t| t.starts_with(prefix)));
    }
}

/// Re-export of the hit-path shape so callers can build their own
/// path strings without pulling pinion-core directly.
pub use pinion_core::scene::HitPath as HighlightHitPath;

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Color;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect};

    fn box_at(x: u32, y: u32, w: u32, h: u32) -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(x, y, w, h), Color::default()))
    }

    fn tagged_box_at(x: u32, y: u32, w: u32, h: u32, tag: &'static str) -> Scene {
        Scene::Box(BoxNode::filled(Rect::new(x, y, w, h), Color::default()).with_tag(tag))
    }

    fn container_with(children: Vec<Scene>) -> Scene {
        let mut c = ContainerNode::new(children);
        c.rect = Rect::new(0, 0, 1000, 1000);
        Scene::Container(c)
    }

    fn count_overlay_children(scene: &Scene) -> usize {
        let Scene::Container(c) = scene else { return 0 };
        c.children
            .iter()
            .filter(|ch| {
                ch.tag()
                    .is_some_and(|t| t.starts_with(HIGHLIGHT_TAG_PREFIX))
            })
            .count()
    }

    #[test]
    fn inject_into_lone_box_wraps_in_container() {
        let scene = box_at(10, 20, 30, 40);
        let out = inject_highlight(scene, "", HighlightStyle::default());
        let Scene::Container(c) = &out else {
            panic!("expected wrap into Container");
        };
        // 2 children: original Box + highlight Box
        assert_eq!(c.children.len(), 2);
        assert_eq!(count_overlay_children(&out), 1);
    }

    #[test]
    fn inject_into_container_appends_sibling() {
        let scene = container_with(vec![tagged_box_at(10, 10, 50, 50, "save_btn")]);
        let out = inject_highlight(scene, "save_btn", HighlightStyle::default());
        // Original child + highlight
        let Scene::Container(c) = &out else {
            panic!("Container")
        };
        assert_eq!(c.children.len(), 2);
        let Scene::Box(highlight) = &c.children[1] else {
            panic!("Box")
        };
        assert_eq!(
            highlight.tag.as_deref(),
            Some("ai-overlay/save_btn"),
            "tag carries prefix + target path",
        );
        // bbox copied from the target
        assert_eq!(highlight.rect, Rect::new(10, 10, 50, 50));
    }

    #[test]
    fn highlight_on_top_flush_widget_clears_the_vello_flood_row() {
        // R807 §5.16 — a highlight box flush at the window top (y=0) must,
        // like the focus ring, keep its 2px border off the vello y=0 flood
        // row via the shared `crate::edge` SSOT. Top nudges to 1, bottom edge
        // preserved (h shrinks 1); x/w untouched (the left edge does not
        // flood). This is the R806.1 incompleteness (highlight was unfixed)
        // cleared by routing both overlays through one helper.
        let scene = container_with(vec![tagged_box_at(96, 0, 96, 40, "title")]);
        let out = inject_highlight(scene, "title", HighlightStyle::default());
        let Scene::Container(c) = &out else {
            panic!("Container")
        };
        let Scene::Box(highlight) = &c.children[1] else {
            panic!("Box")
        };
        assert_eq!(
            highlight.rect,
            Rect::new(96, 1, 96, 39),
            "top off flood row"
        );
        assert_eq!(
            highlight.rect.y + highlight.rect.h,
            40,
            "bottom edge preserved"
        );
    }

    #[test]
    fn inject_on_unknown_path_is_silent_no_op() {
        let scene = container_with(vec![box_at(0, 0, 10, 10)]);
        let out = inject_highlight(scene, "ghost", HighlightStyle::default());
        // No new child; not wrapped further.
        let Scene::Container(c) = &out else {
            panic!("Container")
        };
        assert_eq!(c.children.len(), 1);
        assert_eq!(count_overlay_children(&out), 0);
    }

    #[test]
    fn inject_is_idempotent_on_same_path() {
        let scene = container_with(vec![tagged_box_at(10, 10, 20, 20, "btn")]);
        let once = inject_highlight(scene, "btn", HighlightStyle::default());
        let twice = inject_highlight(once, "btn", HighlightStyle::default());
        // Still exactly one overlay child despite double injection.
        assert_eq!(count_overlay_children(&twice), 1);
        let Scene::Container(c) = &twice else {
            panic!("Container")
        };
        assert_eq!(c.children.len(), 2, "1 original + 1 highlight");
    }

    #[test]
    fn inject_different_paths_accumulate() {
        let scene = container_with(vec![
            tagged_box_at(0, 0, 10, 10, "a"),
            tagged_box_at(50, 50, 10, 10, "b"),
        ]);
        let s = inject_highlight(scene, "a", HighlightStyle::default());
        let s = inject_highlight(s, "b", HighlightStyle::default());
        assert_eq!(count_overlay_children(&s), 2);
    }

    #[test]
    fn clear_removes_only_overlay_tagged_children() {
        let scene = container_with(vec![tagged_box_at(10, 10, 20, 20, "btn")]);
        let with_overlay = inject_highlight(scene, "btn", HighlightStyle::default());
        assert_eq!(count_overlay_children(&with_overlay), 1);
        let cleared = clear_highlights(with_overlay);
        assert_eq!(count_overlay_children(&cleared), 0);
        // Original child preserved
        let Scene::Container(c) = &cleared else {
            panic!("Container")
        };
        assert_eq!(c.children.len(), 1);
        assert_eq!(c.children[0].tag(), Some("btn"));
    }

    #[test]
    fn clear_is_idempotent_on_clean_scene() {
        let scene = container_with(vec![box_at(0, 0, 10, 10)]);
        let cleared = clear_highlights(scene);
        let cleared2 = clear_highlights(cleared);
        let Scene::Container(c) = &cleared2 else {
            panic!("Container")
        };
        assert_eq!(c.children.len(), 1);
    }

    #[test]
    fn with_stroke_overrides_color_const_safely() {
        // const-friendly builder so downstream crates can declare
        // semantic-tagged styles as compile-time constants. Sanity-
        // checks that `with_stroke` actually replaces the default red.
        const PENDING: HighlightStyle = HighlightStyle::new()
            .with_stroke(Color::from_argb(0x00ff_d000))
            .with_stroke_width(3);
        assert_eq!(PENDING.stroke, Color::from_argb(0x00ff_d000));
        assert_eq!(PENDING.stroke_width, 3);
    }

    #[test]
    fn highlight_box_uses_supplied_style() {
        let style = HighlightStyle {
            stroke: Color::from_argb(0x0000_ff00),
            stroke_width: 5,
        };
        let scene = container_with(vec![box_at(0, 0, 10, 10)]);
        let out = inject_highlight(scene, "0", style);
        let Scene::Container(c) = &out else {
            panic!("Container")
        };
        let Scene::Box(highlight) = &c.children[1] else {
            panic!("Box")
        };
        let b = highlight.style.border.expect("border emitted");
        assert_eq!(b.color, Color::from_argb(0x0000_ff00));
        assert_eq!(b.width, 5);
    }

    #[test]
    fn highlight_box_has_transparent_fill() {
        // The injected box must not occlude the underlying content —
        // only the border draws.
        let scene = container_with(vec![box_at(0, 0, 10, 10)]);
        let out = inject_highlight(scene, "0", HighlightStyle::default());
        let Scene::Container(c) = &out else {
            panic!("Container")
        };
        let Scene::Box(highlight) = &c.children[1] else {
            panic!("Box")
        };
        assert_eq!(highlight.style.fill, Color::TRANSPARENT);
    }
}
