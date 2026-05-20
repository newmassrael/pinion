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

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::slider::{SliderEvent, SliderExternal, SliderState};
use pinion_core::{scale_normalized_to_px, Color, Frame, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloSliderRenderer, HelloSliderRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 220;
const BG_FILL: Color = Color::rgb(0x20, 0x30, 0x40);
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
    // Filled-portion blue accent: the canonical Material "active"
    // colour ramp through the state axis, plus the muted-brown
    // Disabled cell shared with hello-toggle / hello-checkbox /
    // hello-radio so users can read disabled state at a glance.
    let filled_color: Color = match state {
        SliderState::Idle => Color::rgb(0x30, 0x70, 0xd0),
        SliderState::Hover => Color::rgb(0x40, 0x80, 0xe0),
        SliderState::Dragging => Color::rgb(0x20, 0x50, 0xa0),
        SliderState::Disabled => Color::rgb(0x70, 0x66, 0x58),
    };
    // Unfilled portion: chromatic-neutral track rail. Disabled cell
    // muted-brown again.
    let unfilled_color: Color = match state {
        SliderState::Disabled => Color::rgb(0x4a, 0x42, 0x38),
        _ => Color::rgb(0x40, 0x40, 0x40),
    };
    // Thumb fill: bright white in active states, light violet when
    // dragging (Material's pressed-thumb affordance — visual feedback
    // that capture is in flight), muted grey when disabled.
    let thumb_fill: Color = match state {
        SliderState::Idle => Color::rgb(0xf0, 0xf0, 0xf0),
        SliderState::Hover => Color::rgb(0xff, 0xff, 0xff),
        SliderState::Dragging => Color::rgb(0xe0, 0xe0, 0xff),
        SliderState::Disabled => Color::rgb(0xa0, 0xa0, 0xa0),
    };
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
            .with_fg(Color::rgb(0xe0, 0xe0, 0xe0)),
    ));
    let status_str = format!(
        "{} | {value_clamped:.2}",
        slider_state_name(state),
    );
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(Color::rgb(0x90, 0x90, 0x90)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label, track_row, status])
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

struct SliderView;

impl WidgetCore for SliderView {
    type State = (SliderState, f32);
    type Event = SliderEvent;

    fn create_external() -> Box<dyn External> {
        Box::new(SliderExternal::new())
    }

    fn tag() -> &'static str {
        "main_slider"
    }

    fn read_state(scene: &Scene) -> (SliderState, f32) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                    parse_slider_state(&name)
                } else {
                    SliderState::Idle
                };
                #[allow(clippy::cast_possible_truncation)]
                let value = if let Some(IntrospectValue::Float(v)) = intro.query("value") {
                    v as f32
                } else {
                    0.0
                };
                return (state, value);
            }
        }
        (SliderState::Idle, 0.0)
    }

    fn view(state: (SliderState, f32), frame: &Frame) -> Scene {
        view(state.0, state.1, frame)
    }

    fn event_name(event: SliderEvent) -> &'static str {
        match event {
            SliderEvent::PointerEnter => "PointerEnter",
            SliderEvent::PointerLeave => "PointerLeave",
            SliderEvent::PointerDown => "PointerDown",
            SliderEvent::PointerUp => "PointerUp",
            SliderEvent::Disable => "Disable",
            SliderEvent::Enable => "Enable",
            _ => "__internal__",
        }
    }

    fn title() -> &'static str {
        "pinion hello-slider (R51.35 §5.38 pinion-shell + R51.34 capture)"
    }

    fn keybinding(key: &str) -> Option<SliderEvent> {
        match key {
            "d" => Some(SliderEvent::Disable),
            "e" => Some(SliderEvent::Enable),
            _ => None,
        }
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
    fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str) -> bool {
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
            if name == "Disabled" {
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

    fn fmt_state_log(state: &(SliderState, f32)) -> String {
        format!("{} / {:.2}", slider_state_name(state.0), state.1)
    }
}

impl WidgetA11y for SliderView {
    /// R51.65 §5.40 — AccessKit semantic tree contribution. Emits a
    /// single `AriaRole::Slider` node carrying an `AccessValue::Float`
    /// with `min` / `max` matching the widget's normalized [0.0, 1.0]
    /// range (the same range the introspect schema's `"value"` key
    /// reports). `state.checked` stays `None` (Slider is not a
    /// check-like widget).
    ///
    /// R51.69 §5.40 — the accessible name comes from the track
    /// container's `aria_label` override (set in `view`) so the
    /// `"Volume"` literal lives in exactly one place.
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

impl WidgetView for SliderView {
    type Renderer = HelloSliderRenderer;

    fn initial_size() -> (u32, u32) {
        (WIN_W, WIN_H)
    }
}

fn parse_slider_state(name: &str) -> SliderState {
    match name {
        "Hover" => SliderState::Hover,
        "Dragging" => SliderState::Dragging,
        "Disabled" => SliderState::Disabled,
        _ => SliderState::Idle,
    }
}

fn slider_state_name(state: SliderState) -> &'static str {
    match state {
        SliderState::Idle => "Idle",
        SliderState::Hover => "Hover",
        SliderState::Dragging => "Dragging",
        SliderState::Disabled => "Disabled",
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
        let Some(IntrospectValue::Float(v)) = intro.query("value") else {
            panic!("value path returns Float");
        };
        #[allow(clippy::cast_possible_truncation)]
        {
            v as f32
        }
    }

    #[test]
    fn arrow_right_increments_by_small_step() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowRight"));
        assert!((current_value(&scene) - 0.55).abs() < 1e-5);
    }

    #[test]
    fn arrow_left_decrements_by_small_step() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowLeft"));
        assert!((current_value(&scene) - 0.45).abs() < 1e-5);
    }

    #[test]
    fn arrow_up_aliases_arrow_right() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowUp"));
        assert!((current_value(&scene) - 0.55).abs() < 1e-5);
    }

    #[test]
    fn arrow_down_aliases_arrow_left() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowDown"));
        assert!((current_value(&scene) - 0.45).abs() < 1e-5);
    }

    #[test]
    fn home_jumps_to_minimum() {
        let mut scene = scene_at(0.7);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "Home"));
        assert!((current_value(&scene) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn end_jumps_to_maximum() {
        let mut scene = scene_at(0.3);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "End"));
        assert!((current_value(&scene) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn page_up_increments_by_large_step() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "PageUp"));
        assert!((current_value(&scene) - 0.60).abs() < 1e-5);
    }

    #[test]
    fn page_down_decrements_by_large_step() {
        let mut scene = scene_at(0.5);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "PageDown"));
        assert!((current_value(&scene) - 0.40).abs() < 1e-5);
    }

    #[test]
    fn arrow_left_clamps_at_minimum() {
        let mut scene = scene_at(0.0);
        // ARIA: handled (consumed key) even when the result is the
        // same value — analogous to a browser's Slider keyboard
        // dispatcher returning a stateful event.
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowLeft"));
        assert!((current_value(&scene) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn arrow_right_clamps_at_maximum() {
        let mut scene = scene_at(1.0);
        assert!(SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowRight"));
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
        assert!(!SliderView::apply_key(&mut scene, Some("main_slider"), "ArrowRight"));
        assert!((current_value(&scene) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn unknown_key_returns_false() {
        let mut scene = scene_at(0.5);
        assert!(!SliderView::apply_key(&mut scene, Some("main_slider"), "F1"));
        assert!((current_value(&scene) - 0.5).abs() < 1e-5);
    }

    // ----- R51.56 §5.39 focused-only routing -----

    #[test]
    fn no_focus_returns_false_and_leaves_value() {
        // `FocusManager::focused()` returns `None` between Tab
        // boundaries; the slider must stay silent.
        let mut scene = scene_at(0.5);
        assert!(!SliderView::apply_key(&mut scene, None, "ArrowRight"));
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
        ));
        assert!((current_value(&scene) - 0.5).abs() < 1e-5);
    }
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn enriched(state: (SliderState, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (s, v) = state;
        let scene = view(s, v, &Frame::new());
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
}
