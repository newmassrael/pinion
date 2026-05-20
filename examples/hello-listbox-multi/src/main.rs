//! R51.104 §5.38 hello-listbox-multi — first-client visual demo for
//! the R51.98 multi-select `ListBox` primitive.
//!
//! Axis vs [`hello-listbox`]: the single-select sibling shows the
//! WAI-ARIA Listbox single-select model (`aria-multiselectable="false"`,
//! activate-one-deselect-rest); this binary shows the multi-select
//! model (`aria-multiselectable="true"`, activate-toggle, sibling
//! state untouched). The visual cues mirror the single-select sibling
//! pixel-for-pixel — both binaries on screen side-by-side make the
//! axis observable without an AT client.
//!
//! N = 6 with deliberately-overlapping prefixes (`Apple`,
//! `Apricot`, `Banana`, `Blueberry`, `Cherry`, `Date`) so the
//! R51.103 multi-character type-ahead buffer is demonstrable in the
//! same binary: typing `b` cycles between Banana / Blueberry,
//! typing `bl` (within 500 ms) jumps directly to Blueberry.
//!
//! Keyboard model (WAI-ARIA APG multi-select Listbox):
//!
//! * `ArrowDown` / `ArrowUp` — move `focused_index` (no selection
//!   change).
//! * `Home` / `End` — first / last option (focus only).
//! * `Space` / `Enter` — toggle the focused option (multi-mode
//!   `send` activate path; siblings stay untouched).
//! * Printable letter — type-ahead jump (cyclic single-char,
//!   prefix-match multi-char within 500 ms).
//!
//! AccessKit surfacing (R51.98 §5.40):
//!
//! * Parent listbox: `AriaRole::Listbox` + `aria-multiselectable=true`.
//! * Per option: `AriaRole::ListBoxOption` + `aria-selected=true|false`
//!   (the explicit `false` makes AT announce "not selected" in
//!   multi-select containers per WAI-ARIA 1.2 §6.6.7).
//!
//! Inherits the framework patterns paid through R51.x: composite
//! cancel propagation (R51.93), roving-tabindex active descendant
//! (R51.87 / R51.71), incremental tree updates (R51.72), no-change
//! frame skip (R51.75).

use std::cell::RefCell;
use std::time::Instant;

use pinion_a11y::{AccessAction, AccessFocus, AccessNode, AccessState, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::listbox::ListBoxExternal;
use pinion_core::widgets::listbox_item::ListboxItemState;
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_shell::typeahead::{is_typeahead_char, TypeaheadCursor};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloListboxMultiRenderer, HelloListboxMultiRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 380;
const BG_FILL: Color = Color::rgb(0x18, 0x30, 0x24);
const N: usize = 6;
const PRIMARY_TAG: &str = "main_list";

const ROW_HEIGHT: u32 = 28;
const ROW_WIDTH: u32 = 240;
const ROW_GAP: u32 = 6;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct ListState {
    rows: [(ListboxItemState, bool); N],
    focused: Option<usize>,
}

impl ListState {
    fn idle() -> Self {
        Self {
            rows: [(ListboxItemState::Idle, false); N],
            focused: None,
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ListState, _frame: &Frame) -> Scene {
    let active = active_option_index(state);
    let rows: Vec<Scene> = (0..N)
        .map(|i| listbox_row(i, state.rows[i].0, state.rows[i].1, i == active))
        .collect();
    let column = Scene::Container(
        ContainerNode::new(rows).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_gap(ROW_GAP),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![column])
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// Row visual. Multi-select-aware: every selected row stays blue
/// (no mutual exclusion); the focused row gets a left accent
/// regardless of selection state. The combined "focused AND
/// selected" visual stacks both cues.
fn listbox_row(index: usize, state: ListboxItemState, selected: bool, focused: bool) -> Scene {
    let (fill, label_color, border) = if selected {
        let f = if focused {
            // Focused + selected: brighter blue with a left accent.
            Color::rgb(0x3c, 0x80, 0xe0)
        } else {
            Color::rgb(0x30, 0x70, 0xd0)
        };
        let b = focused.then(|| Border::new(Color::rgb(0xff, 0xff, 0xff), 2));
        (f, Color::rgb(0xff, 0xff, 0xff), b)
    } else {
        let f = match state {
            ListboxItemState::Pressed => Color::rgb(0x2a, 0x48, 0x38),
            ListboxItemState::Hover => Color::rgb(0x2c, 0x52, 0x3e),
            ListboxItemState::Disabled => Color::rgb(0x20, 0x30, 0x28),
            ListboxItemState::Idle => {
                if focused {
                    Color::rgb(0x24, 0x4a, 0x36)
                } else {
                    Color::rgb(0x1f, 0x3a, 0x2c)
                }
            }
        };
        let lc = match state {
            ListboxItemState::Disabled => Color::rgb(0x70, 0x80, 0x76),
            _ => Color::rgb(0xe0, 0xee, 0xe6),
        };
        let b = focused.then(|| Border::new(Color::rgb(0x70, 0xc8, 0xa0), 2));
        (f, lc, b)
    };
    let label = Scene::Text(TextNode::styled(
        option_label(index),
        Rect::default(),
        TextStyle::new().with_size_px(15).with_fg(label_color),
    ));
    let row_tag = format!("{PRIMARY_TAG}#{index}");
    let mut row_style = BoxStyle::filled(fill).with_corner_radius(4);
    if let Some(b) = border {
        row_style = row_style.with_border(b);
    }
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(row_tag)
            .with_style(row_style)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(ROW_WIDTH, ROW_HEIGHT))
                    .with_padding(Rect::new(12, 4, 12, 4)),
            ),
    )
}

fn option_label(index: usize) -> &'static str {
    match index {
        0 => "Apple",
        1 => "Apricot",
        2 => "Banana",
        3 => "Blueberry",
        4 => "Cherry",
        5 => "Date",
        _ => "?",
    }
}

struct ListBoxMultiView;

impl WidgetCore for ListBoxMultiView {
    type State = ListState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ListBoxExternal::with_multiselect(N))
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> ListState {
        let mut out = ListState::idle();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        for (i, slot) in out.rows.iter_mut().enumerate() {
            let state = match intro.query(&format!("state.{i}")) {
                Some(IntrospectValue::Text(name)) => parse_listbox_item_state(&name),
                _ => ListboxItemState::Idle,
            };
            // Multi-mode: per-row selected bool (selected_index is
            // null because no single index applies).
            let selected = matches!(
                intro.query(&format!("selected.{i}")),
                Some(IntrospectValue::Bool(true)),
            );
            *slot = (state, selected);
        }
        out.focused = match intro.query("focused_index") {
            Some(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
            _ => None,
        };
        out
    }

    fn view(state: ListState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-listbox-multi (R51.104 §5.38 aria-multiselectable)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// WAI-ARIA APG multi-select Listbox keyboard model.
    /// Identical to the single-select sibling for `Arrow*` / `Home` /
    /// `End` / type-ahead; `Space` / `Enter` now toggles the focused
    /// row in place (multi-mode `send` is toggle, not replace).
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        if focused != Some(Self::tag()) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        match key {
            "ArrowDown" | "ArrowRight" => move_focus(node, 1),
            "ArrowUp" | "ArrowLeft" => move_focus(node, -1),
            "Home" => set_focus(node, 0),
            "End" => set_focus(node, N - 1),
            "Space" | "Enter" => commit_focused(node),
            other => type_ahead_jump(node, other),
        }
    }

    fn fmt_state_log(state: &ListState) -> String {
        let rows = state
            .rows
            .iter()
            .enumerate()
            .map(|(i, (s, sel))| {
                format!(
                    "{i}={}{}",
                    listbox_state_short(*s),
                    if *sel { "+" } else { "-" },
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        match state.focused {
            Some(idx) => format!("{rows} focused={idx}"),
            None => rows,
        }
    }
}

impl WidgetA11y for ListBoxMultiView {
    /// Emits N + 1 nodes: parent listbox with `aria-multiselectable=true`,
    /// plus one `ListBoxOption` per index with explicit
    /// `aria-selected=true|false` so AT can announce both directions
    /// in a multi-select container (the "explicit false" branch of
    /// WAI-ARIA 1.2 §6.6.7).
    fn access_node(state: &ListState, focused: Option<&str>) -> Vec<AccessNode> {
        let list_focused = focused == Some(<Self as WidgetCore>::tag());
        let active_idx = active_option_index(*state);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(N + 1);
        let mut list = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Listbox)
            .with_name("Fruit picker (multi-select)")
            .with_multiselectable();
        for i in 0..N {
            list = list.with_child(format!("{PRIMARY_TAG}#{i}"));
        }
        nodes.push(list);
        for (i, (item_state, selected)) in state.rows.iter().copied().enumerate() {
            let option_tag = format!("{PRIMARY_TAG}#{i}");
            let access_state = AccessState {
                focused: list_focused && i == active_idx,
                disabled: matches!(item_state, ListboxItemState::Disabled),
                hovered: matches!(item_state, ListboxItemState::Hover),
                pressed: matches!(item_state, ListboxItemState::Pressed),
                checked: None,
            };
            nodes.push(
                AccessNode::new(&option_tag, AriaRole::ListBoxOption)
                    .with_selected(selected)
                    .with_state(access_state),
            );
        }
        nodes
    }

    fn access_focus_target(
        state: &ListState,
        focused: Option<&str>,
    ) -> Option<AccessFocus> {
        if focused == Some(<Self as WidgetCore>::tag()) {
            let idx = active_option_index(*state);
            Some(AccessFocus::composite(
                <Self as WidgetCore>::tag(),
                format!("{PRIMARY_TAG}#{idx}"),
            ))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    fn access_child_invoke(scene: &mut Scene, sub_tag: &str, action: AccessAction) -> bool {
        let Ok(idx) = sub_tag.parse::<usize>() else {
            return false;
        };
        if idx >= N {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match action {
            AccessAction::Click | AccessAction::Default => {
                for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
                    let _ = intro.invoke(
                        "send",
                        IntrospectValue::Text(format!("{idx}:{ev}")),
                    );
                }
                true
            }
            AccessAction::Focus => {
                if let Ok(i) = i64::try_from(idx) {
                    let _ = intro.intervene(
                        "focused_index",
                        IntrospectValue::Int(i),
                    );
                }
                true
            }
            AccessAction::Increment | AccessAction::Decrement | AccessAction::Other => {
                false
            }
        }
    }
}

impl WidgetView for ListBoxMultiView {
    type Renderer = HelloListboxMultiRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }
}

fn move_focus(
    node: &mut pinion_core::scene::ExternalNode,
    direction: i32,
) -> bool {
    let current: Option<usize> = node
        .handle
        .introspect()
        .and_then(|i| i.query("focused_index"))
        .and_then(|v| match v {
            IntrospectValue::Int(i) => usize::try_from(i).ok(),
            _ => None,
        });
    let next = match (current, direction) {
        (Some(c), 1) => (c + 1) % N,
        (Some(c), -1) => (c + N - 1) % N,
        (None, 1) => 0,
        (None, -1) => N - 1,
        _ => 0,
    };
    set_focus(node, next)
}

fn set_focus(node: &mut pinion_core::scene::ExternalNode, idx: usize) -> bool {
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    let Ok(i) = i64::try_from(idx) else {
        return false;
    };
    let _ = intro.intervene("focused_index", IntrospectValue::Int(i));
    true
}

/// `Space` / `Enter`: toggle the focused row. Multi-mode `send`
/// flips only the addressed row's `selected` bit; siblings stay
/// untouched. The full `PointerEnter / Down / Up / Leave` cycle
/// runs through the composite wire format so the `"selected"`
/// intent fires identically to a mouse click (R51.98 multi-mode
/// emits on every toggle, both directions).
fn commit_focused(node: &mut pinion_core::scene::ExternalNode) -> bool {
    let current: Option<usize> = node
        .handle
        .introspect()
        .and_then(|i| i.query("focused_index"))
        .and_then(|v| match v {
            IntrospectValue::Int(i) => usize::try_from(i).ok(),
            _ => None,
        });
    let idx = current.unwrap_or(0);
    if idx >= N {
        return false;
    }
    let Some(intro) = node.handle.introspect_mut() else {
        return false;
    };
    for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
        let _ = intro.invoke(
            "send",
            IntrospectValue::Text(format!("{idx}:{ev}")),
        );
    }
    true
}

/// Active option resolution mirrors the single-select sibling:
/// `focused` wins, then `0` (multi-mode has no "primary selected"
/// fallback because the selection set is a set, not a sole index).
fn active_option_index(state: ListState) -> usize {
    state.focused.unwrap_or(0)
}

// R51.106 §5.38 — type-ahead state lifted to
// `pinion_shell::typeahead` substrate; this binary now holds only
// the per-window thread-local cursor.
thread_local! {
    static TYPEAHEAD: RefCell<TypeaheadCursor> = RefCell::new(TypeaheadCursor::new());
}

fn type_ahead_jump(node: &mut pinion_core::scene::ExternalNode, key: &str) -> bool {
    let Some(first) = is_typeahead_char(key) else {
        return false;
    };
    let current = node
        .handle
        .introspect()
        .and_then(|i| i.query("focused_index"))
        .and_then(|v| match v {
            IntrospectValue::Int(i) => usize::try_from(i).ok(),
            _ => None,
        });
    let labels: [&str; N] = std::array::from_fn(option_label);
    let target = TYPEAHEAD.with(|cell| {
        let mut cursor = cell.borrow_mut();
        cursor.step(first, current, Instant::now(), &labels)
    });
    let Some(idx) = target else {
        return false;
    };
    set_focus(node, idx)
}

fn parse_listbox_item_state(name: &str) -> ListboxItemState {
    match name {
        "Hover" => ListboxItemState::Hover,
        "Pressed" => ListboxItemState::Pressed,
        "Disabled" => ListboxItemState::Disabled,
        _ => ListboxItemState::Idle,
    }
}

fn listbox_state_short(state: ListboxItemState) -> &'static str {
    match state {
        ListboxItemState::Idle => "I",
        ListboxItemState::Hover => "H",
        ListboxItemState::Pressed => "P",
        ListboxItemState::Disabled => "D",
    }
}

fn main() {
    pinion_shell::run::<ListBoxMultiView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn unselected() -> ListState {
        ListState::idle()
    }

    fn multi_selected(indices: &[usize]) -> ListState {
        let mut s = unselected();
        for &i in indices {
            s.rows[i].1 = true;
        }
        s
    }

    #[test]
    fn r51_104_listbox_parent_carries_multiselectable() {
        let nodes = ListBoxMultiView::access_node(&unselected(), None);
        assert!(
            nodes[0].multiselectable,
            "multi-select listbox parent must set aria-multiselectable",
        );
        assert_eq!(nodes[0].role, AriaRole::Listbox);
    }

    #[test]
    fn r51_104_emits_one_listbox_plus_n_option_nodes() {
        let nodes = ListBoxMultiView::access_node(&unselected(), None);
        assert_eq!(nodes.len(), N + 1, "1 listbox parent + N option children");
        for node in &nodes[1..=N] {
            assert_eq!(node.role, AriaRole::ListBoxOption);
        }
    }

    #[test]
    fn r51_104_options_report_explicit_aria_selected_per_row() {
        // Multi-select containers benefit from explicit
        // aria-selected=false (per WAI-ARIA 1.2 §6.6.7); the single-
        // select sibling could omit the false branch, but multi-
        // select should not.
        let state = multi_selected(&[1, 4]);
        let nodes = ListBoxMultiView::access_node(&state, None);
        for i in 0..N {
            let opt = &nodes[i + 1];
            let expected = i == 1 || i == 4;
            assert_eq!(
                opt.selected,
                Some(expected),
                "option {i} aria-selected must be explicit",
            );
        }
    }

    #[test]
    fn r51_104_multi_listbox_focus_target_resolves_active_descendant() {
        let mut state = unselected();
        state.focused = Some(3);
        let target = ListBoxMultiView::access_focus_target(
            &state,
            Some(ListBoxMultiView::tag()),
        )
        .expect("listbox focused → composite focus");
        assert_eq!(target.focus_tag, ListBoxMultiView::tag());
        assert_eq!(
            target.active_descendant.as_deref(),
            Some(format!("{PRIMARY_TAG}#3").as_str())
        );
    }

    #[test]
    fn r51_104_overlapping_prefixes_enable_multi_char_typeahead() {
        // Sanity: the N=6 labels indeed have overlapping prefixes
        // (multiple `A*`, multiple `B*`) so multi-char prefix match
        // is observable.
        let labels: [&str; N] = std::array::from_fn(option_label);
        let count_a: usize = labels
            .iter()
            .filter(|l| l.starts_with('A'))
            .count();
        let count_b: usize = labels
            .iter()
            .filter(|l| l.starts_with('B'))
            .count();
        assert!(
            count_a >= 2 && count_b >= 2,
            "overlapping prefixes required for multi-char typeahead demo",
        );
    }

    #[test]
    fn r51_104_typeahead_multi_char_jumps_to_apricot() {
        // R51.106 substrate test: the binary's overlapping-prefix
        // labels indeed reach Apricot via 'apr' through the lifted
        // `TypeaheadCursor`. Algorithm tests live in
        // `pinion_shell::typeahead`; this is the integration assertion
        // for the binary's specific label set.
        use std::time::Duration;
        let labels: [&str; N] = std::array::from_fn(option_label);
        let mut cursor = TypeaheadCursor::new();
        let t0 = Instant::now();
        assert_eq!(cursor.step('a', None, t0, &labels), Some(0));
        let t1 = t0 + Duration::from_millis(50);
        assert_eq!(cursor.step('p', Some(0), t1, &labels), Some(0));
        let t2 = t1 + Duration::from_millis(50);
        assert_eq!(
            cursor.step('r', Some(0), t2, &labels),
            Some(1),
            "Apricot uniquely matches 'apr'"
        );
    }
}
