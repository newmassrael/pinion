#!/usr/bin/env python3
"""R1395 §5.16 §5.7 — numeric DIFFERENT-geometry cross-filter, over RPC.

The cross-filter matrix so far: categorical bar-click (R1384), numeric same-type
scatter brush (R1391), arbitrary via legend (R1392), numeric cross-TYPE line +
scatter (R1394). R1395 adds the completing leg — a single numeric x-window brush
that cross-filters TWO UNLIKE GEOMETRIES at once. `ScatterChart::select_x_range`
(R1391) mutes the point marks outside the window; `BarChart::select_x_range`
(new in R1395) mutes the histogram BINS whose numeric `[k, k+1)` extent falls
outside it — a numeric brush over a categorical bar layout. Both read the SAME
window.

`examples/hello-histogram-brush` wires it: one overview brush strip (a
`RangeSliderExternal` sibling, tag `hist_brush`, the FOURTH consumer of the
`pinion_chart::Brush` substrate — a call to the R1394 SSOT, not a new lift)
under a histogram, whose window drives BOTH a `ScatterChart` (top) and the
`BarChart` histogram (bottom). Both panels are re-prefixed (`scatter.*` /
`hist.*`) so their tags never collide. Driving the brush over `scene/intervene`
and reading each panel back over `scene/snapshot` proves one control
cross-filters two GEOMETRIES without pixels (§2 #7), an AI agent driving it
(§2 #2).

Both panels share the x-axis [0, 12]; a scatter point sits at bucket centre
k + 0.5 and its matching histogram bin is [k, k+1), so an integer-edged window
classifies a point and its bin identically — one window dims the same buckets on
both panels.

  (A) boot — full window: every histogram bin and scatter point is full; both
      panels + the brush strip + the scrub surface present.
  (B) brush x in [3, 6] (f in [0.25, 0.5]): buckets 3,4,5 stay full and every
      other bucket mutes — on the histogram BINS and the scatter POINTS alike,
      the same buckets. ONE brush, TWO geometries.
  (C) brush x in [6, 9] (f in [0.5, 0.75]): the filter SHIFTS on both panels
      together (buckets 6,7,8 go full, 3,4,5 mute).
  (D) reset to the full span: both panels return to full.

Run from the workspace root:
    cargo build -p hello-histogram-brush --release
    python3 tools/demos/r1395_histogram_brush.py
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

VIEWPORT = (760, 560)
BRUSH = "/hist_brush/external"
MUTED = 77  # MUTED_ALPHA = 0x4D — a muted mark's alpha.
N_BINS = 12


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def bar_alpha(snap, k: int) -> int:
    """The fill alpha of histogram bin k (a `hist.bar.{k}` box)."""
    node = find_by_tag(snap, f"hist.bar.{k}")
    assert node is not None, f"hist bin {k} is present (drawn, not dropped)"
    fill = (node.get("style") or {}).get("fill")
    assert fill is not None, f"hist bin {k} carries a queryable fill"
    return fill["a"]


def point_alpha(snap, j: int) -> int:
    """The fill alpha of scatter point j (a `scatter.point.0.{j}` circle)."""
    node = find_by_tag(snap, f"scatter.point.0.{j}")
    assert node is not None, f"scatter point {j} is present (drawn, not dropped)"
    fill = (node.get("style") or {}).get("fill")
    assert fill is not None, f"scatter point {j} carries a queryable fill"
    return fill["a"]


def has(snap, tag: str) -> bool:
    return find_by_tag(snap, tag) is not None


def count_tags(snap, prefix: str) -> int:
    def walk(node) -> int:
        n = 1 if (node.get("tag") or "").startswith(prefix) else 0
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
    with RpcSubprocess("hello-histogram-brush", boot_grace=1.5) as tf:
        # ── (A) boot — both panels full, no cross-filter ─────────────
        snap = paint(tf)
        assert has(snap, "scatter"), "the scatter panel root"
        assert has(snap, "hist"), "the histogram panel root"
        assert has(snap, "hist_brush"), "the brush strip"
        assert has(snap, "hist_scrub"), "the scrub surface"
        # Distinct prefixes: neither panel keeps the chart default.
        assert not has(snap, "chart"), "no node keeps the default 'chart' prefix"
        assert_eq(count_tags(snap, "hist.bar."), N_BINS, "12 histogram bins")
        assert_eq(count_tags(snap, "scatter.point.0."), N_BINS, "12 scatter points")
        # Full window filters nothing: every bin and point is full.
        for k in range(N_BINS):
            assert_eq(bar_alpha(snap, k), 255, f"boot: bin {k} full")
            assert_eq(point_alpha(snap, k), 255, f"boot: point {k} full")
        assert abs(tf.query(f"{BRUSH}/low") - 0.0) < 0.02, "boot brush low = 0"
        assert abs(tf.query(f"{BRUSH}/high") - 1.0) < 0.02, "boot brush high = 1"

        # ── (B) brush x in [3, 6] — TWO geometries filter the SAME buckets ─
        set_brush(tf, 0.25, 0.5)
        snap = paint(tf)
        # Muting DIMS, it does not DROP: every mark still emits a node.
        assert_eq(count_tags(snap, "hist.bar."), N_BINS, "muting keeps every bin drawn")
        assert_eq(
            count_tags(snap, "scatter.point.0."), N_BINS, "muting keeps every point drawn"
        )
        in_window = (3, 4, 5)
        out_window = (0, 1, 2, 6, 7, 8, 9, 10, 11)
        for k in in_window:
            assert_eq(bar_alpha(snap, k), 255, f"in-window bin {k} full")
            assert_eq(point_alpha(snap, k), 255, f"in-window point {k} full")
        for k in out_window:
            assert_eq(bar_alpha(snap, k), MUTED, f"out-of-window bin {k} muted")
            assert_eq(point_alpha(snap, k), MUTED, f"out-of-window point {k} muted")
        # The headline claim, stated directly: for EACH bucket, the histogram bin
        # and the scatter point carry the SAME alpha — one window, two geometries,
        # the identical selection.
        for k in range(N_BINS):
            assert_eq(
                bar_alpha(snap, k),
                point_alpha(snap, k),
                f"bucket {k}: bin and point dim together",
            )

        # ── (C) brush x in [6, 9] — the filter SHIFTS on both panels ─
        set_brush(tf, 0.5, 0.75)
        snap = paint(tf)
        for k in (6, 7, 8):
            assert_eq(bar_alpha(snap, k), 255, f"shifted-in bin {k} full")
            assert_eq(point_alpha(snap, k), 255, f"shifted-in point {k} full")
        for k in (3, 4, 5):
            assert_eq(bar_alpha(snap, k), MUTED, f"shifted-out bin {k} muted")
            assert_eq(point_alpha(snap, k), MUTED, f"shifted-out point {k} muted")
        assert bar_alpha(snap, 7) > bar_alpha(snap, 4), "the histogram filter shifted up"
        assert point_alpha(snap, 7) > point_alpha(snap, 4), "the scatter filter shifted up"

        # ── (D) reset to the full span — both panels full again ──────
        set_brush(tf, 0.0, 1.0)
        snap = paint(tf)
        for k in range(N_BINS):
            assert_eq(bar_alpha(snap, k), 255, f"reset: bin {k} full again")
            assert_eq(point_alpha(snap, k), 255, f"reset: point {k} full again")


if __name__ == "__main__":
    sys.exit(run_demo("R1395 numeric different-geometry cross-filter", body))
