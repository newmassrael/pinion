//! `hello-slider-discrete` — R737 §5.38 §5.40 §5.50 **discrete (stepped)
//! slider** (the WAI-ARIA discrete-slider / Material "slider with tick
//! marks" variant; the form factor of a quality preset, a difficulty
//! selector, a zoom-level stepper).
//!
//! ## 1st consumer of the R737 step substrate
//!
//! R737 added an optional discrete-snap increment to the core
//! [`Slider`](pinion_core::widgets::slider) — [`SliderExternal::with_step`].
//! The snap lives inside [`Slider::set_value`], so **one funnel** snaps
//! every value path identically: a pointer drag (`pointer_move`), a
//! keyboard arrow, an `intervene`, and an RPC write all land on the
//! nearest tick. There is no per-binding snap code — the binding just
//! constructs the external with a step and paints tick marks at the
//! stops.
//!
//! This is that substrate's 1st consumer (the R696 "small additive
//! substrate lands with its first consumer" pattern). The continuous
//! `hello-slider` is untouched: `step` defaults to `None` and
//! `set_value` snaps to a no-op (clamp only).
//!
//! ## Composition (mirror of hello-slider, plus ticks)
//!
//! `STEP = 0.2` gives the six stops `0.0 / 0.2 / 0.4 / 0.6 / 0.8 / 1.0`.
//! The track row carries the `disc_slider` hit-test tag (the shell's
//! `InputRouter` routes pointer + capture-drag to the matching
//! `Scene::External`). The rail, filled portion, tick dots, and thumb
//! are absolutely positioned inside the track so the ticks align with
//! the discrete thumb-centre positions.
//!
//! ## Keyboard model (WAI-ARIA discrete slider)
//!
//! `ArrowRight` / `ArrowUp` advance one tick (`+STEP`), `ArrowLeft` /
//! `ArrowDown` retreat one tick, `Home` / `End` jump to the min / max
//! stop. Every key writes through the same `intervene("value", Float)`
//! channel the drag + RPC use, so the snap applies uniformly (a `+STEP`
//! from an on-grid value lands exactly on the next tick).
//!
//! ## a11y (R737 §5.40) — no new axis
//!
//! One [`AriaRole::Slider`] node carrying the snapped value as
//! [`AccessValue::Float`] (`aria-valuenow` / `aria-valuemin` /
//! `aria-valuemax`) — the same lowering the continuous slider uses, **no
//! new a11y axis**. (Labeled-step `aria-valuetext` — e.g. "Medium"
//! instead of "0.4" — is a separate additive axis deferred until a
//! consumer needs named stops; numeric `valuenow` is correct for a
//! numeric discrete slider.)

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{use_theme, ColorRole, Theme};
use pinion_core::widgets::slider::{SliderExternal, SliderState};
use pinion_core::{scale_normalized_to_px, Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{vello_renderer_impl, WidgetView};
use pinion_widget_paint::slider::{
    read_slider_state, slider_accent_for, slider_apply_key, slider_thumb_fill,
    slider_track_inactive,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloSliderDiscreteRenderer, HelloSliderDiscreteRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 220;
const THEME_TAG: &str = "app";

/// The focusable discrete-slider track tag (hit-test + capture-drag root).
const TAG: &str = "disc_slider";

/// Normalised discrete increment: six stops at 0.0 / 0.2 / … / 1.0.
const STEP: f32 = 0.2;
/// Number of tick stops (`1.0 / STEP + 1`). Kept as a const + asserted
/// against `STEP` in the tests so the two never drift.
const STOPS: usize = 6;
/// Boot value sits on a tick (0.4 = the "Medium" third stop).
const START: f32 = 0.4;

// Track geometry (mirror of hello-slider's Material rail).
const TRACK_W: u32 = 240;
const TRACK_H: u32 = 8;
const TRACK_RADIUS: u32 = 4;
const THUMB_SIZE: u32 = 18;
const THUMB_RADIUS: u32 = 9;
/// Drag range = track width minus thumb width; thumb centres span
/// `[THUMB_SIZE/2, THUMB_SIZE/2 + RANGE]`.
const RANGE: u32 = TRACK_W - THUMB_SIZE;
const TICK_SIZE: u32 = 4;
const ROW_GAP: u32 = 18;

/// Cached posture: the SCXML interaction state + the snapped value. Both
/// read from the live `SliderExternal` so the cache never diverges from
/// the AI-observable introspect surface ([[update-by-value-snapshot]]).
#[derive(Copy, Clone, PartialEq, Debug)]
struct DiscState {
    interaction: SliderState,
    value: f32,
}

impl DiscState {
    /// The boot posture (Idle at the START tick). Used by the view /
    /// a11y unit tests as a stable fixture; the live binding reads the
    /// real posture from the external via [`read_disc`].
    #[cfg(test)]
    fn boot() -> Self {
        Self { interaction: SliderState::Idle, value: START }
    }
}

/// Absolutely-positioned tick dot at stop `i` (its centre aligned with
/// the thumb centre at that stop). Painted on the rail, under the thumb.
fn tick(i: usize, theme: &Theme, on_fill: bool) -> Scene {
    // Stop fraction → thumb-centre x = THUMB_SIZE/2 + frac * RANGE.
    #[allow(clippy::cast_precision_loss)]
    let frac = i as f32 * STEP;
    let centre_x = THUMB_SIZE / 2 + scale_normalized_to_px(frac, RANGE);
    let x = centre_x.saturating_sub(TICK_SIZE / 2);
    let y = (THUMB_SIZE - TICK_SIZE) / 2;
    // Ticks under the filled portion read as on-accent; the rest are a
    // muted on-surface dot (Material tick-mark contrast).
    let fill = if on_fill {
        theme.resolve(ColorRole::OnAccent)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(fill).with_corner_radius(TICK_SIZE / 2),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(x, y)
                .with_size(Size::px(TICK_SIZE, TICK_SIZE)),
        ),
    )
}

/// view-fn (§6.3): pure sync `DiscState -> Scene`. A label, the tagged
/// track (rail + filled + ticks + thumb, absolutely positioned), and a
/// status line.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &DiscState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let value = state.value.clamp(0.0, 1.0);
    let interaction = state.interaction;

    let filled_w = scale_normalized_to_px(value, RANGE);
    let rail_y = (THUMB_SIZE - TRACK_H) / 2;

    // Rail (inactive track) spanning the thumb-centre travel.
    let rail = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(slider_track_inactive(&theme, interaction))
                .with_corner_radius(TRACK_RADIUS),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(THUMB_SIZE / 2, rail_y)
                .with_size(Size::px(RANGE, TRACK_H)),
        ),
    );
    // Filled portion from the first tick to the thumb centre.
    let filled = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(slider_accent_for(&theme, interaction)).with_corner_radius(TRACK_RADIUS),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(THUMB_SIZE / 2, rail_y)
                .with_size(Size::px(filled_w, TRACK_H)),
        ),
    );

    // Tick dots: those at or below the current value read on-accent.
    let mut children = vec![rail, filled];
    for i in 0..STOPS {
        #[allow(clippy::cast_precision_loss)]
        let frac = i as f32 * STEP;
        children.push(tick(i, &theme, frac <= value + f32::EPSILON));
    }

    // Thumb at the snapped value position.
    let thumb_fill = slider_thumb_fill(&theme, interaction);
    let thumb = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(thumb_fill).with_corner_radius(THUMB_RADIUS),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(filled_w, 0)
                .with_size(Size::px(THUMB_SIZE, THUMB_SIZE)),
        ),
    );
    children.push(thumb);

    let track = Scene::Container(
        ContainerNode::new(children)
            .with_tag(TAG)
            .with_aria_label("Quality")
            .with_layout(LayoutStyle::new().with_size(Size::px(TRACK_W, THUMB_SIZE))),
    );

    let label = Scene::Text(TextNode::styled(
        "Quality",
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    // Status echoes the snapped value + its stop index (AI text mirror).
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let stop_index = (value / STEP).round() as usize;
    let status = Scene::Text(
        TextNode::styled(
            format!("{} | {value:.1} (stop {stop_index}/{})", interaction.as_name(), STOPS - 1),
            Rect::default(),
            TextStyle::new()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag("disc_status"),
    );

    Scene::Container(
        ContainerNode::new(vec![label, track, status])
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

/// Read `(state, value)` from the live `SliderExternal` via the lifted
/// R737 [`read_slider_state`] introspect reader (shared with
/// `hello-slider` / `hello-slider-vertical` / `settings-panel`). The
/// missing-external fallback is this binding's own boot tick.
fn read_disc(scene: &Scene) -> DiscState {
    let (interaction, value) =
        read_slider_state(scene, TAG).unwrap_or((SliderState::Idle, START));
    DiscState { interaction, value }
}

struct DiscView;

impl WidgetCore for DiscView {
    type State = DiscState;
    // Value mutation flows through drag (pointer_move) + apply_key; no
    // keybinding-channel typed events.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        // Discrete slider stepped at STEP, seeded to the START tick so
        // the boot frame shows a real on-grid value (the substrate
        // snaps START onto the grid; 0.4 is already a tick).
        let mut ext = SliderExternal::with_step(STEP);
        ext.set_value(START);
        Box::new(ext)
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> DiscState {
        read_disc(scene)
    }

    fn view(state: DiscState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-slider-discrete (R737 §5.38 discrete slider)"
    }

    /// WAI-ARIA discrete-slider keyboard: arrows move one tick, Home /
    /// End jump to the extreme stops. The focus-guard / disabled-check /
    /// read / `intervene("value", Float)` scaffold is the lifted R739.1
    /// [`slider_apply_key`] SSOT (shared with the continuous / vertical /
    /// labeled sliders); only the tick key-map is per-widget. The write
    /// funnels through `intervene` so the substrate snap applies (a
    /// `+STEP` from an on-grid value lands on the next tick).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        slider_apply_key(scene, focused, TAG, |current| match key {
            "ArrowRight" | "ArrowUp" => Some((current + STEP).clamp(0.0, 1.0)),
            "ArrowLeft" | "ArrowDown" => Some((current - STEP).clamp(0.0, 1.0)),
            "Home" => Some(0.0),
            "End" => Some(1.0),
            _ => None,
        })
    }

    fn fmt_state_log(state: &DiscState) -> String {
        format!("{} / {:.1}", state.interaction.as_name(), state.value)
    }
}

impl WidgetA11y for DiscView {
    /// R737 §5.40 — one `slider` node carrying the snapped value as
    /// [`AccessValue::Float`] (`aria-valuenow` / `aria-valuemin` /
    /// `aria-valuemax`). No new a11y axis — the same lowering the
    /// continuous slider uses.
    fn access_node(state: &DiscState, focused: Option<&str>) -> Vec<AccessNode> {
        vec![AccessNode::new(TAG, AriaRole::Slider)
            .with_value(AccessValue::Float { value: state.value, min: 0.0, max: 1.0 })
            .with_state(AccessState {
                focused: focused == Some(TAG),
                disabled: matches!(state.interaction, SliderState::Disabled),
                hovered: matches!(state.interaction, SliderState::Hover),
                pressed: matches!(state.interaction, SliderState::Dragging),
                checked: None,
            })]
    }
}

impl WidgetView for DiscView {
    type Renderer = HelloSliderDiscreteRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<DiscView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::IntrospectValue;
    use pinion_core::scene::ExternalNode;

    fn scene_fixture() -> Scene {
        Scene::External(ExternalNode::new(DiscView::create_external()).with_tag(TAG))
    }

    fn value_of(scene: &Scene) -> f32 {
        let Scene::External(node) = scene else {
            panic!("expected External root");
        };
        node.handle
            .introspect()
            .expect("introspect opted in")
            .query("value")
            .and_then(|v| v.as_f32())
            .expect("value path returns Float")
    }

    #[test]
    fn step_and_stops_are_consistent() {
        // STOPS = 1/STEP + 1; guards the two consts against drift.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let derived = (1.0_f32 / STEP).round() as usize + 1;
        assert_eq!(derived, STOPS, "STOPS must equal 1/STEP + 1");
    }

    #[test]
    fn boot_value_sits_on_a_tick() {
        let scene = scene_fixture();
        // START seeded through with_step → snapped; 0.4 is already a tick.
        assert!((value_of(&scene) - START).abs() < 1e-5, "boot on the START tick");
    }

    #[test]
    fn external_reports_the_step() {
        let scene = scene_fixture();
        let Scene::External(node) = &scene else { panic!() };
        let intro = node.handle.introspect().unwrap();
        match intro.query("step") {
            Some(IntrospectValue::Float(v)) => assert!((v - f64::from(STEP)).abs() < 1e-6),
            other => panic!("expected Float step, got {other:?}"),
        }
    }

    #[test]
    fn arrow_right_advances_one_tick() {
        let mut scene = scene_fixture(); // boot 0.4
        assert!(DiscView::apply_key(&mut scene, Some(TAG), "ArrowRight", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 0.6).abs() < 1e-5, "0.4 -> 0.6 (one tick)");
        assert!(DiscView::apply_key(&mut scene, Some(TAG), "ArrowLeft", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 0.4).abs() < 1e-5, "0.6 -> 0.4");
    }

    #[test]
    fn arrows_clamp_at_extreme_stops() {
        let mut scene = scene_fixture();
        assert!(DiscView::apply_key(&mut scene, Some(TAG), "End", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 1.0).abs() < 1e-5, "End -> 1.0");
        assert!(DiscView::apply_key(&mut scene, Some(TAG), "ArrowRight", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 1.0).abs() < 1e-5, "ArrowRight at max clamps");
        assert!(DiscView::apply_key(&mut scene, Some(TAG), "Home", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 0.0).abs() < 1e-5, "Home -> 0.0");
        assert!(DiscView::apply_key(&mut scene, Some(TAG), "ArrowLeft", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 0.0).abs() < 1e-5, "ArrowLeft at min clamps");
    }

    #[test]
    fn off_grid_intervene_snaps_to_nearest_tick() {
        let mut scene = scene_fixture();
        let Scene::External(node) = &mut scene else { panic!() };
        let intro = node.handle.introspect_mut().unwrap();
        // An AI client writing an off-grid value is snapped by the substrate.
        intro.intervene("value", IntrospectValue::Float(0.71)).unwrap();
        assert!((value_of(&scene) - 0.8).abs() < 1e-5, "0.71 snaps to 0.8");
    }

    #[test]
    fn keys_ignored_when_not_focused() {
        let mut scene = scene_fixture();
        assert!(!DiscView::apply_key(&mut scene, None, "ArrowRight", pinion_core::Modifiers::empty()));
        assert!(!DiscView::apply_key(&mut scene, Some("other"), "ArrowRight", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - START).abs() < 1e-5, "value unchanged");
    }

    #[test]
    fn view_carries_track_and_status_tags() {
        let scene = pinion_core::Owner::new().run(|| view(&DiscState::boot(), &Frame::new()));
        assert!(scene.contains_tag(TAG), "track tag painted");
        assert!(scene.contains_tag("disc_status"), "status tag painted");
    }

    #[test]
    fn view_contains_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<DiscView>(
            DiscState::boot(),
            &Frame::default(),
        );
    }

    #[test]
    fn emits_slider_node_with_float_value() {
        let nodes = DiscView::access_node(&DiscState::boot(), Some(TAG));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Slider);
        assert!(nodes[0].state.focused);
        match nodes[0].value {
            Some(AccessValue::Float { value, min, max }) => {
                assert!((value - START).abs() < f32::EPSILON);
                assert!((min - 0.0).abs() < f32::EPSILON);
                assert!((max - 1.0).abs() < f32::EPSILON);
            }
            ref other => panic!("expected Float value, got {other:?}"),
        }
    }
}
