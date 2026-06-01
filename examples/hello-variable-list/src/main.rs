//! `hello-variable-list` — R745 §5.27 **Model/View variable-height
//! virtualization consumer**.
//!
//! The R744 sibling [`hello-virtual-list`] virtualizes a list of
//! *uniform*-height rows: every row is `ROW_PITCH` tall, so a row's top is
//! `index · pitch` in O(1). Real data grids rarely have uniform rows
//! (wrapped text, mixed media, group headers), so R745 adds the
//! *variable*-height peer: each row may be a different height, and the
//! windowing is driven by a **prefix-sum offset table** searched in
//! O(log n) — `react-window`'s `VariableSizeList` to R744's `FixedSizeList`.
//!
//! This binding renders `N = 10_000` rows whose heights cycle through four
//! distinct values ([`HEIGHTS`]), so the variability is visible at a
//! glance: scroll and the rows are plainly different heights, yet still
//! only the visible window (~viewport rows + overscan) ever exists in the
//! scene tree.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` reports only the windowed row nodes even though
//! `aria-setsize` is `10_000`, *and* each rendered row's absolute `y` and
//! height match the prefix-sum table exactly (row `i`'s slot top =
//! `Σ heights[0..i]`). Scroll via `scene/wheel`, snapshot again, and the
//! band shifts to higher indices whose tops follow the cumulative sum. The
//! whole proof is data — no pixels required (see
//! `tools/r745_variable_list.py`).
//!
//! ## Why the offset table is cached
//!
//! Building the prefix sum is O(n); doing it every frame would defeat the
//! point. [`list_offsets`] memoizes the [`RowOffsets`] in the reactive
//! [`Owner`](pinion_core::Owner) cache, so it is constructed once and every
//! frame's windowing is the O(log n) binary search alone.
//!
//! ## a11y (WAI-ARIA virtualized list)
//!
//! Identical AT model to the R744 sibling: one [`AriaRole::List`] parent
//! with [`aria-setsize`](pinion_a11y::AccessNode::with_size_of_set) `= N`,
//! one [`AriaRole::ListItem`] per *rendered* row with its absolute
//! [`aria-posinset`](pinion_a11y::AccessNode::with_position_in_set). Rows
//! are display-only this slice, so [`StubExternal`] is the addressable list
//! anchor and the scrollbar is the only interactive peer.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue, StubExternal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::scroll::use_scroll_state;
use pinion_core::widgets::scrollbar::{use_scrollbar_interaction, ScrollBarExternal};
use pinion_core::widgets::virtual_list::{compute_visible_range_variable, RowOffsets};
use pinion_core::{Frame, Owner, Scene, WidgetCore};
use pinion_widget_paint::scrollbar::{view_vertical_scrollbar, VerticalScrollbarStyle};
use pinion_widget_paint::virtual_list::view_variable_virtual_list;
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloVariableListRenderer, HelloVariableListRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 520;
/// Shared [`ThemeProvider`] cache key (the `"app"` convention shared
/// across the catalogue).
const THEME_TAG: &str = "app";
/// Total dataset size — large while the rendered node count stays small.
const N: usize = 10_000;
/// The four per-row heights this list cycles through (logical pixels).
/// `row_height(i) = HEIGHTS[i % 4]`, so consecutive rows are visibly
/// different sizes — the point of the variable-height path. One cycle is
/// `28 + 44 + 60 + 76 = 208` px tall.
const HEIGHTS: [u32; 4] = [28, 44, 60, 76];
/// Extra rows built above + below the strict visible window so a fast
/// wheel-flick never exposes a blank gap before the next frame.
const OVERSCAN: usize = 3;
/// Scroll viewport width (frames each row slot).
const VIEWPORT_W: u32 = 300;
/// Scroll viewport height. Because rows vary in height, the *number* of
/// rows that fit changes with the scroll position — another witness that
/// the windowing is height-driven, not a fixed count.
const VIEWPORT_H: u32 = 360;
/// Paint-root + a11y `list` container tag, and the state-scene tag of the
/// [`StubExternal`] list anchor.
const LIST_TAG: &str = "vlist";
/// Cache key for the scroll container's reactive `ScrollState`.
const SCROLL_KEY: &str = "vlist_scroll";
/// Cache key for the memoized prefix-sum offset table.
const OFFSETS_KEY: &str = "vlist_offsets";
/// Paint + state tag for the interactive scrollbar peer.
const SCROLLBAR_TAG: &str = "vlist_scrollbar";

/// Height of row `index` — cycles through [`HEIGHTS`]. Mirrored exactly by
/// the verification harness so the RPC-observed slot heights can be tied
/// to this model.
fn row_height(index: usize) -> u32 {
    HEIGHTS[index % HEIGHTS.len()]
}

/// The memoized prefix-sum [`RowOffsets`] table for the whole dataset.
///
/// Built once (O(n)) and cached in the [`Owner`] scope; every view /
/// a11y frame reuses the same `Rc` so the per-frame cost is the O(log n)
/// window search alone. The view fn and `access_node` both call this, so
/// they window against one identical table and never diverge on which rows
/// exist.
fn list_offsets() -> Rc<RowOffsets> {
    Owner::current()
        .expect("list_offsets requires an active Owner scope")
        .cache(OFFSETS_KEY, || {
            RowOffsets::from_heights(&(0..N).map(row_height).collect::<Vec<_>>())
        })
}

/// Cached projection: the scrollbar peer's interaction phase (scroll
/// *offset* repaints through the reactive `ScrollState` subscription the
/// view opens, so it is not part of this state). `Copy` for the §6.3
/// projection contract.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum BarPhase {
    Idle,
    Hover,
    Dragging,
}

/// One variable-height row: a strip whose height comes from
/// [`row_height`], tinted by its height tier so taller rows read as a
/// distinct tone. Tagged `"vlist#<i>"` so `enrich_names_from_scene`
/// derives the AT name from the label and `rect_for_tag` can bound the
/// matching `listitem` a11y node. The strip fills the slot the
/// [`view_variable_virtual_list`] positioning wrapper frames it into.
fn build_row(index: usize, theme: &Theme) -> Scene {
    let height = row_height(index);
    // Tier-keyed tone (one per distinct height) makes the variability
    // visible: a glance shows four interleaved row sizes + tones.
    let fill = match index % HEIGHTS.len() {
        0 => theme.resolve(ColorRole::SurfaceContainerLow),
        1 => theme.resolve(ColorRole::SurfaceContainer),
        2 => theme.resolve(ColorRole::SurfaceContainerHigh),
        _ => theme.resolve(ColorRole::SurfaceContainerHighest),
    };
    let label = Scene::Text(TextNode::styled(
        row_label(index, height),
        Rect::default(),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(format!("{LIST_TAG}#{index}"))
            .with_style(BoxStyle::filled(fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(VIEWPORT_W, height))
                    .with_padding(Rect::new(12, 0, 12, 0)),
            ),
    )
}

/// Synthetic row content. The zero-padded index keeps a stable width, and
/// the explicit height suffix makes the per-row variability legible in a
/// `scene/snapshot` readout.
fn row_label(index: usize, height: u32) -> String {
    format!("Row {index:05} \u{00B7} {height}px")
}

/// view-fn (§6.3): pure sync mapping `BarPhase -> Scene`. The dataset is
/// virtual — `view_variable_virtual_list` invokes [`build_row`] only for
/// the indices in the current scroll window.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_phase: BarPhase, _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let offsets = list_offsets();
    let theme = use_theme(THEME_TAG).theme_animated();

    // The windowed variable-height list. `view_variable_virtual_list`
    // reads `scroll_state`'s offset, binary-searches `offsets` for the
    // visible window, and builds only those rows. `Theme` is `Copy`.
    let list = view_variable_virtual_list(
        &scroll_state,
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        &offsets,
        OVERSCAN,
        |index| build_row(index, &theme),
    );

    // Scrollbar peer — sized against the *total* extent (the sizer height
    // the layout pass wrote into `ScrollState::max_y`), so the thumb is
    // tiny on a 10 000-row list. Shares the same `Rc<ScrollState>`.
    let scrollbar_style = VerticalScrollbarStyle::material(VIEWPORT_H, SCROLLBAR_TAG);
    let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
    let scrollbar_visual = view_vertical_scrollbar(
        &scroll_state,
        &theme,
        &scrollbar_style,
        scrollbar_interaction.get(),
    );

    // Composite paint root tagged `LIST_TAG` so `scene/wheel` /
    // `scene/snapshot` `{path: "vlist"}` resolves and the `list` a11y
    // bounds attach to the visible list + gutter. Flex Row lays the
    // windowed list beside the scrollbar peer.
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

struct VariableListView;

impl WidgetCore for VariableListView {
    type State = BarPhase;
    type Event = ();

    /// No per-item or aggregate widget statechart — the scroll position
    /// lives in [`ScrollState`] and the only interactive peer is the
    /// scrollbar. The primary External is the no-op [`StubExternal`]
    /// anchor, addressable at [`LIST_TAG`].
    fn create_external() -> Box<dyn External> {
        Box::new(StubExternal::new())
    }

    /// Sibling [`ScrollBarExternal`] sharing the list's `Rc<ScrollState>`.
    fn create_extra_externals() -> Vec<ExtraExternal> {
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let scrollbar_interaction = use_scrollbar_interaction(SCROLLBAR_TAG);
        let scrollbar = ScrollBarExternal::new()
            .attach_state(scroll_state)
            .attach_interaction(scrollbar_interaction);
        vec![ExtraExternal::new(SCROLLBAR_TAG, Box::new(scrollbar))]
    }

    fn tag() -> &'static str {
        LIST_TAG
    }

    /// Project the scrollbar peer's interaction phase; the scroll offset
    /// drives repaints through the reactive `ScrollState` subscription.
    fn read_state(scene: &Scene) -> BarPhase {
        let Some(node) = scene.find_external_with_tag(SCROLLBAR_TAG) else {
            return BarPhase::Idle;
        };
        let Some(intro) = node.handle.introspect() else {
            return BarPhase::Idle;
        };
        match intro.query("state") {
            Some(IntrospectValue::Text(s)) if s == "Dragging" => BarPhase::Dragging,
            Some(IntrospectValue::Text(s)) if s == "Hover" => BarPhase::Hover,
            _ => BarPhase::Idle,
        }
    }

    fn view(state: BarPhase, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Display-only list: nothing is a keyboard tab stop (the scrollbar is
    /// pointer / RPC driven). Empty so Tab never lands.
    fn focusable_tags() -> Vec<&'static str> {
        Vec::new()
    }

    fn title() -> &'static str {
        "pinion hello-variable-list (R745 §5.27 variable-height virtualization)"
    }

    fn fmt_state_log(state: &BarPhase) -> String {
        match state {
            BarPhase::Idle => "scrollbar=idle".to_string(),
            BarPhase::Hover => "scrollbar=hover".to_string(),
            BarPhase::Dragging => "scrollbar=dragging".to_string(),
        }
    }
}

impl WidgetA11y for VariableListView {
    /// WAI-ARIA virtualized `list`: one [`AriaRole::List`] parent with
    /// `aria-setsize = N` claiming the **rendered** row tags as children,
    /// plus one [`AriaRole::ListItem`] per visible row carrying its
    /// absolute `aria-posinset`. The window is resolved from the same
    /// cached [`RowOffsets`] the view fn uses, so the a11y tree and the
    /// painted tree never diverge on which rows exist.
    fn access_node(_state: &BarPhase, _focused: Option<&str>) -> Vec<AccessNode> {
        let scroll_state = use_scroll_state(SCROLL_KEY);
        let offsets = list_offsets();
        let window =
            compute_visible_range_variable(scroll_state.offset_y(), VIEWPORT_H, &offsets, OVERSCAN);

        let total = u32::try_from(N).unwrap_or(u32::MAX);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(window.count + 1);
        let mut list = AccessNode::new(LIST_TAG, AriaRole::List)
            .with_name("Variable-height item list")
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
                    .with_size_of_set(total),
            );
        }
        nodes
    }
}

impl WidgetView for VariableListView {
    type Renderer = HelloVariableListRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<VariableListView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_view() -> Scene {
        Owner::new().run(|| view(BarPhase::Idle, &Frame::default()))
    }

    fn find_scroll(scene: &Scene) -> Option<&pinion_core::scene::ScrollNode> {
        match scene {
            Scene::Scroll(s) => Some(s),
            Scene::Container(c) => c.children.iter().find_map(find_scroll),
            _ => None,
        }
    }

    /// Collect `(index, size)` for every `vlist#<i>` row container.
    fn row_sizes(scene: &Scene) -> Vec<(usize, Size)> {
        fn walk(scene: &Scene, out: &mut Vec<(usize, Size)>) {
            match scene {
                Scene::Container(c) => {
                    if let Some(t) = c.tag.as_deref() {
                        if let Some(idx) = t.strip_prefix("vlist#") {
                            out.push((idx.parse().unwrap(), c.layout.size));
                        }
                    }
                    for child in &c.children {
                        walk(child, out);
                    }
                }
                Scene::Scroll(s) => walk(s.content.as_ref(), out),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(scene, &mut out);
        out
    }

    #[test]
    fn renders_a_small_window_not_the_whole_dataset() {
        let scene = run_view();
        let rows = row_sizes(&scene);
        assert!(
            rows.len() < 40,
            "variable list must render a small window, got {} of {N}",
            rows.len(),
        );
        assert!(!rows.is_empty(), "must render at least the visible rows");
    }

    #[test]
    fn rendered_rows_carry_their_modeled_height() {
        let scene = run_view();
        for (idx, size) in row_sizes(&scene) {
            assert_eq!(
                size,
                Size::px(VIEWPORT_W, row_height(idx)),
                "row {idx} slot height matches the variable-height model",
            );
        }
    }

    #[test]
    fn view_wraps_the_window_in_a_scroll_node() {
        let scene = run_view();
        let scroll = find_scroll(&scene).expect("view must contain a Scene::Scroll");
        assert_eq!(scroll.viewport.w, VIEWPORT_W);
        assert_eq!(scroll.viewport.h, VIEWPORT_H);
        assert!(scroll.state.is_some(), "scroll must carry the state Rc");
    }

    #[test]
    fn a11y_list_reports_full_setsize_with_windowed_items() {
        let nodes = Owner::new().run(|| VariableListView::access_node(&BarPhase::Idle, None));
        assert_eq!(nodes[0].role, AriaRole::List);
        assert_eq!(
            nodes[0].size_of_set,
            Some(u32::try_from(N).unwrap()),
            "aria-setsize conveys the FULL dataset size",
        );
        assert!(nodes.len() - 1 < 40, "only the rendered window has listitem nodes");
        for item in &nodes[1..] {
            assert_eq!(item.role, AriaRole::ListItem);
            assert!(item.position_in_set.is_some(), "each row carries aria-posinset");
        }
        assert_eq!(nodes[1].position_in_set, Some(1), "top window starts at posinset 1");
    }

    #[test]
    fn row_label_encodes_index_and_height() {
        assert_eq!(row_label(0, 28), "Row 00000 \u{00B7} 28px");
        assert_eq!(row_label(42, 60), "Row 00042 \u{00B7} 60px");
    }

    #[test]
    fn height_model_cycles_four_distinct_values() {
        assert_eq!(row_height(0), 28);
        assert_eq!(row_height(1), 44);
        assert_eq!(row_height(2), 60);
        assert_eq!(row_height(3), 76);
        assert_eq!(row_height(4), 28, "cycles every four rows");
    }
}
