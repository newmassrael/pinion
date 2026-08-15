//! R1694 §5.2 §5.40 — an **addressable text run**: a styled run placed at an
//! absolute rectangle, carrying the tag by which a pointer, a reader and a gate
//! all address it.
//!
//! ## Why this module exists (rule-of-three lift)
//!
//! Three screens paint one, and the first two had written the same four lines:
//!
//! - the node lab's `tagged_label`, which is where the pattern started — R1654
//!   records that a run carrying no tag is invisible to every gate in the tree,
//!   because every gate is tag-keyed.
//! - the capture viewer's `tagged_label`, **byte-identical** to it.
//! - the dashboard's table cells (R1694), which is what made this a lift rather
//!   than a coincidence: a table cell painted without a tag is a value a reader
//!   can see and cannot ask about, so both of its tables now tag every cell.
//!
//! Mechanical, which is what makes it liftable rather than three legitimate
//! variants — the *style* is per-screen (each has its own run style, and the
//! dashboard's carries an overflow policy) and the style is the parameter.
//!
//! ## The two decisions it holds in a single place
//!
//! **The run's layout is the rectangle it was measured for.** A tagged run whose
//! layout rectangle and text rectangle disagree is addressable at one place and
//! painted at another, and every tag-keyed check — hit routing, the pointer
//! reach census, the accessibility bounds walk — then reads a box the glyphs are
//! not in. Passing the rectangle once is what makes that unrepresentable.
//!
//! **A run is transparent to the pointer.** Text is content; the box that owns
//! it is the control, and a run that took presses would shadow its own owner —
//! which the pointer-reach census reports as exactly that. All three call sites
//! had said so separately. A widget that genuinely wants the glyphs to be the
//! target builds the node itself; this helper is for the common case, and the
//! common case is that they are not.
//!
//! Deliberately **not** a `label` sibling. An untagged run is one line at the
//! call site and carries neither decision; a helper for it would only move the
//! line.

use pinion_core::Scene;
use pinion_core::scene::{Rect, TextNode};
use pinion_core::style::{LayoutStyle, Size, TextStyle};

/// A styled text run at `rect`, tagged so it can be addressed.
///
/// The rectangle is used twice on purpose — as the run's own box and as its
/// absolute layout — so the place a caller aims at and the place the glyphs land
/// cannot be different rectangles.
#[must_use]
pub fn text_run(
    tag: impl Into<String>,
    text: impl Into<String>,
    rect: Rect,
    style: TextStyle,
) -> Scene {
    Scene::Text(
        TextNode::styled(text.into(), rect, style)
            .with_tag(tag.into())
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x, rect.y)
                    .with_size(Size::px(rect.w, rect.h))
                    .with_pointer_transparent(true),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::style::Color;

    fn probe_box() -> Rect {
        Rect::new(12, 4, 92, 13)
    }

    fn probe() -> Scene {
        text_run(
            "row.0_1",
            "12:04:38.221",
            probe_box(),
            TextStyle::new()
                .with_size_px(11)
                .with_fg(Color::rgb(1, 2, 3)),
        )
    }

    fn node(scene: Scene) -> pinion_core::scene::TextNode {
        match scene {
            Scene::Text(node) => node,
            _ => panic!("a text run is Scene::Text"),
        }
    }

    #[test]
    fn a_run_is_tagged_and_carries_the_style_it_was_given() {
        let run = node(probe());
        assert_eq!(run.tag.as_deref(), Some("row.0_1"));
        assert_eq!(run.content, "12:04:38.221");
        assert_eq!(run.style.font_size_px, 11);
    }

    #[test]
    fn the_box_a_caller_aims_at_is_the_box_the_glyphs_land_in() {
        // The first decision this module holds in one place: a run addressable
        // at one rectangle and painted at another passes every check that asks
        // about its tag and is not where those checks say it is.
        let want = probe_box();
        let run = node(probe());
        assert_eq!(run.rect, want, "the run's own box");
        assert_eq!(
            run.layout.absolute_position,
            Some((want.x, want.y)),
            "and the position the layout places it at",
        );
        assert_eq!(run.layout.size, Size::px(want.w, want.h), "and its extent");
    }

    #[test]
    fn a_run_does_not_take_the_press_its_owner_should() {
        // The second: a tagged run that is opaque to the pointer shadows the
        // control it is the label of, which is what the pointer-reach census
        // reports it as.
        assert!(node(probe()).layout.pointer_transparent);
    }
}
