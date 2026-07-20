#!/usr/bin/env python3
"""R1384 — hello-cross-filter: click a bar in ONE widget to filter ANOTHER.

The forcing consumer for `pinion_chart::BarChart::select` + `::selectable`. A
bar chart of per-category totals is the SELECTOR; a line chart of per-category
timelines is the DEPENDENT view. Each bar's whole slot column is a focusable,
hit-testable region (carrying the caller's tag), so a COORDINATE click in a
category's column routes — through the real geometric hit-test — to that
category's `ToggleExternal`, and the SAME active mask both mutes the other bars
AND filters the timeline. A selection in one widget reshapes another — the
dashboard interaction the chart family was building toward.

Atomic verification scope (>=30 assertions):

  (A) boot — no category selected: all four timelines drawn, all four bars full
      (alpha 255), four focusable bar-column hit regions present + all toggles
      OFF (an empty filter = all data).
  (B) COORDINATE-click bar 1 (beta): only beta's timeline survives; the other
      three are filtered out; beta's bar stays full while the others mute
      (alpha drops) — proving the click drove BOTH widgets.
  (C) add bar 3 (delta): a filter is a SET — beta AND delta timelines shown,
      alpha + gamma gone; those two bars full, the other two muted.
  (D) drop bar 1: only delta remains (its round-trip did not disturb delta).
  (E) drop bar 3: empty filter again — every timeline back, every bar full.
  (F) keyboard — each bar column is its own Tab stop; Space toggles ONLY the
      focused category; Arrow roves focus.
  (G) BOUNDED / CROSS-widget — a click in the TIMELINE area (the dependent
      widget, below the bars) toggles nothing: the hit surface is the selector's
      bar columns, not the whole window.
  (H) negative — an unknown introspect path is rejected.

`chart.series.{i}` is category i's timeline polyline (a filtered-out category
emits none); `chart.bar.{i}` is its selector bar (its `style.fill.a` drops when
muted); `cat_{i}` is the focusable bar-column hit region whose click toggles i.
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

EXAMPLE = "hello-cross-filter"
VIEWPORT = (640, 480)
N = 4


def cat_tag(i: int) -> str:
    return f"cat_{i}"


def value(tf, i: int):
    """The on/off boolean of category `i` via its `ToggleExternal`."""
    return tf.query(f"/{cat_tag(i)}/external/value")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def line_present(snap, i: int) -> bool:
    """Is category i's timeline (`chart.series.{i}`) in the paint scene?"""
    return find_by_tag(snap, f"chart.series.{i}") is not None


def bar_alpha(snap, i: int) -> int:
    """The alpha of bar i's fill — 255 at full strength, lower when muted."""
    node = find_by_tag(snap, f"chart.bar.{i}")
    assert node is not None, f"bar {i} present in the snapshot"
    fill = (node.get("style") or {}).get("fill")
    assert fill is not None, f"bar {i} carries a queryable style.fill"
    return fill["a"]


def click_bar(tf, snap, i: int) -> None:
    """Click bar `i` at its column CENTRE — a real coordinate click through the
    bar chart's hit geometry, not a tag shortcut."""
    hit = find_by_tag(snap, cat_tag(i))
    assert hit is not None, f"bar column {i} present with a rect"
    tf.click(at=node_center(hit))
    tf.pointer_leave()


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: no filter — all timelines, all bars full, all OFF ──────
        snap = paint(tf)
        for i in range(N):
            hit = find_by_tag(snap, cat_tag(i))
            assert hit is not None, f"bar column {i} present"
            assert hit.get("rect"), f"bar column {i} has hit geometry (a rect)"
            assert_eq(value(tf, i), False, f"boot: category {i} unselected")
            assert line_present(snap, i), f"boot: category {i} timeline drawn"
            assert_eq(bar_alpha(snap, i), 255, f"boot: bar {i} full strength")

        # ── (B) coordinate-click bar 1 (beta): filters BOTH widgets ──────────
        click_bar(tf, snap, 1)
        assert_eq(value(tf, 1), True, "coordinate click on bar 1 selects beta")
        assert_eq(value(tf, 0), False, "category 0 untouched")
        snap = paint(tf)
        assert line_present(snap, 1), "beta's timeline stays (it is selected)"
        for i in (0, 2, 3):
            assert not line_present(snap, i), f"category {i} timeline filtered out"
        assert_eq(bar_alpha(snap, 1), 255, "the selected bar stays full")
        assert bar_alpha(snap, 0) < 255, "an unselected bar mutes"
        assert bar_alpha(snap, 2) < 255, "an unselected bar mutes"
        assert bar_alpha(snap, 3) < 255, "an unselected bar mutes"

        # ── (C) add bar 3 (delta): a filter is a SET ─────────────────────────
        click_bar(tf, snap, 3)
        assert_eq(value(tf, 3), True, "delta joins the selection")
        assert_eq(value(tf, 1), True, "beta still selected (a set, not a radio)")
        snap = paint(tf)
        assert line_present(snap, 1), "beta timeline kept"
        assert line_present(snap, 3), "delta timeline kept"
        assert not line_present(snap, 0), "alpha still filtered out"
        assert not line_present(snap, 2), "gamma still filtered out"
        assert_eq(bar_alpha(snap, 1), 255, "beta bar full")
        assert_eq(bar_alpha(snap, 3), 255, "delta bar full")
        assert bar_alpha(snap, 0) < 255, "alpha bar muted"
        assert bar_alpha(snap, 2) < 255, "gamma bar muted"

        # ── (D) drop bar 1 (beta): only delta remains ────────────────────────
        click_bar(tf, snap, 1)
        assert_eq(value(tf, 1), False, "beta deselected")
        assert_eq(value(tf, 3), True, "delta's selection undisturbed")
        snap = paint(tf)
        assert line_present(snap, 3), "delta timeline stays"
        assert not line_present(snap, 1), "beta timeline now filtered out"
        assert_eq(bar_alpha(snap, 3), 255, "delta bar full")
        assert bar_alpha(snap, 1) < 255, "beta bar muted"

        # ── (E) drop bar 3: empty filter again — everything back ─────────────
        click_bar(tf, snap, 3)
        assert_eq(value(tf, 3), False, "delta deselected -> empty filter")
        snap = paint(tf)
        for i in range(N):
            assert line_present(snap, i), f"category {i} timeline returns (no filter)"
            assert_eq(bar_alpha(snap, i), 255, f"bar {i} full again (no filter)")

        # ── (F) keyboard: each bar column is its own Tab stop ────────────────
        focused = tf.request("focus/set", {"tag": cat_tag(2)}).result.get("focused")
        assert_eq(focused, cat_tag(2), "bar column 2 is an independent Tab stop")
        tf.key(path=cat_tag(2), name="Space")
        assert_eq(value(tf, 2), True, "Space selects the focused category 2")
        assert_eq(value(tf, 0), False, "sibling 0 untouched by Space")
        snap = paint(tf)
        assert line_present(snap, 2), "gamma timeline is the only one now"
        assert not line_present(snap, 0), "alpha filtered out by the gamma selection"
        # Enter toggles it back off -> empty filter.
        tf.key(path=cat_tag(2), name="Enter")
        assert_eq(value(tf, 2), False, "Enter deselects category 2 again")
        # Arrow roves FOCUS across columns (no selection change).
        tf.key(path=cat_tag(2), name="ArrowRight")
        assert_eq(
            tf.request("focus/get").result.get("focused"),
            cat_tag(3),
            "ArrowRight moves focus 2 -> 3",
        )
        assert_eq(value(tf, 3), False, "roving focus does not select category 3")

        # ── (G) BOUNDED / cross-widget: a click in the TIMELINE toggles nothing
        before = [value(tf, i) for i in range(N)]
        # A point well inside the dependent line chart (bottom half), clearly
        # below the selector bars. If the hit surface were "the whole window"
        # this would toggle a category.
        tf.click(at=(VIEWPORT[0] / 2, VIEWPORT[1] * 0.75))
        tf.pointer_leave()
        for i in range(N):
            assert_eq(
                value(tf, i),
                before[i],
                f"a timeline-area click leaves category {i} unchanged "
                f"(the hit surface is the selector, not the dependent view)",
            )

        # ── (H) negative — unknown introspect path is rejected ───────────────
        raised = False
        try:
            tf.query(f"/{cat_tag(0)}/external/no_such_field")
        except Exception:  # noqa: BLE001 — RpcError type varies by harness
            raised = True
        assert raised, "unknown introspect path must be rejected"


if __name__ == "__main__":
    sys.exit(run_demo("R1384 — click a bar to cross-filter the timeline", body))
