#!/usr/bin/env python3
"""R1375 §5.16 §5.7 — the frame-time histogram's scrub INSPECTOR (the bar-chart
twin of the line chart's R1355 scrub).

R1374 gave the profiler a distribution histogram (a `pinion_chart::BarChart`).
R1375 makes it interactive: a press-drag scrub over the panel highlights the
focused bin and shows a value tooltip — the same interaction the line chart's
`inspect` gives, now for the bar chart, over the lifted `callout` tooltip core
both chart types share. The scrub rides a `SliderExternal` (a captured 1-D
fraction) hosted in the `extra_externals` slot, so it is RPC-drivable exactly
like `hello-chart`'s scrub.

## The verification idea (the r1374 shape): geometry, not wall-clock

The histogram bins wall-clock frame times, so a demo can never assert "bin 3 is
focused shows 5". But the scrub GEOMETRY is deterministic on any machine and it
is scene-as-data (§2 #7) — every overlay element is a tagged node whose rect the
AI reads without sampling a pixel:

  (A) the panel renders one bar per bin + axes (reused from R1374), the field
      the scrub inspects.
  (B) the boot scrub (0.5) paints the overlay: a highlight ring + a tooltip box
      + a bin-label header + a value row.
  (C) the highlight ring FRAMES a real bar — its rect equals one of the bars'.
  (D) driving the scrub over `scene/intervene` MOVES the ring: a low fraction
      lands a left bin, a high fraction a right bin, so the ring's x strictly
      increases (a wire proof of fraction -> categorical slot -> overlay).
  (E) the leftmost slot (fraction 0.0) is bin 0, whose lower edge is 0ms, so its
      header reads "0" — the one deterministic bin label.
  (F) near the right edge the tooltip would overflow, so it flips to the LEFT of
      the bar it points at and stays inside the window.

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
SCRUB = "/hist_scrub/external/value"  # the extra external, addressed by tag.


def _drive_redraw(tf: RpcSubprocess) -> None:
    try:
        tf.request("scene/click", {"path": "repaint"})
    except Exception:  # noqa: BLE001
        pass


def _drive_frame_beyond(tf: RpcSubprocess, baseline: int, desc: str) -> None:
    """Drive repaints until the profiler's frame counter advances (never
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


def find_node(node, tag: str):
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


def header_text(tf: RpcSubprocess) -> str | None:
    node = find_node(tf.snapshot(source="paint", viewport=VIEWPORT), "histogram.inspect.header")
    return None if node is None else node.get("content")


def wait_histogram(tf: RpcSubprocess) -> dict[str, tuple[int, int, int, int]]:
    """Poll the paint snapshot until the histogram panel's bars have painted."""

    def ready() -> Any:
        r = rects(tf)
        return r if f"histogram.bar.{HIST_BINS - 1}" in r else None

    return wait_until(ready, desc="the histogram panel paints its bars")


def set_scrub(tf: RpcSubprocess, value: float) -> None:
    tf.intervene(SCRUB, value)
    got = tf.query(SCRUB)
    assert abs(got - value) < 0.02, f"scrub set to {value}, got {got}"


def ring_after_scrub(
    tf: RpcSubprocess, value: float
) -> tuple[int, int, int, int]:
    """Drive the scrub and return the highlight ring's rect once it settles at
    the requested fraction (the paint re-view lands the moved ring)."""
    set_scrub(tf, value)

    def settled() -> Any:
        r = rects(tf)
        return r.get("histogram.inspect.highlight")

    return wait_until(settled, desc=f"the highlight ring settles at scrub {value}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        _wait_available(tf)
        # Drive fresh frames so the rolling window has a distribution to bin,
        # then let the histogram's measured-rect seam settle.
        base = int(tf.frame_timings()["frame_count"])
        for i in range(6):
            _drive_frame_beyond(tf, base + i, f"driven frame {i + 1}")
        r = wait_histogram(tf)

        # ── (A) the field the scrub inspects: one bar per bin + axes ─────────
        for k in range(HIST_BINS):
            assert f"histogram.bar.{k}" in r, f"bar {k} present"
        assert "histogram.axis.x" in r, "the histogram has its own x-axis"
        assert "histogram.axis.y" in r, "and its own y-axis"

        # ── (B) the boot scrub (0.5) paints the full overlay ─────────────────
        set_scrub(tf, 0.5)
        r = wait_histogram(tf)
        for tag in (
            "histogram.inspect.highlight",
            "histogram.inspect.tooltip",
            "histogram.inspect.header",
            "histogram.inspect.value",
        ):
            assert tag in r, f"the scrub paints {tag}"
        # The timeline (line chart) is NOT scrubbed here, so it emits no overlay.
        assert "chart.inspect.tooltip" not in r, "only the histogram is scrubbed"

        # ── (C) the highlight ring FRAMES a real bar (geom SSOT) ─────────────
        ring = r["histogram.inspect.highlight"]
        bar_rects = [r[f"histogram.bar.{k}"] for k in range(HIST_BINS)]
        assert ring in bar_rects, (
            f"the ring frames exactly one bar's rect; ring={ring} bars={bar_rects}"
        )

        # ── (D) driving the scrub MOVES the ring right ───────────────────────
        low_ring = ring_after_scrub(tf, 0.15)
        low_header = header_text(tf)
        assert low_header is not None, "a low-fraction scrub names a bin"
        assert find_node(
            tf.snapshot(source="paint", viewport=VIEWPORT), "histogram.inspect.value"
        ) is not None, "a low-fraction scrub carries a value row"

        high_ring = ring_after_scrub(tf, 0.85)
        high_header = header_text(tf)
        assert high_header is not None, "a high-fraction scrub names a bin"
        assert high_ring[0] > low_ring[0], (
            f"the ring tracks the scrub rightward: x {low_ring[0]} -> {high_ring[0]}"
        )
        # Different slots -> the framed bar rect differs.
        assert high_ring != low_ring, "a high vs low scrub frames a different bar"

        # ── (E) the leftmost slot is bin 0, header "0" (its 0ms lower edge) ──
        ring_after_scrub(tf, 0.0)
        assert header_text(tf) == "0", (
            f"fraction 0.0 focuses bin 0 (lower edge 0ms), got {header_text(tf)!r}"
        )

        # ── (F) near the right edge the tooltip flips LEFT and stays in-window ─
        ring_after_scrub(tf, 1.0)
        r_edge = rects(tf)
        tip = r_edge["histogram.inspect.tooltip"]
        window_right = VIEWPORT[0]
        assert tip[0] + tip[2] <= window_right, (
            f"the flipped tooltip stays within the window: "
            f"{tip[0]}+{tip[2]} <= {window_right}"
        )
        # The ring at the right edge frames the rightmost drawn bar family.
        edge_ring = r_edge["histogram.inspect.highlight"]
        assert edge_ring in [r_edge[f"histogram.bar.{k}"] for k in range(HIST_BINS)], (
            "the right-edge ring still frames a real bar"
        )
        # It actually FLIPPED (not merely in-bounds): the tooltip box sits to the
        # LEFT of the bar it points at, which a right-placed box never would.
        assert tip[0] < edge_ring[0], (
            f"the tooltip flipped left of its bar: tip.x {tip[0]} < bar.x {edge_ring[0]}"
        )
        # …and it still names the rightmost bin with a value row.
        assert "histogram.inspect.header" in r_edge, "the right-edge scrub still labels"
        assert "histogram.inspect.value" in r_edge, "and still shows a value"


if __name__ == "__main__":
    sys.exit(run_demo("R1375 §5.16 — frame-time histogram scrub inspector", body))
