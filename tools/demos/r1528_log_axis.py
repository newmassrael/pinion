#!/usr/bin/env python3
"""R1528 §5.38 — the value axis can be logarithmic, and says so as data.

`pinion-chart` had one mapping, `LinearScale`, for eleven chart types. That
is the right default and the wrong one for the data a profiler carries:
latency percentiles spanning decades. `hello-log-chart` plots three of them
(`p50` ~0.4 ms, `p99` ~40 ms, `p99.9` ~700 ms) and toggles the value axis
between linear and logarithmic, so the comparison this round exists for is
one click apart in the same window.

What this script checks, and why each check discriminates:

* **The labels are the discriminator, not the spacing.** Gridlines are
  evenly spaced on BOTH axes — asserting even spacing alone would pass on a
  linear axis and prove nothing. What only a log axis produces is evenly
  spaced lines whose LABELS are `0.1 / 1 / 10 / … / 10k`: equal pixel
  distances carrying equal ratios. Both facts are read here, together.
* **The x-axis must not move.** `y_log()` names one axis. The x labels are
  captured on both scales and required to be identical, so a change that
  logged both axes would fail rather than look like a success.
* **A zero sample has no pixel, and is reported.** Bucket 6 took no traffic,
  so `p50` is 0 there. On the log axis that sample contributes no vertex to
  the polyline (11 of 12), and the caption — which the binding renders from
  `LineChart::off_scale`, not from a restated rule — names it. On the linear
  axis the same data draws 12 vertices and the caption says nothing was
  dropped: being off-scale is a property of the AXIS.
* **The minor gridlines are their own tag namespace.** `.grid.minor.y.{k}`
  rather than more `.grid.y.{k}`, so every pre-R1528 gridline assertion in
  the tree still counts what it always counted.

the toolkit reference: log value axis, attached to a series' value axis. What is
different here is that the resolved axis — its decades, its subdivisions,
and the samples it could not carry — is all scene data over RPC (§2 #1,
§2 #7), so an agent verifies the axis without sampling a pixel.

Run from the workspace root:
    cargo build -p hello-log-chart --release
    python3 tools/demos/r1528_log_axis.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
)

VIEWPORT = (640, 420)
SCALE_TAG = "scale_toggle"

#: The decades the positive latency data (0.38 .. 1600 ms) brackets OUT to.
#: Six, not five: the top spike at 1600 ms opens the `1k` decade, and a log
#: axis snaps outward to whole decades exactly as the linear one snaps to
#: whole nice steps. The empty room above 1600 is the cost of round labels,
#: and it is the same trade `nice_ticks` already makes.
DECADES = ["0.1", "1", "10", "100", "1k", "10k"]

#: Buckets in the dataset, and the one with no traffic.
BUCKETS = 12
IDLE_BUCKET = 6


def node(snap, tag: str) -> dict:
    found = find_by_tag(snap, tag)
    assert found is not None, f"tag {tag!r} present in paint snapshot"
    return found


def text_of(snap, tag: str) -> str:
    n = node(snap, tag)
    assert_eq(n["type"], "Text", f"{tag} is a text node")
    return n["content"]


def y_labels(snap) -> list[str]:
    """Every `chart.label.y.{k}`, in axis order."""
    out = []
    k = 0
    while (n := find_by_tag(snap, f"chart.label.y.{k}")) is not None:
        out.append(n["content"])
        k += 1
    return out


def x_labels(snap) -> list[str]:
    out = []
    k = 0
    while (n := find_by_tag(snap, f"chart.label.x.{k}")) is not None:
        out.append(n["content"])
        k += 1
    return out


def gridline_ys(snap, prefix: str) -> list[float]:
    """The window-y of each gridline under `prefix`, in axis order."""
    out = []
    k = 0
    while (n := find_by_tag(snap, f"{prefix}{k}")) is not None:
        out.append(float(n["rect"]["y"]))
        k += 1
    return out


def vertex_count(snap, tag: str) -> int:
    n = node(snap, tag)
    assert_eq(n["type"], "Path", f"{tag} is a path node")
    return sum(1 for c in n["commands"] if "point" in c)


def series_height(snap, tag: str) -> int:
    return int(node(snap, tag)["rect"]["h"])


def body() -> None:
    with RpcSubprocess("hello-log-chart") as d:
        # ── Phase 1 — the log axis (the boot state) ──────────────────
        log = d.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(log, "chart") is not None, "chart container present"

        # The axis is labelled in decades. This — not the spacing — is what
        # a linear axis cannot produce.
        assert_eq(y_labels(log), DECADES, "y-axis labels are decades")

        # And those decades are evenly spaced: equal ratios, equal pixels.
        grid = gridline_ys(log, "chart.grid.y.")
        assert_eq(len(grid), len(DECADES), "one labelled gridline per decade")
        gaps = [abs(b - a) for a, b in zip(grid, grid[1:])]
        assert len(gaps) == len(DECADES) - 1, f"a gap between each decade: {gaps}"
        for i, g in enumerate(gaps):
            assert abs(g - gaps[0]) <= 1.0, (
                f"decade {i} spans {g}px vs {gaps[0]}px for the first — a log "
                "axis gives every decade the same pixel span"
            )
        # A decade is a real span, not a degenerate axis collapsed to a point.
        assert gaps[0] > 20.0, f"a decade spans {gaps[0]}px"

        # The subdivisions inside each decade, in their OWN tag namespace so
        # `chart.grid.y.{k}` keeps counting only the labelled lines.
        minors = gridline_ys(log, "chart.grid.minor.y.")
        assert_eq(len(minors), 40, "eight subdivisions in each of five decades")
        assert find_by_tag(log, "chart.grid.minor.x.0") is None, (
            "the x-axis is still linear, so it contributes no minor ticks"
        )
        # Minor lines sit strictly between the decades they subdivide.
        assert min(grid) <= min(minors) and max(minors) <= max(grid), (
            "every subdivision lies inside the decade range"
        )

        # The zero sample has no pixel: 11 of 12 buckets reach the polyline.
        assert_eq(
            vertex_count(log, "chart.series.0"),
            BUCKETS - 1,
            "the p50 polyline skips the bucket with no traffic",
        )
        for i in (1, 2):
            assert_eq(
                vertex_count(log, f"chart.series.{i}"),
                BUCKETS,
                f"series {i} is positive throughout, so nothing is dropped",
            )

        # ...and it is REPORTED rather than drawn on the baseline. The caption
        # is rendered from `LineChart::off_scale`, so this reads the crate's
        # own answer, not a restatement of the rule.
        cap = text_of(log, "caption")
        assert "log scale" in cap, f"caption names the scale: {cap}"
        assert "1 sample not plotted" in cap, f"caption counts it: {cap}"
        assert "p50" in cap, f"caption names the series: {cap}"
        assert str(IDLE_BUCKET) in cap, f"caption names the bucket: {cap}"

        log_p50 = series_height(log, "chart.series.0")
        log_x = x_labels(log)
        assert len(log_x) >= 2, f"the x-axis is labelled: {log_x}"

        # ── Phase 2 — one click, and the same data on a linear axis ──
        d.click(path=SCALE_TAG)
        lin = d.snapshot(source="paint", viewport=VIEWPORT)

        lin_y = y_labels(lin)
        assert lin_y != DECADES, f"the y-axis is no longer decades: {lin_y}"
        assert "0" in lin_y, f"a linear value axis reaches zero: {lin_y}"
        assert_eq(
            find_by_tag(lin, "chart.grid.minor.y.0"),
            None,
            "a linear axis has no minor ticks, so it draws none",
        )

        # ★ The x-axis did NOT move. `y_log()` names one axis, and a change
        # that logged both would pass every check above.
        assert_eq(x_labels(lin), log_x, "the x-axis is untouched by y_log")

        # The zero sample is an ordinary value here — all 12 buckets draw.
        assert_eq(
            vertex_count(lin, "chart.series.0"),
            BUCKETS,
            "zero is plottable on a linear axis",
        )
        lin_cap = text_of(lin, "caption")
        assert lin_cap.startswith("linear scale"), f"got {lin_cap}"
        assert "not plotted" not in lin_cap, (
            f"nothing is off-scale on a linear axis: {lin_cap}"
        )

        # ★ The whole round in one number: the same series, the same plot,
        # and four times the room to be read in.
        lin_p50 = series_height(lin, "chart.series.0")
        assert lin_p50 < 10, (
            f"linear: p50 spans {lin_p50}px — pressed onto the baseline by p99.9"
        )
        assert log_p50 > lin_p50 * 4, (
            f"log: p50 spans {log_p50}px, over 4x the linear {lin_p50}px"
        )
        # The dominant series keeps its room on both — the log axis reveals
        # the small series without hiding the big one.
        assert series_height(lin, "chart.series.2") > 40, "p99.9 fills the plot"
        assert series_height(log, "chart.series.2") > 20, "p99.9 stays readable"

        # ── Phase 3 — the toggle is a real, reversible control ───────
        d.click(path=SCALE_TAG)
        back = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(y_labels(back), DECADES, "clicking again returns to the log axis")
        assert_eq(
            len(gridline_ys(back, "chart.grid.minor.y.")),
            40,
            "and brings its subdivisions back",
        )
        assert_eq(
            vertex_count(back, "chart.series.0"),
            BUCKETS - 1,
            "and drops the zero sample again",
        )
        assert_eq(text_of(back, "caption"), cap, "and reports the same sample")

        # The axis scale reaches assistive tech as a pressed toggle button,
        # not only as pixels.
        access = d.request("scene/access", {}).result
        chip = access_node_by_tag(access, SCALE_TAG)
        assert chip is not None, "the scale toggle is in the access tree"
        assert_eq(chip["role"], "button", "exposed as a toggle button")
        assert_eq(chip.get("name"), "logarithmic y-axis", "named to AT")
        assert chip.get("state", {}).get("checked") is True, (
            f"and checked, because the axis IS logarithmic: {chip.get('state')}"
        )
        group = access_node_by_tag(access, "scale_group")
        assert group is not None, "the toggle is grouped"
        assert_eq(group.get("name"), "Value axis scale", "under what it controls")


if __name__ == "__main__":
    run_demo("r1528 logarithmic value axis", body)
