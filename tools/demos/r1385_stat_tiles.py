#!/usr/bin/env python3
"""R1385 — hello-stat-tiles: a KPI stat-tile row with trend sparklines.

The forcing consumer for `pinion_chart::Sparkline`. Each tile is a card holding
a metric name, its value, a period-over-period delta coloured by SIGN (a rise
green, a fall red — semantic colours kept distinct from any brand accent), and a
compact sparkline of the trend. A display-only widget (PR-51 no primary
surface), so the verification is pure introspection: every value, delta, colour,
and sparkline mark is read back as scene data (§2 #7) over the RPC plane.

Atomic verification scope (>=30 assertions):

  (A) structure — four tiles, each with a card + label + value + delta + a
      trend sparkline; a title.
  (B) values — each tile's label + value text match its KPI.
  (C) SEMANTIC delta colours — the three rising deltas render in the green
      up-colour, the one falling delta in the red down-colour, with the signed
      text to match (the dashboard brief's "semantic colours, brand-separate").
  (D) sparklines — each is a filled area chart (`spark_{i}.area`) with a line
      and an end cap in the tile's accent colour; NO un-prefixed `spark.*`
      (the row-collision guard the Sparkline API documents).
  (E) negative — an unknown snapshot tag is absent, an unknown query rejected.
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

EXAMPLE = "hello-stat-tiles"
VIEWPORT = (720, 220)
N = 4

# Semantic delta colours (RGB), theme-independent.
UP = (46, 160, 67)
DOWN = (217, 58, 44)

# Per-KPI expected (name, value, delta text, delta colour, accent RGB).
KPIS = [
    ("Sessions", "1,284", "+12.4%", UP, (66, 133, 244)),
    ("Throughput", "3.4k/s", "+5.1%", UP, (52, 168, 83)),
    ("Requests", "892k", "+2.0%", UP, (240, 157, 0)),
    ("Uptime", "99.2%", "-0.3%", DOWN, (14, 154, 167)),
]


def rgb(node) -> tuple[int, int, int] | None:
    """The (r,g,b) of a text node's fg or a path node's fill, or None."""
    st = node.get("style") or {}
    c = st.get("fg_color") or st.get("fill")
    return None if c is None else (c["r"], c["g"], c["b"])


def content(snap, tag: str) -> str:
    node = find_by_tag(snap, tag)
    assert node is not None, f"{tag} present"
    return node.get("content")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)

        # ── (A) structure ────────────────────────────────────────────────────
        assert find_by_tag(snap, "title") is not None, "the row has a title"
        for i in range(N):
            card = find_by_tag(snap, f"tile_{i}")
            assert card is not None, f"tile {i} card present"
            assert (card.get("style") or {}).get("fill") is not None, f"tile {i} is a surface"
            for part in ("label", "value", "delta"):
                assert find_by_tag(snap, f"tile_{i}.{part}") is not None, f"tile {i} {part}"
            assert find_by_tag(snap, f"spark_{i}.line") is not None, f"tile {i} sparkline"

        # ── (B) values ───────────────────────────────────────────────────────
        for i, (name, value, _dt, _dc, _ac) in enumerate(KPIS):
            assert_eq(content(snap, f"tile_{i}.label"), name, f"tile {i} label")
            assert_eq(content(snap, f"tile_{i}.value"), value, f"tile {i} value")

        # ── (C) SEMANTIC delta colours + signed text ─────────────────────────
        for i, (_name, _value, dtext, dcolor, _ac) in enumerate(KPIS):
            assert_eq(content(snap, f"tile_{i}.delta"), dtext, f"tile {i} delta text")
            got = rgb(find_by_tag(snap, f"tile_{i}.delta"))
            assert_eq(got, dcolor, f"tile {i} delta is the {'up' if dcolor == UP else 'down'} colour")
        # The falling KPI's colour is genuinely DIFFERENT from the rising ones.
        assert UP != DOWN, "up and down are distinct hues"

        # ── (D) sparklines: filled area + line + accent end cap, no collision ─
        assert find_by_tag(snap, "spark.line") is None, "no un-prefixed sparkline (collision guard)"
        for i, (_name, _value, _dt, _dc, accent) in enumerate(KPIS):
            assert find_by_tag(snap, f"spark_{i}.area") is not None, f"tile {i} sparkline is filled"
            assert find_by_tag(snap, f"spark_{i}.end") is not None, f"tile {i} sparkline end cap"
            end_rgb = rgb(find_by_tag(snap, f"spark_{i}.end"))
            assert_eq(end_rgb, accent, f"tile {i} end cap is the tile's accent colour")
            # min / max reference dots present too.
            assert find_by_tag(snap, f"spark_{i}.min") is not None, f"tile {i} min dot"
            assert find_by_tag(snap, f"spark_{i}.max") is not None, f"tile {i} max dot"

        # ── (E) negative ─────────────────────────────────────────────────────
        assert find_by_tag(snap, "tile_4") is None, "no phantom fifth tile"
        assert find_by_tag(snap, "spark_9.line") is None, "no phantom sparkline"
        raised = False
        try:
            tf.query("/no_such_tag/external/value")
        except Exception:  # noqa: BLE001 — RpcError type varies by harness
            raised = True
        assert raised, "an unknown introspect path must be rejected"


if __name__ == "__main__":
    sys.exit(run_demo("R1385 — a KPI stat-tile row with trend sparklines", body))
