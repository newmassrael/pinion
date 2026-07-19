#!/usr/bin/env python3
"""R1374 §5.16 §5.7 — frame-time DISTRIBUTION histogram (the bar-chart twin).

R1361 gave the profiler a live TIMELINE (a `pinion_chart::LineChart` of each
frame's phases over the last 120 frames). A timeline answers "how did frame N
do?"; it cannot answer "how OFTEN do frames land in each duration band?" — the
distribution. R1374 adds that: the NEW `pinion_chart::BarChart` bins the same
rolling window's total frame times and draws one bar per bucket, over-budget
bins in the error (jank) colour. The bar chart shares the line chart's
scale / ticks / palette / draw core (the crate's `draw.rs` lift), so it docks /
flexes / resizes and reads back over §2 #7 introspection exactly the same way.

## The verification idea (the r925 shape): geometry, not wall-clock

Frame timings are wall-clock, so a demo can never assert "bin 3 has 5 frames".
But the histogram's STRUCTURE is deterministic on any machine, and it is
scene-as-data (§2 #7) — every bar is a tagged node whose rect the AI reads
without sampling a pixel:

  (A) the panel renders one bar per bin (`histogram.bar.0..11`) + its own axes,
      a DISTINCT panel below the timeline (both `chart.series.0` and
      `histogram.bar.0` present).
  (B) every bar sits on the one baseline (the histogram x-axis) — a bar chart's
      defining invariant.
  (C) bars are laid out left-to-right across the panel, non-descending x.
  (D) the window's frames make a real distribution: the modal bin has a tall
      bar, empty bins still emit a >=1px stub (present-if-finite).
  (E) the histogram is layout-native: a real `scene/resize` re-spans the bars
      across the wider window (the `build_fill` measured-rect seam, its own
      `HIST_TAG` key).
  (F) the over-budget colour classification reaches the wire: a bar's
      `style.fill` is queryable in the snapshot (R55.G.8), so under a 1-second
      budget (a 1-fps target — no healthy frame exceeds it) EVERY bin is
      under-budget and every bar carries the one under-budget colour. (The
      over-budget -> error MAPPING is unit-tested; a wall-clock demo cannot
      force a specific bin past the budget without flaking, so it drives the
      deterministic all-under case here.)

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-frame-profiler"
VIEWPORT = (760, 520)
HIST_BINS = 12  # mirrors the binding const.


def _drive_redraw(tf: RpcSubprocess) -> None:
    try:
        tf.request("scene/click", {"path": "repaint"})
    except Exception:  # noqa: BLE001
        pass


def _drive_frame_beyond(tf: RpcSubprocess, baseline: int, desc: str) -> None:
    """Drive repaints until the profiler's own frame counter advances (never
    wall-clock — the r883 zero-flake gate)."""

    def advanced() -> bool:
        try:
            fc = int(tf.frame_timings()["frame_count"])
        except RpcError:
            fc = -1
        if fc > baseline:
            return True
        _drive_redraw(tf)
        return False

    wait_until(advanced, desc=desc)


def _wait_available(tf: RpcSubprocess) -> dict[str, Any]:
    def poll() -> Any:
        try:
            return tf.frame_timings()
        except RpcError:
            _drive_redraw(tf)
            return None

    return wait_until(poll, desc="scene/frame_timings becomes available")


def rects(tf: RpcSubprocess) -> dict[str, tuple[int, int, int, int]]:
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def bar(r: dict[str, tuple[int, int, int, int]], k: int) -> tuple[int, int, int, int]:
    tag = f"histogram.bar.{k}"
    assert tag in r, f"{tag} present in the paint scene"
    return r[tag]


def find_node(node, tag: str):
    """The raw snapshot node for `tag` (carries `style.fill`, which
    `abs_rects_of` discards)."""
    if not isinstance(node, dict):
        return None
    if node.get("tag") == tag:
        return node
    if node.get("type") == "Scroll":
        return find_node(node.get("content"), tag)
    for child in node.get("children") or []:
        found = find_node(child, tag)
        if found is not None:
            return found
    return None


def bar_fill(snap, k: int) -> tuple[int, int, int]:
    node = find_node(snap, f"histogram.bar.{k}")
    assert node is not None, f"histogram.bar.{k} node present in the snapshot"
    fill = (node.get("style") or {}).get("fill")
    assert fill is not None, f"bar {k} carries a queryable style.fill"
    return (fill["r"], fill["g"], fill["b"])


def wait_histogram(tf: RpcSubprocess) -> dict[str, tuple[int, int, int, int]]:
    """Poll the paint snapshot until the histogram panel's bars have painted
    (the measured-rect seam publishes + the re-view lands its bars)."""

    def ready() -> Any:
        r = rects(tf)
        return r if f"histogram.bar.{HIST_BINS - 1}" in r else None

    return wait_until(ready, desc="the histogram panel paints its bars")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        _wait_available(tf)
        # Drive several fresh frames so the rolling window has a distribution to
        # bin, then let the histogram's measured-rect seam settle.
        base = int(tf.frame_timings()["frame_count"])
        for i in range(6):
            _drive_frame_beyond(tf, base + i, f"driven frame {i + 1}")
        r = wait_histogram(tf)

        # ── (A) one bar per bin + axes, a DISTINCT panel below the timeline ──
        for k in range(HIST_BINS):
            assert f"histogram.bar.{k}" in r, f"bar {k} of the distribution painted"
        assert f"histogram.bar.{HIST_BINS}" not in r, "no phantom extra bin"
        assert "histogram.axis.x" in r, "the histogram has its own x-axis"
        assert "histogram.axis.y" in r, "and its own y-axis"
        assert "chart.series.0" in r, "the timeline (line chart) still paints"
        timeline_axis_y = r["chart.axis.x"][1]
        hist_axis_y = r["histogram.axis.x"][1]
        assert hist_axis_y > timeline_axis_y, (
            "the histogram is a distinct panel BELOW the timeline "
            f"(timeline x-axis at y={timeline_axis_y}, histogram at y={hist_axis_y})"
        )

        # ── (B) every bar sits on the one baseline (the x-axis) ──────────────
        baseline = hist_axis_y
        for k in range(HIST_BINS):
            _, y, _, h = bar(r, k)
            assert abs((y + h) - baseline) <= 3, (
                f"bar {k}'s bottom edge (y+h={y + h}) sits on the baseline {baseline}"
            )

        # ── (C) bars are laid out left-to-right, non-overlapping order ───────
        xs = [bar(r, k)[0] for k in range(HIST_BINS)]
        assert xs == sorted(xs), f"bars ascend left-to-right by x: {xs}"
        assert len(set(xs)) == HIST_BINS, "each bar occupies its own x slot"

        # ── (D) the window's frames make a real distribution ─────────────────
        # The modal bin has a tall bar — a non-tautological "frames landed and
        # were binned" check (a broken binning would leave every bar at the 1px
        # stub floor). Not asserting `min >= 1`: the `max(1.0)` clamp forces it,
        # so it could never fail.
        heights = [bar(r, k)[3] for k in range(HIST_BINS)]
        assert max(heights) > 8, (
            f"the modal bin has a real bar (frames landed in it); heights={heights}"
        )
        widths = [bar(r, k)[2] for k in range(HIST_BINS)]
        assert min(widths) >= 1 and max(widths) - min(widths) <= 2, (
            f"the bars share a uniform slot width: {widths}"
        )

        # ── (E) layout-native: a real resize re-spans the bars ───────────────
        before_right = max(x + w for (x, _, w, _) in (bar(r, k) for k in range(HIST_BINS)))
        resp = tf.request("scene/resize", {"width": 1040, "height": 560})
        assert resp is not None and resp.result is not None, "scene/resize accepted"

        def widened() -> Any:
            s = tf.snapshot(source="paint", viewport=(1040, 560))
            if s["rect"]["w"] != 1040:
                return None
            rr = abs_rects_of(s)
            return rr if f"histogram.bar.{HIST_BINS - 1}" in rr else None

        r2 = wait_until(widened, desc="the window settles wider and the histogram repaints")
        after_right = max(
            x + w for (x, _, w, _) in (r2[f"histogram.bar.{k}"] for k in range(HIST_BINS))
        )
        assert after_right > before_right, (
            f"a wider window re-spans the histogram: rightmost edge "
            f"{before_right} -> {after_right} (a stale body would not move)"
        )
        # The baseline invariant still holds at the new size.
        base2 = r2["histogram.axis.x"][1]
        for k in range(HIST_BINS):
            _, y, _, h = r2[f"histogram.bar.{k}"]
            assert abs((y + h) - base2) <= 3, f"bar {k} still on the baseline after resize"

        # ── (F) the over-budget colour classification reaches the wire ───────
        # A 1-fps target = a 1-second budget; no frame (even a vsync-blocked
        # paced one) exceeds it, so every bin is under budget and every bar
        # carries the ONE under-budget colour — read back through the Box fill
        # the snapshot wire carries (R55.G.8). The over-budget -> error MAPPING
        # is unit-tested; a wall-clock demo cannot force a bin over without flake.
        tf.set_fps(1)
        base_f = int(tf.frame_timings()["frame_count"])
        for i in range(2):
            _drive_frame_beyond(tf, base_f + i, f"(F) paced frame {i + 1}")

        def coloured() -> Any:
            s = tf.snapshot(source="paint", viewport=(1040, 560))
            return s if find_node(s, f"histogram.bar.{HIST_BINS - 1}") is not None else None

        snap_f = wait_until(coloured, desc="the histogram repaints under the 1s budget")
        fills = [bar_fill(snap_f, k) for k in range(HIST_BINS)]
        assert len(set(fills)) == 1, (
            f"under a 1s budget no bin is over -> every bar carries the one "
            f"under-budget colour (the classification, over the wire); got {sorted(set(fills))}"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1374 §5.16 — frame-time distribution histogram", body))
