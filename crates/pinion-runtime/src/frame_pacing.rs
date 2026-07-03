//! R51.145 §5.28 — per-frame `dt` clamp helper +
//! R681 §2 #4 atomic 3 — per-window paint policy.
//!
//! Both backends (`pinion_shell::ShellCore` +
//! `pinion_tui::ShellCoreTui`) measure `dt` from a wall clock
//! ([`Instant::now`](std::time::Instant::now)). Wall-clock measurement
//! is unbounded — a backgrounded app, a sleeping laptop lid, a
//! debugger breakpoint, the very first paint after a hibernate cycle
//! can all hand the substrate a `dt` of seconds or minutes.
//!
//! Feeding that raw `dt` to the §5.28 spring solver causes two
//! problems:
//!
//! 1. **Numerical instability** — the semi-implicit Euler integrator
//!    inside `SpringState::step` is stable for `dt` near the frame
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
/// integrator inside [`SpringState::step`](pinion_core::animation::SpringState::step)
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

/// R830 §5.28 — advance `step_fn` over the WHOLE of `dt` in fixed
/// `FIXED_TIMESTEP_SUBSTEP_SECS` sub-steps, with a partial final step
/// so the entire delta is consumed and NOTHING is carried. SSOT for the
/// §5.28 *animation-clock* sub-stepping policy — the `max(0.0)` floor,
/// the `min` clamp, the `remaining -= step` drain, and the
/// semi-implicit-Euler stability decision. `dt <= 0.0` invokes `step_fn`
/// zero times (a frozen clock).
///
/// This is a *splitter*, NOT an accumulator (it keeps no state and
/// leaves no remainder) — the right shape for the animation clock, which
/// `scene/tick` advances IN PLACE: a §5.28 spring is an attractor that
/// converges to its target regardless of step size, so the contract is
/// "advance the whole injected delta now, sub-stepped only for
/// integrator stability." The §2 #4 immediate-mode game loop has the
/// opposite requirement (path-dependent simulations, deferred to the
/// paint cycle) and uses [`FixedTimestep`] — a true remainder-carrying
/// accumulator — instead. The shared `FIXED_TIMESTEP_SUBSTEP_SECS`
/// constant keeps the two sub-step granularities in lockstep without
/// conflating the two policies.
///
/// **Known limitation (honest):** the animation clock's *live* path
/// (`pinion_shell` per-window paint) advances the full clamped frame `dt`
/// in ONE step, while `scene/tick` substeps — the same live-vs-RPC
/// asymmetry R831 removed for the immediate-mode loop. Springs converge,
/// so the *settled* state matches, but for the same total elapsed time
/// the two paths trace different *mid-flight* values (observable via §2
/// #7 introspection). Unifying the animation live path on a fixed
/// timestep too is a deferred axis (broad regression surface across the
/// spring-animated catalog); it is NOT yet done, contrary to any reading
/// that R831 made the animation clock deterministic.
pub fn substep(dt: f32, mut step_fn: impl FnMut(f32)) {
    let mut remaining = dt.max(0.0);
    while remaining > 0.0 {
        let step = remaining.min(FIXED_TIMESTEP_SUBSTEP_SECS);
        step_fn(step);
        remaining -= step;
    }
}

/// R831 §2 #4 §5.28 — fixed-timestep accumulator for the immediate-mode
/// game loop (Glenn Fiedler, "Fix Your Timestep!"). Carries the leftover
/// simulation time across frames and steps the simulation in EXACTLY
/// `FIXED_TIMESTEP_SUBSTEP_SECS` increments — discarding nothing,
/// adding nothing. Contrast [`substep`], which consumes a whole injected
/// delta with a partial final step and keeps no state.
///
/// Two execution models, two policies:
///
/// - [`substep`] drives the §5.28 animation clock (advanced IN PLACE by
///   `scene/tick`; springs are attractors → "advance the whole delta
///   now, sub-stepped for stability", no remainder).
/// - [`FixedTimestep`] drives the §2 #4 immediate-mode game loop
///   (DEFERRED to the per-window paint cycle; drivers run arbitrary,
///   often non-attractor simulations → every step must be the same size
///   and the sub-step remainder must carry to the next frame).
///
/// The carry is what gives the immediate-mode loop its two canonical
/// game-loop properties:
///
/// - **frame-rate independence** — a 30fps and a 144fps render of the
///   same wall-clock window take the same number of fixed steps (modulo
///   a sub-step of carried phase), so simulation behaviour does not
///   depend on render cadence.
/// - **`dt`-chunking determinism** — `scene/tick 1.0` and ten
///   `scene/tick 0.1` calls advance the simulation by the same number of
///   fixed steps: the total elapsed time, not its delivery, drives the
///   step count. This is what lets `scene/tick` reproduce live behaviour
///   (§2 #3 dry-run / deterministic-stepping extended to Phase C).
///   **Bound:** the accumulator is `f32`, so chunks far below the
///   accumulator's magnitude can be absorbed by rounding (≈10^6 chunks of
///   1µs summing to 1s lose one step). Holds for realistic chunk sizes
///   (frames, RPC ticks ≫ the f32 ULP at ~10⁻²s); a future round may
///   widen to `f64` / fixed-point for cross-machine bit-determinism.
///
/// The pre-R831 immediate-mode path had neither: live ticked the full
/// wall-clock delta in one variable step, while `scene/tick` ran
/// [`substep`]'s partial-final-step splitter — two divergent time bases
/// that could not reproduce each other. R831 routes BOTH through one
/// accumulator.
#[derive(Debug, Default, Clone, Copy)]
pub struct FixedTimestep {
    /// Simulation seconds accumulated but not yet consumed by a whole
    /// `FIXED_TIMESTEP_SUBSTEP_SECS` step. Always in
    /// `[0, FIXED_TIMESTEP_SUBSTEP_SECS)` once [`Self::advance`] returns.
    accumulator: f32,
}

impl FixedTimestep {
    /// Add `dt` elapsed seconds (a clamped wall-clock delta OR an
    /// injected `scene/tick` delta — the SAME accumulator serves both,
    /// which is precisely why `scene/tick` reproduces live) and invoke
    /// `step_fn(FIXED_TIMESTEP_SUBSTEP_SECS)` once per whole fixed step
    /// the accumulator now holds, carrying the remainder forward.
    /// Returns the number of fixed steps taken.
    ///
    /// `dt <= 0.0` (and `NaN`, via `max`) adds nothing and steps zero
    /// times — a frozen clock. The caller pre-clamps live wall-clock
    /// `dt` to [`MAX_FRAME_DT_SECS`], bounding the live step count (no
    /// spiral of death); an injected `scene/tick` delta is intentionally
    /// unbounded — an AI client asking to advance 10 seconds
    /// deterministically runs every fixed step it asked for.
    pub fn advance(&mut self, dt: f32, mut step_fn: impl FnMut(f32)) -> u32 {
        self.accumulator += dt.max(0.0);
        let mut steps = 0;
        while self.accumulator >= FIXED_TIMESTEP_SUBSTEP_SECS {
            step_fn(FIXED_TIMESTEP_SUBSTEP_SECS);
            self.accumulator -= FIXED_TIMESTEP_SUBSTEP_SECS;
            steps += 1;
        }
        steps
    }

    /// Discard the sub-step remainder, snapping the accumulator back to
    /// a zero phase. `pinion_shell` calls this when a window pauses
    /// (`scene/set_fps 0`) so an AI client frame-steps the loop from a
    /// known fixed-step boundary — the deterministic-debugging contract
    /// (pause snaps to a frame edge, like a debugger breaking between
    /// frames). The discarded remainder is below
    /// `FIXED_TIMESTEP_SUBSTEP_SECS` (< ~8 ms of simulation), beneath
    /// any observable threshold.
    pub fn reset(&mut self) {
        self.accumulator = 0.0;
    }

    /// The carried sub-step remainder in seconds, in
    /// `[0, FIXED_TIMESTEP_SUBSTEP_SECS)`. Exposed for tests and future
    /// introspection.
    #[must_use]
    pub fn remainder(self) -> f32 {
        self.accumulator
    }
}

/// R681 §2 #4 atomic 3 — per-window paint pacing policy. Selects
/// between input-driven (idle) and game-loop (polled) lifecycles for
/// a window slot.
///
/// `Idle` maps to `winit::event_loop::ControlFlow::Wait` — the
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

/// R888 §5.49 §5.28 §2 #4 — the addressed window's frame-pacing
/// *override slot*: the `scene/pacing_state` READ payload, mirroring
/// the `scene/set_fps` write axis (read = inverse of write,
/// [[wire-form-read-write-symmetry]]). Moved here from `pinion-rpc`
/// at R888.1 — pacing vocabulary lives with its domain, next to
/// [`WindowFramePolicy`].
///
/// Relation to [`WindowFramePolicy`] (deliberately NOT folded): this
/// enum is the *installed override slot* (what the AI client wrote);
/// `WindowFramePolicy` is the *per-frame resolved policy* the render
/// loop executes. `Override(n)` resolves to `Polled { fps: n }`;
/// `DefaultPolicy` resolves adaptively (Polled while immediate-mode
/// content is active, `Idle` otherwise).
///
/// A named two-state enum, NOT `Option<u32>` doubled into the
/// dispatch context's availability `Option` — "no override
/// installed" is a *value* of the axis, categorically different from
/// "this backend keeps no pacing clock" (the TUI, which surfaces
/// `PacingStateUnavailable`); nesting `Option`s would alias the two
/// (the R874 named-enum-over-nested-Option rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacingState {
    /// No override installed — the adaptive per-window default policy
    /// applies. Serializes as `{"fps": null}`.
    DefaultPolicy,
    /// An override is installed: paint at `n` fps; `0` = paused
    /// (frame-step mode — the window repaints only on explicit
    /// `scene/tick` / redraw). Serializes as `{"fps": n}`.
    Override(u32),
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
            Self::Polled { fps } => Some(core::time::Duration::from_secs_f64(1.0 / f64::from(fps))),
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
/// surface (`pinion_shell::AppShell::about_to_wait`) when no
/// explicit per-window policy has been registered:
///
/// - `has_immediate_mode_subtree = true` →
///   `Polled { fps: DEFAULT_IMMEDIATE_MODE_FPS }`.
/// - `has_immediate_mode_subtree = false` → `Idle`.
///
/// Explicit per-window overrides are stored on the substrate
/// (`pinion_shell::ShellCore::set_target_fps_for_window`) and win
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
        DEFAULT_IMMEDIATE_MODE_FPS, FIXED_TIMESTEP_SUBSTEP_SECS, FixedTimestep, MAX_FRAME_DT_SECS,
        WindowFramePolicy, clamp_frame_dt, default_window_frame_policy, frame_budget_for_window,
        substep,
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
        assert_eq!(
            clamp_frame_dt(f32::NEG_INFINITY).to_bits(),
            0.0_f32.to_bits()
        );
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

    // ── R830 substep splitter (animation clock) ──────────────────────

    #[test]
    fn r830_substep_consumes_whole_delta_with_partial_final() {
        // The splitter advances the WHOLE delta: a delta that is not a
        // multiple of FIXED runs `ceil` steps whose sizes sum back to
        // the input exactly (last step partial), leaving no remainder.
        let mut seen: Vec<f32> = Vec::new();
        substep(2.5 * FIXED_TIMESTEP_SUBSTEP_SECS, |s| seen.push(s));
        assert_eq!(
            seen.len(),
            3,
            "2.5 fixed steps → 3 sub-steps (2 full + partial)"
        );
        let total: f32 = seen.iter().sum();
        assert!(
            (total - 2.5 * FIXED_TIMESTEP_SUBSTEP_SECS).abs() < 1e-7,
            "sub-steps sum back to the whole delta: {total}",
        );
        // The final (partial) step is half a fixed step.
        assert!((seen[2] - 0.5 * FIXED_TIMESTEP_SUBSTEP_SECS).abs() < 1e-7);
    }

    #[test]
    fn r830_substep_frozen_clock_runs_zero_steps() {
        let mut n = 0;
        substep(0.0, |_| n += 1);
        substep(-1.0, |_| n += 1);
        assert_eq!(n, 0, "dt <= 0 is a frozen clock");
    }

    // ── R831 FixedTimestep accumulator (immediate-mode game loop) ─────

    #[test]
    fn r831_advance_steps_in_exact_fixed_increments() {
        let mut ts = FixedTimestep::default();
        let mut steps: Vec<f32> = Vec::new();
        let n = ts.advance(2.5 * FIXED_TIMESTEP_SUBSTEP_SECS, |s| steps.push(s));
        // Floor (not ceil): 2 whole steps, 0.5 carried — the opposite of
        // the splitter, which would run 3 sub-steps with a partial final.
        assert_eq!(n, 2, "2.5 fixed steps → 2 whole steps, remainder carried");
        assert_eq!(steps.len(), 2);
        assert!(
            steps
                .iter()
                .all(|s| (s - FIXED_TIMESTEP_SUBSTEP_SECS).abs() < 1e-9),
            "every step is EXACTLY one fixed timestep, never partial",
        );
        assert!(
            (ts.remainder() - 0.5 * FIXED_TIMESTEP_SUBSTEP_SECS).abs() < 1e-7,
            "the 0.5-step remainder carries forward: {}",
            ts.remainder(),
        );
    }

    #[test]
    fn r831_remainder_carries_across_calls() {
        // Two sub-fixed advances that individually step zero times sum to
        // a whole step on the second call — the carry is the whole point.
        let mut ts = FixedTimestep::default();
        let mut fired = 0;
        let first = ts.advance(0.6 * FIXED_TIMESTEP_SUBSTEP_SECS, |_| fired += 1);
        assert_eq!(first, 0, "0.6 of a step does not fire yet");
        assert_eq!(fired, 0);
        let second = ts.advance(0.6 * FIXED_TIMESTEP_SUBSTEP_SECS, |_| fired += 1);
        // 0.6 + 0.6 = 1.2 → one whole step on the second call, 0.2 carried.
        assert_eq!(second, 1, "0.6 + 0.6 carries to exactly one fixed step");
        assert_eq!(fired, 1);
        assert!((ts.remainder() - 0.2 * FIXED_TIMESTEP_SUBSTEP_SECS).abs() < 1e-6);
    }

    #[test]
    fn r831_chunking_determinism_total_steps_independent_of_delivery() {
        // THE canonical property: the same total elapsed time produces
        // the same number of fixed steps regardless of how it is chunked
        // — what makes `scene/tick` reproduce live and the simulation
        // frame-rate-independent. (The splitter `substep` would NOT: it
        // runs a partial step per call, so 100 calls = 100 partials.)
        let total = 1.0_f32; // a full second
        let mut one_shot = FixedTimestep::default();
        let mut steps_one = 0u32;
        steps_one += one_shot.advance(total, |_| {});

        let mut chunked = FixedTimestep::default();
        let mut steps_many = 0u32;
        // 1000 chunks of 1ms — none a multiple of the 1/120 fixed step,
        // so this exercises the carry hard.
        for _ in 0..1000 {
            steps_many += chunked.advance(0.001, |_| {});
        }
        assert_eq!(
            steps_one, steps_many,
            "one-shot {steps_one} vs chunked {steps_many} fixed steps must match",
        );
        // And the leftover phase agrees to within an f32 epsilon budget.
        assert!(
            (one_shot.remainder() - chunked.remainder()).abs() < 1e-3,
            "carried phase converges: {} vs {}",
            one_shot.remainder(),
            chunked.remainder(),
        );
    }

    #[test]
    fn r831_advance_frozen_and_negative_step_zero() {
        let mut ts = FixedTimestep::default();
        let mut fired = 0;
        let mut n = ts.advance(0.0, |_| fired += 1);
        n += ts.advance(-5.0, |_| fired += 1);
        n += ts.advance(f32::NAN, |_| fired += 1);
        assert_eq!(n, 0, "dt <= 0 and NaN add nothing and step zero times");
        assert_eq!(fired, 0);
        assert_eq!(ts.remainder().to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn r831_reset_snaps_to_zero_phase() {
        // Pause discards the sub-step remainder so frame-stepping starts
        // from a known boundary.
        let mut ts = FixedTimestep::default();
        ts.advance(0.7 * FIXED_TIMESTEP_SUBSTEP_SECS, |_| {});
        assert!(
            ts.remainder() > 0.0,
            "a sub-fixed advance leaves a remainder"
        );
        ts.reset();
        assert_eq!(
            ts.remainder().to_bits(),
            0.0_f32.to_bits(),
            "reset clears it"
        );
        // After reset, an exact-multiple advance fires exactly that many
        // steps with no off-by-one from a stale phase.
        let mut fired = 0;
        ts.advance(3.0 * FIXED_TIMESTEP_SUBSTEP_SECS, |_| fired += 1);
        assert_eq!(fired, 3, "clean phase → exact step count");
    }
}
