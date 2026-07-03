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
/// `scene`. `focused_tag` is the §5.40 `pinion_runtime::FocusManager`
/// focused tag — the same tag the input router hit-tests against,
/// including the R51.42 composite `tag#sub` form a roving
/// active-descendant widget (`RadioGroup`, the R704 datepicker grid)
/// paints its focused cell with.
///
/// The ring box is the focused node's post-layout rect inflated by
/// `style.offset` on every side (clamped at all four framebuffer edges
/// — top/left near via R806, bottom/right far via R1022 using the
/// `viewport` extent — so a flush widget keeps all four ring strokes
/// on-screen; a widget fully off the far edge yields no ring, see
/// `build_focus_ring_box`), with a border of `style.stroke` /
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
pub fn inject_focus_ring(
    scene: Scene,
    focused_tag: Option<&str>,
    style: FocusRingStyle,
    viewport: Option<(u32, u32)>,
) -> Scene {
    let Some(tag) = focused_tag else {
        return scene;
    };
    // Window-absolute, viewport-clipped rect so the top-level overlay box
    // lands where the widget actually paints — even inside a Scroll.
    let Some(rect) = scene.rect_for_tag_absolute(tag) else {
        return scene;
    };
    let corner_radius = corner_radius_for_tag(&scene, tag);
    // `viewport` = the layout viewport `(w, h)` the shell fed `compute_layout`
    // (R1006 canon; `None` = unknown/headless): the ring's far (bottom/right)
    // edges clamp into it so a widget flush against the window's bottom/right
    // edge keeps all four ring strokes on-screen (R1022 far-edge sibling of the
    // R806 near-edge clamp). A widget fully off the far edge yields no ring.
    let Some(ring) = build_focus_ring_box(rect, corner_radius, style, viewport) else {
        return scene;
    };
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

fn build_focus_ring_box(
    target: Rect,
    target_radius: u32,
    style: FocusRingStyle,
    viewport: Option<(u32, u32)>,
) -> Option<BoxNode> {
    let off = style.offset;
    // Concentric outset, boundary-clipped on all four edges (R806 near +
    // R1022 far §5.39), with the framebuffer-edge clamps owned by [`crate::edge`].
    //
    // Near (top/left): `Rect` origins are unsigned, so a widget flush against the
    // top/left edge (`x` or `y` < `off`) cannot carry the negative origin a full
    // outward offset needs. We clamp the origin here and shrink the span by the
    // *same* amount so the far edge still lands at `target + off` — the ring stays
    // concentric and the framebuffer edge clips the lost near gap.
    let x = target.x.saturating_sub(off);
    let y = target.y.saturating_sub(off);
    let ideal_right = target.x.saturating_add(target.w).saturating_add(off);
    let ideal_bottom = target.y.saturating_add(target.h).saturating_add(off);
    let ideal_rect = Rect::new(
        x,
        y,
        ideal_right.saturating_sub(x),
        ideal_bottom.saturating_sub(y),
    );
    // Far (bottom/right): unlike the near edge — which has outward room and keeps
    // the far edge concentric — a widget flush against the bottom/right edge has
    // no outward room, so the ring INSETS to land on-screen (the same shape as the
    // top flood-row inset, not a mirror of the near `saturating_sub`). The default
    // `Inside` border alignment draws the whole stroke within the rect, so a far
    // edge clamped to the viewport lands the stroke fully visible. `None` viewport
    // (headless / pure geometry) leaves the ring un-clamped. Then the top stroke
    // is nudged off the vello `y = 0` flood row (R807). Both clamps live in
    // [`crate::edge`] — the single home for overlay framebuffer-edge handling.
    let far_clamped = crate::edge::clamp_far_edges_into_viewport(ideal_rect, viewport);
    let ring_rect = crate::edge::clamp_top_off_flood_row(far_clamped);
    // R1022 §5.39 — a focused widget laid out (nearly) fully past the far edge, or
    // a sub-`off` viewport, clamps to a zero-span rect. Skip rather than push a
    // degenerate box: an off-screen widget has no visible ring, matching the
    // `rect_for_tag_absolute -> None` skip for a scrolled-out widget. This is the
    // case the pre-R1022 always-positive span could never reach.
    if ring_rect.w == 0 || ring_rect.h == 0 {
        return None;
    }
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
    Some(node)
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
        let Scene::Container(c) = scene else {
            panic!("expected Container")
        };
        let ring = c
            .children
            .iter()
            .find(|ch| ch.tag() == Some(FOCUS_RING_TAG))
            .expect("ring child present");
        let Scene::Box(b) = ring else {
            panic!("ring is a Box")
        };
        b
    }

    #[test]
    fn none_focus_is_unchanged_no_op() {
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let out = inject_focus_ring(scene, None, FocusRingStyle::default(), None);
        let Scene::Container(c) = &out else { panic!() };
        assert_eq!(c.children.len(), 1, "no ring when nothing focused");
    }

    #[test]
    fn unknown_tag_is_unchanged_no_op() {
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("ghost"), FocusRingStyle::default(), None);
        let Scene::Container(c) = &out else { panic!() };
        assert_eq!(c.children.len(), 1, "no ring when focused tag absent");
    }

    #[test]
    fn ring_inflated_by_offset_around_target() {
        let scene = container(vec![tagged_box(100, 50, 40, 30, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default(), None);
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
        let out = inject_focus_ring(scene, Some("menu#t0"), FocusRingStyle::default(), None);
        let ring = ring_child(&out);
        assert_eq!(
            ring.rect,
            Rect::new(0, 1, 98, 41),
            "concentric, top inset off y=0"
        );
        // The far edges keep the full outward offset (concentric); only the
        // near gap is clipped (left by the framebuffer, top by the inset).
        assert_eq!(
            ring.rect.x + ring.rect.w,
            98,
            "right edge = widget right (96) + 2"
        );
        assert_eq!(
            ring.rect.y + ring.rect.h,
            42,
            "bottom edge = widget bottom (40) + 2"
        );
        assert!(
            ring.rect.y >= crate::edge::TOP_EDGE_INSET,
            "top stroke kept off the y=0 flood row",
        );
    }

    #[test]
    fn ring_clips_only_the_overflowing_axis() {
        // A widget flush against the TOP edge only (y = 0) but well clear of
        // the left edge (x = 200): the y-axis insets to TOP_EDGE_INSET while
        // the x-axis keeps its symmetric outset. Proves the clamp is
        // per-axis, not a whole-rect shift.
        let scene = container(vec![tagged_box(200, 0, 40, 30, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default(), None);
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
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default(), None);
        let ring = ring_child(&out);
        // y: max(1, 1-2)=1. h: (1+30+2)-1 = 32.
        assert_eq!(ring.rect, Rect::new(48, 1, 44, 32));
        assert_eq!(
            ring.rect.y + ring.rect.h,
            33,
            "bottom = widget bottom (31) + 2"
        );
    }

    #[test]
    fn ring_clamps_far_edges_into_framebuffer_at_bottom_right_corner() {
        // R1022 far-edge guard (sibling of R806's near-edge
        // `ring_stays_concentric_at_top_left_flush_corner`). A full-bleed pane
        // filling a 200x100 framebuffer (the sprag undock terminal): a naive
        // +2 outset pushes the far edges to right=202 / bottom=102, off-screen,
        // so the bottom + right strokes rasterise past the framebuffer and
        // vanish. The far clamp pulls them back to the framebuffer extent so
        // all four strokes land on-screen.
        let scene = container(vec![tagged_box(0, 0, 200, 100, 0, "pane")]);
        let out = inject_focus_ring(
            scene,
            Some("pane"),
            FocusRingStyle::default(),
            Some((200, 100)),
        );
        let ring = ring_child(&out);
        // x:0 (left flush), y:1 (top flush, off the flood row),
        // right: min(202, 200)=200, bottom: min(102, 100)=100.
        assert_eq!(ring.rect, Rect::new(0, 1, 200, 99));
        assert_eq!(
            ring.rect.x + ring.rect.w,
            200,
            "right stroke inside the framebuffer"
        );
        assert_eq!(
            ring.rect.y + ring.rect.h,
            100,
            "bottom stroke inside the framebuffer"
        );
        assert!(ring.rect.x + ring.rect.w <= 200 && ring.rect.y + ring.rect.h <= 100);
    }

    #[test]
    fn ring_far_clamp_is_per_axis() {
        // A widget flush against the BOTTOM edge only (bottom at fb_h = 100) but
        // well clear of the right edge: the bottom insets to the framebuffer
        // while the right keeps its symmetric outset. Proves the far clamp is
        // per-axis, not a whole-rect shift (mirror of the near-edge
        // `ring_clips_only_the_overflowing_axis`).
        let scene = container(vec![tagged_box(50, 80, 40, 20, 0, "btn")]);
        let out = inject_focus_ring(
            scene,
            Some("btn"),
            FocusRingStyle::default(),
            Some((200, 100)),
        );
        let ring = ring_child(&out);
        // x:48, right: min(50+40+2, 200)=92 (full symmetric outset, w=44).
        // y:78, bottom: min(80+20+2, 100)=100 (clamped, h=22).
        assert_eq!(ring.rect, Rect::new(48, 78, 44, 22));
        assert_eq!(
            ring.rect.x + ring.rect.w,
            92,
            "right stays concentric (widget right 90 + 2)"
        );
        assert_eq!(
            ring.rect.y + ring.rect.h,
            100,
            "bottom clamps to the framebuffer"
        );
    }

    #[test]
    fn ring_far_clamp_disabled_when_viewport_unknown() {
        // `None` viewport (unknown / headless / pure-geometry callers) disables
        // the far clamp — the ring keeps the concentric outset exactly as the
        // pre-R1022 behaviour, never collapsed against an unmeasured extent.
        let scene = container(vec![tagged_box(100, 50, 40, 30, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default(), None);
        let ring = ring_child(&out);
        assert_eq!(
            ring.rect,
            Rect::new(98, 48, 44, 34),
            "no far clamp when viewport unknown"
        );
    }

    #[test]
    fn ring_skipped_when_widget_is_past_the_far_edge() {
        // R1022.1 — a focused widget laid out (nearly) fully beyond the far edge
        // (e.g. a top-level child wider than the window). The far clamp would
        // collapse the ring to a zero span; rather than push a degenerate box,
        // the ring is skipped — an off-screen widget has no visible ring, matching
        // the `rect_for_tag_absolute -> None` scrolled-out skip. The pre-R1022
        // always-positive span could never reach this.
        let scene = container(vec![tagged_box(250, 10, 40, 30, 0, "btn")]);
        let out = inject_focus_ring(
            scene,
            Some("btn"),
            FocusRingStyle::default(),
            Some((200, 100)),
        );
        let Scene::Container(c) = &out else {
            panic!("expected Container")
        };
        assert!(
            c.children.iter().all(|ch| ch.tag() != Some(FOCUS_RING_TAG)),
            "no ring for a widget past the far edge (degenerate span skipped)",
        );
    }

    #[test]
    fn ring_far_clamp_at_zero_offset() {
        // `offset = 0`: the ring traces the widget rect exactly. A full-bleed
        // widget at zero offset still clamps far edges into the viewport and
        // still insets the top off the flood row — all four strokes on-screen.
        let style = FocusRingStyle::new().with_offset(0);
        let scene = container(vec![tagged_box(0, 0, 200, 100, 0, "pane")]);
        let out = inject_focus_ring(scene, Some("pane"), style, Some((200, 100)));
        let ring = ring_child(&out);
        // ideal = widget exactly (0,0,200,100); far clamp is a no-op (already at
        // the viewport); top flood-row inset -> (0,1,200,99).
        assert_eq!(ring.rect, Rect::new(0, 1, 200, 99));
        assert_eq!(ring.rect.x + ring.rect.w, 200, "right on-screen");
        assert_eq!(ring.rect.y + ring.rect.h, 100, "bottom on-screen");
    }

    #[test]
    fn ring_radius_tracks_rounded_target_concentrically() {
        // Circular day-cell: 40x40, radius 20 (the R704 datepicker
        // geometry). The ring must be rounded too — radius 20 + 2px
        // offset = 22 — not a sharp square.
        let scene = container(vec![tagged_box(0, 0, 40, 40, 20, "day#15")]);
        let out = inject_focus_ring(scene, Some("day#15"), FocusRingStyle::default(), None);
        let ring = ring_child(&out);
        assert_eq!(ring.style.corner_radius, 22, "concentric rounded ring");
    }

    #[test]
    fn ring_stays_sharp_for_sharp_target() {
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default(), None);
        let ring = ring_child(&out);
        assert_eq!(ring.style.corner_radius, 0, "sharp widget keeps sharp ring");
    }

    #[test]
    fn ring_is_pointer_transparent_and_transparent_fill() {
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default(), None);
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
        let out = inject_focus_ring(scene, Some("btn"), style, None);
        let ring = ring_child(&out);
        let b = ring.style.border.expect("border emitted");
        assert_eq!(b.color, Color::rgb(255, 0, 0));
        assert_eq!(b.width, 3);
    }

    #[test]
    fn reinjection_replaces_not_duplicates() {
        let scene = container(vec![tagged_box(0, 0, 40, 40, 0, "btn")]);
        let once = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default(), None);
        let twice = inject_focus_ring(once, Some("btn"), FocusRingStyle::default(), None);
        let Scene::Container(c) = &twice else {
            panic!()
        };
        let rings = c
            .children
            .iter()
            .filter(|ch| ch.tag() == Some(FOCUS_RING_TAG))
            .count();
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
        let scroll = Scene::Scroll(
            ScrollNode::new(Rect::new(10, 100, 200, 200), content).with_offset(0, 50),
        );
        let scene = container(vec![scroll]);
        let out = inject_focus_ring(scene, Some("row#3"), FocusRingStyle::default(), None);
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
        let out = inject_focus_ring(scene, Some("btn"), FocusRingStyle::default(), None);
        let Scene::Container(c) = &out else {
            panic!("lone root wrapped into Container")
        };
        assert_eq!(c.children.len(), 2, "original + ring");
    }
}
