#!/usr/bin/env python3
"""R975 §5.41 — cell-native `TextGrid` cursor (S5 slice 3).

The third S5 data-model slice: the grid **cursor** (`GridCursor` =
position / shape / visibility). The `hello-textgrid` window grows a fifth
grid, `htg_cursor` (8x2 cells), isolated from R973/R974 so their demos
stay regression-free.

The proof is pure DATA over RPC (glyph paint is still a follow-up round).
Every `scene/snapshot` of a `TextGrid` now carries a `cursor` object:
`{col, row, shape, visible}`. The cursor is the producer's *effective*
cursor — the one it wants shown — carried on the projection alongside the
cells (R969 "producer owns authoritative state"):

  * `htg_cursor` reports a VISIBLE bar cursor at the input column (col 2,
    row 1) of a prompt line — non-zero on both axes, so the snapshot
    witnesses real cell addressing, not a trivial home position;
  * a client tells the cursor is in bounds by comparing `(col, row)`
    against the grid's `(cols, rows)` — both are first-class facts;
  * the other four grids never set a cursor, so each reports the DEFAULT:
    hidden, home `(0, 0)`, block (the cursor is opt-in per projection);
  * the `cursor` field is additive — R973's colours and R974's attrs are
    unchanged.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r975_textgrid_cursor.py
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

WIN = (680, 840)
CURSOR_TAG = "htg_cursor"

# The default a grid carries when its producer has not set a cursor:
# hidden, home, block, no explicit OSC-12 colour (R1424 added `cursor_color`).
DEFAULT_CURSOR = {
    "col": 0,
    "row": 0,
    "shape": "block",
    "visible": False,
    "cursor_color": None,
}


def assert_default_cursor(grid: dict, tag: str) -> None:
    """Assert `grid` reports the default (hidden / home / block) cursor."""
    assert_eq(grid["cursor"], DEFAULT_CURSOR, f"{tag} carries the default hidden cursor")


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll the paint snapshot until the cursor grid's
        # layout has resolved (cols == 8) AND its 2-row projection is
        # present. No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, CURSOR_TAG) or {}).get("cols") == 8
            and len((find_by_tag(s, CURSOR_TAG) or {}).get("grid_rows", [])) == 2,
            source="paint",
            viewport=WIN,
            desc=f"{CURSOR_TAG} projection resolved",
        )

        # --- htg_cursor: an explicit visible bar cursor ---
        grid = find_by_tag(snap, CURSOR_TAG)
        assert grid is not None, "cursor grid present in paint scene"
        assert_eq((grid["cols"], grid["rows"]), (8, 2), "cursor dims 8x2")
        assert_eq((grid["buffer_cols"], grid["buffer_rows"]), (8, 2), "cursor buffer 8x2")

        # The node-local 8x16 baseline metric round-trips the geometry.
        assert_eq((grid["cell_w"], grid["cell_h"]), (8, 16), "cursor grid 8x16 metric")
        assert_eq((grid["rect"]["w"], grid["rect"]["h"]), (64, 32), "cursor grid 64x32 px")

        cur = grid["cursor"]
        assert_eq(cur["col"], 2, "cursor col = input column")
        assert_eq(cur["row"], 1, "cursor row = prompt line")
        assert_eq(cur["shape"], "bar", "cursor shape = bar (insertion beam)")
        assert_eq(cur["visible"], True, "cursor is visible")
        # R1424 — htg_cursor sets no OSC-12 colour, so the cursor reports the
        # None default (the backward-compatible cell-foreground render).
        assert_eq(cur["cursor_color"], None, "bar cursor carries no explicit colour")
        # In bounds: derived from the two first-class facts, not a flag.
        assert cur["col"] < grid["cols"] and cur["row"] < grid["rows"], "cursor in bounds"
        # Genuinely non-default on every axis (shape and visibility differ
        # from the hidden block default the other grids carry).
        assert cur != DEFAULT_CURSOR, "cursor is the explicit, not the default, one"
        assert_eq(cur["shape"] != "block", True, "cursor shape is not the default block")

        rows = grid["grid_rows"]
        assert_eq(len(rows), 2, "cursor grid has one entry per row")
        # Row 0 is the label; row 1 is the prompt. The cursor is on row 1,
        # provably not the home row.
        assert_eq(rows[0]["text"], "line one", "row0 = 8-glyph label")
        assert rows[1]["text"].startswith("$ "), "row1 = prompt"
        assert_eq(len(rows[1]["text"]), 8, "row1 padded to 8 cells")
        # The cell under the cursor (row 1, col 2) is blank — the cursor is
        # grid state, not a cell mutation.
        assert_eq(rows[1]["text"][2], " ", "input cell under cursor is blank")
        # Default colours, so each row collapses to a single style run.
        assert_eq(len(rows[0]["runs"]), 1, "row0 single default run")
        assert_eq(rows[0]["runs"][0]["fg"]["kind"], "default", "row0 fg default")
        assert_eq(len(rows[1]["runs"]), 1, "row1 single default run")
        assert_eq(rows[1]["runs"][0]["bg"]["kind"], "default", "row1 bg default")

        # --- Regression: the other four grids carry the default cursor ---
        for tag in ("htg_default", "htg_measured", "htg_content", "htg_attrs"):
            other = find_by_tag(snap, tag)
            assert other is not None, f"{tag} still present"
            assert_default_cursor(other, tag)

        # --- Regression: R973 colours / R974 attrs are untouched (the
        #     cursor field is additive) ---
        content = find_by_tag(snap, "htg_content")
        assert_eq(len(content["grid_rows"]), 4, "htg_content still 4 rows")
        bar0 = content["grid_rows"][0]["runs"][0]
        assert_eq(bar0["bg"]["index"], 0, "htg_content bar[0] bg still index 0")
        assert_eq(bar0["attrs"]["bold"], False, "htg_content attrs still off")
        attrs = find_by_tag(snap, "htg_attrs")
        assert_eq(len(attrs["grid_rows"]), 2, "htg_attrs still 2 rows")
        assert_eq(attrs["grid_rows"][0]["text"], "BDIUKRHS", "htg_attrs row0 unchanged")


def main() -> int:
    return run_demo("r975_textgrid_cursor", body)


if __name__ == "__main__":
    sys.exit(main())
