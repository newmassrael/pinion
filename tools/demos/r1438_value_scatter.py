#!/usr/bin/env python3
"""R1438 §5.35 — value-encoded mark colour + a colour bar that reports it.

Every earlier pinion chart colours by SERIES IDENTITY: a categorical palette
answers "which one is this". R1438 adds the other question — "how much is
this" — by giving `DataPoint` a third channel and mapping it through a
`ColorScale`. Two things follow, and this demo asserts both over the wire
rather than describing them:

1. **The legend changes kind.** A swatch row claims "this colour names that
   series", which is false once colour means magnitude. So the chart drops the
   swatches and emits a COLOUR BAR: a gradient strip (`chart.colorbar.strip`)
   plus domain ticks (`chart.colorbar.tick.{k}`).
2. **A diverging encoding is not an evenly-split bar.** The domain here is
   asymmetric (-6 .. +18), so the target (0.0) sits a QUARTER along it. The bar
   is built from the encoding's own stop placement, so its middle stop lands at
   0.25 — while the same scale used sequentially puts its middle stop at 0.50.

The refutation channel is the point: the oracle publishes `color_at` (the live
encoding) AND `linear_color_at` (the same value through a plain linear map).
At the target they DISAGREE, which is the whole reason `map_diverging` exists.
If they agreed, every assertion here would be worthless.

Qt reference: `QXYSeries::setPointConfiguration` (per-point colour) and
`Q3DTheme::ColorStyleRangeGradient` (colour by value), with a colour-scale axis
in the QCustomPlot ecosystem. What is checked below goes past pixels: the ramp
rides in the scene as gradient stops, so the encoding is verifiable as data.

Run from the workspace root:
    cargo build -p hello-value-scatter --release
    python3 tools/demos/r1438_value_scatter.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    assert_action_refused,
    assert_out_of_range,
    assert_rpc_error,
    find_by_tag,
    run_demo,
    wait_until,
)

EXT = "/external"
VIEWPORT = (620, 420)
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


def stop_hex(stop: dict) -> str:
    """A gradient stop's colour as `#rrggbb` — the form the oracle publishes."""
    c = stop["color"]
    return "#{:02x}{:02x}{:02x}".format(c["r"], c["g"], c["b"])


def strip_rect(ta: RpcSubprocess) -> dict:
    node = find_by_tag(snap(ta), STRIP)
    assert node is not None, "the colour bar strip is in the paint scene"
    return node["rect"]


def strip_stops(ta: RpcSubprocess) -> list[dict]:
    """The colour bar's gradient stops, straight out of the paint scene."""
    node = find_by_tag(snap(ta), STRIP)
    assert node is not None, "the colour bar strip is in the paint scene"
    gradient = node.get("style", {}).get("gradient")
    assert gradient is not None, "the strip carries a real gradient, not a flat fill"
    return gradient["stops"]


def tags_with_prefix(ta: RpcSubprocess, prefix: str) -> list[str]:
    return [
        n["tag"]
        for n in _walk(snap(ta), [])
        if isinstance(n.get("tag"), str) and n["tag"].startswith(prefix)
    ]


def body() -> None:
    with RpcSubprocess("hello-value-scatter", request_timeout=12.0) as ta:
        # ── Phase 1 — the encoding and its asymmetric domain ──────────
        assert_eq(query(ta, "encoding"), "diverging", "boots on the diverging map")
        low = query(ta, "domain_low")
        high = query(ta, "domain_high")
        neutral = query(ta, "neutral")
        assert_eq(low, -6.0, "domain low")
        assert_eq(high, 18.0, "domain high")
        assert_eq(neutral, 0.0, "the target is the neutral anchor")
        below, above = neutral - low, high - neutral
        assert above > below, "the domain is ASYMMETRIC — the case a linear map fails"
        assert_eq(above, below * 3.0, "the overshoot wing is 3x the shortfall wing")
        offset = query(ta, "neutral_offset")
        assert abs(offset - 0.25) < 1e-9, f"the target sits a quarter along, got {offset}"

        # ── Phase 2 — the colour bar IS the legend now ────────────────
        assert find_by_tag(snap(ta), STRIP) is not None, "a colour bar is painted"
        bar = strip_rect(ta)
        assert bar["h"] > 0, f"the strip has height (it would be invisible at 0): {bar}"
        assert bar["w"] > 100, f"the strip spans the plot width: {bar}"
        assert_eq(
            [t for t in tags_with_prefix(ta, "chart.legend.")],
            [],
            "no categorical swatch row while colour encodes magnitude",
        )
        ticks = tags_with_prefix(ta, "chart.colorbar.tick.")
        assert len(ticks) == 3, f"low + neutral + high ticks, got {ticks}"

        # ── Phase 3 — ★ the bar reports the ENCODING, not an even split ─
        stops = strip_stops(ta)
        assert_eq(len(stops), 3, "blue_orange has three stops")
        assert abs(stops[0]["offset"] - 0.0) < 1e-3, "ramp starts at the domain low"
        assert abs(stops[2]["offset"] - 1.0) < 1e-3, "ramp ends at the domain high"
        assert abs(stops[1]["offset"] - 0.25) < 1e-3, (
            "★ the neutral stop sits at the TARGET'S fraction of the domain "
            f"(0.25), not the bar midpoint — got {stops[1]['offset']}"
        )

        # ── Phase 4 — the two maps DISAGREE at the target ─────────────
        # This is the refutation channel. If these matched, nothing above
        # would be evidence of anything.
        encoded_zero = invoke(ta, "color_at", "0")
        linear_zero = invoke(ta, "linear_color_at", "0")
        neutral_hex = query(ta, "neutral_hex")
        assert_eq(
            encoded_zero, neutral_hex, "the diverging map paints the target neutral"
        )
        assert encoded_zero != linear_zero, (
            "★ a linear map does NOT put the target on the neutral "
            f"({encoded_zero} vs {linear_zero}) — the contract's evidence"
        )

        # The domain ENDS are shared by both maps, so they must agree there;
        # otherwise the disagreement above could be a trivial artefact.
        assert_eq(
            invoke(ta, "color_at", "-6"),
            invoke(ta, "linear_color_at", "-6"),
            "both maps agree at the low end",
        )
        assert_eq(
            invoke(ta, "color_at", "18"),
            invoke(ta, "linear_color_at", "18"),
            "both maps agree at the high end",
        )

        # ── Phase 5 — the bar agrees with the MARKS ───────────────────
        # An on-target sample must be painted the exact colour the bar's
        # neutral stop publishes: legend and geometry, one encoding.
        assert_eq(invoke(ta, "value_at", "0,2"), 0.0, "probe-a sample 2 is on target")
        assert_eq(
            invoke(ta, "mark_color_at", "0,2"),
            neutral_hex,
            "★ the on-target mark IS the bar's neutral colour",
        )
        assert_eq(
            stop_hex(stops[1]),
            neutral_hex,
            "the bar's middle stop and the oracle's neutral are one colour",
        )

        # Colour ranks magnitude, not series: equal values across DIFFERENT
        # series get one colour, unequal values in ONE series get two.
        assert_eq(invoke(ta, "value_at", "1,1"), 0.0, "probe-b sample 1 is on target")
        assert_eq(
            invoke(ta, "mark_color_at", "1,1"),
            invoke(ta, "mark_color_at", "0,2"),
            "equal values in different series share a colour",
        )
        assert invoke(ta, "mark_color_at", "0,0") != invoke(ta, "mark_color_at", "0,4"), (
            "different values in the same series do not"
        )

        # ── Phase 6 — the marks are painted, one per sample ───────────
        marks = tags_with_prefix(ta, "chart.point.")
        assert_eq(len(marks), 15, "three probes of five samples each")

        # ── Phase 7 — switch to sequential: the bar re-spaces ─────────
        set_encoding(ta, "sequential")
        wait_until(
            lambda: query(ta, "encoding") == "sequential",
            desc="the encoding switched",
        )
        wait_until(
            lambda: abs(strip_stops(ta)[1]["offset"] - 0.5) < 1e-3,
            desc="★ the sequential ramp spaces its stops EVENLY (0.5)",
        )
        seq_stops = strip_stops(ta)
        assert abs(seq_stops[1]["offset"] - 0.25) > 1e-2, (
            "the neutral stop MOVED — the bar tracks the live encoding"
        )
        assert_eq(
            len(tags_with_prefix(ta, "chart.colorbar.tick.")),
            2,
            "a sequential bar has no neutral tick — nothing anchors a middle",
        )
        # Under a sequential map the target is no longer special: its colour
        # now equals the linear map's, which is exactly what changed.
        assert_eq(
            invoke(ta, "color_at", "0"),
            invoke(ta, "linear_color_at", "0"),
            "sequential IS the linear map",
        )
        assert invoke(ta, "color_at", "0") != neutral_hex, (
            "and it no longer paints the target neutral"
        )
        assert_eq(
            len(tags_with_prefix(ta, "chart.point.")), 15, "same samples, new colours"
        )

        # ── Phase 8 — switch back; the paint follows both ways ────────
        set_encoding(ta, "diverging")
        wait_until(
            lambda: abs(strip_stops(ta)[1]["offset"] - 0.25) < 1e-3,
            desc="the neutral stop returned to the target's fraction",
        )
        assert_eq(
            invoke(ta, "mark_color_at", "0,2"),
            neutral_hex,
            "the on-target mark is neutral again",
        )

        # ── Phase 9 — the wire rejects what it should ─────────────────
        # Typed rejections, not a generic failure: an unknown MODE is a value
        # outside the accepted set, a derived projection is read-only, and a
        # coordinate past the data is rejected rather than clamped.
        assert_out_of_range(
            lambda: ta.intervene(f"{EXT}/encoding", "rainbow"),
            saying='"rainbow" is not an encoding',
        )
        assert_rpc_error(
            lambda: ta.intervene(f"{EXT}/neutral_offset", 0.9), data="ReadOnly"
        )
        # The wire distinguishes a wrong TYPE from a wrong VALUE, and names the
        # channel it happened on (`Intervene…`), so a client knows whether a
        # retry with a different value could ever succeed.
        assert_rpc_error(
            lambda: ta.intervene(f"{EXT}/encoding", 7), data="InterveneTypeMismatch"
        )
        # R1564 — an unaddressable point and an unparseable number were the
        # same frame; a SURFACE refusal now says which, while the READ below
        # stays a framework finding under -32602.
        assert_action_refused(
            lambda: ta.invoke(f"{EXT}/mark_color_at", "9,9"),
            saying='"9,9" does not address a point',
        )
        assert_action_refused(
            lambda: ta.invoke(f"{EXT}/color_at", "not-a-number"),
            saying='"not-a-number" is not a number',
        )
        assert_rpc_error(lambda: ta.query(f"{EXT}/no_such_field"), data="UnknownIntrospectPath")

        # ── Phase 10 — and still works after the rejects ──────────────
        assert_eq(query(ta, "encoding"), "diverging", "the rejects changed nothing")
        assert_eq(
            invoke(ta, "mark_color_at", "0,2"), neutral_hex, "recovered and correct"
        )
        assert abs(strip_stops(ta)[1]["offset"] - 0.25) < 1e-3, "the bar survived too"


if __name__ == "__main__":
    sys.exit(run_demo("R1438 value-encoded scatter colour", body))
