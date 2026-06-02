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

use pinion_core::external::External;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widgets::radio::RadioState;
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::{Color, Frame, Scene, WidgetCore};
use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y,
};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::radio_composite as rc;
use pinion_widget_paint::state_layer::state_layer;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloRadioGroupRenderer, HelloRadioGroupRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 280;
/// (R57.X.radio §5.50) [`ThemeProvider`] cache key — matches the
/// `"app"` convention shared across the example gallery.
const THEME_TAG: &str = "app";
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
/// per radio plus the R51.87 §5.40 AT-side active-descendant index.
/// `Copy` because both fields' inner types are `Copy`, so the shell
/// can hand the snapshot into the `paint_producer` closure without
/// lifetime gymnastics.
///
/// R51.87 §5.40 — `focused` is the WAI-ARIA roving-tabindex active
/// descendant. `None` falls back to the selected row (or 0 if no
/// selection) in the `access_focus_target` resolution; `Some(i)`
/// records that an AT `Focus` action (or programmatic intervene)
/// pinned the addressed row to `i` independent of selection.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct GroupState {
    rows: [(RadioState, bool); N],
    focused: Option<usize>,
}

impl GroupState {
    fn idle() -> Self {
        Self {
            rows: [(RadioState::Idle, false); N],
            focused: None,
        }
    }
}

/// view-fn (§6.3): pure sync mapping `GroupState -> Scene`. Builds a
/// vertical column of N rows, each row tagged `"main_group#<i>"` so
/// the `InputRouter`'s R51.42 sub-index split routes cursor hits on
/// row `i` to `invoke("send", Text("<i>:<EventName>"))` against the
/// single composite `RadioGroupExternal`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: GroupState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let rows: Vec<Scene> = (0..N)
        .map(|i| radio_row(i, state.rows[i].0, state.rows[i].1, &theme))
        .collect();
    let column = Scene::Container(
        ContainerNode::new(rows)
            .with_tag(PRIMARY_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Start)
                    .with_gap(ROW_VERTICAL_GAP),
            ),
    );
    // R55.G.18 §5.49 — the inner column carries `PRIMARY_TAG` so the
    // composite root is paint-addressable via `{path: "main_group"}`
    // (AI-side `scene/click` / `scene/key` / `scene/wheel` routing,
    // and `rect_for_tag` AT bounds attach to the radio column rather
    // than the full window). Mirrors `hello-listbox`'s R55.G.17
    // wrapper pattern but folded into the existing column container
    // (no extra layer) since the column already has the layout
    // sidecar that bounds the composite's visual surface.
    Scene::Container(
        ContainerNode::new(vec![column])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
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
/// (R57.X.radio §5.50) Material 3 Radio border colour — mirrors
/// `hello-radio::radio_border_color` (the two binaries share the
/// same visual axis per the existing doc comment). Selected =
/// `Accent`, unselected = `Outline`, hover / pressed / disabled
/// layered via `Color::lerp`.
fn radio_border_color(theme: &Theme, state: RadioState, selected: bool) -> Color {
    let base = if selected {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::Outline)
    };
    state_layer(base, state, theme)
}

fn radio_row(index: usize, state: RadioState, selected: bool, theme: &Theme) -> Scene {
    let border_color = radio_border_color(theme, state, selected);
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
        RadioState::Disabled => theme.resolve(ColorRole::OnSurfaceMuted),
        _ => theme.resolve(ColorRole::OnSurface),
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

impl WidgetCore for RadioGroupView {
    type State = GroupState;
    // The composite drives every state change through `apply_key`
    // (typed wire format `"<i>:<EventName>"`) and the InputRouter
    // sub-index dispatch, so no keybinding-channel events flow
    // through `event_name`. `()` satisfies the trait's `Copy`
    // bound without introducing an unused variant.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(RadioGroupExternal::new(N))
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> GroupState {
        let mut out = GroupState::idle();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        // The R51.43 per-radio query paths, via the shared R732 helper:
        // the introspect channel is the single source of truth (an AI
        // client running `scene/query /external/main_group/state.0` sees
        // exactly the value the view fn renders). `focused_index` is the
        // R51.87 §5.40 AT active descendant (`None` → `selected || 0`).
        rc::read_rows(intro, &mut out.rows);
        out.focused = rc::focused_index(intro);
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

    fn keybinding(_key: &str) -> Option<()> {
        // Every key the composite cares about flows through
        // `apply_key` so the enum-typed channel stays unused. The
        // single source of truth for the keymap lives in
        // `apply_key` below.
        None
    }

    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: pinion_core::Modifiers) -> bool {
        // R51.57 §5.39 — ARIA radio-group roving tabindex through the shared
        // R751 roving-key shell: the group is a single tab stop, and
        // `resolve_target_index` (Arrow / Home / End / a / b / c) routes only
        // when the group itself is focused, so sibling controls keep their
        // keymaps. `RadioGroup::send` enforces mutual exclusion on the
        // activate edge and fires `"selected"` only on a real change.
        rc::roving_key(scene, focused, Self::tag(), key, resolve_target_index)
    }

    fn fmt_state_log(state: &GroupState) -> String {
        let rows = state
            .rows
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
            .join(" ");
        // R51.87 §5.40 — focused_index in the log so stderr traces
        // distinguish AT navigation from selection commits.
        match state.focused {
            Some(idx) => format!("{rows} focused={idx}"),
            None => rows,
        }
    }
}

impl WidgetA11y for RadioGroupView {
    /// R51.66 §5.40 — composite AccessKit semantic tree contribution.
    /// Emits N+1 nodes: one `AriaRole::RadioGroup` parent holding
    /// every radio's sub-tag as a child, plus one
    /// `AriaRole::RadioButton` per index. Each radio carries its
    /// own role, selected state, and interaction state flags. The
    /// parent claims the children via [`AccessNode::with_child`] so
    /// the tree builder routes them under the group instead of the
    /// synthetic root.
    ///
    /// R51.69 §5.40 — per-radio accessible names are derived by
    /// `enrich_names_from_scene` from each row's `tier_label(i)`
    /// `TextNode` (WAI-ARIA name-from-contents). The parent group's
    /// `"Subscription tier"` name has no paint-scene equivalent
    /// (the group is a logical container; the view fn's outermost
    /// node is untagged background fill) so it stays as an explicit
    /// override on the parent `AccessNode`.
    fn access_node(state: &GroupState, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(<Self as WidgetCore>::tag());
        let active_idx = rc::active_index(&state.rows, state.focused);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(N + 1);
        let mut group = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::RadioGroup)
            .with_name("Subscription tier");
        for i in 0..N {
            group = group.with_child(format!("{PRIMARY_TAG}#{i}"));
        }
        nodes.push(group);
        for (i, (radio_state, selected)) in state.rows.iter().copied().enumerate() {
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
                    .with_value(AccessValue::Bool(selected))
                    .with_state(radio_access_state),
            );
        }
        nodes
    }

    /// R51.66 / R51.71 §5.40 — composite focus model. When the
    /// group itself is focused, return [`AccessFocus::composite`]
    /// with the parent tag (`"main_group"`) as the
    /// `TreeUpdate::focus` target and the active radio's sub-tag
    /// as the `aria-activedescendant`. ARIA Authoring Practices
    /// roving-tabindex model — the parent owns the tab stop and
    /// the descendant is reported as the addressed item. Pinion-
    /// shell focus ring and key dispatch continue to address
    /// `"main_group"` itself; only AccessKit consumers see the
    /// descendant hint.
    fn access_focus_target(
        state: &GroupState,
        focused: Option<&str>,
    ) -> Option<AccessFocus> {
        if focused == Some(<Self as WidgetCore>::tag()) {
            let idx = rc::active_index(&state.rows, state.focused);
            Some(rc::composite_focus(<Self as WidgetCore>::tag(), idx))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// R51.70 §5.40 — composite child action dispatch. Mirrors the
    /// `apply_key` wire-format path (`PointerEnter` / `PointerDown` /
    /// `PointerUp` / `PointerLeave` against `format!("{idx}:{ev}")`)
    /// but is reached when an AT invokes `AccessKit::Action::Click` /
    /// `Default` on a specific radio's `NodeId`
    /// (`"main_group#<i>"`), bypassing the keyboard-resolved index
    /// step.
    ///
    /// `Click` / `Default` activate the addressed radio (mutual
    /// exclusion + `"selected"` intent fires through the standard
    /// composite path). `Focus` returns `true` to suppress the
    /// shell's atomic-fallback `apply_key("Enter")` while letting
    /// the parent focus + active-descendant model surface the right
    /// active row visually. Other actions decline so the shell
    /// stays in charge of the fallback chain.
    fn access_child_invoke(scene: &mut Scene, _parent_tag: &str, sub_tag: &str, action: AccessAction) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        rc::child_invoke(intro, sub_tag, action, N)
    }
}

impl WidgetView for RadioGroupView {
    type Renderer = HelloRadioGroupRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
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
        "ArrowDown" | "ArrowRight" => Some(rc::step(intro, 1, N)),
        "ArrowUp" | "ArrowLeft" => Some(rc::step(intro, -1, N)),
        _ => None,
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
        GroupState::idle()
    }

    fn selected_state(idx: usize) -> GroupState {
        let mut s = unselected_state();
        s.rows[idx].1 = true;
        s
    }

    /// R51.87 §5.40 — fixture for AT `Focus`-pinned state. Carries
    /// `focused = Some(idx)` independent of selection so the active
    /// descendant resolution prefers AT navigation over the
    /// selected fallback.
    fn focused_state(focused_idx: usize) -> GroupState {
        let mut s = unselected_state();
        s.focused = Some(focused_idx);
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
        let state = unselected_state();
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = RadioGroupView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(nodes[1].name.as_deref(), Some("Tier 0  (a)"));
        assert_eq!(nodes[2].name.as_deref(), Some("Tier 1  (b)"));
        assert_eq!(nodes[3].name.as_deref(), Some("Tier 2  (c)"));
    }

    #[test]
    fn group_explicit_name_survives_enrichment() {
        let state = unselected_state();
        let scene = pinion_core::Owner::new().run(|| view(state, &Frame::new()));
        let mut nodes = RadioGroupView::access_node(&state, None);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        assert_eq!(
            nodes[0].name.as_deref(),
            Some("Subscription tier"),
            "explicit access_node name must not be overwritten by enrichment",
        );
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
    fn access_focus_target_composite_parent_focus_with_active_radio() {
        let target = RadioGroupView::access_focus_target(
            &selected_state(2),
            Some("main_group"),
        )
        .expect("group focused returns Some");
        // R51.71 §5.40 — focus stays on the parent; active
        // descendant is the selected radio's sub-tag.
        assert_eq!(target.focus_tag, "main_group");
        assert_eq!(target.active_descendant.as_deref(), Some("main_group#2"));
    }

    #[test]
    fn access_focus_target_unselected_active_descendant_is_first() {
        let target = RadioGroupView::access_focus_target(
            &unselected_state(),
            Some("main_group"),
        )
        .expect("group focused returns Some");
        assert_eq!(target.focus_tag, "main_group");
        assert_eq!(target.active_descendant.as_deref(), Some("main_group#0"));
    }

    #[test]
    fn access_focus_target_atomic_when_sibling_focused() {
        let target = RadioGroupView::access_focus_target(
            &selected_state(1),
            Some("save_btn"),
        )
        .expect("non-group focused returns Some(atomic)");
        // R51.71 §5.40 — when a different widget owns focus, the
        // composite contributes an atomic pass-through; no active
        // descendant claim.
        assert_eq!(target.focus_tag, "save_btn");
        assert!(target.active_descendant.is_none());
    }

    #[test]
    fn access_focus_target_none_when_no_focus() {
        let target = RadioGroupView::access_focus_target(&unselected_state(), None);
        assert!(target.is_none());
    }

    // ----- R51.87 §5.40 focused_index regression -----

    #[test]
    fn r51_87_active_descendant_honors_focused_over_selected() {
        // User selected row 0 (mouse / arrow), AT then navigated
        // Focus to row 2 without committing. The active descendant
        // reported to AT should be row 2, not row 0.
        let mut state = selected_state(0);
        state.focused = Some(2);
        let target = RadioGroupView::access_focus_target(
            &state,
            Some("main_group"),
        )
        .expect("group focused returns Some");
        assert_eq!(target.focus_tag, "main_group");
        assert_eq!(
            target.active_descendant.as_deref(),
            Some("main_group#2"),
            "AT-side focused_index must take precedence over selected",
        );
    }

    #[test]
    fn r51_87_active_descendant_falls_back_to_selected_when_focused_none() {
        // Pre-R51.87 behavior — `focused = None` should still resolve
        // active descendant to the selected row.
        let target = RadioGroupView::access_focus_target(
            &selected_state(1),
            Some("main_group"),
        )
        .expect("group focused returns Some");
        assert_eq!(target.active_descendant.as_deref(), Some("main_group#1"));
    }

    #[test]
    fn r51_87_focused_state_marks_correct_radio_focused() {
        // `access_node` consults `active_radio_index` which now
        // honors `state.focused`. AT-focused row 2 should carry
        // `state.focused = true` in its `AccessNode`.
        let nodes =
            RadioGroupView::access_node(&focused_state(2), Some("main_group"));
        assert!(nodes[3].state.focused, "AT-focused radio must mark focused");
        assert!(!nodes[1].state.focused);
        assert!(!nodes[2].state.focused);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::IntrospectValue;
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
        , pinion_core::Modifiers::empty()));
        assert_eq!(selected_index(&s), Some(0));
    }

    #[test]
    fn no_focus_swallows_arrow_silently() {
        // Pre-Tab idle state — group must not steal Arrow keys.
        let mut s = scene();
        assert!(!RadioGroupView::apply_key(&mut s, None, "ArrowDown", pinion_core::Modifiers::empty()));
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
        , pinion_core::Modifiers::empty()));
        assert_eq!(selected_index(&s), None);
    }

    #[test]
    fn focused_main_group_routes_home_to_first() {
        let mut s = scene();
        // Step to last first so Home has somewhere to go from.
        let _ = RadioGroupView::apply_key(&mut s, Some("main_group"), "End", pinion_core::Modifiers::empty());
        assert_eq!(
            selected_index(&s),
            Some(i64::try_from(N - 1).expect("N fits in i64"))
        );
        assert!(RadioGroupView::apply_key(
            &mut s,
            Some("main_group"),
            "Home"
        , pinion_core::Modifiers::empty()));
        assert_eq!(selected_index(&s), Some(0));
    }

    // ----- R51.70 §5.40 access_child_invoke ----------------------

    #[test]
    fn access_child_invoke_click_selects_addressed_radio() {
        let mut s = scene();
        assert!(RadioGroupView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "1",
            AccessAction::Click,
        ));
        assert_eq!(selected_index(&s), Some(1));
    }

    #[test]
    fn access_child_invoke_default_acts_as_click() {
        let mut s = scene();
        assert!(RadioGroupView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "2",
            AccessAction::Default,
        ));
        assert_eq!(selected_index(&s), Some(2));
    }

    #[test]
    fn access_child_invoke_subsequent_click_switches_selection() {
        let mut s = scene();
        assert!(RadioGroupView::access_child_invoke(&mut s, PRIMARY_TAG, "0", AccessAction::Click));
        assert_eq!(selected_index(&s), Some(0));
        assert!(RadioGroupView::access_child_invoke(&mut s, PRIMARY_TAG, "2", AccessAction::Click));
        assert_eq!(selected_index(&s), Some(2));
    }

    #[test]
    fn access_child_invoke_focus_returns_true_without_mutation() {
        let mut s = scene();
        // Pre-select index 1 via Click so we can detect that Focus
        // does not perturb the existing selection.
        let _ = RadioGroupView::access_child_invoke(&mut s, PRIMARY_TAG, "1", AccessAction::Click);
        let before = selected_index(&s);
        assert!(RadioGroupView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "0",
            AccessAction::Focus,
        ));
        assert_eq!(selected_index(&s), before);
    }

    #[test]
    fn access_child_invoke_out_of_range_returns_false() {
        let mut s = scene();
        assert!(!RadioGroupView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "9",
            AccessAction::Click,
        ));
        assert_eq!(selected_index(&s), None);
    }

    #[test]
    fn access_child_invoke_non_numeric_sub_tag_returns_false() {
        let mut s = scene();
        assert!(!RadioGroupView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "foo",
            AccessAction::Click,
        ));
        assert_eq!(selected_index(&s), None);
    }

    #[test]
    fn access_child_invoke_increment_declines_for_composite() {
        let mut s = scene();
        // RadioGroup is not a continuous-value composite — Increment
        // is meaningless here; the shell falls back to parent
        // activation.
        assert!(!RadioGroupView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "0",
            AccessAction::Increment,
        ));
    }

    #[test]
    fn access_child_invoke_other_action_declines() {
        let mut s = scene();
        assert!(!RadioGroupView::access_child_invoke(
            &mut s,
            PRIMARY_TAG,
            "0",
            AccessAction::Other,
        ));
    }

    #[test]
    fn r55_g18_view_contains_composite_paint_root_tag() {
        // R55.G.18 §5.49 — paint scene must carry `PRIMARY_TAG` on
        // the radio column so `{path: "main_group"}` AI-side input
        // routing and `rect_for_tag` AT bounds attach resolve to
        // the composite. Pins the convention against accidental
        // regression (dropping the column-tag in a future refactor).
        //
        // R55.G.22 §5.49 — pinned via the framework helper which
        // calls `V::view` under an `Owner::new()` scope and asserts
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<RadioGroupView>(
            GroupState::idle(),
            &Frame::default(),
        );
    }
}
