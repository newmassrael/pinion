//! R1523 §5.16 — shared **layout spacer**: a fixed-size, untagged, unpainted
//! box that occupies space and nothing else.
//!
//! ## Why this module exists (rule-of-three lift)
//!
//! Three widget paints need "reserve exactly this rectangle and render nothing
//! in it", and each had rolled its own two-line copy:
//!
//! - [`datepicker`](crate::datepicker)'s `blank_cell` — the leading blanks
//!   before the first of the month.
//! - [`tree_view`](crate::tree_view)'s depth indent — `depth × indent_step` of
//!   dead width before a row's disclosure glyph.
//! - [`table`](crate::table)'s R1523 column-window pad — the width of the
//!   columns a windowed row did not build, so the built ones still land at the
//!   x they would have had and the horizontal scroll still bounds against the
//!   whole content.
//!
//! Mechanical (no per-widget opinion), which is what makes it liftable rather
//! than three legitimate variants. But it does carry **one** decision worth
//! having in a single place: a spacer is **untagged**. `tree_view`'s copy said
//! so in a comment ("carries no tag (presentational) so the AT layer doesn't
//! expose it") and the others were silently the same; a fourth copy that
//! tagged its spacer would put a phantom node into the a11y tree, the hit
//! router and every `scene/snapshot` — the kind of divergence that reads as a
//! widget bug far from its cause.
//!
//! Deliberately **not** [`scrim`](crate::scrim) or [`barrier`](crate::barrier):
//! those are full-window *hit targets* (a scrim dims and traps, a barrier
//! catches the outside click). A spacer is neither painted nor hit — it is
//! layout ballast.

use pinion_core::Scene;
use pinion_core::scene::ContainerNode;
use pinion_core::style::{LayoutStyle, Size};

/// A `width × height` box that paints nothing, carries no tag, and holds no
/// children — pure layout ballast.
///
/// A zero dimension is valid and yields a zero-extent node; a caller that wants
/// *no node* for a zero extent should not call this (see
/// [`table`](crate::table)'s column pad, which skips the call).
#[must_use]
pub fn spacer(width: u32, height: u32) -> Scene {
    Scene::Container(
        ContainerNode::new(Vec::new())
            .with_layout(LayoutStyle::new().with_size(Size::px(width, height))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacer_is_sized_untagged_and_empty() {
        let Scene::Container(c) = spacer(24, 36) else {
            panic!("a spacer is a Container");
        };
        assert!(c.children.is_empty(), "no children");
        assert!(
            c.tag.is_none(),
            "untagged — a tagged spacer would appear in the a11y tree, the hit \
             router and every snapshot",
        );
        assert_eq!(
            c.layout.size,
            Size::px(24, 36),
            "an explicit size is the whole point",
        );
    }

    #[test]
    fn spacer_paints_nothing() {
        let Scene::Container(c) = spacer(10, 10) else {
            panic!("a spacer is a Container");
        };
        assert_eq!(
            c.style,
            pinion_core::style::BoxStyle::default(),
            "no fill / border / shadow — a visible spacer is a rule, not ballast",
        );
    }
}
