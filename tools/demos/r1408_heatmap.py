#!/usr/bin/env python3
"""R1408 §5.41 — the heatmap's bare-hover cell inspector.

A `12 x 8` intensity matrix renders over `Scene::TextGrid` as squarish colour
cells (a sequential surface->accent ramp + a per-cell value label). Moving the
pointer over the grid — WITH NO BUTTON HELD — reveals the cell under it: the
hovered data cell reverse-videos (R974) and the `HeatmapOracle` reports its
`(row, col, value)`. This is the third consumer of `External::wants_hover_move`
(after `hello-hyperlink` and `hello-crosshair`) and the third consumer of the
rect-fraction -> cell hit-test, the lift of `CellMetric::frac_to_cell`.

The proof is pure DATA over RPC: a REAL `scene/hover` drives `wants_hover_move`
end-to-end through the router; the hovered cell's char cells carry `reverse` in
`grid_rows`, cross-checked against the `hovered_row/col/value` readout and the
`value_at` matrix oracle (the painted highlight and the readout can never
diverge). An AI client drives it with no pixel via `intervene hovered_cell`.

Run from the workspace root:
    cargo build -p hello-heatmap --release
    python3 tools/demos/r1408_heatmap.py
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

GRID = "heatmap"
EXT = "/external"
# Layout mirrors main.rs: GRID_POS=(24,44), CELL_CW=6, CELL_CH=3 at the 8x16 cell.
GRID_POS = (24, 44)
CELL_CW, CELL_CH = 6, 3
CHAR_W, CHAR_H = 8, 16
GCOLS, GROWS = 12 * CELL_CW, 8 * CELL_CH
WIN = (12 * CELL_CW * CHAR_W + 48, 8 * CELL_CH * CHAR_H + 108)


def cell_center_px(row: int, col: int) -> tuple[float, float]:
    """The window pixel at the centre char cell of data cell (row, col)."""
    gcol = col * CELL_CW + CELL_CW // 2
    grow = row * CELL_CH + CELL_CH // 2
    return (
        GRID_POS[0] + gcol * CHAR_W + CHAR_W / 2,
        GRID_POS[1] + grow * CHAR_H + CHAR_H / 2,
    )


def reverse_at(snap, gcol: int, grow: int) -> bool:
    """Whether the char cell (gcol, grow) paints reverse-video."""
    row = find_by_tag(snap, GRID)["grid_rows"][grow]
    for run in row["runs"]:
        if run["start"] <= gcol < run["start"] + run["len"]:
            return bool(run["attrs"]["reverse"])
    raise AssertionError(f"char cell ({gcol},{grow}) is covered by a run")


def value_at(tf, row: int, col: int) -> int:
    return int(tf.invoke(f"{EXT}/value_at", f"{row},{col}"))


def assert_hovered(tf, snap, row: int, col: int) -> None:
    """The full witness: the queried cell + value, the reverse-video over the
    hovered data cell's char block, and the readout == the value_at matrix."""
    assert_eq(tf.query(f"{EXT}/hovered_row"), row, "hovered_row")
    assert_eq(tf.query(f"{EXT}/hovered_col"), col, "hovered_col")
    v = value_at(tf, row, col)
    assert_eq(tf.query(f"{EXT}/hovered_value"), v, "hovered_value == value_at")
    # Every char cell of the hovered data cell reverse-videos; a neighbour does
    # not.
    for dgc in range(CELL_CW):
        for dgr in range(CELL_CH):
            assert reverse_at(snap, col * CELL_CW + dgc, row * CELL_CH + dgr), (
                f"hovered cell char ({dgc},{dgr}) reversed"
            )
    if col > 0:
        assert not reverse_at(snap, (col - 1) * CELL_CW, row * CELL_CH), (
            "the left neighbour is not reversed"
        )


def body() -> None:
    with RpcSubprocess("hello-heatmap") as tf:
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, GRID) or {}).get("cols") == GCOLS,
            source="paint",
            viewport=WIN,
            desc="heatmap grid resolved",
        )

        # --- boot: nothing hovered ---
        assert_eq(tf.query(f"{EXT}/rows"), 8, "8 rows")
        assert_eq(tf.query(f"{EXT}/cols"), 12, "12 cols")
        assert_eq(tf.query(f"{EXT}/max_value"), 100, "value range top")
        assert_eq(tf.query(f"{EXT}/hovered_row"), None, "boot: no hover")
        assert_eq(tf.query(f"{EXT}/hovered_value"), None, "boot: no value")

        # --- a REAL bare hover over the hotspot cell (row 2, col 8) ---
        tf.hover(at=cell_center_px(2, 8))
        hot = wait_snap(
            tf,
            lambda s: tf.query(f"{EXT}/hovered_row") == 2
            and tf.query(f"{EXT}/hovered_col") == 8,
            source="paint",
            viewport=WIN,
            desc="hover the hotspot cell (2,8)",
        )
        assert_hovered(tf, hot, 2, 8)

        # --- hover the far corner (row 7, col 11) = the brightest cell ---
        tf.hover(at=cell_center_px(7, 11))
        corner = wait_snap(
            tf,
            lambda s: tf.query(f"{EXT}/hovered_row") == 7
            and tf.query(f"{EXT}/hovered_col") == 11,
            source="paint",
            viewport=WIN,
            desc="hover the far corner (7,11)",
        )
        assert_hovered(tf, corner, 7, 11)
        # The far corner is brighter than the origin (the diagonal ramp rises);
        # the global peak is the hotspot at (2, 8), not a corner.
        assert value_at(tf, 7, 11) > value_at(tf, 0, 0), "far corner > origin"
        assert value_at(tf, 2, 8) >= value_at(tf, 7, 11), "the hotspot is the peak"

        # --- hover OFF the grid (the title area) fires Leave -> clears ---
        tf.hover(at=(300, 16))
        gone = wait_snap(
            tf,
            lambda s: tf.query(f"{EXT}/hovered_row") is None,
            source="paint",
            viewport=WIN,
            desc="hovering off the grid clears the inspector",
        )
        assert_eq(tf.query(f"{EXT}/hovered_value"), None, "cleared")
        # No cell is reversed once the hover left.
        assert not reverse_at(gone, 11 * CELL_CW, 7 * CELL_CH), "corner no longer lit"

        # --- the AI-first no-pixel drive: intervene hovered_cell ---
        tf.intervene(f"{EXT}/hovered_cell", "0,0")
        origin = wait_snap(
            tf,
            lambda s: tf.query(f"{EXT}/hovered_row") == 0,
            source="paint",
            viewport=WIN,
            desc="intervene hovered_cell 0,0",
        )
        assert_hovered(tf, origin, 0, 0)

        # --- value_at reads the whole matrix; a monotone diagonal check ---
        assert value_at(tf, 0, 0) < value_at(tf, 4, 6) < value_at(tf, 7, 11), (
            "the diagonal ramp rises to the far corner"
        )
        assert tf.invoke(f"{EXT}/value_at", "9,9") == "none", "off the matrix"


if __name__ == "__main__":
    sys.exit(run_demo("r1408_heatmap", body))
