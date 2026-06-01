//! R744 §5.27 — **virtualized list view assembly**.
//!
//! The view-construction half of the Model/View virtualization slice
//! (the windowing arithmetic lives backend-agnostically in
//! [`pinion_core::widgets::virtual_list`]). [`view_virtual_list`]
//! assembles a [`ScrollNode`] whose content materializes scene nodes for
//! **only the visible window** of a potentially enormous dataset, while
//! still presenting the runtime scroll-bound pass with the *full* content
//! height so the scrollbar peer thumb sizes against the whole list.
//!
//! ## The assembled shape
//!
//! ```text
//! Scroll(viewport, state)                         ← clips + offsets
//!   └─ Container (auto height — the content root)  ← wrapper, forced to
//!        └─ Container (size = w × total_height)         auto by the
//!             ├─ Container(abs_pos 0, first·pitch)      scroll-content
//!             │    └─ <row built by build_row(first)>   layout pass; it
//!             ├─ Container(abs_pos 0, (first+1)·pitch)  wraps the
//!             │    └─ <row built by build_row(first+1)> fixed-height
//!             └─ …only the visible window…             sizer so its
//!                                                       rect.h == total
//! ```
//!
//! Two structural facts make this work with **zero** changes to the
//! scroll / layout / scrollbar substrate:
//!
//! 1. The runtime scroll-content pass forces the *content root* node's
//!    height to `auto` (so a normal column can overflow the clip
//!    window). A virtualized content root therefore cannot itself carry
//!    the total height — it would be overwritten. The fix is the inner
//!    **sizer** container: an explicit `Size::px(w, total_height)` child
//!    that the auto wrapper resolves its own height from. The pass then
//!    reads `content.rect().h == total_height` into
//!    [`ScrollState::set_max`](pinion_core::widgets::scroll::ScrollState::set_max)
//!    exactly as for a fully-materialized column.
//! 2. Each visible row is lifted out of flow with
//!    [`LayoutStyle::with_absolute_position`](pinion_core::style::LayoutStyle::with_absolute_position)
//!    at `(0, index · pitch)` inside the sizer — the R55.D.6 CSS-mirror
//!    positioning substrate. Absolute children do not contribute to the
//!    sizer's content height (the explicit `Size` already fixes it), so
//!    rendering a sparse window leaves the scroll extent intact.
//!
//! ## Consumer contract
//!
//! The `build_row` closure is the "view" half of Model/View: given a
//! data index it returns the row's [`Scene`] (typically a tagged
//! `Container` so the binding's `access_node` / input router can address
//! it). The closure is invoked **only** for indices in the window, so a
//! 10 000-row list pays for ~viewport-plus-overscan rows per frame. The
//! row's own `LayoutStyle` controls its content; the positioning wrapper
//! this helper adds owns only the slot placement and a `w × row_pitch`
//! frame.
//!
//! Uniform `row_pitch` only this slice — see the
//! [`virtual_list`](pinion_core::widgets::virtual_list) module's scope
//! notes.

use std::rc::Rc;

use pinion_core::scene::{ContainerNode, Rect, ScrollNode};
use pinion_core::style::{LayoutStyle, Size};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::virtual_list::{
    compute_visible_range, content_height, VisibleWindow,
};
use pinion_core::Scene;

/// R744 §5.27 — assemble a virtualized vertical list as a [`ScrollNode`].
///
/// Resolves the visible window from `scroll.offset_y()`, builds scene
/// nodes for only those rows via `build_row`, positions each absolutely
/// inside a full-height sizer, and wraps the whole thing in a
/// `ScrollNode` bound to `scroll` (so wheel / scrollbar input drives the
/// offset and the layout pass writes the total-height scroll bound).
///
/// # Parameters
///
/// - `scroll` — the reactive [`ScrollState`] (resolved by the binding via
///   `use_scroll_state`); cloned into the `ScrollNode` so the input
///   router and scrollbar peer share one offset source.
/// - `viewport` — the clip window rect (its `w` also frames each row
///   slot; `h` is the visible height fed to the windowing math).
/// - `item_count` — total dataset size (decoupled from rendered count).
/// - `row_pitch` — uniform per-row vertical slot in logical pixels.
/// - `overscan` — rows rendered beyond the strict window on each side.
/// - `build_row` — row builder invoked once per visible index.
pub fn view_virtual_list(
    scroll: &Rc<ScrollState>,
    viewport: Rect,
    item_count: usize,
    row_pitch: u32,
    overscan: usize,
    mut build_row: impl FnMut(usize) -> Scene,
) -> Scene {
    let window: VisibleWindow =
        compute_visible_range(scroll.offset_y(), viewport.h, item_count, row_pitch, overscan);
    let total_h = content_height(item_count, row_pitch);

    let mut slots: Vec<Scene> = Vec::with_capacity(window.count);
    for index in window.indices() {
        let row = build_row(index);
        // Slot top = index · pitch. `content_height`'s saturation logic
        // keeps the total in `u32`; an individual slot top is always
        // below that total, so the same saturating cast is safe.
        let top = u32::try_from((index as u64).saturating_mul(u64::from(row_pitch)))
            .unwrap_or(u32::MAX);
        slots.push(Scene::Container(
            ContainerNode::new(vec![row]).with_layout(
                LayoutStyle::new()
                    .with_absolute_position(0, top)
                    .with_size(Size::px(viewport.w, row_pitch)),
            ),
        ));
    }

    // Sizer: explicit full-height frame the auto content-root wrapper
    // resolves its height from, so the scroll-bound pass sees the total
    // extent even though only the window exists.
    let sizer = Scene::Container(
        ContainerNode::new(slots)
            .with_layout(LayoutStyle::new().with_size(Size::px(viewport.w, total_h))),
    );
    // Content root: the scroll-content layout pass forces this node's
    // height to `auto`; it wraps the fixed-height sizer so its resolved
    // rect height equals `total_h`.
    let content = Scene::Container(ContainerNode::new(vec![sizer]));

    Scene::Scroll(ScrollNode::from_state(Rc::clone(scroll), viewport, content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::TextNode;
    use pinion_core::style::TextStyle;
    use pinion_core::Owner;

    const PITCH: u32 = 40;
    const VIEWPORT: Rect = Rect::new(0, 0, 200, 200);
    const N: usize = 10_000;

    fn build_row(i: usize) -> Scene {
        Scene::Text(TextNode::styled(
            format!("row {i}"),
            Rect::default(),
            TextStyle::new(),
        ))
    }

    fn run(offset_y: i32) -> Scene {
        // `ScrollState::new` starts at offset 0; seed the offset by
        // running inside an Owner so the math reads it back. We bypass
        // the cache and drive the state directly for a deterministic
        // unit shape.
        let state = Rc::new(ScrollState::new());
        // A generous max so scroll_to is not clamped to 0.
        state.set_max(0, i32::try_from(N).unwrap() * i32::try_from(PITCH).unwrap());
        state.scroll_to(0, offset_y);
        Owner::new().run(|| view_virtual_list(&state, VIEWPORT, N, PITCH, 2, build_row))
    }

    fn unwrap_sizer(scene: &Scene) -> &ContainerNode {
        let Scene::Scroll(s) = scene else {
            panic!("root must be a Scroll");
        };
        let Scene::Container(wrapper) = s.content.as_ref() else {
            panic!("content root must be a Container wrapper");
        };
        assert_eq!(wrapper.children.len(), 1, "wrapper holds exactly the sizer");
        let Scene::Container(sizer) = &wrapper.children[0] else {
            panic!("wrapper child must be the sizer Container");
        };
        sizer
    }

    #[test]
    fn renders_only_the_window_not_the_whole_dataset() {
        let scene = run(0);
        let sizer = unwrap_sizer(&scene);
        // offset 0, viewport 5 rows, overscan 2 → rows 0..=6 = 7 slots,
        // NOT 10 000.
        assert_eq!(sizer.children.len(), 7, "only the windowed rows exist");
    }

    #[test]
    fn sizer_carries_full_content_height_for_scroll_bound() {
        let scene = run(0);
        let sizer = unwrap_sizer(&scene);
        assert_eq!(
            sizer.layout.size,
            Size::px(VIEWPORT.w, content_height(N, PITCH)),
            "sizer height = item_count × pitch so set_max sees total extent",
        );
    }

    #[test]
    fn visible_rows_are_absolutely_positioned_at_index_times_pitch() {
        // Scroll into the middle so the first visible index is nonzero.
        let scene = run(4000); // 100 rows down at pitch 40
        let sizer = unwrap_sizer(&scene);
        // first_visible 100, overscan 2 → first slot is index 98 at
        // top 98*40 = 3920.
        let Scene::Container(first_slot) = &sizer.children[0] else {
            panic!("slot must be a Container");
        };
        assert_eq!(
            first_slot.layout.absolute_position,
            Some((0, 98 * PITCH)),
            "first windowed slot pinned at its data index offset",
        );
        assert_eq!(first_slot.layout.size, Size::px(VIEWPORT.w, PITCH));
    }

    #[test]
    fn scroll_node_is_bound_to_state() {
        let Scene::Scroll(s) = run(0) else {
            panic!("root is a Scroll");
        };
        assert_eq!(s.viewport, VIEWPORT);
        assert!(s.state.is_some(), "ScrollNode must carry the state Rc");
    }

    #[test]
    fn empty_dataset_yields_empty_sizer() {
        let state = Rc::new(ScrollState::new());
        let scene = Owner::new()
            .run(|| view_virtual_list(&state, VIEWPORT, 0, PITCH, 2, build_row));
        let sizer = unwrap_sizer(&scene);
        assert!(sizer.children.is_empty(), "no rows for an empty dataset");
        assert_eq!(sizer.layout.size, Size::px(VIEWPORT.w, 0));
    }
}
