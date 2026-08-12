//! R1662 §5.45 §5.21 — a pane whose content may not fit it.
//!
//! Assembling one by hand is three decisions, and every consumer that made them
//! separately got a different subset right: the clip window, the content extent
//! the scroll range is derived from, and the binding to the [`ScrollState`] the
//! input router writes. The analysis-tool screens made none of them — their
//! panes are plain containers, so a control below the fold is painted outside
//! the window and cannot be reached at all, which is what
//! `debt-the-node-lab-panes-do-not-scroll` and
//! `debt-the-analyzer-canvas-does-not-scroll` each record from a different
//! screen.
//!
//! # The extent is derived, not declared
//!
//! [`content_extent`] reads the children the caller is already handing over.
//! That is the whole point: the number a consumer would otherwise have to keep
//! in step with its own layout is the number that goes stale, and a scroll
//! range that is short does not fail loudly — the content past it is simply
//! never reachable, silently, which is the defect R1653.1 fixed by hand on one
//! surface.
//!
//! The reference toolkit spells this `setWidgetResizable(true)`, and measured
//! at 6.11.1 it derives the range from the laid-out child and keeps it live as
//! the child grows. What it has no equivalent of is the other half:
//! `scene/scroll_reach` publishes, per mark, whether any offset in the derived
//! range reaches it — so a pane whose extent is *still* short is a finding
//! rather than an absence. On the reference an area that never set its range
//! reports `maximum() == 0`, byte-identical to a pane whose content genuinely
//! fits.
//!
//! # The extent is a floor, not a ceiling
//!
//! It is lowered to `min_size`, so an in-flow child that lays out taller than
//! its declared rectangle grows the content rather than being cut by it. The
//! union can only under-report — a direct child that is itself a flex container
//! overflowing its own box contributes its declared size, not its laid-out one
//! — and under-reporting is the direction the `scene/scroll_reach` gate
//! catches. Over-reporting would scroll into empty space, which nothing catches.

use std::rc::Rc;

use pinion_core::scene::{ContainerNode, Rect, Scene, ScrollAxis, ScrollNode};
use pinion_core::style::{LayoutStyle, Size, SizeValue};
use pinion_core::widgets::scroll::ScrollState;

/// The box a child DECLARES, before any layout pass has run.
///
/// ★ Not [`Scene::rect`]. That field is where the layout pass WROTE the node,
/// and a view function runs before it: on a freshly composed subtree every
/// `rect` is still zero, so a union over them answers `(0, 0)` for a pane full
/// of content. Measured on the first run of [`content_extent`] — it reported
/// no extent at all for two absolutely-placed children, which would have made
/// every pane here a viewport onto nothing.
///
/// The declaration lives in the layout style: `absolute_position` for the
/// origin and a pixel `size` for the extent. Anything the style leaves `Auto`
/// falls back to the node's own rectangle, which is the right answer for a
/// subtree that HAS been laid out and for the primitives that carry their
/// geometry directly.
fn declared_box(child: &Scene) -> Rect {
    let r = child.rect();
    let Some(layout) = child.layout_style() else {
        return r;
    };
    let (x, y) = layout.absolute_position.map_or((r.x, r.y), |(x, y)| (x, y));
    let px = |v: SizeValue, fallback: u32| match v {
        SizeValue::Px(n) => n,
        _ => fallback,
    };
    Rect::new(
        x,
        y,
        px(layout.size.width, r.w),
        px(layout.size.height, r.h),
    )
}

/// How far the content reaches, from the children themselves.
///
/// The union of the **direct** children's declared rectangles. Direct only,
/// because a descendant's rectangle is stated in its own parent's frame and
/// folding those needs the layout pass this runs before; the caller of a pane
/// places its children in pane coordinates, which is exactly the frame this
/// union is taken in.
///
/// Zero for no children — a pane with nothing in it needs no range.
#[must_use]
pub fn content_extent(children: &[Scene]) -> (u32, u32) {
    children.iter().fold((0, 0), |(w, h), child| {
        let r = declared_box(child);
        (w.max(r.x + r.w), h.max(r.y + r.h))
    })
}

/// Whether presses inside a pane are the pane's business.
///
/// ★ R1662 — not a detail: a tagged node the hit test resolves, with no
/// `External` behind that tag, makes the router forward nothing and the press
/// disappears without a trace. That is R1655's law, and adding a pane to a
/// screen that routes every press through one root `External` reproduced it on
/// two panes at once, caught by that screen's own gate.
///
/// The scroll node's tag is unaffected either way, and so is the wheel: the
/// wheel resolves the innermost scroll node by geometry
/// (`Scene::scroll_target_at`) and never reads this flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanePointer {
    /// The scene's hit test resolves the pane and the widgets in it — the
    /// ordinary case, and what a pane full of framework widgets wants.
    Targets,
    /// The pane and everything under it is invisible to the hit test, so a
    /// press reaches whatever is behind. What a screen that routes every press
    /// through one `External` needs, because there is nothing else for the
    /// router to deliver to.
    PassesThrough,
}

/// A pane that scrolls when its content does not fit, with the range derived
/// from the content.
///
/// `viewport` is the clip window in the enclosing frame. `children` are placed
/// in pane coordinates — the same coordinates they would use in a plain
/// container, so adopting this on an existing pane moves no rectangle.
///
/// `gutter` is trailing space past the content on each axis, for the same
/// reason every document view has one: a last row flush against the bottom edge
/// reads as cut off even when it is whole.
#[must_use]
pub fn scroll_pane(
    state: &Rc<ScrollState>,
    viewport: Rect,
    gutter: (u32, u32),
    pointer: PanePointer,
    children: Vec<Scene>,
) -> Scene {
    let (w, h) = content_extent(&children);
    let content = Scene::Container(
        ContainerNode::new(children)
            .with_layout(LayoutStyle::new().with_min_size(Size::px(w + gutter.0, h + gutter.1))),
    );
    let node =
        ScrollNode::from_state(Rc::clone(state), viewport, content).with_axis(ScrollAxis::Both);
    let layout = node.layout.clone();
    // ★ R1672 — the viewport's ORIGIN is honoured, not only its size. It was
    // used for the extent alone and the node took its position from the flow,
    // so a caller that handed a pane its CONTENT rectangle (inset by the frame
    // the pane draws) got a body that was the right size in the wrong place:
    // measured, the right and bottom overhangs closed and the left and top
    // stayed at one pixel each. A rectangle is a position and a size, and a
    // consumer that passes one has said both.
    let placed = layout
        .with_pointer_transparent(pointer == PanePointer::PassesThrough)
        .with_absolute_position(viewport.x, viewport.y);
    Scene::Scroll(node.with_layout(placed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R1672 — the viewport's ORIGIN is honoured, not only its size.
    ///
    /// It was used for the extent alone and the node took its position from the
    /// flow, so a caller handing a pane its CONTENT rectangle (inset by the
    /// frame the pane draws) got a body of the right size in the wrong place:
    /// measured on two screens, the right and bottom overhangs closed and the
    /// left and top stayed at one pixel each.
    ///
    /// A rectangle is a position AND a size, and a consumer that passes one has
    /// said both. Asserted here because the change is a contract this crate
    /// makes to twelve call sites, and a contract nothing checks is a comment.
    #[test]
    fn r1672_the_viewport_origin_places_the_pane() {
        use pinion_core::reactive::Owner;
        use pinion_core::scene::Rect;
        use pinion_core::widgets::scroll::ScrollState;

        let owner = Owner::new();
        owner.run(|| {
            let state = std::rc::Rc::new(ScrollState::with_tag("probe"));
            let placed = scroll_pane(
                &state,
                Rect::new(7, 11, 80, 40),
                (0, 0),
                PanePointer::PassesThrough,
                Vec::new(),
            );
            let layout = placed
                .layout_style()
                .expect("a scroll node carries a layout sidecar");
            assert_eq!(
                layout.absolute_position,
                Some((7, 11)),
                "the pane is placed where its viewport says, not at the flow's origin",
            );
        });
    }

    use pinion_core::reactive::Owner;
    use pinion_core::style::BoxStyle;
    use pinion_core::widgets::scroll::use_scroll_state;

    fn boxed(rect: Rect) -> Scene {
        Scene::Container(
            ContainerNode::new(Vec::new())
                .with_style(BoxStyle::filled(pinion_core::style::Color::rgb(0, 0, 0)))
                .with_layout(
                    LayoutStyle::new()
                        .with_absolute_position(rect.x, rect.y)
                        .with_size(Size::px(rect.w, rect.h)),
                ),
        )
    }

    /// ★ The extent comes from the children, which is the number a consumer
    /// would otherwise keep in step with its own layout by hand.
    #[test]
    fn r1662_the_extent_is_the_union_of_the_children() {
        assert_eq!(
            content_extent(&[
                boxed(Rect::new(0, 0, 40, 20)),
                boxed(Rect::new(10, 300, 40, 20))
            ]),
            (50, 320)
        );
    }

    /// ★ An empty pane needs no range, and says so as a number rather than by
    /// omission.
    #[test]
    fn r1662_an_empty_pane_has_no_extent() {
        assert_eq!(content_extent(&[]), (0, 0));
    }

    /// ★ The extent is lowered as a floor: an in-flow child that lays out
    /// taller than its declared box grows the content instead of being cut.
    #[test]
    fn r1662_the_extent_is_a_minimum_not_a_size() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            scroll_pane(
                &use_scroll_state("pane"),
                Rect::new(0, 0, 100, 100),
                (0, 8),
                PanePointer::Targets,
                vec![boxed(Rect::new(0, 200, 40, 20))],
            )
        });
        let Scene::Scroll(s) = &scene else {
            panic!("a pane is a scroll node: {scene:?}");
        };
        let Scene::Container(c) = s.content.as_ref() else {
            panic!("content is a container");
        };
        assert_eq!(c.layout.min_size.height, SizeValue::Px(228));
        assert_eq!(c.layout.size.height, SizeValue::Auto, "a floor, not a size");
    }

    /// ★★ The claim [`PanePointer::PassesThrough`] rests on, as a test: a pane
    /// the hit test skips is still the pane the WHEEL finds.
    ///
    /// The two resolutions are different walks — the hit test honours
    /// transparency and `Scene::scroll_target_at` is pure geometry — and a pane
    /// that let presses through while swallowing the wheel would be a worse
    /// defect than the one the flag exists to avoid. Asserted rather than read
    /// off the two functions, because "these two walks differ" is exactly the
    /// kind of claim that is true until someone unifies them.
    #[test]
    fn r1662_a_pane_that_passes_presses_through_still_takes_the_wheel() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            scroll_pane(
                &use_scroll_state("body"),
                Rect::new(0, 0, 100, 100),
                (0, 0),
                PanePointer::PassesThrough,
                vec![boxed(Rect::new(0, 0, 40, 400))],
            )
        });
        assert!(
            scene.is_pointer_transparent(),
            "the press must reach what is behind"
        );
        let found = scene
            .scroll_target_at(50, 50)
            .expect("the wheel resolves the pane under the cursor");
        assert_eq!(found.tag.as_deref(), Some("body"));
        // And the other arm is a hit target, so a pane of widgets works.
        let opaque = owner.run(|| {
            scroll_pane(
                &use_scroll_state("targets"),
                Rect::new(0, 0, 100, 100),
                (0, 0),
                PanePointer::Targets,
                vec![boxed(Rect::new(0, 0, 40, 400))],
            )
        });
        assert!(!opaque.is_pointer_transparent());
    }

    /// ★ The pane is bound to the state the input router writes, and carries
    /// its tag — a pane a wheel cannot reach is the defect with extra steps.
    #[test]
    fn r1662_the_pane_is_bound_to_the_state_the_router_writes() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            scroll_pane(
                &use_scroll_state("lab.palette.body"),
                Rect::new(4, 8, 100, 100),
                (0, 0),
                PanePointer::Targets,
                vec![boxed(Rect::new(0, 0, 40, 20))],
            )
        });
        let Scene::Scroll(s) = &scene else {
            panic!("a pane is a scroll node");
        };
        assert_eq!(s.tag.as_deref(), Some("lab.palette.body"));
        assert!(s.state.is_some(), "the router needs somewhere to write");
        assert_eq!(s.viewport, Rect::new(4, 8, 100, 100));
        assert_eq!(s.axis, ScrollAxis::Both);
    }
}
