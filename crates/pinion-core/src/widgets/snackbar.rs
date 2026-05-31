//! R725 §5.28 §5.38 — Snackbar / Toast timed-auto-dismiss driver.
//!
//! A snackbar is a transient, bottom-anchored message that appears in
//! response to an action, optionally carries a single action label
//! (Material 3 `inversePrimary`), and **auto-dismisses after a
//! duration** unless the user dismisses it sooner. This primitive owns
//! only the *visibility + countdown* — the message text, the action
//! button, and the inverse-surface chrome are the binding's
//! composition (mirroring how [`CaretBlink`](super::caret_blink) owns
//! only the blink phase, not the caret geometry).
//!
//! ## Timed dismissal rides the §5.28 animation driver
//!
//! [`SnackbarTimer`] is a [`Tickable`]: each
//! [`Owner::tick_animations`](crate::reactive::Owner::tick_animations)
//! step advances an `elapsed` accumulator, and once it reaches
//! `duration` the `visible` [`Signal`] flips to `false`. That makes the
//! dismissal **deterministically drivable** by the R724 `scene/tick`
//! RPC — an AI client shows the snackbar, ticks past the duration, and
//! observes the auto-dismiss without waiting on wall-clock time. It is
//! the [`CaretBlink`](super::caret_blink) precedent applied to a
//! *one-shot* countdown rather than a repeating phase.
//!
//! ## Material 3 duration
//!
//! [`SnackbarTimer::DEFAULT_DURATION_SECS`] = 4 s — the Material 3
//! "short" snackbar duration (M3 allows up to 10 s for snackbars with
//! an action; the binding passes its own value to [`Self::show`]).

use std::cell::Cell;
use std::rc::Rc;

use crate::animation::Tickable;
use crate::reactive::{Owner, Signal};

/// R725 §5.28 §5.38 — visibility + auto-dismiss countdown for a single
/// snackbar. One instance per snackbar, reached through
/// [`use_snackbar_timer`]. The binding reads [`Self::visible`] in the
/// view-fn to decide whether to paint the overlay; the value
/// auto-subscribes so the paint pass re-runs the instant the timer
/// dismisses.
///
/// Thread-safety: not `Send` / `Sync` (`Rc` + `Cell`), matching every
/// other `pinion-core` reactive primitive. UI thread only.
#[derive(Debug)]
pub struct SnackbarTimer {
    /// `true` = snackbar shown (paint the overlay), `false` = hidden.
    /// Auto-subscribes when read inside a view-fn.
    visible: Signal<bool>,
    /// Seconds elapsed since the current [`Self::show`]. Advanced by
    /// [`Tickable::tick`]; reset to `0.0` on each `show`.
    elapsed: Cell<f32>,
    /// Auto-dismiss horizon in seconds — the snackbar hides once
    /// `elapsed >= duration`. Set per-`show` so different messages can
    /// carry different durations.
    duration: Cell<f32>,
}

impl SnackbarTimer {
    /// Material 3 "short" snackbar duration. M3 permits up to 10 s for
    /// snackbars carrying an action; callers pass their own value to
    /// [`Self::show`].
    pub const DEFAULT_DURATION_SECS: f32 = 4.0;

    /// Construct a hidden timer (no countdown running).
    #[must_use]
    pub fn new() -> Self {
        Self {
            visible: Signal::new(false),
            elapsed: Cell::new(0.0),
            duration: Cell::new(Self::DEFAULT_DURATION_SECS),
        }
    }

    /// Whether the snackbar should paint this frame. Triggers a
    /// [`Signal`] subscription inside a view-fn — the view re-runs when
    /// the timer shows or auto-dismisses.
    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible.get()
    }

    /// Show the snackbar and (re)start the auto-dismiss countdown for
    /// `duration` seconds. Calling `show` while already visible
    /// restarts the countdown (the canonical "new message replaces the
    /// current one" behaviour).
    pub fn show(&self, duration: f32) {
        self.duration.set(duration);
        self.elapsed.set(0.0);
        self.visible.set(true);
    }

    /// Dismiss immediately (user tapped the action or the close
    /// affordance). No-op if already hidden (Signal equality-skip).
    pub fn dismiss(&self) {
        self.visible.set(false);
    }

    /// Seconds remaining before auto-dismiss, clamped to `>= 0`; `0.0`
    /// while hidden. Exposed so an AI client / introspection surface
    /// can read the countdown as data (§2 #7).
    #[must_use]
    pub fn remaining(&self) -> f32 {
        if self.visible.get() {
            (self.duration.get() - self.elapsed.get()).max(0.0)
        } else {
            0.0
        }
    }

    /// The current auto-dismiss horizon in seconds (the last
    /// [`Self::show`] duration, or [`Self::DEFAULT_DURATION_SECS`]).
    #[must_use]
    pub fn duration(&self) -> f32 {
        self.duration.get()
    }
}

impl Default for SnackbarTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Tickable for SnackbarTimer {
    fn tick(&self, dt: f32) {
        if !self.visible.get() {
            return;
        }
        let elapsed = self.elapsed.get() + dt;
        self.elapsed.set(elapsed);
        if elapsed >= self.duration.get() {
            self.visible.set(false);
        }
    }

    fn is_at_rest(&self, _epsilon: f32) -> bool {
        // A hidden snackbar has no further state evolution until the
        // next `show`; a visible one is counting down, so the backend
        // must keep requesting frames until it auto-dismisses.
        !self.visible.get()
    }
}

/// R725 §5.28 §5.38 — resolve (or lazily initialize) a
/// [`SnackbarTimer`] for the current view scope and register it with
/// the owner's animation driver. Mirrors
/// [`use_caret_blink`](super::caret_blink::use_caret_blink): delegates
/// to [`Owner::cache`](crate::reactive::Owner::cache) so the same
/// `Rc<SnackbarTimer>` resolves across view re-runs, and registers the
/// `Tickable` exactly once (gated by
/// [`Owner::cache_contains`](crate::reactive::Owner::cache_contains))
/// so the `tick_animations` walk does not advance it twice per frame.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (invoked outside a
/// `root_owner.run(...)` wrap) — framework dispatch sites supply the
/// wrap per the callback-root-owner-wrap discipline. Panics if the
/// cache key was previously bound to a different concrete type within
/// the same owner (see [`Owner::cache`]).
#[must_use]
pub fn use_snackbar_timer(key: &'static str) -> Rc<SnackbarTimer> {
    let owner = Owner::current().expect("use_snackbar_timer requires an active Owner scope");
    let first_time = !owner.cache_contains::<SnackbarTimer>(key);
    let timer = owner.cache(key, SnackbarTimer::new);
    if first_time {
        let as_tickable: Rc<dyn Tickable> = Rc::clone(&timer) as Rc<dyn Tickable>;
        owner.register_animation(as_tickable);
    }
    timer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Owner;

    #[test]
    fn hidden_by_default_and_at_rest() {
        let t = SnackbarTimer::new();
        assert!(!t.visible());
        assert!(t.is_at_rest(0.0));
        assert!(t.remaining().abs() < f32::EPSILON);
    }

    #[test]
    fn show_makes_visible_and_starts_countdown() {
        let t = SnackbarTimer::new();
        t.show(4.0);
        assert!(t.visible());
        assert!(!t.is_at_rest(0.0), "a visible timer is counting down");
        assert!((t.remaining() - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn tick_auto_dismisses_after_duration() {
        let t = SnackbarTimer::new();
        t.show(4.0);
        t.tick(1.0);
        assert!(t.visible(), "still visible mid-countdown");
        assert!((t.remaining() - 3.0).abs() < 1e-6);
        t.tick(3.0); // total 4.0 == duration
        assert!(!t.visible(), "auto-dismissed at duration");
        assert!(t.remaining().abs() < f32::EPSILON);
    }

    #[test]
    fn tick_while_hidden_is_noop() {
        let t = SnackbarTimer::new();
        t.tick(10.0);
        assert!(!t.visible());
    }

    #[test]
    fn dismiss_hides_immediately() {
        let t = SnackbarTimer::new();
        t.show(4.0);
        t.dismiss();
        assert!(!t.visible());
        assert!(t.is_at_rest(0.0));
    }

    #[test]
    fn show_restarts_countdown() {
        let t = SnackbarTimer::new();
        t.show(4.0);
        t.tick(3.5);
        t.show(4.0); // restart
        assert!(t.visible());
        assert!((t.remaining() - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn use_hook_registers_animation_once_per_key() {
        let owner = Owner::new();
        owner.run(|| {
            let _a = use_snackbar_timer("snack");
            let _b = use_snackbar_timer("snack");
            let _c = use_snackbar_timer("snack");
        });
        assert_eq!(owner.registered_animation_count(), 1);
    }
}
