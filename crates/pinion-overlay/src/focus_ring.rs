//! §5.39 focus ring as an introspectable overlay Scene node (R705).
//!
//! ## Why this module exists
//!
//! pinion's reason to exist is that an AI agent understands the screen
//! WITHOUT a screenshot — §2 #7 (scene-as-data) + §2 #1 (no opaque
//! paint callbacks) + §2 #2 (RPC headless primary). Until R705 the
//! keyboard focus ring violated both #1 and #7: the shell stroked it
//! straight into the `vello::Scene` *after* the Scene→Vello tree walk
//! (`pinion_runtime::paint_adapter::paint_focus_ring`). That stroke was
//! opaque paint — it never appeared in `scene/snapshot`, so an AI agent
//! could not observe which widget was focused or where its ring drew,
//! and it ignored the focused node's `corner_radius` (always a sharp
//! rectangle, so a circular day-cell got a square ring — the R704
//! datepicker visual defect that surfaced this).
//!
//! [`inject_focus_ring`] promotes the ring to a real
//! [`Scene::Box`] overlay: a transparent-fill, bordered, corner-radius-
//! matched, **pointer-transparent** sibling layered on top of the
//! focused widget. It is the 2nd consumer of the inject-overlay-box
//! pattern [`crate::highlight::inject_highlight`] established (§5.33),
//! and it relies on the R705 §5.39 `pointer-events: none` substrate
//! ([`pinion_core::style::LayoutStyle::pointer_transparent`]) so the
//! ring never shadows its widget for input even though it lives in the
//! live, hit-tested paint scene.
//!
//! The result: the ring is (a) introspectable via `scene/snapshot`
//! (§2 #7), (b) a real Scene node painted by the generic box path
//! rather than an opaque callback (§2 #1), and (c) corner-radius-aware
//! so a rounded widget gets a concentric rounded ring.

use pinion_core::scene::{BoxNode, Rect, Scene};
use pinion_core::style::{Border, BoxStyle, Color};

use crate::highlight::{push_top_level, strip_tag, wrap_into_container};

/// Tag carried by the injected focus-ring overlay box. Shares the
/// `ai-overlay/` family prefix with [`crate::HIGHLIGHT_TAG_PREFIX`] so
/// the same "strip every overlay node" discipline applies, but is a
/// single fixed tag (there is at most one focused widget, hence one
/// ring) rather than a per-target suffix.
pub const FOCUS_RING_TAG: &str = "ai-overlay/focus-ring";

/// Visual style of the focus ring. Mirrors [`crate::HighlightStyle`]
/// but adds the outward `offset` that keeps the ring clear of the
/// widget's own visual edge (Material / Fluent convention). The ring's
/// corner radius is NOT a style field — it is derived from the focused
/// node so the ring stays concentric with rounded widgets.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusRingStyle {
    /// Stroke colour for the ring border. ARGB.
    pub stroke: Color,
    /// Stroke width in logical pixels (WCAG 2.4.11 asks for ≥2).
    pub stroke_width: u32,
    /// Outward offset from the focused widget's rect, in logical
    /// pixels, on every side. The ring box is the widget rect inflated
    /// by this much, so a non-zero offset leaves a gap between the
    /// widget edge and the ring.
    pub offset: u32,
}

impl FocusRingStyle {
    /// Default ring: 2px stroke, 2px outward offset, Material default
    /// focus blue `#1A73E8` (26, 115, 232). This is the same colour /
    /// geometry the pre-R705 opaque `paint_focus_ring` used, so the
    /// migration is visually faithful for sharp-cornered widgets and
    /// strictly better (concentric) for rounded ones.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stroke: Color::rgb(26, 115, 232),
            stroke_width: 2,
            offset: 2,
        }
    }

    /// Builder: override the ring colour.
    #[must_use]
    pub const fn with_stroke(mut self, stroke: Color) -> Self {
        self.stroke = stroke;
        self
    }

    /// Builder: override the ring stroke width.
    #[must_use]
    pub const fn with_stroke_width(mut self, width: u32) -> Self {
        self.stroke_width = width;
        self
    }

    /// Builder: override the outward offset.
    #[must_use]
    pub const fn with_offset(mut self, offset: u32) -> Self {
        self.offset = offset;
        self
    }
}

impl Default for FocusRingStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Inject a focus ring around the node tagged `focused_tag` inside
/// `scene`. `focused_tag` is the §5.40 [`pinion_runtime::FocusManager`]
/// focused tag — the same tag the input router hit-tests against,
/// including the R51.42 composite `tag#sub` form a roving
/// active-descendant widget (`RadioGroup`, the R704 datepicker grid)
/// paints its focused cell with.
///
/// The ring box is the focused node's post-layout rect inflated by
/// `style.offset` on every side (clamped at the top/left framebuffer
/// edge so a flush widget keeps a concentric ring — R806, see
/// [`build_focus_ring_box`]), with a border of `style.stroke` /
/// `style.stroke_width`, a transparent fill, and a corner radius that
/// tracks the focused node's own rounding (`node_radius + offset` when
/// the node is rounded, `0` otherwise) so the ring stays concentric.
/// The box is pointer-transparent (§5.39) so it never shadows the
/// widget for input.
///
/// ## Coordinate space (R705.1 §5.45 §2 #7)
///
/// The ring rect is the focused node's **window-absolute** post-layout
/// rect, resolved through the single coordinate-translation authority
/// [`pinion_core::scene::Scene::rect_for_tag_absolute`]. This is the fix
/// for the original R705 defect: a focused widget inside a
/// [`Scene::Scroll`] (a listbox row, a tree row, a control below the
/// fold) carries a *scroll-local* post-layout rect, but the ring is a
/// top-level overlay painted in *window-absolute* space. The pre-R705.1
/// walker read the raw scroll-local rect and pushed the ring top-level
/// unchanged, so it drew tens of pixels off the widget on screen for
/// every scrolled widget — and because `scene/snapshot from: paint`
/// compared the ring rect against the widget's (also scroll-local) rect,
/// the demos could not catch the drift (the assertion was tautological,
/// [[introspection-from-paint-not-screen]]).
///
/// Resolving through `rect_for_tag_absolute` translates the rect by the
/// enclosing Scroll offsets and clips it to the viewport stack — exactly
/// the resolver the RPC click/drag path uses to land input — so the ring
/// frames the widget concentrically on screen, and a grounded test can
/// independently recompute the window-absolute rect and assert the ring
/// matches it.
///
/// Returns the scene **unchanged** when:
/// * `focused_tag` is `None` (no widget focused — `FocusManager`
///   between Tab boundaries), or
/// * no node in the (post-layout) paint scene carries `focused_tag`
///   (the focused widget was removed by the view fn this frame, or it is
///   scrolled fully out of the viewport — `rect_for_tag_absolute`
///   returns `None`).
///
/// Idempotent: any pre-existing ring (same [`FOCUS_RING_TAG`]) is
/// stripped before the fresh one is appended, so re-injecting replaces
/// rather than duplicates.
#[must_use]
pub fn inject_focus_ring(scene: Scene, focused_tag: Option<&str>, style: FocusRingStyle) -> Scene {
    let Some(tag) = focused_tag else {
        return scene;
    };
    // Window-absolute, viewport-clipped rect so the top-level overlay box
    // lands where the widget actually paints — even inside a Scroll.
    let Some(rect) = scene.rect_for_tag_absolute(tag) else {
        return scene;
    };
    let corner_radius = corner_radius_for_tag(&scene, tag);
    let ring = build_focus_ring_box(rect, corner_radius, style);
    let mut wrapped = wrap_into_container(scene);
    strip_tag(&mut wrapped, FOCUS_RING_TAG);
    push_top_level(&mut wrapped, Scene::Box(ring));
    wrapped
}

/// Depth-first walk for the focused node's corner radius (space-
/// invariant, so no translation needed — the rect comes from
/// [`pinion_core::scene::Scene::rect_for_tag_absolute`] separately).
/// `Box` / `Container` carry an explicit `corner_radius`; every other
/// variant has no rounding concept and reports `0` (sharp ring). Mirrors
/// the scroll-descending walk shape so a rounded day-cell inside a
/// scrolled grid still gets a concentric rounded ring.
fn corner_radius_for_tag(scene: &Scene, target: &str) -> u32 {
    radius_opt(scene, target).unwrap_or(0)
}

fn radius_opt(scene: &Scene, target: &str) -> Option<u32> {
    if scene.tag() == Some(target) {
        return Some(match scene {
            Scene::Box(n) => n.style.corner_radius,
            Scene::Container(n) => n.style.corner_radius,
            _ => 0,
        });
    }
    match scene {
        Scene::Container(c) => c.children.iter().find_map(|s| radius_opt(s, target)),
        Scene::Scroll(s) => radius_opt(&s.content, target),
        _ => None,
    }
}

/// Minimum gap (logical px) the ring's painted geometry keeps below the
/// **top** framebuffer edge (R806 §5.39 §5.16). Vello's sparse-strip
/// rasteriser floods its whole top 16px coarse tile when a stroke's
/// coverage reaches the `y = 0` scanline (reproduced deterministically in
/// `pinion_shell::headless_screenshot` — a top-flush 2px Inside border
/// rasterises a ~16px-thick top band, while the same border one pixel
/// lower, or against the left edge, is a faithful 2px). The defect is
/// invisible to `scene/snapshot` (the scene carries a 2px border) and was
/// the real "thick menubar focus-ring top edge" the user observed. Until
/// the upstream vello fix lands we keep the ring's top stroke one pixel
/// inside the framebuffer; the left/right/bottom edges do NOT flood, so
/// only the top is inset. See [[introspection-from-paint-not-screen]].
const TOP_EDGE_INSET: u32 = 1;

fn build_focus_ring_box(target: Rect, target_radius: u32, style: FocusRingStyle) -> BoxNode {
    let off = style.offset;
    // Concentric outset, boundary-clipped (R806 §5.39). `Rect` origins are
    // unsigned, so a widget flush against the top/left framebuffer edge
    // (`x` or `y` < `off`) cannot carry the negative origin a full outward
    // offset needs. We clamp the origin and shrink the span by the *same*
    // clamped amount, so the far (bottom/right) edge still lands at
    // `target + off` — the ring stays concentric with the widget and the
    // framebuffer edge clips the lost near gap.
    //
    // Top edge additionally floors at `TOP_EDGE_INSET` (1px), not 0, to
    // dodge the vello top-tile flood documented on that constant: a stroke
    // whose coverage reaches `y = 0` is rasterised ~16px thick. The left
    // edge does not flood, so `x` floors at 0 (a left-flush widget keeps a
    // concentric left ring); only `y` carries the inset.
    let x = target.x.saturating_sub(off);
    let y = target.y.saturating_sub(off).max(TOP_EDGE_INSET);
    // Ideal far edges = widget far edge + the full outward offset. The span
    // back from the clamped near origin keeps the ring concentric.
    let ideal_right = target.x.saturating_add(target.w).saturating_add(off);
    let ideal_bottom = target.y.saturating_add(target.h).saturating_add(off);
    let ring_rect = Rect::new(
        x,
        y,
        ideal_right.saturating_sub(x),
        ideal_bottom.saturating_sub(y),
    );
    // Keep the ring concentric with a rounded widget: grow the radius
    // by the same offset the rect grew. A sharp widget (radius 0) keeps
    // a sharp ring.
    let ring_radius = if target_radius == 0 {
        0
    } else {
        target_radius.saturating_add(off)
    };
    let border = Border::new(style.stroke, style.stroke_width);
    let bstyle = BoxStyle::filled(Color::TRANSPARENT)
        .with_border(border)
        .with_corner_radius(ring_radius);
    let mut node = BoxNode::new(ring_rect, bstyle);
    node.tag = Some(FOCUS_RING_TAG.into());
    // §5.39 — decorative overlay: invisible to hit-testing.
    node.layout = node.layout.with_pointer_transparent(true);
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ContainerNode;

    fn tagged_box(x: u32, y: u32, w: u32, h: u32, radius: u32, tag: &'static str) -> Scene {
        let style = BoxStyle::filled(Color::rgb(10, 10, 10)).with_corner_radius(radius);
        let mut node = BoxNode::new(Rect::new(x, y, w, h), style);
        node.tag = Some(tag.into());
        Scene::Box(node)
    }

    fn container(children: Vec<Scene>) -> Scene {
        let mut c = ContainerNode::new(children);
        c.rect = Rect::new(0, 0, 1000, 1000);
        Scene::Container(c)
    }

    fn ring_child(scene: &Scene) -> &BoxNode {
        let Scene::Container(c) = scene else { panic!("expected Container") };
        let ring = c
            .children
            .iter()
            .find(|ch| ch.tag() == Some(FOCUS_RING_TAG))
            .expect("ring child present");
        let Scene::Box(b) = ring else { panic!("ring is a Box") };
        b
    }

    #[test]
    fn none_focus_is_unchanged_no_op() {
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let out = inject_focus_ring(scene, None, FocusRingStyle::default());
        let Scene::Container(c) = &out else { panic!() };
        assert_eq!(c.children.len(), 1, "no ring when nothing focused");
    }

    #[test]
    fn unknown_tag_is_unchanged_no_op() {
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("ghost"), FocusRingStyle::default());
        let Scene::Container(c) = &out else { panic!() };
        assert_eq!(c.children.len(), 1, "no ring when focused tag absent");
    }

    #[test]
    fn ring_inflated_by_offset_around_target() {
        let scene = container(vec![tagged_box(100, 50, 40, 30, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default());
        let ring = ring_child(&out);
        // default offset = 2 → rect grows by 2 on each side.
        assert_eq!(ring.rect, Rect::new(98, 48, 44, 34));
    }

    #[test]
    fn ring_stays_concentric_at_top_left_flush_corner() {
        // R806 regression guard. A menubar title flush at the window
        // top-left corner: (0, 0, 96, 40). A full +2 outset would need a
        // (-2, -2) origin the unsigned `Rect` cannot carry. The ring clamps
        // the LEFT origin to 0 and the TOP origin to TOP_EDGE_INSET (1, to
        // dodge the vello y=0 top-tile flood), shrinking the span so the
        // bottom/right edge still lands at the concentric `target + 2` —
        // i.e. the ring is (0, 1, 98, 41), NOT the pre-R806 naive
        // (0, 0, 100, 44) whose bottom/right was pushed a doubled gap out.
        let scene = container(vec![tagged_box(0, 0, 96, 40, 0, "menu#t0")]);
        let out = inject_focus_ring(scene, Some("menu#t0"), FocusRingStyle::default());
        let ring = ring_child(&out);
        assert_eq!(ring.rect, Rect::new(0, 1, 98, 41), "concentric, top inset off y=0");
        // The far edges keep the full outward offset (concentric); only the
        // near gap is clipped (left by the framebuffer, top by the inset).
        assert_eq!(ring.rect.x + ring.rect.w, 98, "right edge = widget right (96) + 2");
        assert_eq!(ring.rect.y + ring.rect.h, 42, "bottom edge = widget bottom (40) + 2");
        assert!(ring.rect.y >= TOP_EDGE_INSET, "top stroke kept off the y=0 flood row");
    }

    #[test]
    fn ring_clips_only_the_overflowing_axis() {
        // A widget flush against the TOP edge only (y = 0) but well clear of
        // the left edge (x = 200): the y-axis insets to TOP_EDGE_INSET while
        // the x-axis keeps its symmetric outset. Proves the clamp is
        // per-axis, not a whole-rect shift.
        let scene = container(vec![tagged_box(200, 0, 40, 30, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default());
        let ring = ring_child(&out);
        // x: 200-2=198, w: (200+40+2)-198 = 44 (full symmetric outset).
        // y: floored to 1 (off the flood row), h: (0+30+2)-1 = 31.
        assert_eq!(ring.rect, Rect::new(198, 1, 44, 31));
    }

    #[test]
    fn ring_clamp_within_offset_of_edge_shrinks_partially() {
        // A widget 1px below the top edge (y = 1), offset 2: the ideal top
        // origin (y-2) saturates to 0 but floors to TOP_EDGE_INSET (1), so
        // the top stroke clears the flood row and the height shrinks to suit.
        // Bottom edge stays concentric.
        let scene = container(vec![tagged_box(50, 1, 40, 30, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default());
        let ring = ring_child(&out);
        // y: max(1, 1-2)=1. h: (1+30+2)-1 = 32.
        assert_eq!(ring.rect, Rect::new(48, 1, 44, 32));
        assert_eq!(ring.rect.y + ring.rect.h, 33, "bottom = widget bottom (31) + 2");
    }

    #[test]
    fn ring_radius_tracks_rounded_target_concentrically() {
        // Circular day-cell: 40x40, radius 20 (the R704 datepicker
        // geometry). The ring must be rounded too — radius 20 + 2px
        // offset = 22 — not a sharp square.
        let scene = container(vec![tagged_box(0, 0, 40, 40, 20, "day#15")]);
        let out = inject_focus_ring(scene, Some("day#15"), FocusRingStyle::default());
        let ring = ring_child(&out);
        assert_eq!(ring.style.corner_radius, 22, "concentric rounded ring");
    }

    #[test]
    fn ring_stays_sharp_for_sharp_target() {
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default());
        let ring = ring_child(&out);
        assert_eq!(ring.style.corner_radius, 0, "sharp widget keeps sharp ring");
    }

    #[test]
    fn ring_is_pointer_transparent_and_transparent_fill() {
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default());
        let ring = ring_child(&out);
        assert!(
            ring.layout.pointer_transparent,
            "ring must not shadow its widget for input",
        );
        assert_eq!(ring.style.fill, Color::TRANSPARENT, "ring must not occlude");
    }

    #[test]
    fn ring_carries_style_border() {
        let style = FocusRingStyle::new()
            .with_stroke(Color::rgb(255, 0, 0))
            .with_stroke_width(3);
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), style);
        let ring = ring_child(&out);
        let b = ring.style.border.expect("border emitted");
        assert_eq!(b.color, Color::rgb(255, 0, 0));
        assert_eq!(b.width, 3);
    }

    #[test]
    fn reinjection_replaces_not_duplicates() {
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let once = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default());
        let twice = inject_focus_ring(once, Some("btn"), FocusRingStyle::default());
        let Scene::Container(c) = &twice else { panic!() };
        let rings = c.children.iter().filter(|ch| ch.tag() == Some(FOCUS_RING_TAG)).count();
        assert_eq!(rings, 1, "exactly one ring after double injection");
    }

    #[test]
    fn ring_tracks_scrolled_widget_into_window_absolute_space() {
        // R705.1 regression guard. A focused row inside a Scroll carries
        // a scroll-LOCAL rect; the top-level ring overlay must be placed
        // at the row's WINDOW-ABSOLUTE position (translated by the scroll
        // offset), not at the raw scroll-local rect. Pre-R705.1 the ring
        // was drawn at the local rect → tens of pixels off on screen for
        // every scrolled widget.
        use pinion_core::scene::ScrollNode;
        // viewport at (10, 100), scrolled down 50px. A row at scroll-local
        // (0, 60) therefore paints at window-absolute (10, 60 + (100-50)).
        let row = tagged_box(0, 60, 100, 30, 0, "row#3");
        let content = container(vec![row]);
        let scroll = Scene::Scroll(ScrollNode::new(Rect::new(10, 100, 200, 200), content)
            .with_offset(0, 50));
        let scene = container(vec![scroll]);
        let out = inject_focus_ring(scene, Some("row#3"), FocusRingStyle::default());
        let ring = ring_child(&out);
        // window-abs row = (0+10, 60+(100-50)) = (10, 110); ring = +/-2.
        assert_eq!(
            ring.rect,
            Rect::new(8, 108, 104, 34),
            "ring frames the scrolled row in window-absolute space",
        );
    }

    #[test]
    fn lone_node_root_is_wrapped() {
        // Single-widget paint root (bare Box, no Container). The ring
        // needs a Container to live in alongside the original content.
        let scene = tagged_box(0, 0, 40, 40, 0, "btn");
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default());
        let Scene::Container(c) = &out else { panic!("lone root wrapped into Container") };
        assert_eq!(c.children.len(), 2, "original + ring");
    }
}
