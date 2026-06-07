//! R51.145 §5.28 — per-frame `dt` clamp helper +
//! R681 §2 #4 atomic 3 — per-window paint policy.
//!
//! Both backends ([`pinion_shell::ShellCore`] +
//! [`pinion_tui::ShellCoreTui`]) measure `dt` from a wall clock
//! ([`Instant::now`](std::time::Instant::now)). Wall-clock measurement
//! is unbounded — a backgrounded app, a sleeping laptop lid, a
//! debugger breakpoint, the very first paint after a hibernate cycle
//! can all hand the substrate a `dt` of seconds or minutes.
//!
//! Feeding that raw `dt` to the §5.28 spring solver causes two
//! problems:
//!
//! 1. **Numerical instability** — the semi-implicit Euler integrator
//!    inside `SpringState::tick` is stable for `dt` near the frame
//!    budget; large `dt` causes oscillation amplification. The
//!    canonical guard in the game-engine and `SwiftUI` / Compose
//!    literature is a single-frame-worth-of-time cap on `dt` before
//!    it reaches the integrator.
//! 2. **Visual jump on resume** — even if the math stayed stable, a
//!    spring fed a 5-second `dt` would jump straight to its target
//!    (or past it). The clamp instead spreads the catch-up across
//!    the next few real frames, which matches user expectation when
//!    a paused app resumes.
//!
//! The cap is anchored at 1/30s ≈ 33.3ms — twice the 60fps frame
//! budget, so a single dropped frame on a smooth pipeline still sees
//! the real measured `dt` (clamp is a no-op for healthy frames) but
//! anything beyond a dropped frame is treated as "missed frames go
//! to the integrator's blind spot, not the visible motion".

/// Maximum per-frame `dt` (in seconds) the framework hands to the
/// spring solver and the view fn.
///
/// `1.0 / 30.0` ≈ 33.3ms. See the module doc for the
/// rationale — this is the longest `dt` the semi-implicit Euler
/// integrator inside [`pinion_core::animation::SpringState::tick`]
/// stays numerically well-behaved for.
pub const MAX_FRAME_DT_SECS: f32 = 1.0 / 30.0;

/// Clamp a measured per-frame `dt` to the [`MAX_FRAME_DT_SECS`]
/// ceiling and a `0.0` floor.
///
/// Negative inputs collapse to `0.0`. The monotonic-clock contract
/// behind [`Instant::now`](std::time::Instant::now) rules them out
/// on a healthy machine, but `f32` precision drift (or a system
/// clock that violates monotonicity through a backstep) can still
/// produce a negative `now - prev`. Treating that as "no progress"
/// is the safe default — the alternative (negative dt fed to the
/// spring integrator) breaks every invariant the solver relies on.
///
/// `NaN` inputs collapse to `0.0` too. The bare `f32::clamp` propagates
/// `NaN`; the explicit guard here ensures the substrate never hands
/// `NaN` to the integrator.
///
/// Values inside `[0.0, MAX_FRAME_DT_SECS]` pass through unchanged.
/// This means a smooth 60fps pipeline (16.6ms per frame) never sees
/// the clamp activate — the `clamp_frame_dt` call is a no-op on the
/// hot path and only fires when the framework was paused / blocked
/// / interrupted.
#[must_use]
pub fn clamp_frame_dt(dt: f32) -> f32 {
    if dt.is_nan() {
        return 0.0;
    }
    dt.clamp(0.0, MAX_FRAME_DT_SECS)
}

/// R724 / R829 §5.28 — fixed-timestep sub-step (seconds) for advancing
/// time-driven state from an RPC-injected `dt`. The §5.28 spring solver
/// is semi-implicit Euler — stable only for small steps — so a large
/// injected delta is advanced in sub-steps rather than one giant step
/// that would overshoot. Private: the policy is exposed only through
/// [`substep`], so no caller can re-derive the loop with a different
/// granularity.
const FIXED_TIMESTEP_SUBSTEP_SECS: f32 = 1.0 / 120.0;

/// R830 §5.28 — drive `step_fn` over `dt` in fixed
/// [`FIXED_TIMESTEP_SUBSTEP_SECS`] sub-steps (the canonical
/// fixed-timestep accumulator). SSOT for the sub-stepping *policy* — the
/// `max(0.0)` floor, the `min` clamp, the `remaining -= step`
/// accumulation, and the semi-implicit-Euler stability decision — shared
/// by every RPC-injected time advance (`scene/tick` animation clock +
/// the R829 deterministic immediate-mode step) so the two time bases
/// cannot silently desync (R829 lifted the constant; R830 lifts the loop
/// the constant lived inside). `dt <= 0.0` invokes `step_fn` zero times
/// (a frozen clock).
pub fn substep(dt: f32, mut step_fn: impl FnMut(f32)) {
    let mut remaining = dt.max(0.0);
    while remaining > 0.0 {
        let step = remaining.min(FIXED_TIMESTEP_SUBSTEP_SECS);
        step_fn(step);
        remaining -= step;
    }
}

/// R681 §2 #4 atomic 3 — per-window paint pacing policy. Selects
/// between input-driven (idle) and game-loop (polled) lifecycles for
/// a window slot.
///
/// `Idle` maps to [`winit::event_loop::ControlFlow::Wait`] — the
/// canonical retained-tree GUI semantics every Phase A binding uses.
/// `Polled { fps }` maps to
/// [`winit::event_loop::ControlFlow::WaitUntil(last_paint + 1/fps)`]
/// — the canonical game-engine fixed-step semantics, used by windows
/// that contain at least one
/// [`pinion_core::Scene::ImmediateModeNode`].
///
/// `fps = 0` collapses to `Idle` semantics (no paint deadline, no
/// frame budget). Sentinel for "explicit polled-but-paused" — a
/// future round may use this to model a paused game loop without
/// flipping the policy back to `Idle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowFramePolicy {
    /// Input-driven pacing — `ControlFlow::Wait` semantics. Wakes
    /// only on input / state-change-driven `request_redraw`.
    Idle,
    /// Game-loop pacing at the given frame rate. Wakes every
    /// `1/fps` seconds via `ControlFlow::WaitUntil(deadline)`.
    Polled {
        /// Target frames per second. `0` collapses to idle
        /// semantics (no deadline, no budget) as the sentinel for
        /// "paused polled window".
        fps: u32,
    },
}

impl WindowFramePolicy {
    /// Convenience constructor for `Polled { fps }`.
    #[must_use]
    pub const fn polled(fps: u32) -> Self {
        Self::Polled { fps }
    }

    /// Frame budget for one paint cycle under this policy. `None`
    /// for [`Self::Idle`] and the `fps == 0` sentinel; `Some(1/fps)`
    /// otherwise.
    ///
    /// `pinion_shell::AppShell::about_to_wait` reads this to compute
    /// the next-paint deadline per window:
    /// `Instant::now() + budget` — or, more precisely, the per-window
    /// `last_paint_instant + budget` so the timing locks to the
    /// per-window paint clock rather than to the wake-up moment.
    #[must_use]
    pub fn frame_budget(self) -> Option<core::time::Duration> {
        match self {
            Self::Idle | Self::Polled { fps: 0 } => None,
            Self::Polled { fps } => {
                Some(core::time::Duration::from_secs_f64(1.0 / f64::from(fps)))
            }
        }
    }

    /// `true` iff this policy drives a wake-up cadence
    /// ([`Self::Polled`] with non-zero `fps`).
    #[must_use]
    pub fn is_polled(self) -> bool {
        matches!(self, Self::Polled { fps } if fps != 0)
    }
}

/// R681 §2 #4 atomic 3 — default `fps` for an immediate-mode window
/// that has not been explicitly configured. 60fps matches the
/// standard refresh rate of every desktop monitor pinion supports;
/// a future round may raise the default to match higher-refresh
/// displays via a runtime probe.
pub const DEFAULT_IMMEDIATE_MODE_FPS: u32 = 60;

/// R681 §2 #4 atomic 3 — derive the per-window
/// [`WindowFramePolicy`] from the substrate signals. Used by the
/// surface ([`pinion_shell::AppShell::about_to_wait`]) when no
/// explicit per-window policy has been registered:
///
/// - `has_immediate_mode_subtree = true` →
///   `Polled { fps: DEFAULT_IMMEDIATE_MODE_FPS }`.
/// - `has_immediate_mode_subtree = false` → `Idle`.
///
/// Explicit per-window overrides are stored on the substrate
/// ([`pinion_shell::ShellCore::set_target_fps_for_window`]) and win
/// over this default — the convenience surface for binding authors
/// to opt into 120fps (high-refresh display) or 30fps (battery
/// saver) without rewriting `WindowFramePolicy` construction.
#[must_use]
pub fn default_window_frame_policy(has_immediate_mode_subtree: bool) -> WindowFramePolicy {
    if has_immediate_mode_subtree {
        WindowFramePolicy::polled(DEFAULT_IMMEDIATE_MODE_FPS)
    } else {
        WindowFramePolicy::Idle
    }
}

/// R681 §2 #4 atomic 3 — frame budget for one paint cycle under the
/// derived policy. Thin wrapper over
/// [`WindowFramePolicy::frame_budget`] that handles the
/// `has_immediate_mode_subtree = false` short-circuit
/// (`Idle → None`).
#[must_use]
pub fn frame_budget_for_window(
    has_immediate_mode_subtree: bool,
    override_fps: Option<u32>,
) -> Option<core::time::Duration> {
    let policy = match override_fps {
        Some(fps) => WindowFramePolicy::polled(fps),
        None => default_window_frame_policy(has_immediate_mode_subtree),
    };
    policy.frame_budget()
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_frame_dt, default_window_frame_policy, frame_budget_for_window,
        WindowFramePolicy, DEFAULT_IMMEDIATE_MODE_FPS, MAX_FRAME_DT_SECS,
    };
    use core::time::Duration;

    #[test]
    fn zero_passes_through() {
        // R51.145 — the first paint hands `dt = 0.0`; the clamp must
        // be a no-op so the synthetic first-frame guarantee carries
        // through.
        assert_eq!(clamp_frame_dt(0.0).to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn typical_frame_budget_passes_through() {
        // 16.6ms (60fps) and 33.3ms (30fps boundary) both fall
        // inside the cap; the clamp must be invisible on the
        // healthy hot path.
        let sixty_fps = 1.0_f32 / 60.0;
        let thirty_fps = 1.0_f32 / 30.0;
        assert_eq!(
            clamp_frame_dt(sixty_fps).to_bits(),
            sixty_fps.to_bits(),
            "60fps frame dt passes through unclamped",
        );
        assert_eq!(
            clamp_frame_dt(thirty_fps).to_bits(),
            thirty_fps.to_bits(),
            "30fps frame dt sits exactly at the cap",
        );
    }

    #[test]
    fn long_pause_clamps_to_ceiling() {
        // 5 seconds (~ a backgrounded app resuming, or a debugger
        // breakpoint releasing) must collapse to the ceiling so the
        // spring solver does not see a destabilizing `dt`.
        assert_eq!(
            clamp_frame_dt(5.0).to_bits(),
            MAX_FRAME_DT_SECS.to_bits(),
            "5s pause clamps to MAX_FRAME_DT_SECS",
        );
    }

    #[test]
    fn negative_collapses_to_zero() {
        // A monotonic-clock violation (or `f32` precision drift on a
        // sub-microsecond delta) can synthesize a negative measured
        // delta. The clamp normalizes it to `0.0` rather than passing
        // a negative `dt` into the integrator.
        assert_eq!(clamp_frame_dt(-0.1).to_bits(), 0.0_f32.to_bits());
        assert_eq!(clamp_frame_dt(f32::NEG_INFINITY).to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn nan_collapses_to_zero() {
        // `f32::clamp` propagates NaN per the IEEE-754 default; but
        // `f32::clamp` actually substitutes the lower bound when
        // either operand is NaN. Documenting the observable shape
        // here so a future runtime that swaps clamp impls can not
        // silently regress the contract.
        let clamped = clamp_frame_dt(f32::NAN);
        assert!(
            clamped.is_finite(),
            "clamp_frame_dt must never propagate NaN to the solver",
        );
        assert_eq!(
            clamped.to_bits(),
            0.0_f32.to_bits(),
            "NaN input collapses to the floor (0.0)",
        );
    }

    #[test]
    fn ceiling_is_one_thirtieth_second() {
        // Anchor the cap value so the module doc stays in sync.
        let expected = 1.0_f32 / 30.0;
        assert_eq!(MAX_FRAME_DT_SECS.to_bits(), expected.to_bits());
    }

    // ── R681 atomic 3 — WindowFramePolicy + default + budget helpers ──

    #[test]
    fn r681_default_policy_idle_when_no_immediate_subtree() {
        assert_eq!(default_window_frame_policy(false), WindowFramePolicy::Idle);
    }

    #[test]
    fn r681_default_policy_polled_60fps_when_immediate_subtree_present() {
        assert_eq!(
            default_window_frame_policy(true),
            WindowFramePolicy::polled(DEFAULT_IMMEDIATE_MODE_FPS),
        );
        assert_eq!(DEFAULT_IMMEDIATE_MODE_FPS, 60);
    }

    #[test]
    fn r681_idle_policy_has_no_frame_budget() {
        assert_eq!(WindowFramePolicy::Idle.frame_budget(), None);
        assert!(!WindowFramePolicy::Idle.is_polled());
    }

    #[test]
    fn r681_polled_zero_fps_collapses_to_no_budget() {
        // Sentinel: paused polled window (no deadline).
        let p = WindowFramePolicy::polled(0);
        assert_eq!(p.frame_budget(), None);
        assert!(!p.is_polled());
    }

    #[test]
    fn r681_polled_60fps_budget_is_one_sixtieth_second() {
        let budget = WindowFramePolicy::polled(60)
            .frame_budget()
            .expect("60fps gives a budget");
        // 1/60 sec ≈ 16.667 ms; encoded exactly through
        // `Duration::from_secs_f64`.
        let expected = Duration::from_secs_f64(1.0 / 60.0);
        assert_eq!(budget, expected);
    }

    #[test]
    fn r681_polled_120fps_budget_is_half_60fps_budget() {
        let b60 = WindowFramePolicy::polled(60).frame_budget().unwrap();
        let b120 = WindowFramePolicy::polled(120).frame_budget().unwrap();
        // 120fps budget is half of 60fps budget. The ratio sits
        // very close to 2.0 — `Duration::from_secs_f64` truncates
        // to nanosecond resolution, so 1/60 → 16_666_666 ns and
        // 1/120 → 8_333_333 ns give a ratio of 2.0000000400
        // (well under a 1e-6 tolerance, comfortably better than
        // any real timer fidelity we care about).
        let ratio = b60.as_secs_f64() / b120.as_secs_f64();
        assert!((ratio - 2.0).abs() < 1e-6, "ratio = {ratio}");
    }

    #[test]
    fn r681_polled_is_polled_for_non_zero_fps() {
        assert!(WindowFramePolicy::polled(60).is_polled());
        assert!(WindowFramePolicy::polled(30).is_polled());
        assert!(WindowFramePolicy::polled(144).is_polled());
        // Sentinel: fps == 0 collapses.
        assert!(!WindowFramePolicy::polled(0).is_polled());
    }

    #[test]
    fn r681_frame_budget_for_window_default_idle_returns_none() {
        // No immediate subtree, no override → None.
        assert_eq!(frame_budget_for_window(false, None), None);
    }

    #[test]
    fn r681_frame_budget_for_window_immediate_default_60fps() {
        let budget = frame_budget_for_window(true, None).expect("immediate default");
        assert_eq!(budget, Duration::from_secs_f64(1.0 / 60.0));
    }

    #[test]
    fn r681_frame_budget_for_window_override_wins_over_default() {
        // Override applies even when has_immediate_mode_subtree is
        // false (a binding may opt into polled lifecycle for a
        // retained-tree window — e.g. a smooth animation overlay
        // without an immediate-mode driver).
        let budget = frame_budget_for_window(false, Some(30)).expect("30fps override");
        assert_eq!(budget, Duration::from_secs_f64(1.0 / 30.0));
        // Override fps = 0 collapses to None.
        assert_eq!(frame_budget_for_window(true, Some(0)), None);
        // Override raises rate above default.
        let high = frame_budget_for_window(true, Some(144)).expect("144fps override");
        assert!(high < Duration::from_secs_f64(1.0 / 60.0));
    }
}
