//! `hello-scrubber` — R1415 §5.28 §5.38 §5.16 §5.7: a **scrub / seek bar** over
//! the R1414 [`TransportClock`] substrate. A draggable seek bar places the
//! playhead anywhere, and a play/pause toggle exercises jump-and-continue.
//!
//! ## What this demonstrates — the substrate's missing verb
//!
//! R1414 lifted the play/pause/stop transport clock into
//! [`pinion_core::widgets::transport`]. Its doc-comment named "an animation
//! preview scrubber" as an intended consumer — but the substrate had **no way
//! to scrub**: [`play`](TransportClock::play) / [`pause`](TransportClock::pause)
//! / [`stop`](TransportClock::stop) / `tick` only ever zero the playhead or
//! advance it monotonically from wall-clock. A seek bar needs to *place* the
//! playhead at an arbitrary point. This binding is that gap's forcing consumer:
//! it drives the new [`TransportClock::seek`], the scrub verb that completes the
//! substrate.
//!
//! ## The bridge: a Slider drag writes the clock
//!
//! `hello-timeline` (R1389) proved the §5.38 [`SliderExternal`] reused as a 1-D
//! playhead capture — RPC-drivable (`scene/intervene`), introspectable, no new
//! external invented. There the slider value *was* the playhead. Here the
//! **clock owns the playhead** (the position SSOT), and the scrub Slider is a
//! transparent capture whose drag *seeks* the clock: every `value_changing`
//! intent (drag / keyboard / `scene/intervene`) maps straight onto
//! [`TransportClock::seek`]. The bar's fill and knob are painted from
//! [`TransportClock::position`], so while playing they follow the clock, and a
//! drag jumps them — the two substrates (the R1389 slider drag machinery and
//! the R1414 clock) bridged at the reducer.
//!
//! ## Why a play/pause toggle, not the three-button deck
//!
//! `hello-transport` / `hello-replay` share a play/pause/**stop** three-button
//! deck; a *scrubber* wants a compact play/pause toggle beside the bar (the
//! video-preview idiom), which is a different affordance — so this binding does
//! not replicate that deck's glue (it stays a deferred two-consumer pattern).
//! The toggle is enough to show the one seek semantic a static bar cannot:
//! **seeking while playing keeps playing from the new spot** (jump-and-continue).
//!
//! ## Verification
//!
//! `tools/demos/r1415_scrubber.py`: boot (Stopped, 0 %) -> `scene/intervene` the
//! scrub value jumps the fill / knob / readout and Pauses a stopped clock ->
//! Play advances under `scene/tick` -> a seek while playing jumps yet stays
//! Playing -> seek to the end then Play rewinds. The playhead geometry (fill
//! width, knob x) is read structurally from the snapshot; exact values only in
//! at-rest states (the r726 discipline a time-driven tool needs).

use pinion_a11y::{AccessNode, AccessState, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, TextNode, capture_surface};
use pinion_core::style::{BoxStyle, LayoutStyle, Size, TextStyle};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::slider::{SliderExternal, SliderState};
use pinion_core::widgets::transport::{TransportClock, use_transport_clock};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, button_scene, read_button_state,
};
use pinion_widget_paint::slider::read_slider_state;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloScrubberRenderer, HelloScrubberRendererError);

const WIN_W: u32 = 760;
const WIN_H: u32 = 280;
const THEME_TAG: &str = "app";

/// The scrub Slider is the primary external — a transparent 1-D capture over
/// the bar (the R1389 idiom). Its `value_changing` reaches the reducer as
/// `scrubber_scrub.value_changing`, the wire tag the `update` arm matches.
const SCRUB_TAG: &str = "scrubber_scrub";
/// The play/pause toggle button (a secondary external).
const TOGGLE_TAG: &str = "scrubber_toggle";
const TOGGLE_HOVER_KEY: &str = "scrubber_toggle_hover";
const STATUS_TAG: &str = "scrubber_status";

/// `register_animation_once` key for the §5.28 [`TransportClock`].
const CLOCK_KEY: &str = "scrubber.clock";
/// Wall-clock seconds for a full `0 -> 1` playthrough.
const DURATION_SECS: f32 = 10.0;
/// Keyboard scrub increment (`ArrowLeft` / `ArrowRight`), in playhead fraction.
const SEEK_STEP: f32 = 0.05;

// ── the seek-bar geometry (window-absolute) ──────────────────────────────────
const TRACK_X: u32 = 24;
const TRACK_Y: u32 = 120;
const TRACK_W: u32 = WIN_W - 48;
const TRACK_H: u32 = 14;
const KNOB_W: u32 = 14;
const KNOB_H: u32 = TRACK_H + 16;

/// The filled-portion width for a playhead fraction — the bar the eye reads as
/// "how far in". Rounds to a pixel; a paint proxy the demo reads structurally.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "fraction in [0,1] * a small track width is far below 2^24; the \
              round-then-cast is exact and non-negative"
)]
fn fill_w(pos: f32) -> u32 {
    (pos.clamp(0.0, 1.0) * TRACK_W as f32).round() as u32
}

/// The playhead x (window-absolute) for a fraction — the knob centre.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "TRACK_X + fraction*TRACK_W stays a small positive pixel value"
)]
fn playhead_x(pos: f32) -> u32 {
    (TRACK_X as f32 + pos.clamp(0.0, 1.0) * TRACK_W as f32).round() as u32
}

/// The status readout — the single SSOT for the visible line and the
/// `role=status` live-region name: the transport state and the playhead
/// percent (so a screen reader hears where the scrub landed).
fn status_line(clock: &TransportClock) -> String {
    let pos = clock.position();
    format!(
        "{}  |  {:.0}%  |  drag the bar or Arrow keys to seek",
        clock.status().as_str(),
        pos * 100.0
    )
}

/// The play/pause toggle button. Its label flips with the clock state (Play
/// when idle/paused, Pause when playing) — the video-preview toggle idiom.
fn toggle_button(label: &str, posture: ButtonState, theme: &Theme) -> Scene {
    button_scene(
        label,
        posture,
        TOGGLE_HOVER_KEY,
        &ButtonColors::filled_tonal(theme),
        &ButtonStyle::m3_default(TOGGLE_TAG)
            .with_size(Size::px(120, 40))
            .with_label_font_size_px(14),
    )
}

/// Cached interaction state read from the scene each frame: the scrub Slider's
/// interaction posture (for the a11y node), the toggle button's posture, and
/// whether the toggle is focused (for its focus ring). The playhead itself
/// lives in the animation-driven [`TransportClock`] the view reads directly.
type ScrubState = (SliderState, ButtonState);

/// view-fn (§6.3): pure sync `ScrubState -> Scene`, reading the animation-driven
/// clock for the playhead + status. No side-effects — the clock only *advances*
/// under the shell's `tick_animations`, and is *seeked* by the reducer.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the WidgetCore::view trait hands the frame by reference"
)]
fn view(state: ScrubState, _frame: &Frame) -> Scene {
    let (scrub_state, toggle_posture) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let accent = theme.resolve(ColorRole::Accent);

    let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
    let pos = clock.position();

    let title = Scene::Text(
        TextNode::styled(
            "Scrubber — drag the seek bar to place the playhead; Play resumes from there",
            Rect::default(),
            TextStyle::new().with_size_px(16).with_fg(on_surface),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(24, 20)),
    );

    // The bar: an inactive track, an accent fill up to the playhead, and a knob
    // at it — all painted from `clock.position()` (the SSOT), so they follow the
    // clock while playing and jump on a seek.
    let track = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHighest)),
        )
        .with_tag("scrubber.track")
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(TRACK_X, TRACK_Y)
                .with_size(Size::px(TRACK_W, TRACK_H)),
        ),
    );
    let fill = Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(accent))
            .with_tag("scrubber.fill")
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(TRACK_X, TRACK_Y)
                    .with_size(Size::px(fill_w(pos), TRACK_H)),
            ),
    );
    let knob = Scene::Box(
        BoxNode::new(Rect::default(), BoxStyle::filled(accent))
            .with_tag("scrubber.playhead")
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(
                        playhead_x(pos).saturating_sub(KNOB_W / 2),
                        TRACK_Y.saturating_sub((KNOB_H - TRACK_H) / 2),
                    )
                    .with_size(Size::px(KNOB_W, KNOB_H)),
            ),
    );

    // Transparent capture surface over the bar — the `scrubber_scrub` primary
    // Slider tag. Focusable so Tab reaches it and Arrow keys scrub; on top so a
    // press anywhere on the bar drives a seek. The hit area is the track
    // inflated by 12px vertically so a slightly-off press still grabs. R1417
    // capture_surface lift.
    let scrub_surface = capture_surface(
        SCRUB_TAG,
        Rect::new(TRACK_X, TRACK_Y.saturating_sub(12), TRACK_W, TRACK_H + 24),
        true,
    );

    let toggle_label = if clock.is_playing() { "Pause" } else { "Play" };
    let toggle = Scene::Container(
        ContainerNode::new(vec![toggle_button(toggle_label, toggle_posture, &theme)])
            .with_layout(LayoutStyle::new().with_absolute_position(24, 168)),
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
        .with_layout(LayoutStyle::new().with_absolute_position(24, WIN_H - 28)),
    );

    // `scrub_state` colours nothing (the bar paint is clock-driven); it feeds
    // the a11y interaction posture only. Kept in the view signature so the read
    // path stays uniform with the other bindings.
    let _ = scrub_state;

    Scene::Container(
        ContainerNode::new(vec![
            track,
            fill,
            knob,
            scrub_surface,
            title,
            toggle,
            status,
        ])
        .with_style(BoxStyle::filled(surface))
        .with_layout(LayoutStyle::new().with_size(Size::px(WIN_W, WIN_H))),
    )
}

struct ScrubberView;

impl WidgetCore for ScrubberView {
    type State = ScrubState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        // The primary external is the scrub Slider (the transparent bar capture).
        Box::new(SliderExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Register the clock at boot (the window owner) so it is ticking before
        // the first paint; idempotent with the view's own `use_transport_clock`.
        let _clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
        vec![ExtraExternal::new(
            TOGGLE_TAG,
            Box::new(ButtonExternal::new()),
        )]
    }

    fn tag() -> &'static str {
        SCRUB_TAG
    }

    fn read_state(scene: &Scene) -> ScrubState {
        let scrub =
            read_slider_state(scene, SCRUB_TAG).map_or(SliderState::Idle, |(state, _value)| state);
        (scrub, read_button_state(scene, TOGGLE_TAG))
    }

    fn view(state: ScrubState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-scrubber (R1415 §5.28 scrub / seek over TransportClock)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// Keyboard: Space/Enter activates the toggle button (via ARIA), and Arrow
    /// keys scrub the clock **relative to its live position** (not the Slider's
    /// stale value), so a keyboard seek after playback moves from where the
    /// playhead actually is. `apply_key` runs under the root Owner scope, so
    /// `use_transport_clock` resolves the same registered clock the view drives.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if apply_aria_activate(scene, focused, key, TOGGLE_TAG) {
            return true;
        }
        if focused == Some(SCRUB_TAG) {
            let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
            let pos = clock.position();
            let next = match key {
                "ArrowLeft" | "ArrowDown" => Some((pos - SEEK_STEP).clamp(0.0, 1.0)),
                "ArrowRight" | "ArrowUp" => Some((pos + SEEK_STEP).clamp(0.0, 1.0)),
                "Home" => Some(0.0),
                "End" => Some(1.0),
                _ => None,
            };
            if let Some(target) = next {
                clock.seek(target);
                return true;
            }
        }
        false
    }

    /// The reducer bridges the two substrates: a scrub `value_changing` (from a
    /// drag, `scene/intervene`, or keyboard) seeks the clock; the toggle click
    /// flips play/pause. The reducer shares the owner, so `use_transport_clock`
    /// resolves the same registered instance the view drives.
    fn update(
        _state: ScrubState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
        match intent.tag_str() {
            // Both the live-preview and drag-end channels seek — a scrub has no
            // separate "commit", the playhead is wherever the drag left it.
            "scrubber_scrub.value_changing" | "scrubber_scrub.value_committed" => {
                if let pinion_core::external::IntrospectValue::Float(v) = intent.payload {
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "the wire float is a normalised [0,1] fraction"
                    )]
                    clock.seek(v as f32);
                }
            }
            "scrubber_toggle.click" => {
                if clock.is_playing() {
                    clock.pause();
                } else {
                    clock.play();
                }
            }
            _ => {}
        }
        Vec::new()
    }
}

impl WidgetA11y for ScrubberView {
    /// A `role=status` live region (state + playhead), the scrub bar as a
    /// `role=slider` whose value is the **clock** playhead (the SSOT, not the
    /// Slider's stale internal value), and the play/pause toggle `button`. The
    /// bar geometry itself is read as data via `scene/snapshot`.
    fn access_node(state: &ScrubState, focused: Option<&str>) -> Vec<AccessNode> {
        let (scrub_state, toggle_posture) = *state;
        let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
        let status = status_line(&clock);
        vec![
            AccessNode::new(STATUS_TAG, AriaRole::Status)
                .with_name(status.clone())
                .with_value(AccessValue::Text(status)),
            AccessNode::new(SCRUB_TAG, AriaRole::Slider)
                .with_value(AccessValue::Float {
                    value: clock.position(),
                    min: 0.0,
                    max: 1.0,
                })
                .with_state(AccessState {
                    focused: focused == Some(SCRUB_TAG),
                    ..AccessState::from_interaction(scrub_state, None)
                }),
            AccessNode::new(TOGGLE_TAG, AriaRole::Button).with_state(button_a11y_state(
                toggle_posture,
                focused == Some(TOGGLE_TAG),
            )),
        ]
    }
}

impl WidgetView for ScrubberView {
    type Renderer = HelloScrubberRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ScrubberView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;
    use pinion_core::widgets::transport::TransportStatus;

    fn idle() -> ScrubState {
        (SliderState::Idle, ButtonState::Idle)
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

    fn box_w(scene: &Scene, tag: &str) -> u32 {
        match find(scene, tag) {
            Some(Scene::Box(b)) => match b.layout.size.width {
                pinion_core::style::SizeValue::Px(w) => w,
                other => panic!("{tag} has a pixel width, got {other:?}"),
            },
            _ => panic!("{tag} is a box with an absolute size"),
        }
    }

    fn rendered(owner: &Owner) -> Scene {
        owner.run(|| view(idle(), &Frame::new()))
    }

    // The clock + seek mechanics are the substrate's own tests in
    // `pinion_core::widgets::transport`; these cover the binding's integration:
    // the bar structure, the seek reducer wiring, keyboard scrub, and a11y.

    #[test]
    fn boot_is_stopped_at_zero_with_an_empty_fill() {
        let owner = Owner::new();
        let scene = rendered(&owner);
        assert!(find(&scene, "scrubber.track").is_some(), "the bar track");
        assert!(find(&scene, SCRUB_TAG).is_some(), "the scrub capture");
        assert!(find(&scene, TOGGLE_TAG).is_some(), "the play/pause toggle");
        let status = text_under(&scene, STATUS_TAG).expect("status text");
        assert!(status.starts_with("Stopped"), "boot is Stopped: {status}");
        assert!(status.contains("0%"), "boot playhead at 0%: {status}");
        assert_eq!(box_w(&scene, "scrubber.fill"), 0, "boot fill is empty");
    }

    #[test]
    fn seeking_fills_the_bar_and_pauses_a_stopped_clock() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
            // Simulate the reducer's seek path (what a drag / intervene drives).
            let _ = ScrubberView::update(
                idle(),
                &pinion_core::Intent::new_owned(
                    "scrubber_scrub.value_changing".to_owned(),
                    pinion_core::external::IntrospectValue::Float(0.5),
                ),
            );
            assert_eq!(
                clock.status(),
                TransportStatus::Paused,
                "seek pauses a stop"
            );
            assert!((clock.position() - 0.5).abs() < 1e-4, "seeked to 0.5");
            let scene = view(idle(), &Frame::new());
            let full = box_w(&scene, "scrubber.fill");
            assert!(full > 0, "a 50% seek fills the bar: {full}");
            assert!(
                text_under(&scene, STATUS_TAG).unwrap().contains("50%"),
                "the readout names the sought percent"
            );
        });
    }

    #[test]
    fn toggle_click_plays_then_pauses() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
            let toggle_intent = pinion_core::Intent::new_owned(
                "scrubber_toggle.click".to_owned(),
                pinion_core::external::IntrospectValue::Null,
            );
            let _ = ScrubberView::update(idle(), &toggle_intent);
            assert_eq!(clock.status(), TransportStatus::Playing, "1st click plays");
            let _ = ScrubberView::update(idle(), &toggle_intent);
            assert_eq!(clock.status(), TransportStatus::Paused, "2nd click pauses");
        });
    }

    #[test]
    fn seek_while_playing_keeps_playing() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
            clock.play();
            let _ = ScrubberView::update(
                idle(),
                &pinion_core::Intent::new_owned(
                    "scrubber_scrub.value_changing".to_owned(),
                    pinion_core::external::IntrospectValue::Float(0.9),
                ),
            );
            assert_eq!(
                clock.status(),
                TransportStatus::Playing,
                "a seek mid-play stays playing (jump-and-continue)"
            );
            assert!((clock.position() - 0.9).abs() < 1e-4, "jumped to 0.9");
        });
    }

    #[test]
    fn keyboard_arrow_scrubs_relative_to_the_live_position() {
        let owner = Owner::new();
        owner.run(|| {
            let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
            clock.seek(0.4);
            let mut scene = view(idle(), &Frame::new());
            let handled = ScrubberView::apply_key(
                &mut scene,
                Some(SCRUB_TAG),
                "ArrowRight",
                pinion_core::Modifiers::empty(),
            );
            assert!(handled, "ArrowRight on the focused scrub is handled");
            assert!(
                (clock.position() - (0.4 + SEEK_STEP)).abs() < 1e-4,
                "arrow scrubs one step from the live 0.4: {}",
                clock.position()
            );
        });
    }

    #[test]
    fn scrub_is_focusable_and_a11y_reports_the_clock_playhead() {
        let owner = Owner::new();
        let scene = rendered(&owner);
        assert!(
            scene
                .collect_focusable_tags()
                .contains(&SCRUB_TAG.to_owned()),
            "the scrub bar is focusable"
        );
        let nodes = owner.run(|| {
            let clock = use_transport_clock(CLOCK_KEY, DURATION_SECS);
            clock.seek(0.7);
            <ScrubberView as WidgetA11y>::access_node(&idle(), Some(SCRUB_TAG))
        });
        let slider = nodes
            .iter()
            .find(|n| n.tag == SCRUB_TAG && n.role == AriaRole::Slider)
            .expect("the scrub slider node");
        match &slider.value {
            Some(AccessValue::Float { value, .. }) => {
                assert!(
                    (value - 0.7).abs() < 1e-4,
                    "a11y value is the clock playhead"
                );
            }
            other => panic!("expected a Float slider value, got {other:?}"),
        }
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == STATUS_TAG && n.role == AriaRole::Status),
            "status live region"
        );
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == TOGGLE_TAG && n.role == AriaRole::Button),
            "toggle button node"
        );
    }

    #[test]
    fn view_carries_the_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<ScrubberView>(
            idle(),
            &Frame::new(),
        );
    }

    #[test]
    fn r1360_2_view_paints_an_opaque_root() {
        pinion_core::test_fixtures::assert_widget_view_paints_opaque_root::<ScrubberView>(
            idle(),
            &Frame::new(),
        );
    }
}
