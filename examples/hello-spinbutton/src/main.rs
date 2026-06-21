//! `hello-spinbutton` — R734 §5.38 §5.40 WAI-ARIA **spinbutton** (a
//! bounded stepped numeric field; the property-panel "number input" of a
//! DCC / CAD / IDE tool).
//!
//! A spin button is a single focusable numeric control the user steps
//! through a bounded `[min, max]` range. The interaction substrate is the
//! R734 [`SpinButtonExternal`] — a plain value holder (no interaction
//! statechart, mirroring [`ProgressBarExternal`]), distinct from
//! [`SliderExternal`] (a *continuous* normalized drag): a spin button
//! holds a value in **domain units** with an explicit `step`, stepped
//! discretely. That domain difference is why it is its own widget, not a
//! slider re-skin (the "similar-grammar-is-not-a-shared-statechart"
//! rule).
//!
//! Composition: a `[ − ][ value ][ + ]` row. The decrement / increment
//! affordances are paint regions tagged `spin#dec` / `spin#inc`, so a
//! click routes through the R51.42 §5.35 composite hit-target protocol to
//! `invoke("send", "dec:PointerUp")` / `"inc:PointerUp"` against the
//! single [`SpinButtonExternal`] (the `RadioGroup` composite-tag pattern).
//! The numeric readout is the focusable `spinbutton` itself.
//!
//! a11y (R734 §5.40): one [`AriaRole::SpinButton`] node carrying the
//! value as [`AccessValue::Float`] (`aria-valuenow` / `aria-valuemin` /
//! `aria-valuemax`) — the 3rd `Float` consumer after `Slider` +
//! `ProgressBar`, on the *operable* role (Focus + Increment + Decrement AT
//! actions). The − / + regions are presentational (no separate AT nodes);
//! AT users step via the Increment / Decrement actions the arrow keys
//! drive.
//!
//! Keyboard (`apply_key`, WAI-ARIA spinbutton): the readout is a single
//! Tab stop; when focused, `ArrowUp` / `ArrowRight` step + step,
//! `ArrowDown` / `ArrowLeft` step − step, `Home` / `End` jump to min /
//! max, `PageUp` / `PageDown` step by the larger page step. RPC / AI reach
//! the identical arithmetic via `invoke "increment" / "decrement" /
//! "page_up" / "page_down"` and `intervene "value"` (§2 #2).

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::button::ButtonState;
use pinion_core::widgets::spin_button::SpinButtonExternal;
use pinion_core::{Color, Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloSpinButtonRenderer, HelloSpinButtonRendererError);

const WIN_W: u32 = 320;
const WIN_H: u32 = 160;
const THEME_TAG: &str = "app";

/// The focusable spinbutton tag + composite-paint-root (R55.G.17 §5.49).
const TAG: &str = "spin";
/// Decrement / increment paint-region sub-tags (R51.42 §5.35 composite
/// hit-target → `invoke("send", "dec:PointerUp")` / `"inc:PointerUp"`).
const DEC_TAG: &str = "spin#dec";
const INC_TAG: &str = "spin#inc";

/// Quantity field bounds: 0..=10, single step 1, page step 5, start 3.
const MIN: f32 = 0.0;
const MAX: f32 = 10.0;
const STEP: f32 = 1.0;
const PAGE: f32 = 5.0;
const START: f32 = 3.0;

const STEP_W: u32 = 48;
const FIELD_H: u32 = 48;
const READOUT_W: u32 = 96;
const GLYPH_PX: u32 = 24;
const READOUT_PX: u32 = 22;
/// U+2212 MINUS SIGN / U+002B PLUS SIGN — the stepper glyphs (named
/// const + escape per the non-ASCII-source rule; raw glyph in docs only).
const MINUS_GLYPH: &str = "\u{2212}";
const PLUS_GLYPH: &str = "+";

/// Cached projection: the current value + its (construction-fixed) range,
/// all read from the live [`SpinButtonExternal`] so the cache and the AI
/// client never diverge. `Copy` (all `f32`).
#[derive(Copy, Clone, PartialEq, Debug)]
struct SpinState {
    value: f32,
    min: f32,
    max: f32,
    /// R734.2 — per-stepper interaction state (drives the hover / pressed
    /// state-layer paint).
    dec: ButtonState,
    inc: ButtonState,
}

impl SpinState {
    fn boot() -> Self {
        Self {
            value: START,
            min: MIN,
            max: MAX,
            dec: ButtonState::Idle,
            inc: ButtonState::Idle,
        }
    }
}

/// view-fn (§6.3): pure sync `SpinState -> Scene`. A `[ − ][ value ][ + ]`
/// row, the whole row tagged `TAG` (the focusable spinbutton + composite
/// paint root), with the − / + regions tagged `spin#dec` / `spin#inc`.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &SpinState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let field = Scene::Container(
        ContainerNode::new(vec![
            stepper(DEC_TAG, MINUS_GLYPH, state.dec, &theme),
            readout(state.value, &theme),
            stepper(INC_TAG, PLUS_GLYPH, state.inc, &theme),
        ])
        .with_tag(TAG)
        .with_style(
            BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest))
                .with_corner_radius(FIELD_H / 2),
        )
        // (R1030 §5.39) hand-composed focus stop — composing view owns the opt-in.
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_focusable(true)
                .with_gap(0),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![field])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// A circular − / + stepper affordance, tagged for composite-pointer
/// routing. Presentational at the AT layer (the spinbutton owns the
/// Increment / Decrement actions). R734.2 — `state` is the stepper's own
/// `Button` interaction state; the fill layers the shared M3 hover (0.08)
/// / pressed (0.12) `OnSurface` state overlay so the affordance has full
/// desktop-grade pointer feedback.
fn stepper(tag: &'static str, glyph: &str, state: ButtonState, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            glyph,
            Rect::default(),
            TextStyle::new()
                .with_size_px(GLYPH_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(tag)
        .with_style(BoxStyle::filled(stepper_fill(theme, state)).with_corner_radius(FIELD_H / 2))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(STEP_W, FIELD_H)),
        ),
    )
}

/// Stepper fill = the `SurfaceContainerHighest` base with the shared M3
/// hover / pressed `OnSurface` state-layer (0.08 / 0.12) — the same overlay
/// `hello-segmented-button` / `hello-radio-group` use, applied per stepper.
fn stepper_fill(theme: &Theme, state: ButtonState) -> Color {
    let base = theme.resolve(ColorRole::SurfaceContainerHighest);
    match state {
        ButtonState::Idle | ButtonState::Disabled => base,
        ButtonState::Hover => base.lerp(theme.resolve(ColorRole::OnSurface), 0.08),
        ButtonState::Pressed => base.lerp(theme.resolve(ColorRole::OnSurface), 0.12),
    }
}

/// The numeric readout (the spinbutton's visible value). Integer-formatted
/// because the demo uses whole steps; untagged so a click on it falls
/// through to the `TAG` container (a no-op `"PointerUp"` with no sub-index).
fn readout(value: f32, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            format!("{value:.0}"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(READOUT_PX)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(READOUT_W, FIELD_H)),
        ),
    )
}

/// Read `(value, min, max)` from the live `SpinButtonExternal` via the
/// §5.15 introspect channel — the same path an RPC
/// `scene/query /external/value` request walks.
fn read_spin(scene: &Scene) -> SpinState {
    let mut out = SpinState::boot();
    let Scene::External(node) = scene else {
        return out;
    };
    let Some(intro) = node.handle.introspect() else {
        return out;
    };
    if let Some(IntrospectValue::Float(v)) = intro.query("value") {
        #[allow(clippy::cast_possible_truncation)]
        {
            out.value = v as f32;
        }
    }
    if let Some(IntrospectValue::Float(v)) = intro.query("min") {
        #[allow(clippy::cast_possible_truncation)]
        {
            out.min = v as f32;
        }
    }
    if let Some(IntrospectValue::Float(v)) = intro.query("max") {
        #[allow(clippy::cast_possible_truncation)]
        {
            out.max = v as f32;
        }
    }
    if let Some(IntrospectValue::Text(name)) = intro.query("dec_state") {
        out.dec = ButtonState::from_name_or_default(&name);
    }
    if let Some(IntrospectValue::Text(name)) = intro.query("inc_state") {
        out.inc = ButtonState::from_name_or_default(&name);
    }
    out
}

struct SpinButtonView;

impl WidgetCore for SpinButtonView {
    type State = SpinState;
    // Every mutation flows through `apply_key` (keyboard) or the
    // InputRouter composite pointer dispatch (the − / + regions), never
    // the enum-typed `keybinding` channel — so `()` satisfies the bound.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(SpinButtonExternal::new(START, MIN, MAX, STEP).with_page_step(PAGE))
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> SpinState {
        read_spin(scene)
    }

    fn view(state: SpinState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-spinbutton (R734 §5.38 WAI-ARIA spinbutton)"
    }

    /// WAI-ARIA spinbutton keyboard model, gated on the spinbutton owning
    /// focus (roving-tabindex `apply_key` discipline). Steps route through
    /// the same `invoke` / `intervene` arithmetic the pointer + RPC use.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(TAG) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        match key {
            "ArrowUp" | "ArrowRight" => intro.invoke("increment", IntrospectValue::Null).is_ok(),
            "ArrowDown" | "ArrowLeft" => intro.invoke("decrement", IntrospectValue::Null).is_ok(),
            "PageUp" => intro.invoke("page_up", IntrospectValue::Null).is_ok(),
            "PageDown" => intro.invoke("page_down", IntrospectValue::Null).is_ok(),
            "Home" => intro
                .intervene("value", IntrospectValue::Float(f64::from(MIN)))
                .is_ok(),
            "End" => intro
                .intervene("value", IntrospectValue::Float(f64::from(MAX)))
                .is_ok(),
            _ => false,
        }
    }

    fn fmt_state_log(state: &SpinState) -> String {
        format!(
            "value={:.0} [{:.0}..{:.0}]",
            state.value, state.min, state.max
        )
    }
}

impl WidgetA11y for SpinButtonView {
    /// R734 §5.40 — one `spinbutton` node carrying the value as
    /// [`AccessValue::Float`] (`aria-valuenow` / `aria-valuemin` /
    /// `aria-valuemax`). The − / + regions are presentational (no separate
    /// AT nodes); the spinbutton's operable role surfaces the Increment /
    /// Decrement AT actions the arrow keys drive.
    fn access_node(state: &SpinState, focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(TAG, AriaRole::SpinButton)
                .with_name("Quantity")
                .with_value(AccessValue::Float {
                    value: state.value,
                    min: state.min,
                    max: state.max,
                })
                .with_state(AccessState {
                    focused: focused == Some(TAG),
                    ..AccessState::default()
                }),
        ]
    }
}

impl WidgetView for SpinButtonView {
    type Renderer = HelloSpinButtonRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<SpinButtonView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene_fixture() -> Scene {
        use pinion_core::scene::ExternalNode;
        Scene::External(ExternalNode::new(SpinButtonView::create_external()).with_tag(TAG))
    }

    // ── view / composition ────────────────────────────────────────────

    #[test]
    fn view_carries_primary_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<SpinButtonView>(
            SpinState::boot(),
            &Frame::new(),
        );
    }

    #[test]
    fn view_carries_both_stepper_regions() {
        let scene = pinion_core::Owner::new().run(|| view(&SpinState::boot(), &Frame::new()));
        assert!(scene.contains_tag(DEC_TAG), "decrement region present");
        assert!(scene.contains_tag(INC_TAG), "increment region present");
    }

    // ── keyboard ───────────────────────────────────────────────────────

    #[test]
    fn arrows_step_by_one_and_clamp() {
        let mut scene = scene_fixture();
        assert!(SpinButtonView::apply_key(
            &mut scene,
            Some(TAG),
            "ArrowUp",
            pinion_core::Modifiers::empty()
        ));
        assert!(
            (SpinButtonView::read_state(&scene).value - 4.0).abs() < f32::EPSILON,
            "3 -> 4"
        );
        assert!(SpinButtonView::apply_key(
            &mut scene,
            Some(TAG),
            "ArrowDown",
            pinion_core::Modifiers::empty()
        ));
        assert!(SpinButtonView::apply_key(
            &mut scene,
            Some(TAG),
            "ArrowDown",
            pinion_core::Modifiers::empty()
        ));
        assert!(
            (SpinButtonView::read_state(&scene).value - 2.0).abs() < f32::EPSILON,
            "4 -> 2"
        );
    }

    #[test]
    fn page_keys_step_by_five() {
        let mut scene = scene_fixture();
        assert!(SpinButtonView::apply_key(
            &mut scene,
            Some(TAG),
            "PageUp",
            pinion_core::Modifiers::empty()
        ));
        assert!(
            (SpinButtonView::read_state(&scene).value - 8.0).abs() < f32::EPSILON,
            "3 +5 -> 8"
        );
        assert!(SpinButtonView::apply_key(
            &mut scene,
            Some(TAG),
            "PageDown",
            pinion_core::Modifiers::empty()
        ));
        assert!(
            (SpinButtonView::read_state(&scene).value - 3.0).abs() < f32::EPSILON,
            "8 -5 -> 3"
        );
    }

    #[test]
    fn home_and_end_jump_to_min_max() {
        let mut scene = scene_fixture();
        assert!(SpinButtonView::apply_key(
            &mut scene,
            Some(TAG),
            "End",
            pinion_core::Modifiers::empty()
        ));
        assert!(
            (SpinButtonView::read_state(&scene).value - MAX).abs() < f32::EPSILON,
            "End -> max"
        );
        assert!(SpinButtonView::apply_key(
            &mut scene,
            Some(TAG),
            "Home",
            pinion_core::Modifiers::empty()
        ));
        assert!(
            (SpinButtonView::read_state(&scene).value - MIN).abs() < f32::EPSILON,
            "Home -> min"
        );
    }

    #[test]
    fn keys_ignored_when_not_focused() {
        let mut scene = scene_fixture();
        assert!(!SpinButtonView::apply_key(
            &mut scene,
            None,
            "ArrowUp",
            pinion_core::Modifiers::empty()
        ));
        assert!(!SpinButtonView::apply_key(
            &mut scene,
            Some("other"),
            "ArrowUp",
            pinion_core::Modifiers::empty()
        ));
        assert!(
            (SpinButtonView::read_state(&scene).value - START).abs() < f32::EPSILON,
            "value unchanged"
        );
    }

    // ── a11y ───────────────────────────────────────────────────────────

    #[test]
    fn emits_one_spinbutton_node_with_float_value() {
        let nodes = SpinButtonView::access_node(&SpinState::boot(), None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::SpinButton);
        assert_eq!(nodes[0].name.as_deref(), Some("Quantity"));
        assert_eq!(
            nodes[0].value,
            Some(AccessValue::Float {
                value: START,
                min: MIN,
                max: MAX
            }),
            "aria-valuenow / valuemin / valuemax",
        );
    }

    #[test]
    fn focused_flag_tracks_the_spinbutton_tag() {
        assert!(
            SpinButtonView::access_node(&SpinState::boot(), Some(TAG))[0]
                .state
                .focused
        );
        assert!(
            !SpinButtonView::access_node(&SpinState::boot(), None)[0]
                .state
                .focused
        );
    }
}
