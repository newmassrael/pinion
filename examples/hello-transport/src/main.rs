//! `hello-transport` — R1413 §5.16 §5.28 §5.7: a **media transport**
//! (play / pause / stop) over a `pinion-chart` [`Timeline`], with a **live,
//! auto-advancing now-playhead** that the animation driver moves on its own —
//! not a hand-scrubbed one.
//!
//! ## What this demonstrates
//!
//! `hello-timeline` (R1389) proved the [`Timeline`] track view with a playhead a
//! user drags. The dashboard-substrate audit's gap #4 was the *other* half: a
//! transport — the playhead a sequencer / capture-replay tool *drives*, sweeping
//! left-to-right over wall-clock while "playing", freezing on "pause",
//! rewinding on "stop". This binding is that gap's forcing consumer, and it is
//! the [`Timeline`]'s 2nd consumer.
//!
//! ## The clock is an animation, not a thread
//!
//! The obvious shape for "advance a value over wall-clock" is a background timer
//! thread. pinion already has the *right* substrate, though: the §5.28 animation
//! driver. `TransportClock` is a [`Tickable`] — the same trait the theme fade,
//! the caret blink, and the [`IndeterminateSweep`](pinion_core::widgets::progress_bar::IndeterminateSweep)
//! implement — registered once through
//! [`Owner::register_animation_once`](pinion_core::reactive::Owner::register_animation_once).
//! Each frame the shell hands it the real elapsed `dt`; while `Playing` it
//! advances the `0.0..=1.0` playhead **linearly** (a transport plays at constant
//! speed — a spring would decelerate into the end, which is wrong), and its
//! [`Tickable::is_at_rest`] reports `true` unless playing, so the backend
//! requests frames only while the playhead is actually moving. No thread, no
//! poll, no `RepaintSink` poke — the existing frame loop is the clock.
//!
//! Because the driver is the §5.28 one, the R724 `scene/tick` RPC frame-steps it
//! **deterministically** (a known `dt` in, a known advance out) — so a live,
//! wall-clock-driven transport is nonetheless CI-testable without racing real
//! frames (the discipline `hello-progress`'s indeterminate sweep already needs).
//!
//! ## Architecture (view registers, reducer commands)
//!
//! The `hello-progress` owner-scoping: the clock is resolved-or-registered in
//! the pure `view` via `use_transport_clock` (idempotent — a re-run does not
//! re-register), so it lives on the window owner the shell ticks and its
//! `position` [`Signal`] auto-subscribes the paint. The three transport buttons
//! are M3 buttons; a click is an edge command the reducer maps straight onto
//! `TransportClock`'s `play` / `pause` / `stop` (the reducer shares the owner,
//! exactly as `hello-live-chart`'s reducer reaches its cached producer). Play
//! from the end rewinds first (replay); reaching the end parks the playhead at
//! `1.0` and stops the driver.
//!
//! ## Verification
//!
//! `tools/demos/r1413_transport.py`: boot (Stopped, 0 %) -> Play -> `scene/tick`
//! advances the playhead (its pixel x travels rightward, the readout names the
//! clip under it) -> Pause -> `scene/tick` leaves it **frozen** -> Play resumes
//! from the frozen spot -> Stop rewinds to 0 -> a tick past the end clamps at
//! 100 % and auto-stops. Every assertion is a structural invariant (status,
//! monotonic pixel advance, the frozen-while-paused equality), never a pinned
//! wall-clock value — the ZERO-FLAKE discipline a time-driven tool needs.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_chart::{ChartStyle, Lane, Span, Timeline};
use pinion_core::animation::Tickable;
use pinion_core::external::External;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, button_scene, read_button_focused,
    read_button_state,
};
use std::cell::Cell;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTransportRenderer, HelloTransportRendererError);

const WIN_W: u32 = 780;
const WIN_H: u32 = 460;
const THEME_TAG: &str = "app";

const PLAY_TAG: &str = "transport_play";
const PAUSE_TAG: &str = "transport_pause";
const STOP_TAG: &str = "transport_stop";
const PLAY_HOVER_KEY: &str = "transport_play_hover";
const PAUSE_HOVER_KEY: &str = "transport_pause_hover";
const STOP_HOVER_KEY: &str = "transport_stop_hover";
/// The tag order the state tuple + a11y follow: `[play, pause, stop]`.
const BUTTON_TAGS: [&str; 3] = [PLAY_TAG, PAUSE_TAG, STOP_TAG];

const STATUS_TAG: &str = "transport_status";

/// `Owner::register_animation_once` key for the transport clock.
const CLOCK_KEY: &str = "transport.clock";

/// Window-absolute plot region. The timeline is pinned to it (the
/// `pinion-chart` `build` coordinate contract), and the playhead readout is
/// resolved against the SAME rect so the visible callout and the a11y readout
/// cannot disagree (the `hello-timeline` parity).
const CHART_RECT: Rect = Rect::new(14, 52, WIN_W - 28, WIN_H - 120);

// ─────────────────────────── the transport clock ───────────────────────────

/// The three states of a media transport.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TransportStatus {
    /// Parked at the start (playhead `0.0`), not advancing.
    Stopped,
    /// Advancing the playhead with wall-clock time.
    Playing,
    /// Frozen at the current playhead, not advancing.
    Paused,
}

impl TransportStatus {
    /// The word shown in the status readout / heard by a screen reader.
    fn label(self) -> &'static str {
        match self {
            TransportStatus::Stopped => "Stopped",
            TransportStatus::Playing => "Playing",
            TransportStatus::Paused => "Paused",
        }
    }
}

/// A §5.28 [`Tickable`] driving a `0.0..=1.0` playhead fraction at constant
/// speed while playing — the [`IndeterminateSweep`](pinion_core::widgets::progress_bar::IndeterminateSweep)
/// shape (a `Signal` for the observed value, `Cell`s for the private state), but
/// one-shot and gated by a three-state status rather than a boolean, and *not*
/// looping: it stops at the end.
struct TransportClock {
    /// Playhead fraction in `0.0..=1.0` — auto-subscribes inside the view-fn so
    /// the paint follows the sweep.
    position: Signal<f32>,
    /// The transport state. A plain [`Cell`] (like the sweep's `active`): its
    /// changes ride the re-render an intent / a `position` write already forces,
    /// so it needs no reactivity of its own.
    status: Cell<TransportStatus>,
}

impl TransportClock {
    /// Wall-clock seconds for a full `0.0 -> 1.0` playthrough. Chosen to equal
    /// the timeline's time span (`0..12`) so the playhead reads as one
    /// timeline-second per wall-second.
    const DURATION_SECS: f32 = 12.0;

    /// A parked transport (playhead `0`, `Stopped`).
    fn new() -> Self {
        Self {
            position: Signal::new(0.0),
            status: Cell::new(TransportStatus::Stopped),
        }
    }

    /// Current playhead fraction. Auto-subscribes in a view-fn.
    fn position(&self) -> f32 {
        self.position.get()
    }

    /// Current transport state.
    fn status(&self) -> TransportStatus {
        self.status.get()
    }

    /// Start (or resume) playback. From the end, rewinds to the start first, so
    /// Play on a finished transport replays it.
    fn play(&self) {
        if self.position.get() >= 1.0 {
            self.position.set(0.0);
        }
        self.status.set(TransportStatus::Playing);
    }

    /// Freeze the playhead where it is. A no-op unless playing (Stop, not Pause,
    /// is what rewinds).
    fn pause(&self) {
        if self.status.get() == TransportStatus::Playing {
            self.status.set(TransportStatus::Paused);
        }
    }

    /// Rewind to the start and park.
    fn stop(&self) {
        self.status.set(TransportStatus::Stopped);
        self.position.set(0.0);
    }
}

impl Tickable for TransportClock {
    fn tick(&self, dt: f32) {
        if self.status.get() != TransportStatus::Playing {
            return;
        }
        let next = (self.position.get() + dt / Self::DURATION_SECS).min(1.0);
        self.position.set(next);
        if next >= 1.0 {
            // Reached the end: park the playhead there and stop advancing (so
            // `is_at_rest` frees the frame loop). Play rewinds from here.
            self.status.set(TransportStatus::Paused);
        }
    }

    fn is_at_rest(&self, _epsilon: f32) -> bool {
        // Only a playing transport needs more frames; a stopped / paused one is
        // settled, so the backend releases the frame loop.
        self.status.get() != TransportStatus::Playing
    }
}

/// Resolve (or, on the first call, construct **and register**) the transport
/// clock for the current window scope — the R727 §5.28
/// [`Owner::register_animation_once`](pinion_core::reactive::Owner::register_animation_once)
/// SSOT, exactly as [`use_indeterminate_sweep`](pinion_core::widgets::progress_bar::use_indeterminate_sweep).
///
/// # Panics
///
/// Panics outside an active [`Owner`] scope.
fn use_transport_clock() -> Rc<TransportClock> {
    Owner::current()
        .expect("use_transport_clock requires an active Owner scope")
        .register_animation_once(CLOCK_KEY, TransportClock::new)
}

// ───────────────────────────── the demo sequence ────────────────────────────

/// A three-track editor sequence over `0..12` (seconds), so the transport has
/// something legible to sweep: labelled clips per lane whose name the playhead
/// readout surfaces as it crosses them.
fn sequence_lanes() -> Vec<Lane> {
    vec![
        Lane::new(
            "video",
            vec![
                Span::new(0.0, 4.0, "intro"),
                Span::new(4.0, 9.0, "action"),
                Span::new(9.0, 12.0, "outro"),
            ],
        ),
        Lane::new(
            "audio",
            vec![
                Span::new(0.0, 6.0, "theme"),
                Span::new(6.0, 12.0, "ambient"),
            ],
        ),
        Lane::new(
            "fx",
            vec![
                Span::new(2.0, 3.5, "spark"),
                Span::new(7.0, 8.5, "boom"),
                Span::new(10.0, 11.5, "fade"),
            ],
        ),
    ]
}

/// The timeline over the demo sequence at the given playhead fraction.
fn timeline(playhead: f32) -> Timeline {
    Timeline::new(sequence_lanes()).playhead(Some(playhead))
}

/// The status readout — the single SSOT for the visible line *and* the
/// `role=status` live-region name: the transport state, the playhead percent,
/// and the clip(s) under the playhead (so a screen reader hears what is "now
/// playing"). Reuses [`Timeline::playhead_readout`] against [`CHART_RECT`], the
/// same geometry the painted callout uses.
fn status_line(clock: &TransportClock) -> String {
    let pos = clock.position();
    let readout = timeline(pos).playhead_readout(CHART_RECT, &ChartStyle::default());
    let now = readout.as_deref().unwrap_or("t = -");
    format!(
        "{}  |  {:.0}%  |  {now}",
        clock.status().label(),
        pos * 100.0
    )
}

/// Resolve the theme into a [`ChartStyle`] — the deliberate consumer-side
/// theme->style mapping (`pinion-chart` stays theme-independent). Only COLOURS
/// are overridden; the margins / tick targets stay the defaults, which is what
/// lets [`status_line`]'s default-style readout resolve the identical playhead
/// geometry the themed overlay paints.
fn chart_style(theme: &Theme) -> ChartStyle {
    ChartStyle {
        axis: theme.resolve(ColorRole::OnSurfaceMuted),
        grid: theme.resolve(ColorRole::Outline).with_alpha(0x40),
        label: theme.resolve(ColorRole::OnSurfaceMuted),
        background: Some(theme.resolve(ColorRole::SurfaceContainerLow)),
        crosshair: theme.resolve(ColorRole::OnSurface),
        tooltip_bg: theme.resolve(ColorRole::SurfaceContainerHighest),
        tooltip_fg: theme.resolve(ColorRole::OnSurface),
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
/// stop]`). The playhead itself lives in the animation-driven [`TransportClock`]
/// the owner-scoped view reads directly.
type TransportState = ([ButtonState; 3], [bool; 3]);

/// view-fn (§6.3): pure sync mapping `(button postures) -> Scene`, reading the
/// animation-driven clock for the playhead + status. No side-effects — the
/// clock only *advances* under the shell's `tick_animations`.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the WidgetCore::view trait hands the frame by reference"
)]
fn view(state: TransportState, _frame: &Frame) -> Scene {
    let (postures, focused) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);

    let clock = use_transport_clock();
    let playhead = clock.position();

    let title = Scene::Text(
        TextNode::styled(
            "Transport — Play to sweep the playhead; Pause freezes it, Stop rewinds",
            Rect::default(),
            TextStyle::new().with_size_px(16).with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(14, 16)),
    );

    let track = timeline(playhead).build(CHART_RECT, &chart_style(&theme));

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
                .with_absolute_position(14, WIN_H - 54)
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Start)
                .with_gap(10),
        ),
    );

    let status = Scene::Text(
        TextNode::styled(
            status_line(&clock),
            Rect::default(),
            TextStyle::new()
                .with_size_px(12)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        )
        .with_tag(STATUS_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(14, WIN_H - 20)),
    );

    Scene::Container(
        ContainerNode::new(vec![track, title, button_row, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct TransportView;

impl WidgetCore for TransportView {
    type State = TransportState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Register the clock at boot (the window owner) so it is ticking before
        // the first paint; idempotent with the view's own `use_transport_clock`.
        let _clock = use_transport_clock();
        vec![
            ExtraExternal::new(PAUSE_TAG, Box::new(ButtonExternal::new())),
            ExtraExternal::new(STOP_TAG, Box::new(ButtonExternal::new())),
        ]
    }

    fn tag() -> &'static str {
        PLAY_TAG
    }

    fn read_state(scene: &Scene) -> TransportState {
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

    fn view(state: TransportState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-transport (R1413 §5.28 timeline transport + live playhead)"
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
    /// clock (the reducer shares the owner, so `use_transport_clock` resolves
    /// the same registered instance the view drives).
    fn update(
        _state: TransportState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        match intent.tag_str() {
            "transport_play.click" => use_transport_clock().play(),
            "transport_pause.click" => use_transport_clock().pause(),
            "transport_stop.click" => use_transport_clock().stop(),
            _ => {}
        }
        Vec::new()
    }
}

impl WidgetA11y for TransportView {
    /// A `role=status` live region announcing the transport state + playhead +
    /// the clip under it (so a screen reader hears the sweep), plus the three
    /// transport `button`s. The timeline geometry itself is read as data via
    /// `scene/snapshot`.
    fn access_node(state: &TransportState, focused: Option<&str>) -> Vec<AccessNode> {
        let (postures, _focus) = state;
        let clock = use_transport_clock();
        let status = status_line(&clock);
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

impl WidgetView for TransportView {
    type Renderer = HelloTransportRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TransportView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> TransportState {
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

    /// Render the view once inside an owner scope seeded with the clock.
    fn rendered(owner: &Owner) -> Scene {
        owner.run(|| view(idle(), &Frame::new()))
    }

    #[test]
    fn boot_is_stopped_at_zero() {
        let owner = Owner::new();
        let scene = rendered(&owner);
        assert!(find(&scene, "timeline").is_some(), "the timeline root");
        for tag in BUTTON_TAGS {
            assert!(find(&scene, tag).is_some(), "transport button {tag}");
        }
        let status = text_under(&scene, STATUS_TAG).expect("status text");
        assert!(status.starts_with("Stopped"), "boot is Stopped: {status}");
        assert!(status.contains("0%"), "boot playhead at 0%: {status}");
    }

    #[test]
    fn play_then_tick_advances_the_playhead() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock();
            clock.play();
            assert_eq!(clock.status(), TransportStatus::Playing);
            assert!(!clock.is_at_rest(0.001), "a playing clock wants frames");
            // Half the duration advances the playhead to ~50 %.
            owner.tick_animations(TransportClock::DURATION_SECS / 2.0);
            let pos = clock.position();
            assert!((0.45..=0.55).contains(&pos), "half-duration ~50%: {pos}");
        });
    }

    #[test]
    fn pause_freezes_the_playhead_across_ticks() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock();
            clock.play();
            owner.tick_animations(3.0);
            let frozen = clock.position();
            clock.pause();
            assert_eq!(clock.status(), TransportStatus::Paused);
            assert!(clock.is_at_rest(0.001), "a paused clock releases frames");
            owner.tick_animations(3.0);
            assert!(
                (clock.position() - frozen).abs() < f32::EPSILON,
                "paused playhead does not advance: {frozen} -> {}",
                clock.position()
            );
        });
    }

    #[test]
    fn play_resumes_from_the_paused_spot() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock();
            clock.play();
            owner.tick_animations(3.0);
            clock.pause();
            let resumed_from = clock.position();
            clock.play();
            owner.tick_animations(3.0);
            assert!(
                clock.position() > resumed_from,
                "resume advances from {resumed_from}, got {}",
                clock.position()
            );
        });
    }

    #[test]
    fn stop_rewinds_to_zero() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock();
            clock.play();
            owner.tick_animations(5.0);
            assert!(clock.position() > 0.0);
            clock.stop();
            assert_eq!(clock.status(), TransportStatus::Stopped);
            assert!(
                (clock.position() - 0.0).abs() < f32::EPSILON,
                "stop rewinds to 0"
            );
        });
    }

    #[test]
    fn reaching_the_end_clamps_and_auto_stops() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock();
            clock.play();
            // A tick well past the full duration.
            owner.tick_animations(TransportClock::DURATION_SECS * 2.0);
            assert!(
                (clock.position() - 1.0).abs() < f32::EPSILON,
                "playhead clamps at 1.0"
            );
            assert_eq!(
                clock.status(),
                TransportStatus::Paused,
                "the finished transport parks at the end"
            );
            assert!(clock.is_at_rest(0.001), "a finished clock releases frames");
        });
    }

    #[test]
    fn play_from_the_end_rewinds_first() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock();
            clock.play();
            owner.tick_animations(TransportClock::DURATION_SECS * 2.0);
            assert!((clock.position() - 1.0).abs() < f32::EPSILON);
            // Play at the end replays from the start.
            clock.play();
            assert_eq!(clock.status(), TransportStatus::Playing);
            assert!(
                (clock.position() - 0.0).abs() < f32::EPSILON,
                "play at the end rewinds to 0"
            );
        });
    }

    #[test]
    fn status_names_the_clip_under_the_playhead() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock();
            clock.play();
            owner.tick_animations(5.0); // t ~ 5s -> video:action, audio:theme
            let status = status_line(&clock);
            assert!(status.starts_with("Playing"), "state: {status}");
            assert!(status.contains("action"), "names the active clip: {status}");
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
        let nodes = owner.run(|| <TransportView as WidgetA11y>::access_node(&idle(), None));
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
    fn reducer_play_pause_stop_drive_the_clock() {
        let owner = Owner::new();
        owner.run(|| {
            let play = pinion_core::Intent::new_owned(
                "transport_play.click".to_owned(),
                pinion_core::external::IntrospectValue::Null,
            );
            let _ = TransportView::update(idle(), &play);
            assert_eq!(use_transport_clock().status(), TransportStatus::Playing);

            let pause = pinion_core::Intent::new_owned(
                "transport_pause.click".to_owned(),
                pinion_core::external::IntrospectValue::Null,
            );
            let _ = TransportView::update(idle(), &pause);
            assert_eq!(use_transport_clock().status(), TransportStatus::Paused);

            let stop = pinion_core::Intent::new_owned(
                "transport_stop.click".to_owned(),
                pinion_core::external::IntrospectValue::Null,
            );
            let _ = TransportView::update(idle(), &stop);
            assert_eq!(use_transport_clock().status(), TransportStatus::Stopped);
        });
    }

    #[test]
    fn view_carries_the_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TransportView>(
            idle(),
            &Frame::new(),
        );
    }

    #[test]
    fn r1360_2_view_paints_an_opaque_root() {
        pinion_core::test_fixtures::assert_widget_view_paints_opaque_root::<TransportView>(
            idle(),
            &Frame::new(),
        );
    }
}
