#!/usr/bin/env python3
"""R1440 §5.35 — one colour encoding, two geometries.

R1438 gave a scatter mark its colour from a third channel; R1439 gave a treemap
tile its colour from a second measure. Both were easy in one respect: a mark was
one shape with one colour. A LINE is not. A trace's colour has to change *along*
it, and pinion's scene primitives split the problem in two:

1. **The line becomes one stroked path per SEGMENT.** A stroke takes a flat
   colour — `PathStyle`'s gradient replaces the FILL — so `chart.series.0.seg.{k}`
   is as continuous as a polyline can be made. Each segment is coloured at the
   MEAN of its endpoints' measures, and this demo checks that it is the mean
   rather than either endpoint (they disagree everywhere in this data).
2. **The area keeps one path and takes a real GRADIENT along x**, whose stops sit
   at the samples' own x positions. Checked against the oracle's
   `x_fraction_at`, not against even spacing.

The data is an elevation profile whose height and slope deliberately DISAGREE:
the summit is the highest sample AND the flattest one, so a colour tracking y
would peak exactly where this one goes neutral. Without that the second channel
would be redundant and every assertion here vacuous.

Qt reference: `QLineSeries` carries one pen — Qt Charts has no per-segment line
colour and no third-channel gradient fill, so a heat-line there is custom
`QPainter` work. What is checked below goes past pixels: the segment colours and
the gradient stops ride in the scene as data (§2 #7).

Run from the workspace root:
    cargo build -p hello-elevation-trace --release
    python3 tools/demos/r1440_elevation_trace.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    assert_action_refused,
    assert_rpc_error,
    find_by_tag,
    run_demo,
    wait_until,
)

EXT = "/external"
VIEWPORT = (660, 460)
AREA = "chart.area.0"
STRIP = "chart.colorbar.strip"


def _walk(node, out):
    out.append(node)
    for c in node.get("children", []) or []:
        _walk(c, out)
    if isinstance(node.get("content"), dict):
        _walk(node["content"], out)
    return out


def snap(ta: RpcSubprocess):
    return ta.snapshot(source="paint", viewport=VIEWPORT)


def query(ta: RpcSubprocess, field: str):
    """The harness's own `scene/query` wrapper — no hand-rolled envelope."""
    return ta.query(f"{EXT}/{field}")


def invoke(ta: RpcSubprocess, path: str, arg):
    return ta.invoke(f"{EXT}/{path}", arg)


def set_encoding(ta: RpcSubprocess, mode: str) -> None:
    ta.intervene(f"{EXT}/encoding", mode)
    wait_until(lambda: query(ta, "encoding") == mode, desc=f"encoding -> {mode}")


def set_filled(ta: RpcSubprocess, filled: bool) -> None:
    ta.intervene(f"{EXT}/filled", filled)
    wait_until(lambda: query(ta, "filled") == filled, desc=f"filled -> {filled}")


def color_hex(node: dict) -> str:
    """A stroked path's colour as `#rrggbb` — the form the oracle publishes."""
    stroke = node.get("style", {}).get("stroke")
    assert stroke is not None, f"expected a stroked path, got {node.get('style')}"
    c = stroke["color"]
    return "#{:02x}{:02x}{:02x}".format(c["r"], c["g"], c["b"])


def segment(ta: RpcSubprocess, k: int) -> dict:
    node = find_by_tag(snap(ta), f"chart.series.0.seg.{k}")
    assert node is not None, f"segment {k} is in the paint scene"
    return node


def area_stops(ta: RpcSubprocess) -> list[dict]:
    node = find_by_tag(snap(ta), AREA)
    assert node is not None, "the area mark is in the paint scene"
    gradient = node.get("style", {}).get("gradient")
    assert gradient is not None, "the area carries a real gradient, not a flat fill"
    return gradient["stops"]


def tags_with_prefix(ta: RpcSubprocess, prefix: str) -> list[str]:
    return [
        n["tag"]
        for n in _walk(snap(ta), [])
        if isinstance(n.get("tag"), str) and n["tag"].startswith(prefix)
    ]


def body() -> None:
    with RpcSubprocess("hello-elevation-trace", request_timeout=12.0) as ta:
        # ── Phase 1 — two channels that do NOT co-rank ────────────────
        assert_eq(query(ta, "encoding"), "diverging", "boots on the diverging map")
        assert_eq(query(ta, "filled"), True, "and with the area filled")
        samples = int(query(ta, "sample_count"))
        segments = int(query(ta, "segment_count"))
        assert_eq(segments, samples - 1, "n samples make n-1 segments")
        peak = int(query(ta, "peak_index"))
        steepest = int(query(ta, "steepest_index"))
        assert peak != steepest, (
            "★ the highest sample must not also be the steepest — otherwise "
            f"colour would just restate y (both {peak})"
        )
        assert_eq(invoke(ta, "slope_at", str(peak)), 0.0, "the summit is FLAT")
        assert steepest < peak, "and the climb's steepest stretch is partway up"
        assert invoke(ta, "elevation_at", str(peak)) > invoke(
            ta, "elevation_at", str(steepest)
        ), "the summit really is higher than the steepest sample"

        # ── Phase 2 — the line is one path per segment ────────────────
        seg_tags = tags_with_prefix(ta, "chart.series.0.seg.")
        assert_eq(len(seg_tags), segments, f"one path per segment, got {seg_tags}")
        assert find_by_tag(snap(ta), "chart.series.0") is None, (
            "and the flat polyline is NOT also drawn (it would double-strike)"
        )
        # Every segment is a stroked path with a real width.
        for k in range(segments):
            node = segment(ta, k)
            assert node["style"]["stroke"]["width"] >= 1, f"segment {k} has width"

        # ── Phase 3 — ★ a segment is the MEAN, not an endpoint ────────
        # The refutation channel: if the chart coloured by the start value, the
        # painted colour would equal `endpoint_color_at k` instead.
        differed = 0
        for k in range(segments):
            painted = color_hex(segment(ta, k))
            assert_eq(
                painted,
                invoke(ta, "segment_color_at", str(k)),
                f"segment {k} is the endpoint mean",
            )
            start = invoke(ta, "endpoint_color_at", str(k))
            end = invoke(ta, "endpoint_color_at", str(k + 1))
            assert start != end, f"segment {k}'s endpoints differ in measure"
            if painted != start and painted != end:
                differed += 1
        assert_eq(
            differed,
            segments,
            "★ EVERY segment differs from both its endpoints — the mean is not "
            "silently one of them",
        )
        # Consecutive segments differ, so the trace varies along itself.
        assert color_hex(segment(ta, 0)) != color_hex(segment(ta, 1)), (
            "the colour changes along the trace"
        )

        # ── Phase 4 — the legend became a colour bar ──────────────────
        assert find_by_tag(snap(ta), STRIP) is not None, "a colour bar is painted"
        assert_eq(
            tags_with_prefix(ta, "chart.legend."),
            [],
            "no categorical swatch row while colour encodes magnitude",
        )
        ticks = tags_with_prefix(ta, "chart.colorbar.tick.")
        assert_eq(len(ticks), 3, f"low + neutral + high, got {ticks}")
        neutral_hex = query(ta, "neutral_hex")
        assert_eq(
            invoke(ta, "endpoint_color_at", str(peak)),
            neutral_hex,
            "the flat summit takes the ramp's centre colour",
        )
        offset = query(ta, "neutral_offset")
        assert abs(offset - 0.25) < 1e-9, (
            f"the asymmetric domain seats level ground a quarter along, got {offset}"
        )

        # ── Phase 5 — ★ the area's stops sit at the samples' x ────────
        stops = area_stops(ta)
        assert_eq(len(stops), samples, "one stop per sample")
        offsets = [s["offset"] for s in stops]
        assert all(a < b for a, b in zip(offsets, offsets[1:])), (
            f"stops ascend with x: {offsets}"
        )
        # The profile's x samples are UNEVEN, so this discriminates: a
        # stop-per-index implementation would put stop 1 at 1/8 = 0.125 where the
        # data puts it at 0.05. Count the disagreements to prove it can tell.
        even_would_differ = 0
        for i in range(samples):
            want = invoke(ta, "x_fraction_at", str(i))
            assert abs(offsets[i] - want) < 0.02, (
                f"★ stop {i} sits at the sample's own x fraction "
                f"({offsets[i]} vs {want})"
            )
            if abs(want - i / (samples - 1)) > 0.02:
                even_would_differ += 1
        assert even_would_differ >= 4, (
            "★ the profile must be unevenly sampled for the check above to mean "
            f"anything — only {even_would_differ} stops differ from even spacing"
        )
        # The area's colours agree with the line's endpoints: one encoding.
        first_stop = "#{:02x}{:02x}{:02x}".format(
            stops[0]["color"]["r"], stops[0]["color"]["g"], stops[0]["color"]["b"]
        )
        assert_eq(
            first_stop,
            invoke(ta, "endpoint_color_at", "0"),
            "the gradient's first stop IS sample 0's encoded colour",
        )

        # ── Phase 6 — the fill toggle switches which geometry carries it ─
        set_filled(ta, False)
        assert find_by_tag(snap(ta), AREA) is None, "no area mark when unfilled"
        assert_eq(
            len(tags_with_prefix(ta, "chart.series.0.seg.")),
            segments,
            "but the segmented line is untouched",
        )
        set_filled(ta, True)
        assert_eq(len(area_stops(ta)), samples, "and the gradient comes back")

        # ── Phase 7 — sequential re-maps, off reverts to one polyline ──
        set_encoding(ta, "sequential")
        assert invoke(ta, "endpoint_color_at", str(peak)) != neutral_hex, (
            "★ under a linear map the flat summit is NOT the centre colour — "
            "the asymmetric domain is why map_diverging exists"
        )
        assert_eq(
            len(tags_with_prefix(ta, "chart.series.0.seg.")),
            segments,
            "still segmented",
        )

        set_encoding(ta, "off")
        assert find_by_tag(snap(ta), "chart.series.0") is not None, (
            "★ the flat polyline returns"
        )
        assert_eq(
            tags_with_prefix(ta, "chart.series.0.seg."),
            [],
            "and the segments are gone",
        )
        assert find_by_tag(snap(ta), STRIP) is None, "no bar without an encoding"
        assert find_by_tag(snap(ta), "chart.legend.0.swatch") is not None, (
            "the swatch row returns"
        )
        area = find_by_tag(snap(ta), AREA)
        assert area is not None, "the area is still filled"
        assert area["style"].get("gradient") is None, (
            "but flat — no encoding, no gradient"
        )
        assert_action_refused(
            lambda: invoke(ta, "endpoint_color_at", "0"),
            saying="assigns no colour to that segment",
        )

        set_encoding(ta, "diverging")
        assert_eq(
            invoke(ta, "endpoint_color_at", str(peak)), neutral_hex, "neutral again"
        )

        # ── Phase 8 — the write surface is honest ─────────────────────
        assert_rpc_error(lambda: ta.intervene(f"{EXT}/encoding", "sideways"))
        assert_rpc_error(lambda: ta.intervene(f"{EXT}/filled", "yes"))
        assert_rpc_error(lambda: ta.intervene(f"{EXT}/domain_low", 1.0))
        assert_action_refused(
            lambda: invoke(ta, "slope_at", "99"), saying="no sample 99 in this profile"
        )
        assert_eq(query(ta, "encoding"), "diverging", "a rejected write changed nothing")
        assert_eq(query(ta, "filled"), True, "and neither did the other one")


if __name__ == "__main__":
    sys.exit(run_demo("r1440_elevation_trace", body))
