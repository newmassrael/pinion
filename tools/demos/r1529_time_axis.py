#!/usr/bin/env python3
"""R1529 §5.38 — the x-axis can be UTC time, and says so as data.

`pinion-chart` had two axis kinds after R1528, linear and logarithmic, and
neither is the axis a monitoring chart actually has. A timestamp reaching
either one is a plain number. `hello-time-chart` plots four hours of request
latency across a real incident window — `2026-03-02 22:00` to
`2026-03-03 02:00` UTC — and toggles the x-axis between numeric and time, so
the comparison this round exists for is one click apart in the same window.

What this script checks, and why each check discriminates:

* **The labels are the discriminator, not the spacing.** Gridlines are evenly
  spaced on BOTH axes — asserting even spacing alone would pass on the
  numeric axis and prove nothing. What only a time axis produces is nine
  evenly spaced lines carrying nine DISTINCT clock labels. On the numeric
  axis the same nine lines carry one string, `1772.5G`, repeated: the
  nice-number step is decimal while time above a second is mixed-radix, and
  `format_si` at the giga scale has 27-hour resolution. Both counts are read
  here, on both axes.
* **The date is named exactly once.** The window straddles midnight, so the
  multi-resolution label has something to do: eight ticks name a clock time
  and the one crossing into a new day names `Mar 03`. A fixed format string
  cannot produce that — `HH:MM` everywhere loses the date, and a full stamp
  everywhere repeats it nine times. The count is asserted as exactly one, so
  both degenerate formats fail.
* **The y-axis must not move.** `x_time()` names one axis. The y labels are
  captured on both kinds and required to be identical, so a change that
  timed both axes would fail rather than look like a success.
* **A readout is not a tick label.** The scrub header carries the full stamp
  (`x = 2026-03-02 23:50:00`) where the axis label for that same instant is
  `23:50`. An axis label leans on its neighbours to be legible; a lone
  readout has none, so the two forms genuinely differ — and only on a time
  axis, which is why the distinction did not exist before this round.

Qt reference: `QDateTimeAxis`, attached to a series' horizontal axis (d3's
`scaleUtc`). What is different here is that the resolved axis — its ticks,
its labels, and the readout derived from it — is all scene data over RPC
(§2 #1, §2 #7), so an agent verifies the axis without sampling a pixel.

Run from the workspace root:
    cargo build -p hello-time-chart --release
    python3 tools/demos/r1529_time_axis.py
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

VIEWPORT = (680, 430)
AXIS_TAG = "axis_toggle"

#: The half-hourly ticks the four-hour window brackets, as the multi-resolution
#: formatter names them. The fifth crosses midnight, so it names the day
#: instead of the clock — that one label is the property under test.
CLOCK_LABELS = [
    "22:00",
    "22:30",
    "23:00",
    "23:30",
    "Mar 03",
    "00:30",
    "01:00",
    "01:30",
    "02:00",
]

#: What the numeric axis manages for those same nine gridlines. One decimal at
#: the giga scale is 27-hour resolution, so a four-hour window cannot move it.
NUMERIC_LABEL = "1772.5G"

#: The instant the mid-plot scrub lands on (22:00 + 11 * 10 min).
SCRUB_STAMP = "2026-03-02 23:50:00"


def node(snap, tag: str) -> dict:
    found = find_by_tag(snap, tag)
    assert found is not None, f"tag {tag!r} present in paint snapshot"
    return found


def text_of(snap, tag: str) -> str:
    n = node(snap, tag)
    assert_eq(n["type"], "Text", f"{tag} is a text node")
    return n["content"]


def axis_labels(snap, axis: str) -> list[str]:
    """Every `chart.label.{axis}.{k}`, in axis order."""
    out = []
    k = 0
    while (n := find_by_tag(snap, f"chart.label.{axis}.{k}")) is not None:
        out.append(n["content"])
        k += 1
    return out


def gridline_xs(snap) -> list[float]:
    """The window-x of each vertical gridline, in axis order."""
    out = []
    k = 0
    while (n := find_by_tag(snap, f"chart.grid.x.{k}")) is not None:
        out.append(float(n["rect"]["x"]))
        k += 1
    return out


def body() -> None:
    with RpcSubprocess("hello-time-chart") as d:
        # ── Phase 1 — the time axis (the boot state) ─────────────────
        timed = d.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(timed, "chart") is not None, "chart container present"

        # The axis is labelled in clock time. This — not the spacing — is
        # what a numeric axis cannot produce.
        assert_eq(axis_labels(timed, "x"), CLOCK_LABELS, "x labels are clock times")

        # ★ The date is named exactly ONCE, on the tick that crosses into a
        # new day. `HH:MM` everywhere would give zero; a full stamp
        # everywhere would give nine.
        dated = [lab for lab in CLOCK_LABELS if lab.startswith("Mar")]
        assert_eq(len(dated), 1, "exactly one label carries the date")
        assert_eq(
            axis_labels(timed, "x").index("Mar 03"),
            4,
            "and it sits at the midnight crossing, not at an end",
        )

        # Every gridline is labelled distinctly — nine lines, nine strings.
        timed_x = axis_labels(timed, "x")
        assert_eq(len(timed_x), 9, "nine x gridlines are labelled")
        assert_eq(len(set(timed_x)), 9, f"each one distinctly: {timed_x}")

        # ...and they are evenly spaced, so the labels above describe a real
        # half-hourly ruler rather than a ragged one.
        grid = gridline_xs(timed)
        assert_eq(len(grid), 9, "one gridline per label")
        gaps = [abs(b - a) for a, b in zip(grid, grid[1:])]
        assert_eq(len(gaps), 8, f"a gap between each pair: {gaps}")
        for i, g in enumerate(gaps):
            assert abs(g - gaps[0]) <= 1.0, (
                f"gap {i} is {g}px vs {gaps[0]}px for the first — equal "
                "durations occupy equal pixel spans"
            )
        assert gaps[0] > 20.0, f"a half hour spans {gaps[0]}px"

        # ★ A readout is not a tick label: the scrub header is the full stamp,
        # because it has no neighbouring labels to read the day from.
        header = text_of(timed, "chart.inspect.header")
        assert_eq(header, f"x = {SCRUB_STAMP}", "the scrub says which day")
        assert "23:50" in header, "and which minute"
        assert header.replace("x = ", "") not in timed_x, (
            f"no tick label is the full stamp: {header}"
        )

        cap = text_of(timed, "caption")
        assert cap.startswith("UTC time x-axis"), f"caption names the axis: {cap}"
        assert "9 gridlines, 9 distinct labels" in cap, f"and counts them: {cap}"

        timed_y = axis_labels(timed, "y")
        assert len(timed_y) >= 2, f"the y-axis is labelled: {timed_y}"

        # ── Phase 2 — one click, and the same data on a numeric axis ─
        d.click(path=AXIS_TAG)
        numeric = d.snapshot(source="paint", viewport=VIEWPORT)

        num_x = axis_labels(numeric, "x")
        assert_eq(len(num_x), 9, "the same nine gridline positions")
        assert_eq(len(set(num_x)), 1, f"labelled with ONE string: {num_x}")
        assert_eq(num_x[0], NUMERIC_LABEL, "an epoch ms compacted by magnitude")
        assert num_x != timed_x, "the axis genuinely changed"
        assert not any(":" in lab for lab in num_x), (
            f"nothing on the numeric axis reads as a clock: {num_x}"
        )

        # ★ The y-axis did NOT move. `x_time()` names one axis, and a change
        # that timed both would pass every check above.
        assert_eq(axis_labels(numeric, "y"), timed_y, "the y-axis is untouched")

        # The gridlines are evenly spaced here too — which is exactly why
        # spacing alone could never have been the discriminator.
        num_grid = gridline_xs(numeric)
        assert_eq(len(num_grid), 9, "nine gridlines on the numeric axis as well")
        num_gaps = [abs(b - a) for a, b in zip(num_grid, num_grid[1:])]
        for i, g in enumerate(num_gaps):
            assert abs(g - num_gaps[0]) <= 1.0, (
                f"numeric gap {i} is {g}px vs {num_gaps[0]}px — evenly spaced, "
                "and unreadable anyway"
            )

        # Off a time axis the readout and the label coincide, which is why the
        # distinction did not exist before this round.
        num_header = text_of(numeric, "chart.inspect.header")
        assert_eq(num_header, f"x = {NUMERIC_LABEL}", "the readout is the label")
        assert num_header.replace("x = ", "") in num_x, (
            "and is indistinguishable from every tick on this axis"
        )

        num_cap = text_of(numeric, "caption")
        assert num_cap.startswith("numeric x-axis"), f"got {num_cap}"
        assert "9 gridlines, 1 distinct label" in num_cap, f"and counts it: {num_cap}"

        # ── Phase 3 — the toggle is a real, reversible control ───────
        d.click(path=AXIS_TAG)
        back = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(axis_labels(back, "x"), CLOCK_LABELS, "clicking again returns to time")
        assert_eq(
            text_of(back, "chart.inspect.header"),
            f"x = {SCRUB_STAMP}",
            "and brings the full-stamp readout back",
        )
        assert_eq(text_of(back, "caption"), cap, "and reports the same axis")
        assert_eq(axis_labels(back, "y"), timed_y, "with the y-axis still put")

        # The axis kind reaches assistive tech as a pressed toggle button,
        # not only as pixels.
        access = d.request("scene/access", {}).result
        chip = access_node_by_tag(access, AXIS_TAG)
        assert chip is not None, "the axis toggle is in the access tree"
        assert_eq(chip["role"], "button", "exposed as a toggle button")
        assert_eq(chip.get("name"), "UTC time x-axis", "named to AT")
        assert chip.get("state", {}).get("checked") is True, (
            f"and checked, because the axis IS time: {chip.get('state')}"
        )
        group = access_node_by_tag(access, "axis_group")
        assert group is not None, "the toggle is grouped"
        assert_eq(group.get("name"), "Horizontal axis kind", "under what it controls")


if __name__ == "__main__":
    run_demo("r1529 UTC time axis", body)
