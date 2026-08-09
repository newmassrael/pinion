#!/usr/bin/env python3
"""R1439 §5.35 — the two-variable treemap + a VERTICAL colour bar.

A treemap sizes its rectangles by one measure. R1439 gives a tile a SECOND,
independent measure and encodes it as colour, which is the display the form was
invented as (Wattenberg's Map of the Market, 1998): the thing you are looking
for is the big rectangle that is also the wrong colour. Three claims follow,
and this demo checks each over the wire rather than describing it:

1. **Colour ranks the second measure, not the area.** The data is authored so
   the two rankings DISAGREE — the largest sector has the worst change — and
   two sectors of very different weight share a change, so equal colour cannot
   be coming from equal area.
2. **★ A vertical bar is not a rotated horizontal one.** The value axis runs UP
   (high at the top) while a vertical gradient paints DOWN, so the stops are
   mirrored. Nothing crashes if you skip the mirror; the legend just lies. The
   oracle publishes `strip_offset_at`, the offset a value occupies IN THE
   STRIP, so the mirror is checkable against the scene's own gradient stops.
3. **The bar takes its space from the tiles.** A treemap tiles its whole frame,
   so the legend's gutter is carved out of it — the tiles re-pack when the
   encoding turns on, and reclaim the column when it turns off.

The refutation channel is the point: `color_at` (the live encoding) sits beside
`linear_color_at` (a plain linear map). They disagree at the neutral — which is
the whole reason `map_diverging` exists — and agree at the domain ends, so the
disagreement is not a trivial artefact.

the toolkit reference: the toolkit ships no treemap at all (neither the toolkit Charts nor the toolkit Graphs has
an area-encoded part-of-whole form), and a CP color scale-class legend is a
pixel widget. What is checked below goes past pixels: the ramp rides in the
scene as gradient stops, so the encoding — including the mirror — is verifiable
as data.

Run from the workspace root:
    cargo build -p hello-market-map --release
    python3 tools/demos/r1439_market_map.py
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
VIEWPORT = (640, 470)
STRIP = "chart.colorbar.strip"
TILE_COUNT = 8


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
    wait_until(lambda: query(ta, "encoding") == mode, desc=f"encoding becomes {mode}")


def stop_hex(stop: dict) -> str:
    """A gradient stop's colour as `#rrggbb` — the form the oracle publishes."""
    c = stop["color"]
    return "#{:02x}{:02x}{:02x}".format(c["r"], c["g"], c["b"])


def strip_node(ta: RpcSubprocess):
    return find_by_tag(snap(ta), STRIP)


def strip_stops(ta: RpcSubprocess) -> list[dict]:
    """The colour bar's gradient stops, straight out of the paint scene."""
    node = strip_node(ta)
    assert node is not None, "the colour bar strip is in the paint scene"
    gradient = node.get("style", {}).get("gradient")
    assert gradient is not None, "the strip carries a real gradient, not a flat fill"
    return gradient["stops"]


def tile_rects(ta: RpcSubprocess) -> list[dict]:
    out = []
    for i in range(TILE_COUNT):
        node = find_by_tag(snap(ta), f"chart.tile.{i}")
        if node is not None:
            out.append(node["rect"])
    return out


def tiles_right_edge(ta: RpcSubprocess) -> int:
    rects = tile_rects(ta)
    assert rects, "the treemap draws tiles"
    return max(r["x"] + r["w"] for r in rects)


def tags_with_prefix(ta: RpcSubprocess, prefix: str) -> list[str]:
    return [
        n["tag"]
        for n in _walk(snap(ta), [])
        if isinstance(n.get("tag"), str) and n["tag"].startswith(prefix)
    ]


def body() -> None:
    with RpcSubprocess("hello-market-map", request_timeout=12.0) as ta:
        # ── Phase 1 — two measures, two rankings ─────────────────────
        assert_eq(query(ta, "encoding"), "diverging", "boots on the diverging map")
        by_area = query(ta, "area_order").split(",")
        by_change = query(ta, "change_order").split(",")
        assert_eq(len(by_area), TILE_COUNT, "every sector is ranked by weight")
        assert_eq(sorted(by_area), sorted(by_change), "the same sectors, both ways")
        assert by_area != by_change, (
            "★ the two rankings must DISAGREE — a treemap whose colour tracked "
            f"its area would encode one number twice ({by_area} vs {by_change})"
        )
        assert_eq(by_area[0], "Core Compute", "the heaviest sector")
        assert_eq(by_change[0], "Data Platform", "but not the best-performing one")
        assert_eq(by_change[-1], "Core Compute", "the heaviest is the WORST")

        # ── Phase 2 — the asymmetric domain and its mirror ───────────
        low = query(ta, "domain_low")
        high = query(ta, "domain_high")
        neutral = query(ta, "neutral")
        assert_eq(low, -3.2, "domain low")
        assert_eq(high, 9.6, "domain high")
        assert_eq(neutral, 0.0, "no-change is the anchor")
        below, above = neutral - low, high - neutral
        assert above > below, "the domain is ASYMMETRIC — the case a linear map fails"
        assert abs(above - below * 3.0) < 1e-9, "the up wing is 3x the down wing"
        value_off = query(ta, "neutral_offset")
        strip_off = query(ta, "neutral_strip_offset")
        assert abs(value_off - 0.25) < 1e-9, f"a quarter UP the value axis, got {value_off}"
        assert abs(strip_off - 0.75) < 1e-9, (
            f"★ and three quarters DOWN the strip — the mirror, got {strip_off}"
        )
        assert abs((value_off + strip_off) - 1.0) < 1e-9, "the two are complements"

        # ── Phase 3 — ★ the strip is mirrored, high at the TOP ───────
        node = strip_node(ta)
        assert node is not None, "a colour bar is painted"
        bar = node["rect"]
        assert bar["h"] > bar["w"], f"the bar stands VERTICALLY: {bar}"
        assert bar["w"] > 0 and bar["h"] > 0, f"and is visible: {bar}"
        stops = strip_stops(ta)
        assert_eq(len(stops), 3, "blue_orange has three stops")
        offsets = [s["offset"] for s in stops]
        assert all(a <= b for a, b in zip(offsets, offsets[1:])), (
            f"gradient stops ascend, the form a gradient is defined on: {offsets}"
        )
        assert abs(offsets[1] - strip_off) < 1e-3, (
            "★ the neutral stop sits at the MIRRORED offset the oracle publishes "
            f"({strip_off}), not the value-space one ({value_off}) — got {offsets[1]}"
        )
        # The colour at gradient offset 0 (the strip's TOP) must be the ramp's
        # HIGH end. This is the assertion that fails if the mirror is dropped.
        high_hex = invoke(ta, "color_at", str(high))
        low_hex = invoke(ta, "color_at", str(low))
        assert_eq(stop_hex(stops[0]), high_hex, "★ the ramp's HIGH colour paints at the top")
        assert_eq(stop_hex(stops[-1]), low_hex, "and its LOW colour at the bottom")
        assert high_hex != low_hex, "the two ends are different colours"

        # The tick labels descend in value as they descend the strip, and each
        # prints its domain end FAITHFULLY — an end of -3.2 that rendered as
        # "-3" would mislabel the ramp's own extent by 0.2 (R1439 fix).
        ticks = tags_with_prefix(ta, "chart.colorbar.tick.")
        assert_eq(len(ticks), 3, f"low + neutral + high ticks, got {ticks}")
        tick_nodes = [find_by_tag(snap(ta), f"chart.colorbar.tick.{k}") for k in range(3)]
        for k, t in enumerate(tick_nodes):
            assert t is not None, f"tick {k} is in the scene"
        tick_y = [t["rect"]["y"] for t in tick_nodes]
        assert tick_y[2] < tick_y[1] < tick_y[0], (
            f"★ high above neutral above low, going DOWN the strip: {tick_y}"
        )
        assert_eq(tick_nodes[0]["content"], "-3.2", "the low end prints in full")
        assert_eq(tick_nodes[1]["content"], "0.0", "the neutral")
        assert_eq(tick_nodes[2]["content"], "9.6", "and the high end")
        # Every tick sits beside the strip, not on it.
        for k, t in enumerate(tick_nodes):
            assert t["rect"]["x"] >= bar["x"] + bar["w"], f"tick {k} clears the strip"

        # ── Phase 4 — the two maps DISAGREE at the neutral ───────────
        # The refutation channel. If these matched, nothing above would be
        # evidence of anything.
        encoded_zero = invoke(ta, "color_at", "0")
        linear_zero = invoke(ta, "linear_color_at", "0")
        neutral_hex = query(ta, "neutral_hex")
        assert_eq(encoded_zero, neutral_hex, "the diverging map paints no-change neutral")
        assert encoded_zero != linear_zero, (
            "★ a linear map does NOT put the neutral on the ramp's centre "
            f"({encoded_zero} vs {linear_zero}) — the contract's evidence"
        )
        for end in (low, high):
            assert_eq(
                invoke(ta, "color_at", str(end)),
                invoke(ta, "linear_color_at", str(end)),
                f"both maps agree at {end}, so the disagreement is not an artefact",
            )

        # ── Phase 5 — colour follows the MEASURE, not the area ───────
        heavy_w = invoke(ta, "tile_weight", "Networking")
        light_w = invoke(ta, "tile_weight", "Edge")
        assert heavy_w > light_w * 2.0, f"very different weights: {heavy_w} vs {light_w}"
        assert_eq(invoke(ta, "tile_change", "Networking"), 0.0, "both on target")
        assert_eq(invoke(ta, "tile_change", "Edge"), 0.0, "both on target")
        assert_eq(
            invoke(ta, "tile_color", "Networking"),
            invoke(ta, "tile_color", "Edge"),
            "★ equal measure -> equal colour, at 3.75x the area",
        )
        assert_eq(
            invoke(ta, "tile_color", "Networking"),
            neutral_hex,
            "and it is the bar's neutral colour — legend and tiles, one encoding",
        )
        # The heaviest sector is NOT the ramp's high end; the second heaviest is.
        assert_eq(
            invoke(ta, "tile_color", "Core Compute"),
            low_hex,
            "the largest tile takes the LOW colour (its change is the worst)",
        )
        assert_eq(
            invoke(ta, "tile_color", "Data Platform"),
            high_hex,
            "and the high colour belongs to a smaller one",
        )

        # ── Phase 6 — the bar's gutter comes out of the tiles ────────
        encoded_edge = tiles_right_edge(ta)
        assert encoded_edge <= bar["x"], (
            f"no tile reaches into the bar's column ({encoded_edge} vs {bar['x']})"
        )

        set_encoding(ta, "off")
        assert strip_node(ta) is None, "no bar once the encoding is off"
        off_edge = tiles_right_edge(ta)
        assert off_edge > encoded_edge, (
            f"★ the tiles reclaim the gutter ({off_edge} > {encoded_edge})"
        )
        assert_eq(len(tile_rects(ta)), TILE_COUNT, "and every sector is still drawn")
        # Colour changes KIND: the two on-target sectors are one colour while
        # encoded, and distinct categories once it is off.
        assert invoke(ta, "tile_color", "Networking") != invoke(ta, "tile_color", "Edge"), (
            "with the encoding off the colours name categories again"
        )
        assert_action_refused(
            lambda: invoke(ta, "color_at", "0"),
            saying="assigns no colour to that value",
        )

        # ── Phase 7 — sequential re-seats the ramp, live ─────────────
        set_encoding(ta, "sequential")
        seq_stops = strip_stops(ta)
        assert_eq(len(seq_stops), 3, "the bar is back")
        assert abs(seq_stops[1]["offset"] - 0.5) < 1e-3, (
            "★ a sequential ramp is evenly spaced (0.50) where the diverging one "
            f"was mirrored to 0.75 — got {seq_stops[1]['offset']}"
        )
        assert invoke(ta, "tile_color", "Networking") != neutral_hex, (
            "and no-change is no longer the ramp's centre colour"
        )
        assert_eq(
            invoke(ta, "tile_color", "Networking"),
            invoke(ta, "tile_color", "Edge"),
            "equal measure still means equal colour under either map",
        )

        set_encoding(ta, "diverging")
        assert abs(strip_stops(ta)[1]["offset"] - 0.75) < 1e-3, (
            "★ and back: the bar re-seats to the mirrored quarter"
        )
        assert_eq(
            invoke(ta, "tile_color", "Networking"), neutral_hex, "neutral again"
        )

        # ── Phase 8 — the write surface is honest ────────────────────
        assert_out_of_range(
            lambda: ta.intervene(f"{EXT}/encoding", "sideways"),
            saying='"sideways" is not an encoding',
        )
        assert_rpc_error(lambda: ta.intervene(f"{EXT}/domain_low", 1.0), data="ReadOnly")
        assert_action_refused(
            lambda: invoke(ta, "tile_color", "Nonexistent"),
            saying='no sector named "Nonexistent" on this map',
        )
        assert_eq(query(ta, "encoding"), "diverging", "a rejected write changed nothing")


if __name__ == "__main__":
    sys.exit(run_demo("r1439_market_map", body))
