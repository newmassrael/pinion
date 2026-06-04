//! `hello-multi-select` — R780 §5.40 **multi-select index model**.
//!
//! R746 (`hello-virtual-select`) + R776 (`hello-virtual-nav`) land a
//! *single*-select index model over a flex-viewport virtualized list:
//! exactly one row at a time, the macOS/Windows file-list "selection IS the
//! cursor" model. The selection coordinator's own doc flagged multi-select
//! as "a later additive axis". This binding closes it: the same
//! [`VirtualSelectExternal`] is constructed in **`Multi`** mode (the Qt
//! `QItemSelectionModel` `ExtendedSelection` analogue — a selection *set*
//! plus an `anchor` for range extension), and the keyboard grows / shrinks
//! the set:
//!
//! * **plain `ArrowDown` / `ArrowUp` / `Home` / `End` / `Page`** — move the
//!   active row and replace the selection with just it (and reset the
//!   `anchor`), exactly as R776.
//! * **`Shift` + nav** — extend the contiguous range from the `anchor` to
//!   the navigated row (the canonical "select a span" gesture).
//! * **`Ctrl+A`** — select every one of the 10 000 rows.
//! * **`Ctrl+Space`** — toggle the active row's membership, leaving the
//!   rest of the set intact.
//!
//! Everything else is the unchanged R774 composition: the windowing is
//! [`view_flex_virtual_list`], the offset is the [`ScrollState`], and a
//! navigated row scrolls into view by index even when it was never
//! materialized ([`scroll_offset_to_reveal`], reached through
//! `nav_select_key`).
//!
//! The coordinator holds the selection as a `BTreeSet` of **data indices**
//! (no per-row state, decoupled from materialization). The view projects
//! that into a Copy per-row bitmap purely as the paint snapshot the shell
//! hands into the paint closure — the same shape `hello-table-multi` uses
//! for its eager multi-select grid.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/key` `End` then `Shift`+`Home` → `query("selection")` reports the
//! full `[0, 1, …, 9999]` span, and `query("mode")` reports `"multi"`.
//! Pointer modifier-click is intentionally **not** wired here — the router
//! pointer wire does not yet carry keyboard modifiers (a distinct R781
//! cross-cutting axis); keyboard + RPC drive every multi-select operation.
//! See `tools/r780_multi_select.py`.
//!
//! ## a11y
//!
//! Multi-select WAI-ARIA virtualized `list` via the R780-lifted
//! [`windowed_list_nodes_multiselected`]: `aria-setsize = N`, the container
//! `aria-multiselectable`, each rendered row an `AriaRole::ListItem` with
//! `aria-posinset` + `aria-selected = selection.contains(index)` (so several
//! visible rows can be `aria-selected` at once). The list container is the
//! focusable tab stop.

use pinion_a11y::{windowed_list_nodes_multiselected, AccessNode, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::{
    nav_select_key, read_selection, RowMetrics, VirtualSelectExternal,
};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::virtual_list::view_flex_virtual_list;
use std::collections::BTreeSet;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloMultiSelectRenderer, HelloMultiSelectRendererError);

/// Initial window size — freely resizable; the list re-windows on every
/// `Resized` event (R774 `AutoSizer`).
const WIN_W: u32 = 380;
const WIN_H: u32 = 480;
const THEME_TAG: &str = "app";
/// Total dataset size — large while the rendered node count stays small.
const N: usize = 10_000;
/// Uniform per-row vertical slot in logical pixels.
const ROW_PITCH: u32 = 32;
/// Extra rows built above + below the strict visible window.
const OVERSCAN: usize = 3;
/// Fixed header-bar height; the flex list fills the window below it.
const HEADER_H: u32 = 44;
/// Gutter width reserved for the scrollbar peer.
const GUTTER_W: u32 = 12;
/// Paint-root + a11y `list` container tag, and the state-scene tag of the
/// multi-select [`VirtualSelectExternal`] anchor.
const LIST_TAG: &str = "vlist";
const SCROLL_KEY: &str = "vlist_scroll";
const SCROLLBAR_TAG: &str = "vlist_scrollbar";
const STATUS_TAG: &str = "vlist_status";

/// Copy paint-snapshot of the coordinator's selection: a per-row bitmap
/// indexed by data row, plus the running total. The coordinator itself
/// holds the selection as a `BTreeSet` of indices (no per-row state); this
/// projection exists only so the shell can hand a `Copy` snapshot into the
/// paint closure without lifetime gymnastics — exactly the shape
/// `hello-table-multi` projects for its eager multi-select grid.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct MultiSelection {
    /// Per-row selection bit, indexed by absolute data row.
    selected: [bool; N],
    /// Total selected rows (the header readout).
    total: usize,
}

impl MultiSelection {
    fn empty() -> Self {
        Self { selected: [false; N], total: 0 }
    }

    /// Build the bitmap from a list of selected data indices (out-of-range
    /// dropped, duplicates idempotent).
    fn from_indices(indices: &[usize]) -> Self {
        let mut s = Self::empty();
        for &i in indices {
            if i < N && !s.selected[i] {
                s.selected[i] = true;
                s.total += 1;
            }
        }
        s
    }

    fn is_selected(&self, index: usize) -> bool {
        index < N && self.selected[index]
    }
}

/// One virtualized row. Every selected row paints in the Accent tone (M3
/// selected state layer) — in multi-select several visible rows can be
/// Accent at once; unselected rows zebra-stripe. Tagged `"vlist#<i>"` so a
/// click routes to the [`VirtualSelectExternal`] anchor (R51.42 composite
/// protocol) and `rect_for_tag` bounds the `listitem`.
fn build_row(index: usize, theme: &Theme, width: u32, selection: &MultiSelection) -> Scene {
    let (fill, fg) = if selection.is_selected(index) {
        (theme.resolve(ColorRole::Accent), theme.resolve(ColorRole::OnAccent))
    } else {
        let stripe = if index % 2 == 0 {
            ColorRole::SurfaceContainerLow
        } else {
            ColorRole::SurfaceContainer
        };
        (theme.resolve(stripe), theme.resolve(ColorRole::OnSurface))
    };
    let label = Scene::Text(TextNode::styled(
        row_label(index),
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(fg),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(format!("{LIST_TAG}#{index}"))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(width, ROW_PITCH))
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// Synthetic row content; zero-padded for stable width + unambiguous
/// `scene/snapshot` readout.
fn row_label(index: usize) -> String {
    const CATEGORIES: [&str; 5] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"];
    format!("Item {index:05} \u{00B7} {}", CATEGORIES[index % CATEGORIES.len()])
}

/// Header-bar status line: the selection cardinality + the measured
/// viewport extent. A literal scene-as-data readout — `Shift`-extend a span
/// and the text reports `selected 12 rows`, proving the set survives even
/// though most of its members were never materialized.
fn header(
    scroll: &std::rc::Rc<pinion_core::widgets::scroll::ScrollState>,
    theme: &Theme,
    total: usize,
) -> Scene {
    let (mw, mh) = scroll.measured_viewport();
    let noun = if total == 1 { "row" } else { "rows" };
    let text = Scene::Text(
        TextNode::styled(
            format!("selected {total} {noun} \u{00B7} viewport {mw}\u{00D7}{mh}"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        )
        .with_tag(STATUS_TAG),
    );
    Scene::Container(
        ContainerNode::new(vec![text])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::auto().with_height(SizeValue::Px(HEADER_H)))
                    .with_flex_grow(0.0)
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping `selection bitmap -> Scene`. The
/// dataset is virtual — `view_flex_virtual_list` invokes [`build_row`] only
/// for the indices in the current window, whose *size* is the
/// runtime-measured viewport height (R774 `AutoSizer`).
#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(selection: &MultiSelection, _frame: &Frame) -> Scene {
    let scroll = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let (measured_w, measured_h) = scroll.measured_viewport();

    let list = view_flex_virtual_list(&scroll, N, ROW_PITCH, OVERSCAN, |index| {
        build_row(index, &theme, measured_w, selection)
    });

    let scrollbar_style = {
        let mut s = VerticalScrollbarStyle::material(measured_h, SCROLLBAR_TAG);
        s.gutter_w = GUTTER_W;
        s
    };
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    let band = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_tag(LIST_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row).with_flex_grow(1.0)),
    );

    Scene::Container(
        ContainerNode::new(vec![header(&scroll, &theme, selection.total), band])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
    )
}

struct MultiSelectView;

impl WidgetCore for MultiSelectView {
    /// The selection bitmap — the widget's entire projected paint state.
    type State = MultiSelection;
    type Event = ();

    /// The primary External is the R746 index-held coordinator, built in
    /// R780 **`Multi`** mode at [`LIST_TAG`]. `apply_key` drives it from the
    /// keyboard (Shift-range / Ctrl+A / Ctrl+Space).
    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new_multi(N))
    }

    /// Sibling `ScrollBarExternal` sharing the list's `Rc<ScrollState>`.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG)]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    /// Project the selection set off the primary coordinator's `selection`
    /// JSON-array query into the Copy paint bitmap. A selection change
    /// repaints; scroll offset + scrollbar phase repaint via their own
    /// reactive `Signal` subscriptions the view opens.
    fn read_state(scene: &Scene) -> MultiSelection {
        let indices = scene
            .find_external_with_tag(LIST_TAG)
            .and_then(|node| node.handle.introspect())
            .map(read_selection)
            .unwrap_or_default();
        MultiSelection::from_indices(&indices)
    }

    fn view(state: MultiSelection, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// The list is a single keyboard tab stop (roving by data index, the
    /// WAI-ARIA listbox convention). Tab lands on the list; Arrow / Home /
    /// End / Page / Shift / Ctrl then drive the multi-select via
    /// [`apply_key`].
    fn focusable_tags() -> Vec<&'static str> {
        vec![LIST_TAG]
    }

    /// R780 §5.40 — keyboard multi-select over the windowed list, delegated
    /// to the shared `nav_select_key` controller (the same one
    /// `hello-virtual-nav` / `hello-grid-nav` use). Because the coordinator
    /// is in `Multi` mode, the `modifiers` now matter: `Shift`+nav extends a
    /// range, `Ctrl+A` selects all, `Ctrl+Space` toggles the active row.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        modifiers: pinion_core::Modifiers,
    ) -> bool {
        nav_select_key(
            scene,
            &use_scroll_state(SCROLL_KEY),
            LIST_TAG,
            focused,
            key,
            modifiers,
            RowMetrics { item_count: N, row_pitch: ROW_PITCH },
        )
    }

    fn title() -> &'static str {
        "pinion hello-multi-select (R780 §5.40 multi-select index model)"
    }

    fn fmt_state_log(state: &MultiSelection) -> String {
        format!("selected={} rows", state.total)
    }
}

impl WidgetA11y for MultiSelectView {
    /// Multi-select WAI-ARIA virtualized `list` via the R780-lifted
    /// [`windowed_list_nodes_multiselected`]: `aria-setsize = N`, the
    /// container `aria-multiselectable`, each rendered row an
    /// `AriaRole::ListItem` with `aria-posinset` + `aria-selected =
    /// selection.contains(index)`. Only the windowed rows are decorated, so
    /// the membership set is built over the window (not all `N`); the window
    /// is computed from the same measured viewport the view fn windows
    /// against, so the a11y tree and the painted tree never diverge.
    fn access_node(selection: &MultiSelection, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_KEY);
        let (_, measured_h) = scroll.measured_viewport();
        let window = compute_visible_range(scroll.offset_y(), measured_h, N, ROW_PITCH, OVERSCAN);
        let windowed_set: BTreeSet<usize> =
            window.indices().filter(|&i| selection.is_selected(i)).collect();
        windowed_list_nodes_multiselected(
            LIST_TAG,
            "Multi-selectable item list",
            u32::try_from(N).unwrap_or(u32::MAX),
            &window,
            &windowed_set,
        )
    }
}

impl WidgetView for MultiSelectView {
    type Renderer = HelloMultiSelectRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<MultiSelectView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_a11y::AriaRole;
    use pinion_core::Owner;

    fn run_view_with_measured(indices: &[usize], offset_y: i32, measured_h: u32) -> Scene {
        let owner = Owner::new();
        owner.run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_max(0, i32::try_from(N).unwrap() * i32::try_from(ROW_PITCH).unwrap());
            scroll.set_measured_viewport(360, measured_h);
            scroll.scroll_to(0, offset_y);
            view(&MultiSelection::from_indices(indices), &Frame::default())
        })
    }

    /// Find the `vlist#<i>` row container and return its fill color.
    fn row_fill(scene: &Scene, index: usize) -> Option<pinion_core::style::Color> {
        fn walk(scene: &Scene, want: &str) -> Option<pinion_core::style::Color> {
            match scene {
                Scene::Container(c) => {
                    if c.tag.as_deref() == Some(want) {
                        return Some(c.style.fill);
                    }
                    c.children.iter().find_map(|ch| walk(ch, want))
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), want),
                _ => None,
            }
        }
        walk(scene, &format!("{LIST_TAG}#{index}"))
    }

    #[test]
    fn several_selected_rows_paint_accent_at_once() {
        let theme = pinion_core::theme::Theme::light();
        let accent = theme.resolve(ColorRole::Accent);
        // Rows 1 and 3 selected, viewport tall enough to render the top.
        let scene = run_view_with_measured(&[1, 3], 0, 384);
        assert_eq!(row_fill(&scene, 1), Some(accent), "row 1 is Accent");
        assert_eq!(row_fill(&scene, 3), Some(accent), "row 3 is Accent too");
        assert_ne!(row_fill(&scene, 2), Some(accent), "row 2 between them is not");
    }

    #[test]
    fn a11y_marks_multiselectable_and_every_member() {
        let nodes = Owner::new().run(|| {
            let scroll = use_scroll_state(SCROLL_KEY);
            scroll.set_measured_viewport(360, 384);
            MultiSelectView::access_node(&MultiSelection::from_indices(&[0, 2]), None)
        });
        assert_eq!(nodes[0].role, AriaRole::List);
        assert!(nodes[0].multiselectable, "container is aria-multiselectable");
        let selected_count = nodes[1..].iter().filter(|n| n.selected == Some(true)).count();
        assert_eq!(selected_count, 2, "both members are aria-selected at once");
    }
}
