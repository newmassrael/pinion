#!/usr/bin/env python3
"""R1391 §5.16 §5.7 — numeric BRUSH-range cross-filter, over RPC.

R1384 gave the chart family its first cross-filter: a CATEGORICAL bar selection
mutes a companion chart. R1391 adds the NUMERIC leg — a brush over a continuous
x-range. `ScatterChart::select_x_range(Some((lo, hi)))` mutes every point mark
whose x falls OUTSIDE `[lo, hi]` (dimmed, but still DRAWN as context, unlike a
pinned domain which drops out-of-domain points); `None` = no filter. It is the
continuous-range twin of `BarChart::select`, sharing the one lifted `MUTED_ALPHA`.

`examples/hello-scatter` wires it cross-widget: an overview brush STRIP (a
`RangeSliderExternal` sibling, tag `scatter_brush`) under the plot, whose selected
window drives the scatter's `select_x_range`. A brush in ONE widget dims the
corresponding points in ANOTHER — the numeric analogue of a bar click. Driving
the brush low/high over `scene/intervene` and reading each point's `style.fill.a`
back over `scene/snapshot` proves the whole round-trip without pixels (§2 #7) and
with an AI agent driving it (§2 #2).

The scatter is two series: `rising` (i=0) at x=1..8 and `falling` (i=1) at
x=1.5..8.5, so the x-extent is [1.0, 8.5]. A brush fraction f maps to x =
1.0 + f * 7.5.

  (A) boot — 16 points, all full (alpha 255); brush strip present; window full.
  (B) brush the lower half (high -> 0.5, x <= 4.75): the low-x points stay full
      and the high-x points mute — yet all 16 points remain DRAWN (mute != drop).
  (C) brush the upper half (low -> 0.5, high -> 1.0, x >= 4.75): the filter
      REVERSES — the previously-muted high-x points go full and the low-x mute.
  (D) reset to the full span: every point returns to full.

Run from the workspace root:
    cargo build -p hello-scatter --release
    python3 tools/demos/r1391_brush_filter.py
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
    wait_until,
)

VIEWPORT = (560, 460)
BRUSH = "/scatter_brush/external"


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def point_alpha(snap, i: int, j: int) -> int:
    """The alpha of series i's point j — 255 at full strength, lower when muted."""
    node = find_by_tag(snap, f"chart.point.{i}.{j}")
    assert node is not None, f"point {i}.{j} is present (drawn, not dropped)"
    fill = (node.get("style") or {}).get("fill")
    assert fill is not None, f"point {i}.{j} carries a queryable style.fill"
    return fill["a"]


def point_count(snap) -> int:
    def walk(node) -> int:
        n = 1 if (node.get("tag") or "").startswith("chart.point.") else 0
        for ch in node.get("children") or []:
            n += walk(ch)
        return n

    return walk(snap)


def set_brush(tf, low: float, high: float) -> None:
    # Set high first, then low (the r738 order — a range never crosses itself).
    tf.intervene(f"{BRUSH}/high", high)
    tf.intervene(f"{BRUSH}/low", low)
    wait_until(
        lambda: abs(tf.query(f"{BRUSH}/low") - low) < 0.02
        and abs(tf.query(f"{BRUSH}/high") - high) < 0.02,
        desc=f"brush -> [{low}, {high}]",
    )


def body() -> None:
    with RpcSubprocess("hello-scatter", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy — every point full, brush strip present ─
        snap = paint(tf)
        assert find_by_tag(snap, "chart") is not None, "the scatter root"
        assert find_by_tag(snap, "scatter_brush") is not None, "the brush strip"
        assert_eq(point_count(snap), 16, "16 points across the two series")
        for i, j in [(0, 0), (0, 7), (1, 0), (1, 7)]:
            assert_eq(point_alpha(snap, i, j), 255, f"boot: point {i}.{j} full")
        assert abs(tf.query(f"{BRUSH}/low") - 0.0) < 0.02, "boot brush low = 0"
        assert abs(tf.query(f"{BRUSH}/high") - 1.0) < 0.02, "boot brush high = 1"

        # ── (B) brush the lower half (x <= ~4.75) ────────────────────
        set_brush(tf, 0.0, 0.5)
        snap = paint(tf)
        # Mute DIMS but does not DROP — all 16 still present.
        assert_eq(point_count(snap), 16, "muting keeps every point drawn")
        # rising: x=1,2,3,4 (j=0..3) IN; x=5,6,7,8 (j=4..7) OUT.
        for j in (0, 1, 2, 3):
            assert_eq(point_alpha(snap, 0, j), 255, f"rising {j} (low x) full")
        for j in (4, 5, 6, 7):
            assert point_alpha(snap, 0, j) < 255, f"rising {j} (high x) muted"
        # falling: x=1.5,2.5,3.5 (j=0..2) IN; x>=4.5 (j>=3) OUT.
        assert_eq(point_alpha(snap, 1, 0), 255, "falling 0 (low x) full")
        assert point_alpha(snap, 1, 7) < 255, "falling 7 (high x) muted"
        # An in-range point is strictly brighter than an out-of-range one.
        assert point_alpha(snap, 0, 0) > point_alpha(snap, 0, 7), "in-range brighter than out"

        # ── (C) brush the upper half (x >= ~4.75) — the filter REVERSES ─
        set_brush(tf, 0.5, 1.0)
        snap = paint(tf)
        assert_eq(point_count(snap), 16, "still every point drawn")
        for j in (4, 5, 6, 7):
            assert_eq(point_alpha(snap, 0, j), 255, f"rising {j} (high x) now full")
        for j in (0, 1, 2, 3):
            assert point_alpha(snap, 0, j) < 255, f"rising {j} (low x) now muted"
        assert point_alpha(snap, 0, 7) > point_alpha(snap, 0, 0), "the filter reversed"

        # ── (D) reset to the full span — every point full again ──────
        set_brush(tf, 0.0, 1.0)
        snap = paint(tf)
        for i, j in [(0, 0), (0, 7), (1, 0), (1, 7)]:
            assert_eq(point_alpha(snap, i, j), 255, f"reset: point {i}.{j} full")


if __name__ == "__main__":
    sys.exit(run_demo("R1391 numeric brush-range cross-filter", body))
