#!/usr/bin/env python3
"""R829 §2 #4 §5.28 — deterministic immediate-mode frame stepping.

The tick-control peer of R827 (emit) + R828 (query): an AI client now
*drives the clock* of the immediate-mode game loop deterministically.
`scene/set_fps 0` pauses a window's continuous paint clock (so wall-clock
paints stop advancing the simulation); `scene/tick { dt }` then advances
the immediate-mode drivers by EXACTLY `dt` (substepped through the shared
fixed-timestep for integrator stability). This is the frame-step
debugging every game engine offers, surfaced as an RPC primitive — a
developer can pause/step a render loop in a debugger, and per §2 #2
("every input a human makes has an RPC peer") an AI client now can too.

Determinism is proven two ways, both read through the R828 introspect
channel (`scene/query "/ball/external/{pos,velocity,bounces}"`):

  (A) FROZEN WHILE PAUSED — after `set_fps 0`, pos / bounces do not
      change over a wall-clock window WITHOUT a tick. The continuous
      loop is genuinely quiescent; the AI owns the clock. (The headline
      determinism proof — no tick, no motion.)

  (B) ADVANCES EXACTLY ON TICK — `tick(dt)` advances pos / bounces by
      exactly the amount a substep-accurate Python mirror of the driver
      predicts from the queried pre-tick pos + velocity. Repeatable:
      back-to-back ticks each advance by their own dt and nothing else.

  (C) RESUME — `set_fps 60` restarts the continuous loop; the ball moves
      again on wall-clock.

Full assertion count >= 30.
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
    wait_until,
)

_WIN_W = 340
_WIN_H = 360
_BALL = "ball"
_BALL_SPEED = 2.5
_SUBSTEP = 1.0 / 120.0  # mirrors FIXED_TIMESTEP_SUBSTEP_SECS (shell SSOT)


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
    steps in EXACTLY `_SUBSTEP` increments (carrying the new remainder),
    NOT the pre-R831 partial-final-step splitter. Returns
    (pos, vel, leftover, bounce_delta)."""
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


def body() -> None:
    with RpcSubprocess("hello-immediate-intent", boot_grace=1.5) as tf:
        # Sanity: the ball node is addressable and the loop is live.
        snap = tf.snapshot("", source="paint", viewport=(_WIN_W, _WIN_H))
        assert find_by_tag(snap, _BALL) is not None, "ball node addressable"
        # Live (unpaused): bounces climb on wall-clock.
        live0 = _bounces(tf)
        wait_until(lambda: _bounces(tf) > live0, timeout=8.0, interval=0.05,
                   desc="ball bounces while the continuous loop runs")
        assert _bounces(tf) > live0

        # ── (A) Pause, then FROZEN WHILE PAUSED ──────────────────────
        tf.set_fps(0)
        # Let any in-flight paints drain, then confirm the clock froze:
        # two reads spaced apart must be identical (no wall-clock motion).
        def _frozen_pair() -> bool:
            a = (_pos(tf), _bounces(tf))
            # wall-clock semantic: proving the sim clock FROZE needs two
            # samples spaced by a real-time window (absence of motion has
            # no positive observable to gate on); the outer wait_until
            # keeps retrying until the pair stabilises.
            time.sleep(0.12)
            b = (_pos(tf), _bounces(tf))
            return a == b
        wait_until(_frozen_pair, timeout=6.0, interval=0.05,
                   desc="paint clock settles to frozen after set_fps(0)")
        p0, b0 = _pos(tf), _bounces(tf)
        # wall-clock semantic: a real-time window with NO tick — the
        # paused-clock no-drift proof needs wall time to elapse; there
        # is no positive observable for "nothing happened".
        time.sleep(0.5)
        p1, b1 = _pos(tf), _bounces(tf)
        assert abs(p1 - p0) < 1e-6, f"paused pos must not drift: {p0} -> {p1}"
        assert b1 == b0, f"paused bounces must not change: {b0} -> {b1}"

        # ── (B) ADVANCES EXACTLY ON TICK ─────────────────────────────
        # set_fps(0) reset the shell's fixed-timestep accumulator to a
        # zero phase (R831 deterministic-debugging contract), so a mirror
        # seeded with leftover=0 here tracks it. Each `tick(dt)` injects
        # `dt` of simulation time; the accumulator releases the whole
        # fixed steps it now owes and carries the sub-step remainder. The
        # dts have clear fractional step margins (e.g. 0.055 s = 6.6 fixed
        # steps) so the f32 (shell) and f64 (mirror) accumulators agree on
        # the step COUNT; the exact fixed-step math is pinned by the Rust
        # unit tests (frame_pacing::r831_*).
        leftover = 0.0
        for dt in (0.055, 0.13, 0.37, 1.005):
            pre_pos, pre_vel, pre_b = _pos(tf), _vel(tf), _bounces(tf)
            exp_pos, _exp_vel, leftover, exp_db = _accumulate(
                pre_pos, pre_vel, leftover, dt
            )
            tf.tick(dt)
            # The paused window repaints once on the tick's redraw; poll
            # until the new frame lands (pos or bounces moved).
            wait_until(
                lambda pb=pre_pos, bb=pre_b: (
                    abs(_pos(tf) - pb) > 1e-7 or _bounces(tf) != bb
                ),
                timeout=6.0,
                interval=0.04,
                desc=f"tick(dt={dt}) advances the frozen driver",
            )
            got_pos, got_b = _pos(tf), _bounces(tf)
            assert abs(got_pos - exp_pos) < 0.04, (
                f"tick(dt={dt}): pos {got_pos} != mirror {exp_pos:.4f} "
                f"(from pos={pre_pos:.4f} vel={pre_vel:.4f})"
            )
            assert (got_b - pre_b) == exp_db, (
                f"tick(dt={dt}): bounce delta {got_b - pre_b} != mirror {exp_db}"
            )
            # Stays frozen after the step until the next tick.
            still_pos, still_b = _pos(tf), _bounces(tf)
            assert abs(still_pos - got_pos) < 1e-6 and still_b == got_b, (
                "driver must refreeze after a step (no continuous advance)"
            )

        # ── (C) RESUME — continuous loop restarts ────────────────────
        resume_b = _bounces(tf)
        tf.set_fps(60)
        wait_until(lambda: _bounces(tf) > resume_b, timeout=8.0, interval=0.05,
                   desc="ball resumes bouncing on wall-clock after set_fps(60)")
        assert _bounces(tf) > resume_b


if __name__ == "__main__":
    sys.exit(run_demo("hello-immediate-intent R829 §2 #4 §5.28 deterministic stepping", body))
