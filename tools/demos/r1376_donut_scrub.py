#!/usr/bin/env python3
"""R1376 §5.16 §5.7 — the DonutChart part-of-whole form + its scrub inspector.

`pinion-chart` had line (time-series), bar / histogram (distribution) but no
PART-OF-WHOLE form. R1376 adds `DonutChart`: filled Bézier-arc sectors, a centre
hole, a legend, and the R1375 scrub inspect (now ringing a slice + showing its
percent share). `hello-donut` is the forcing consumer — a labelled proportion
breakdown driven through the real windowed shell + RPC.

## The verification idea: geometry as data (§2 #7)

Every element is a tagged node whose shape the AI reads without sampling a pixel:

  (A) one filled sector per slice (`chart.slice.0..4`) + a legend per slice.
  (B) each sector is a CLOSED cubic-Bézier path (a real arc, not a polygon).
  (C) the boot scrub (0.5) paints the overlay: a stroked highlight ring + a
      tooltip box + a slice-label header + a `value (percent%)` row.
  (D) driving the scrub over `scene/intervene` re-rings a DIFFERENT slice: a low
      fraction lands the first slice (Media), a high fraction the last (Free),
      so the header + the ring's first point both change.
  (E) the tooltip value carries the slice's percent share (Media = 128/300 = 43%).

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
# Mirrors the binding's `composition()` (label, value); total = 300.
SLICES = [("Media", 128), ("Apps", 46), ("Documents", 22), ("System", 31), ("Free", 73)]
TOTAL = sum(v for _, v in SLICES)


def _node(snap, tag: str) -> dict:
    node = find_by_tag(snap, tag)
    assert node is not None, f"tag {tag!r} present in the paint snapshot"
    return node


def _cmd_types(node: dict) -> list[str]:
    return [c["type"] for c in node["commands"]]


def _ring_start(snap) -> tuple[float, float]:
    ring = _node(snap, "chart.inspect.highlight")
    first = ring["commands"][0]
    assert first["type"] == "MoveTo", "the ring starts with a MoveTo"
    return (first["point"]["x"], first["point"]["y"])


def body() -> None:
    with RpcSubprocess("hello-donut") as d:
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, "chart") is not None, "donut container present"

        # ── (A) one filled sector + a legend per slice ───────────────────────
        for i, (label, _v) in enumerate(SLICES):
            s = _node(snap, f"chart.slice.{i}")
            assert_eq(s["type"], "Path", f"slice {i} is a path")
            assert s["style"]["fill"] is not None, f"slice {i} is filled"
            assert s["style"]["stroke"] is None, f"slice {i} is not stroked"
            swatch = _node(snap, f"chart.legend.{i}.swatch")
            assert_eq(swatch["type"], "Box", f"legend {i} swatch is a box")
            assert_eq(
                _node(snap, f"chart.legend.{i}.label").get("content"),
                label,
                f"legend {i} names the slice",
            )
        assert find_by_tag(snap, f"chart.slice.{len(SLICES)}") is None, "no phantom slice"

        # ── (B) a sector is a CLOSED cubic-Bézier path (a real arc) ──────────
        types = _cmd_types(_node(snap, "chart.slice.0"))
        assert types[0] == "MoveTo", "sector starts with MoveTo"
        assert types[-1] == "Close", "sector is closed"
        assert "CurveTo" in types, "the arc is drawn as cubic Béziers"

        # ── (C) the boot scrub (0.5) paints the full overlay ─────────────────
        for tag in (
            "chart.inspect.highlight",
            "chart.inspect.tooltip",
            "chart.inspect.header",
            "chart.inspect.value",
        ):
            assert find_by_tag(snap, tag) is not None, f"the scrub paints {tag}"
        ring = _node(snap, "chart.inspect.highlight")
        assert ring["style"]["stroke"] is not None, "the highlight is a stroked ring"
        assert ring["style"]["fill"] is None, "the ring frames, it does not cover"
        # The ring shares a sector's rect (all sectors are in the one circle).
        slice_rects = {
            (
                _node(snap, f"chart.slice.{i}")["rect"]["x"],
                _node(snap, f"chart.slice.{i}")["rect"]["y"],
                _node(snap, f"chart.slice.{i}")["rect"]["w"],
                _node(snap, f"chart.slice.{i}")["rect"]["h"],
            )
            for i in range(len(SLICES))
        }
        rr = ring["rect"]
        assert (rr["x"], rr["y"], rr["w"], rr["h"]) in slice_rects, "the ring traces a sector"

        # ── (D) driving the scrub re-rings a DIFFERENT slice ─────────────────
        d.intervene("/external/value", 0.1)
        assert abs(d.query("/external/value") - 0.1) < 0.02, "scrub set to 0.1"
        low = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(_node(low, "chart.inspect.header").get("content"), "Media", "0.1 -> first slice")
        low_start = _ring_start(low)

        d.intervene("/external/value", 0.9)
        assert abs(d.query("/external/value") - 0.9) < 0.02, "scrub set to 0.9"
        high = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(_node(high, "chart.inspect.header").get("content"), "Free", "0.9 -> last slice")
        high_start = _ring_start(high)
        assert low_start != high_start, (
            f"the ring frames a different slice as the scrub moves: {low_start} -> {high_start}"
        )

        # ── (E) the tooltip value carries the slice's percent share ──────────
        media_pct = round(128 / TOTAL * 100)  # 43
        free_pct = round(73 / TOTAL * 100)  # 24
        media_val = _node(low, "chart.inspect.value").get("content")
        assert media_val is not None and f"{media_pct}%" in media_val, (
            f"Media shows its {media_pct}% share, got {media_val!r}"
        )
        free_val = _node(high, "chart.inspect.value").get("content")
        assert free_val is not None and f"{free_pct}%" in free_val, (
            f"Free shows its {free_pct}% share, got {free_val!r}"
        )
        assert media_val != free_val, "the two slices report different shares"


if __name__ == "__main__":
    sys.exit(run_demo("R1376 §5.16 — DonutChart part-of-whole + scrub inspector", body))
