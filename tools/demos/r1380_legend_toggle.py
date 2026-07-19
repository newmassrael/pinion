#!/usr/bin/env python3
"""R1380 §5.38 — hello-legend-toggle: click the chart's OWN legend to toggle.

The forcing consumer for `pinion_chart::LineChart::interactive_legend`. Unlike
R1379 (a separate chip bar), the toggle surface IS the chart's legend: each
entry is a focusable, hit-testable region carrying the caller's tag, so a click
at the entry's pixels routes — through the real geometric hit-test — to that
series' `ToggleExternal`.

Atomic verification scope (>=30 assertions):

  (A) boot — all three series lines drawn, three focusable legend entries
      present + all ON.
  (B) COORDINATE-click legend entry 1 (its rect centre, via the paint scene) —
      proving the chart legend now HAS hit geometry: only series 1's polyline
      disappears; the other two + all other toggles are untouched; the entry
      stays (it is the toggle back on).
  (C) coordinate-click the same entry ON — its polyline returns on the SAME grid.
  (D) a second entry OFF — the first's round trip did not disturb it.
  (E) keyboard — each entry is its own Tab stop; Space/Enter toggles ONLY the
      focused entry; Arrow roves focus (multi-select, no exclusion).
  (F) BOUNDED hit region — a click in the plot body (NOT on any legend entry)
      toggles nothing: the hit geometry is the legend, not the whole chart.
  (G) negative — an unknown introspect path is rejected.

`chart.series.{i}` is series i's polyline node; a hidden series emits none.
`legend_{i}` is the interactive legend entry (a focusable container) whose
click routes to toggle i.
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

EXAMPLE = "hello-legend-toggle"
VIEWPORT = (560, 360)
N = 3


def entry_tag(i: int) -> str:
    return f"legend_{i}"


def value(tf, i: int):
    """The on/visible boolean of legend entry `i` via its `ToggleExternal`."""
    return tf.query(f"/{entry_tag(i)}/external/value")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def line_present(snap, i: int) -> bool:
    """Is series i's polyline (`chart.series.{i}`) in the paint scene?"""
    return find_by_tag(snap, f"chart.series.{i}") is not None


def click_entry(tf, snap, i: int) -> None:
    """Click legend entry `i` at its rect CENTRE — a real coordinate click, so
    it exercises the chart legend's hit geometry, not a tag shortcut."""
    entry = find_by_tag(snap, entry_tag(i))
    assert entry is not None, f"legend entry {i} present with a rect"
    tf.click(at=node_center(entry))
    tf.pointer_leave()


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: all three series drawn, three entries, all ON ──────
        snap = paint(tf)
        for i in range(N):
            entry = find_by_tag(snap, entry_tag(i))
            assert entry is not None, f"legend entry {i} present"
            assert entry.get("rect"), f"legend entry {i} has hit geometry (a rect)"
            assert_eq(value(tf, i), True, f"boot: series {i} visible")
            assert line_present(snap, i), f"boot: series {i} polyline drawn"

        # ── (B) coordinate-click entry 1 OFF: only its line goes ─────────
        click_entry(tf, snap, 1)
        assert_eq(value(tf, 1), False, "coordinate click on entry 1 hides series 1")
        assert_eq(value(tf, 0), True, "series 0 untouched (independent)")
        assert_eq(value(tf, 2), True, "series 2 untouched (independent)")
        snap = paint(tf)
        assert not line_present(snap, 1), "hidden series 1 draws no polyline"
        assert line_present(snap, 0), "series 0 polyline still drawn"
        assert line_present(snap, 2), "series 2 polyline still drawn"
        assert find_by_tag(snap, entry_tag(1)) is not None, "hidden series keeps its entry"

        # ── (C) coordinate-click entry 1 ON again: its line returns ──────
        click_entry(tf, snap, 1)
        assert_eq(value(tf, 1), True, "click shows series 1 again")
        snap = paint(tf)
        assert line_present(snap, 1), "series 1 polyline returns"
        assert line_present(snap, 0), "series 0 unaffected"
        assert line_present(snap, 2), "series 2 unaffected"

        # ── (D) coordinate-click entry 0 OFF: independent of 1's round trip
        click_entry(tf, snap, 0)
        assert_eq(value(tf, 0), False, "click hides series 0")
        snap = paint(tf)
        assert not line_present(snap, 0), "hidden series 0 draws no polyline"
        assert line_present(snap, 1), "series 1 still drawn"
        assert line_present(snap, 2), "series 2 still drawn"
        # restore series 0 for a clean keyboard phase.
        click_entry(tf, snap, 0)
        assert_eq(value(tf, 0), True, "series 0 restored")

        # ── (E) keyboard: each entry is its own Tab stop ─────────────────
        focused = tf.request("focus/set", {"tag": entry_tag(2)}).result.get("focused")
        assert_eq(focused, entry_tag(2), "entry 2 is an independent Tab stop")

        # Space toggles ONLY the focused entry (2: on -> off) -> its line goes.
        tf.key(path=entry_tag(2), name="Space")
        assert_eq(value(tf, 2), False, "Space hides focused series 2")
        assert_eq(value(tf, 0), True, "sibling 0 untouched by Space")
        assert_eq(value(tf, 1), True, "sibling 1 untouched by Space")
        snap = paint(tf)
        assert not line_present(snap, 2), "Space-hidden series 2 draws no polyline"
        assert line_present(snap, 0), "series 0 still drawn"

        # Enter toggles it back (2: off -> on) -> its line returns.
        tf.key(path=entry_tag(2), name="Enter")
        assert_eq(value(tf, 2), True, "Enter shows focused series 2 again")
        snap = paint(tf)
        assert line_present(snap, 2), "series 2 polyline returns via Enter"

        # Arrow roves FOCUS across entries (no selection change — multi-select).
        tf.key(path=entry_tag(2), name="ArrowRight")
        assert_eq(
            tf.request("focus/get").result.get("focused"),
            entry_tag(0),
            "ArrowRight wraps: focus 2 -> 0",
        )
        assert_eq(value(tf, 2), True, "roving focus does not change series 2 visibility")

        # ── (F) BOUNDED hit region: a click in the plot body toggles nothing
        before = [value(tf, i) for i in range(N)]
        # The chart's plot centre — well below the legend band, clearly not on
        # any entry. If the hit region were "the whole chart" this would toggle.
        tf.click(at=(VIEWPORT[0] / 2, VIEWPORT[1] * 0.6))
        tf.pointer_leave()
        for i in range(N):
            assert_eq(
                value(tf, i),
                before[i],
                f"a plot-body click leaves series {i} unchanged (hit region is the legend)",
            )

        # ── (G) negative — unknown introspect path is rejected ───────────
        raised = False
        try:
            tf.query(f"/{entry_tag(0)}/external/no_such_field")
        except Exception:  # noqa: BLE001 — RpcError type varies by harness
            raised = True
        assert raised, "unknown introspect path must be rejected"


if __name__ == "__main__":
    sys.exit(run_demo("R1380 §5.38 — click the chart's own legend to toggle", body))
