//! R56.1.c §5.38 §5.28 — Caret blink animation primitive for
//! [`TextField`](crate::widgets::text_field::TextField).
//!
//! Independent of the §5.36 text shaping cache: this primitive only
//! tracks the **on / off phase** of the blinking caret. The geometry
//! is supplied by R56.1.b
//! [`caret_rect`](crate::widgets::text_field::caret_rect); the paint
//! backend draws the caret only when [`CaretBlink::visible`] returns
//! `true`.
//!
//! ## Period
//!
//! 530 ms per phase ([`CaretBlink::PERIOD_SECS`]) — the canonical
//! cross-platform half-period:
//!
//! | Platform | Half-period | Reference |
//! |---|---|---|
//! | an embedded browser engine / Firefox / Safari (Web) | 530 ms | DOM spec leaves it to the UA; all three converged on 530 ms |
//! | macOS (`NSTextView`) | 533 ms | `NSTextInsertionPointBlinkPeriodOn` default |
//! | iOS (`UITextField`) | 600 ms | tighter UX heuristic; pinion matches the web default |
//! | GTK (`gtk-cursor-blink-time`) | 1200 ms (full period = 2×600 ms) | settings-tunable |
//! | Windows (`SPI_GETCARETWIDTH` / `SPI_GETCURSORBLINKTIME`) | 530 ms | system-tunable default |
//!
//! 530 ms gives the textbook "two phases per second" UX
//! (`1000/530 ≈ 1.89 Hz`) and is what the majority of pinion's GUI
//! workload (web-app dogfood, R47 cosmic-text / parley
//! integration) inherits from its environment.
//!
//! ## Reset semantics
//!
//! [`CaretBlink::reset`] is the canonical "user just touched the
//! caret" hook: text insertion, deletion, or caret-position change
//! all snap the blink phase back to fully visible with the timer
//! restarted. Mirrors the macOS / iOS / Web "caret stays solid for
//! one phase after activity" convention — keeps the caret visible
//! while the user is typing and only resumes blinking once the user
//! pauses.
//!
//! ## Enable / disable
//!
//! [`CaretBlink::set_enabled`] gates the entire animation. The
//! canonical wire is:
//!
//! - `TextField` enters `Focused` → `caret.set_enabled(true)`;
//! - `TextField` enters `Idle` / `Disabled` → `caret.set_enabled(false)`.
//!
//! While disabled, [`CaretBlink::visible`] returns `false` (caret
//! hidden — un-focused fields show no caret per WCAG and platform
//! convention) and [`Tickable::is_at_rest`] returns `true` so the
//! framework's animation driver short-circuits the dispatch and the
//! backend can request fewer frames.

use std::cell::Cell;
use std::rc::Rc;

use crate::animation::Tickable;
use crate::reactive::{Owner, Signal};

/// R56.1.c §5.38 §5.28 — Two-phase blink driver for a text caret.
///
/// One instance per [`TextField`](crate::widgets::text_field::TextField).
/// The application reaches it through [`use_caret_blink`] (the
/// canonical `Owner::cache`-keyed hook) and reads
/// [`Self::visible`] from the paint pass — when `true`, paint the
/// caret; when `false`, skip it.
///
/// Thread-safety: not `Send` / `Sync` (uses `Rc` + `Cell`), matching
/// every other `pinion-core` reactive primitive. UI thread only.
#[derive(Debug)]
pub struct CaretBlink {
    /// Current blink phase — `true` = caret visible (drawn), `false`
    /// = caret hidden (skipped). Auto-subscribes when read inside a
    /// view-fn, so the paint pass re-runs at exactly the moment the
    /// phase flips.
    visible: Signal<bool>,
    /// Accumulated time within the current phase. Resets to `0.0`
    /// every time [`Self::tick`] flips the phase or [`Self::reset`]
    /// snaps the caret back to visible.
    elapsed_in_phase: Cell<f32>,
    /// Master gate — when `false`, the caret stays hidden and the
    /// timer does not advance. Set by [`Self::set_enabled`] from the
    /// `TextField` interaction-state listener.
    enabled: Cell<bool>,
}

impl CaretBlink {
    /// Canonical caret blink half-period (one visible-or-hidden
    /// phase). See module docs for the cross-platform comparison
    /// table; 530 ms matches the embedded browser engine / Firefox / Safari /
    /// Windows-default UX and is the value pinion's web-app dogfood
    /// workload inherits from its host environment.
    pub const PERIOD_SECS: f32 = 0.530;

    /// Construct a fresh `CaretBlink` in the disabled-and-hidden
    /// state. Callers wire it to the `TextField` interaction state
    /// via [`Self::set_enabled`] (on `Focused → enable`,
    /// `Idle/Disabled → disable`).
    #[must_use]
    pub fn new() -> Self {
        Self {
            visible: Signal::new(false),
            elapsed_in_phase: Cell::new(0.0),
            enabled: Cell::new(false),
        }
    }

    /// Whether the caret should be painted on this frame. Triggers a
    /// [`Signal`] subscription when called inside a view-fn — the
    /// view re-runs on every phase flip.
    #[must_use]
    pub fn visible(&self) -> bool {
        self.visible.get()
    }

    /// Whether the blink driver is currently enabled (i.e. whether
    /// [`Self::tick`] will advance the phase). Distinct from
    /// [`Self::visible`] — a disabled blink is always hidden, but an
    /// enabled blink alternates.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    /// Master gate. Transitions:
    ///
    /// - `false → true`: caret becomes visible immediately; the
    ///   timer resets to `0.0` so the user sees a fresh full-period
    ///   visible phase (the canonical "focus reveals caret" UX).
    /// - `true → false`: caret becomes hidden immediately; the
    ///   timer is irrelevant while disabled.
    /// - same-value `set_enabled`: no-op (Signal equality-skip).
    pub fn set_enabled(&self, on: bool) {
        if self.enabled.get() == on {
            return;
        }
        self.enabled.set(on);
        self.elapsed_in_phase.set(0.0);
        self.visible.set(on);
    }

    /// Reset the blink phase to "fully visible, timer at zero" —
    /// the canonical user-just-typed hook. Call from text edit
    /// (insert / backspace / delete) and caret-position change
    /// (arrow keys / mouse click). No-op when [`Self::enabled`] is
    /// `false`: an un-focused caret has nothing to reset.
    pub fn reset(&self) {
        if !self.enabled.get() {
            return;
        }
        self.elapsed_in_phase.set(0.0);
        self.visible.set(true);
    }
}

impl Default for CaretBlink {
    fn default() -> Self {
        Self::new()
    }
}

impl Tickable for CaretBlink {
    fn tick(&self, dt: f32) {
        if !self.enabled.get() {
            return;
        }
        let elapsed = self.elapsed_in_phase.get() + dt;
        if elapsed >= Self::PERIOD_SECS {
            // Phase flip. Carry over the overshoot so the average
            // period stays at `PERIOD_SECS` independent of frame
            // jitter (a 16.6 ms frame following a 530.4 ms phase
            // leaves 0.4 ms in the next phase).
            self.elapsed_in_phase.set(elapsed - Self::PERIOD_SECS);
            self.visible.set(!self.visible.get());
        } else {
            self.elapsed_in_phase.set(elapsed);
        }
    }

    fn is_at_rest(&self, _epsilon: f32) -> bool {
        // A disabled blink is at rest (no further state evolution
        // possible until `set_enabled(true)` fires). An enabled
        // blink is *never* at rest — it alternates forever, which
        // is the canonical caret behaviour. The backend's
        // `request_redraw` loop honours this so a focused field
        // keeps requesting frames; an un-focused field releases.
        !self.enabled.get()
    }
}

/// R56.1.c §5.38 §5.22 — Resolve (or lazily initialize) a
/// [`CaretBlink`] for the current view scope and register it with
/// the owner's animation driver.
///
/// Delegates to [`Owner::cache`](crate::reactive::Owner::cache) — the
/// same `Rc<CaretBlink>` resolves across view re-runs, but
/// [`Owner::register_animation`](crate::reactive::Owner::register_animation)
/// only fires on the first construction (gated via
/// [`Owner::cache_contains`](crate::reactive::Owner::cache_contains)
/// so subsequent calls do not re-register the same driver).
///
/// `key` MUST be a `&'static str`; the canonical pattern is to pass
/// the matching [`TextField`]'s tag verbatim so the blink driver and
/// the text-edit state share the same symbolic identifier.
/// (R56.1.b.1 §5.22) The underlying `Owner::cache` is keyed by
/// `(TypeId, &'static str)`, so the same widget tag composes cleanly
/// across typed hooks: `use_caret_blink(tag)` and
/// [`use_text_edit_state`](crate::widgets::text_edit::use_text_edit_state)`(tag)`
/// resolve to distinct slots without collision.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set — i.e. when invoked outside
/// a `root_owner.run(...)` wrap. Per the callback-root-owner-wrap
/// discipline (R51.146 / R51.152 / R51.171), framework-internal
/// dispatch sites supply this wrap.
///
/// Panics if the cache key was previously bound to a value of a
/// different concrete type within the same owner — see
/// [`Owner::cache`](crate::reactive::Owner::cache) for the
/// underlying contract.
///
/// [`TextField`]: crate::widgets::text_field::TextField
#[must_use]
pub fn use_caret_blink(key: &'static str) -> Rc<CaretBlink> {
    // R727 §5.28 — delegates to `Owner::register_animation_once`, the
    // SSOT for cache-and-register-once (the lift this comment historically
    // predicted, now landed when the 2nd/3rd Tickable hook arrived).
    Owner::current()
        .expect("use_caret_blink requires an active Owner scope")
        .register_animation_once(key, CaretBlink::new)
}

#[cfg(test)]
mod tests {
    //! R56.1.c §5.38 §5.28 — `CaretBlink` regression battery.
    //! Covers the period flip, enable/disable transitions, reset
    //! semantics, `Tickable::is_at_rest` reporting, and the
    //! `use_caret_blink` hook's first-time-only registration.

    use super::{CaretBlink, use_caret_blink};
    use crate::animation::Tickable;
    use crate::reactive::Owner;
    use std::rc::Rc;

    // ─────────────────────────────────────────────────────────────
    // Initial state
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_c_initial_state_is_disabled_and_hidden() {
        let b = CaretBlink::new();
        assert!(!b.enabled());
        assert!(!b.visible());
    }

    #[test]
    fn r56_1_c_default_matches_new() {
        // The Default impl is the no-arg constructor; defensive
        // pin so a future divergence (e.g. defaulting to enabled)
        // surfaces here.
        let a = CaretBlink::new();
        let b = CaretBlink::default();
        assert_eq!(a.enabled(), b.enabled());
        assert_eq!(a.visible(), b.visible());
    }

    // ─────────────────────────────────────────────────────────────
    // Enable / disable
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_c_set_enabled_true_makes_caret_visible_immediately() {
        let b = CaretBlink::new();
        b.set_enabled(true);
        assert!(b.enabled());
        assert!(b.visible(), "enable must surface caret immediately");
    }

    #[test]
    fn r56_1_c_set_enabled_false_hides_caret_immediately() {
        let b = CaretBlink::new();
        b.set_enabled(true);
        b.set_enabled(false);
        assert!(!b.enabled());
        assert!(!b.visible(), "disable must hide caret immediately");
    }

    #[test]
    fn r56_1_c_set_enabled_same_value_is_noop() {
        // Idempotence — re-enabling a focused field should not reset
        // its visible phase (annoying flicker for focus-stays-here
        // events). Equality-skip on the gate.
        let b = CaretBlink::new();
        b.set_enabled(true);
        // Tick partway through the visible phase.
        b.tick(0.2);
        let _ = b.visible(); // subscribed via signal read
        b.set_enabled(true);
        // No reset — but we cannot read elapsed externally. The
        // next tick still completes the phase in PERIOD - 0.2 secs.
        b.tick(CaretBlink::PERIOD_SECS - 0.2);
        assert!(
            !b.visible(),
            "redundant set_enabled must not reset the phase",
        );
    }

    // ─────────────────────────────────────────────────────────────
    // Tick → period flip
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_c_tick_before_period_does_not_flip() {
        let b = CaretBlink::new();
        b.set_enabled(true);
        assert!(b.visible());
        b.tick(0.1);
        b.tick(0.1);
        b.tick(0.1);
        assert!(b.visible(), "phase must not flip before PERIOD_SECS");
    }

    #[test]
    fn r56_1_c_tick_at_period_flips_visible_to_hidden() {
        let b = CaretBlink::new();
        b.set_enabled(true);
        assert!(b.visible());
        b.tick(CaretBlink::PERIOD_SECS);
        assert!(!b.visible(), "phase must flip at PERIOD_SECS boundary");
    }

    #[test]
    fn r56_1_c_two_periods_complete_full_blink_cycle() {
        let b = CaretBlink::new();
        b.set_enabled(true);
        assert!(b.visible());
        b.tick(CaretBlink::PERIOD_SECS);
        assert!(!b.visible());
        b.tick(CaretBlink::PERIOD_SECS);
        assert!(b.visible(), "full blink cycle returns to visible");
    }

    #[test]
    fn r56_1_c_tick_carries_overshoot_into_next_phase() {
        // 530 ms first phase + 30 ms overshoot → next phase
        // already has 30 ms accumulated → only 500 ms needed to flip.
        let b = CaretBlink::new();
        b.set_enabled(true);
        b.tick(CaretBlink::PERIOD_SECS + 0.030);
        assert!(!b.visible(), "first phase flipped");
        b.tick(CaretBlink::PERIOD_SECS - 0.030);
        assert!(b.visible(), "second flip used overshoot");
    }

    #[test]
    fn r56_1_c_disabled_blink_does_not_advance() {
        let b = CaretBlink::new();
        // No set_enabled — start disabled.
        b.tick(CaretBlink::PERIOD_SECS * 10.0);
        assert!(!b.visible(), "disabled tick stays hidden");
    }

    // ─────────────────────────────────────────────────────────────
    // Reset semantics
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_c_reset_makes_caret_visible_and_restarts_phase() {
        // User typed mid-blink: caret was hidden, reset snaps to
        // visible + restarts the phase timer.
        let b = CaretBlink::new();
        b.set_enabled(true);
        b.tick(CaretBlink::PERIOD_SECS);
        assert!(!b.visible(), "mid-blink hidden");
        b.reset();
        assert!(b.visible(), "reset surfaces caret");
        b.tick(CaretBlink::PERIOD_SECS / 2.0);
        assert!(
            b.visible(),
            "phase fresh after reset; mid-phase still visible"
        );
        b.tick(CaretBlink::PERIOD_SECS / 2.0);
        assert!(!b.visible(), "next flip lands at PERIOD after reset");
    }

    #[test]
    fn r56_1_c_reset_on_disabled_is_noop() {
        let b = CaretBlink::new();
        b.reset();
        assert!(!b.visible(), "reset on disabled stays hidden");
    }

    // ─────────────────────────────────────────────────────────────
    // Tickable::is_at_rest contract
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_c_disabled_is_at_rest() {
        // Backend animation-active gate: a disabled caret should
        // release the redraw request.
        let b = CaretBlink::new();
        assert!(b.is_at_rest(0.001));
    }

    #[test]
    fn r56_1_c_enabled_is_never_at_rest() {
        // Backend animation-active gate: a focused caret keeps
        // requesting frames so the blink keeps animating.
        let b = CaretBlink::new();
        b.set_enabled(true);
        assert!(!b.is_at_rest(0.001));
        // Even after many ticks.
        b.tick(CaretBlink::PERIOD_SECS * 100.0);
        assert!(!b.is_at_rest(0.001));
    }

    // ─────────────────────────────────────────────────────────────
    // use_caret_blink hook — Owner::cache + animation registration
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_c_use_hook_returns_same_state_across_runs() {
        let owner = Owner::new();
        let (a, b) = owner.run(|| {
            let a = use_caret_blink("k1");
            let b = use_caret_blink("k1");
            (a, b)
        });
        assert!(Rc::ptr_eq(&a, &b));
    }

    #[test]
    fn r56_1_c_use_hook_distinct_keys_distinct_states() {
        let owner = Owner::new();
        let (a, b) = owner.run(|| {
            let a = use_caret_blink("k1");
            let b = use_caret_blink("k2");
            (a, b)
        });
        assert!(!Rc::ptr_eq(&a, &b));
    }

    #[test]
    fn r56_1_c_use_hook_registers_animation_exactly_once_per_key() {
        // Calling the hook many times per key must NOT re-register
        // the Tickable — the framework driver would then tick the
        // blink twice per frame, doubling the period.
        let owner = Owner::new();
        owner.run(|| {
            let _b = use_caret_blink("primary");
            let _b = use_caret_blink("primary");
            let _b = use_caret_blink("primary");
        });
        assert_eq!(
            owner.registered_animation_count(),
            1,
            "single key registers exactly once across multiple use calls",
        );
    }

    #[test]
    fn r56_1_c_use_hook_registers_one_per_distinct_key() {
        let owner = Owner::new();
        owner.run(|| {
            let _ = use_caret_blink("primary");
            let _ = use_caret_blink("secondary");
        });
        assert_eq!(owner.registered_animation_count(), 2);
    }

    #[test]
    #[should_panic(expected = "use_caret_blink requires an active Owner scope")]
    fn r56_1_c_use_hook_panics_outside_owner_scope() {
        let _ = use_caret_blink("k");
    }

    // ─────────────────────────────────────────────────────────────
    // Integration: hook + Owner::tick_animations drives the blink
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn r56_1_c_owner_tick_drives_blink_phase_flip() {
        let owner = Owner::new();
        let blink = owner.run(|| use_caret_blink("primary"));
        blink.set_enabled(true);
        assert!(blink.visible());
        owner.tick_animations(CaretBlink::PERIOD_SECS);
        assert!(!blink.visible(), "owner.tick must drive blink phase flip");
    }
}
