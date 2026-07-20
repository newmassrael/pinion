#!/usr/bin/env python3
"""R1394 §5.16 §5.7 — numeric CROSS-TYPE cross-filter, over RPC.

The cross-filter matrix so far: categorical bar-click (R1384), numeric same-type
scatter brush (R1391), arbitrary via legend (R1392). R1394 adds the missing leg
— a single numeric x-window brush that cross-filters TWO UNLIKE chart types at
once. `LineChart::select_x_range(Some((lo, hi)))` (new in R1394) mutes the line's
out-of-window portion to a context ghost (its whole polyline dims, alpha
MUTED_ALPHA=77) and OVERDRAWS the in-window slice at full colour
(`line.focus.series.{i}`); `ScatterChart::select_x_range` (R1391) mutes the
points outside the window. Both read the SAME window.

`examples/hello-linked-brush` wires it: one overview brush strip (a
`RangeSliderExternal` sibling, tag `linked_brush`, the THIRD consumer of the
lifted `pinion_chart::Brush` substrate) under a scatter, whose window drives
BOTH a `LineChart` (top) and the `ScatterChart` (bottom). Both panels are
re-prefixed (`line.*` / `scatter.*`) so their tags never collide. Driving the
brush over `scene/intervene` and reading each panel back over `scene/snapshot`
proves one control cross-filters two chart types without pixels (§2 #7), an AI
agent driving it (§2 #2).

Both panels are pinned to the shared x-domain [0, 11], so a brush fraction f
maps to x = 11 * f on each.

  (A) boot — full window: the line is full (no focus overdraw) and every scatter
      point is full; both panels + the brush strip + the scrub surface present.
  (B) brush the middle (f in [0.3, 0.6] -> x in [3.3, 6.6]): the line dims to a
      context ghost AND grows a full-colour focus segment, while the scatter
      points outside the window mute — ONE brush, TWO chart types.
  (C) brush the upper third (f in [0.6, 0.9] -> x in [6.6, 9.9]): the filter
      SHIFTS on both panels together (previously-muted points/segment go full).
  (D) reset to the full span: both panels return to full, focus overdraw gone.

Run from the workspace root:
    cargo build -p hello-linked-brush --release
    python3 tools/demos/r1394_linked_brush.py
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
BRUSH = "/linked_brush/external"
MUTED = 77  # MUTED_ALPHA = 0x4D — a muted mark's alpha.


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def stroke_alpha(snap, tag: str) -> int:
    """The alpha of a stroked path (`line.series.*` / `line.focus.series.*`)."""
    node = find_by_tag(snap, tag)
    assert node is not None, f"{tag} is present"
    stroke = (node.get("style") or {}).get("stroke")
    assert stroke is not None, f"{tag} carries a queryable stroke"
    return stroke["color"]["a"]


def point_alpha(snap, i: int, j: int) -> int:
    """The alpha of scatter series i's point j — 255 full, lower when muted."""
    node = find_by_tag(snap, f"scatter.point.{i}.{j}")
    assert node is not None, f"scatter point {i}.{j} is present (drawn, not dropped)"
    fill = (node.get("style") or {}).get("fill")
    assert fill is not None, f"scatter point {i}.{j} carries a queryable fill"
    return fill["a"]


def has(snap, tag: str) -> bool:
    return find_by_tag(snap, tag) is not None


def scatter_point_count(snap) -> int:
    def walk(node) -> int:
        n = 1 if (node.get("tag") or "").startswith("scatter.point.") else 0
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
    with RpcSubprocess("hello-linked-brush", boot_grace=1.5) as tf:
        # ── (A) boot — both panels full, no cross-filter ─────────────
        snap = paint(tf)
        assert has(snap, "line"), "the line panel root"
        assert has(snap, "scatter"), "the scatter panel root"
        assert has(snap, "linked_brush"), "the brush strip"
        assert has(snap, "linked_scrub"), "the scrub surface"
        # Distinct prefixes: neither panel keeps the chart default.
        assert not has(snap, "chart"), "no node keeps the default 'chart' prefix"
        assert has(snap, "line.series.0"), "the line polyline is drawn"
        assert has(snap, "line.area.0"), "the filled area is drawn"
        assert_eq(scatter_point_count(snap), 23, "23 scatter points (12 + 11)")
        # Full window filters nothing: the line is full and has no focus overdraw.
        assert_eq(stroke_alpha(snap, "line.series.0"), 255, "boot: line at full colour")
        assert not has(snap, "line.focus.series.0"), "boot: no line focus overdraw"
        assert not has(snap, "line.focus.area.0"), "boot: no line focus area"
        for i, j in [(0, 0), (0, 6), (0, 11), (1, 0), (1, 10)]:
            assert_eq(point_alpha(snap, i, j), 255, f"boot: scatter {i}.{j} full")
        assert abs(tf.query(f"{BRUSH}/low") - 0.0) < 0.02, "boot brush low = 0"
        assert abs(tf.query(f"{BRUSH}/high") - 1.0) < 0.02, "boot brush high = 1"

        # ── (B) brush the middle (x in [3.3, 6.6]) — TWO types filter ─
        set_brush(tf, 0.3, 0.6)
        snap = paint(tf)
        # LINE: the whole polyline dims to a context ghost, a focus segment at
        # full colour is overdrawn (the R1394 capability).
        assert_eq(stroke_alpha(snap, "line.series.0"), MUTED, "line dims to context")
        assert has(snap, "line.focus.series.0"), "line grows a focus segment"
        assert_eq(stroke_alpha(snap, "line.focus.series.0"), 255, "focus at full colour")
        assert has(snap, "line.focus.area.0"), "the focus area is overdrawn too"
        # SCATTER: points outside [3.3, 6.6] mute; muting DIMS, never DROPS.
        assert_eq(scatter_point_count(snap), 23, "muting keeps every scatter point drawn")
        # sensor A (x = 0..11): x in {4,5,6} are IN.
        for j in (4, 5, 6):
            assert_eq(point_alpha(snap, 0, j), 255, f"sensor A {j} (in window) full")
        for j in (0, 1, 2, 3, 7, 8, 9, 10, 11):
            assert point_alpha(snap, 0, j) < 255, f"sensor A {j} (out of window) muted"
        # sensor B (x = j + 0.5): x in {3.5,4.5,5.5,6.5} are IN.
        for j in (3, 4, 5, 6):
            assert_eq(point_alpha(snap, 1, j), 255, f"sensor B {j} (in window) full")
        for j in (0, 1, 2, 7, 8, 9, 10):
            assert point_alpha(snap, 1, j) < 255, f"sensor B {j} (out of window) muted"
        # The cross-TYPE claim, stated directly: one window, a muted line AND a
        # muted scatter point, both strictly dimmer than their in-window peers.
        assert stroke_alpha(snap, "line.series.0") < 255, "line context is dimmed"
        assert point_alpha(snap, 0, 0) < point_alpha(snap, 0, 5), "scatter in-window brighter"

        # ── (C) brush the upper third (x in [6.6, 9.9]) — filter SHIFTS ─
        set_brush(tf, 0.6, 0.9)
        snap = paint(tf)
        assert has(snap, "line.focus.series.0"), "the focus segment moved, not vanished"
        assert_eq(stroke_alpha(snap, "line.series.0"), MUTED, "line still a context ghost")
        # sensor A: x in {7,8,9} now IN — the previously-muted high points go full.
        for j in (7, 8, 9):
            assert_eq(point_alpha(snap, 0, j), 255, f"sensor A {j} now in window (full)")
        for j in (4, 5, 6):
            assert point_alpha(snap, 0, j) < 255, f"sensor A {j} now out of window (muted)"
        assert point_alpha(snap, 0, 8) > point_alpha(snap, 0, 5), "the filter shifted up"

        # ── (D) reset to the full span — both panels full again ──────
        set_brush(tf, 0.0, 1.0)
        snap = paint(tf)
        assert_eq(stroke_alpha(snap, "line.series.0"), 255, "reset: line full again")
        assert not has(snap, "line.focus.series.0"), "reset: no focus overdraw"
        for i, j in [(0, 0), (0, 11), (1, 0), (1, 10)]:
            assert_eq(point_alpha(snap, i, j), 255, f"reset: scatter {i}.{j} full")


if __name__ == "__main__":
    sys.exit(run_demo("R1394 numeric cross-type cross-filter", body))
