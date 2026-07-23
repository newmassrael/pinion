#!/usr/bin/env python3
"""R1425 §5.41 — DECSCUSR blink-vs-steady mode on a cell-native `TextGrid`.

A terminal producer's cursor is either a *blinking* DECSCUSR variant
(`CSI 0/1/3/5 SP q`) or a *steady* one (`2/4/6`), an axis orthogonal to the
cursor SHAPE and to DECTCEM show/hide. Until R1425 pinion's `GridCursor` —
the only type the projection describes the cursor with — carried no blink
field, so a client could not read whether the cursor was a blinking-type
cursor. R1425 adds `GridCursor::blink: bool` (and `with_blink`): the MODE is
reported as data. It is a first-class fact DISTINCT from `visible` (DECTCEM):
a producer that folded the render-time blink phase into `visible` would make
"app hid the cursor" and "blink off-phase" indistinguishable — pinion keeps
the mode pure, so `scene/snapshot` reports `blink` apart from show/hide
(§2 #7). The render-time phase animation is the deferred renderer-owned slice
(as the R993 Vello paint followed the R975 cursor data model).

The `hello-textgrid` window grows a twelfth grid, `htg_cursor_blink`
(8x2 cells), carrying a VISIBLE BLOCK cursor in the blinking DECSCUSR mode.
The proof is pure DATA over RPC: every `scene/snapshot` of a `TextGrid`
`cursor` object now carries a `blink` boolean:

  * `htg_cursor_blink` reports `blink == True` on its visible block cursor at
    the input glyph (col 2, row 1) — and `visible == True`, so the mode and
    the DECTCEM state are read apart;
  * its sibling `htg_cursor` (a BAR, steady) reports `blink == False` — the
    default arm, so both DECSCUSR blink states are witnessed;
  * every scaffold grid that never set a cursor reports the DEFAULT cursor
    with `blink == False` — the mode is opt-in per projection;
  * `blink` is additive — R973's colours, R974's attrs, R975's
    position/shape/visibility, and R1424's `cursor_color` are all unchanged.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r1425_textgrid_cursor_blink.py
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
BLINK_TAG = "htg_cursor_blink"
BAR_TAG = "htg_cursor"
COLOR_TAG = "htg_cursor_color"

# The default cursor a grid carries when its producer set none: hidden, home,
# block, no explicit OSC-12 colour, and — R1425 — steady (blink False).
DEFAULT_CURSOR = {
    "col": 0,
    "row": 0,
    "shape": "block",
    "visible": False,
    "cursor_color": None,
    "blink": False,
}


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll the paint snapshot until the blink grid's
        # layout has resolved (cols == 8) AND its 2-row projection is present.
        # No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, BLINK_TAG) or {}).get("cols") == 8
            and len((find_by_tag(s, BLINK_TAG) or {}).get("grid_rows", [])) == 2,
            source="paint",
            viewport=WIN,
            desc=f"{BLINK_TAG} projection resolved",
        )

        # --- htg_cursor_blink: a visible BLOCK cursor in the blinking mode ---
        grid = find_by_tag(snap, BLINK_TAG)
        assert grid is not None, "blink grid present in paint scene"
        assert_eq((grid["cols"], grid["rows"]), (8, 2), "blink grid dims 8x2")
        assert_eq(
            (grid["buffer_cols"], grid["buffer_rows"]),
            (8, 2),
            "blink grid buffer 8x2",
        )
        # The node-local 8x16 baseline metric round-trips the geometry.
        assert_eq((grid["cell_w"], grid["cell_h"]), (8, 16), "blink grid 8x16 metric")
        assert_eq(
            (grid["rect"]["w"], grid["rect"]["h"]),
            (64, 32),
            "blink grid 64x32 px",
        )

        cur = grid["cursor"]
        assert_eq(cur["col"], 2, "cursor col = input glyph column")
        assert_eq(cur["row"], 1, "cursor row = prompt line")
        assert_eq(cur["shape"], "block", "cursor shape = block")
        assert_eq(cur["visible"], True, "cursor is visible (DECTCEM shown)")
        # The headline: the DECSCUSR blinking mode is reported as data.
        assert_eq(cur["blink"], True, "cursor is a blinking DECSCUSR variant")
        # The MODE is read APART from show/hide: both are True, independently.
        assert cur["blink"] is True and cur["visible"] is True, (
            "blink mode and DECTCEM visibility are distinct first-class facts"
        )
        assert isinstance(cur["blink"], bool), "blink is a boolean mode"
        # No OSC-12 colour on this grid, so blink is orthogonal to colour too.
        assert_eq(cur["cursor_color"], None, "blink grid sets no OSC-12 colour")
        # In bounds: derived from the two first-class facts, not a flag.
        assert cur["col"] < grid["cols"] and cur["row"] < grid["rows"], "cursor in bounds"
        # Genuinely non-default (blink + shape + visibility all differ).
        assert cur != DEFAULT_CURSOR, "cursor is the explicit, not the default, one"

        rows = grid["grid_rows"]
        assert_eq(len(rows), 2, "blink grid has one entry per row")
        assert_eq(rows[0]["text"], "[blinks]", "row0 = 8-glyph mode label")
        assert rows[1]["text"].startswith("$ x"), "row1 = prompt with input glyph"
        assert_eq(len(rows[1]["text"]), 8, "row1 padded to 8 cells")
        # The cell under the cursor (row 1, col 2) is the input glyph 'x'.
        assert_eq(rows[1]["text"][2], "x", "input cell under cursor is 'x'")
        # Default colours, so each row collapses to a single style run.
        assert_eq(len(rows[0]["runs"]), 1, "row0 single default run")
        assert_eq(rows[0]["runs"][0]["fg"]["kind"], "default", "row0 fg default")
        assert_eq(len(rows[1]["runs"]), 1, "row1 single default run")

        # --- Sibling htg_cursor: the steady default arm (a bar, no blink) ---
        bar = find_by_tag(snap, BAR_TAG)
        assert bar is not None, "sibling bar-cursor grid present"
        bar_cur = bar["cursor"]
        assert_eq(bar_cur["shape"], "bar", "sibling cursor shape = bar")
        assert_eq(bar_cur["visible"], True, "sibling cursor visible")
        assert_eq(bar_cur["blink"], False, "sibling bar cursor is steady (the False arm)")
        # The two grids witness both DECSCUSR blink states: one blinking, one
        # steady — while BOTH are visible (blink is not folded into visible).
        assert cur["blink"] != bar_cur["blink"], (
            "the two cursor grids report distinct blink modes"
        )
        assert cur["visible"] == bar_cur["visible"] == True, (
            "yet both cursors are shown — blink is orthogonal to DECTCEM"
        )

        # --- Regression: the R1424 OSC-12 colour grid is untouched ---
        color = find_by_tag(snap, COLOR_TAG)
        assert color is not None, "R1424 cursor-colour grid still present"
        color_cur = color["cursor"]
        assert_eq(color_cur["cursor_color"], "#2ecc71", "R1424 OSC-12 green intact")
        # blink is additive: the coloured cursor is steady (never set blinking).
        assert_eq(color_cur["blink"], False, "coloured cursor is steady (blink additive)")
        assert_eq(color_cur["visible"], True, "coloured cursor still visible")

        # --- Regression: scaffold grids report the default cursor (steady) ---
        for tag in ("htg_default", "htg_measured", "htg_content", "htg_attrs"):
            other = find_by_tag(snap, tag)
            assert other is not None, f"{tag} still present"
            assert_eq(
                other["cursor"],
                DEFAULT_CURSOR,
                f"{tag} carries the default cursor with blink False",
            )
            # The default cursor is hidden AND steady — two independent Falses.
            assert other["cursor"]["visible"] is False and other["cursor"]["blink"] is False, (
                f"{tag} default cursor is both hidden and steady"
            )

        # --- Regression: R973 colours are untouched (blink is additive) ---
        content = find_by_tag(snap, "htg_content")
        assert_eq(len(content["grid_rows"]), 4, "htg_content still 4 rows")
        bar0 = content["grid_rows"][0]["runs"][0]
        assert_eq(bar0["bg"]["index"], 0, "htg_content bar[0] bg still index 0")
        # --- Regression: the R1403 hyperlink grid is still present ---
        assert find_by_tag(snap, "htg_hyperlink") is not None, "htg_hyperlink still present"


def main() -> int:
    return run_demo("r1425_textgrid_cursor_blink", body)


if __name__ == "__main__":
    sys.exit(main())
