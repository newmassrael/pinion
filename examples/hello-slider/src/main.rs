//! `hello-slider` — R51.35 §5.38 paint-side N=5 amortization on the
//! pinion-shell substrate (R51.30), the first visual demo that
//! consumes the R51.34 §5.15 + §5.35 pointer-capture + `pointer_move`
//! forward substrate, and the first client of the R51.37 §5.35
//! [`WidgetView::apply_key`] hook (W3C/ARIA Slider keyboard
//! accessibility). The widget body has no drag-specific code — the
//! framework's [`InputRouter`](pinion_runtime::InputRouter) routes
//! the cursor X through `External::pointer_move` and the
//! `SliderExternal` impl rewrites the value sidecar on every
//! effective change. Click-to-position (Material precedent) is free
//! out of the substrate's R51.35.a click-point patch.
//!
//! Visual contract: Material-style horizontal slider with a 200×8
//! pill track split into a filled-portion / thumb / unfilled-portion
//! triple. The thumb (16×16 circle with `corner_radius = 8`) sits at
//! the value position; the filled portion is the blue accent left of
//! the thumb, the unfilled portion is the dim grey right of it. The
//! whole row carries the `main_slider` tag so the shell's
//! `InputRouter` routes pointer events to the matching
//! `Scene::External("main_slider")`.
//!
//! State / colour cross product:
//!
//! * Idle: white thumb, grey-blue filled, no extra affordance
//! * Hover: brighter thumb (the cursor is *over* the rect but not
//!   yet pressed)
//! * Dragging: light-violet thumb (visual cue that capture is in
//!   flight — Material's "pressed" affordance)
//! * Disabled: muted brown-grey thumb + muted brown-grey filled
//!
//! Keybindings (typed-event channel): `d` / `e` Disable / Enable —
//! routed through [`WidgetView::keybinding`] because both map to
//! existing `SliderEvent` variants.
//!
//! Keybindings (R51.37 §5.35 `apply_key` channel, W3C/ARIA Slider
//! accessibility):
//!
//! * `ArrowLeft` / `ArrowDown` — value − 5% (small step)
//! * `ArrowRight` / `ArrowUp` — value + 5% (small step)
//! * `Home` — value 0.0 (minimum)
//! * `End` — value 1.0 (maximum)
//! * `PageDown` — value − 10% (large step)
//! * `PageUp` — value + 10% (large step)
//!
//! ARIA convention: the Disabled state ignores keyboard input, so
//! the override returns `false` (unhandled) while disabled — the
//! same shape a screen-reader-aware browser would observe via
//! `aria-disabled`. Value mutation flows through the same
//! `intervene("value", Float)` channel the RPC `scene/intervene`
//! route uses, so the AI client and the keyboard path see the
//! identical observable state transitions.

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole};
use pinion_core::widgets::slider::{SliderEvent, SliderExternal, SliderState};
use pinion_core::{
    scale_normalized_to_px, Color, Frame, Scene, WidgetCore, WidgetStateName,
};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;
use pinion_widget_paint::slider::{
    read_slider_state, slider_accent_for, slider_thumb_fill, slider_track_inactive,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloSliderRenderer, HelloSliderRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 220;
/// (R57.X.slider §5.50) [`ThemeProvider`] cache key — matches the
/// `"app"` convention shared across the example gallery.
const THEME_TAG: &str = "app";

/// (R57.X.slider §5.50) Material 3 slider filled-track + thumb base
// (R738 §5.38) The track/thumb M3 color contract is the lifted SSOT in
// `pinion_widget_paint::slider` (`slider_accent_for` / `slider_track_
// inactive` / `slider_thumb_fill`) — 4-consumer opinionated-paint lift.
// Track is 200×8 — the thin Material rail. The track's pill radius
// is 4 (= TRACK_H / 2) so its ends round flush with the thumb.
const TRACK_W: u32 = 200;
const TRACK_H: u32 = 8;
const TRACK_RADIUS: u32 = 4;
// Thumb is 16×16 — twice the track height per the Material spec
// thumb-to-track ratio. corner_radius = THUMB_SIZE / 2 inscribes a
// perfect circle.
const THUMB_SIZE: u32 = 16;
const THUMB_RADIUS: u32 = 8;
// Available drag range = track width minus thumb width. Value 0.0
// puts the thumb at the left edge (= 0 leading pixels); value 1.0
// puts it at the right edge (= 184 leading pixels). Outside this
// range the thumb would clip past the track edges.
const RANGE: u32 = TRACK_W - THUMB_SIZE;
const ROW_GAP: u32 = 16;

/// view-fn (§6.3): pure sync mapping `(SliderState, f32) -> Scene`.
///
/// Layout (top-to-bottom, centred):
///
/// 1. "Volume" label (18 px white) — descriptive caption.
/// 2. Slider track (`main_slider` tag, 200×16 row to vertically
///    centre the thumb against the 8-px rail): the
///    `[filled | thumb | unfilled]` triple. `filled` is value*RANGE
///    wide, `unfilled` is the remainder; the thumb sits between.
/// 3. Status line — `"<state> | <value:0.42>"` — text mirror so the
///    AI side can verify by reading the scene tree even when the
///    screenshot path is unavailable.
///
/// R48 + R51.34 §5.35: the `main_slider` tag on the track row is the
/// shell's `InputRouter` hit-test handle. The router resolves
/// pointer events to that node, hands them to the matching
/// `Scene::External("main_slider")` in the state scene, and (because
/// `SliderExternal::wants_pointer_capture` returns true) pins the
/// cursor across the drag while forwarding cursor X to
/// `External::pointer_move` for value mutation.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn view(state: SliderState, value: f32, _frame: &Frame) -> Scene {
    // (R57.X.slider §5.50) Active palette — auto-subscribes the
    // view-fn for theme swaps.
    let theme = use_theme(THEME_TAG).theme_animated();
    // Filled-portion canonical M3 active colour = `Accent` with the
    // state-layer overlays applied via [`slider_accent_for`].
    let filled_color: Color = slider_accent_for(&theme, state);
    // Unfilled portion = M3 `surfaceContainerHighest` (the inactive-
    // track tier). Disabled fades toward `Surface`.
    let unfilled_color: Color = slider_track_inactive(&theme, state);
    // Thumb = `OnAccent` (canonical M3 paired-contrast role for
    // controls on accent fills). Dragging tints slightly toward the
    // filled track so the moment of capture is visible; Disabled
    // washes toward `Surface`.
    let thumb_fill: Color = slider_thumb_fill(&theme, state);
    // R51.154 §5.3 — value * RANGE → leading-pixel width of the
    // filled portion. [`scale_normalized_to_px`] handles the clamp +
    // safe cast + drift saturation; the bespoke per-channel cast
    // (with three `#[allow(clippy::cast_*)]` lints sprinkled around
    // it) collapsed to one framework primitive call.
    let filled_w = scale_normalized_to_px(value, RANGE);
    let unfilled_w = RANGE.saturating_sub(filled_w);
    // Status-line clamp mirrors the framework primitive's clamp so
    // the printed value stays inside `[0, 1]` even on float drift.
    let value_clamped = value.clamp(0.0, 1.0);
    let filled = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(filled_color).with_corner_radius(TRACK_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(filled_w, TRACK_H))),
    );
    let unfilled = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(unfilled_color).with_corner_radius(TRACK_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(unfilled_w, TRACK_H))),
    );
    let thumb = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(thumb_fill).with_corner_radius(THUMB_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(THUMB_SIZE, THUMB_SIZE))),
    );
    // Track row: [filled | thumb | unfilled] vertically centred so
    // the 8-px rail aligns with the 16-px thumb's midline. Tag
    // applies to the *row* (the full 200×16 hit-test surface) so
    // clicking anywhere on the track jumps the thumb (Material
    // click-to-position UX, enabled by R51.34 → R51.35.a click-point
    // forward in InputRouter::pointer_down).
    let track_row = Scene::Container(
        ContainerNode::new(vec![filled, thumb, unfilled])
            .with_tag("main_slider")
            // R51.69 §5.40 — explicit accessible-name. The "Volume"
            // caption lives outside the track row for layout reasons
            // (label sits above), so the scene-walk derivation
            // cannot reach it; the override pins the AT-exposed
            // name without a duplicate literal in `access_node`.
            .with_aria_label("Volume")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(TRACK_W, THUMB_SIZE)),
            ),
    );
    let label = Scene::Text(TextNode::styled(
        "Volume",
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let status_str = format!(
        "{} | {value_clamped:.2}",
        state.as_name(),
    );
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label, track_row, status])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// `WidgetView` binding for the Slider widget. R645 §5.16 lifted the
/// mechanical [`WidgetCore`] / [`WidgetA11y`] / [`WidgetView`] trait
/// wiring into the [`#[widget]`](pinion_derive::widget) attribute,
/// + extended R642's `state_flags(...)` to tuple state types like
/// `(SliderState, f32)` (the macro auto-extracts `state.0` for the
/// flag matches). `event_name_derive` + `fmt_state_log` flags drop
/// the per-binding match arms; `read_state` stays inherent because
/// tuple read requires two introspect queries (`state` + `value`)
/// that the R643 single-field derive cannot express today.
///
/// [`WidgetCore`]: pinion_core::WidgetCore
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
/// [`WidgetView`]: pinion_shell::WidgetView
#[widget(
    tag = "main_slider",
    state = (SliderState, f32),
    event = SliderEvent,
    title = "pinion hello-slider (R645 §5.16 #[widget] tuple-state)",
    renderer = HelloSliderRenderer,
    initial_size = (WIN_W, WIN_H),
    external = SliderExternal::new,
    apply_key,
    keybinding,
    event_name_derive,
    fmt_state_log,
    a11y_manual,
)]
struct SliderView;

impl SliderView {
    /// R645 inherent forward for [`WidgetCore::view`]. The macro
    /// emits `<SliderView>::view(state, *frame)`; we receive the
    /// `Copy` tuple state by value, then dispatch into the free
    /// `view(state, value, frame)` fn that paints.
    fn view(state: (SliderState, f32), frame: Frame) -> Scene {
        view(state.0, state.1, &frame)
    }

    /// R645 inherent forward for [`WidgetCore::read_state`]. Tuple
    /// state reads through two introspect fields (`state` text +
    /// `value` float). The state half routes through the R643
    /// [`WidgetStateName::from_name_or_default`] derive (drops the
    /// per-binding `parse_slider_state` helper); the value half
    /// uses [`IntrospectValue::as_f32`] (R51.155) for the f64 → f32
    /// narrowing.
    fn read_state(scene: &Scene) -> (SliderState, f32) {
        // R737 §5.38 — shared introspect reader; the continuous
        // slider's missing-external fallback is `(Idle, 0.0)`.
        read_slider_state(scene, <Self as WidgetCore>::tag())
            .unwrap_or((SliderState::Idle, 0.0))
    }

    /// W3C/ARIA Slider keyboard accessibility — wires the six
    /// standard navigation keys (arrows, Home/End, PageUp/PageDown)
    /// to value mutation via the §5.15 introspect channel. Walks the
    /// authoritative state scene to the root [`Scene::External`]
    /// (the `SliderExternal` opt-in to [`ExternalIntrospect`]),
    /// reads the current value via `query("value")`, computes the
    /// next clamped value, and writes it back through
    /// `intervene("value", Float)` — the same side door the RPC
    /// `scene/intervene` route uses, so the AI client observes
    /// keyboard mutations identically to drag-driven mutations.
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str, _modifiers: pinion_core::Modifiers) -> bool {
        if focused != Some(Self::tag()) {
            return false;
        }
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        if let Some(IntrospectValue::Text(name)) = intro.query("state") {
            // R698.A §5.16 — compare against the SSOT variant, not a
            // hard-coded SCXML-id literal.
            if matches!(SliderState::from_name_or_default(&name), SliderState::Disabled) {
                return false;
            }
        }
        let Some(IntrospectValue::Float(current)) = intro.query("value") else {
            return false;
        };
        let new_value = match key {
            "ArrowLeft" | "ArrowDown" => (current - 0.05).clamp(0.0, 1.0),
            "ArrowRight" | "ArrowUp" => (current + 0.05).clamp(0.0, 1.0),
            "Home" => 0.0,
            "End" => 1.0,
            "PageDown" => (current - 0.10).clamp(0.0, 1.0),
            "PageUp" => (current + 0.10).clamp(0.0, 1.0),
            _ => return false,
        };
        intro
            .intervene("value", IntrospectValue::Float(new_value))
            .is_ok()
    }

    fn keybinding(key: &str) -> Option<SliderEvent> {
        match key {
            "d" => Some(SliderEvent::Disable),
            "e" => Some(SliderEvent::Enable),
            _ => None,
        }
    }

    /// R645 §5.16 — `fmt_state_log` flag forwards to this inherent
    /// method. Replaced the per-binding `slider_state_name` helper
    /// with the R643 [`WidgetStateName::as_name`] derive — single
    /// source of truth for the variant ↔ string mapping.
    /// [[widget-macro-by-value-bridge]] — by-value signature matches
    /// the `clippy::trivially_copy_pass_by_ref` preference for Copy
    /// state types; macro forwards `<View>::fmt_state_log(*state)`
    /// from the trait's `&Self::State` argument.
    fn fmt_state_log(state: (SliderState, f32)) -> String {
        format!("{} / {:.2}", state.0.as_name(), state.1)
    }
}

// R645 §5.16 — Slider's a11y node carries `AccessValue::Float
// {value, min, max}` extracted from `state.1` — beyond the
// state_flags-only derive in R642. The macro auto-emits the
// `role + state_flags` body for `focused / disabled / hovered /
// pressed` (R642 + R645 tuple expansion) but cannot reach the
// `.with_value(...)` chain without value-extraction syntax. Per
// [[abstraction-needs-second-consumer]] the value-bearing form
// waits on a 2nd consumer (TextField publishes AccessValue::Text,
// not Float, so not the same shape; future ProgressBar or
// secondary Slider would be the 2nd Float consumer). For now the
// macro's derived a11y impl is overridden by this manual one.
impl pinion_a11y::WidgetA11y for SliderView {
    fn access_node(state: &(SliderState, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (interaction, value) = (state.0, state.1);
        let access_state = AccessState {
            focused: focused == Some(<Self as WidgetCore>::tag()),
            disabled: matches!(interaction, SliderState::Disabled),
            hovered: matches!(interaction, SliderState::Hover),
            pressed: matches!(interaction, SliderState::Dragging),
            checked: None,
        };
        vec![AccessNode::new(<Self as WidgetCore>::tag(), AriaRole::Slider)
            .with_value(AccessValue::Float { value, min: 0.0, max: 1.0 })
            .with_state(access_state)]
    }
}

fn main() {
    pinion_shell::run::<SliderView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    /// Build a fresh slider scene at a given starting value. Mirrors
    /// the shape `WidgetView::create_external` wraps the
    /// `SliderExternal` into at shell startup so `apply_key` walks
    /// exactly the same scene topology the live binary observes.
    fn scene_at(start_value: f32) -> Scene {
        let mut ext = SliderExternal::new();
        ext.set_value(start_value);
        Scene::External(ExternalNode::new(Box::new(ext)).with_tag("main_slider"))
    }

    fn current_value(scene: &Scene) -> f32 {
        let Scene::External(node) = scene else {
            panic!("expected External root");
        };
        let intro = node.handle.introspect().expect("introspect opted in");
        intro
            .query("value")
            .and_then(|v| v.as_f32())
            .expect("value path returns Float")
    }

    #[test]
    fn arrow_right_increments_by_small_step() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowRight", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.55).abs() < 1e-5);
    }

    #[test]
    fn arrow_left_decrements_by_small_step() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowLeft", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.45).abs() < 1e-5);
    }

    #[test]
    fn arrow_up_aliases_arrow_right() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowUp", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.55).abs() < 1e-5);
    }

    #[test]
    fn arrow_down_aliases_arrow_left() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowDown", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.45).abs() < 1e-5);
    }

    #[test]
    fn home_jumps_to_minimum() {
        let mut scene = scene_at(0.7);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "Home", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn end_jumps_to_maximum() {
        let mut scene = scene_at(0.3);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "End", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn page_up_increments_by_large_step() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "PageUp", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.60).abs() < 1e-5);
    }

    #[test]
    fn page_down_decrements_by_large_step() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "PageDown", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.40).abs() < 1e-5);
    }

    #[test]
    fn arrow_left_clamps_at_minimum() {
        let mut scene = scene_at(0.0);
        // ARIA: handled (consumed key) even when the result is the
        // same value — analogous to a browser's Slider keyboard
        // dispatcher returning a stateful event.
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowLeft", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn arrow_right_clamps_at_maximum() {
        let mut scene = scene_at(1.0);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowRight", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn disabled_state_ignores_keyboard() {
        let mut scene = scene_at(0.5);
        // Drive into the Disabled cell via the typed event channel,
        // mirroring the `keybinding("d")` route at runtime.
        if let Scene::External(node) = &mut scene {
            let intro = node.handle.introspect_mut().expect("opted in");
            intro
                .invoke("send", IntrospectValue::Text("Disable".to_string()))
                .expect("Disable invoke succeeds");
        }
        // ARIA: a disabled slider does not consume keyboard input.
        assert!(!SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowRight", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn unknown_key_returns_false() {
        let mut scene = scene_at(0.5);
        assert!(!SliderView::apply_key(&mut scene, Some("main_slider"), "F1", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.5).abs() < 1e-5);
    }

    // ----- R51.56 §5.39 focused-only routing -----

    #[test]
    fn no_focus_returns_false_and_leaves_value() {
        // `FocusManager::focused()` returns `None` between Tab
        // boundaries; the slider must stay silent.
        let mut scene = scene_at(0.5);
        assert!(!SliderView::apply_key(&mut scene, None, "ArrowRight", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn other_widget_focused_returns_false_and_leaves_value() {
        // A sibling focusable widget (`save_btn`, etc.) holding focus
        // must not route slider arrow keys.
        let mut scene = scene_at(0.5);
        assert!(!SliderView::apply_key(
            &mut scene,
            Some("save_btn"),
            "ArrowRight"
        , pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.5).abs() < 1e-5);
    }
}

#[cfg(test)]
mod a11y_tests {
    use super::*;
    use pinion_a11y::WidgetA11y;

    fn enriched(state: (SliderState, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (s, v) = state;
        // (R57.X.slider §5.50) `view` calls [`use_theme`] so the
        // call must run inside an Owner scope (callback-root-owner-
        // wrap discipline).
        let scene = pinion_core::Owner::new().run(|| view(s, v, &Frame::new()));
        let mut nodes = SliderView::access_node(&state, focused);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        nodes
    }

    #[test]
    fn idle_emits_slider_role_with_volume_label() {
        let nodes = enriched((SliderState::Idle, 0.5), None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Slider);
        assert_eq!(nodes[0].name.as_deref(), Some("Volume"));
    }

    #[test]
    fn float_value_carries_normalized_range() {
        let nodes = SliderView::access_node(&(SliderState::Idle, 0.75), None);
        match &nodes[0].value {
            Some(AccessValue::Float { value, min, max }) => {
                assert!((value - 0.75).abs() < f32::EPSILON);
                assert!((min - 0.0).abs() < f32::EPSILON);
                assert!((max - 1.0).abs() < f32::EPSILON);
            }
            other => panic!("expected Float value, got {other:?}"),
        }
    }

    #[test]
    fn dragging_state_sets_pressed_flag() {
        let nodes = SliderView::access_node(&(SliderState::Dragging, 0.5), None);
        assert!(nodes[0].state.pressed);
    }

    #[test]
    fn disabled_state_sets_disabled_flag() {
        let nodes = SliderView::access_node(&(SliderState::Disabled, 0.0), None);
        assert!(nodes[0].state.disabled);
    }

    #[test]
    fn focused_tag_sets_focused_flag() {
        let nodes =
            SliderView::access_node(&(SliderState::Idle, 0.5), Some("main_slider"));
        assert!(nodes[0].state.focused);
    }

    #[test]
    fn checked_stays_none_for_slider() {
        let nodes = SliderView::access_node(&(SliderState::Idle, 0.5), None);
        assert_eq!(nodes[0].state.checked, None);
    }

    #[test]
    fn r55_g20_view_contains_composite_paint_root_tag() {
        // R55.G.20 §5.49 — paint scene must carry the composite
        // `WidgetCore::tag()` so AI-side `{path: "main_slider"}`
        // input routing and `rect_for_tag` AT bounds attach resolve.
        //
        // R55.G.22 §5.49 — pinned via the framework helper which
        // calls `V::view` under an `Owner::new()` scope and asserts
        // `Scene::contains_tag(V::tag())`.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<SliderView>(
            (SliderState::Idle, 0.5),
            &Frame::new(),
        );
    }
}
