#!/usr/bin/env python3
"""R1459+R1460 §5.16 §5.36 §5.45 §2 #6 — a frame reports its WORK, not only its duration.

`scene/frame_timings` has carried seven durations since R907. None of them can
answer two questions a pro-tool profile actually asks:

  - **how many passes?** `build_us` is the whole settle loop (R1458), so a
    frame that ran one heavy pass and a frame whose four cheap passes disagree
    are the same microseconds and want opposite fixes. And a frame that spends
    the whole budget without converging is, until now, visible only as a
    `tracing::warn!` no RPC client can read.
  - **how much shaping?** R1454 measured one shaper miss at 18.5us against a
    118ns hit and bounded the worst offender with Qt's
    `resizeContentsPrecision` — but that bound is CONSUMER-honoured. A binding
    that ignores it re-shapes everything every frame and nothing noticed.

This demo proves both counts mean "work this frame did" by contrasting two
bindings on the same wire:

  - `hello-tail-reveal` — static text. Idle repaints must read EXACTLY zero
    shaper misses and exactly one settle pass.
  - `hello-frame-profiler` — a HUD whose labels ARE the timings, so its text
    changes every frame. Its idle repaints must read shaper misses ABOVE zero,
    every single frame, forever.

R1460 adds the third section: work the RPC scene PRODUCER did. It settles a
scene exactly as a paint does but records no frame, so an agent on the §2 #2
path could not see the work its own calls caused. `produce` is cumulative and
is never folded into the frame ring — a produce is not a frame, and a pure read
(which produces nothing) is charged nothing.

That contrast is the whole claim. A counter that reported a lifetime total
would grow on the first binding; one that reported a constant would not
separate them; one that reported nothing is where we were.

ZERO-FLAKE: every frame is driven by the profiler's own `frame_count` gate (no
wall-clock sleeps), and no assertion names a microsecond value or a font
metric — only counts, their zero/non-zero character, and invariants.

Run from the workspace root:
    cargo build -p hello-tail-reveal -p hello-frame-profiler --release
    python3 tools/demos/r1460_frame_work.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

STATIC_TEXT_APP = "hello-tail-reveal"
LIVE_TEXT_APP = "hello-frame-profiler"

# The shell's settle budget (`pinion_runtime::SETTLE_PASS_BUDGET`). Mirrored so
# the demo can assert a real frame never exceeds it.
SETTLE_PASS_BUDGET = 4


def drive_frame(tf: RpcSubprocess, baseline: int, desc: str) -> None:
    """Drive real winit paints until `frame_count` passes `baseline`.

    A `scene/screenshot` forces `render_window`, which is the only path that
    records a timing sample — the RPC produce mirror deliberately does not, so
    an introspection read never manufactures a frame that the user never saw.
    """

    def advanced() -> bool:
        try:
            if int(tf.frame_timings()["frame_count"]) > baseline:
                return True
        except RpcError:
            pass
        tf.request("scene/screenshot", {"path": ""})
        return False

    wait_until(advanced, desc=desc)


def last(tf: RpcSubprocess) -> dict:
    return tf.frame_timings()["last"]


def assert_wire_shape(sample: dict, label: str) -> None:
    """The three R1459 fields are present, typed, and self-consistent."""
    for field in ("settle_passes", "shape_misses"):
        assert field in sample, f"{label}: `{field}` missing from the wire"
        assert isinstance(sample[field], int) and not isinstance(
            sample[field], bool
        ), f"{label}: `{field}` must be an integer count, got {sample[field]!r}"
    assert "settled" in sample, f"{label}: `settled` missing from the wire"
    assert isinstance(sample["settled"], bool), (
        f"{label}: `settled` must be a bool — it answers 'did it finish', "
        f"which no count can"
    )
    assert sample["settle_passes"] >= 1, (
        f"{label}: a recorded frame ran at least one pass; 0 is the "
        f"never-painted sentinel and must not reach a sample"
    )
    assert sample["settle_passes"] <= SETTLE_PASS_BUDGET, (
        f"{label}: a paint cannot exceed its own budget "
        f"({sample['settle_passes']} > {SETTLE_PASS_BUDGET})"
    )
    assert sample["shape_misses"] >= 0, f"{label}: counts are unsigned"


def body() -> None:
    # ── (A) a static-text binding: idle frames do NO work ───────────────────
    with RpcSubprocess(STATIC_TEXT_APP, boot_grace=1.5) as tf:
        boot = tf.frame_timings()
        assert_wire_shape(boot["last"], "static: boot")

        # The cold-start claim, with its precondition ASSERTED rather than
        # assumed: this must be the very first recorded frame, or "boot" below
        # would silently be describing some later frame.
        assert_eq(int(boot["frame_count"]), 1, "static: this is the boot frame")
        assert boot["last"]["shape_misses"] > 0, (
            "static: the boot frame shaped its labels — a binding that painted "
            "text and reported zero shaping would mean the counter is dead"
        )
        # And it took MORE THAN ONE pass: the first layout writes a scroll
        # bound the view had not read yet, so a cold frame re-passes by
        # construction. Asserting only `>= 1` here would be satisfied by a
        # counter hard-wired to 1 — which is exactly what a counterfactual run
        # proved, so this line is the one that makes the count load-bearing.
        assert boot["last"]["settle_passes"] > 1, (
            f"static: the boot frame re-passed (got "
            f"{boot['last']['settle_passes']}); a cold frame that reports one "
            f"pass is reporting a constant, not a measurement"
        )
        assert_eq(boot["last"]["settled"], True, "static: boot converged")

        # Now the steady state. Every idle repaint must read EXACTLY zero
        # shaping and EXACTLY one pass — and stay there, which is what
        # separates a per-frame count from a lifetime total.
        count = int(boot["frame_count"])
        for i in range(4):
            drive_frame(tf, count, f"static idle frame {i + 1}")
            sample = last(tf)
            assert_wire_shape(sample, f"static: idle {i + 1}")
            assert_eq(
                sample["shape_misses"],
                0,
                f"static: idle {i + 1} handed the shaper nothing (a lifetime "
                f"counter would report ~{boot['last']['shape_misses']} here)",
            )
            assert_eq(
                sample["settle_passes"],
                1,
                f"static: idle {i + 1} converged on its first pass",
            )
            assert_eq(sample["settled"], True, f"static: idle {i + 1} converged")
            count = int(tf.frame_timings()["frame_count"])

    # ── (B) a live-text binding: idle frames DO work, every frame ───────────
    # The profiler HUD draws the timings themselves, so its labels are new
    # strings on every frame. This is the control that makes (A)'s zeros mean
    # "measured no work" instead of "measures nothing".
    with RpcSubprocess(LIVE_TEXT_APP, boot_grace=1.5) as tf:
        boot = tf.frame_timings()
        assert_wire_shape(boot["last"], "live: boot")

        count = int(boot["frame_count"])
        live_samples = []
        for i in range(4):
            drive_frame(tf, count, f"live idle frame {i + 1}")
            sample = last(tf)
            assert_wire_shape(sample, f"live: idle {i + 1}")
            assert sample["shape_misses"] > 0, (
                f"live: idle {i + 1} re-shapes its changing labels — this is "
                f"the per-frame cost that no duration on this wire exposed, "
                f"and the reason the count exists"
            )
            assert_eq(sample["settled"], True, f"live: idle {i + 1} converged")
            live_samples.append(sample["shape_misses"])
            count = int(tf.frame_timings()["frame_count"])

        # The two bindings are on the SAME wire and the SAME shell. Only the
        # content differs, and only the count can tell you so.
        assert all(v > 0 for v in live_samples), (
            f"live: every idle frame did shaping work: {live_samples}"
        )

    # ── (C) R1460: the work an RPC call causes is visible too ───────────────
    # The scene producer settles exactly as a paint does but records no frame —
    # introspection must not manufacture a picture the user never saw. That
    # contract left the §2 #2 path unable to price its own calls, which is what
    # `produce` answers: cumulative, differenced across a call, and never
    # folded into the frame ring.
    with RpcSubprocess(STATIC_TEXT_APP, boot_grace=1.5) as tf:
        before = tf.frame_timings()
        assert "produce" in before, "the produce section is on the wire"
        for field in ("passes_total", "shape_misses_total"):
            assert field in before["produce"], f"`produce.{field}` missing"
            assert isinstance(before["produce"][field], int), (
                f"`produce.{field}` is a cumulative count"
            )

        # A pure READ costs nothing. `scene/snapshot from: paint` serializes the
        # stored last-painted scene rather than producing one, so it must move
        # NEITHER counter — the control that stops "produce work" from meaning
        # "any RPC traffic".
        passes = int(before["produce"]["passes_total"])
        frames = int(before["frame_count"])
        tf.snapshot(source="paint", viewport=(520, 620))
        idle = tf.frame_timings()
        assert_eq(
            int(idle["produce"]["passes_total"]),
            passes,
            "a read that produces nothing is charged nothing",
        )
        assert_eq(int(idle["frame_count"]), frames, "and it painted nothing")

        # A dispatch that has to resolve geometry DOES produce, and says so.
        tf.click(path="reveal_reply")
        after = tf.frame_timings()
        assert int(after["produce"]["passes_total"]) > passes, (
            f"the click's produce ran and was counted ({passes} -> "
            f"{after['produce']['passes_total']})"
        )

        # Monotonic, so differencing it across a call is well defined.
        prev = int(after["produce"]["passes_total"])
        for i in range(3):
            tf.click(path="reveal_reply")
            now = int(tf.frame_timings()["produce"]["passes_total"])
            assert now > prev, f"produce total advanced on call {i + 1}"
            prev = now


if __name__ == "__main__":
    run_demo("R1460 per-frame + per-produce work counts, both backends", body)
