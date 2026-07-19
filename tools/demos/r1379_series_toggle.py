#!/usr/bin/env python3
"""R1379 §5.38 — hello-series-toggle: per-series visibility over a line chart.

A bar of three toggle chips (the chart legend) shows / hides each series'
polyline. The forcing consumer for `pinion_chart::Series::visible`.

Atomic verification scope (>=30 assertions):

  (A) boot — all three series lines drawn, three chips present + all ON.
  (B) click a chip OFF — only that series' polyline disappears; the other
      two stay, the chip stays (it is the toggle back on), independence holds.
  (C) click the same chip ON — its polyline returns on the SAME grid.
  (D) a second chip OFF — the first's re-appearance did not disturb it.
  (E) keyboard — focus a chip, Space/Enter toggles ONLY it (hides/shows its
      line); Arrow roves focus (multi-select, no exclusion).

`chart.series.{i}` is series i's polyline node; a hidden series emits none.
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
)

EXAMPLE = "hello-series-toggle"
VIEWPORT = (520, 360)
GROUP = "series_legend"
N = 3


def chip_tag(i: int) -> str:
    return f"series_{i}"


def value(tf, i: int):
    """The on/visible boolean of series-chip `i` via its `ToggleExternal`."""
    return tf.query(f"/{chip_tag(i)}/external/value")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def line_present(snap, i: int) -> bool:
    """Is series i's polyline (`chart.series.{i}`) in the paint scene?"""
    return find_by_tag(snap, f"chart.series.{i}") is not None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: all three series drawn, all chips ON ───────────────
        snap = paint(tf)
        assert find_by_tag(snap, GROUP) is not None, "legend row tagged series_legend"
        for i in range(N):
            assert find_by_tag(snap, chip_tag(i)) is not None, f"chip {i} present"
            assert_eq(value(tf, i), True, f"boot: series {i} visible")
            assert line_present(snap, i), f"boot: series {i} polyline drawn"

        # ── (B) toggle series 1 OFF: only its line goes, others stay ─────
        tf.click(path=chip_tag(1))
        tf.pointer_leave()
        assert_eq(value(tf, 1), False, "click hides series 1")
        assert_eq(value(tf, 0), True, "series 0 stays visible (independent)")
        assert_eq(value(tf, 2), True, "series 2 stays visible (independent)")
        snap = paint(tf)
        assert not line_present(snap, 1), "hidden series 1 draws no polyline"
        assert line_present(snap, 0), "series 0 polyline still drawn"
        assert line_present(snap, 2), "series 2 polyline still drawn"
        assert find_by_tag(snap, chip_tag(1)) is not None, "hidden series keeps its chip"

        # ── (C) toggle series 1 ON again: its line returns ───────────────
        tf.click(path=chip_tag(1))
        tf.pointer_leave()
        assert_eq(value(tf, 1), True, "click shows series 1 again")
        snap = paint(tf)
        assert line_present(snap, 1), "series 1 polyline returns"
        assert line_present(snap, 0), "series 0 unaffected"
        assert line_present(snap, 2), "series 2 unaffected"

        # ── (D) toggle series 0 OFF: independent of 1's round trip ───────
        tf.click(path=chip_tag(0))
        tf.pointer_leave()
        assert_eq(value(tf, 0), False, "click hides series 0")
        snap = paint(tf)
        assert not line_present(snap, 0), "hidden series 0 draws no polyline"
        assert line_present(snap, 1), "series 1 still drawn"
        assert line_present(snap, 2), "series 2 still drawn"
        # restore series 0 for a clean keyboard phase.
        tf.click(path=chip_tag(0))
        tf.pointer_leave()
        assert_eq(value(tf, 0), True, "series 0 restored")

        # ── (E) keyboard: each chip is its own Tab stop ──────────────────
        focused = tf.request("focus/set", {"tag": chip_tag(2)}).result.get("focused")
        assert_eq(focused, chip_tag(2), "chip 2 is an independent Tab stop")

        # Space toggles ONLY the focused chip (2: on -> off) -> its line goes.
        tf.key(path=chip_tag(2), name="Space")
        assert_eq(value(tf, 2), False, "Space hides focused series 2")
        assert_eq(value(tf, 0), True, "sibling 0 untouched by Space")
        assert_eq(value(tf, 1), True, "sibling 1 untouched by Space")
        snap = paint(tf)
        assert not line_present(snap, 2), "Space-hidden series 2 draws no polyline"
        assert line_present(snap, 0), "series 0 still drawn"

        # Enter toggles it back (2: off -> on) -> its line returns.
        tf.key(path=chip_tag(2), name="Enter")
        assert_eq(value(tf, 2), True, "Enter shows focused series 2 again")
        snap = paint(tf)
        assert line_present(snap, 2), "series 2 polyline returns via Enter"

        # Arrow roves FOCUS across chips (no selection change — multi-select).
        tf.key(path=chip_tag(2), name="ArrowRight")
        assert_eq(
            tf.request("focus/get").result.get("focused"),
            chip_tag(0),
            "ArrowRight wraps: focus 2 -> 0",
        )
        assert_eq(value(tf, 2), True, "roving focus does not change series 2 visibility")

        # ── (F) negative — unknown toggle path is rejected ───────────────
        raised = False
        try:
            tf.query(f"/{chip_tag(0)}/external/no_such_field")
        except Exception:  # noqa: BLE001 — RpcError type varies by harness
            raised = True
        assert raised, "unknown introspect path must be rejected"


if __name__ == "__main__":
    sys.exit(run_demo("R1379 §5.38 — series-visibility toggles over a chart", body))
