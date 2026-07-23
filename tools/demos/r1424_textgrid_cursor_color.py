#!/usr/bin/env python3
"""R1424 §5.41 — explicit OSC-12 cursor colour on a cell-native `TextGrid`.

sprag (and any terminal producer) tracks the OSC-12 cursor colour a modal
editor (`vim` / `kakoune`) sets per mode, but until R1424 pinion's
`GridCursor` — the only type the projection describes the cursor with —
carried no colour field, so the cursor could not be RENDERED in it
(PINION-PR74). R1424 adds `GridCursor::cursor_color: Option<Color>` (and
`with_cursor_color`): the painter fills the cursor in that absolute colour
and `scene/snapshot` reports it as data.

The `hello-textgrid` window grows an eleventh grid, `htg_cursor_color`
(8x2 cells), carrying a VISIBLE BLOCK cursor painted an insert-mode green
(`#2ecc71`). The proof is pure DATA over RPC: every `scene/snapshot` of a
`TextGrid` `cursor` object now carries a `cursor_color` — the resolved hex
literal, or `null` when the producer set none:

  * `htg_cursor_color` reports `cursor_color == "#2ecc71"` on its visible
    block cursor at the input glyph (col 2, row 1) — an absolute colour,
    no palette semantics;
  * its sibling `htg_cursor` (a BAR, no OSC-12 colour) reports
    `cursor_color == null` — the `None` default the backward-compatible
    cell-foreground render still covers, so both arms are witnessed;
  * every scaffold grid that never set a cursor reports the DEFAULT cursor
    with `cursor_color == null` — the colour is opt-in per projection;
  * the `cursor_color` field is additive — R973's colours, R974's attrs,
    R975's position/shape/visibility are unchanged.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r1424_textgrid_cursor_color.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

WIN = (680, 900)
COLOR_TAG = "htg_cursor_color"
BAR_TAG = "htg_cursor"

# The OSC-12 insert-mode green the grid sets (resolved hex, opaque).
GREEN_HEX = "#2ecc71"

# The default cursor a grid carries when its producer set none: hidden, home,
# block, no explicit OSC-12 colour.
DEFAULT_CURSOR = {
    "col": 0,
    "row": 0,
    "shape": "block",
    "visible": False,
    "cursor_color": None,
}


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll the paint snapshot until the cursor-colour
        # grid's layout has resolved (cols == 8) AND its 2-row projection is
        # present. No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, COLOR_TAG) or {}).get("cols") == 8
            and len((find_by_tag(s, COLOR_TAG) or {}).get("grid_rows", [])) == 2,
            source="paint",
            viewport=WIN,
            desc=f"{COLOR_TAG} projection resolved",
        )

        # --- htg_cursor_color: a visible BLOCK cursor with an explicit colour ---
        grid = find_by_tag(snap, COLOR_TAG)
        assert grid is not None, "cursor-colour grid present in paint scene"
        assert_eq((grid["cols"], grid["rows"]), (8, 2), "colour grid dims 8x2")
        assert_eq(
            (grid["buffer_cols"], grid["buffer_rows"]),
            (8, 2),
            "colour grid buffer 8x2",
        )
        # The node-local 8x16 baseline metric round-trips the geometry.
        assert_eq((grid["cell_w"], grid["cell_h"]), (8, 16), "colour grid 8x16 metric")
        assert_eq(
            (grid["rect"]["w"], grid["rect"]["h"]),
            (64, 32),
            "colour grid 64x32 px",
        )

        cur = grid["cursor"]
        assert_eq(cur["col"], 2, "cursor col = input glyph column")
        assert_eq(cur["row"], 1, "cursor row = prompt line")
        assert_eq(cur["shape"], "block", "cursor shape = block")
        assert_eq(cur["visible"], True, "cursor is visible")
        # The headline: the explicit OSC-12 colour is reported as its hex.
        assert_eq(cur["cursor_color"], GREEN_HEX, "cursor carries the OSC-12 green")
        # In bounds: derived from the two first-class facts, not a flag.
        assert cur["col"] < grid["cols"] and cur["row"] < grid["rows"], "cursor in bounds"
        # Genuinely non-default (colour + shape + visibility all differ).
        assert cur != DEFAULT_CURSOR, "cursor is the explicit, not the default, one"
        # An absolute colour: opaque 6-digit hex, not a palette kind/index shape.
        assert isinstance(cur["cursor_color"], str), "cursor_color is a hex string"
        assert cur["cursor_color"].startswith("#") and len(cur["cursor_color"]) == 7, (
            "cursor_color is a 6-digit opaque hex literal"
        )

        rows = grid["grid_rows"]
        assert_eq(len(rows), 2, "colour grid has one entry per row")
        assert_eq(rows[0]["text"], "[insert]", "row0 = 8-glyph mode label")
        assert rows[1]["text"].startswith("$ x"), "row1 = prompt with input glyph"
        assert_eq(len(rows[1]["text"]), 8, "row1 padded to 8 cells")
        # The cell under the cursor (row 1, col 2) is the input glyph 'x'; the
        # block redraws it inverse under the cursor colour.
        assert_eq(rows[1]["text"][2], "x", "input cell under cursor is 'x'")
        # Default colours, so each row collapses to a single style run.
        assert_eq(len(rows[0]["runs"]), 1, "row0 single default run")
        assert_eq(rows[0]["runs"][0]["fg"]["kind"], "default", "row0 fg default")
        assert_eq(len(rows[1]["runs"]), 1, "row1 single default run")

        # --- Sibling htg_cursor: the None default arm (a bar, no colour) ---
        bar = find_by_tag(snap, BAR_TAG)
        assert bar is not None, "sibling bar-cursor grid present"
        bar_cur = bar["cursor"]
        assert_eq(bar_cur["shape"], "bar", "sibling cursor shape = bar")
        assert_eq(bar_cur["visible"], True, "sibling cursor visible")
        assert_eq(
            bar_cur["cursor_color"],
            None,
            "sibling bar cursor carries no explicit colour (the None arm)",
        )
        # The two grids witness both arms: one Some(colour), one None.
        assert cur["cursor_color"] != bar_cur["cursor_color"], (
            "the two cursor grids report distinct colour arms"
        )

        # --- Regression: scaffold grids report the default cursor (colour None) ---
        for tag in ("htg_default", "htg_measured", "htg_content", "htg_attrs"):
            other = find_by_tag(snap, tag)
            assert other is not None, f"{tag} still present"
            assert_eq(
                other["cursor"],
                DEFAULT_CURSOR,
                f"{tag} carries the default cursor with cursor_color None",
            )

        # --- Regression: R973 colours are untouched (cursor_color is additive) ---
        content = find_by_tag(snap, "htg_content")
        assert_eq(len(content["grid_rows"]), 4, "htg_content still 4 rows")
        bar0 = content["grid_rows"][0]["runs"][0]
        assert_eq(bar0["bg"]["index"], 0, "htg_content bar[0] bg still index 0")
        # --- Regression: the R1403 hyperlink grid is still present ---
        assert find_by_tag(snap, "htg_hyperlink") is not None, "htg_hyperlink still present"


def main() -> int:
    return run_demo("r1424_textgrid_cursor_color", body)


if __name__ == "__main__":
    sys.exit(main())
