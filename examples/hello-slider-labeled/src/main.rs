//! `hello-slider-labeled` — R739 §5.38 §5.40 §5.50 **labeled-step slider**
//! (the WAI-ARIA `aria-valuetext` named-stop slider; the form factor of a
//! brightness preset, a typography size selector, a density control where
//! each tick has a *name* — "Off / Low / Medium / High / Max" — rather than
//! a bare number).
//!
//! ## 2nd consumer of the R737 step substrate
//!
//! Like `hello-slider-discrete`, the value snaps to the nearest of five
//! stops through the single [`Slider::set_value`] funnel
//! ([`SliderExternal::with_step`]) — drag, keyboard, `intervene`, and RPC
//! all land on a tick with no per-binding snap code. `STEP = 0.25` gives
//! the stops `0.0 / 0.25 / 0.5 / 0.75 / 1.0`.
//!
//! ## 1st consumer of the R739 `aria-valuetext` axis
//!
//! The new bit: a discrete slider whose stops are *named*. When the
//! numeric `aria-valuenow` is not meaningful (WAI-ARIA 1.2 §6.6.2), the
//! author supplies `aria-valuetext` — the human-readable label AT
//! announces instead of the number ("Medium", not "0.5"). R739 added
//! [`AccessNode::with_value_text`], which lowers to AccessKit's string
//! value *alongside* the numeric range from [`AccessValue::Float`], so the
//! node still carries `valuenow/min/max` for context.
//!
//! [`LABEL_FOR`]-style mapping is a single source of truth: [`label_for`]
//! is called once for the prominent on-screen readout *and* for the a11y
//! `value_text`, so the visible label and the announced label can never
//! diverge. The per-stop label row under the track visualises every named
//! stop (the active one drawn in the accent colour), which is what makes
//! the labeled variant visually distinct from the numeric discrete one —
//! no tick-dot paint is duplicated from `hello-slider-discrete`.
//!
//! ## Keyboard model (WAI-ARIA discrete slider)
//!
//! `ArrowRight` / `ArrowUp` advance one stop (`+STEP`), `ArrowLeft` /
//! `ArrowDown` retreat, `Home` / `End` jump to the `Off` / `Max` stop.
//! Every key writes through the same `intervene("value", Float)` channel
//! the drag + RPC use, so the substrate snap applies uniformly.

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
vello_renderer_impl!(HelloSliderLabeledRenderer, HelloSliderLabeledRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 240;
const THEME_TAG: &str = "app";

/// The focusable labeled-slider track tag (hit-test + capture-drag root).
const TAG: &str = "bright_slider";
/// The prominent current-stop readout (the AI text mirror of
/// `aria-valuetext`).
const READOUT: &str = "bright_readout";
/// The interaction-state + value status line (AI text mirror).
const STATUS: &str = "bright_status";

/// Normalised discrete increment: five stops at 0.0 / 0.25 / … / 1.0.
const STEP: f32 = 0.25;
/// Number of named stops (`1.0 / STEP + 1`). Kept as a const + asserted
/// against `STEP` and [`LABELS`] in the tests so the three never drift.
const STOPS: usize = 5;
/// The named stop labels — the `aria-valuetext` vocabulary. Index `i`
/// names the stop at value `i * STEP`. Binding-local app data (the
/// `const LABELS: [&str; N]` idiom shared with combobox / tabs /
/// segmented / toolbar), not framework substrate.
const LABELS: [&str; STOPS] = ["Off", "Low", "Medium", "High", "Max"];
/// Boot value sits on the middle "Medium" stop.
const START: f32 = 0.5;

// Track geometry (mirror of hello-slider's Material rail).
const TRACK_W: u32 = 240;
const TRACK_H: u32 = 8;
const TRACK_RADIUS: u32 = 4;
const THUMB_SIZE: u32 = 18;
const THUMB_RADIUS: u32 = 9;
/// Drag range = track width minus thumb width; thumb centres span
/// `[THUMB_SIZE/2, THUMB_SIZE/2 + RANGE]`.
const RANGE: u32 = TRACK_W - THUMB_SIZE;
const ROW_GAP: u32 = 16;
/// Stop-label box geometry. Each label is laid out in a fixed-width box so
/// it can be centred on (or flushed to) its stop; `LABEL_W` is wide enough
/// for the widest label ("Medium") at the 11px label size.
const LABEL_W: u32 = 56;
const LABEL_H: u32 = 16;

/// The stop index nearest `value` (`round(value / STEP)`), clamped into
/// `0..STOPS`. The single value→stop primitive both the readout and the
/// a11y `value_text` funnel through.
fn active_index(value: f32) -> usize {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = (value.clamp(0.0, 1.0) / STEP).round() as usize;
    i.min(STOPS - 1)
}

/// The named label for `value`'s nearest stop. **Single source of truth**
/// for the visible readout *and* the `aria-valuetext` — calling it once on
/// each path means the announced label can never drift from the painted
/// one.
fn label_for(value: f32) -> &'static str {
    LABELS[active_index(value)]
}

/// Cached posture: the SCXML interaction state + the snapped value. Both
/// read from the live `SliderExternal` so the cache never diverges from
/// the AI-observable introspect surface ([[update-by-value-snapshot]]).
#[derive(Copy, Clone, PartialEq, Debug)]
struct LabeledState {
    interaction: SliderState,
    value: f32,
}

impl LabeledState {
    /// The boot posture (Idle at the START stop). Used by the view / a11y
    /// unit tests as a stable fixture; the live binding reads the real
    /// posture from the external via [`read_labeled`].
    #[cfg(test)]
    fn boot() -> Self {
        Self { interaction: SliderState::Idle, value: START }
    }
}

/// The x of stop `i`'s centre within the track (the same thumb-centre the
/// filled portion reaches at that stop's value). R739.2 — each interior
/// label is centred on exactly this x.
fn stop_centre_x(i: usize) -> u32 {
    #[allow(clippy::cast_precision_loss)]
    let frac = i as f32 * STEP;
    THUMB_SIZE / 2 + scale_normalized_to_px(frac, RANGE)
}

/// The named-stop label row: every stop named, the active one in accent so
/// the discrete stops read as *labels* (no tick-dot paint duplicated from
/// `hello-slider-discrete`).
///
/// R739.2 — pixel-exact alignment (replacing the earlier `SpaceBetween`
/// approximation): every *interior* label is absolutely centred on its
/// stop's thumb-centre [`stop_centre_x`], so "Medium" sits exactly under
/// the thumb at value 0.5. The two *end* stops sit only `THUMB_SIZE / 2`
/// from the track edge — too close to centre a full label without clipping
/// — so the first label is flush-left and the last flush-right (the
/// canonical Material value-label layout: end labels align to the ends,
/// not past them).
fn stop_label_row(theme: &Theme, interaction: SliderState, active: usize) -> Scene {
    let accent = slider_accent_for(theme, interaction);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let children = LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let fg = if i == active { accent } else { muted };
            let (box_left, justify) = if i == 0 {
                (0, JustifyContent::Start)
            } else if i == STOPS - 1 {
                (TRACK_W - LABEL_W, JustifyContent::End)
            } else {
                (stop_centre_x(i).saturating_sub(LABEL_W / 2), JustifyContent::Center)
            };
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::styled(
                    *label,
                    Rect::default(),
                    TextStyle::new().with_size_px(11).with_fg(fg),
                ))])
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(justify)
                        .with_absolute_position(box_left, 0)
                        .with_size(Size::px(LABEL_W, LABEL_H)),
                ),
            )
        })
        .collect();
    Scene::Container(
        ContainerNode::new(children)
            .with_layout(LayoutStyle::new().with_size(Size::px(TRACK_W, LABEL_H))),
    )
}

/// view-fn (§6.3): pure sync `LabeledState -> Scene`. A title, the tagged
/// track (rail + filled + thumb, absolutely positioned), the named-stop
/// label row (active stop in accent), a prominent current-stop readout,
/// and a status line.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &LabeledState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let value = state.value.clamp(0.0, 1.0);
    let interaction = state.interaction;
    let active = active_index(value);

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
    // Filled portion from the first stop to the thumb centre.
    let filled = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(slider_accent_for(&theme, interaction))
                .with_corner_radius(TRACK_RADIUS),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(THUMB_SIZE / 2, rail_y)
                .with_size(Size::px(filled_w, TRACK_H)),
        ),
    );
    // Thumb at the snapped value position.
    let thumb = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(slider_thumb_fill(&theme, interaction))
                .with_corner_radius(THUMB_RADIUS),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(filled_w, 0)
                .with_size(Size::px(THUMB_SIZE, THUMB_SIZE)),
        ),
    );

    let track = Scene::Container(
        ContainerNode::new(vec![rail, filled, thumb])
            .with_tag(TAG)
            .with_aria_label("Brightness")
            .with_layout(LayoutStyle::new().with_size(Size::px(TRACK_W, THUMB_SIZE))),
    );

    let stop_labels = stop_label_row(&theme, interaction, active);

    let title = Scene::Text(TextNode::styled(
        "Brightness",
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    // Prominent current-stop readout — the visible twin of `aria-valuetext`
    // (both funnel through `label_for`, so they cannot diverge).
    let readout = Scene::Text(
        TextNode::styled(
            label_for(value),
            Rect::default(),
            TextStyle::new()
                .with_size_px(22)
                .with_fg(slider_accent_for(&theme, interaction)),
        )
        .with_tag(READOUT),
    );
    // Status echoes interaction state + the snapped value + its stop index
    // (AI text mirror, mirror of the discrete slider's status line).
    let status = Scene::Text(
        TextNode::styled(
            format!(
                "{} | {value:.2} (stop {active}/{})",
                interaction.as_name(),
                STOPS - 1
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(STATUS),
    );

    Scene::Container(
        ContainerNode::new(vec![title, track, stop_labels, readout, status])
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
/// `hello-slider` / `hello-slider-vertical` / `hello-slider-discrete` /
/// `hello-range-slider` / `settings-panel`). The missing-external
/// fallback is this binding's own boot stop.
fn read_labeled(scene: &Scene) -> LabeledState {
    let (interaction, value) =
        read_slider_state(scene, TAG).unwrap_or((SliderState::Idle, START));
    LabeledState { interaction, value }
}

struct LabeledView;

impl WidgetCore for LabeledView {
    type State = LabeledState;
    // Value mutation flows through drag (pointer_move) + apply_key; no
    // keybinding-channel typed events.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        // Labeled slider stepped at STEP, seeded to the START stop so the
        // boot frame shows a real on-grid value (0.5 = "Medium").
        let mut ext = SliderExternal::with_step(STEP);
        ext.set_value(START);
        Box::new(ext)
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> LabeledState {
        read_labeled(scene)
    }

    fn view(state: LabeledState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-slider-labeled (R739 §5.40 aria-valuetext)"
    }

    /// WAI-ARIA discrete-slider keyboard: arrows move one stop, Home / End
    /// jump to the extreme stops. The focus-guard / disabled-check / read /
    /// `intervene("value", Float)` scaffold is the lifted R739.1
    /// [`slider_apply_key`] SSOT (shared with the continuous / vertical /
    /// discrete sliders); only the stop key-map is per-widget. The write
    /// funnels through `intervene` so the substrate snap applies.
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

    fn fmt_state_log(state: &LabeledState) -> String {
        format!("{} / {} ({:.2})", state.interaction.as_name(), label_for(state.value), state.value)
    }
}

impl WidgetA11y for LabeledView {
    /// R739 §5.40 — one `slider` node carrying BOTH the snapped numeric
    /// value as [`AccessValue::Float`] (`aria-valuenow` / `aria-valuemin` /
    /// `aria-valuemax`) AND the named stop as `aria-valuetext` via
    /// [`AccessNode::with_value_text`]. AT announces the label ("Medium")
    /// but retains the numeric range for context. The label comes from the
    /// same [`label_for`] the visible readout uses.
    fn access_node(state: &LabeledState, focused: Option<&str>) -> Vec<AccessNode> {
        vec![AccessNode::new(TAG, AriaRole::Slider)
            .with_value(AccessValue::Float { value: state.value, min: 0.0, max: 1.0 })
            .with_value_text(label_for(state.value))
            .with_state(AccessState {
                focused: focused == Some(TAG),
                disabled: matches!(state.interaction, SliderState::Disabled),
                hovered: matches!(state.interaction, SliderState::Hover),
                pressed: matches!(state.interaction, SliderState::Dragging),
                checked: None,
            })]
    }
}

impl WidgetView for LabeledView {
    type Renderer = HelloSliderLabeledRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed { width: WIN_W, height: WIN_H }
    }
}

fn main() {
    pinion_shell::run::<LabeledView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::IntrospectValue;
    use pinion_core::scene::ExternalNode;

    fn scene_fixture() -> Scene {
        Scene::External(ExternalNode::new(LabeledView::create_external()).with_tag(TAG))
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
    fn step_stops_and_labels_are_consistent() {
        // STOPS = 1/STEP + 1, and LABELS has exactly one name per stop;
        // guards the three consts against drift.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let derived = (1.0_f32 / STEP).round() as usize + 1;
        assert_eq!(derived, STOPS, "STOPS must equal 1/STEP + 1");
        assert_eq!(LABELS.len(), STOPS, "one label per stop");
    }

    #[test]
    fn label_for_maps_every_stop() {
        // Each on-grid stop value maps to its name; off-grid values map to
        // the nearest stop's name (the snap the substrate also applies).
        assert_eq!(label_for(0.0), "Off");
        assert_eq!(label_for(0.25), "Low");
        assert_eq!(label_for(0.5), "Medium");
        assert_eq!(label_for(0.75), "High");
        assert_eq!(label_for(1.0), "Max");
        assert_eq!(label_for(0.6), "Medium", "0.6 rounds to the 0.5 stop");
        assert_eq!(label_for(0.66), "High", "0.66 rounds to the 0.75 stop");
    }

    #[test]
    fn boot_value_sits_on_the_medium_stop() {
        let scene = scene_fixture();
        // START seeded through with_step → snapped; 0.5 is already a stop.
        assert!((value_of(&scene) - START).abs() < 1e-5, "boot on the START stop");
        assert_eq!(label_for(value_of(&scene)), "Medium", "boot stop is Medium");
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
    fn arrow_right_advances_one_stop() {
        let mut scene = scene_fixture(); // boot 0.5 (Medium)
        assert!(LabeledView::apply_key(&mut scene, Some(TAG), "ArrowRight", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 0.75).abs() < 1e-5, "0.5 -> 0.75 (one stop)");
        assert_eq!(label_for(value_of(&scene)), "High");
        assert!(LabeledView::apply_key(&mut scene, Some(TAG), "ArrowLeft", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 0.5).abs() < 1e-5, "0.75 -> 0.5");
        assert_eq!(label_for(value_of(&scene)), "Medium");
    }

    #[test]
    fn arrows_clamp_at_extreme_stops() {
        let mut scene = scene_fixture();
        assert!(LabeledView::apply_key(&mut scene, Some(TAG), "End", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 1.0).abs() < 1e-5, "End -> 1.0 (Max)");
        assert_eq!(label_for(value_of(&scene)), "Max");
        assert!(LabeledView::apply_key(&mut scene, Some(TAG), "ArrowRight", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 1.0).abs() < 1e-5, "ArrowRight at max clamps");
        assert!(LabeledView::apply_key(&mut scene, Some(TAG), "Home", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 0.0).abs() < 1e-5, "Home -> 0.0 (Off)");
        assert_eq!(label_for(value_of(&scene)), "Off");
        assert!(LabeledView::apply_key(&mut scene, Some(TAG), "ArrowLeft", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - 0.0).abs() < 1e-5, "ArrowLeft at min clamps");
    }

    #[test]
    fn off_grid_intervene_snaps_to_nearest_stop() {
        let mut scene = scene_fixture();
        let Scene::External(node) = &mut scene else { panic!() };
        let intro = node.handle.introspect_mut().unwrap();
        // An AI client writing an off-grid value is snapped by the substrate.
        intro.intervene("value", IntrospectValue::Float(0.66)).unwrap();
        assert!((value_of(&scene) - 0.75).abs() < 1e-5, "0.66 snaps to 0.75");
        assert_eq!(label_for(value_of(&scene)), "High");
    }

    #[test]
    fn keys_ignored_when_not_focused() {
        let mut scene = scene_fixture();
        assert!(!LabeledView::apply_key(&mut scene, None, "ArrowRight", pinion_core::Modifiers::empty()));
        assert!(!LabeledView::apply_key(&mut scene, Some("other"), "ArrowRight", pinion_core::Modifiers::empty()));
        assert!((value_of(&scene) - START).abs() < 1e-5, "value unchanged");
    }

    #[test]
    fn view_carries_track_readout_and_status_tags() {
        let scene = pinion_core::Owner::new().run(|| view(&LabeledState::boot(), &Frame::new()));
        assert!(scene.contains_tag(TAG), "track tag painted");
        assert!(scene.contains_tag(READOUT), "readout tag painted");
        assert!(scene.contains_tag(STATUS), "status tag painted");
    }

    #[test]
    fn view_contains_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<LabeledView>(
            LabeledState::boot(),
            &Frame::default(),
        );
    }

    #[test]
    fn emits_slider_node_with_float_value_and_valuetext() {
        // R739 §5.40 — the labeled slider carries BOTH the numeric Float
        // (aria-valuenow/min/max) AND the named aria-valuetext.
        let nodes = LabeledView::access_node(&LabeledState::boot(), Some(TAG));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Slider);
        assert!(nodes[0].state.focused);
        assert_eq!(nodes[0].value_text.as_deref(), Some("Medium"), "boot valuetext = Medium");
        match nodes[0].value {
            Some(AccessValue::Float { value, min, max }) => {
                assert!((value - START).abs() < f32::EPSILON);
                assert!((min - 0.0).abs() < f32::EPSILON);
                assert!((max - 1.0).abs() < f32::EPSILON);
            }
            ref other => panic!("expected Float value, got {other:?}"),
        }
    }

    #[test]
    fn valuetext_follows_the_snapped_value() {
        // Drive the value to each stop and confirm the a11y value_text
        // tracks label_for — the announced label twins the visible readout.
        let mut scene = scene_fixture();
        for (key, expected) in [("End", "Max"), ("Home", "Off")] {
            assert!(LabeledView::apply_key(&mut scene, Some(TAG), key, pinion_core::Modifiers::empty()));
            let st = LabeledView::read_state(&scene);
            let nodes = LabeledView::access_node(&st, Some(TAG));
            assert_eq!(nodes[0].value_text.as_deref(), Some(expected), "{key} -> {expected}");
        }
    }
}
