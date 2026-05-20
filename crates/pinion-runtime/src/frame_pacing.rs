//! R51.145 §5.28 — per-frame `dt` clamp helper.
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

#[cfg(test)]
mod tests {
    use super::{clamp_frame_dt, MAX_FRAME_DT_SECS};

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
}
