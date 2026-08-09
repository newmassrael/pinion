#!/usr/bin/env python3
"""R1549 §5.35 §5.38 §5.12 — press-and-hold AUTO-REPEAT.

Before this round, holding a spin arrow stepped the value exactly ONCE, no
matter how long the finger stayed down: the tree contained no repeat timer
of any kind (censused — the single `auto_repeat` hit in the whole
repository was *keyboard* auto-repeat SUPPRESSION, R1071). The toolkit has had
`setAutoRepeat` since the toolkit 1, abstract spin box repeats its
arrows by default, and every professional table, editor and DCC tool in
existence keeps stepping while you hold. This closes it.

The shape: a widget DECLARES a cadence
(`External::auto_repeat -> Option<AutoRepeat>`) and the router supplies the
clock. A fire re-dispatches the widget's own activation arc (`PointerUp`
then `PointerDown` — the toolkit's `released(); clicked(); pressed();` in statechart
vocabulary), so a repeat is a click by the same derivation a finger's click
is, and no widget needed a new SCXML transition to become repeatable.

Three things this proves that the toolkit 6.11 cannot answer:

  1. THE HOLD IS DRIVABLE AS DATA. The toolkit's repeat is a basic timer on the
     event loop: a test sleeps, and there is no API by which a client can
     say "hold this for 900 ms". Here the hold rides the same clock
     `scene/tick` drives, so this demo asserts an EXACT fire count at each
     step, with no wall clock and no tolerance.

  2. THE RUN IS PUBLISHED. `scene/auto_repeat` reports every in-flight
     press: its target, whether it is repeating, the declared cadence, the
     fires so far, and how long until the next one. The toolkit keeps all of it in
     a private basic timer inside abstract button private; the only
     public fact is the static `autoRepeat()` property of a widget you
     already hold a pointer to. The census here is PREDICTIVE — this demo
     reads `next_fire_in_secs`, ticks exactly that, and asserts a repeat
     landed.

  3. A HELD ARROW THAT CANNOT MOVE STOPS. abstract spin box keeps its
     10 Hz timer running against a value pinned at `maximum()` for as long
     as the user holds. Here the widget answers `None` — it reads its own
     bound — so the repeat goes quiet AND the frame loop is released.

And the structural claim, asserted directly: a repeat cannot outlive its
press. The run lives IN the router's press record, which is created by
`pointer_down` and removed by `pointer_up`, so there is no separate armed
flag to leave set. The toolkit's runaway-button bug class has nowhere to live.

Determinism note: `set_fps(0)` freezes the window's wall-clock contribution
(the R829/R831 frame-step contract), so from that call on the hold advances
by exactly the seconds `scene/tick` injects and every count below is an
equality, not a tolerance.

Run from the workspace root:
    cargo build -p hello-spinbutton --release
    python3 tools/demos/r1549_held_press_repeats.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

EXAMPLE = "hello-spinbutton"
VIEWPORT = (320, 160)
TAG = "spin"
DEC = "spin#dec"
INC = "spin#inc"

# `SpinButtonExternal`'s default cadence == `AutoRepeat::desktop()` == the toolkit's `AUTO_REPEAT_DELAY` / `AUTO_REPEAT_INTERVAL`
# (qabstractbutton.cpp).
DELAY = 0.300
INTERVAL = 0.100
# Comfortably inside a float step, and never equal to a fire instant: the
# guarantee is "one fire per threshold CROSSED", so a demo must tick past
# an instant rather than exactly onto it.
NUDGE = 0.020


def near(a: float, b: float, tol: float = 1e-4) -> bool:
    """`f32` cadence values widen to `f64` on the wire (0.3f32 reads back as
    0.30000001192...), so every seconds-valued assertion here compares to a
    tolerance well inside one frame."""
    return abs(a - b) < tol


def val(tf) -> float:
    return tf.query("/external/value")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def advance(tf, dt: float) -> None:
    """Inject exactly `dt` simulated seconds into the hold.

    Synchronous: the shell drains its deferred-input inbox before this
    request returns, so the repeats this crosses have already fired by the
    time the next call is served. No paint to wait for, no wall clock to
    tolerate — which is the whole point.
    """
    tf.tick(dt)


def holds(tf) -> list:
    return tf.request("scene/auto_repeat", {}).result["holds"]


def hold_down(tf, path: str) -> None:
    tf.pointer_button("left", "down", path=path)


def release(tf, path: str) -> None:
    tf.pointer_button("left", "up", path=path)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # `set_fps(0)` freezes the wall clock: from here the hold advances
        # by exactly what `tick` injects, so every count below is exact.
        tf.set_fps(0)

        snap = paint(tf)
        assert find_by_tag(snap, INC) is not None, "the increment arrow is painted"
        assert_eq(val(tf), 3.0, "boot value = 3")

        # ── the method exists and is discoverable ────────────────────────
        methods = tf.request("rpc/methods", {}).result["methods"]
        names = [m["name"] for m in methods]
        assert "scene/auto_repeat" in names, "scene/auto_repeat is enumerable"
        entry = next(m for m in methods if m["name"] == "scene/auto_repeat")
        assert_eq(entry["occ"], "read", "reading a hold mutates nothing")
        schema = tf.request("rpc/schema", {}).result["types"]
        shapes = {t["name"] for t in schema}
        assert "AutoRepeatOutcome" in shapes, "the response shape is published"
        assert "AutoRepeatHoldOutcome" in shapes, "and so is one hold's"

        # ── nothing held: an empty list, not an error ────────────────────
        assert_eq(holds(tf), [], "no press in flight")

        # ── press and hold the + arrow ───────────────────────────────────
        hold_down(tf, INC)
        assert_eq(tf.query("/external/inc_state"), "Pressed", "the arrow is down")
        assert_eq(val(tf), 3.0, "a press is not yet a step")

        h = holds(tf)
        assert_eq(len(h), 1, "exactly one press is in flight")
        assert_eq(h[0]["target"], INC, "and it names the arrow it landed on")
        assert_eq(h[0]["repeating"], True, "which declares a cadence")
        assert_eq(h[0]["fires"], 0, "no repeat yet")
        # The cadence values are `f32` widened to `f64` on the wire, so they
        # are compared to a tolerance, not for bit equality.
        assert near(h[0]["delay_secs"], DELAY), "the toolkit AUTO_REPEAT_DELAY (300 ms)"
        assert near(h[0]["interval_secs"], INTERVAL), "the toolkit AUTO_REPEAT_INTERVAL (100 ms)"
        assert near(h[0]["accel"], 1.0), "un-accelerated, as push button is"
        assert near(h[0]["min_interval_secs"], INTERVAL), "no ramp, so no floor below it"
        assert near(h[0]["next_fire_in_secs"], DELAY), (
            f"the whole delay is still ahead: {h[0]['next_fire_in_secs']}"
        )

        # ── the delay is real: just short of it, nothing happens ─────────
        advance(tf, DELAY - NUDGE)
        assert_eq(val(tf), 3.0, "0.28s held: still no repeat")
        h = holds(tf)
        assert_eq(h[0]["fires"], 0, "and the census agrees")
        assert near(h[0]["next_fire_in_secs"], NUDGE), (
            "the census counts down toward the first repeat"
        )

        # ── PREDICTIVE: tick exactly what the wire said, plus a nudge ────
        advance(tf, h[0]["next_fire_in_secs"] + NUDGE)
        assert_eq(val(tf), 4.0, "the delay elapsed: 3 -> 4")
        h = holds(tf)
        assert_eq(h[0]["fires"], 1, "one repeat fired")
        assert near(h[0]["next_fire_in_secs"], INTERVAL - NUDGE), (
            "and the schedule moved on to the interval"
        )

        # ── then one per interval, exactly ───────────────────────────────
        advance(tf, INTERVAL)
        assert_eq(val(tf), 5.0, "one interval, one step")
        advance(tf, INTERVAL)
        assert_eq(val(tf), 6.0, "and again")
        assert_eq(holds(tf)[0]["fires"], 3, "three repeats so far")

        # ── THE structural claim: the release ends it, with no un-arm ────
        release(tf, INC)
        assert_eq(val(tf), 7.0, "the release is itself an activation")
        assert_eq(holds(tf), [], "the press record is gone")
        for _ in range(5):
            advance(tf, 1.0)
        assert_eq(val(tf), 7.0, "five seconds after release: nothing fired")
        assert_eq(holds(tf), [], "and nothing came back")

        # ── PAST THE FLOOR: a held arrow at its bound stops repeating
        # ───────────
        tf.pointer_leave()
        hold_down(tf, INC)
        # One big tick would reach 10 and, in the toolkit, keep firing at 10 Hz
        # forever. Here it stops at the bound — the widget reads its own
        # range, and the router re-asks before every single fire, so the
        # catch-up inside ONE tick cannot overshoot either.
        advance(tf, 5.0)
        assert_eq(val(tf), 10.0, "it walked to the maximum and stopped there")
        h = holds(tf)
        assert_eq(len(h), 1, "the finger is still down")
        assert_eq(h[0]["target"], INC, "on the same arrow")
        assert_eq(tf.query("/external/inc_state"), "Pressed", "and it is still Pressed")
        assert_eq(h[0]["repeating"], False, "but there is nowhere left to step")
        assert "delay_secs" not in h[0], "so no cadence is reported at all"
        assert "next_fire_in_secs" not in h[0], "and nothing is scheduled"
        fires_at_bound = h[0]["fires"]
        advance(tf, 5.0)
        assert_eq(val(tf), 10.0, "ten more seconds held: still 10")
        assert_eq(holds(tf)[0]["fires"], fires_at_bound, "and not one more fire")
        release(tf, INC)
        tf.pointer_leave()

        # ── the opposite arrow is live from the same position ────────────
        assert_eq(val(tf), 10.0, "released at the ceiling")
        hold_down(tf, DEC)
        assert_eq(holds(tf)[0]["target"], DEC, "now holding the - arrow")
        assert_eq(holds(tf)[0]["repeating"], True, "down from the ceiling still steps")
        advance(tf, DELAY + NUDGE)
        assert_eq(val(tf), 9.0, "9 after the first repeat")
        release(tf, DEC)
        tf.pointer_leave()

        # ── a press that strays keeps its identity and stops repeating ───
        assert_eq(val(tf), 8.0, "the release stepped once more")
        hold_down(tf, DEC)
        assert_eq(holds(tf)[0]["repeating"], True, "repeating on -")
        tf.hover(path=INC)  # the cursor slides across to the other arrow
        h = holds(tf)
        assert_eq(len(h), 1, "the press is still in flight")
        assert_eq(h[0]["target"], DEC, "and still names the arrow it LANDED on")
        assert_eq(h[0]["repeating"], False, "which is no longer Pressed, so it is quiet")
        before = val(tf)
        advance(tf, 5.0)
        assert_eq(val(tf), before, "five seconds strayed: nothing stepped")
        release(tf, INC)
        tf.pointer_leave()

        # ── a press on a NON-repeating target is still reported ──────────
        # The readout falls through to the `spin` container: a real press,
        # on a widget with no cadence. Reporting it is what keeps
        # "held, and nothing will come of it" distinct from "not held".
        hold_down(tf, TAG)
        h = holds(tf)
        assert_eq(len(h), 1, "a non-repeating press is a press")
        assert_eq(h[0]["target"], TAG, "named honestly")
        assert_eq(h[0]["repeating"], False, "and reported as quiet")
        assert "interval_secs" not in h[0], "with no cadence invented for it"
        held = val(tf)
        advance(tf, 5.0)
        assert_eq(val(tf), held, "five seconds on the readout: no steps")
        release(tf, TAG)
        tf.pointer_leave()

        # ── DETERMINISM: one big tick == many small frames ─────────────── The
        # property the toolkit's wall-clock basic timer cannot offer, and the
        # reason the hold rides the same clock the paint does.
        tf.intervene("/external/value", 0.0)
        assert_eq(val(tf), 0.0, "reset to the floor")
        hold_down(tf, INC)
        advance(tf, DELAY + 4 * INTERVAL + NUDGE)  # 1 delay + 4 intervals
        one_big = val(tf)
        assert_eq(one_big, 5.0, "one 0.72s tick = 5 steps")
        release(tf, INC)
        tf.pointer_leave()

        tf.intervene("/external/value", 0.0)
        hold_down(tf, INC)
        for _ in range(44):  # 44 x 1/60 s = 0.7333s, past the same 5 instants
            advance(tf, 1.0 / 60.0)
        assert_eq(val(tf), one_big, "and 44 sixtieths of a second = the same 5")
        release(tf, INC)


if __name__ == "__main__":
    run_demo("r1549 press-and-hold auto-repeat", body)
