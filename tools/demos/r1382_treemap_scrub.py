#!/usr/bin/env python3
"""R1382 §5.16 §5.7 — the Treemap area-encoded part-of-whole form + its scrub inspector.

`pinion-chart` had a donut (angular part-of-whole) but no AREA-encoded one. A
treemap tiles its whole frame with rectangles sized by value, which stays legible
for many more items than a donut's thin sectors — the asset-browser / disk-usage
shape. R1382 adds `Treemap`: a squarified (Bruls-Huizing-van Wijk) tile layout,
contrast-aware in-tile labels, and the R1375 scrub inspect (now ringing a tile +
showing its percent share). `hello-treemap` is the forcing consumer — a labelled
asset-size breakdown driven through the real windowed shell + RPC.

## The verification idea: geometry as data (§2 #7)

Every element is a tagged node whose shape the AI reads without sampling a pixel:

  (A) one filled box per tile (`chart.tile.0..7`), each with a positive rect; the
      tiles PARTITION the frame (their combined area fills most of it) and their
      area tracks value (Textures dominates, Fonts is a sliver).
  (B) tiles large enough carry a contrast-aware in-tile label naming the tile.
  (C) the boot scrub (0.5) paints the overlay: a stroked ring box (a border, no
      cover) framing exactly the focused tile's rect + a tooltip whose header
      names the tile and whose value row carries its `value (percent%)`.
  (D) driving the scrub over `scene/intervene` re-rings a DIFFERENT tile: a low
      fraction lands the largest tile (Textures), a high fraction the smallest
      (Fonts), so the header + the ring's rect both change.
  (E) the tooltip value carries the tile's percent share (Textures = 240/680 = 35%).

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

VIEWPORT = (620, 460)
# Mirrors the binding's `assets()` (label, value); already descending; total = 680.
TILES = [
    ("Textures", 240),
    ("Meshes", 180),
    ("Audio", 96),
    ("Animations", 62),
    ("Shaders", 44),
    ("Scripts", 30),
    ("Materials", 20),
    ("Fonts", 8),
]
TOTAL = sum(v for _, v in TILES)
# Mirrors the binding's `CHART_RECT` = (10, 40, WIN_W - 20, WIN_H - 74).
CHART = (10, 40, 600, 386)


def _node(snap, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present in the paint snapshot"
    return node


def _rect(node: dict) -> tuple[int, int, int, int]:
    r = node["rect"]
    return (r["x"], r["y"], r["w"], r["h"])


def _ring_rect(snap) -> tuple[int, int, int, int]:
    return _rect(_node(snap, "chart.inspect.highlight"))


def body() -> None:
    with RpcSubprocess("hello-treemap") as d:
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, "chart") is not None, "treemap container present"

        # ── (A) one filled box per tile; the tiles partition the frame ───────
        total_area = 0
        for i, _entry in enumerate(TILES):
            t = _node(snap, f"chart.tile.{i}")
            assert_eq(t["type"], "Box", f"tile {i} is a box")
            assert t["style"]["fill"] is not None, f"tile {i} is filled"
            x, y, w, h = _rect(t)
            assert w > 0 and h > 0, f"tile {i} has a positive rect"
            total_area += w * h
        assert find_by_tag(snap, f"chart.tile.{len(TILES)}") is None, "no phantom tile"

        chart_area = CHART[2] * CHART[3]
        assert total_area >= chart_area * 0.5, (
            f"the tiles fill most of the frame ({total_area} of {chart_area})"
        )
        # Area encodes value: the largest tile dominates the smallest.
        big = _node(snap, "chart.tile.0")
        small = _node(snap, "chart.tile.7")
        big_area = big["rect"]["w"] * big["rect"]["h"]
        small_area = small["rect"]["w"] * small["rect"]["h"]
        assert big_area > small_area, "tile area tracks value (Textures > Fonts)"

        # ── (B) tiles carry contrast-aware in-tile labels ───────────────────
        # Every tile large enough to hold its label gets one drawn in place; at
        # this size the top ranks all clear the gate, and their content names the
        # tile. (The unit test `a_tiny_tile_is_left_unlabelled` proves the
        # too-small case with a controlled sliver, which is size-dependent and so
        # not asserted here.)
        for i, (label, _v) in enumerate(TILES[:3]):
            assert_eq(
                _node(snap, f"chart.tile.{i}.label").get("content"),
                label,
                f"tile {i} is labelled {label!r} in place",
            )

        # ── (C) the boot scrub (0.5) paints the full overlay ─────────────────
        for tag in (
            "chart.inspect.highlight",
            "chart.inspect.tooltip",
            "chart.inspect.header",
            "chart.inspect.value",
        ):
            assert find_by_tag(snap, tag) is not None, f"the scrub paints {tag}"
        ring = _node(snap, "chart.inspect.highlight")
        assert_eq(ring["type"], "Box", "the highlight is a box ring")
        assert ring["style"]["border"] is not None, "the ring is a bordered frame"
        # The ring frames EXACTLY the boot-focused tile (0.5 -> tile 4, Shaders).
        assert_eq(
            _node(snap, "chart.inspect.header").get("content"),
            "Shaders",
            "0.5 focuses the middle-rank tile",
        )
        assert _ring_rect(snap) == _rect(_node(snap, "chart.tile.4")), (
            "the ring's rect equals the focused tile's own rect (geom SSOT)"
        )

        # ── (D) driving the scrub re-rings a DIFFERENT tile ──────────────────
        d.intervene("/external/value", 0.05)
        assert abs(d.query("/external/value") - 0.05) < 0.02, "scrub set to 0.05"
        low = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(
            _node(low, "chart.inspect.header").get("content"),
            "Textures",
            "0.05 -> the largest tile",
        )
        low_ring = _ring_rect(low)

        d.intervene("/external/value", 0.95)
        assert abs(d.query("/external/value") - 0.95) < 0.02, "scrub set to 0.95"
        high = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(
            _node(high, "chart.inspect.header").get("content"),
            "Fonts",
            "0.95 -> the smallest tile",
        )
        high_ring = _ring_rect(high)
        assert low_ring != high_ring, (
            f"the ring frames a different tile as the scrub moves: {low_ring} -> {high_ring}"
        )

        # ── (E) the tooltip value carries the tile's percent share ───────────
        textures_pct = round(240 / TOTAL * 100)  # 35
        fonts_pct = round(8 / TOTAL * 100)  # 1
        textures_val = _node(low, "chart.inspect.value").get("content")
        assert textures_val is not None and f"{textures_pct}%" in textures_val, (
            f"Textures shows its {textures_pct}% share, got {textures_val!r}"
        )
        fonts_val = _node(high, "chart.inspect.value").get("content")
        assert fonts_val is not None and f"{fonts_pct}%" in fonts_val, (
            f"Fonts shows its {fonts_pct}% share, got {fonts_val!r}"
        )
        assert textures_val != fonts_val, "the two tiles report different shares"


if __name__ == "__main__":
    sys.exit(run_demo("R1382 §5.16 — Treemap area-encoded part-of-whole + scrub inspector", body))
