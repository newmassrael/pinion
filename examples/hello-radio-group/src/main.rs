//! `hello-radio-group` — R51.44 §5.38 composite hit-target first-
//! client. N=3 [`RadioGroup`](pinion_core::widgets::radio_group::RadioGroup)
//! rendered as a vertical column of three [`Radio`]-style rows, each
//! row tagged with the paint convention `"main_group#<index>"` from
//! the R51.41 RFC. The state scene carries one composite
//! [`RadioGroupExternal`] tagged `"main_group"`; the
//! [`InputRouter`](pinion_runtime::input::InputRouter) splits the
//! `'#'` suffix (R51.42) so a cursor on row `i` drives
//! `invoke("send", Text("<i>:<EventName>"))` against the single
//! composite handle. Framework-owned mutual exclusion (R51.15) snaps
//! `selected_index` to the activated row and emits the `"selected"`
//! intent.
//!
//! Visual contract: three 24×24 rings (outer Container, transparent
//! fill, 2-px border, `corner_radius = 12` for a true circle) each
//! with a `(state, selected)`-driven border colour. Selected rings
//! carry a centred 12×12 filled dot child — the Material / `SwiftUI`
//! / Qt convention hello-radio already uses, replicated three times
//! under the composite group. Each row pairs the ring with a
//! `"Tier {0|1|2}"` label so the multi-radio picker reads at a
//! glance.
//!
//! Keyboard navigation (`apply_key` escape hatch, R51.37): the
//! single-key shortcuts `a` / `b` / `c` select index `0` / `1` / `2`,
//! and the ARIA-standard `ArrowDown` / `ArrowUp` move the selection
//! one step (wrapping at the ends). `Home` / `End` jump to the first
//! and last radio. Selection runs the full `PointerEnter → Down →
//! Up → Leave` activation cycle through the composite's `"send"`
//! wire format so the §5.20 `"selected"` intent fires the same way
//! a real mouse click would (RPC headless / human cursor / keyboard
//! all converge on the same statechart path — §2 invariant #2 holds).
//!
//! AI clients reach the group through the same introspect surface:
//! `query("selected_index")`, the per-radio `query("state.<i>")` /
//! `query("selected.<i>")` paths (R51.43), and the same `invoke
//! "send" → "<i>:<EventName>"` wire format the keyboard path uses.

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::{Color, Frame, Scene};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloRadioGroupRenderer, HelloRadioGroupRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 280;
const BG_FILL: Color = Color::rgb(0x20, 0x30, 0x40);
const N: usize = 3;
const PRIMARY_TAG: &str = "main_group";
// Outer ring + inner dot match the hello-radio single-radio fixture
// so the visual reads as "three copies of hello-radio under a group"
// — the composite extension is structural (state model), not visual.
const RING_SIZE: u32 = 24;
const RING_RADIUS: u32 = 12;
const DOT_SIZE: u32 = 12;
const DOT_RADIUS: u32 = 6;
const ROW_GAP: u32 = 10;
const ROW_VERTICAL_GAP: u32 = 14;

/// Cached projection of the group. One `(RadioState, selected)` pair
/// per radio — `Copy` because `RadioState` is a flat enum and `bool`
/// is trivially copyable, so the shell can hand the snapshot into
/// the `paint_producer` closure without lifetime gymnastics.
type GroupState = [(RadioState, bool); N];

/// view-fn (§6.3): pure sync mapping `GroupState -> Scene`. Builds a
/// vertical column of N rows, each row tagged `"main_group#<i>"` so
/// the `InputRouter`'s R51.42 sub-index split routes cursor hits on
/// row `i` to `invoke("send", Text("<i>:<EventName>"))` against the
/// single composite `RadioGroupExternal`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: GroupState, _frame: &Frame) -> Scene {
    let rows: Vec<Scene> = (0..N)
        .map(|i| radio_row(i, state[i].0, state[i].1))
        .collect();
    let column = Scene::Container(
        ContainerNode::new(rows).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Start)
                .with_gap(ROW_VERTICAL_GAP),
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

/// One row of the composite — ring + label. Tagged
/// `"main_group#<i>"` so the `InputRouter` R51.42 sub-index split
/// (`#` split + `"<i>:<EventName>"` wire-format rewrite) reaches the
/// composite `RadioGroupExternal` with the right radio index in the
/// payload. Visual rendering mirrors `examples/hello-radio` — the
/// composite is a structural / state extension, not a visual one.
fn radio_row(index: usize, state: RadioState, selected: bool) -> Scene {
    let border_color = match (state, selected) {
        (RadioState::Idle, false) => Color::rgb(0xc0, 0xc0, 0xc0),
        (RadioState::Hover, false) => Color::rgb(0xe0, 0xe0, 0xe0),
        (RadioState::Pressed, false) => Color::rgb(0x90, 0x90, 0x90),
        (RadioState::Disabled, false) => Color::rgb(0x70, 0x66, 0x58),
        (RadioState::Idle, true) => Color::rgb(0x30, 0x70, 0xd0),
        (RadioState::Hover, true) => Color::rgb(0x40, 0x80, 0xe0),
        (RadioState::Pressed, true) => Color::rgb(0x20, 0x50, 0xa0),
        (RadioState::Disabled, true) => Color::rgb(0x4a, 0x42, 0x38),
    };
    let mut ring_children: Vec<Scene> = Vec::new();
    if selected {
        ring_children.push(Scene::Box(
            BoxNode::new(
                Rect::default(),
                BoxStyle::filled(border_color).with_corner_radius(DOT_RADIUS),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(DOT_SIZE, DOT_SIZE))),
        ));
    }
    let ring = Scene::Container(
        ContainerNode::new(ring_children)
            .with_style(
                BoxStyle::filled(Color::rgba(0, 0, 0, 0))
                    .with_corner_radius(RING_RADIUS)
                    .with_border(Border::new(border_color, 2)),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(RING_SIZE, RING_SIZE)),
            ),
    );
    let label_color = match state {
        RadioState::Disabled => Color::rgb(0x90, 0x86, 0x78),
        _ => Color::rgb(0xe0, 0xe0, 0xe0),
    };
    let label = Scene::Text(TextNode::styled(
        tier_label(index),
        Rect::default(),
        TextStyle::new().with_size_px(16).with_fg(label_color),
    ));
    let row_tag = format!("{PRIMARY_TAG}#{index}");
    Scene::Container(
        ContainerNode::new(vec![ring, label])
            .with_tag(row_tag)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

fn tier_label(index: usize) -> &'static str {
    match index {
        0 => "Tier 0  (a)",
        1 => "Tier 1  (b)",
        2 => "Tier 2  (c)",
        _ => "Tier ?",
    }
}

struct RadioGroupView;

impl WidgetView for RadioGroupView {
    type State = GroupState;
    // The composite drives every state change through `apply_key`
    // (typed wire format `"<i>:<EventName>"`) and the InputRouter
    // sub-index dispatch, so no keybinding-channel events flow
    // through `event_name`. `()` satisfies the trait's `Copy`
    // bound without introducing an unused variant.
    type Event = ();
    type Renderer = HelloRadioGroupRenderer;

    fn create_external() -> Box<dyn External> {
        Box::new(RadioGroupExternal::new(N))
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> GroupState {
        let mut out: GroupState = [(RadioState::Idle, false); N];
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        // Each radio's interaction state + selected bit comes from
        // the R51.43 per-radio query paths. The introspect channel
        // is the single source of truth: an AI client running
        // `scene/query /external/main_group/state.0` sees exactly
        // the same value the view fn renders.
        for (i, slot) in out.iter_mut().enumerate() {
            let state = match intro.query(&format!("state.{i}")) {
                Some(IntrospectValue::Text(name)) => parse_radio_state(&name),
                _ => RadioState::Idle,
            };
            let selected = matches!(
                intro.query(&format!("selected.{i}")),
                Some(IntrospectValue::Bool(true)),
            );
            *slot = (state, selected);
        }
        out
    }

    fn view(state: GroupState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        // The composite never threads typed events through the
        // shell's keybinding channel — every key flows through
        // `apply_key` which speaks the wire format directly. The
        // `()` event reaches `event_name` only if the trait's
        // default shell path were exercised; routing it to the
        // same opaque `"__internal__"` literal the SCXML-internal
        // Radio events use keeps the symbol consistent.
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-radio-group (R51.44 §5.38 composite hit-target)"
    }

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }

    fn keybinding(_key: &str) -> Option<()> {
        // Every key the composite cares about flows through
        // `apply_key` so the enum-typed channel stays unused. The
        // single source of truth for the keymap lives in
        // `apply_key` below.
        None
    }

    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
        // R51.57 §5.39 — ARIA radio-group roving tabindex. The group
        // is a single tab stop (`focusable_tags()` default returns
        // just `Self::tag()` per the §5.39 caveat on composite focus),
        // and Arrow / Home / End / a / b / c keys route only when the
        // group itself is focused. Sibling controls on the same
        // screen no longer alias the radio-group's keymap.
        if focused != Some(Self::tag()) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let target_index = resolve_target_index(node.handle.introspect(), key);
        let Some(idx) = target_index else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        // ARIA radio-group keyboard activation: the full PointerEnter
        // / Down / Up / Leave cycle runs against the target radio
        // through the composite's wire format. PointerUp is the
        // edge that activates (Radio sets `selected = true` on
        // `Pressed → Hover`); the trailing Leave returns the row's
        // interaction state to `Idle` so the visual doesn't carry a
        // phantom `Hover` under a cursor that isn't actually on the
        // row. `RadioGroup::send` enforces mutual exclusion on the
        // activate edge, deselecting whichever sibling was selected
        // before; the `"selected"` intent fires on the actual
        // selection-change transition only.
        for ev in ["PointerEnter", "PointerDown", "PointerUp", "PointerLeave"] {
            let _ = intro.invoke(
                "send",
                IntrospectValue::Text(format!("{idx}:{ev}")),
            );
        }
        true
    }

    /// R51.66 §5.40 — composite AccessKit semantic tree contribution.
    /// Emits N+1 nodes: one `AriaRole::RadioGroup` parent holding
    /// every radio's sub-tag as a child, plus one
    /// `AriaRole::RadioButton` per index. Each radio carries its
    /// own role, label, selected state, and interaction state
    /// flags. The parent claims the children via
    /// [`AccessNode::with_child`] so the tree builder routes them
    /// under the group instead of the synthetic root.
    fn access_node(state: &GroupState, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(Self::tag());
        let active_idx = active_radio_index(*state);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(N + 1);
        let mut group = AccessNode::new(Self::tag(), AriaRole::RadioGroup)
            .with_name("Subscription tier");
        for i in 0..N {
            group = group.with_child(format!("{PRIMARY_TAG}#{i}"));
        }
        nodes.push(group);
        for (i, (radio_state, selected)) in state.iter().copied().enumerate() {
            let radio_tag = format!("{PRIMARY_TAG}#{i}");
            let radio_access_state = AccessState {
                focused: group_focused && i == active_idx,
                disabled: matches!(radio_state, RadioState::Disabled),
                hovered: matches!(radio_state, RadioState::Hover),
                pressed: matches!(radio_state, RadioState::Pressed),
                checked: Some(selected),
            };
            nodes.push(
                AccessNode::new(&radio_tag, AriaRole::RadioButton)
                    .with_name(tier_label(i))
                    .with_value(AccessValue::Bool(selected))
                    .with_state(radio_access_state),
            );
        }
        nodes
    }

    /// R51.66 §5.40 — redirect AT-side focus from the group tag to
    /// the active descendant radio (the selected radio, or index 0
    /// when nothing is selected — same fallback as the pointer /
    /// keyboard activation paths). The pinion-shell focus ring and
    /// key dispatch continue to address `"main_group"` itself; only
    /// the AT-side `TreeUpdate::focus` is rerouted.
    fn access_focus_target(state: &GroupState, focused: Option<&str>) -> Option<String> {
        if focused == Some(Self::tag()) {
            let idx = active_radio_index(*state);
            Some(format!("{PRIMARY_TAG}#{idx}"))
        } else {
            focused.map(str::to_owned)
        }
    }

    fn fmt_state_log(state: &GroupState) -> String {
        state
            .iter()
            .enumerate()
            .map(|(i, (s, sel))| {
                format!(
                    "{i}={}{}",
                    radio_state_short(*s),
                    if *sel { "+" } else { "-" },
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Resolve a keyboard key to a target radio index, given the
/// composite's current `selected_index`. `a` / `b` / `c` map
/// directly; `ArrowDown` / `ArrowUp` move one step (wrapping);
/// `Home` / `End` jump to the first / last radio. Returns `None`
/// for unrecognised keys so the shell's `apply_key` swallow path
/// matches the unrecognised-keybinding contract.
fn resolve_target_index(
    intro: Option<&dyn pinion_core::external::ExternalIntrospect>,
    key: &str,
) -> Option<usize> {
    match key {
        "a" | "Home" => Some(0),
        "b" => Some(1),
        "c" | "End" => Some(N - 1),
        "ArrowDown" | "ArrowRight" => Some(arrow_step(intro, 1)),
        "ArrowUp" | "ArrowLeft" => Some(arrow_step(intro, -1)),
        _ => None,
    }
}

/// Compute the next target index for an arrow-key step. `direction`
/// is `+1` for `ArrowDown` / `ArrowRight` and `-1` for `ArrowUp` /
/// `ArrowLeft`. Wraps at the ends so the keyboard navigates the
/// group as a cyclic ring (ARIA convention). When no radio is
/// currently selected, an `ArrowDown` lands on `0` (start of the
/// ring) and an `ArrowUp` lands on `N - 1` (end of the ring) — the
/// same boundary the Material `RadioGroup` keyboard navigation
/// reports.
fn arrow_step(
    intro: Option<&dyn pinion_core::external::ExternalIntrospect>,
    direction: i32,
) -> usize {
    let current: Option<usize> = intro
        .and_then(|i| i.query("selected_index"))
        .and_then(|v| match v {
            IntrospectValue::Int(i) => usize::try_from(i).ok(),
            _ => None,
        });
    match (current, direction) {
        (Some(c), 1) => (c + 1) % N,
        (Some(c), -1) => (c + N - 1) % N,
        (None, 1) => 0,
        (None, -1) => N - 1,
        _ => 0,
    }
}

/// R51.66 §5.40 — active radio index (selected radio, or `0` when
/// nothing is selected). Mirrors `arrow_step`'s fallback so the
/// keyboard activation path, the visual focus indication, and the
/// AccessKit `access_focus_target` redirect all agree on which
/// radio is the active descendant.
fn active_radio_index(state: GroupState) -> usize {
    state.iter().position(|(_, sel)| *sel).unwrap_or(0)
}

fn parse_radio_state(name: &str) -> RadioState {
    match name {
        "Hover" => RadioState::Hover,
        "Pressed" => RadioState::Pressed,
        "Disabled" => RadioState::Disabled,
        _ => RadioState::Idle,
    }
}

fn radio_state_short(state: RadioState) -> &'static str {
    match state {
        RadioState::Idle => "I",
        RadioState::Hover => "H",
        RadioState::Pressed => "P",
        RadioState::Disabled => "D",
    }
}

fn main() {
    pinion_shell::run::<RadioGroupView>();
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn unselected_state() -> GroupState {
        [
            (RadioState::Idle, false),
            (RadioState::Idle, false),
            (RadioState::Idle, false),
        ]
    }

    fn selected_state(idx: usize) -> GroupState {
        let mut s = unselected_state();
        s[idx].1 = true;
        s
    }

    #[test]
    fn emits_one_group_plus_n_radio_nodes() {
        let nodes = RadioGroupView::access_node(&unselected_state(), None);
        assert_eq!(nodes.len(), N + 1);
        assert_eq!(nodes[0].role, AriaRole::RadioGroup);
        assert_eq!(nodes[0].name.as_deref(), Some("Subscription tier"));
        for i in 0..N {
            assert_eq!(nodes[i + 1].role, AriaRole::RadioButton);
        }
    }

    #[test]
    fn group_lists_all_children_in_order() {
        let nodes = RadioGroupView::access_node(&unselected_state(), None);
        assert_eq!(nodes[0].children.len(), N);
        for i in 0..N {
            assert_eq!(nodes[0].children[i], format!("main_group#{i}"));
        }
    }

    #[test]
    fn each_radio_has_tier_label() {
        let nodes = RadioGroupView::access_node(&unselected_state(), None);
        assert_eq!(nodes[1].name.as_deref(), Some("Tier 0  (a)"));
        assert_eq!(nodes[2].name.as_deref(), Some("Tier 1  (b)"));
        assert_eq!(nodes[3].name.as_deref(), Some("Tier 2  (c)"));
    }

    #[test]
    fn selected_radio_carries_checked_true() {
        let nodes = RadioGroupView::access_node(&selected_state(1), None);
        assert_eq!(nodes[2].state.checked, Some(true));
        assert_eq!(nodes[2].value, Some(AccessValue::Bool(true)));
        assert_eq!(nodes[1].state.checked, Some(false));
        assert_eq!(nodes[3].state.checked, Some(false));
    }

    #[test]
    fn group_focused_marks_active_radio_focused() {
        let nodes = RadioGroupView::access_node(&selected_state(1), Some("main_group"));
        assert!(nodes[2].state.focused);
        assert!(!nodes[1].state.focused);
        assert!(!nodes[3].state.focused);
    }

    #[test]
    fn unselected_group_focuses_first_radio() {
        let nodes = RadioGroupView::access_node(&unselected_state(), Some("main_group"));
        assert!(nodes[1].state.focused);
    }

    #[test]
    fn unfocused_group_no_radio_focused() {
        let nodes = RadioGroupView::access_node(&selected_state(1), None);
        for radio in nodes.iter().skip(1).take(N) {
            assert!(!radio.state.focused);
        }
    }

    #[test]
    fn access_focus_target_redirects_to_active_radio() {
        let target = RadioGroupView::access_focus_target(
            &selected_state(2),
            Some("main_group"),
        );
        assert_eq!(target.as_deref(), Some("main_group#2"));
    }

    #[test]
    fn access_focus_target_unselected_falls_back_to_first() {
        let target = RadioGroupView::access_focus_target(
            &unselected_state(),
            Some("main_group"),
        );
        assert_eq!(target.as_deref(), Some("main_group#0"));
    }

    #[test]
    fn access_focus_target_passthrough_when_other_focused() {
        let target = RadioGroupView::access_focus_target(
            &selected_state(1),
            Some("save_btn"),
        );
        assert_eq!(target.as_deref(), Some("save_btn"));
    }

    #[test]
    fn access_focus_target_none_when_no_focus() {
        let target = RadioGroupView::access_focus_target(&unselected_state(), None);
        assert!(target.is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;
    use pinion_core::widgets::radio_group::RadioGroupExternal;

    /// Build a fresh group scene mirroring `create_external` so
    /// `apply_key` walks the live shell's exact topology.
    fn scene() -> Scene {
        let ext = RadioGroupExternal::new(N);
        Scene::External(ExternalNode::new(Box::new(ext)).with_tag(PRIMARY_TAG))
    }

    fn selected_index(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else {
            return None;
        };
        node.handle
            .introspect()?
            .query("selected_index")
            .and_then(|v| match v {
                IntrospectValue::Int(i) => Some(i),
                _ => None,
            })
    }

    // ----- R51.57 §5.39 focused-only routing -----

    #[test]
    fn focused_main_group_routes_arrow_to_selection() {
        // Group focused — Arrow advances selection per ARIA radio-
        // group "focus + check" pattern.
        let mut s = scene();
        assert!(RadioGroupView::apply_key(
            &mut s,
            Some("main_group"),
            "ArrowDown"
        ));
        assert_eq!(selected_index(&s), Some(0));
    }

    #[test]
    fn no_focus_swallows_arrow_silently() {
        // Pre-Tab idle state — group must not steal Arrow keys.
        let mut s = scene();
        assert!(!RadioGroupView::apply_key(&mut s, None, "ArrowDown"));
        assert_eq!(selected_index(&s), None);
    }

    #[test]
    fn other_widget_focused_swallows_arrow_silently() {
        // A sibling slider holding focus must keep its Arrow keys —
        // group must not route them.
        let mut s = scene();
        assert!(!RadioGroupView::apply_key(
            &mut s,
            Some("volume_slider"),
            "ArrowDown"
        ));
        assert_eq!(selected_index(&s), None);
    }

    #[test]
    fn focused_main_group_routes_home_to_first() {
        let mut s = scene();
        // Step to last first so Home has somewhere to go from.
        let _ = RadioGroupView::apply_key(&mut s, Some("main_group"), "End");
        assert_eq!(
            selected_index(&s),
            Some(i64::try_from(N - 1).expect("N fits in i64"))
        );
        assert!(RadioGroupView::apply_key(
            &mut s,
            Some("main_group"),
            "Home"
        ));
        assert_eq!(selected_index(&s), Some(0));
    }
}
