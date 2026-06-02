//! `hello-stepper` — R750 §5.38 §5.40 horizontal stepper (progress steps).
//!
//! A stepper presents an ordered process as a row of numbered steps; the
//! **active step** is marked `aria-current="step"`. Earlier steps are
//! *completed* (a check glyph), the active step is *current* (filled
//! indicator + number), later steps are *upcoming* (outlined indicator).
//! Selecting a step navigates the process to it (a non-linear stepper —
//! any step is reachable, the MUI / Material `StepButton` model).
//!
//! ## substrate-0 interaction + the 2nd `AriaCurrent` consumer
//!
//! The "current step" is exactly a **1-of-N exclusive selection**, so this
//! binding reuses the R51.44 [`RadioGroupExternal`] coordinator (selected
//! index = current step) with **no new interaction substrate** — the same
//! composition `hello-breadcrumb` (R731) and `hello-segmented-button` use.
//! What R750 adds is the **2nd consumer of the [`AriaCurrent`] axis**: it
//! exercises [`AriaCurrent::Step`], the variant R731 minted *for exactly
//! this widget* while only the breadcrumb's [`AriaCurrent::Page`] had a
//! consumer (clearing the R731 "stepper=step future consumer" carry,
//! validating the closed positive-vocabulary enum per the
//! second-consumer rule). Each step is an [`AriaRole::Button`] (it changes
//! state in-context — distinct from the breadcrumb's [`AriaRole::Link`],
//! which navigates to another resource) under an [`AriaRole::Group`]
//! parent, the same group-of-buttons shape the multi-select segmented
//! button uses.
//!
//! ## Keyboard model (honest carry — shared with breadcrumb)
//!
//! The strip is a **single tab stop** with `ArrowRight` / `ArrowLeft`
//! roving + `Home` / `End` (the [`RadioGroupExternal`] roving model),
//! reported to AT as the Group owning the tab stop with the active step as
//! `aria-activedescendant`. The strict per-step-Tab variant (each step in
//! the normal Tab order) is a deferred axis; pointer / RPC / AT all reach
//! every step regardless.
//!
//! ## AI clients (§2 invariant #2)
//!
//! `query("selected_index")` is the current step; `scene/click
//! {path:"stepper#<i>"}`, `focus/set` + `scene/key`, and the AccessKit
//! tree (`group` > `button` with `aria-current="step"`) all converge on
//! the same `RadioGroupExternal` statechart.

use pinion_core::external::External;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::style::Border;
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widgets::radio::{RadioEvent, RadioState};
use pinion_core::widgets::radio_group::RadioGroupExternal;
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, AccessState, AriaCurrent, AriaRole, WidgetA11y,
};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::radio_composite as rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloStepperRenderer, HelloStepperRendererError);

const WIN_W: u32 = 620;
const WIN_H: u32 = 120;
const THEME_TAG: &str = "app";
const N: usize = 4;
const PRIMARY_TAG: &str = "stepper";

/// The step labels (single source — paint label + a11y button name).
const STEPS: [&str; N] = ["Account", "Address", "Payment", "Review"];

/// Completed-step indicator glyph (U+2713 CHECK MARK). A named const with a
/// `\u{}` escape keeps the source ASCII-clean (the project's non-ASCII
/// literal convention).
const CHECK: &str = "\u{2713}";

const LABEL_FONT_PX: u32 = 15;
const NUM_FONT_PX: u32 = 15;
/// Step-indicator circle diameter (corner radius = half = full round).
const CIRCLE: u32 = 32;
const CIRCLE_R: u32 = CIRCLE / 2;
/// Gap between a step's circle and its label.
const CIRCLE_LABEL_GAP: u32 = 8;
/// Connector segment between two consecutive steps.
const CONNECTOR_W: u32 = 40;
const CONNECTOR_H: u32 = 2;

/// Where a step sits relative to the current step. Derived at view time
/// from `(index, current_index)` — a pure paint decision, not a stored
/// projection (the canonical state stays the `RadioGroupExternal`
/// selection; this only chooses the glyph + tone, exactly as the
/// breadcrumb chooses `OnSurface`-vs-`Accent` from `selected`).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Phase {
    Completed,
    Current,
    Upcoming,
}

impl Phase {
    fn of(index: usize, current: Option<usize>) -> Self {
        match current {
            Some(cur) if index < cur => Self::Completed,
            Some(cur) if index == cur => Self::Current,
            _ => Self::Upcoming,
        }
    }
}

/// Cached projection: one `(RadioState, selected)` per step + the §5.40
/// active-descendant index. `Copy` for the paint closure.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct StepperState {
    rows: [(RadioState, bool); N],
    focused: Option<usize>,
}

impl StepperState {
    fn idle() -> Self {
        Self {
            rows: [(RadioState::Idle, false); N],
            focused: None,
        }
    }

    /// Index of the current step (the selected row), if any.
    fn current(&self) -> Option<usize> {
        self.rows.iter().position(|(_, sel)| *sel)
    }
}

/// view-fn (§6.3): a Group landmark holding the step strip
/// `Account — Address — Payment — Review` with connectors between steps.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: StepperState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let current = state.current();
    let mut row: Vec<Scene> = Vec::with_capacity(N * 2 - 1);
    for i in 0..N {
        if i > 0 {
            // The connector leading into step `i` is "done" once the prior
            // step is completed (i.e. step `i` has been reached or passed).
            let done = current.is_some_and(|cur| i <= cur);
            row.push(connector(done, &theme));
        }
        row.push(step(i, Phase::of(i, current), state.rows[i].0, &theme));
    }
    // PRIMARY_TAG on the strip so `{path:"stepper"}` AI routing +
    // `rect_for_tag` AT bounds attach to the Group.
    let strip = Scene::Container(
        ContainerNode::new(row)
            .with_tag(PRIMARY_TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![strip])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// One step, tagged `"stepper#<i>"` for R51.42 sub-index routing: a
/// circular indicator (filled `Accent` for completed / current, outlined
/// for upcoming) beside its label. The shared hover / pressed state-layer
/// overlay tints the indicator fill.
fn step(index: usize, phase: Phase, state: RadioState, theme: &Theme) -> Scene {
    let (fill_base, glyph_color, label_color, border) = match phase {
        // Completed + Current share the filled-`Accent` indicator (they
        // diverge only in the glyph — check vs number, chosen below).
        Phase::Completed | Phase::Current => (
            theme.resolve(ColorRole::Accent),
            theme.resolve(ColorRole::OnAccent),
            theme.resolve(ColorRole::OnSurface),
            None,
        ),
        Phase::Upcoming => (
            theme.resolve(ColorRole::SurfaceContainerHighest),
            theme.resolve(ColorRole::OnSurfaceMuted),
            theme.resolve(ColorRole::OnSurfaceMuted),
            Some(Border::new(theme.resolve(ColorRole::Outline), 1)),
        ),
    };
    let fill = rc::state_layer(fill_base, state, theme);
    let glyph = if phase == Phase::Completed {
        CHECK.to_string()
    } else {
        (index + 1).to_string()
    };

    let mut circle_style = BoxStyle::filled(fill).with_corner_radius(CIRCLE_R);
    if let Some(b) = border {
        circle_style = circle_style.with_border(b);
    }
    let circle = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            glyph,
            Rect::default(),
            TextStyle::new().with_size_px(NUM_FONT_PX).with_fg(glyph_color),
        ))])
        .with_style(circle_style)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(CIRCLE, CIRCLE)),
        ),
    );

    let label = Scene::Text(TextNode::styled(
        STEPS[index],
        Rect::default(),
        TextStyle::new().with_size_px(LABEL_FONT_PX).with_fg(label_color),
    ));

    Scene::Container(
        ContainerNode::new(vec![circle, label])
            .with_tag(format!("{PRIMARY_TAG}#{index}"))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_gap(CIRCLE_LABEL_GAP),
            ),
    )
}

/// Connector segment between two steps. A "done" connector (the segment
/// leading into a reached step) is `Accent`; an upcoming one is `Outline`.
/// Decorative — no tag, no a11y node.
fn connector(done: bool, theme: &Theme) -> Scene {
    let color = if done {
        theme.resolve(ColorRole::Accent)
    } else {
        theme.resolve(ColorRole::Outline)
    };
    Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(color).with_corner_radius(CONNECTOR_H / 2),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(CONNECTOR_W, CONNECTOR_H))),
    )
}

struct StepperView;

impl WidgetCore for StepperView {
    type State = StepperState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        let mut ext = RadioGroupExternal::new(N);
        // Default current step = the first step ("Account"): a stepper
        // starts at the beginning of its process. KeyboardActivate selects
        // without a hover / pressed residue (state stays Idle).
        ext.send(0, RadioEvent::KeyboardActivate);
        Box::new(ext)
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> StepperState {
        let mut out = StepperState::idle();
        let Scene::External(node) = scene else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        rc::read_rows(intro, &mut out.rows);
        out.focused = rc::focused_index(intro);
        out
    }

    fn view(state: StepperState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-stepper (R750 §5.38 §5.40 progress steps)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        // Navigate to the resolved step through the shared roving-key shell
        // (1-of-N exclusion + `"selected"` intent); `resolve_target_index`
        // is this stepper's opinionated ArrowLeft/Right + Home/End key map.
        rc::roving_key(scene, focused, Self::tag(), key, resolve_target_index)
    }

    fn fmt_state_log(state: &StepperState) -> String {
        let cur = state
            .current()
            .map_or_else(|| "none".to_string(), |i| i.to_string());
        format!("current={cur}")
    }
}

impl WidgetA11y for StepperView {
    /// §5.40 — an [`AriaRole::Group`] whose children are the step
    /// [`AriaRole::Button`] nodes; the current step (selected) carries
    /// `aria-current="step"`. Button names come from [`STEPS`].
    fn access_node(state: &StepperState, focused: Option<&str>) -> Vec<AccessNode> {
        let group_focused = focused == Some(<Self as WidgetCore>::tag());
        let active_idx = rc::active_index(&state.rows, state.focused);
        let mut nodes: Vec<AccessNode> = Vec::with_capacity(N + 1);
        let mut group = AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Group)
            .with_name("Progress");
        for i in 0..N {
            group = group.with_child(format!("{PRIMARY_TAG}#{i}"));
        }
        nodes.push(group);
        for (i, (step_state, selected)) in state.rows.iter().copied().enumerate() {
            let step_tag = format!("{PRIMARY_TAG}#{i}");
            let mut button = AccessNode::new(&step_tag, AriaRole::Button)
                .with_name(STEPS[i])
                .with_state(AccessState {
                    focused: group_focused && i == active_idx,
                    disabled: matches!(step_state, RadioState::Disabled),
                    hovered: matches!(step_state, RadioState::Hover),
                    pressed: matches!(step_state, RadioState::Pressed),
                    checked: None,
                });
            if selected {
                button = button.with_current(AriaCurrent::Step);
            }
            nodes.push(button);
        }
        nodes
    }

    /// §5.40 composite focus model — when the strip owns focus, the Group
    /// is the `TreeUpdate::focus` target and the active step is the
    /// `aria-activedescendant` (the roving model; see the module docs for
    /// the per-step-Tab alternative).
    fn access_focus_target(state: &StepperState, focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(<Self as WidgetCore>::tag()) {
            let idx = rc::active_index(&state.rows, state.focused);
            Some(rc::composite_focus(<Self as WidgetCore>::tag(), idx))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// §5.40 composite child action dispatch — an AT `Click` / `Default`
    /// on a step navigates to it; `Focus` pins the active descendant.
    fn access_child_invoke(
        scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        rc::child_invoke(intro, sub_tag, action, N)
    }
}

impl WidgetView for StepperView {
    type Renderer = HelloStepperRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Resolve a key to a target step index. `ArrowRight` / `ArrowLeft` step
/// (wrapping); `Home` / `End` jump to the first / last step.
fn resolve_target_index(
    intro: Option<&dyn pinion_core::external::ExternalIntrospect>,
    key: &str,
) -> Option<usize> {
    match key {
        "Home" => Some(0),
        "End" => Some(N - 1),
        "ArrowRight" | "ArrowDown" => Some(rc::step(intro, 1, N)),
        "ArrowLeft" | "ArrowUp" => Some(rc::step(intro, -1, N)),
        _ => None,
    }
}

fn main() {
    pinion_shell::run::<StepperView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::IntrospectValue;
    use pinion_core::scene::ExternalNode;

    fn selected_state(idx: usize) -> StepperState {
        let mut s = StepperState::idle();
        s.rows[idx].1 = true;
        s
    }

    #[test]
    fn group_with_n_button_children() {
        let nodes = StepperView::access_node(&selected_state(0), None);
        assert_eq!(nodes.len(), N + 1);
        assert_eq!(nodes[0].role, AriaRole::Group);
        assert_eq!(nodes[0].name.as_deref(), Some("Progress"));
        for i in 0..N {
            assert_eq!(nodes[i + 1].role, AriaRole::Button);
            assert_eq!(nodes[i + 1].name.as_deref(), Some(STEPS[i]));
        }
    }

    #[test]
    fn current_step_carries_aria_current_step() {
        let nodes = StepperView::access_node(&selected_state(2), None);
        assert_eq!(nodes[3].current, Some(AriaCurrent::Step), "step 2 is current");
        assert!(nodes[1].current.is_none(), "non-current step has no aria-current");
        assert!(nodes[2].current.is_none());
        assert!(nodes[4].current.is_none());
    }

    #[test]
    fn phase_partitions_around_current() {
        // current = 2 → steps 0,1 completed, 2 current, 3 upcoming.
        assert_eq!(Phase::of(0, Some(2)), Phase::Completed);
        assert_eq!(Phase::of(1, Some(2)), Phase::Completed);
        assert_eq!(Phase::of(2, Some(2)), Phase::Current);
        assert_eq!(Phase::of(3, Some(2)), Phase::Upcoming);
        // no current → every step is upcoming.
        assert_eq!(Phase::of(0, None), Phase::Upcoming);
    }

    #[test]
    fn focused_group_marks_active_step() {
        let nodes = StepperView::access_node(&selected_state(1), Some(PRIMARY_TAG));
        assert!(nodes[2].state.focused);
        assert!(!nodes[1].state.focused);
    }

    #[test]
    fn focus_target_is_composite_group_plus_active_descendant() {
        let target = StepperView::access_focus_target(&selected_state(2), Some(PRIMARY_TAG))
            .expect("group focused returns Some");
        assert_eq!(target.focus_tag, PRIMARY_TAG);
        assert_eq!(target.active_descendant.as_deref(), Some("stepper#2"));
    }

    fn scene() -> Scene {
        let ext = RadioGroupExternal::new(N);
        Scene::External(ExternalNode::new(Box::new(ext)).with_tag(PRIMARY_TAG))
    }

    fn current_index(scene: &Scene) -> Option<i64> {
        let Scene::External(node) = scene else { return None };
        node.handle.introspect()?.query("selected_index").and_then(|v| match v {
            IntrospectValue::Int(i) => Some(i),
            _ => None,
        })
    }

    #[test]
    fn arrow_navigates_and_wraps_when_focused() {
        let mut s = scene();
        for expected in [0_i64, 1, 2, 3, 0] {
            assert!(StepperView::apply_key(
                &mut s,
                Some(PRIMARY_TAG),
                "ArrowRight",
                pinion_core::Modifiers::empty(),
            ));
            assert_eq!(current_index(&s), Some(expected));
        }
    }

    #[test]
    fn home_end_jump_to_edges() {
        let mut s = scene();
        assert!(StepperView::apply_key(&mut s, Some(PRIMARY_TAG), "End", pinion_core::Modifiers::empty()));
        assert_eq!(current_index(&s), Some(i64::try_from(N - 1).unwrap()));
        assert!(StepperView::apply_key(&mut s, Some(PRIMARY_TAG), "Home", pinion_core::Modifiers::empty()));
        assert_eq!(current_index(&s), Some(0));
    }

    #[test]
    fn unfocused_swallows_arrow() {
        let mut s = scene();
        assert!(!StepperView::apply_key(&mut s, None, "ArrowRight", pinion_core::Modifiers::empty()));
        assert_eq!(current_index(&s), None);
    }

    #[test]
    fn at_click_navigates_to_step() {
        let mut s = scene();
        assert!(StepperView::access_child_invoke(&mut s, PRIMARY_TAG, "1", AccessAction::Click));
        assert_eq!(current_index(&s), Some(1));
    }

    #[test]
    fn view_carries_group_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<StepperView>(
            StepperState::idle(),
            &Frame::default(),
        );
    }
}
