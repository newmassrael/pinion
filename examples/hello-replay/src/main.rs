//! `hello-replay` — R1414 §5.16 §5.28 §5.7: a **capture-replay** tool. A fixed
//! recorded signal played back under the SAME transport (play / pause / stop) as
//! `hello-transport`, but driving a **different visualization** — a
//! `pinion-chart` [`LineChart`] that **progressively reveals** the recording up
//! to the playhead, like scrubbing a video.
//!
//! ## Why this exists (the 2nd consumer that forced the lift)
//!
//! `hello-transport` (R1413) was the 1st consumer of a play/pause/stop clock,
//! and it kept that clock example-local (the abstraction-needs-a-second-consumer
//! discipline). This binding is the 2nd consumer — the "capture-replay tool" the
//! dashboard-substrate audit named — so R1414 lifted the clock into a real
//! substrate: [`pinion_core::widgets::transport`]. The ONLY thing the two
//! bindings share is that [`TransportClock`]; everything else diverges (a
//! timeline playhead line vs. a growing chart reveal), which is exactly the
//! signal that the clock, and only the clock, was the right thing to lift.
//!
//! ## The replay
//!
//! A deterministic recorded signal (`recorded`, `N_SAMPLES` points over
//! `0..RECORDED_SPAN`) is fixed data. The transport's `0.0..=1.0` playhead maps
//! to a reveal cursor: at fraction `f`, the chart shows the first
//! `round(f * N)` samples over a **fixed** x/y frame, so the line grows
//! left-to-right into a stable axis as playback advances (Play), freezes on
//! Pause, and empties on Stop. The chart auto-fits nothing — a replay wants a
//! stable frame the recording plays back into.
//!
//! ## Verification
//!
//! `tools/demos/r1414_replay.py`: boot (Stopped, 0 revealed) -> Play ->
//! `scene/tick` reveals more samples (the count climbs, the polyline lengthens)
//! -> Pause -> `scene/tick` leaves the count **frozen** -> Play resumes -> Stop
//! empties to 0 -> a tick past the end reveals all `N`. Deterministic via
//! `scene/tick`; exact counts asserted only in at-rest states (the r726
//! discipline the transport substrate documents).

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, DataPoint, LineChart, Series};
use pinion_core::external::External;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::transport::{TransportClock, use_transport_clock};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, button_scene, read_button_focused,
    read_button_state,
};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloReplayRenderer, HelloReplayRendererError);

const WIN_W: u32 = 760;
const WIN_H: u32 = 460;
const THEME_TAG: &str = "app";

const PLAY_TAG: &str = "replay_play";
const PAUSE_TAG: &str = "replay_pause";
const STOP_TAG: &str = "replay_stop";
const PLAY_HOVER_KEY: &str = "replay_play_hover";
const PAUSE_HOVER_KEY: &str = "replay_pause_hover";
const STOP_HOVER_KEY: &str = "replay_stop_hover";
/// The tag order the state tuple + a11y follow: `[play, pause, stop]`.
const BUTTON_TAGS: [&str; 3] = [PLAY_TAG, PAUSE_TAG, STOP_TAG];

const STATUS_TAG: &str = "replay_status";

/// `register_animation_once` key for the §5.28 [`TransportClock`].
const CLOCK_KEY: &str = "replay.clock";

/// Window-absolute plot region (the `pinion-chart` `build` coordinate contract).
const CHART_RECT: Rect = Rect::new(14, 46, WIN_W - 28, WIN_H - 116);

// ── the recorded signal (the fixed data the transport replays) ──
const RECORDED_SPAN: f64 = 10.0;
const N_SAMPLES: usize = 60;
const Y_MAX: f64 = 110.0;
const SIGNAL_BASE: f64 = 50.0;
const RIPPLE_AMP: f64 = 22.0;
const RIPPLE_FREQ: f64 = 0.9;
const BUMP_AMP: f64 = 26.0;
const BUMP_CENTER: f64 = 6.0;
const BUMP_WIDTH: f64 = 1.4;

/// Playback runs at 1x: a full sweep takes `RECORDED_SPAN` wall-seconds.
#[allow(
    clippy::cast_possible_truncation,
    reason = "RECORDED_SPAN is a small exact f64"
)]
const DURATION_SECS: f32 = RECORDED_SPAN as f32;

/// The `i`-th recorded sample — a deterministic waveform (a ripple plus a
/// gaussian bump near `t=6`) so the replay is fully reproducible (ZERO-FLAKE).
/// `x` is `(i+1)` steps in so the first sample sits just past `t=0` and a
/// zero-fraction cursor reveals nothing.
#[allow(
    clippy::cast_precision_loss,
    reason = "the sample index / count is a small display count, exact in f64"
)]
fn recorded_sample(i: usize) -> DataPoint {
    let x = (i + 1) as f64 * (RECORDED_SPAN / N_SAMPLES as f64);
    let bump = BUMP_AMP * (-((x - BUMP_CENTER).powi(2)) / BUMP_WIDTH).exp();
    let y = SIGNAL_BASE + RIPPLE_AMP * (x * RIPPLE_FREQ).sin() + bump;
    DataPoint::new(x, y)
}

/// The full recorded signal — the SSOT the reveal windows into.
fn recorded() -> Vec<DataPoint> {
    (0..N_SAMPLES).map(recorded_sample).collect()
}

/// How many samples the playhead fraction reveals: `round(fraction * N)`,
/// clamped to `N`. `0.0 -> 0`, `1.0 -> N`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "fraction is 0..=1 and N_SAMPLES is small, so the product is an exact small count"
)]
fn revealed_count(fraction: f32) -> usize {
    (fraction * N_SAMPLES as f32).round().min(N_SAMPLES as f32) as usize
}

/// The status readout — the SSOT for the visible line *and* the `role=status`
/// live-region name: the transport state, the playhead percent, how many of the
/// `N_SAMPLES` are revealed, and the latest revealed value.
fn status_line(clock: &TransportClock, revealed: &[DataPoint]) -> String {
    let latest = revealed.last().map_or_else(
        || "--".to_owned(),
        |p| {
            let v = p.y;
            format!("{v:.0}")
        },
    );
    format!(
        "{}  |  {:.0}%  |  revealed {}/{N_SAMPLES}  |  latest {latest}",
        clock.status().as_str(),
        clock.position() * 100.0,
        revealed.len(),
    )
}

/// Resolve the theme into a [`ChartStyle`] (the deliberate consumer-side
/// theme->style mapping; `pinion-chart` stays theme-independent).
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        crosshair: theme.resolve(ColorRole::OnSurface),
        tooltip_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
        tooltip_fg: theme.resolve(ColorRole::OnSurface),
        x_ticks: 6,
        y_ticks: 5,
        ..ChartStyle::default()
    }
}

/// One M3 transport button.
fn transport_button(
    tag: &'static str,
    label: &str,
    posture: ButtonState,
    focused: bool,
    hover_key: &'static str,
    theme: &Theme,
) -> Scene {
    button_scene(
        label,
        posture,
        focused,
        hover_key,
        &ButtonColors::filled_tonal(theme),
        &ButtonStyle::m3_default(tag)
            .with_size(Size::px(104, 36))
            .with_label_font_size_px(14),
    )
}

/// Cached posture + focus of the three transport buttons (`[play, pause,
/// stop]`). The playhead lives in the animation-driven [`TransportClock`].
type ReplayState = ([ButtonState; 3], [bool; 3]);

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`, reading the
/// animation-driven clock for the reveal cursor. No side-effects.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the WidgetCore::view trait hands the frame by reference"
)]
fn view(state: ReplayState, _frame: &Frame) -> Scene {
    let (postures, focused) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);

    let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
    let all = recorded();
    let revealed = &all[..revealed_count(clock.position())];

    let title = Scene::Text(
        TextNode::styled(
            "Capture-replay — Play to reveal the recording; Pause holds, Stop rewinds",
            Rect::default(),
            TextStyle::new().with_size_px(16).with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(14, 14)),
    );

    // A FIXED x/y frame the recording plays back into: the polyline grows into a
    // stable axis rather than the axis chasing the revealed subset.
    let chart = LineChart::new(vec![Series::new("signal", revealed.to_vec())])
        .filled(true)
        .with_x_domain(0.0, RECORDED_SPAN)
        .with_y_domain(0.0, Y_MAX)
        .build(CHART_RECT, &chart_style(&theme));

    let labels = [
        ("Play", PLAY_HOVER_KEY),
        ("Pause", PAUSE_HOVER_KEY),
        ("Stop", STOP_HOVER_KEY),
    ];
    let buttons: Vec<Scene> = BUTTON_TAGS
        .iter()
        .zip(labels)
        .enumerate()
        .map(|(i, (tag, (label, hover)))| {
            transport_button(tag, label, postures[i], focused[i], hover, &theme)
        })
        .collect();
    let button_row = Scene::Container(
        ContainerNode::new(buttons).with_layout(
            LayoutStyle::new()
                .with_absolute_position(14, WIN_H - 52)
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Start)
                .with_gap(10),
        ),
    );

    let status = Scene::Text(
        TextNode::styled(
            status_line(&clock, revealed),
            Rect::default(),
            TextStyle::new()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(STATUS_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(14, WIN_H - 20)),
    );

    Scene::Container(
        ContainerNode::new(vec![chart, title, button_row, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct ReplayView;

impl WidgetCore for ReplayView {
    type State = ReplayState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Register the clock at boot so it is ticking before the first paint.
        let _clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
        vec![
            ExtraExternal::new(PAUSE_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(STOP_TAG, Box::new(ButtonExternal::new())),
        ]
    }

    fn tag() -> &'static str {
        PLAY_TAG
    }

    fn read_state(scene: &Scene) -> ReplayState {
        (
            [
                read_button_state(scene, PLAY_TAG),
                read_button_state(scene, PAUSE_TAG),
                read_button_state(scene, STOP_TAG),
            ],
            [
                read_button_focused(scene, PLAY_TAG),
                read_button_focused(scene, PAUSE_TAG),
                read_button_focused(scene, STOP_TAG),
            ],
        )
    }

    fn view(state: ReplayState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-replay (R1414 §5.28 capture-replay transport)"
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
        BUTTON_TAGS
            .into_iter()
            .any(|tag| apply_aria_activate(scene, focused, key, tag))
    }

    /// A transport button click is an edge command mapped straight onto the
    /// shared [`TransportClock`] (the reducer resolves the same registered
    /// instance the view drives).
    fn update(
        _state: ReplayState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "replay_play.click" => use_transport_clock(CLOCK_KEY, DURATION_SECS).play(),
            "replay_pause.click" => use_transport_clock(CLOCK_KEY, DURATION_SECS).pause(),
            "replay_stop.click" => use_transport_clock(CLOCK_KEY, DURATION_SECS).stop(),
            _ => {}
        }
        Vec::new()
    }
}

impl WidgetA11y for ReplayView {
    /// A `role=status` live region announcing the transport state, reveal count,
    /// and latest value (so a screen reader hears the replay), plus the three
    /// transport `button`s. The chart geometry itself is read via `scene/snapshot`.
    fn access_node(state: &ReplayState, focused: Option<&str>) -> Vec<AccessNode> {
        let (postures, _focus) = state;
        let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
        let all = recorded();
        let revealed = &all[..revealed_count(clock.position())];
        let status = status_line(&clock, revealed);
        let mut nodes = vec![
            AccessNode::new(STATUS_TAG, AriaRole::Status)
                .with_name(status.clone())
                .with_value(AccessValue::Text(status)),
        ];
        for (i, tag) in BUTTON_TAGS.into_iter().enumerate() {
            nodes.push(
                AccessNode::new(tag, AriaRole::Button)
                    .with_state(button_a11y_state(postures[i], focused == Some(tag))),
            );
        }
        nodes
    }
}

impl WidgetView for ReplayView {
    type Renderer = HelloReplayRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ReplayView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;
    use pinion_core::widgets::transport::TransportStatus;

    fn idle() -> ReplayState {
        ([ButtonState::Idle; 3], [false; 3])
    }

    fn find<'a>(scene: &'a Scene, tag: &str) -> Option<&'a Scene> {
        match scene {
            Scene::Container(c) => {
                if c.tag.as_deref() == Some(tag) {
                    return Some(scene);
                }
                c.children.iter().find_map(|ch| find(ch, tag))
            }
            other => (other.tag() == Some(tag)).then_some(scene),
        }
    }

    fn text_under(scene: &Scene, tag: &str) -> Option<String> {
        match find(scene, tag)? {
            Scene::Text(t) => Some(t.content.clone()),
            _ => None,
        }
    }

    #[test]
    fn reveal_maps_the_fraction_to_a_sample_count() {
        assert_eq!(revealed_count(0.0), 0, "zero fraction reveals nothing");
        assert_eq!(revealed_count(1.0), N_SAMPLES, "full fraction reveals all");
        assert_eq!(revealed_count(0.5), N_SAMPLES / 2, "half reveals half");
        assert!(
            revealed_count(0.25) < revealed_count(0.75),
            "reveal is monotonic in the fraction"
        );
    }

    #[test]
    fn boot_is_stopped_with_nothing_revealed() {
        let owner = Owner::new();
        let scene = owner.run(|| view(idle(), &Frame::new()));
        assert!(find(&scene, "chart").is_some(), "the chart root");
        for tag in BUTTON_TAGS {
            assert!(find(&scene, tag).is_some(), "transport button {tag}");
        }
        let status = text_under(&scene, STATUS_TAG).expect("status text");
        assert!(status.starts_with("Stopped"), "boot is Stopped: {status}");
        assert!(
            status.contains("revealed 0/"),
            "boot reveals nothing: {status}"
        );
    }

    #[test]
    fn play_then_tick_reveals_more_samples() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
            clock.play();
            owner.tick_animations(DURATION_SECS / 2.0);
            let revealed = revealed_count(clock.position());
            assert!(
                (N_SAMPLES / 2).abs_diff(revealed) <= 2,
                "half the duration reveals about half: {revealed}"
            );
        });
    }

    #[test]
    fn stop_empties_the_reveal() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
            clock.play();
            owner.tick_animations(4.0);
            assert!(revealed_count(clock.position()) > 0);
            clock.stop();
            assert_eq!(clock.status(), TransportStatus::Stopped);
            assert_eq!(
                revealed_count(clock.position()),
                0,
                "stop empties the reveal"
            );
        });
    }

    #[test]
    fn playing_to_the_end_reveals_all_samples() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
            clock.play();
            owner.tick_animations(DURATION_SECS * 2.0);
            assert_eq!(clock.status(), TransportStatus::Paused, "ends parked");
            assert_eq!(
                revealed_count(clock.position()),
                N_SAMPLES,
                "the whole recording is revealed at the end"
            );
        });
    }

    #[test]
    fn buttons_are_focusable() {
        let scene = Owner::new().run(|| view(idle(), &Frame::new()));
        assert_eq!(
            scene.collect_focusable_tags(),
            vec![
                PLAY_TAG.to_owned(),
                PAUSE_TAG.to_owned(),
                STOP_TAG.to_owned()
            ]
        );
    }

    #[test]
    fn a11y_status_live_region_and_three_buttons() {
        let owner = Owner::new();
        let nodes = owner.run(|| <ReplayView as WidgetA11y>::access_node(&idle(), None));
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == STATUS_TAG && n.role == AriaRole::Status),
            "status live region"
        );
        for tag in BUTTON_TAGS {
            assert!(
                nodes
                    .iter()
                    .any(|n| n.tag == tag && n.role == AriaRole::Button),
                "button {tag} node"
            );
        }
    }

    #[test]
    fn reducer_drives_the_shared_transport_clock() {
        let owner = Owner::new();
        owner.run(|| {
            let intent = |tag: &str| {
                pinion_core::Intent::new_owned(
                    tag.to_owned(),
                    pinion_core::external::IntrospectValue::Null,
                )
            };
            let _ = ReplayView::update(idle(), &intent("replay_play.click"));
            assert_eq!(
                use_transport_clock(CLOCK_KEY, DURATION_SECS).status(),
                TransportStatus::Playing
            );
            let _ = ReplayView::update(idle(), &intent("replay_stop.click"));
            assert_eq!(
                use_transport_clock(CLOCK_KEY, DURATION_SECS).status(),
                TransportStatus::Stopped
            );
        });
    }

    #[test]
    fn view_carries_the_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ReplayView>(
            idle(),
            &Frame::new(),
        );
    }

    #[test]
    fn r1360_2_view_paints_an_opaque_root() {
        pinion_core::test_fixtures::assert_widget_view_paints_opaque_root::<ReplayView>(
            idle(),
            &Frame::new(),
        );
    }
}
