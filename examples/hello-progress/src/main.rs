//! `hello-progress` — R718 §5.38 §5.40 §5.50 — first consumer of the
//! [`ProgressBarExternal`](pinion_core::widgets::progress_bar) value
//! holder (the second `AccessValue::Float` a11y consumer after
//! [`Slider`](pinion_core::widgets::slider), as anticipated in
//! `slider.rs`).
//!
//! A determinate linear progress bar: a 240x6 pill track split into a
//! filled (accent) leading portion and an unfilled (inactive-track)
//! remainder, with a percent readout beneath. The bar is descriptive
//! (non-interactive): it owns no statechart, takes no focus, and emits
//! no intents. Its single observable axis is the normalized progress
//! fraction, **driven through the §5.15 introspect channel** —
//! `intervene("value", Float)`, the same side door the RPC
//! `scene/intervene` route and a real application's progress updater
//! both use. So an AI client setting `/external/value` and the host
//! advancing the task converge on one observable state.
//!
//! a11y: an [`AriaRole::ProgressBar`] node carrying the fraction as
//! [`AccessValue::Float`] (`aria-valuenow` / `aria-valuemin` /
//! `aria-valuemax`). The role is *passive* — AT reads the value as a
//! read-only status, not an operable control (distinct from a slider).
//!
//! R726 §5.28 §5.40 — the bar also supports an **indeterminate**
//! ("busy, completion unknown") mode: `intervene("indeterminate", true)`
//! switches the fraction track for a looping accent segment driven by
//! the [`IndeterminateSweep`](pinion_core::widgets::progress_bar::IndeterminateSweep)
//! repeating Tickable, and the a11y node omits `aria-valuenow` (the
//! WAI-ARIA "unknown progress" signal). The sweep advances on the §5.28
//! animation driver, so the R724 `scene/tick` RPC drives + verifies it
//! deterministically.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::progress_bar::{ProgressBarExternal, use_indeterminate_sweep};
use pinion_core::{Frame, Scene, WidgetCore, scale_normalized_to_px};
use pinion_shell::{WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloProgressRenderer, HelloProgressRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 160;
/// [`ThemeProvider`](pinion_core::theme::ThemeProvider) cache key — matches the `"app"` convention shared
/// across the example gallery.
const THEME_TAG: &str = "app";
/// The track row's hit-test + a11y tag; also the root `External` tag
/// the framework maps `read_state` / RPC `/external/*` onto.
const TAG: &str = "progress";
/// The percent-readout text node tag — a deterministic mirror so the
/// AI side (and the demo) can read the live fraction as text without a
/// pixel round-trip.
const STATUS_TAG: &str = "progress_status";
/// Accessible name for the bar (what the task is).
const LABEL: &str = "Download";
/// Track is 240x6 — a thin Material-style rail; the pill radius is 3
/// (= `TRACK_H` / 2) so the ends round flush.
const TRACK_W: u32 = 240;
const TRACK_H: u32 = 6;
const TRACK_RADIUS: u32 = 3;
const ROW_GAP: u32 = 14;
/// Boot fraction — a clear non-trivial value (40 %) so the filled
/// portion is visible at startup for the `PINION_SCREENSHOT` live-pixel
/// guard, while leaving an unfilled remainder to contrast against.
const BOOT_VALUE: f32 = 0.4;
/// R726 — width of the travelling indicator segment for the
/// indeterminate sweep (30 % of the track). Its left edge slides across
/// `[0, TRACK_W - SEG_W]` as the sweep phase runs `0.0..1.0`.
const SEG_W: u32 = 72;
/// R726 — `Owner::cache` key for the indeterminate sweep driver.
const SWEEP_KEY: &str = "progress.sweep";
/// R726 — the travelling indicator segment's tag (so the demo reads its
/// moving rect via `scene/snapshot`).
const INDICATOR_TAG: &str = "progress_indicator";

/// Determinate track: the `[filled | unfilled]` accent/inactive split.
fn determinate_track(value: f32, theme: &pinion_core::theme::Theme) -> Scene {
    let filled_color = theme.resolve(ColorRole::Accent);
    let unfilled_color = theme.resolve(ColorRole::SurfaceContainerHighest);
    // `scale_normalized_to_px` clamps + casts the fraction to a leading
    // pixel width; the remainder saturating-subtracts so the two pieces
    // always sum to the full track width.
    let filled_w = scale_normalized_to_px(value, TRACK_W);
    let unfilled_w = TRACK_W.saturating_sub(filled_w);
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
    Scene::Container(
        ContainerNode::new(vec![filled, unfilled])
            .with_tag(TAG)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(TRACK_W, TRACK_H)),
            ),
    )
}

/// R726 — indeterminate track: an inactive-tone rail with a travelling
/// accent segment whose left edge follows the sweep `phase` (`0.0..1.0`)
/// across `[0, TRACK_W - SEG_W]`. The segment is absolutely positioned
/// (out of flex flow) inside the tagged track container.
fn indeterminate_track(phase: f32, theme: &pinion_core::theme::Theme) -> Scene {
    let left = scale_normalized_to_px(phase, TRACK_W.saturating_sub(SEG_W));
    let indicator = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(theme.resolve(ColorRole::Accent)).with_corner_radius(TRACK_RADIUS),
        )
        .with_tag(INDICATOR_TAG)
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(left, 0)
                .with_size(Size::px(SEG_W, TRACK_H)),
        ),
    );
    Scene::Container(
        ContainerNode::new(vec![indicator])
            .with_tag(TAG)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest))
                    .with_corner_radius(TRACK_RADIUS),
            )
            .with_layout(LayoutStyle::new().with_size(Size::px(TRACK_W, TRACK_H))),
    )
}

/// view-fn (§6.3): pure sync mapping `(fraction, indeterminate) ->
/// Scene`. Determinate: `[filled | unfilled]` fraction track + percent
/// readout. Indeterminate: a looping sweep segment (driven by the
/// §5.28 `IndeterminateSweep` Tickable) + a "Working…" readout, with
/// no fraction (WAI-ARIA "completion unknown").
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: (f32, bool), _frame: &Frame) -> Scene {
    let (value, indeterminate) = state;
    let theme = use_theme(THEME_TAG).theme_animated();

    // R726/R727 — gate the sweep on the mode. Unlike `CaretBlink`, whose
    // `set_enabled` is driven from the shell's focus-change *listener*,
    // the `indeterminate` flag lives on the `ProgressBarExternal` and is
    // flipped by `intervene` (RPC / task driver) — which has no listener
    // hook, so the view-fn is the only owner-scoped site that observes
    // the change (`read_state` is not owner-wrapped). This is a derived
    // *idempotent* gate, NOT an edge-triggered mailbox side effect, so
    // the R693 "side effects belong in the reducer for handle_tail
    // timing" concern does not apply: `set_active` early-returns on a
    // same-value call (Signal equality-skip), writing the `position`
    // Signal only on an actual transition — no re-render loop, no timing
    // dependency. Parked while determinate (the sweep reports at-rest so
    // the backend releases frames); looping while indeterminate. A
    // dedicated "External-field-changed → animation-gate" hook is the
    // only fully-listener-driven alternative; deferred until a 2nd
    // consumer needs it (premature for one).
    let sweep = use_indeterminate_sweep(SWEEP_KEY);
    sweep.set_active(indeterminate);

    let track = if indeterminate {
        indeterminate_track(sweep.position(), &theme)
    } else {
        determinate_track(value, &theme)
    };
    let heading = Scene::Text(TextNode::styled(
        LABEL,
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    // Determinate shows the whole-percent string ("40%"); indeterminate
    // has no fraction to report, so it shows a busy label.
    let readout = if indeterminate {
        "Working\u{2026}".to_string()
    } else {
        format!("{:.0}%", value.clamp(0.0, 1.0) * 100.0)
    };
    let status = Scene::Text(
        TextNode::styled(
            readout,
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(STATUS_TAG),
    );
    Scene::Container(
        ContainerNode::new(vec![heading, track, status])
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

/// Manual [`WidgetCore`] binding (the descriptive-widget pattern shared
/// with `hello-tooltip`): a value-only widget has no interaction-state
/// enum and no keyboard-channel events, so the `#[widget]` derive
/// (geared to `state_flags` over an interaction enum) does not apply.
struct ProgressView;

impl WidgetCore for ProgressView {
    type State = (f32, bool);
    // A progress bar emits no keyboard-channel events (mirrors
    // hello-tooltip / hello-menu): value changes flow through the RPC /
    // application `intervene` channel, not `event_name`.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ProgressBarExternal::with_value(BOOT_VALUE))
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(scene: &Scene) -> (f32, bool) {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                let value = intro.query("value").and_then(|v| v.as_f32()).unwrap_or(0.0);
                let indeterminate = matches!(
                    intro.query("indeterminate"),
                    Some(pinion_core::external::IntrospectValue::Bool(true))
                );
                return (value, indeterminate);
            }
        }
        (0.0, false)
    }

    fn view(state: (f32, bool), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-progress (R718 §5.38 §5.40 §5.50)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn fmt_state_log(state: &(f32, bool)) -> String {
        if state.1 {
            "indeterminate".to_string()
        } else {
            format!("{:.2}", state.0)
        }
    }
}

impl WidgetA11y for ProgressView {
    /// R718 §5.40 — a single passive [`AriaRole::ProgressBar`] node
    /// carrying the fraction as [`AccessValue::Float`] (`aria-valuenow`
    /// in `[0, 1]`). The bar is non-focusable, so `focused` is ignored
    /// and the default (all-false) interaction state applies.
    /// R726 §5.40 — an indeterminate bar omits `aria-valuenow` entirely
    /// (the WAI-ARIA signal for "progress is unknown"); a determinate
    /// bar carries the fraction as [`AccessValue::Float`].
    fn access_node(state: &(f32, bool), _focused: Option<&str>) -> Vec<AccessNode> {
        let (value, indeterminate) = *state;
        let node = AccessNode::new(TAG, AriaRole::ProgressBar).with_name(LABEL);
        let node = if indeterminate {
            node
        } else {
            node.with_value(AccessValue::Float {
                value,
                min: 0.0,
                max: 1.0,
            })
        };
        vec![node]
    }
}

impl WidgetView for ProgressView {
    type Renderer = HelloProgressRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ProgressView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::IntrospectValue;
    use pinion_core::scene::ExternalNode;

    /// Build a fresh progress scene at a given fraction — the shape
    /// `WidgetCore::create_external` wraps the `ProgressBarExternal`
    /// into at shell startup, so `read_state` walks the same topology
    /// the live binary observes.
    fn scene_at(value: f32) -> Scene {
        let ext = ProgressBarExternal::with_value(value);
        Scene::External(ExternalNode::new(Box::new(ext)).with_tag(TAG))
    }

    #[test]
    fn read_state_round_trips_the_value() {
        let scene = scene_at(0.6);
        let (value, indeterminate) = ProgressView::read_state(&scene);
        assert!((value - 0.6).abs() < 1e-5);
        assert!(!indeterminate);
    }

    #[test]
    fn read_state_reflects_intervene() {
        let mut scene = scene_at(0.2);
        if let Scene::External(node) = &mut scene {
            let intro = node.handle.introspect_mut().expect("opted in");
            intro
                .intervene("value", IntrospectValue::Float(0.85))
                .expect("value is writable");
        }
        assert!((ProgressView::read_state(&scene).0 - 0.85).abs() < 1e-5);
    }

    #[test]
    fn read_state_reflects_indeterminate_intervene() {
        let mut scene = scene_at(0.2);
        if let Scene::External(node) = &mut scene {
            let intro = node.handle.introspect_mut().expect("opted in");
            intro
                .intervene("indeterminate", IntrospectValue::Bool(true))
                .expect("indeterminate is writable");
        }
        assert!(
            ProgressView::read_state(&scene).1,
            "indeterminate flag round-trips"
        );
    }

    #[test]
    fn read_state_defaults_to_zero_without_external() {
        // A non-External scene (a bare paint container) yields 0.0 — the
        // `read_state` walk only resolves a root `Scene::External`.
        let (value, indeterminate) =
            ProgressView::read_state(&Scene::Container(ContainerNode::new(vec![])));
        assert!((value - 0.0).abs() < 1e-5);
        assert!(!indeterminate);
    }
}

#[cfg(test)]
mod a11y_tests {
    use super::*;

    #[test]
    fn emits_progressbar_role_with_label() {
        let nodes = ProgressView::access_node(&(0.4, false), None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::ProgressBar);
        assert_eq!(nodes[0].name.as_deref(), Some(LABEL));
    }

    #[test]
    fn float_value_carries_normalized_range() {
        let nodes = ProgressView::access_node(&(0.75, false), None);
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
    fn r726_indeterminate_omits_valuenow() {
        // WAI-ARIA: an indeterminate bar carries no aria-valuenow.
        let nodes = ProgressView::access_node(&(0.4, true), None);
        assert_eq!(nodes[0].role, AriaRole::ProgressBar);
        assert!(
            nodes[0].value.is_none(),
            "indeterminate omits the Float value"
        );
    }

    #[test]
    fn progress_bar_is_not_focusable() {
        // Descriptive widget: no focusable tags (default), so Tab never
        // lands on it. §5.39: collected from the paint scene.
        let scene = pinion_core::Owner::new().run(|| view((BOOT_VALUE, false), &Frame::new()));
        assert!(scene.collect_focusable_tags().is_empty());
    }

    #[test]
    fn r55_g20_view_contains_paint_root_tag() {
        // The paint scene must carry `WidgetCore::tag()` so AI-side
        // `{path: "progress"}` routing + `rect_for_tag` AT bounds
        // attach resolve.
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ProgressView>(
            (BOOT_VALUE, false),
            &Frame::new(),
        );
    }
}
