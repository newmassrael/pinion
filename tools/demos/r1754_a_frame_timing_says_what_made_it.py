#!/usr/bin/env python3
"""R1754 §5.16 — **a frame timing says which GPU stack produced it.**

## What forced this demo

R1752 read `render_us`, found it was 97.8% of the frame, and recorded that as
a fact about pinion. It was a fact about the display that round ran on. No
field in `scene/frame_timings` said so, and no client could have recovered it:
adapter selection is constrained by the window's *surface*, so which adapter
arrives is a property of the window and not of the host.

## ⚠ What measuring it actually found

The field was built expecting a virtual framebuffer to force a software
rasterizer, which would have explained the spread. **Asking the two displays
refuted that.** Same machine, same app, same scene (133 draws), 150 ticks:

    Xvfb :97     mean_render_us =  10,384    gpu_us =   491    encode_us = 126
    real Xorg :1 mean_render_us = 997,132    gpu_us = 1,759    encode_us = 141

Ninety-six-fold apart — and `scene/frame_timings` answered `adapter` IDENTICALLY
both times: the same discrete GPU, `backend: vulkan`, `hardware: true`.

So the adapter is *necessary and not sufficient*. Necessary, because a bug
report is not reproducible without it and `device_class: cpu` is a real state
on a host with no GPU. Not sufficient, because the spread lives elsewhere:
`render_us` brackets `present()` along with the record and the submit, and a
present blocks on whoever consumes the swapchain. That remains open —
see the debt note this round filed.

This demo therefore proves what the field DOES claim, and deliberately does
not claim what the measurement refuted.

## What is proved here, over RPC

  * the record carries `adapter`, and it is a complete object — a name, a
    device class, a backend, and a derived `hardware` verdict;
  * the tokens come from pinion's closed vocabulary, not a passthrough of a
    dependency's spelling;
  * `hardware` agrees with the two terms it is derived from;
  * the qualifier is stable across reads while the durations move, which is
    what makes it a premise rather than a sample.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r1754_a_frame_timing_says_what_made_it.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo

TICKS = 40

# The closed vocabularies `pinion_runtime::GpuDeviceClass` / `GpuBackend` are
# projected onto. Spelled out here so the demo fails if the wire ever starts
# echoing a dependency's own spelling instead — which is the coupling the
# closed vocabulary exists to prevent.
DEVICE_CLASSES = {"discrete", "integrated", "virtual", "cpu", "other"}
BACKENDS = {"vulkan", "metal", "dx12", "gl", "webgpu", "noop", "other"}


def timings(tf) -> dict:
    """The `scene/frame_timings` payload as a plain dict."""
    r = tf.request("scene/frame_timings")
    return getattr(r, "result", r)


def body(tf) -> None:
    for _ in range(TICKS):
        tf.request("scene/tick", {"dt": 0.05})
    first = timings(tf)

    # ---- the record carries its premise ---------------------------------
    # This demo runs a real windowed backend, so an absent adapter here is a
    # broken seam rather than an honest `None`. (A backend that renders
    # through no adapter reports `None`; that case is pinned by the unit
    # tests, which can build one and this cannot.)
    assert "adapter" in first, (
        "`adapter` missing — the qualifier IS the round; a duration without it "
        "gets read as a property of the software, which is how R1752 came to "
        "record a display's cost as a fact about pinion"
    )
    a = first["adapter"]
    assert isinstance(a, dict), "the adapter is an object, not a bare string"

    for field, kind in (
        ("name", str),
        ("device_class", str),
        ("backend", str),
        ("hardware", bool),
    ):
        assert field in a, f"adapter.{field} missing"
        assert isinstance(a[field], kind), f"adapter.{field} is not a {kind.__name__}"

    assert a["name"], "an adapter with no name identifies nothing"

    # ---- the vocabulary is pinion's, and closed -------------------------
    assert a["device_class"] in DEVICE_CLASSES, (
        f"device_class {a['device_class']!r} is outside pinion's vocabulary — "
        "the wire must not echo a dependency's spelling"
    )
    assert a["backend"] in BACKENDS, (
        f"backend {a['backend']!r} is outside pinion's vocabulary"
    )

    # ---- the derived verdict agrees with what it is derived from --------
    # Published so a client need not carry this table; asserted so the two can
    # never drift apart on the wire.
    expected_hardware = a["device_class"] != "cpu" and a["backend"] != "noop"
    assert_eq(a["hardware"], expected_hardware, "hardware agrees with its two terms")

    # ---- it is a premise, not a sample ----------------------------------
    # More frames change every duration in the record; the qualifier must not
    # move, or it would be describing the read rather than the window.
    for _ in range(TICKS):
        tf.request("scene/tick", {"dt": 0.05})
    second = timings(tf)
    assert second["frame_count"] > first["frame_count"], "more frames were driven"
    assert_eq(second["adapter"], a, "the adapter is stable across reads")

    # And it qualifies the WHOLE record — both halves, not one frame.
    assert second["window"]["mean_render_us"] > 0, "the window submitted frames"
    assert second["last"]["render_us"] > 0, "the last frame submitted"

    # ★ The honest limit, asserted rather than left to prose: this object says
    # what the numbers were made ON. It does not explain why two windows
    # disagree — measured 96x apart with this field answering identically. A
    # reader who wants that needs the present split, which is not on this wire.
    assert "present_us" not in second["last"], (
        "if `present_us` has landed, this demo's stated limit is stale and the "
        "R1754 debt note should be closed rather than left claiming otherwise"
    )

    w = second["window"]
    print(
        f"[r1754] adapter={a['name']!r} class={a['device_class']} "
        f"backend={a['backend']} hardware={a['hardware']} | "
        f"mean_render={w['mean_render_us']}us gpu={w.get('mean_gpu_us')}us "
        f"encode={w['mean_encode_us']}us draws={w['max_draw']['draws']}"
    )


def main() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        body(tf)


if __name__ == "__main__":
    run_demo("r1754 a frame timing says what made it", main)
