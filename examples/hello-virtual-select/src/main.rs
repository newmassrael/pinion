//! `hello-virtual-select` — R746 §5.27 **selectable virtualized list**.
//!
//! R744 (`hello-virtual-list`) and R745 (`hello-variable-list`) land
//! *display-only* virtualization. R746 adds the next Model/View slice:
//! **selection**. The decisive design point is that selection on a
//! virtualized collection is held by **data index**, not per-rendered-row
//! — `pinion_core::widgets::virtual_select::VirtualSelectExternal` keeps a
//! single `Option<usize>` selected index, decoupled from which rows are
//! materialized.
//!
//! Why not reuse the R735.1 [`selection`](pinion_core::widgets::selection)
//! substrate? That model is *leaf-based*: it operates on a `&mut [L]` slice
//! of materialized leaves, each carrying its own bit. A virtualized list
//! has no such slice — the 9 995 off-window leaves do not exist. Selection
//! by index is the canonical virtualized model (Qt `QItemSelectionModel`,
//! a Flutter selection controller over `ListView`).
//!
//! ## The witness (§2 #7 scene-as-data, selection ⊥ virtualization)
//!
//! Click row 3 → it paints in the Accent (selected) tone and the rendered
//! `ListItem` reports `aria-selected=true`. Scroll 4 000 px away (row 3
//! leaves the window and the tree entirely), scroll back — row 3 is still
//! selected. Or select row 5 000 *after* scrolling there: it works though
//! row 5 000 never existed at boot. The whole proof is data — the selected
//! index survives in the coordinator while the rows come and go (see
//! `tools/r746_virtual_select.py`).
//!
//! ## a11y
//!
//! Single-select `AriaRole::List` (no `aria-multiselectable`) with
//! `aria-setsize=N`; each *rendered* row is an `AriaRole::ListItem` with
//! `aria-posinset` and `aria-selected = (index == selected)`. The list is
//! addressable at [`LIST_TAG`] (the `VirtualSelectExternal` anchor); the
//! scrollbar is the other interactive peer. Keyboard roving over a
//! windowed list (focus-by-index + scroll-into-view) is a later additive
//! axis — this slice is pointer + RPC + AT selection (R730 keyboard-defer
//! precedent).

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{scrollbar_extra_external, use_scrollbar_interaction};
use pinion_core::widgets::virtual_list::compute_visible_range;
use pinion_core::widgets::virtual_select::VirtualSelectExternal;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::virtual_list::view_virtual_list;
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloVirtualSelectRenderer, HelloVirtualSelectRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 460;
const THEME_TAG: &str = "app";
/// Total dataset size — large while the rendered node count stays small.
const N: usize = 10_000;
/// Uniform per-row vertical slot in logical pixels (R744 fixed-pitch path;
/// selection is orthogonal to the pitch model).
const ROW_PITCH: u32 = 32;
/// Extra rows built above + below the strict visible window.
const OVERSCAN: usize = 3;
const VIEWPORT_W: u32 = 280;
/// Scroll viewport height — exactly 12 rows tall.
const VIEWPORT_H: u32 = 12 * ROW_PITCH;
/// Paint-root + a11y `list` container tag, and the state-scene tag of the
/// [`VirtualSelectExternal`] anchor (clicks on `vlist#<i>` rows route here).
const LIST_TAG: &str = "vlist";
const SCROLL_KEY: &str = "vlist_scroll";
const SCROLLBAR_TAG: &str = "vlist_scrollbar";

/// Cached projection of the widget state worth change-detecting: the
/// selected data index + the scrollbar peer's interaction phase. Scroll
/// *offset* repaints through the reactive `ScrollState` subscription. `Copy`
/// for the §6.3 projection contract.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
struct ListState {
    selected: Option<usize>,
    bar: BarPhase,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
enum BarPhase {
    #[default]
    Idle,
    Hover,
    Dragging,
}

/// One virtualized row. The selected row paints in the Accent tone
/// (M3 selected state layer); unselected rows zebra-stripe. Tagged
/// `"vlist#<i>"` so a click routes to the [`VirtualSelectExternal`] anchor
/// (R51.42 composite protocol) and `rect_for_tag` bounds the `listitem`.
fn build_row(index: usize, theme: &Theme, selected: Option<usize>) -> Scene {
    let is_selected = selected == Some(index);
    let (fill, fg) = if is_selected {
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
                    .with_size(Size::px(VIEWPORT_W, ROW_PITCH))
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

/// view-fn (§6.3): pure sync mapping `ListState -> Scene`. The dataset is
/// virtual — `view_virtual_list` invokes [`build_row`] only for the
/// indices in the current scroll window, each tinted by `state.selected`.
#[allow(clippy::trivially_copy_pass_by_ref)] // mirrors the WidgetCore::view `&Frame` signature
fn view(state: ListState, _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let theme = use_theme(THEME_TAG).theme_animated();
    let selected = state.selected;

    let list = view_virtual_list(
        &scroll_state,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        N,
        ROW_PITCH,
        OVERSCAN,
        |index| build_row(index, &theme, selected),
    );

    let scrollbar_style = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll_state,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    let list_root = Scene::Container(
        ContainerNode::new(vec![list, scrollbar_visual])
            .with_tag(LIST_TAG)
            .with_layout(LayoutStyle::new().flex(FlexDirection::Row)),
    );

    Scene::Container(
        ContainerNode::new(vec![list_root])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

struct VirtualSelectView;

impl WidgetCore for VirtualSelectView {
    type State = ListState;
    type Event = ();

    /// The primary External is the index-held selection coordinator,
    /// addressable at [`LIST_TAG`]. Clicks on the windowed `vlist#<i>` rows
    /// route here via the R51.42 composite protocol.
    fn create_external() -> Box<dyn External> {
        Box::new(VirtualSelectExternal::new(N))
    }

    /// Sibling `ScrollBarExternal` sharing the list's `Rc<ScrollState>`.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        vec![scrollbar_extra_external(use_scroll_state(SCROLL_KEY), SCROLLBAR_TAG)]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    /// Project the selected data index off the primary coordinator + the
    /// scrollbar peer's phase. A selection change (`Option<usize>` differs)
    /// raises the §5.20 transition and repaints; scroll offset repaints via
    /// the reactive `ScrollState` subscription.
    fn read_state(scene: &Scene) -> ListState {
        let selected = scene
            .find_external_with_tag(LIST_TAG)
            .and_then(|node| node.handle.introspect())
            .and_then(|intro| match intro.query("selected") {
                Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
                _ => None,
            });
        let bar = scene
            .find_external_with_tag(SCROLLBAR_TAG)
            .and_then(|node| node.handle.introspect())
            .and_then(|intro| intro.query("state"))
            .map_or(BarPhase::Idle, |v| match v {
                IntrospectValue::Text(s) if s == "Dragging" => BarPhase::Dragging,
                IntrospectValue::Text(s) if s == "Hover" => BarPhase::Hover,
                _ => BarPhase::Idle,
            });
        ListState { selected, bar }
    }

    fn view(state: ListState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Pointer / RPC selection this slice; rows are not keyboard tab stops
    /// (windowed-list roving is a later axis). Empty so Tab never lands.
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-virtual-select (R746 §5.27 selectable virtualization)"
    }

    fn fmt_state_log(state: &ListState) -> String {
        match state.selected {
            Some(i) => format!("selected=row {i}"),
            None => "selected=none".to_string(),
        }
    }
}

impl WidgetA11y for VirtualSelectView {
    /// Single-select WAI-ARIA virtualized `list`: one [`AriaRole::List`]
    /// parent (`aria-setsize = N`, no `aria-multiselectable`) over the
    /// rendered window; each visible row is an [`AriaRole::ListItem`] with
    /// `aria-posinset` and `aria-selected = (index == selected)`.
    fn access_node(state: &ListState, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let window =
            compute_visible_range(scroll_state.offset_y(), VIEWPORT_H, N, ROW_PITCH, OVERSCAN);

        let total = u32::try_from(N).unwrap_or(u32::MAX);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(window.count + 1);
        let mut list = AccessNode::new(LIST_TAG, AriaRole::List)
            .with_name("Selectable item list")
            .with_size_of_set(total);
        for index in window.indices() {
            list = list.with_child(format!("{LIST_TAG}#{index}"));
        }
        nodes.push(list);
        for index in window.indices() {
            let posinset = u32::try_from(index + 1).unwrap_or(u32::MAX);
            nodes.push(
                AccessNode::new(format!("{LIST_TAG}#{index}"), AriaRole::ListItem)
                    .with_position_in_set(posinset)
                    .with_size_of_set(total)
                    .with_selected(state.selected == Some(index)),
            );
        }
        nodes
    }
}

impl WidgetView for VirtualSelectView {
    type Renderer = HelloVirtualSelectRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<VirtualSelectView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Owner;

    fn run_view(selected: Option<usize>) -> Scene {
        Owner::new().run(|| view(ListState { selected, bar: BarPhase::Idle }, &Frame::default()))
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
    fn selected_row_paints_accent_others_do_not() {
        let theme = pinion_core::theme::Theme::light();
        let accent = theme.resolve(ColorRole::Accent);
        // With row 2 selected, row 2 is Accent and a neighbor is not.
        let scene = run_view(Some(2));
        assert_eq!(row_fill(&scene, 2), Some(accent), "selected row is Accent");
        assert_ne!(row_fill(&scene, 3), Some(accent), "unselected row is not Accent");
    }

    #[test]
    fn no_selection_paints_no_accent() {
        let theme = pinion_core::theme::Theme::light();
        let accent = theme.resolve(ColorRole::Accent);
        let scene = run_view(None);
        for i in 0..6 {
            assert_ne!(row_fill(&scene, i), Some(accent), "row {i} not Accent when unselected");
        }
    }

    #[test]
    fn a11y_marks_selected_row_aria_selected() {
        let nodes = Owner::new().run(|| {
            VirtualSelectView::access_node(&ListState { selected: Some(1), bar: BarPhase::Idle }, None)
        });
        assert_eq!(nodes[0].role, AriaRole::List);
        // Find the listitem for index 1 (posinset 2) and assert selected.
        let item1 = nodes[1..]
            .iter()
            .find(|n| n.position_in_set == Some(2))
            .expect("rendered listitem for index 1");
        assert_eq!(item1.selected, Some(true), "selected row carries aria-selected=true");
        let item0 = nodes[1..]
            .iter()
            .find(|n| n.position_in_set == Some(1))
            .expect("rendered listitem for index 0");
        assert_eq!(item0.selected, Some(false), "unselected row carries aria-selected=false");
    }

    #[test]
    fn read_state_default_is_unselected() {
        // A fresh scene with the coordinator at its default reports None.
        let state = ListState::default();
        assert_eq!(state.selected, None);
        assert_eq!(state.bar, BarPhase::Idle);
    }
}
