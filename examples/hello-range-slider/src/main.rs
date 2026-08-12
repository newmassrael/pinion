//! `hello-range-slider` — R738 §5.38 §5.40 **dual-thumb range slider**
//! (the WAI-ARIA "Slider (Multi-Thumb)" pattern and the Material
//! range-slider form factor: a price filter, a histogram level window, an
//! audio/video trim, a min/max constraint pair).
//!
//! ## 1st consumer of the R738 `RangeSliderExternal` substrate
//!
//! R738 added a dual-thumb coordinating widget to the core
//! ([`RangeSliderExternal`](pinion_core::widgets::range_slider)). The two
//! thumbs share one track rect, so a single press / drag forwards exactly
//! one cursor position — the external picks the *nearest* thumb on the
//! first move of a drag and latches it for the gesture (a drag started on
//! the low thumb keeps moving the low thumb), and enforces the monotonic
//! `0 <= low <= high <= 1` invariant (each setter clamps to the other
//! thumb). A range slider therefore cannot be two independent
//! `SliderExternal`s: the framework's `InputRouter` could not know which
//! stacked external the user grabbed. See the substrate module docs.
//!
//! ## Two Tab stops (APG Multi-Thumb keyboard)
//!
//! WAI-ARIA "Slider (Multi-Thumb)" places **each** thumb in the Tab
//! sequence. The two thumbs paint with the composite hit/focus tags
//! `range#low` / `range#high` (the R51.42 §5.35 composite protocol routes
//! their pointer events to the single `Scene::External("range")`), and
//! both thumbs are marked `.with_focusable(true)` (the scene-derived §5.39
//! Tab stops), so **clicking a thumb focuses it**. `ArrowRight`/`ArrowUp` raise the
//! focused thumb by [`KEY_STEP`], `ArrowLeft`/`ArrowDown` lower it,
//! `Home`/`End` jump to the thumb's min/max — every key writes through
//! the same `intervene("low"|"high")` channel the drag + RPC use, so the
//! monotonic clamp applies uniformly (an `End` on the low thumb lands on
//! the high thumb, not on `1.0`).
//!
//! Pointer: pressing a thumb captures its sub-tag, but the dragged value
//! spans the whole track — `RangeSliderExternal` opts into
//! [`capture_normalize`](pinion_core::external::External::capture_normalize)
//! returning `CaptureNormalize::Primary` so the cursor normalises against the
//! *track* rect (not the 18px thumb
//! rect, which would saturate and grab the far thumb — the R738.1 bug).
//! The substrate's nearest-thumb
//! [`pick`](pinion_core::widgets::range_slider::RangeSlider) latches the
//! grabbed thumb for the gesture; pressing the bare track between thumbs
//! moves (but does not focus) the nearest thumb.
//!
//! ## a11y (R738 §5.40) — dual-thumb cross-constraint
//!
//! A parent [`AriaRole::Group`] (`range`) claims two
//! [`AriaRole::Slider`] children. Each thumb's range is bounded by the
//! other (the canonical dual-thumb a11y: the low thumb reports
//! `aria-valuemax = high`, the high thumb reports `aria-valuemin = low`),
//! so an assistive client always hears a consistent, non-crossing pair.
//! The slider value lowers through the same
//! [`AccessValue::Float`]
//! `aria-valuenow` path the single slider uses — no new a11y axis, only
//! the `Group` role + two cross-constrained `Slider` children.

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widgets::range_slider::{RangeSliderExternal, ThumbId};
use pinion_core::widgets::slider::SliderState;
use pinion_core::{Frame, Scene, WidgetCore, WidgetStateName, scale_normalized_to_px};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::slider::{slider_accent_for, slider_thumb_fill, slider_track_inactive};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloRangeSliderRenderer, HelloRangeSliderRendererError);

const WIN_W: u32 = 400;
const WIN_H: u32 = 240;
const THEME_TAG: &str = "app";

/// The primary range-slider tag: the track container's hit-test +
/// `Scene::External` registration root (catches between-thumb clicks).
const TAG: &str = "range";
/// Composite hit/focus tag for the low thumb (primary half = `range`).
const LOW_TAG: &str = "range#low";
/// Composite hit/focus tag for the high thumb (primary half = `range`).
const HIGH_TAG: &str = "range#high";

/// Normalised keyboard increment per arrow press (the APG "step").
const KEY_STEP: f32 = 0.05;
/// Boot sub-range — a quarter-to-three-quarters window.
const START_LOW: f32 = 0.25;
const START_HIGH: f32 = 0.75;

// Track geometry (mirror of hello-slider-discrete's Material rail).
const TRACK_W: u32 = 240;
const TRACK_H: u32 = 8;
const TRACK_RADIUS: u32 = 4;
const THUMB_SIZE: u32 = 18;
const THUMB_RADIUS: u32 = 9;
/// Drag range = track width minus thumb width; thumb centres span
/// `[THUMB_SIZE/2, THUMB_SIZE/2 + RANGE]`.
const RANGE: u32 = TRACK_W - THUMB_SIZE;
const ROW_GAP: u32 = 18;

/// Cached posture: the shared SCXML interaction state, the two thumb
/// values, and which thumb a mutation last landed on. All read from the
/// live `RangeSliderExternal` so the cache never diverges from the
/// AI-observable introspect surface ([[update-by-value-snapshot]]).
#[derive(Copy, Clone, PartialEq, Debug)]
struct RangeState {
    interaction: SliderState,
    low: f32,
    high: f32,
    active: ThumbId,
}

impl RangeState {
    /// The boot posture (Idle, the START window, high thumb active). The
    /// single source for both the `read_range` missing-external fallback
    /// (matching `create_external`) and the view / a11y unit-test fixture.
    fn boot() -> Self {
        Self {
            interaction: SliderState::Idle,
            low: START_LOW,
            high: START_HIGH,
            active: ThumbId::High,
        }
    }
}

/// Read `(interaction, low, high, active)` from the live
/// `RangeSliderExternal`. 1st consumer of the range introspect surface,
/// so this reader is inline (a `(low, high, active)` shape distinct from
/// the lifted single-slider [`read_slider_state`](pinion_widget_paint::slider::read_slider_state),
/// which reads one `value`; a 2nd range-class consumer would lift this).
/// The missing-external fallback is this binding's own boot window.
fn read_range(scene: &Scene) -> RangeState {
    let Some(node) = scene.find_external_with_tag(TAG) else {
        return RangeState::boot();
    };
    let Some(intro) = node.handle.introspect() else {
        return RangeState::boot();
    };
    let interaction = match intro.query("state") {
        Ok(IntrospectValue::Text(name)) => SliderState::from_name_or_default(&name),
        _ => SliderState::Idle,
    };
    let low = intro
        .query("low")
        .ok()
        .and_then(|v| v.as_f32())
        .unwrap_or(START_LOW);
    let high = intro
        .query("high")
        .ok()
        .and_then(|v| v.as_f32())
        .unwrap_or(START_HIGH);
    let active = match intro.query("active") {
        Ok(IntrospectValue::Text(name)) => ThumbId::from_name(&name).unwrap_or(ThumbId::High),
        _ => ThumbId::High,
    };
    RangeState {
        interaction,
        low,
        high,
        active,
    }
}

/// A thumb box at normalised position `value`, tagged with its composite
/// hit/focus tag (`range#low` / `range#high`) so a click **focuses** it
/// (the tag is marked `.with_focusable(true)` — a scene-derived §5.39 Tab stop)
/// and the press routes to the shared `range` external (R51.42 §5.35
/// composite protocol). Takes the resolved `theme` (mirrors
/// `hello-slider-discrete`'s `tick`) rather than re-resolving `use_theme`
/// per thumb — the view subscribes once.
///
/// The dragged *value* still maps across the whole track, not this thumb:
/// `RangeSliderExternal` opts into
/// [`capture_normalize`](pinion_core::external::External::capture_normalize)
/// (`CaptureNormalize::Primary`), so the `InputRouter` normalizes the cursor against the
/// primary (track) rect even though capture pinned this thumb's sub-tag.
/// (Without that opt-in the cursor would normalize against the ~18px
/// thumb rect and saturate, so every drag grabbed the far thumb — the
/// R738.1 bug.) The substrate's nearest-thumb
/// [`RangeSlider::pick`](pinion_core::widgets::range_slider::RangeSlider)
/// then latches the closer thumb.
///
/// The handle keeps the shared [`slider_thumb_fill`] (`OnAccent` / white)
/// **plus a 2px accent border ring**. A bare white handle is invisible on
/// the light page (it reads as a *gap* between the rail and the active
/// fill — the dual-thumb form factor makes this obvious where the single
/// slider hides it); the ring defines the knob against both the page
/// (blue ring) and the active fill (white centre).
fn thumb(value: f32, tag: &'static str, theme: &Theme, interaction: SliderState) -> Scene {
    let x = scale_normalized_to_px(value.clamp(0.0, 1.0), RANGE);
    Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(slider_thumb_fill(theme, interaction))
                .with_corner_radius(THUMB_RADIUS)
                .with_border(Border::new(slider_accent_for(theme, interaction), 2)),
        )
        .with_tag(tag)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(x, 0)
                // R1020 §5.39 — each thumb is a Tab stop; opt its tag-carrying
                // Box node into the scene-derived focus enumeration. Painted
                // low-then-high, matching the [LOW_TAG, HIGH_TAG] order.
                .with_focusable(true)
                .with_size(Size::px(THUMB_SIZE, THUMB_SIZE)),
        ),
    )
}

/// view-fn (§6.3): pure sync `RangeState -> Scene`. A label, the tagged
/// track (rail + between-fill + two thumbs, absolutely positioned), and a
/// status line.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: &RangeState, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let low = state.low.clamp(0.0, 1.0);
    let high = state.high.clamp(0.0, 1.0);
    let interaction = state.interaction;

    let low_x = scale_normalized_to_px(low, RANGE);
    let high_x = scale_normalized_to_px(high, RANGE);
    let rail_y = (THUMB_SIZE - TRACK_H) / 2;

    // Rail (inactive track) spanning the full thumb-centre travel.
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
    // Active (filled) portion *between* the two thumb centres.
    let fill = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(slider_accent_for(&theme, interaction))
                .with_corner_radius(TRACK_RADIUS),
        )
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(low_x + THUMB_SIZE / 2, rail_y)
                .with_size(Size::px(high_x.saturating_sub(low_x), TRACK_H)),
        ),
    );

    let track = Scene::Container(
        ContainerNode::new(vec![
            rail,
            fill,
            thumb(low, LOW_TAG, &theme, interaction),
            thumb(high, HIGH_TAG, &theme, interaction),
        ])
        .with_tag(TAG)
        .with_aria_label("Price range")
        .with_layout(LayoutStyle::new().with_size(Size::px(TRACK_W, THUMB_SIZE))),
    );

    let label = Scene::Text(TextNode::styled(
        "Price range",
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let status = Scene::Text(
        TextNode::styled(
            format!(
                "{} | min {low:.2} max {high:.2} (active: {})",
                interaction.as_name(),
                state.active.as_name()
            ),
            Rect::default(),
            TextStyle::new()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag("range_status"),
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

struct RangeView;

impl WidgetCore for RangeView {
    type State = RangeState;
    // Value mutation flows through drag (pointer_move) + apply_key; no
    // keybinding-channel typed events.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(RangeSliderExternal::with_values(START_LOW, START_HIGH))
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> RangeState {
        read_range(scene)
    }

    fn view(state: RangeState, frame: &Frame) -> Scene {
        view(&state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-range-slider (R738 §5.38 dual-thumb range slider)"
    }

    /// APG Multi-Thumb keyboard: the focused thumb's value moves by
    /// `KEY_STEP` per arrow, `Home`/`End` jump to its bounds. Every key
    /// writes through `intervene("low"|"high", Float)`, so the substrate's
    /// monotonic clamp applies (an `End` on the low thumb stops at the
    /// high thumb, not at `1.0`).
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let path = if focused == Some(LOW_TAG) {
            "low"
        } else if focused == Some(HIGH_TAG) {
            "high"
        } else {
            return false;
        };
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let Some(current) = intro.query(path).ok().and_then(|v| v.as_f32()) else {
            return false;
        };
        let next = match key {
            "ArrowRight" | "ArrowUp" => (current + KEY_STEP).clamp(0.0, 1.0),
            "ArrowLeft" | "ArrowDown" => (current - KEY_STEP).clamp(0.0, 1.0),
            "Home" => 0.0,
            "End" => 1.0,
            _ => return false,
        };
        intro
            .intervene(path, IntrospectValue::Float(f64::from(next)))
            .is_ok()
    }

    fn fmt_state_log(state: &RangeState) -> String {
        format!(
            "{} / [{:.2}, {:.2}] active={}",
            state.interaction.as_name(),
            state.low,
            state.high,
            state.active.as_name()
        )
    }
}

impl WidgetA11y for RangeView {
    /// R738 §5.40 — a `group` parent claiming two cross-constrained
    /// `slider` children. The low thumb's `aria-valuemax` is the high
    /// value and the high thumb's `aria-valuemin` is the low value (the
    /// canonical dual-thumb a11y), so the pair never reads as crossed.
    fn access_node(state: &RangeState, focused: Option<&str>) -> Vec<AccessNode> {
        let group = AccessNode::new(TAG, AriaRole::Group)
            .with_name("Price range")
            .with_child(LOW_TAG)
            .with_child(HIGH_TAG);
        let low = AccessNode::new(LOW_TAG, AriaRole::Slider)
            .with_name("Minimum")
            .with_value(AccessValue::Float {
                value: state.low,
                min: 0.0,
                max: state.high,
            })
            .with_state(thumb_state(state, focused, LOW_TAG, ThumbId::Low));
        let high = AccessNode::new(HIGH_TAG, AriaRole::Slider)
            .with_name("Maximum")
            .with_value(AccessValue::Float {
                value: state.high,
                min: state.low,
                max: 1.0,
            })
            .with_state(thumb_state(state, focused, HIGH_TAG, ThumbId::High));
        vec![group, low, high]
    }
}

/// Per-thumb [`AccessState`]: focused when this thumb's tag holds focus,
/// pressed when the slider is dragging *this* (the active) thumb.
fn thumb_state(
    state: &RangeState,
    focused: Option<&str>,
    tag: &str,
    thumb: ThumbId,
) -> AccessState {
    AccessState {
        focused: focused == Some(tag),
        disabled: matches!(state.interaction, SliderState::Disabled),
        pressed: matches!(state.interaction, SliderState::Dragging) && state.active == thumb,
        ..AccessState::default()
    }
}

impl WidgetView for RangeView {
    type Renderer = HelloRangeSliderRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<RangeView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::scene::ExternalNode;

    fn scene_fixture() -> Scene {
        Scene::External(ExternalNode::new(RangeView::create_external()).with_tag(TAG))
    }

    fn vals(scene: &Scene) -> (f32, f32) {
        let Scene::External(node) = scene else {
            panic!("expected External root");
        };
        let intro = node.handle.introspect().expect("introspect opted in");
        let low = intro
            .query("low")
            .ok()
            .and_then(|v| v.as_f32())
            .expect("low");
        let high = intro
            .query("high")
            .ok()
            .and_then(|v| v.as_f32())
            .expect("high");
        (low, high)
    }

    fn key(scene: &mut Scene, focused: &str, k: &str) -> bool {
        RangeView::apply_key(scene, Some(focused), k, pinion_core::Modifiers::empty())
    }

    #[test]
    fn boot_window_seeded() {
        let (low, high) = vals(&scene_fixture());
        assert!((low - START_LOW).abs() < 1e-5);
        assert!((high - START_HIGH).abs() < 1e-5);
    }

    #[test]
    fn two_focusable_tab_stops() {
        // §5.39: tree order IS tab order — collected from the paint scene.
        let scene = pinion_core::Owner::new().run(|| view(&RangeState::boot(), &Frame::new()));
        assert_eq!(
            scene.collect_focusable_tags(),
            vec![LOW_TAG.to_owned(), HIGH_TAG.to_owned()],
        );
    }

    #[test]
    fn arrows_move_the_focused_thumb_only() {
        let mut scene = scene_fixture(); // [0.25, 0.75]
        assert!(key(&mut scene, LOW_TAG, "ArrowRight"));
        let (low, high) = vals(&scene);
        assert!((low - 0.30).abs() < 1e-5, "low thumb +KEY_STEP");
        assert!((high - 0.75).abs() < 1e-5, "high untouched");
        assert!(key(&mut scene, HIGH_TAG, "ArrowLeft"));
        let (low, high) = vals(&scene);
        assert!((low - 0.30).abs() < 1e-5, "low untouched");
        assert!((high - 0.70).abs() < 1e-5, "high thumb -KEY_STEP");
    }

    #[test]
    fn low_thumb_end_clamps_at_high_not_one() {
        let mut scene = scene_fixture(); // [0.25, 0.75]
        assert!(key(&mut scene, LOW_TAG, "End"));
        let (low, high) = vals(&scene);
        // End on the low thumb sends 1.0, monotonic clamp stops it at the
        // high thumb (0.75) — never at 1.0.
        assert!((low - 0.75).abs() < 1e-5, "low clamps at high");
        assert!((high - 0.75).abs() < 1e-5);
    }

    #[test]
    fn high_thumb_home_clamps_at_low_not_zero() {
        let mut scene = scene_fixture();
        assert!(key(&mut scene, HIGH_TAG, "Home"));
        let (low, high) = vals(&scene);
        assert!(
            (high - 0.25).abs() < 1e-5,
            "high clamps at low (0.25), not 0.0"
        );
        assert!((low - 0.25).abs() < 1e-5);
    }

    #[test]
    fn keys_ignored_when_no_thumb_focused() {
        let mut scene = scene_fixture();
        assert!(!RangeView::apply_key(
            &mut scene,
            None,
            "ArrowRight",
            pinion_core::Modifiers::empty()
        ));
        assert!(!RangeView::apply_key(
            &mut scene,
            Some(TAG),
            "ArrowRight",
            pinion_core::Modifiers::empty()
        ));
        let (low, high) = vals(&scene);
        assert!((low - START_LOW).abs() < 1e-5 && (high - START_HIGH).abs() < 1e-5);
    }

    #[test]
    fn view_carries_track_and_thumb_tags() {
        let scene = pinion_core::Owner::new().run(|| view(&RangeState::boot(), &Frame::new()));
        assert!(scene.contains_tag(TAG), "track tag painted");
        assert!(
            scene.contains_tag(LOW_TAG),
            "low thumb tag painted (clickable/focusable)"
        );
        assert!(scene.contains_tag(HIGH_TAG), "high thumb tag painted");
        assert!(scene.contains_tag("range_status"), "status tag painted");
    }

    #[test]
    fn view_contains_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<RangeView>(
            RangeState::boot(),
            &Frame::default(),
        );
    }

    #[test]
    fn a11y_group_with_two_cross_constrained_sliders() {
        let nodes = RangeView::access_node(&RangeState::boot(), Some(LOW_TAG));
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].role, AriaRole::Group);
        assert_eq!(nodes[1].role, AriaRole::Slider);
        assert_eq!(nodes[2].role, AriaRole::Slider);
        assert!(nodes[1].state.focused, "low thumb focused");
        assert!(!nodes[2].state.focused, "high thumb not focused");
        // Cross-constraint: low.max == high value, high.min == low value.
        match (nodes[1].value.clone(), nodes[2].value.clone()) {
            (
                Some(AccessValue::Float { max: low_max, .. }),
                Some(AccessValue::Float { min: high_min, .. }),
            ) => {
                assert!(
                    (low_max - START_HIGH).abs() < f32::EPSILON,
                    "low max = high value"
                );
                assert!(
                    (high_min - START_LOW).abs() < f32::EPSILON,
                    "high min = low value"
                );
            }
            other => panic!("expected Float values, got {other:?}"),
        }
    }
}
