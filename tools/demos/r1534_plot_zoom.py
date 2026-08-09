#!/usr/bin/env python3
"""R1534 §5.38 §5.45 §2 #7 — a plot can be zoomed and panned.

A window over the data has existed since the R1357 brush: a `Brush` resolves a
`(low, high)` fraction pair onto the data extent and the consumer re-domains
the chart with it. But a brush is an **overview strip** — a second widget below
the plot, dragged by two thumbs. Nothing could zoom or pan **the plot itself**,
which is where the toolkit's charting module puts it (`zoomIn` / `scroll` / `zoomReset`) and
where d3 puts it (`d3.zoom` on the plot area).

The difference is not only which pixels take the gesture. A strip cannot zoom
**about a point**, and that is the whole feel of a wheel zoom: the minute of the
incident under the reader's cursor keeps its pixel while four hours spread
around it. Two thumbs cannot express "keep this fixed".

## What this drives

`hello-time-chart` — four hours of request latency across an incident, on the
R1529 UTC time axis. A `PlotZoomExternal` laid over the axis area owns a
`PlotWindow`; the wheel vocabulary is deliberately `hello-node-editor`'s
(R877), because a second dialect for the same input is the divergence this repo
keeps catching:

  * `Ctrl`+wheel — zoom, anchored at the cursor.
  * `Shift`+wheel — pan.
  * plain wheel — **declined**, so a chart in a scrolling dashboard never
    steals the page scroll.

## Verification scope (>= 30 assertions, sections A-H)

  (A) Premise, and the NO-REGRESSION claim: an unzoomed plot paints exactly the
      axis R1529 asserted. The seam is additive — pinning the raw extent is not
      the same as letting the axis derive it (a derived domain is nice-rounded),
      so a zoom that always re-domained would silently change every plot.
  (B) `Ctrl`+wheel narrows the window by the documented factor per notch.
  (C) THE ANCHOR — the fraction under the cursor keeps its place. Asserted at
      three different cursor positions against the window arithmetic, which is
      also what pins the overlay to `plot_area` (the axis) rather than to the
      chart rect: an anchor measured against the outer rect is off by the width
      of the y-label gutter, and these three cases separate the two.
  (D) The ticks RE-GRANULATE. The full window is labelled on the half hour;
      zoomed in, the axis picks a finer step. This is what says the zoom reached
      the axis and not merely the marks.
  (E) `Shift`+wheel pans without changing the span, and clamps at the extent.
  (F) A plain wheel is DECLINED — the window does not move.
  (G) The magnification ceiling holds, and `Escape` resets (`zoomReset`).
  (H) The window is axis-kind agnostic: toggling to the numeric axis shows the
      SAME window in that axis's own vocabulary.
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

VIEWPORT = (680, 430)
PLOT_TAG = "plot"
AXIS_TAG = "axis_toggle"
LOW = "/plot/external/low"
HIGH = "/plot/external/high"
SPAN = "/plot/external/span"

#: `hello-time-chart`'s `ZOOM_PER_NOTCH`, mirrored rather than imported — a
#: demo that read the constant out of the code under test could not catch the
#: code changing it.
ZOOM_PER_NOTCH = 1.25
#: `hello-time-chart`'s `PAN_PER_NOTCH`, as a fraction of the visible window.
PAN_PER_NOTCH = 0.15
#: `PlotWindow::DEFAULT_MIN_SPAN` — the 25x magnification ceiling.
MIN_SPAN = 0.04

#: The half-hourly labels R1529 pinned for the unzoomed four-hour window. The
#: fifth crosses midnight and names the day instead of the clock.
FULL_LABELS = [
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

TOL = 1e-4


def close(got: float, want: float, what: str) -> None:
    assert abs(got - want) < TOL, f"{what}: expected {want}, got {got}"


def snapshot(tf: RpcSubprocess) -> dict:
    res = tf.snapshot(source="paint", viewport=VIEWPORT)
    assert res, "paint snapshot returned no result"
    return res


def window(tf: RpcSubprocess) -> tuple[float, float]:
    return (float(tf.query(LOW)), float(tf.query(HIGH)))


def axis_labels(snap: dict) -> list[str]:
    out = []
    k = 0
    while (n := find_by_tag(snap, f"chart.label.x.{k}")) is not None:
        out.append(n["content"])
        k += 1
    return out


def plot_rect(snap: dict) -> dict:
    """The wheel target's rect — the axis area, not the chart rect."""
    n = find_by_tag(snap, PLOT_TAG)
    assert n is not None, "the zoom target is in the paint tree"
    return n["rect"]


def wheel_at(
    tf: RpcSubprocess,
    fraction: float,
    notches: float,
    *,
    ctrl: bool = False,
    shift: bool = False,
) -> None:
    """One wheel event `notches` forward, with the cursor at `fraction` across
    the AXIS (not the chart rect) — the anchor a zoom pivots about."""
    rect = plot_rect(snapshot(tf))
    x = float(rect["x"]) + fraction * float(rect["w"])
    y = float(rect["y"]) + float(rect["h"]) / 2.0
    if ctrl or shift:
        tf.modifiers(ctrl=ctrl, shift=shift)
    tf.wheel(at=(x, y), lines=(0.0, -notches))
    tf.tick(0.016)
    if ctrl or shift:
        tf.modifiers()


def reset(tf: RpcSubprocess) -> None:
    tf.key(path=PLOT_TAG, name="Escape")
    tf.tick(0.016)


def body() -> None:
    with RpcSubprocess("hello-time-chart", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) premise + no regression ──────────────────────────────
        lo, hi = window(tf)
        close(lo, 0.0, "boots at the full extent")
        close(hi, 1.0, "boots at the full extent")
        close(float(tf.query(SPAN)), 1.0, "span")
        snap = snapshot(tf)
        assert_eq(
            axis_labels(snap),
            FULL_LABELS,
            "an unzoomed plot paints the axis R1529 asserted, label for label "
            "— the zoom seam is additive, and a version that re-domained even "
            "at full extent would nice-round this axis into a different one",
        )
        # The no-regression claim bites on the NUMERIC axis, not on this one:
        # measured, the time axis is indifferent to being handed its own extent
        # (this window starts on a nice boundary, so `nice_time_domain` returns
        # what it was given), while the decimal nice-number step widens the
        # numeric domain and draws 9 gridlines where a pinned one draws 7. A
        # zoom that re-domained even at full extent would change an unzoomed
        # plot, and THIS is where that shows.
        tf.click(path=AXIS_TAG)
        tf.tick(0.016)
        assert_eq(
            len(axis_labels(snapshot(tf))),
            9,
            "the unzoomed numeric axis is the derived one (9 gridlines); a "
            "binding that pinned the raw extent at full window would draw 7",
        )
        tf.click(path=AXIS_TAG)
        tf.tick(0.016)
        assert_eq(
            axis_labels(snapshot(tf)),
            FULL_LABELS,
            "and toggling back restores the time axis unchanged",
        )

        rect = plot_rect(snap)
        assert float(rect["w"]) > 0.0, "the zoom target has a width"
        target = find_by_tag(snap, PLOT_TAG)
        assert_eq(target["type"], "Box", "the target is the shared capture surface")
        assert_eq(
            target["style"]["fill"]["a"],
            0,
            "and it paints NOTHING — an interaction surface that tinted the "
            "plot would be a visual change smuggled in with an input seam",
        )
        # The target must cover the AXIS, not the chart rect: the chart's own
        # margins hold the y labels, and an anchor measured across them pivots
        # about the wrong instant. Asserted as an inequality against the outer
        # rect rather than as pixel constants, so it survives a re-layout.
        assert float(rect["x"]) > 16.0, (
            f"the target starts INSIDE the chart rect's left margin, got "
            f"x={rect['x']}"
        )
        assert float(rect["w"]) < float(VIEWPORT[0] - 32), (
            f"and is narrower than the chart rect, got w={rect['w']}"
        )

        # ── (B) Ctrl+wheel zooms by the documented factor ────────────
        wheel_at(tf, 0.5, 1, ctrl=True)
        close(float(tf.query(SPAN)), 1.0 / ZOOM_PER_NOTCH, "one notch = one factor")
        lo, hi = window(tf)
        close(lo, 0.1, "centred on the anchor")
        close(hi, 0.9, "centred on the anchor")
        wheel_at(tf, 0.5, 3, ctrl=True)
        close(
            float(tf.query(SPAN)),
            ZOOM_PER_NOTCH**-4,
            "three more notches in one event compound continuously",
        )
        wheel_at(tf, 0.5, -2, ctrl=True)
        close(float(tf.query(SPAN)), ZOOM_PER_NOTCH**-2, "and back out again")

        # ── (C) THE ANCHOR ───────────────────────────────────────────
        for fraction in (0.2, 0.5, 0.8):
            reset(tf)
            lo, hi = window(tf)
            close(hi - lo, 1.0, f"premise: full before anchoring at {fraction}")
            pivot_before = lo + fraction * (hi - lo)
            wheel_at(tf, fraction, 2, ctrl=True)
            lo, hi = window(tf)
            pivot_after = lo + fraction * (hi - lo)
            close(
                pivot_after,
                pivot_before,
                f"the fraction under the cursor at {fraction} keeps its place "
                f"— an overlay on the chart rect instead of the axis would "
                f"pivot about a value the cursor is not on",
            )
            close(hi - lo, ZOOM_PER_NOTCH**-2, f"span at {fraction}")

        reset(tf)
        wheel_at(tf, 0.0, 2, ctrl=True)
        lo, hi = window(tf)
        close(lo, 0.0, "anchored at the left edge, the window stays flush left")
        close(hi, ZOOM_PER_NOTCH**-2, "and is one span wide")

        # Where the anchor must GIVE WAY: flush against an edge, a zoom-out
        # anchored away from that edge would put the window outside the extent.
        # Both assertions are needed — `low == 0.0` is also its unmoved value,
        # so on its own it would pass for a zoom that never happened.
        reset(tf)
        wheel_at(tf, 0.5, 4, ctrl=True)
        wheel_at(tf, 0.5, 40, shift=True)
        lo, hi = window(tf)
        close(lo, 0.0, "premise: panned flush against the left edge")
        span = hi - lo
        wheel_at(tf, 0.9, -1, ctrl=True)
        lo2, hi2 = window(tf)
        close(hi2 - lo2, span * ZOOM_PER_NOTCH, "the zoom-out did happen")
        close(
            lo2,
            0.0,
            "and the window stayed inside the extent — at an edge the anchor "
            "gives way, the way it does on every map",
        )

        # ── (D) the ticks re-granulate ───────────────────────────────
        reset(tf)
        wheel_at(tf, 0.5, 8, ctrl=True)
        zoomed = axis_labels(snapshot(tf))
        assert zoomed, "the zoomed axis still labels its gridlines"
        assert zoomed != FULL_LABELS, (
            "zooming re-picked the ticks; identical labels would mean the "
            "window reached the marks but not the axis"
        )
        assert any(
            label.endswith(("5", "0")) and label not in FULL_LABELS
            for label in zoomed
        ), f"the step is finer than the half hour: {zoomed}"
        assert all(":" in label or label.startswith("Mar") for label in zoomed), (
            f"and every label is still a clock time or a date: {zoomed}"
        )

        # ── (E) Shift+wheel pans ─────────────────────────────────────
        reset(tf)
        wheel_at(tf, 0.5, 4, ctrl=True)
        lo, hi = window(tf)
        span = hi - lo
        wheel_at(tf, 0.5, 1, shift=True)
        lo2, hi2 = window(tf)
        close(hi2 - lo2, span, "a pan keeps the span")
        close(
            lo2,
            lo - PAN_PER_NOTCH * span,
            "and travels a fraction of what is VISIBLE, so a notch covers the "
            "same share of the plot at every zoom",
        )
        wheel_at(tf, 0.5, -1, shift=True)
        close(window(tf)[0], lo, "and back the other way")

        wheel_at(tf, 0.5, 40, shift=True)
        lo, hi = window(tf)
        close(lo, 0.0, "a huge pan stops flush against the extent")
        close(hi - lo, span, "with its span intact")
        wheel_at(tf, 0.5, 5, shift=True)
        close(window(tf)[0], 0.0, "and stays there")

        # ── (F) a plain wheel is declined ────────────────────────────
        before = window(tf)
        wheel_at(tf, 0.5, 3)
        assert_eq(
            window(tf),
            before,
            "a plain wheel over the plot does NOT move the window — a chart in "
            "a scrolling dashboard must not steal the page scroll",
        )
        wheel_at(tf, 0.5, -3)
        assert_eq(window(tf), before, "in either direction")

        # ── (G) the ceiling, and reset ───────────────────────────────
        reset(tf)
        wheel_at(tf, 0.5, 40, ctrl=True)
        close(float(tf.query(SPAN)), MIN_SPAN, "zoom floors at the 25x ceiling")
        wheel_at(tf, 0.5, 10, ctrl=True)
        close(float(tf.query(SPAN)), MIN_SPAN, "and stays there")
        status = find_by_tag(snapshot(tf), "zoom_status")["content"]
        assert "25x" in status, f"the status names the magnification: {status}"
        assert "Esc" in status, f"and how to get out: {status}"

        reset(tf)
        lo, hi = window(tf)
        close(lo, 0.0, "Escape restored the full extent")
        close(hi, 1.0, "Escape restored the full extent")
        assert_eq(
            axis_labels(snapshot(tf)),
            FULL_LABELS,
            "and the axis is byte-for-byte the one it booted with",
        )
        reset(tf)
        assert_eq(window(tf), (0.0, 1.0), "Escape on an unzoomed plot is a no-op")

        # ── (H) the window is axis-kind agnostic ─────────────────────
        numeric_full = None
        tf.click(path=AXIS_TAG)
        tf.tick(0.016)
        numeric_full = axis_labels(snapshot(tf))
        assert numeric_full != FULL_LABELS, (
            "premise: the numeric axis labels this window differently"
        )
        wheel_at(tf, 0.5, 6, ctrl=True)
        lo, hi = window(tf)
        assert hi - lo < 1.0, f"premise: the numeric plot zoomed too, {lo}..{hi}"
        numeric_zoomed = axis_labels(snapshot(tf))
        assert numeric_zoomed != numeric_full, (
            "the SAME window moved a numeric axis as well — the zoom is a "
            "property of the window, not of one axis kind"
        )
        tf.click(path=AXIS_TAG)
        tf.tick(0.016)
        close(window(tf)[1] - window(tf)[0], hi - lo, "toggling kept the window")
        time_zoomed = axis_labels(snapshot(tf))
        assert time_zoomed != numeric_zoomed, (
            "and each kind draws that one window in its own vocabulary"
        )


if __name__ == "__main__":
    run_demo("R1534 §5.38 — a plot can be zoomed and panned", body)
