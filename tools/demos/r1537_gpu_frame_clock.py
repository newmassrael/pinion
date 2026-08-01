#!/usr/bin/env python3
"""R1537 §5.16 §5.7 §2 #2 — the frame states what the GPU took.

`scene/frame_timings` has carried CPU durations since R907. Not one of them is
GPU execution time, and `render_us`'s own doc said so: `wgpu` returns from
`submit` long before the GPU has run anything, so a window can be entirely
GPU-bound while every published phase reads fast. That was the
pro-tool-performance axis's largest stated gap, and it was recorded as an
UPSTREAM blocker — `vello::util::RenderContext` creates its device behind a
private constructor with a fixed feature set, so no device it hands back can
ever carry `TIMESTAMP_QUERY`.

It was not an upstream blocker. `vello::Renderer::new` takes a `&Device` the
caller owns. pinion now owns it (`pinion-gpu`), asks for the timestamp
features, and brackets each frame with two queries: one submitted before the
rasterizer's own submission, one riding the blit encoder. What lies between
them is the whole of the frame's GPU work.

This demo asserts the properties that make that number trustworthy — and, just
as much, the properties that make its ABSENCE readable:

  1. **It exists and it is real.** `gpu_us` appears on the wire and is > 0 for
     a window that actually rasterized. A window that painted pixels cannot
     have cost the GPU nothing.
  2. **It is a measurement, not a constant.** Distinct values across frames. A
     hard-wired number would satisfy every bound-style assertion above.
  3. **One sample per frame, exactly, one frame behind.** `gpu_sample_count`
     tracks the driven frame count with a fixed lag — the deterministic
     counter claim, which is what separates "the timer is running" from "the
     timer emitted something once". A timer that blended two frames' spans, or
     re-reported one frame's span twice, breaks this and nothing else.
  4. **It is not a phase of the frame it rides on.** The CPU partition
     (`total >= build + encode + acquire + render`) still holds exactly, no
     matter how large `gpu_us` is. The GPU clock is a different clock on a
     different device about a frame a step back; adding it to that row would
     be an error a client could not detect.
  5. **Absence is stated, never zeroed.** `gpu_timing_supported` is always
     present, and `gpu_dropped_total` is what keeps `supported: true,
     count: 0` honest — that pair means "in flight, read again" for a young
     window and "running and discarding everything" on a broken host, and
     without the drop counter those are the same two values forever.

ZERO-FLAKE: no assertion names a microsecond threshold, a frame rate, or a
machine. Every claim is a count, an ordering, or a presence — except `gpu_us >
0`, which is a claim about physics (rasterizing a window costs the GPU
non-zero time) rather than about this host's speed. Frames are driven by the
window's own `frame_count`, never by a sleep.

Environment: `TIMESTAMP_QUERY` + `TIMESTAMP_QUERY_INSIDE_ENCODERS` were
measured present at R1537 on every adapter this project builds against —
NVIDIA/Vulkan, llvmpipe/Vulkan (what CI uses), and NVIDIA/GL. A host without
them is a NEW environment, not a regression, and section (E) is where that
would surface as a clear statement rather than a confusing zero.

Run from the workspace root:
    cargo build -p hello-checkbox -p hello-heatmap --release
    python3 tools/demos/r1537_gpu_frame_clock.py
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

# A small binding and a chart-heavy one. Both are here to show the clock is
# wired the same everywhere, NOT to compare their speeds: cross-process GPU
# comparisons are scheduling noise, and this demo asserts no such ordering.
SMALL_APP = "hello-checkbox"
HEAVY_APP = "hello-heatmap"

# The GPU clock is read back a frame after it is written, so a window that has
# painted N frames has harvested N-1 samples. Stated as a constant because it
# is the demo's sharpest claim (section C) and not an incidental offset.
READBACK_LAG_FRAMES = 1


def drive_frame(tf: RpcSubprocess, baseline: int, desc: str) -> None:
    """Drive real paints until `frame_count` passes `baseline`.

    `scene/screenshot` forces a real rasterize + blit + present through
    `capture_rgba8`. That path is a genuine GPU frame and is timed by the same
    clock as the winit paint path — which it had to be, or an agent driving
    the window entirely over RPC (the §2 #2 primary path) would see the timer
    report nothing at all.
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


def assert_wire_shape(ft: dict, label: str) -> None:
    """The GPU fields are present, typed, and mutually consistent."""
    win = ft["window"]
    for field in ("gpu_sample_count", "gpu_timing_supported", "gpu_dropped_total"):
        assert field in win, (
            f"{label}: `window.{field}` missing — it must be ALWAYS present, "
            f"because an omitted key cannot say 'no'"
        )
    assert isinstance(win["gpu_timing_supported"], bool), (
        f"{label}: `gpu_timing_supported` is a capability, not a count"
    )
    assert isinstance(win["gpu_sample_count"], int) and not isinstance(
        win["gpu_sample_count"], bool
    ), f"{label}: `gpu_sample_count` must be an integer count"
    assert win["gpu_sample_count"] <= ft["window_len"], (
        f"{label}: the window cannot hold more GPU samples "
        f"({win['gpu_sample_count']}) than frames ({ft['window_len']})"
    )
    # The three GPU durations are omitted together or present together: they
    # are folds of the same set, so a client can branch on any one of them.
    present = [k for k in ("mean_gpu_us", "max_gpu_us") if k in win]
    if win["gpu_sample_count"] == 0:
        assert not present, (
            f"{label}: no timed samples, so no aggregate may be published — "
            f"got {present}; a zero here would read as a free GPU"
        )
    else:
        assert len(present) == 2, (
            f"{label}: {win['gpu_sample_count']} timed samples but only "
            f"{present} published"
        )
        assert win["max_gpu_us"] >= win["mean_gpu_us"], (
            f"{label}: max ({win['max_gpu_us']}) below mean "
            f"({win['mean_gpu_us']}) — the fold is inconsistent"
        )


def assert_cpu_partition_intact(sample: dict, label: str) -> None:
    """`gpu_us` must not have leaked into the CPU accounting.

    R907 documents `total_us >= build + encode + acquire + render` as an
    assertable partition. The GPU clock is a different clock, on a different
    device, about a frame a step back — folding it in would break a claim
    clients were told they could rely on, and would do so silently.
    """
    phase_sum = (
        sample["build_us"] + sample["encode_us"] + sample["acquire_us"] + sample["render_us"]
    )
    assert sample["total_us"] >= phase_sum, (
        f"{label}: CPU partition broken — total={sample['total_us']} < "
        f"phases={phase_sum}"
    )
    assert_eq(
        sample["other_us"],
        sample["total_us"] - phase_sum,
        f"{label}: other_us is the exact CPU remainder",
    )
    assert_eq(
        sample["work_us"],
        sample["build_us"] + sample["encode_us"] + sample["render_us"],
        f"{label}: work_us excludes the acquire block and the GPU clock alike",
    )


def body() -> None:
    # ── (A) the clock exists, and it measures something real ────────────────
    with RpcSubprocess(SMALL_APP, boot_grace=1.5) as tf:
        boot = tf.frame_timings()
        assert_wire_shape(boot, "small: boot")
        assert boot["window"]["gpu_timing_supported"], (
            "this host does not offer TIMESTAMP_QUERY. Measured present at "
            "R1537 on NVIDIA/Vulkan, llvmpipe/Vulkan and NVIDIA/GL — every "
            "adapter this project builds against. A host without it is a new "
            "environment, not a regression: the rest of this demo asserts a "
            "measurement that cannot be taken here."
        )
        # The boot frame has not been read back yet — the lag is real, and
        # asserting it here is what makes section (C)'s exact tracking mean
        # something rather than restating an accident.
        assert_eq(
            boot["window"]["gpu_sample_count"],
            0,
            "small: the boot frame's own timing is still in flight; a timer "
            "reporting a sample for the frame that wrote it would be reading "
            "a timestamp the GPU has not taken",
        )
        assert "gpu_us" not in boot["last"], (
            "small: boot publishes no gpu_us — and OMITS it rather than "
            "sending 0, which would read as a frame that cost the GPU nothing"
        )

        # Drive frames and collect what the clock says.
        readings: list[int] = []
        count = int(boot["frame_count"])
        for i in range(8):
            drive_frame(tf, count, f"small frame {i + 1}")
            ft = tf.frame_timings()
            assert_wire_shape(ft, f"small: frame {i + 1}")
            assert_cpu_partition_intact(ft["last"], f"small: frame {i + 1}")
            count = int(ft["frame_count"])

            # ── (C) one sample per frame, exactly, one frame behind ─────────
            assert_eq(
                ft["window"]["gpu_sample_count"],
                count - READBACK_LAG_FRAMES,
                f"small: frame {i + 1} — every painted frame contributes "
                f"exactly one GPU sample, harvested one frame later. A timer "
                f"that blended two frames' spans, or re-reported one twice, "
                f"breaks this and only this",
            )
            assert_eq(
                ft["window"]["gpu_dropped_total"],
                0,
                f"small: frame {i + 1} — no measurement was taken and thrown "
                f"away; a rising count here would mean the timer runs and "
                f"discards, which reads exactly like a healthy young window "
                f"without this field",
            )

            gpu = ft["last"]["gpu_us"]
            # ── (B) it is real ──────────────────────────────────────────────
            assert gpu > 0, (
                f"small: frame {i + 1} rasterized and blitted a window, which "
                f"cannot cost the GPU zero time (got {gpu})"
            )
            readings.append(gpu)

        # ── (B continued) it is a measurement, not a constant ───────────────
        assert len(set(readings)) > 1, (
            f"small: every frame reported the SAME GPU time {readings} — a "
            f"constant would satisfy every bound above, so this is the "
            f"assertion that makes them mean anything"
        )
        # And the window's max is a real fold over them, not a stale first
        # value or a copy of the last.
        final = tf.frame_timings()["window"]
        assert final["max_gpu_us"] >= max(readings), (
            f"small: window max {final['max_gpu_us']} is below an observed "
            f"sample (observed {readings})"
        )

    # ── (D) the same wiring on a heavier binding ────────────────────────────
    # Not a speed comparison — cross-process GPU scheduling is noise and this
    # demo asserts no ordering between the two. The claim is that the clock is
    # wired identically regardless of what the binding draws, which is what a
    # per-renderer feature would fail.
    with RpcSubprocess(HEAVY_APP, boot_grace=1.5) as tf:
        boot = tf.frame_timings()
        assert_wire_shape(boot, "heavy: boot")
        assert boot["window"]["gpu_timing_supported"], "heavy: same host, same capability"

        count = int(boot["frame_count"])
        heavy_readings: list[int] = []
        for i in range(6):
            drive_frame(tf, count, f"heavy frame {i + 1}")
            ft = tf.frame_timings()
            assert_wire_shape(ft, f"heavy: frame {i + 1}")
            assert_cpu_partition_intact(ft["last"], f"heavy: frame {i + 1}")
            count = int(ft["frame_count"])
            assert_eq(
                ft["window"]["gpu_sample_count"],
                count - READBACK_LAG_FRAMES,
                f"heavy: frame {i + 1} — the same exact per-frame tracking",
            )
            assert_eq(ft["window"]["gpu_dropped_total"], 0, f"heavy: frame {i + 1} lost nothing")
            assert ft["last"]["gpu_us"] > 0, f"heavy: frame {i + 1} did GPU work"
            heavy_readings.append(ft["last"]["gpu_us"])

        assert len(set(heavy_readings)) > 1, (
            f"heavy: the clock varies here too: {heavy_readings}"
        )

        # ── (E) the aggregate is over TIMED samples, not over frames ────────
        # `mean_gpu_us` divides by `gpu_sample_count`. If it divided by the
        # frame count instead, the mean would sit below every single observed
        # reading as soon as one frame went unmeasured — an understatement
        # that grows silently with the miss rate.
        win = tf.frame_timings()["window"]
        assert min(heavy_readings) <= win["mean_gpu_us"] <= max(heavy_readings) or (
            win["gpu_sample_count"] > len(heavy_readings)
        ), (
            f"heavy: mean {win['mean_gpu_us']} lies outside the observed "
            f"range {min(heavy_readings)}..{max(heavy_readings)} over "
            f"{win['gpu_sample_count']} samples — a mean taken over frames "
            f"rather than over timed samples reads exactly like this"
        )
        assert win["max_gpu_us"] >= win["mean_gpu_us"], "heavy: max bounds the mean"
        assert_eq(
            win["gpu_timing_supported"],
            True,
            "heavy: the capability is stated, not inferred from the count",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1537 the frame states what the GPU took", body))
