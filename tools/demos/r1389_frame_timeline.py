#!/usr/bin/env python3
"""R1389 §5.16 §5.7 — the Timeline track view + its draggable playhead.

`pinion-chart` had value forms only (line / bar / scatter / donut / treemap /
sparkline — "how big"). R1389 adds the crate's first TIME form: `Timeline` lays
labelled `Span`s onto horizontal `Lane`s over a shared time ruler, with a
playhead scrubber. `hello-timeline` is the forcing consumer — a FLAME view of
the profiler's already-streaming REAL data (`use_frame_timings`, the seam
`hello-frame-profiler` reads): the four disjoint render sub-phases (build /
encode / acquire / render) run sequentially within a frame, so each frame's
phases abut end-to-end from its cumulative-time offset, each phase on its own
lane. This is the tracing / the engine-Insights track view, built from measured
data rather than a synthetic series.

## The verification idea: geometry as data on non-deterministic input (§2 #7)

Frame timings are wall-clock — never assert "build took 320µs". But the
STRUCTURE is deterministic, and every element is a tagged node the AI reads
without sampling a pixel:

  (A) the ruler + lanes are present the moment the window opens (before any
      frame flows): `timeline.axis.x` (top ruler line), `timeline.grid.x.{k}`
      (a vertical time gridline per tick), `timeline.tick.{k}` (its time
      label), and one `timeline.lane.{i}.label` per phase, named.
  (B) once frames flow, each phase lane fills with span boxes
      (`timeline.lane.{i}.span.{j}`) — filled, stacked so lane 0 sits above
      lane 3, and accumulating (more driven frames -> more spans, up to the
      binding's RECENT cap).
  (C) the boot scrub (0.5) paints the playhead overlay: a stroked line
      (`timeline.playhead`), a readout box (`timeline.playhead.tooltip`), a
      time header (`timeline.playhead.header`), and >= 1 active-lane row
      (`timeline.playhead.value.{i}`).
  (D) driving the playhead over `scene/intervene` moves it: a low fraction
      lands the line at a small x, a high fraction at a large x, and the time
      header changes with it.

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (780, 420)
# Mirrors the binding's CHART_RECT (x=10, w=WIN_W-20) — the playhead's x sits
# inside it; its mid-x separates a "left" scrub from a "right" one.
CHART_MID = 10 + (780 - 20) // 2
# Mirrors the binding's PHASES: the four lanes, in flame order.
PHASES = ("build", "encode", "acquire", "render")


def _snap(d: RpcSubprocess) -> dict:
    return d.snapshot(source="paint", viewport=VIEWPORT)


def _node(snap: dict, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present in the paint snapshot"
    return node


def _count_prefix(snap: dict, prefix: str) -> int:
    n = 0

    def walk(node: dict) -> None:
        nonlocal n
        tag = node.get("tag")
        if isinstance(tag, str) and tag.startswith(prefix):
            n += 1
        for ch in node.get("children", []) or []:
            walk(ch)

    walk(snap)
    return n


def _playhead_x(snap: dict) -> int:
    return _node(snap, "timeline.playhead")["rect"]["x"]


def _header(snap: dict) -> str:
    content = _node(snap, "timeline.playhead.header").get("content")
    assert content is not None, "the playhead header carries a time string"
    return content


# One redraw driver alternates the scrub between two values so every call is a
# distinct state -> the view re-runs -> a frame is recorded. hello-timeline has
# no repaint button (its scrub IS the driver), the intervene analogue of
# r907's click.
_TOGGLE = [0.65]


def _drive_redraw(d: RpcSubprocess) -> None:
    _TOGGLE[0] = 0.35 if _TOGGLE[0] > 0.5 else 0.65
    try:
        d.intervene("/external/value", _TOGGLE[0])
    except RpcError:
        pass


def _drive_frame_beyond(d: RpcSubprocess, baseline: int, desc: str) -> int:
    """Drive redraw scrubs until frame_count strictly exceeds `baseline`
    (R883 zero-flake: the observed profiler counter, not wall-clock, is the
    gate). Returns the reached count."""

    def advanced() -> int | None:
        try:
            fc = int(d.frame_timings()["frame_count"])
        except RpcError:
            fc = -1
        if fc > baseline:
            return fc
        _drive_redraw(d)
        return None

    return wait_until(advanced, desc=desc)


def _wait_available(d: RpcSubprocess) -> None:
    """Poll until the window has painted a frame and `scene/frame_timings`
    is available (no longer the bootstrap-unavailable state)."""

    def poll() -> bool:
        try:
            d.frame_timings()
            return True
        except RpcError:
            _drive_redraw(d)
            return False

    wait_until(poll, desc="scene/frame_timings becomes available")


def body() -> None:
    with RpcSubprocess("hello-timeline", boot_grace=1.5) as d:
        _wait_available(d)

        # ── (A) ruler + lanes exist before any span flows ────────────────────
        snap = _snap(d)
        assert find_by_tag(snap, "timeline") is not None, "the timeline root"
        assert find_by_tag(snap, "timeline_scrub") is not None, "the scrub surface"
        assert _node(snap, "timeline.axis.x")["type"] == "Path", "top ruler is a path"
        assert find_by_tag(snap, "timeline.axis.y") is not None, "left gutter edge"
        assert find_by_tag(snap, "timeline.grid.x.0") is not None, "a time gridline"
        tick0 = _node(snap, "timeline.tick.0")
        assert_eq(tick0["type"], "Text", "a ruler tick is a text label")
        assert tick0.get("content"), "the ruler tick names a time"
        for i, name in enumerate(PHASES):
            label = _node(snap, f"timeline.lane.{i}.label")
            assert_eq(label["type"], "Text", f"lane {i} label is text")
            assert_eq(label.get("content"), name, f"lane {i} is named {name}")

        # ── (B) frames flow -> each phase lane fills, stacked + accumulating ──
        base = int(d.frame_timings()["frame_count"])
        _drive_frame_beyond(d, base + 5, "populate the flame")
        snap = _snap(d)
        for i in range(len(PHASES)):
            span0 = _node(snap, f"timeline.lane.{i}.span.0")
            assert_eq(span0["type"], "Box", f"lane {i} span 0 is a box")
            assert span0["style"]["fill"] is not None, f"lane {i} span 0 is filled"
        # The lanes stack top-to-bottom: build (lane 0) sits above render (lane 3).
        y_build = _node(snap, "timeline.lane.0.span.0")["rect"]["y"]
        y_render = _node(snap, "timeline.lane.3.span.0")["rect"]["y"]
        assert y_build < y_render, (
            f"lane 0 (build) must sit above lane 3 (render): y {y_build} < {y_render}"
        )
        # Several frames drove, so the build lane carries several spans (and no
        # more than the binding's RECENT=24 cap).
        build_spans = _count_prefix(snap, "timeline.lane.0.span.")
        assert 2 <= build_spans <= 24, (
            f"the build lane accumulates spans up to the RECENT cap; got {build_spans}"
        )

        # ── (C) the playhead overlay ─────────────────────────────────────────
        head = _node(snap, "timeline.playhead")
        assert_eq(head["type"], "Path", "the playhead is a path")
        assert head["style"]["stroke"] is not None, "the playhead line is stroked"
        assert _node(snap, "timeline.playhead.tooltip")["type"] == "Box", "readout box"
        assert _header(snap), "the playhead names a time"
        assert _count_prefix(snap, "timeline.playhead.value.") >= 1, (
            "the playhead names at least one active lane/frame"
        )

        # ── (D) driving the playhead moves it ────────────────────────────────
        d.intervene("/external/value", 0.05)
        assert abs(d.query("/external/value") - 0.05) < 0.02, "scrub set to 0.05"
        low = wait_until(
            lambda: (lambda s: s if _playhead_x(s) < CHART_MID else None)(_snap(d)),
            desc="playhead moves into the left half",
        )
        low_x = _playhead_x(low)
        low_header = _header(low)

        d.intervene("/external/value", 0.95)
        assert abs(d.query("/external/value") - 0.95) < 0.02, "scrub set to 0.95"
        high = wait_until(
            lambda: (lambda s: s if _playhead_x(s) > CHART_MID else None)(_snap(d)),
            desc="playhead moves into the right half",
        )
        high_x = _playhead_x(high)
        high_header = _header(high)

        assert low_x < high_x, (
            f"the playhead moves right as the scrub advances: {low_x} -> {high_x}"
        )
        assert low_header != high_header, (
            f"the scrubbed time changes with the playhead: "
            f"{low_header!r} -> {high_header!r}"
        )
        # And the spans survive a re-domain-free scrub (the flame is unchanged
        # by moving the playhead — only the overlay moves).
        assert find_by_tag(high, "timeline.lane.0.span.0") is not None, (
            "the flame persists across a playhead move"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1389 §5.16 — Timeline track view + playhead", body))
