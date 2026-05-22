//! `hello-slider-vertical` — R51.46 §5.38 vertical Slider first-
//! client on the R51.39 future-proof axis substrate. Visual
//! evidence that [`SliderAxis::Vertical`] composes with the rest of
//! the R51.34 → R51.37 → R51.39 → R51.40 → R51.42 → R51.45 substrate
//! stack without any axis-specific `InputRouter` / `WidgetView` /
//! shell branch — the only divergence from `hello-slider` is the
//! axis builder call and the visual track orientation.
//!
//! Visual contract: Material / iOS volume-HUD style vertical slider
//! with an 8×200 pill track split top-to-bottom into
//! `[unfilled (above thumb) | thumb | filled (below thumb)]`. The
//! ARIA convention `aria-orientation=vertical` puts `value = 1.0`
//! at the top of the bar, so the filled portion grows upward as
//! the value increases. Setting `value = 0.0` leaves the thumb at
//! the bottom of the track (no fill); `value = 1.0` raises the
//! thumb to the top of the track (full bar fill below it).
//!
//! State / colour cross product mirrors `hello-slider`:
//!
//! * Idle: white thumb, blue accent filled portion
//! * Hover: brighter thumb (cursor over the rect but not pressed)
//! * Dragging: light-violet thumb (capture-in-flight affordance)
//! * Disabled: muted brown-grey across thumb + fill
//!
//! Keybindings reuse the ARIA-canonical small / large / extreme
//! step mapping (the same hooks `hello-slider`'s `apply_key`
//! exposes). ARIA's vertical-slider keyboard contract is identical
//! to its horizontal counterpart: `ArrowUp` / `ArrowRight` increment,
//! `ArrowDown` / `ArrowLeft` decrement, `Home` / `End` jump to the
//! extremes, `PageUp` / `PageDown` apply the large step.

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::slider::{SliderAxis, SliderEvent, SliderExternal, SliderState};
use pinion_core::{scale_normalized_to_px, Color, Frame, Scene, WidgetCore};
use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_shell::{vello_renderer_impl, WidgetView};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloSliderVerticalRenderer, HelloSliderVerticalRendererError);

const WIN_W: u32 = 220;
const WIN_H: u32 = 360;
const BG_FILL: Color = Color::rgb(0x20, 0x30, 0x40);
// Vertical track: 8 wide × 200 tall — same Material rail thinness
// as the horizontal example, rotated 90°.
const TRACK_W: u32 = 8;
const TRACK_H: u32 = 200;
const TRACK_RADIUS: u32 = 4;
// Thumb size = twice track width per Material spec.
const THUMB_SIZE: u32 = 16;
const THUMB_RADIUS: u32 = 8;
// Available drag range = track height minus thumb height.
const RANGE: u32 = TRACK_H - THUMB_SIZE;
const ROW_GAP: u32 = 16;

/// view-fn (§6.3): pure sync mapping `(SliderState, f32) -> Scene`.
///
/// Top-level layout (column, centred):
///
/// 1. "Volume" label.
/// 2. Vertical track (`main_slider` tag, 16×200 col so the 16-px
///    thumb is horizontally centred on the 8-px rail):
///    `[unfilled | thumb | filled]` top-to-bottom.
/// 3. Status line — `"<state> | <value:0.42>"`.
///
/// The `main_slider` tag is the shell's `InputRouter` hit-test
/// handle; R51.34 capture + R51.39 axis split + R51.42 sub-index
/// dispatch all compose on the same single-tag wiring the
/// horizontal example uses — the axis-specific behaviour lives
/// inside `SliderExternal::pointer_move`.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn view(state: SliderState, value: f32, _frame: &Frame) -> Scene {
    let filled_color: Color = match state {
        SliderState::Idle => Color::rgb(0x30, 0x70, 0xd0),
        SliderState::Hover => Color::rgb(0x40, 0x80, 0xe0),
        SliderState::Dragging => Color::rgb(0x20, 0x50, 0xa0),
        SliderState::Disabled => Color::rgb(0x70, 0x66, 0x58),
    };
    let unfilled_color: Color = match state {
        SliderState::Disabled => Color::rgb(0x4a, 0x42, 0x38),
        _ => Color::rgb(0x40, 0x40, 0x40),
    };
    let thumb_fill: Color = match state {
        SliderState::Idle => Color::rgb(0xf0, 0xf0, 0xf0),
        SliderState::Hover => Color::rgb(0xff, 0xff, 0xff),
        SliderState::Dragging => Color::rgb(0xe0, 0xe0, 0xff),
        SliderState::Disabled => Color::rgb(0xa0, 0xa0, 0xa0),
    };
    // R51.154 §5.3 — value = 1.0 → thumb at top → all space below
    // it is filled; value = 0.0 → thumb at bottom → no fill.
    // [`scale_normalized_to_px`] handles the clamp + safe cast.
    let filled_h = scale_normalized_to_px(value, RANGE);
    let unfilled_h = RANGE.saturating_sub(filled_h);
    // Status-line clamp mirrors the framework primitive's clamp so
    // the printed value stays inside `[0, 1]` even on float drift.
    let value_clamped = value.clamp(0.0, 1.0);
    let unfilled = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(unfilled_color).with_corner_radius(TRACK_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(TRACK_W, unfilled_h))),
    );
    let thumb = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(thumb_fill).with_corner_radius(THUMB_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(THUMB_SIZE, THUMB_SIZE))),
    );
    let filled = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(filled_color).with_corner_radius(TRACK_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(TRACK_W, filled_h))),
    );
    // Track column: [unfilled (top) | thumb | filled (bottom)] so
    // value=1.0 means thumb at top + bar fully filled below. The
    // thumb is wider (16 px) than the rail (8 px); horizontal
    // centering via `AlignItems::Center` matches the Material
    // thumb-on-rail spec. The 16-wide column sets the hit-test
    // rect so the tag covers the full thumb area, not just the
    // thin rail.
    let track_col = Scene::Container(
        ContainerNode::new(vec![unfilled, thumb, filled])
            .with_tag("main_slider")
            // R51.69 §5.40 — explicit accessible-name. The "Volume"
            // caption sits above the column as a sibling; the scene
            // walk inside the tagged container reaches only Box
            // children. Override pins the name without duplicating
            // the literal in `access_node`.
            .with_aria_label("Volume")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(THUMB_SIZE, TRACK_H)),
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
        ContainerNode::new(vec![label, track_col, status])
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

struct SliderVerticalView;

impl WidgetCore for SliderVerticalView {
    type State = (SliderState, f32);
    type Event = SliderEvent;

    fn create_external() -> Box<dyn External> {
        // R51.39 §5.38 — the only line that diverges from
        // `hello-slider`. The rest of the binding (read_state /
        // view / apply_key) does not branch on axis because the
        // axis-specific behaviour is contained in
        // `SliderExternal::pointer_move`'s `1.0 - y_rel`
        // inversion.
        Box::new(SliderExternal::with_axis(SliderAxis::Vertical))
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
                // R51.155 §5.15 — `IntrospectValue::as_f32` (see
                // hello-slider for rationale).
                let value = intro
                    .query("value")
                    .and_then(|v| v.as_f32())
                    .unwrap_or(0.0);
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
        "pinion hello-slider-vertical (R51.46 §5.38 SliderAxis::Vertical)"
    }

    fn keybinding(key: &str) -> Option<SliderEvent> {
        match key {
            "d" => Some(SliderEvent::Disable),
            "e" => Some(SliderEvent::Enable),
            _ => None,
        }
    }

    /// ARIA Slider keyboard accessibility. The vertical orientation
    /// reuses the same key mapping as the horizontal example: ARIA
    /// keeps `ArrowUp` / `ArrowRight` as the increment direction
    /// regardless of orientation so screen readers do not have to
    /// branch on `aria-orientation` to decide the activation
    /// direction. Disabled state ignores keyboard input per the
    /// same ARIA contract.
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

impl WidgetA11y for SliderVerticalView {
    /// R51.65 §5.40 — AccessKit semantic tree contribution for the
    /// vertical slider variant. Same role / value semantics as the
    /// horizontal slider; orientation is conveyed visually + by the
    /// distinct widget tag (`main_slider_v`) — a future round adds an
    /// `accesskit::Orientation` field to [`AccessNode`] once a
    /// non-slider orientation consumer exists (carry).
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

impl WidgetView for SliderVerticalView {
    type Renderer = HelloSliderVerticalRenderer;

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
    pinion_shell::run::<SliderVerticalView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    /// Build a vertical-axis slider scene at a starting value. The
    /// axis builder is the only divergence from `hello-slider`'s
    /// fixture; verifying the axis routing wires through the same
    /// `apply_key` / introspect path keeps the substrate's
    /// orientation invariant honest.
    fn scene_at(start_value: f32) -> Scene {
        let mut ext = SliderExternal::with_axis(SliderAxis::Vertical);
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
    fn vertical_arrow_up_increments() {
        let mut scene = scene_at(0.5);
        assert!(SliderVerticalView::apply_key(&mut scene, Some("main_slider"), "ArrowUp", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.55).abs() < 1e-5);
    }

    #[test]
    fn vertical_arrow_down_decrements() {
        let mut scene = scene_at(0.5);
        assert!(SliderVerticalView::apply_key(&mut scene, Some("main_slider"), "ArrowDown", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.45).abs() < 1e-5);
    }

    #[test]
    fn vertical_home_jumps_to_minimum() {
        let mut scene = scene_at(0.7);
        assert!(SliderVerticalView::apply_key(&mut scene, Some("main_slider"), "Home", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn vertical_end_jumps_to_maximum() {
        let mut scene = scene_at(0.3);
        assert!(SliderVerticalView::apply_key(&mut scene, Some("main_slider"), "End", pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn vertical_orientation_reports_through_introspect() {
        let scene = scene_at(0.5);
        let Scene::External(node) = &scene else {
            panic!("expected External root");
        };
        let intro = node.handle.introspect().expect("introspect opted in");
        // R51.39 §5.38 — the `orientation` slot reports the axis
        // name aligned with ARIA `aria-orientation`. The vertical-
        // axis fixture must report `"vertical"`; the value is
        // construction-time fixed and read-only.
        assert_eq!(
            intro.query("orientation"),
            Some(IntrospectValue::Text("vertical".to_string())),
        );
    }

    // ----- R51.56 §5.39 focused-only routing -----

    #[test]
    fn no_focus_returns_false_and_leaves_value() {
        let mut scene = scene_at(0.5);
        assert!(!SliderVerticalView::apply_key(
            &mut scene,
            None,
            "ArrowUp"
        , pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn other_widget_focused_returns_false_and_leaves_value() {
        let mut scene = scene_at(0.5);
        assert!(!SliderVerticalView::apply_key(
            &mut scene,
            Some("save_btn"),
            "ArrowUp"
        , pinion_core::Modifiers::empty()));
        assert!((current_value(&scene) - 0.5).abs() < 1e-5);
    }
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    fn enriched(state: (SliderState, f32), focused: Option<&str>) -> Vec<AccessNode> {
        let (s, v) = state;
        let scene = view(s, v, &Frame::new());
        let mut nodes = SliderVerticalView::access_node(&state, focused);
        pinion_a11y::enrich_names_from_scene(&mut nodes, &scene);
        nodes
    }

    #[test]
    fn vertical_idle_emits_slider_role() {
        let nodes = enriched((SliderState::Idle, 0.5), None);
        assert_eq!(nodes[0].role, AriaRole::Slider);
        assert_eq!(nodes[0].name.as_deref(), Some("Volume"));
    }

    #[test]
    fn vertical_float_value_carries_range() {
        let nodes = SliderVerticalView::access_node(&(SliderState::Idle, 0.25), None);
        match &nodes[0].value {
            Some(AccessValue::Float { value, min, max }) => {
                assert!((value - 0.25).abs() < f32::EPSILON);
                assert!((min - 0.0).abs() < f32::EPSILON);
                assert!((max - 1.0).abs() < f32::EPSILON);
            }
            other => panic!("expected Float, got {other:?}"),
        }
    }

    #[test]
    fn vertical_focused_tag_sets_focused_flag() {
        let nodes = SliderVerticalView::access_node(
            &(SliderState::Idle, 0.5),
            Some("main_slider"),
        );
        assert!(nodes[0].state.focused);
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
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<SliderVerticalView>(
            (SliderState::Idle, 0.5),
            &Frame::new(),
        );
    }
}
