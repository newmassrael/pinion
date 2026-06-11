#!/usr/bin/env python3
"""R888 §5.49 §5.28 — `scene/pacing_state`: the READ peer of `scene/set_fps`.

R887's entry self-grep (read-write wire-symmetry axis) found the §2 #4
frame-pacing target was a write-only wire: `scene/set_fps` installs a
per-window override, but nothing could read it back — a fresh AI session
attaching to a running app could not tell whether the loop was paused,
throttled, or on the default policy (§2 invariant #2: every state an
input write mutates must be AI-readable; the R885 input-cache cleanup's
sibling).

Designing the READ also exposed a write-side gap: the override map was
insert-only, so the *boot* state (no override — the adaptive default
policy: 60fps while immediate-mode content is active, idle otherwise)
was unreachable once any set landed. R888 therefore also gives
`scene/set_fps` a `{"fps": null}` form = clear the override, making the
axis fully round-trippable: read mirrors write over the same `fps`
field ([[wire-form-read-write-symmetry]]).

Verification scope (31 assertions, counted exactly; gates per
[[zero-flake-policy]] — action→assert edges poll observed state):

  (A) boot — pacing_state reads {"fps": null} (default policy), and
      the response carries exactly the one mirrored field.            (3)
  (B) override round-trip — set 30 → read 30; set 144 → read 144;
      paused set 0 → read 0.                                          (6)
  (C) paused semantics cross-check — while fps=0 reads back, the
      simulation is frozen without ticks and `tick(dt)` still
      frame-steps it (the read peer reports the SAME pause the §2 #4
      clock honours, not a parallel flag).                            (8)
  (D) clear — set null → read null (boot state wire-reachable
      again); the loop resumes on the adaptive default policy
      (immediate content active → sim advances on wall clock).        (5)
  (E) wire edges — set_fps {} missing fps rejected, "x"/-1/1.5
      rejected, all side-effect-free (read unchanged); pacing_state
      is read-only (back-to-back reads identical).                    (9)
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

_BALL = "ball"


def _pos(tf: RpcSubprocess) -> float:
    return float(tf.query(f"/{_BALL}/external/pos"))


def _bounces(tf: RpcSubprocess) -> int:
    return int(tf.query(f"/{_BALL}/external/bounces"))


def body() -> None:
    with RpcSubprocess("hello-immediate-intent", boot_grace=1.5) as tf:
        # ── (A) boot: default policy reads as fps null ───────────────
        r0 = tf.pacing_state()
        assert_eq(sorted(r0.keys()), ["fps"], "response carries exactly the fps axis")  # 1
        assert_eq(r0["fps"], None, "boot: no override -> default policy (null)")        # 2
        assert_eq(tf.pacing_state(), r0, "read is stable (no hidden state)")            # 3

        # ── (B) override round-trip ──────────────────────────────────
        tf.set_fps(30)
        wait_until(lambda: tf.pacing_state()["fps"] == 30,
                   desc="set 30 reads back 30")                                         # 4
        assert_eq(tf.pacing_state()["fps"], 30, "override 30 installed")                # 5
        tf.set_fps(144)
        wait_until(lambda: tf.pacing_state()["fps"] == 144,
                   desc="re-set 144 reads back 144")                                    # 6
        assert_eq(tf.pacing_state()["fps"], 144, "override replaces, not stacks")       # 7
        tf.set_fps(0)
        wait_until(lambda: tf.pacing_state()["fps"] == 0,
                   desc="pause (0) reads back 0")                                       # 8
        assert_eq(tf.pacing_state()["fps"], 0, "paused frame-step mode reads 0")        # 9

        # ── (C) the read reports the SAME pause the clock honours ────
        # Paint clock settles to frozen after set_fps(0): two identical
        # consecutive (pos, bounces) pairs = no wall-clock advance.
        def _frozen_pair():
            a = (_pos(tf), _bounces(tf))
            time.sleep(0.05)  # wall-clock semantic: sampling window
            b = (_pos(tf), _bounces(tf))
            return a if a == b else None

        frozen = wait_until(_frozen_pair, timeout=6.0, interval=0.05,
                            desc="paint clock settles frozen after set_fps(0)")         # 10
        pos0, bounces0 = frozen
        # wall-clock semantic: a real-time window with NO tick — the
        # sim must hold still while paused.
        time.sleep(0.30)
        assert_eq(_pos(tf), pos0, "paused: pos frozen over wall clock")                 # 11
        assert_eq(_bounces(tf), bounces0, "paused: bounces frozen")                     # 12
        assert_eq(tf.pacing_state()["fps"], 0, "read still says paused")                # 13
        tf.tick(0.5)
        wait_until(lambda: _pos(tf) != pos0,
                   desc="tick(dt) frame-steps the paused sim")                          # 14
        pos1 = _pos(tf)
        assert pos1 != pos0, "explicit tick advanced the sim while paused"              # 15
        time.sleep(0.20)  # wall-clock semantic: still paused after the tick
        assert_eq(_pos(tf), pos1, "after the tick the sim freezes again")               # 16
        assert_eq(tf.pacing_state()["fps"], 0, "tick does not mutate the pacing axis")  # 17

        # ── (D) null write clears the override (boot state reachable) ─
        tf.set_fps(None)
        wait_until(lambda: tf.pacing_state()["fps"] is None,
                   desc="null write clears the override")                               # 18
        assert_eq(tf.pacing_state()["fps"], None, "default policy restored")            # 19
        # Adaptive default with live immediate-mode content = the loop
        # runs again: the sim advances on wall clock without any tick.
        wait_until(lambda: _pos(tf) != pos1, timeout=8.0,
                   desc="default policy resumes the continuous loop")                   # 20
        assert tf.pacing_state()["fps"] is None, "resumed loop is policy, not override" # 21
        r_now = tf.pacing_state()
        assert_eq(sorted(r_now.keys()), ["fps"], "shape stable across the whole axis")  # 22

        # ── (E) wire edges, all side-effect-free ─────────────────────
        before = tf.pacing_state()
        for bad_params in ({}, {"fps": "x"}, {"fps": -1}, {"fps": 1.5}):
            try:
                tf.request("scene/set_fps", bad_params)
                raise AssertionError(f"set_fps {bad_params} must be rejected")
            except RpcError as err:
                assert_eq(err.code, -32602, f"set_fps {bad_params} -> invalid params")  # 23,24,25,26
        assert_eq(tf.pacing_state(), before, "rejected writes mutate nothing")          # 27
        missing_hint = None
        try:
            tf.request("scene/set_fps", {})
        except RpcError as err:
            missing_hint = str(err.data)
        assert missing_hint is not None and "null" in missing_hint, (
            f"missing-fps error teaches the null clear form: {missing_hint!r}"
        )                                                                               # 28
        # Read-only contract: two reads with interleaved queries agree.
        a = tf.pacing_state()
        _ = tf.query(f"/{_BALL}/external/pos")
        b = tf.pacing_state()
        assert_eq(a, b, "pacing_state is a pure read")                                  # 29
        assert_eq(a["fps"], None, "axis still on default policy at exit")               # 30
        # And the write peer still works after the rejection volley.
        tf.set_fps(60)
        wait_until(lambda: tf.pacing_state()["fps"] == 60,
                   desc="axis still writable after rejections")                         # 31


if __name__ == "__main__":
    sys.exit(run_demo("R888 §5.49 §5.28 — scene/pacing_state READ peer of set_fps", body))
