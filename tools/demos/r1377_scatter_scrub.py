#!/usr/bin/env python3
"""R1377 §5.16 §5.7 — the ScatterChart correlation form + its scrub inspector.

`pinion-chart` had line (time-series), bar / histogram (distribution), and donut
(part-of-whole), but no CORRELATION form. R1377 adds `ScatterChart`: one filled
circle per sample, numeric x AND y axes, and the R1355 scrub inspect (now a
crosshair + a ring per series' nearest point + a value tooltip). Being the third
CARTESIAN chart is what forced the shared axis furniture (gridlines / axes /
y-labels) to factor out of the line and bar charts, and — as the third legend and
circle-marker consumer — collapsed the legend row + circle geometry to one
definition too. `hello-scatter` is the forcing consumer — two labelled sample
series driven through the real windowed shell + RPC.

## The verification idea: geometry as data (§2 #7)

Every element is a tagged node whose shape the AI reads without sampling a pixel:

  (A) one filled circle per point (`chart.point.{i}.{j}`) + a legend per series.
  (B) each point is a CLOSED cubic-Bézier circle (the shared circle geometry,
      not a square marker).
  (C) numeric x AND y axes => BOTH gridline families exist (unlike the
      categorical bar chart, which has no x-gridlines).
  (D) the boot scrub (0.5) paints the overlay: a crosshair, a stroked ring per
      series, a tooltip box + an `x = …` header + a `name value` row per series.
  (E) driving the scrub over `scene/intervene` moves the focus: a low fraction
      lands the crosshair at a small x, a high fraction at a large x, so the
      crosshair position AND the header both change.

>= 30 assertions.
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

VIEWPORT = (560, 460)
# Mirrors the binding's `samples()`: (series name, point count). 8 points each.
SERIES = [("rising", 8), ("falling", 8)]


def _node(snap, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present in the paint snapshot"
    return node


def _cmd_types(node: dict) -> list[str]:
    return [c["type"] for c in node["commands"]]


def body() -> None:
    with RpcSubprocess("hello-scatter") as d:
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, "chart") is not None, "scatter container present"

        # ── (A) one filled circle per point + a legend per series ────────────
        for i, (name, count) in enumerate(SERIES):
            for j in range(count):
                p = _node(snap, f"chart.point.{i}.{j}")
                assert_eq(p["type"], "Path", f"point {i}.{j} is a path")
                assert p["style"]["fill"] is not None, f"point {i}.{j} is filled"
                assert p["style"]["stroke"] is None, f"point {i}.{j} is not stroked"
            # No phantom point past the series' count.
            assert find_by_tag(snap, f"chart.point.{i}.{count}") is None, (
                f"series {i} has exactly {count} points"
            )
            swatch = _node(snap, f"chart.legend.{i}.swatch")
            assert_eq(swatch["type"], "Box", f"legend {i} swatch is a box")
            assert_eq(
                _node(snap, f"chart.legend.{i}.label").get("content"),
                name,
                f"legend {i} names the series",
            )

        # ── (B) a point is a CLOSED cubic-Bézier circle ──────────────────────
        types = _cmd_types(_node(snap, "chart.point.0.0"))
        assert types[0] == "MoveTo", "a point starts with MoveTo"
        assert types[-1] == "Close", "a point is a closed circle"
        assert types.count("CurveTo") == 4, "the circle is four cubic Bézier arcs"

        # ── (C) numeric x AND y axes => both gridline families ───────────────
        assert find_by_tag(snap, "chart.axis.x") is not None, "x-axis line"
        assert find_by_tag(snap, "chart.axis.y") is not None, "y-axis line"
        assert find_by_tag(snap, "chart.grid.x.0") is not None, "numeric x-gridlines"
        assert find_by_tag(snap, "chart.grid.y.0") is not None, "y-gridlines"

        # ── (D) the boot scrub (0.5) paints the full overlay ─────────────────
        for tag in (
            "chart.inspect.crosshair",
            "chart.inspect.tooltip",
            "chart.inspect.header",
            "chart.inspect.ring.0",
            "chart.inspect.ring.1",
            "chart.inspect.value.0",
            "chart.inspect.value.1",
        ):
            assert find_by_tag(snap, tag) is not None, f"the scrub paints {tag}"
        ring = _node(snap, "chart.inspect.ring.0")
        assert ring["style"]["stroke"] is not None, "the ring is stroked"
        assert ring["style"]["fill"] is None, "the ring frames, it does not cover"
        assert _cmd_types(ring)[-1] == "Close", "the ring is a closed circle"
        header = _node(snap, "chart.inspect.header").get("content")
        assert header is not None and header.startswith("x = "), (
            f"the header leads with the focus x, got {header!r}"
        )
        v0 = _node(snap, "chart.inspect.value.0").get("content")
        assert v0 is not None and "rising" in v0, f"value 0 names its series, got {v0!r}"

        # ── (E) driving the scrub moves the focus ────────────────────────────
        d.intervene("/external/value", 0.05)
        assert abs(d.query("/external/value") - 0.05) < 0.02, "scrub set to 0.05"
        low = d.snapshot(source="paint", viewport=VIEWPORT)
        low_x = _node(low, "chart.inspect.crosshair")["rect"]["x"]
        low_header = _node(low, "chart.inspect.header").get("content")

        d.intervene("/external/value", 0.95)
        assert abs(d.query("/external/value") - 0.95) < 0.02, "scrub set to 0.95"
        high = d.snapshot(source="paint", viewport=VIEWPORT)
        high_x = _node(high, "chart.inspect.crosshair")["rect"]["x"]
        high_header = _node(high, "chart.inspect.header").get("content")

        assert low_x < high_x, (
            f"the crosshair moves right as the scrub advances: {low_x} -> {high_x}"
        )
        assert low_header != high_header, (
            f"the focus x changes with the scrub: {low_header!r} -> {high_header!r}"
        )
        assert low_header.startswith("x = ") and high_header.startswith("x = "), (
            "both headers read as a focus x"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1377 §5.16 — ScatterChart correlation + scrub inspector", body))
