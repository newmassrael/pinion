//! The real-time clock: a [`Tickable`] that advances the VN on the wall
//! clock.
//!
//! This is the answer to the R1296 audit's cardinal-sin finding — live
//! real-time play (the typewriter revealing, the countdown draining as the
//! seconds actually pass — the-tide's "급박함") is **not** Phase-C and does
//! **not** need an immediate-mode node. It is a small retained
//! [`Tickable`], the exact pattern
//! [`CaretBlink`](pinion_core::widgets::caret_blink::CaretBlink) uses: a
//! driver registered with the owner's animation system
//! ([`Owner::register_animation_once`]) that the shell ticks once per paint
//! with the real frame `dt`, and that keeps the frame loop alive while it is
//! not at rest ([`Owner::any_animation_active`]).
//!
//! ## Real-time vs. the deterministic harness — the honest split
//!
//! [`VnState::tick`] is a pure fixed-timestep function of a caller-supplied
//! `dt`; the shared clock feeds it that `dt`:
//!
//! - **Interactive window** — the per-window paint cycle calls
//!   `tick_animations_for_window(wall_clock_dt)`, so a live GUI advances the
//!   runner as real time passes (the-tide's felt urgency).
//! - **Headless harness** — the offscreen `PINION_HIDDEN_WINDOW` window still
//!   renders on the wall clock, so a registered clock advances the play-head by
//!   real time and it drifts run-to-run. This holds **even under
//!   `scene/set_fps 0`**: `scene/tick {dt}` does enqueue a `DeferredInput::Tick`
//!   that ticks this `tick_animations` path, but the RPC-induced repaints
//!   *also* tick it by wall-clock, so the net advance is non-deterministic
//!   (measured: a fixed `scene/tick 0.1` landed on 5–6 revealed chars across
//!   runs). So the harness deliberately does **not** register this clock; the
//!   zero-flake demo drives time by the deterministic `tick {ms}` verb
//!   ([`VnExternal`](crate::vn::VnExternal)) instead.
//!
//! So the `tick` verb IS genuinely forced for the deterministic harness — but
//! the honest reason is that a wall-clock animation cannot be driven
//! deterministically over the wire in this shell, **not** the earlier false
//! claim that "`scene/tick` can't reach a retained `Scene::External`" (it can;
//! it just also races the wall-clock repaint). Discrete player actions —
//! `advance` / `choose` / the stage director — remain `VnExternal` invoke
//! verbs; only *time* flows through this clock.

use std::cell::Cell;
use std::rc::Rc;

use pinion_core::animation::Tickable;
use pinion_core::reactive::Owner;

use crate::vn::state::VnState;

/// A [`Tickable`] driving a shared [`VnState`] on real (or scene-`tick`-fed)
/// time. Holds the sub-millisecond remainder so the `f32`-seconds frame `dt`
/// accumulates into the runner's integer-millisecond clock without drift.
#[derive(Debug)]
pub struct VnClock {
    state: Rc<VnState>,
    /// Fractional milliseconds carried between frames (a 16.67 ms frame after
    /// a 16.67 ms frame must sum to 33 ms + 0.33 ms carry, not 32 ms).
    carry_ms: Cell<f32>,
}

impl VnClock {
    /// Wrap a shared runner. The `Rc` is the same one the view and the
    /// [`VnExternal`](crate::vn::VnExternal) hold, so a real-time advance here
    /// repaints and is queryable there.
    #[must_use]
    pub fn new(state: Rc<VnState>) -> Self {
        Self {
            state,
            carry_ms: Cell::new(0.0),
        }
    }
}

impl Tickable for VnClock {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn tick(&self, dt: f32) {
        // A non-positive or NaN dt (never emitted by the framework) is ignored.
        if dt.is_nan() || dt <= 0.0 {
            return;
        }
        let total = self.carry_ms.get() + dt * 1000.0;
        // Whole milliseconds advance the runner; the remainder carries.
        let whole = total.floor();
        self.carry_ms.set(total - whole);
        if whole >= 1.0 {
            // `whole` is a non-negative, floored f32 < u32::MAX for any sane
            // frame dt; the cast is exact.
            let _ = self.state.tick(whole as u32);
        }
    }

    fn is_at_rest(&self, _epsilon: f32) -> bool {
        // At rest whenever no time-driven state is evolving — a fully-revealed
        // line awaiting the player, an untimed / resolved choice, or the End.
        // The shell's `any_animation_active` reads this to release the
        // continuous frame loop until the next player action re-arms it.
        !self.state.is_animating()
    }
}

/// Retrieve (registering once) the `Rc<VnClock>` for `key` from the current
/// [`Owner`] scope, wired into the animation driver — the `CaretBlink`
/// registration pattern. Call from a view fn; the same `Rc` resolves across
/// re-runs and the driver is registered only on first construction.
///
/// # Panics
///
/// Panics if called outside an active `Owner` scope.
#[must_use]
pub fn use_vn_clock(key: &'static str, state: Rc<VnState>) -> Rc<VnClock> {
    Owner::current()
        .expect("use_vn_clock requires an active Owner scope")
        .register_animation_once(key, || VnClock::new(state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vn::model::{VnOption, VnScript, VnStep};
    use pinion_core::reactive::Owner;

    fn script() -> VnScript {
        VnScript::new(vec![
            VnStep::line("무녀", "돌아오지 마라"), // 7 chars
            VnStep::timed_choice(
                "부름",
                vec![
                    VnOption::new("돌아본다", "turn"),
                    VnOption::new("버틴다", "endure"),
                ],
                4000,
                1,
            ),
        ])
    }

    #[test]
    fn clock_advances_reveal_on_wall_dt_with_carry() {
        let owner = Owner::new();
        owner.run(|| {
            let state = Rc::new(VnState::new(script()));
            let clock = VnClock::new(state.clone());
            assert!(!clock.is_at_rest(0.01), "mid-reveal is not at rest");
            // 40 cps -> 0.05 s = 2 chars; two 0.05 s frames = 4 chars (carry
            // accumulates the exact 0.1 s, no rounding loss).
            clock.tick(0.05);
            assert_eq!(state.revealed_chars(), 2);
            clock.tick(0.05);
            assert_eq!(state.revealed_chars(), 4);
        });
    }

    #[test]
    fn clock_is_at_rest_on_a_fully_revealed_line_and_at_end() {
        let owner = Owner::new();
        owner.run(|| {
            let state = Rc::new(VnState::new(script()));
            let clock = VnClock::new(state.clone());
            assert!(state.advance()); // snap the line to full
            assert!(
                clock.is_at_rest(0.01),
                "a fully-revealed line waits for the player"
            );
            // A live countdown is NOT at rest.
            assert!(state.advance()); // step onto the choice
            assert!(!clock.is_at_rest(0.01), "a live countdown is animating");
            // Drain it to timeout -> resolved -> onto the next line (End here).
            clock.tick(5.0);
            assert!(clock.is_at_rest(0.01), "resolved + at End is at rest");
        });
    }

    #[test]
    fn negative_or_nan_dt_is_ignored() {
        let owner = Owner::new();
        owner.run(|| {
            let state = Rc::new(VnState::new(script()));
            let clock = VnClock::new(state.clone());
            clock.tick(-1.0);
            clock.tick(f32::NAN);
            assert_eq!(state.revealed_chars(), 0, "no advance on bad dt");
        });
    }

    #[test]
    fn use_vn_clock_registers_with_the_animation_system() {
        // Proves the wiring the shell relies on for real-time play: the
        // registered clock's `is_at_rest` reaches `Owner::any_animation_active`,
        // which is what arms the shell's continuous frame loop (the same
        // `register_animation_once` + `any_animation_active` path `CaretBlink`
        // rides). The shell's paint -> `tick_animations` -> this driver is the
        // proven substrate; this test locks the VN-side registration + rest
        // predicate without a window.
        let owner = Owner::new();
        let state = owner.run(|| {
            let s = Rc::new(VnState::new(script()));
            let _clock = use_vn_clock("vn.clock.test", s.clone());
            s
        });
        // Mid-reveal -> the clock is not at rest -> the frame loop stays armed.
        assert!(
            owner.any_animation_active(0.01),
            "a mid-reveal VN keeps requesting frames"
        );
        // Snap the line to full -> at rest -> the loop is released until the
        // next player action (advance / choose) re-arms it.
        assert!(state.advance());
        assert!(
            !owner.any_animation_active(0.01),
            "a fully-revealed line releases the continuous loop"
        );
    }
}
