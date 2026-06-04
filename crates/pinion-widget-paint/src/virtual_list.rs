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
//! Two pitch models share this shape: [`view_virtual_list`] (uniform
//! `row_pitch`, R744) and [`view_variable_virtual_list`] (explicit
//! per-row heights via a prefix-sum [`RowOffsets`] table, R745). They
//! differ only in how each slot's top + height is derived; the slot
//! wrapper ([`positioned_slot`]) and the sizer / content-root / scroll
//! assembly ([`assemble_windowed`]) are shared. See the
//! [`virtual_list`](pinion_core::widgets::virtual_list) module's scope
//! notes for the *measured*-height boundary still deferred.

use std::rc::Rc;

use pinion_core::scene::{ContainerNode, Rect, ScrollNode};
use pinion_core::style::{LayoutStyle, Size};
use pinion_core::widgets::scroll::ScrollState;
use pinion_core::widgets::virtual_list::{
    compute_visible_range, compute_visible_range_variable, content_height, RowOffsets,
    VisibleWindow,
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
        slots.push(positioned_slot(row, viewport.w, top, row_pitch));
    }

    assemble_windowed(scroll, viewport, total_h, slots)
}

/// R745 §5.27 — assemble a **variable-height** virtualized vertical list
/// as a [`ScrollNode`].
///
/// The variable-pitch peer of [`view_virtual_list`]: identical windowed
/// shape, but each row's slot top and height come from the prefix-sum
/// [`RowOffsets`] table (`react-window`'s `VariableSizeList`) instead of a
/// uniform pitch. The shared sizer / content-root / scroll wrapping keeps
/// the scroll bound at the true total extent exactly as the fixed path
/// does, so the scrollbar peer and layout pass need no awareness of which
/// pitch model produced the rows.
///
/// # Parameters
///
/// - `scroll` — the reactive [`ScrollState`] (shared with the scrollbar peer).
/// - `viewport` — the clip window rect (`w` frames each slot; `h` feeds the
///   windowing math).
/// - `offsets` — the prefix-sum table built from the per-row heights
///   (`RowOffsets::from_heights`); owns both the per-row geometry and the
///   total content height.
/// - `overscan` — rows rendered beyond the strict window on each side.
/// - `build_row` — row builder invoked once per visible index.
pub fn view_variable_virtual_list(
    scroll: &Rc<ScrollState>,
    viewport: Rect,
    offsets: &RowOffsets,
    overscan: usize,
    mut build_row: impl FnMut(usize) -> Scene,
) -> Scene {
    let window: VisibleWindow =
        compute_visible_range_variable(scroll.offset_y(), viewport.h, offsets, overscan);
    let total_h = offsets.total_height();

    let mut slots: Vec<Scene> = Vec::with_capacity(window.count);
    for index in window.indices() {
        let row = build_row(index);
        // Per-row top and height read straight off the prefix-sum table —
        // the variable-pitch analogue of the fixed path's `index · pitch`.
        slots.push(positioned_slot(
            row,
            viewport.w,
            offsets.row_top(index),
            offsets.row_height(index),
        ));
    }

    assemble_windowed(scroll, viewport, total_h, slots)
}

/// R774 §5.27 — assemble a **flex-viewport** (`AutoSizer`) virtualized
/// vertical list as a [`ScrollNode`].
///
/// The fixed ([`view_virtual_list`]) and variable
/// ([`view_variable_virtual_list`]) assemblies take a caller-supplied
/// `viewport` rect: the clip-window height is a const the binding picks,
/// so the rendered row count never adapts to the container's real size.
/// This peer drops the height const entirely. The list reads the
/// *measured* viewport from
/// [`ScrollState::measured_viewport`](pinion_core::widgets::scroll::ScrollState::measured_viewport) —
/// the flex-computed clip-window extent the runtime layout pass wrote on
/// the previous frame — and windows against that. The enclosing
/// [`ScrollNode`] is laid out with `flex_grow = 1.0` (no explicit size),
/// so it fills whatever space its flex parent grants; resizing the
/// window, dragging a splitter, or any parent-flex redistribution
/// changes the measured height and the next view re-run materializes a
/// different row count. This is react-window's `AutoSizer` pattern: the
/// container measures itself, the windowing follows.
///
/// First-paint warmup: `measured_viewport()` is `(0, 0)` until the first
/// layout pass runs, so the first `V::view` produces an empty window
/// (height `0` → no rows). The layout pass then measures the flex extent,
/// writes it via
/// [`ScrollState::set_measured_viewport`](pinion_core::widgets::scroll::ScrollState::set_measured_viewport),
/// and the scroll-dirty same-frame re-pass re-runs the view with the true
/// height — identical to the [`ScrollState::set_max`](pinion_core::widgets::scroll::ScrollState::set_max)
/// chicken-and-egg fix, so the user never sees the empty frame.
///
/// # Parameters
///
/// - `scroll` — the reactive [`ScrollState`] (shared with the scrollbar
///   peer); both the offset and the measured viewport are read from it.
/// - `item_count` — total dataset size (decoupled from rendered count).
/// - `row_pitch` — uniform per-row vertical slot in logical pixels.
/// - `overscan` — rows rendered beyond the strict window on each side.
/// - `build_row` — row builder invoked once per visible index. The row is
///   framed to the measured viewport width, so rows fill a resizing pane.
pub fn view_flex_virtual_list(
    scroll: &Rc<ScrollState>,
    item_count: usize,
    row_pitch: u32,
    overscan: usize,
    mut build_row: impl FnMut(usize) -> Scene,
) -> Scene {
    let (measured_w, measured_h) = scroll.measured_viewport();
    let window: VisibleWindow =
        compute_visible_range(scroll.offset_y(), measured_h, item_count, row_pitch, overscan);
    let total_h = content_height(item_count, row_pitch);

    let mut slots: Vec<Scene> = Vec::with_capacity(window.count);
    for index in window.indices() {
        let row = build_row(index);
        let top = u32::try_from((index as u64).saturating_mul(u64::from(row_pitch)))
            .unwrap_or(u32::MAX);
        slots.push(positioned_slot(row, measured_w, top, row_pitch));
    }

    assemble_windowed_flex(scroll, measured_w, total_h, slots)
}

/// Wrap windowed `slots` in the sizer + content-root shape, then in a
/// **flex-grow** `ScrollNode` (no explicit size) so taffy sizes the clip
/// window from the parent flex instead of a caller-supplied const. The
/// seed `viewport` is `0 × 0` — the layout pass overwrites it with the
/// flex-computed rect via `assign_rect` — so the only thing that matters
/// pre-layout is the `flex_grow` sidecar. The cross-axis stretches via
/// the parent's default [`AlignItems::Stretch`], giving the list its
/// full width.
/// (R775 `pub(crate)` for the flex-virtual *table* body) Wrap windowed
/// `slots` in the sizer + content-root shape, then in a **flex-grow**
/// `ScrollNode` (no explicit size) so taffy sizes the clip window from the
/// parent flex. Shared by [`view_flex_virtual_list`] (vertical list) and
/// `crate::table::view_virtual_table` (data-grid body) so the `AutoSizer`
/// windowed-sizer shape is one source of truth — a divergence would be a
/// scroll-bound bug, not a style choice.
pub(crate) fn assemble_windowed_flex(
    scroll: &Rc<ScrollState>,
    width: u32,
    total_h: u32,
    slots: Vec<Scene>,
) -> Scene {
    let content = windowed_content(width, total_h, slots);
    Scene::Scroll(
        ScrollNode::from_state(Rc::clone(scroll), Rect::default(), content)
            .with_layout(LayoutStyle::new().with_flex_grow(1.0)),
    )
}

/// (R775 `pub(crate)` — shared with the flex-virtual table body) Lift a
/// built `row` out of flow into an absolutely-positioned slot at
/// `(0, top)` framed to `width × height` — the R55.D.6 CSS-mirror
/// positioning wrapper shared by both list assemblies. Absolute children
/// do not contribute to the sizer's content height, so a sparse window
/// leaves the scroll extent (fixed by the sizer) intact.
pub(crate) fn positioned_slot(row: Scene, width: u32, top: u32, height: u32) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![row]).with_layout(
            LayoutStyle::new()
                .with_absolute_position(0, top)
                .with_size(Size::px(width, height)),
        ),
    )
}

/// Build the "full-height sizer inside an auto content-root" content
/// scene shared by every windowed-list assembly (fixed
/// [`view_virtual_list`], variable [`view_variable_virtual_list`], and
/// flex [`view_flex_virtual_list`]).
///
/// `total_h` is the sizer's explicit height (the full content extent);
/// `width` frames the sizer. The scroll-content layout pass forces the
/// *content root* to `auto`, so it resolves its height from the
/// fixed-height sizer child and writes that total into
/// [`ScrollState::set_max`] — even though only the window's slots exist
/// in the tree. This is the wire shape both bounds reads
/// (`content.rect().h == total_h`) depend on, so it lives in one place;
/// the caller decides only how the enclosing [`ScrollNode`] sizes itself
/// (fixed clip window vs flex-grow).
fn windowed_content(width: u32, total_h: u32, slots: Vec<Scene>) -> Scene {
    let sizer = Scene::Container(
        ContainerNode::new(slots).with_layout(LayoutStyle::new().with_size(Size::px(width, total_h))),
    );
    Scene::Container(ContainerNode::new(vec![sizer]))
}

/// Wrap windowed `slots` in the canonical "sizer + content-root inside a
/// **fixed-clip** `ScrollNode`" shape shared by the fixed-
/// ([`view_virtual_list`]) and variable-pitch
/// ([`view_variable_virtual_list`]) assemblies. The scroll node's layout
/// sidecar stays at [`ScrollNode::new`]'s default `with_size(viewport)`,
/// so taffy treats it as a fixed-size leaf at the caller-supplied
/// dimensions.
fn assemble_windowed(
    scroll: &Rc<ScrollState>,
    viewport: Rect,
    total_h: u32,
    slots: Vec<Scene>,
) -> Scene {
    let content = windowed_content(viewport.w, total_h, slots);
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

    // ── R745 variable-height view assembly ──────────────────────────

    // Alternating 20 / 60 px rows: tops 0,20,80,100,160,180,240,… (80 px
    // per pair). Total for `N` rows = N/2 * 80.
    fn var_offsets(n: usize) -> RowOffsets {
        RowOffsets::from_heights(
            &(0..n).map(|i| if i % 2 == 0 { 20 } else { 60 }).collect::<Vec<_>>(),
        )
    }

    fn run_variable(offset_y: i32, n: usize) -> Scene {
        let offsets = var_offsets(n);
        let total = i32::try_from(offsets.total_height()).unwrap();
        let state = Rc::new(ScrollState::new());
        state.set_max(0, total);
        state.scroll_to(0, offset_y);
        Owner::new()
            .run(|| view_variable_virtual_list(&state, VIEWPORT, &offsets, 2, build_row))
    }

    #[test]
    fn variable_renders_only_the_window() {
        let scene = run_variable(0, N);
        let sizer = unwrap_sizer(&scene);
        // offset 0, viewport 200: rows 0..=5 (top<200) + 2 overscan below
        // → rows 0..=7 = 8 slots, NOT 10 000.
        assert_eq!(sizer.children.len(), 8, "only the windowed rows exist");
    }

    #[test]
    fn variable_sizer_carries_total_prefix_sum_height() {
        let scene = run_variable(0, N);
        let sizer = unwrap_sizer(&scene);
        // N/2 pairs * 80 = 10000/2 * 80 = 400_000.
        assert_eq!(
            sizer.layout.size,
            Size::px(VIEWPORT.w, var_offsets(N).total_height()),
            "sizer height = prefix-sum total so set_max sees full extent",
        );
        assert_eq!(var_offsets(N).total_height(), 400_000);
    }

    #[test]
    fn variable_rows_positioned_and_sized_by_offset_table() {
        let scene = run_variable(0, N);
        let sizer = unwrap_sizer(&scene);
        // First slot = row 0: top 0, height 20.
        let Scene::Container(slot0) = &sizer.children[0] else {
            panic!("slot must be a Container");
        };
        assert_eq!(slot0.layout.absolute_position, Some((0, 0)));
        assert_eq!(slot0.layout.size, Size::px(VIEWPORT.w, 20));
        // Second slot = row 1: top 20, height 60 (variable!).
        let Scene::Container(slot1) = &sizer.children[1] else {
            panic!("slot must be a Container");
        };
        assert_eq!(slot1.layout.absolute_position, Some((0, 20)));
        assert_eq!(slot1.layout.size, Size::px(VIEWPORT.w, 60));
    }

    #[test]
    fn variable_window_slides_with_offset() {
        // Scroll to top of row 20 (offset 800, see core tests). First
        // windowed slot = 20 - 2 overscan = row 18, top 18 = 9 pairs * 80
        // = 720.
        let scene = run_variable(800, N);
        let sizer = unwrap_sizer(&scene);
        let Scene::Container(first) = &sizer.children[0] else {
            panic!("slot must be a Container");
        };
        assert_eq!(first.layout.absolute_position, Some((0, 720)));
    }

    #[test]
    fn variable_empty_table_yields_empty_sizer() {
        let offsets = RowOffsets::from_heights(&[]);
        let state = Rc::new(ScrollState::new());
        let scene = Owner::new()
            .run(|| view_variable_virtual_list(&state, VIEWPORT, &offsets, 2, build_row));
        let sizer = unwrap_sizer(&scene);
        assert!(sizer.children.is_empty(), "no rows for an empty table");
        assert_eq!(sizer.layout.size, Size::px(VIEWPORT.w, 0));
    }
}
