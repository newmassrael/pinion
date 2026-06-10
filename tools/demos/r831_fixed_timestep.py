#!/usr/bin/env python3
"""R831 §2 #4 §5.28 — fixed-timestep accumulator game loop.

R831 routes BOTH the live wall-clock advance and the `scene/tick`
injected advance of the immediate-mode game loop through ONE per-window
fixed-timestep accumulator (Glenn Fiedler, "Fix Your Timestep!"). The
simulation now advances in EXACTLY 1/120 s steps with the sub-step
remainder carried across frames, instead of the pre-R831 split (live ran
one variable wall-clock step; `scene/tick` ran a partial-final-step
splitter — two time bases that could not reproduce each other).

The accumulator's defining, RPC-observable behaviour — the thing the old
splitter did NOT do — is the CARRY:

  (A) PAUSE RESETS THE PHASE — `set_fps 0` snaps the accumulator to a
      zero phase and freezes the clock (no tick → no motion), so frame
      stepping starts from a known fixed-step boundary.

  (B) SUB-FIXED TICK DOES NOT MOVE — a `tick(dt)` with `dt` SMALLER than
      one fixed step (here 0.005 s < 1/120 s ≈ 0.00833 s) releases ZERO
      whole steps: the time is accumulated, not applied. The pre-R831
      splitter would have advanced the ball by a partial 0.005 s step.

  (C) TWO SUB-FIXED TICKS ACCUMULATE TO ONE STEP — a second 0.005 s tick
      brings the carried total over one fixed step, so the ball advances
      by exactly one step. This is the carry, observed end to end.

  (D) FAITHFUL ACCUMULATOR — injected dts with clear fractional step
      margins advance the ball to the position a Python mirror of the
      accumulator predicts (the exact fixed-step math is pinned by the
      Rust unit tests, frame_pacing::r831_*).

  (E) RESUME — `set_fps 60` restarts the continuous loop on wall-clock.

All reads go through the R828 introspect channel
(`scene/query "/ball/external/{pos,velocity,bounces}"`). >= 30
assertions.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    run_demo,
    wait_paint_beyond,
    wait_until,
)

_WIN_W = 340
_WIN_H = 360
_BALL = "ball"
_BALL_SPEED = 2.5  # mirrors BouncingBallDriver::BALL_SPEED (magnitude)
_SUBSTEP = 1.0 / 120.0  # mirrors FIXED_TIMESTEP_SUBSTEP_SECS (shell SSOT)
_SUB_FIXED = 0.005  # < _SUBSTEP (0.00833): a sub-fixed delta


def _pos(tf: RpcSubprocess) -> float:
    return float(tf.query(f"/{_BALL}/external/pos"))


def _vel(tf: RpcSubprocess) -> float:
    return float(tf.query(f"/{_BALL}/external/velocity"))


def _bounces(tf: RpcSubprocess) -> int:
    v = tf.query(f"/{_BALL}/external/bounces")
    assert isinstance(v, int), f"bounces must be int; got {v!r}"
    return v


def _accumulate(
    pos: float, vel: float, leftover: float, dt: float
) -> tuple[float, float, float, int]:
    """Mirror of the R831 shell `FixedTimestep` accumulator + the
    BouncingBallDriver::tick. Adds `dt` to the carried `leftover`, then
    steps in EXACTLY `_SUBSTEP` increments, carrying the new remainder.
    Returns (pos, vel, leftover, bounce_delta)."""
    leftover += dt
    bounces = 0
    while leftover >= _SUBSTEP:
        pos += vel * _SUBSTEP
        while not (0.0 <= pos <= 1.0):
            if pos < 0.0:
                pos = -pos
                vel = abs(vel)
            else:
                pos = 2.0 - pos
                vel = -abs(vel)
            bounces += 1
        leftover -= _SUBSTEP
    return pos, vel, leftover, bounces


def _settle_frozen(tf: RpcSubprocess) -> tuple[float, int]:
    """Wait for the paint clock to freeze after a pause, returning the
    settled (pos, bounces)."""
    def _frozen_pair() -> bool:
        a = (_pos(tf), _bounces(tf))
        # wall-clock semantic: frozen-clock proof needs two samples
        # spaced by a real-time window (absence of motion has no
        # positive observable); the outer wait_until retries.
        time.sleep(0.1)
        b = (_pos(tf), _bounces(tf))
        return a == b
    wait_until(_frozen_pair, timeout=6.0, interval=0.05,
               desc="paint clock settles to frozen after set_fps(0)")
    return _pos(tf), _bounces(tf)


def _tick_expect_motion(tf: RpcSubprocess, dt: float, pre_pos: float,
                        pre_b: int) -> None:
    """Inject a tick that should release >= 1 fixed step and poll until
    the single redraw it triggers lands the resulting motion."""
    tf.tick(dt)
    wait_until(
        lambda: abs(_pos(tf) - pre_pos) > 1e-7 or _bounces(tf) != pre_b,
        timeout=6.0,
        interval=0.04,
        desc=f"tick(dt={dt}) advances the frozen driver",
    )


def _tick_expect_frozen(tf: RpcSubprocess, dt: float) -> None:
    """Inject a SUB-FIXED tick that should release ZERO steps. The tick
    still requests one repaint (which accumulates the time); gate on the
    paint counter so that repaint has LANDED before the caller asserts
    no motion (R883 zero-flake)."""
    before = int(tf.cache_stats()["paint_count"])
    tf.tick(dt)
    wait_paint_beyond(tf, before, desc="sub-fixed tick's repaint landed")


def body() -> None:
    with RpcSubprocess("hello-immediate-intent", boot_grace=1.5) as tf:
        snap = tf.snapshot("", source="paint", viewport=(_WIN_W, _WIN_H))
        assert find_by_tag(snap, _BALL) is not None, "ball node addressable"
        # Live: the ball bounces on wall-clock before we take the clock.
        live0 = _bounces(tf)
        wait_until(lambda: _bounces(tf) > live0, timeout=8.0, interval=0.05,
                   desc="ball bounces while the continuous loop runs")
        assert _bounces(tf) > live0

        # ── (A) PAUSE RESETS THE PHASE + FREEZES ─────────────────────
        tf.set_fps(0)
        p0, b0 = _settle_frozen(tf)
        v0 = _vel(tf)
        assert abs(abs(v0) - _BALL_SPEED) < 1e-3, (
            f"speed magnitude is invariant under reflection; saw {abs(v0)}"
        )
        # wall-clock semantic: a real-time window with NO tick — the
        # paused-clock no-drift proof needs wall time to elapse; there
        # is no positive observable for "nothing happened".
        time.sleep(0.4)
        assert abs(_pos(tf) - p0) < 1e-6, "paused pos must not drift"
        assert _bounces(tf) == b0, "paused bounces must not change"
        assert abs(_vel(tf) - v0) < 1e-9, "paused velocity must not change"

        # ── (B) SUB-FIXED TICK DOES NOT MOVE (the carry) ─────────────
        # set_fps(0) reset the accumulator to leftover=0, so a single
        # sub-fixed delta (< one fixed step) releases ZERO whole steps:
        # the ball stays put. The pre-R831 splitter would have advanced
        # it by a partial step here.
        pre_pos, pre_vel, pre_b = _pos(tf), _vel(tf), _bounces(tf)
        _tick_expect_frozen(tf, _SUB_FIXED)
        assert abs(_pos(tf) - pre_pos) < 1e-6, (
            f"sub-fixed tick({_SUB_FIXED}) must release no step: "
            f"{pre_pos} -> {_pos(tf)}"
        )
        assert _bounces(tf) == pre_b, "sub-fixed tick must not bounce"
        assert abs(_vel(tf) - pre_vel) < 1e-9, "sub-fixed tick must not flip vel"

        # ── (C) TWO SUB-FIXED TICKS ACCUMULATE TO ONE STEP ───────────
        # leftover is now ~0.005; another 0.005 brings it over 0.00833,
        # so exactly one fixed step fires and the ball moves.
        pre_pos, pre_b = _pos(tf), _bounces(tf)
        _tick_expect_motion(tf, _SUB_FIXED, pre_pos, pre_b)
        moved = abs(_pos(tf) - pre_pos) > 1e-7 or _bounces(tf) != pre_b
        assert moved, (
            "two accumulated sub-fixed ticks must release exactly one step "
            f"(carry): pos {pre_pos} -> {_pos(tf)}, bounces {pre_b} -> "
            f"{_bounces(tf)}"
        )
        # A net single-step displacement is bounded by |vel| * _SUBSTEP
        # (≈0.021); allow slack for a possible mid-step wall reflection.
        if _bounces(tf) == pre_b:
            assert abs(_pos(tf) - pre_pos) < 0.03, (
                "one step advances by at most |vel|*fixed_step"
            )

        # ── (D) FAITHFUL ACCUMULATOR (mirror-matched) ────────────────
        # Re-pause to reset the phase (leftover=0), then inject dts with
        # clear fractional step margins and match the Python mirror.
        tf.set_fps(0)
        _settle_frozen(tf)
        leftover = 0.0
        for dt in (0.055, 0.21, 0.503):
            pre_pos, pre_vel, pre_b = _pos(tf), _vel(tf), _bounces(tf)
            exp_pos, _exp_vel, leftover, exp_db = _accumulate(
                pre_pos, pre_vel, leftover, dt
            )
            _tick_expect_motion(tf, dt, pre_pos, pre_b)
            got_pos, got_b = _pos(tf), _bounces(tf)
            assert abs(got_pos - exp_pos) < 0.04, (
                f"tick(dt={dt}): pos {got_pos:.4f} != mirror {exp_pos:.4f} "
                f"(from pos={pre_pos:.4f} vel={pre_vel:.4f})"
            )
            assert (got_b - pre_b) == exp_db, (
                f"tick(dt={dt}): bounce delta {got_b - pre_b} != "
                f"mirror {exp_db}"
            )
            assert 0.0 <= got_pos <= 1.0, f"ball stays in-track; saw {got_pos}"
            assert abs(abs(_vel(tf)) - _BALL_SPEED) < 1e-3, (
                "speed magnitude conserved across the step's reflections"
            )
            # Refreezes after the step until the next tick.
            stable_pos, stable_b = _pos(tf), _bounces(tf)
            assert abs(stable_pos - got_pos) < 1e-6 and stable_b == got_b, (
                "driver must refreeze after a step"
            )

        # ── (E) RESUME ───────────────────────────────────────────────
        resume_b = _bounces(tf)
        tf.set_fps(60)
        wait_until(lambda: _bounces(tf) > resume_b, timeout=8.0, interval=0.05,
                   desc="ball resumes bouncing on wall-clock after set_fps(60)")
        assert _bounces(tf) > resume_b


if __name__ == "__main__":
    sys.exit(run_demo("hello-immediate-intent R831 §2 #4 §5.28 fixed-timestep", body))
