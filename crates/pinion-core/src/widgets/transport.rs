//! R1414 §5.28 — a media-transport playback clock (play / pause / stop) driving
//! a `0.0..=1.0` playhead fraction at constant speed.
//!
//! A [`TransportClock`] is the [`Tickable`] sibling of
//! [`IndeterminateSweep`](super::progress_bar::IndeterminateSweep),
//! [`SnackbarTimer`](super::snackbar::SnackbarTimer), and
//! [`CaretBlink`](super::caret_blink::CaretBlink) — registered once with the
//! §5.28 animation driver through
//! [`Owner::register_animation_once`](crate::reactive::Owner::register_animation_once),
//! so the existing frame loop advances it. Unlike the sweep (a *looping*
//! sawtooth), the transport is a *one-shot* clock gated by a three-state status:
//! while [`Playing`](TransportStatus::Playing) it advances the playhead
//! **linearly** (a transport plays at constant speed — a spring or eased curve
//! would decelerate into the end, which is wrong for scrubbing / replay), and it
//! stops at the end. [`Tickable::is_at_rest`] is `true` unless playing, so the
//! backend requests frames only while the playhead actually moves.
//!
//! The clock is deliberately **domain-agnostic**: it owns only a `0.0..=1.0`
//! fraction and the wall-clock rate at which it sweeps. The consumer maps the
//! fraction onto its own axis — a timeline playhead
//! (`examples/hello-transport`), a progressive data reveal
//! (`examples/hello-replay`), an animation preview scrubber. Because the driver
//! is the §5.28 one, the R724 `scene/tick` RPC frame-steps it deterministically,
//! so a wall-clock transport is CI-testable without racing real frames.

use std::cell::Cell;
use std::rc::Rc;

use crate::animation::Tickable;
use crate::reactive::{Owner, Signal};

/// The three states of a media transport.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransportStatus {
    /// Parked at the start (playhead `0.0`), not advancing.
    Stopped,
    /// Advancing the playhead with wall-clock time.
    Playing,
    /// Frozen at the current playhead, not advancing.
    Paused,
}

impl TransportStatus {
    /// The canonical state name — the SSOT both a status readout and a
    /// screen-reader announcement can share.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TransportStatus::Stopped => "Stopped",
            TransportStatus::Playing => "Playing",
            TransportStatus::Paused => "Paused",
        }
    }

    /// Whether the transport is advancing the playhead.
    #[must_use]
    pub fn is_playing(self) -> bool {
        matches!(self, TransportStatus::Playing)
    }
}

/// R1414 §5.28 — a one-shot playback clock advancing a `0.0..=1.0` playhead
/// fraction at constant speed while playing.
///
/// The [`IndeterminateSweep`](super::progress_bar::IndeterminateSweep) shape (a
/// [`Signal`] for the observed value, a [`Cell`] for private state), but gated by
/// a three-state [`TransportStatus`] rather than a boolean and **not looping** —
/// it clamps and stops at `1.0`. The application reaches it through
/// [`use_transport_clock`] and reads [`Self::position`] each frame to place the
/// playhead / reveal cursor.
///
/// Thread-safety: not `Send` / `Sync` (uses `Rc` + `Cell` + [`Signal`]), matching
/// every other `pinion-core` reactive primitive. UI thread only.
#[derive(Debug)]
pub struct TransportClock {
    /// Playhead fraction in `0.0..=1.0` — auto-subscribes inside a view-fn so
    /// the paint follows the sweep.
    position: Signal<f32>,
    /// The transport state. A plain [`Cell`] (like the sweep's `active`): its
    /// changes ride the re-render an intent / a `position` write already forces,
    /// so it needs no reactivity of its own.
    status: Cell<TransportStatus>,
    /// Wall-clock seconds for a full `0.0 -> 1.0` playthrough. Clamped positive
    /// at construction so [`Tickable::tick`] never divides by zero.
    duration_secs: f32,
}

impl TransportClock {
    /// A parked transport (playhead `0`, [`Stopped`](TransportStatus::Stopped))
    /// that sweeps `0.0 -> 1.0` over `duration_secs` wall-clock seconds while
    /// playing. `duration_secs` is clamped to a tiny positive so a zero /
    /// negative duration degrades to "one tick completes it" rather than
    /// dividing by zero.
    #[must_use]
    pub fn new(duration_secs: f32) -> Self {
        Self {
            position: Signal::new(0.0),
            status: Cell::new(TransportStatus::Stopped),
            duration_secs: duration_secs.max(f32::MIN_POSITIVE),
        }
    }

    /// Current playhead fraction in `0.0..=1.0`. Auto-subscribes in a view-fn.
    #[must_use]
    pub fn position(&self) -> f32 {
        self.position.get()
    }

    /// Current transport state.
    #[must_use]
    pub fn status(&self) -> TransportStatus {
        self.status.get()
    }

    /// Whether the transport is currently advancing.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.status.get().is_playing()
    }

    /// The full-playthrough wall-clock duration (as clamped at construction).
    #[must_use]
    pub fn duration_secs(&self) -> f32 {
        self.duration_secs
    }

    /// Start (or resume) playback. From the end, rewinds to the start first, so
    /// Play on a finished transport replays it.
    pub fn play(&self) {
        if self.position.get() >= 1.0 {
            self.position.set(0.0);
        }
        self.status.set(TransportStatus::Playing);
    }

    /// Freeze the playhead where it is. A no-op unless playing ([`stop`](Self::stop),
    /// not pause, is what rewinds).
    pub fn pause(&self) {
        if self.status.get() == TransportStatus::Playing {
            self.status.set(TransportStatus::Paused);
        }
    }

    /// Rewind to the start and park.
    pub fn stop(&self) {
        self.status.set(TransportStatus::Stopped);
        self.position.set(0.0);
    }

    /// Jump the playhead to `fraction` (clamped to `0.0..=1.0`) **without**
    /// advancing wall-clock time — the scrub / seek this clock's doc-comment
    /// promises an "animation preview scrubber" needs, but which [`play`](Self::play) /
    /// [`pause`](Self::pause) / [`stop`](Self::stop) / [`tick`](Tickable::tick)
    /// cannot express (they only ever zero the playhead or advance it
    /// monotonically). A seek bar drives this to place the playhead anywhere.
    ///
    /// A seek never *stops* the transport ([`stop`](Self::stop) is the sole
    /// rewind-and-park):
    ///
    /// * while [`Playing`](TransportStatus::Playing) it stays playing and
    ///   resumes advancing from the new spot (jump-and-continue);
    /// * a [`Stopped`](TransportStatus::Stopped) or
    ///   [`Paused`](TransportStatus::Paused) transport is held
    ///   [`Paused`](TransportStatus::Paused) at the new spot — a moved playhead
    ///   is no longer "parked at the start", which is what `Stopped` means, so a
    ///   seek promotes it to the explicit `Paused`-at-`fraction` state.
    ///
    /// Seeking to `1.0` parks at the end exactly as reaching it under
    /// [`tick`](Tickable::tick) does, so a subsequent [`play`](Self::play)
    /// rewinds and replays — one consistent end-of-clip rule however the end is
    /// reached.
    pub fn seek(&self, fraction: f32) {
        self.position.set(fraction.clamp(0.0, 1.0));
        if self.status.get() != TransportStatus::Playing {
            self.status.set(TransportStatus::Paused);
        }
    }
}

impl Tickable for TransportClock {
    fn tick(&self, dt: f32) {
        if self.status.get() != TransportStatus::Playing {
            return;
        }
        let next = (self.position.get() + dt / self.duration_secs).min(1.0);
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

/// R1414 §5.28 — resolve (or, on the first call, construct **and register**) a
/// [`TransportClock`] for the current owner scope, keyed by `key` and sweeping
/// over `duration_secs`. The R727 §5.28
/// [`Owner::register_animation_once`](crate::reactive::Owner::register_animation_once)
/// SSOT, exactly as
/// [`use_indeterminate_sweep`](super::progress_bar::use_indeterminate_sweep).
///
/// `duration_secs` is read only on the first (constructing) call — a later call
/// with a different value resolves the already-registered clock unchanged.
///
/// # Panics
///
/// Panics outside an active [`Owner`] scope.
#[must_use]
pub fn use_transport_clock(key: &'static str, duration_secs: f32) -> Rc<TransportClock> {
    Owner::current()
        .expect("use_transport_clock requires an active Owner scope")
        .register_animation_once(key, move || TransportClock::new(duration_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DURATION: f32 = 12.0;

    fn clock() -> TransportClock {
        TransportClock::new(DURATION)
    }

    #[test]
    fn new_is_stopped_at_zero() {
        let c = clock();
        assert_eq!(c.status(), TransportStatus::Stopped);
        assert!((c.position() - 0.0).abs() < f32::EPSILON);
        assert!(c.is_at_rest(0.001), "a stopped clock releases frames");
        assert!(!c.is_playing());
    }

    #[test]
    fn play_then_tick_advances_linearly() {
        let c = clock();
        c.play();
        assert_eq!(c.status(), TransportStatus::Playing);
        assert!(!c.is_at_rest(0.001), "a playing clock wants frames");
        c.tick(DURATION / 2.0);
        assert!(
            (c.position() - 0.5).abs() < 1e-4,
            "half the duration is half the sweep: {}",
            c.position()
        );
    }

    #[test]
    fn pause_freezes_across_ticks() {
        let c = clock();
        c.play();
        c.tick(3.0);
        let frozen = c.position();
        c.pause();
        assert_eq!(c.status(), TransportStatus::Paused);
        assert!(c.is_at_rest(0.001), "a paused clock releases frames");
        c.tick(3.0);
        assert!(
            (c.position() - frozen).abs() < f32::EPSILON,
            "a paused playhead does not advance"
        );
    }

    #[test]
    fn play_resumes_from_the_paused_spot() {
        let c = clock();
        c.play();
        c.tick(3.0);
        c.pause();
        let from = c.position();
        c.play();
        c.tick(3.0);
        assert!(c.position() > from, "resume advances past {from}");
    }

    #[test]
    fn stop_rewinds_to_zero() {
        let c = clock();
        c.play();
        c.tick(5.0);
        assert!(c.position() > 0.0);
        c.stop();
        assert_eq!(c.status(), TransportStatus::Stopped);
        assert!(
            (c.position() - 0.0).abs() < f32::EPSILON,
            "stop rewinds to 0"
        );
    }

    #[test]
    fn reaching_the_end_clamps_and_auto_stops() {
        let c = clock();
        c.play();
        c.tick(DURATION * 2.0);
        assert!((c.position() - 1.0).abs() < f32::EPSILON, "clamps at 1.0");
        assert_eq!(
            c.status(),
            TransportStatus::Paused,
            "a finished transport parks at the end"
        );
        assert!(c.is_at_rest(0.001), "a finished clock releases frames");
    }

    #[test]
    fn play_from_the_end_rewinds_first() {
        let c = clock();
        c.play();
        c.tick(DURATION * 2.0);
        assert!((c.position() - 1.0).abs() < f32::EPSILON);
        c.play();
        assert_eq!(c.status(), TransportStatus::Playing);
        assert!(
            (c.position() - 0.0).abs() < f32::EPSILON,
            "play at the end rewinds to 0"
        );
    }

    #[test]
    fn a_paused_or_stopped_clock_ignores_ticks() {
        let c = clock();
        // Stopped: a tick is a no-op.
        c.tick(5.0);
        assert!(
            (c.position() - 0.0).abs() < f32::EPSILON,
            "stopped ignores tick"
        );
        // Paused mid-way: a tick is a no-op.
        c.play();
        c.tick(2.0);
        let p = c.position();
        c.pause();
        c.tick(5.0);
        assert!(
            (c.position() - p).abs() < f32::EPSILON,
            "paused ignores tick"
        );
    }

    #[test]
    fn zero_duration_completes_in_one_tick_without_dividing_by_zero() {
        let c = TransportClock::new(0.0);
        c.play();
        c.tick(0.016);
        assert!(
            (c.position() - 1.0).abs() < f32::EPSILON && c.status() == TransportStatus::Paused,
            "a zero-duration transport clamps to done, not NaN: {}",
            c.position()
        );
    }

    #[test]
    fn seek_jumps_the_playhead_and_pauses_a_stopped_clock() {
        let c = clock();
        assert_eq!(c.status(), TransportStatus::Stopped);
        c.seek(0.4);
        assert!(
            (c.position() - 0.4).abs() < f32::EPSILON,
            "seek sets the playhead directly: {}",
            c.position()
        );
        assert_eq!(
            c.status(),
            TransportStatus::Paused,
            "seeking a stopped transport holds it paused at the sought spot"
        );
        assert!(
            c.is_at_rest(0.001),
            "a sought-and-paused clock releases frames"
        );
    }

    #[test]
    fn seek_while_playing_jumps_and_continues() {
        let c = clock();
        c.play();
        c.tick(DURATION / 4.0); // ~0.25
        c.seek(0.8);
        assert_eq!(
            c.status(),
            TransportStatus::Playing,
            "seek keeps it playing"
        );
        assert!((c.position() - 0.8).abs() < 1e-4, "jumped to 0.8");
        c.tick(DURATION / 4.0); // advances from 0.8, not from 0.25
        assert!(
            c.position() > 0.8,
            "playback resumes from the sought spot: {}",
            c.position()
        );
    }

    #[test]
    fn seek_clamps_out_of_range() {
        let c = clock();
        c.seek(1.7);
        assert!(
            (c.position() - 1.0).abs() < f32::EPSILON,
            "clamps above 1.0"
        );
        c.seek(-0.3);
        assert!(
            (c.position() - 0.0).abs() < f32::EPSILON,
            "clamps below 0.0"
        );
    }

    #[test]
    fn seek_a_paused_clock_stays_paused_at_the_new_spot() {
        let c = clock();
        c.play();
        c.tick(2.0);
        c.pause();
        assert_eq!(c.status(), TransportStatus::Paused);
        c.seek(0.6);
        assert_eq!(
            c.status(),
            TransportStatus::Paused,
            "still paused after a seek"
        );
        assert!(
            (c.position() - 0.6).abs() < 1e-4,
            "moved to the sought spot"
        );
    }

    #[test]
    fn seek_to_the_end_then_play_rewinds() {
        let c = clock();
        c.seek(1.0);
        assert!(
            (c.position() - 1.0).abs() < f32::EPSILON,
            "parked at the end"
        );
        assert_eq!(c.status(), TransportStatus::Paused, "seek-to-end is paused");
        // The one end-of-clip rule: Play from the end (however reached) replays.
        c.play();
        assert_eq!(c.status(), TransportStatus::Playing);
        assert!(
            (c.position() - 0.0).abs() < f32::EPSILON,
            "play from a sought end rewinds to 0, exactly as reaching it via tick"
        );
    }

    #[test]
    fn as_str_names_each_state() {
        assert_eq!(TransportStatus::Stopped.as_str(), "Stopped");
        assert_eq!(TransportStatus::Playing.as_str(), "Playing");
        assert_eq!(TransportStatus::Paused.as_str(), "Paused");
        assert!(TransportStatus::Playing.is_playing());
        assert!(!TransportStatus::Paused.is_playing());
    }

    #[test]
    fn use_transport_clock_registers_once_and_is_driven_by_tick_animations() {
        let owner = Owner::new();
        owner.run(|| {
            let c = use_transport_clock("test.transport", DURATION);
            c.play();
            // A second resolve returns the SAME registered instance.
            let again = use_transport_clock("test.transport", DURATION);
            assert!(Rc::ptr_eq(&c, &again), "resolved once, not re-registered");
            // The owner's animation walk advances it (the register_animation_once
            // registration is live).
            owner.tick_animations(DURATION / 2.0);
            assert!(
                (c.position() - 0.5).abs() < 1e-4,
                "tick_animations drove the registered clock: {}",
                c.position()
            );
        });
    }
}
