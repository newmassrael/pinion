#!/usr/bin/env python3
"""R1392 §5.38 §5.39 — an ARBITRARY chart-to-chart cross-filter, over RPC.

R1384 wired the first cross-filter (a categorical BAR click filters a companion
LINE chart); R1391 the numeric leg (a BRUSH range mutes a SCATTER). R1392 proves
the SELECTOR can be an arbitrary chart type too: a SCATTER's own interactive
legend cross-filters a companion LINE chart. The scatter and the line share ONE
per-series visibility state (a `toggle_group`); the scatter's legend entries are
the click surfaces (`ScatterChart::interactive_legend`, the R1380 chip mechanism
now lifted and shared by both charts), and clicking entry `i` hides series `i` in
BOTH charts — a selection in one widget reshapes a DIFFERENT chart type.

Distinct tag prefixes keep the two charts' nodes apart: the scatter's points are
`scatter.point.{i}.{j}`, the line's polylines `line.series.{i}`. The legend entry
`legend_{i}` (a focusable container) is the scatter's; the line is a pure target
(no legend of its own), so no tag is claimed twice.

  (A) boot — both charts full: 3 scatter series (points) + 3 line series
      (polylines) + 3 focusable legend entries, every toggle ON.
  (B) coordinate-click legend entry 1 OFF — series 1 vanishes from BOTH the
      scatter AND the line; the other two survive in both; the entry stays.
  (C) click entry 1 ON again — series 1 returns to BOTH charts.
  (D) click entry 0 OFF — independent of 1's round trip, drops from both.
  (E) keyboard — each entry is its own Tab stop; Space toggles only the focused
      series, in both charts.

Run from the workspace root:
    cargo build -p hello-linked-legend --release
    python3 tools/demos/r1392_linked_legend.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    node_center,
    run_demo,
)

EXAMPLE = "hello-linked-legend"
VIEWPORT = (560, 560)
N = 3


def entry_tag(i: int) -> str:
    return f"legend_{i}"


def value(tf, i: int):
    """Toggle i's visibility state (the shared cross-filter selection)."""
    return tf.query(f"/{entry_tag(i)}/external/value")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def scatter_present(snap, i: int) -> bool:
    """Is scatter series i drawn? (its first point mark is present when visible)."""
    return find_by_tag(snap, f"scatter.point.{i}.0") is not None


def line_present(snap, i: int) -> bool:
    """Is line series i's polyline (`line.series.{i}`) in the paint scene?"""
    return find_by_tag(snap, f"line.series.{i}") is not None


def click_entry(tf, snap, i: int) -> None:
    """Coordinate-click the scatter's legend entry `i` at its rect centre — a
    real click through the chart legend's hit geometry, not a tag shortcut."""
    entry = find_by_tag(snap, entry_tag(i))
    assert entry is not None, f"legend entry {i} present with a rect"
    tf.click(at=node_center(entry))
    tf.pointer_leave()


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: both charts full, three entries, all ON ────────────
        snap = paint(tf)
        for i in range(N):
            entry = find_by_tag(snap, entry_tag(i))
            assert entry is not None, f"legend entry {i} present"
            assert entry.get("rect"), f"legend entry {i} has hit geometry (a rect)"
            assert_eq(value(tf, i), True, f"boot: series {i} visible")
            assert scatter_present(snap, i), f"boot: scatter series {i} drawn"
            assert line_present(snap, i), f"boot: line series {i} drawn"

        # ── (B) click entry 1 OFF: it drops from BOTH charts ─────────────
        click_entry(tf, snap, 1)
        assert_eq(value(tf, 1), False, "clicking the scatter legend hides series 1")
        assert_eq(value(tf, 0), True, "series 0 untouched (independent toggle)")
        assert_eq(value(tf, 2), True, "series 2 untouched (independent toggle)")
        snap = paint(tf)
        assert not scatter_present(snap, 1), "hidden series 1: no scatter points"
        assert not line_present(snap, 1), "hidden series 1: no line polyline (cross-filtered)"
        assert scatter_present(snap, 0), "series 0 scatter points stay"
        assert line_present(snap, 0), "series 0 line polyline stays"
        assert scatter_present(snap, 2), "series 2 scatter points stay"
        assert line_present(snap, 2), "series 2 line polyline stays"
        assert find_by_tag(snap, entry_tag(1)) is not None, "hidden series keeps its entry"

        # ── (C) click entry 1 ON: it returns to BOTH charts ──────────────
        click_entry(tf, snap, 1)
        assert_eq(value(tf, 1), True, "click shows series 1 again")
        snap = paint(tf)
        assert scatter_present(snap, 1), "series 1 scatter points return"
        assert line_present(snap, 1), "series 1 line polyline returns"

        # ── (D) click entry 0 OFF: independent, drops from both ──────────
        click_entry(tf, snap, 0)
        assert_eq(value(tf, 0), False, "click hides series 0")
        snap = paint(tf)
        assert not scatter_present(snap, 0), "hidden series 0: no scatter points"
        assert not line_present(snap, 0), "hidden series 0: no line polyline"
        assert scatter_present(snap, 1), "series 1 still in the scatter"
        assert line_present(snap, 1), "series 1 still in the line"
        # restore series 0 for a clean keyboard phase.
        click_entry(tf, snap, 0)
        assert_eq(value(tf, 0), True, "series 0 restored")

        # ── (E) keyboard: each entry is its own Tab stop ─────────────────
        focused = tf.request("focus/set", {"tag": entry_tag(2)}).result.get("focused")
        assert_eq(focused, entry_tag(2), "entry 2 is an independent Tab stop")
        tf.key(path=entry_tag(2), name="Space")
        assert_eq(value(tf, 2), False, "Space hides focused series 2")
        assert_eq(value(tf, 0), True, "sibling 0 untouched by Space")
        assert_eq(value(tf, 1), True, "sibling 1 untouched by Space")
        snap = paint(tf)
        assert not scatter_present(snap, 2), "Space-hidden series 2: no scatter points"
        assert not line_present(snap, 2), "Space-hidden series 2: no line polyline"


if __name__ == "__main__":
    sys.exit(run_demo("R1392 arbitrary chart-to-chart cross-filter", body))
