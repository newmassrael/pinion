//! §5.33 §5.51 R1113 — drag-image (the "pick up an object" follower) as an
//! introspectable overlay Scene node.
//!
//! ## Why this module exists
//!
//! When a user grabs an object and drags it (a dock panel header, a list row),
//! a desktop UI floats a translucent **drag image** under the cursor — the
//! immediate "I picked this up" feedback (VS Code / Blender / browser tabs).
//! pinion lacked it: a dock-panel drag showed only the target zone highlight,
//! nothing followed the cursor (the handoff's ① complaint).
//!
//! The textbook home is the **shell overlay layer**, exactly where the
//! keyboard focus ring lives ([`crate::focus_ring`]): the shell already owns
//! the live drag session (the [`pinion_runtime::InputRouter`] holds the
//! in-flight payload + cursor) and injects window-level overlays at paint
//! time. So the drag image is a sibling of the focus ring — a
//! pointer-transparent, top-level overlay the shell injects from router state,
//! **not** a per-widget composition. This makes it automatic for every
//! consumer (no per-binding wiring) and correctly anchored at the
//! window-logical cursor the router measured, and it needs no new reactive
//! signal (it is a pure projection of the drag session, the way the focus
//! ring is a pure projection of focus state).
//!
//! The result mirrors [`crate::focus_ring`]: (a) introspectable via
//! `scene/snapshot` (§2 #7 — an AI observes the same follower the user sees),
//! (b) a real Scene node painted by the generic box/text path (§2 #1), and
//! (c) pointer-transparent (§5.39) so it never shadows input under it.

use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{BoxStyle, Color, TextStyle};

use crate::highlight::{push_top_level, strip_tag, wrap_into_container};

/// Tag carried by the injected drag-image overlay container. Shares the
/// `ai-overlay/` family prefix with [`crate::HIGHLIGHT_TAG_PREFIX`] /
/// [`crate::FOCUS_RING_TAG`] so the same "strip every overlay node" discipline
/// applies; a single fixed tag (there is at most one in-flight drag image per
/// window, like one focus ring).
pub const DRAG_IMAGE_TAG: &str = "ai-overlay/drag-image";

/// Visual style of the drag image (the translucent follower chip). Mirrors
/// [`crate::FocusRingStyle`]: hard-coded neutral defaults (the overlay layer
/// has no [`Theme`](pinion_core::theme::Theme)), overridable by the binding
/// through the [`WidgetView::drag_image_style`] hook so a binding can theme it
/// (or suppress the follower by returning `None`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragImageStyle {
    /// Chip fill colour (ARGB). Semi-opaque by default so the label reads
    /// while the content underneath stays faintly visible — a drag image, not
    /// the relocated object.
    pub fill: Color,
    /// Label text colour (ARGB).
    pub text: Color,
    /// Label font size (logical px).
    pub font_px: u32,
    /// Inner padding around the label (logical px, every side).
    pub padding: u32,
    /// Pixels the chip's top-left trails the cursor (so the follower sits
    /// beside the pointer, not under it).
    pub cursor_offset: u32,
}

impl DragImageStyle {
    /// Default chip: a semi-opaque dark surface with white label — a neutral,
    /// theme-agnostic tooltip-like follower. A binding themes it (or returns
    /// `None` to suppress) via [`WidgetView::drag_image_style`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fill: Color::rgba(32, 33, 36, 0xE6),
            text: Color::rgb(255, 255, 255),
            font_px: 14,
            padding: 6,
            cursor_offset: 12,
        }
    }

    /// Builder: override the chip fill colour.
    #[must_use]
    pub const fn with_fill(mut self, fill: Color) -> Self {
        self.fill = fill;
        self
    }

    /// Builder: override the label text colour.
    #[must_use]
    pub const fn with_text(mut self, text: Color) -> Self {
        self.text = text;
        self
    }

    /// Builder: override the label font size (logical px).
    #[must_use]
    pub const fn with_font_px(mut self, font_px: u32) -> Self {
        self.font_px = font_px;
        self
    }
}

impl Default for DragImageStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// (R1113 §5.51 §5.33) The chip's outer rect for a `label` dragged to
/// `cursor`, clamped whole into `viewport`. Width estimates one em per
/// character (CJK-safe — it never under-sizes, so the label never clips; a
/// slightly wide chip for Latin is harmless for a transient follower); height
/// is one line. The chip trails the cursor by `cursor_offset`, then shifts
/// left/up so the WHOLE chip stays on-screen (a tooltip-style flip, not the
/// ring's shrink-to-fit), and the top is nudged off the vello `y = 0` flood
/// row (VELLO-001).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "window-logical cursor is non-negative; sub-pixel rounding is irrelevant to a drag-image anchor; a label is never billions of chars"
)]
fn drag_image_rect(
    label: &str,
    cursor: (f64, f64),
    style: DragImageStyle,
    viewport: Option<(u32, u32)>,
) -> Rect {
    let pad = style.padding;
    let w = pad.saturating_mul(2) + (label.chars().count() as u32).saturating_mul(style.font_px);
    let h = pad.saturating_mul(2) + style.font_px;
    let cx = cursor.0.max(0.0).round() as u32;
    let cy = cursor.1.max(0.0).round() as u32;
    let mut x = cx.saturating_add(style.cursor_offset);
    let mut y = cy.saturating_add(style.cursor_offset);
    // Shift the whole chip back on-screen if trailing the cursor pushed it past
    // the far edge — keep it intact (don't shrink it; a clipped label is worse
    // than a chip that flips to the cursor's other side).
    if let Some((vw, vh)) = viewport {
        if x.saturating_add(w) > vw {
            x = vw.saturating_sub(w);
        }
        if y.saturating_add(h) > vh {
            y = vh.saturating_sub(h);
        }
    }
    crate::edge::clamp_top_off_flood_row(Rect::new(x, y, w, h))
}

/// Inject the drag image — a translucent labelled chip — at `cursor` inside
/// `scene`. The shell calls this at paint time (a sibling of
/// [`crate::inject_focus_ring`]) from the [`pinion_runtime::InputRouter`]'s
/// live drag session: `label` is the dragged payload's text, `cursor` the
/// window-logical pointer the router measured, `viewport` the layout extent
/// `(w, h)` (so the chip stays on-screen).
///
/// The chip is a [`Scene::Container`] (fill + label [`Scene::Text`]) carrying
/// [`DRAG_IMAGE_TAG`], with **explicit post-layout rects** (the overlay is
/// injected after `compute_layout`, like the focus ring, so it carries its own
/// geometry rather than re-running layout) and `pointer_transparent` so it
/// never shadows the drag it represents.
///
/// Idempotent: any pre-existing drag image (same [`DRAG_IMAGE_TAG`]) is
/// stripped before the fresh one is appended, so re-injecting replaces rather
/// than duplicates. An empty `label` is a no-op (nothing to show).
#[must_use]
pub fn inject_drag_image(
    scene: Scene,
    label: &str,
    cursor: (f64, f64),
    style: DragImageStyle,
    viewport: Option<(u32, u32)>,
) -> Scene {
    if label.is_empty() {
        return scene;
    }
    let chip_rect = drag_image_rect(label, cursor, style, viewport);
    let pad = style.padding;
    let inner = Rect::new(
        chip_rect.x.saturating_add(pad),
        chip_rect.y.saturating_add(pad),
        chip_rect.w.saturating_sub(pad.saturating_mul(2)),
        chip_rect.h.saturating_sub(pad.saturating_mul(2)),
    );
    let label_node = Scene::Text(TextNode::styled(
        label.to_string(),
        inner,
        TextStyle::new()
            .with_size_px(style.font_px)
            .with_fg(style.text),
    ));
    let mut chip = ContainerNode::new(vec![label_node]);
    chip.rect = chip_rect;
    chip.style = BoxStyle::filled(style.fill);
    chip.tag = Some(DRAG_IMAGE_TAG.into());
    chip.layout = chip.layout.with_pointer_transparent(true);

    let mut wrapped = wrap_into_container(scene);
    strip_tag(&mut wrapped, DRAG_IMAGE_TAG);
    push_top_level(&mut wrapped, Scene::Container(chip));
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ContainerNode;

    fn container(children: Vec<Scene>) -> Scene {
        let mut c = ContainerNode::new(children);
        c.rect = Rect::new(0, 0, 1000, 1000);
        Scene::Container(c)
    }

    fn chip_child(scene: &Scene) -> &ContainerNode {
        let Scene::Container(c) = scene else {
            panic!("expected Container")
        };
        let chip = c
            .children
            .iter()
            .find(|ch| ch.tag() == Some(DRAG_IMAGE_TAG))
            .expect("drag-image child present");
        let Scene::Container(b) = chip else {
            panic!("drag image is a Container")
        };
        b
    }

    #[test]
    fn empty_label_is_unchanged_no_op() {
        let scene = container(vec![]);
        let out = inject_drag_image(scene, "", (100.0, 100.0), DragImageStyle::default(), None);
        let Scene::Container(c) = &out else { panic!() };
        assert!(c.children.is_empty(), "no chip for an empty label");
    }

    #[test]
    fn chip_trails_cursor_by_offset() {
        let scene = container(vec![]);
        let out = inject_drag_image(
            scene,
            "outliner",
            (300.0, 200.0),
            DragImageStyle::default(),
            None,
        );
        let chip = chip_child(&out);
        // top-left = cursor + offset (12).
        assert_eq!(chip.rect.x, 312, "chip x trails cursor by the offset");
        assert_eq!(chip.rect.y, 212, "chip y trails cursor by the offset");
        assert!(
            chip.rect.w > 0 && chip.rect.h > 0,
            "chip has a non-zero box"
        );
    }

    #[test]
    fn chip_is_pointer_transparent_and_filled_translucent() {
        let scene = container(vec![]);
        let out = inject_drag_image(
            scene,
            "panel",
            (10.0, 10.0),
            DragImageStyle::default(),
            None,
        );
        let chip = chip_child(&out);
        assert!(
            chip.layout.pointer_transparent,
            "the follower must not shadow the drag it represents",
        );
        assert!(
            chip.style.fill.a > 0 && chip.style.fill.a < 255,
            "translucent fill"
        );
    }

    #[test]
    fn chip_carries_the_label_text() {
        let scene = container(vec![]);
        let out = inject_drag_image(
            scene,
            "viewport",
            (50.0, 60.0),
            DragImageStyle::default(),
            None,
        );
        let chip = chip_child(&out);
        let Scene::Text(t) = &chip.children[0] else {
            panic!("chip carries its label as Text")
        };
        assert_eq!(t.content, "viewport");
        // the label sits inside the chip (padded).
        assert!(t.rect.x >= chip.rect.x && t.rect.y >= chip.rect.y);
    }

    #[test]
    fn chip_shifts_left_to_stay_on_screen_near_right_edge() {
        // Cursor near the right edge: trailing the chip would push it off-screen,
        // so the whole chip shifts left to keep its right edge inside the viewport.
        let scene = container(vec![]);
        let out = inject_drag_image(
            scene,
            "panel",
            (195.0, 50.0),
            DragImageStyle::default(),
            Some((200, 100)),
        );
        let chip = chip_child(&out);
        assert!(
            chip.rect.x + chip.rect.w <= 200,
            "chip kept whole inside the viewport right edge (x={}, w={})",
            chip.rect.x,
            chip.rect.w,
        );
    }

    #[test]
    fn chip_shifts_up_to_stay_on_screen_near_bottom_edge() {
        let scene = container(vec![]);
        let out = inject_drag_image(
            scene,
            "panel",
            (50.0, 98.0),
            DragImageStyle::default(),
            Some((200, 100)),
        );
        let chip = chip_child(&out);
        assert!(
            chip.rect.y + chip.rect.h <= 100,
            "chip kept whole inside the viewport bottom edge (y={}, h={})",
            chip.rect.y,
            chip.rect.h,
        );
    }

    #[test]
    fn reinjection_replaces_not_duplicates() {
        let scene = container(vec![]);
        let once = inject_drag_image(scene, "a", (10.0, 10.0), DragImageStyle::default(), None);
        let twice = inject_drag_image(once, "a", (20.0, 20.0), DragImageStyle::default(), None);
        let Scene::Container(c) = &twice else {
            panic!()
        };
        let chips = c
            .children
            .iter()
            .filter(|ch| ch.tag() == Some(DRAG_IMAGE_TAG))
            .count();
        assert_eq!(chips, 1, "exactly one drag image after double injection");
    }

    #[test]
    fn wider_label_yields_a_wider_chip() {
        // The chip sizes to its label (one em per char), so a longer label is
        // wider — the estimate never under-sizes (CJK-safe), so the label fits.
        let s = container(vec![]);
        let short = chip_child(&inject_drag_image(
            s,
            "ab",
            (0.0, 0.0),
            DragImageStyle::default(),
            None,
        ))
        .rect
        .w;
        let s2 = container(vec![]);
        let long = chip_child(&inject_drag_image(
            s2,
            "abcdefgh",
            (0.0, 0.0),
            DragImageStyle::default(),
            None,
        ))
        .rect
        .w;
        assert!(
            long > short,
            "a longer label produces a wider chip ({long} > {short})"
        );
    }
}
