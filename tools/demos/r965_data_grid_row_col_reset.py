#!/usr/bin/env python3
"""R965 §5.38 §5.40 — per-row / per-column reset-to-default on the data grid.

Drives hello-data-grid over JSON-RPC. R960 gave the editable grid a per-cell
"reset to column default" (the Unreal / Qt affordance); R965 adds the row- and
column-granular bulk variants (Qt / Excel "reset this row" / "reset this
column"):

  (A) **reset_row <row>** — reset every modified cell in that row to its column
      default; returns the count cleared, leaving every other row untouched.
  (B) **reset_col <col>** — reset every modified cell in that column (all rows)
      to the column default; returns the count cleared, other columns untouched.
  (C) both are one batched pass through the `reset_cells` SSOT (the R961.1
      single-snapshot discipline — never the O(cells²) per-cell `set_cell`
      loop), shared with `reset_all`.
  (D) the visible reset dot (`data_grid#reset<row>_<col>`) appears for a
      modified cell and vanishes when its row / column is reset.

The Asset column (col 0) is fully inside the h-scroll viewport at boot, so the
*paint* dot check uses it; the Count column (col 2) is bounded 0..1000, so cell
writes there are deterministic.

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r965_data_grid_row_col_reset.py

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
    wait_query,
    wait_snap,
)

EXAMPLE = "hello-data-grid"
GRID = "data_grid"
EXT = "/external"
VIEWPORT = (460, 348)

COUNT_COL = 2  # Int, bounded 0..1000 — off-screen but RPC-addressable
ASSET_COL = 0  # Text, default "" — fully inside the h-scroll viewport


def reset_dot(row: int, col: int) -> str:
    return f"{GRID}#reset{row}_{col}"


def modq(g: RpcSubprocess, row: int, col: int) -> object:
    return g.query(f"{EXT}/modified.{row}.{col}")


def count(g: RpcSubprocess) -> object:
    return g.query(f"{EXT}/modified_count")


def body() -> None:
    with RpcSubprocess(EXAMPLE, request_timeout=12.0) as g:
        rows = g.query(f"{EXT}/row_count")
        assert isinstance(rows, int) and rows >= 3, f"need >= 3 rows, got {rows}"

        # Clean slate so the counts below are exact.
        g.invoke(f"{EXT}/reset_all", None)
        wait_query(g, f"{EXT}/modified_count", 0, desc="reset_all gives a clean slate")

        # ── (A) reset_row: modify Count (col 2) in rows 0, 1, 2 ─────────────
        for r, v in ((0, 50), (1, 60), (2, 70)):
            g.intervene(f"{EXT}/value.{r}.{COUNT_COL}", v)
        wait_query(g, f"{EXT}/modified_count", 3, desc="three Count cells modified")
        for r in (0, 1, 2):
            assert_eq(modq(g, r, COUNT_COL), True, f"row {r} Count modified")

        # reset_row(1) clears row 1 only.
        assert_eq(g.invoke(f"{EXT}/reset_row", 1), 1, "reset_row(1) clears one cell")
        wait_query(g, f"{EXT}/modified.1.{COUNT_COL}", False, desc="row 1 Count reset")
        assert_eq(modq(g, 0, COUNT_COL), True, "row 0 untouched")
        assert_eq(modq(g, 2, COUNT_COL), True, "row 2 untouched")
        assert_eq(count(g), 2, "two cells remain modified")
        assert_eq(g.invoke(f"{EXT}/reset_row", 1), 0, "an already-default row is a 0 no-op")

        # ── (B) reset_col: add an Asset (col 0) edit, then reset the Count col
        g.intervene(f"{EXT}/value.0.{ASSET_COL}", "renamed")
        wait_query(g, f"{EXT}/modified_count", 3, desc="Asset edit + two Count cells")
        assert_eq(modq(g, 0, ASSET_COL), True, "Asset cell modified")

        # reset_col(2) clears both remaining Count cells; Asset (col 0) untouched.
        assert_eq(g.invoke(f"{EXT}/reset_col", COUNT_COL), 2, "reset_col(2) clears both Count cells")
        wait_query(g, f"{EXT}/modified.0.{COUNT_COL}", False, desc="row 0 Count reset by column")
        assert_eq(modq(g, 2, COUNT_COL), False, "row 2 Count reset by column")
        assert_eq(modq(g, 0, ASSET_COL), True, "the Asset column is untouched")
        assert_eq(count(g), 1, "only the Asset edit remains")
        assert_eq(g.invoke(f"{EXT}/reset_col", COUNT_COL), 0, "an already-default column is a 0 no-op")

        # ── (C) out-of-range row / column is a 0 no-op (never a panic) ──────
        assert_eq(g.invoke(f"{EXT}/reset_row", 99), 0, "out-of-range row resets nothing")
        assert_eq(g.invoke(f"{EXT}/reset_col", 99), 0, "out-of-range column resets nothing")

        # ── (D) the visible reset dot tracks a row reset (Asset col, on-screen)
        snap = wait_snap(
            g,
            lambda s: find_by_tag(s, reset_dot(0, ASSET_COL)) is not None,
            viewport=VIEWPORT,
            desc="the modified Asset cell paints a reset dot",
        )
        assert find_by_tag(snap, reset_dot(0, ASSET_COL)) is not None, "dot present while modified"
        assert_eq(g.invoke(f"{EXT}/reset_row", 0), 1, "reset_row(0) clears the Asset edit")
        snap = wait_snap(
            g,
            lambda s: find_by_tag(s, reset_dot(0, ASSET_COL)) is None,
            viewport=VIEWPORT,
            desc="the dot vanishes once the row is reset",
        )
        assert find_by_tag(snap, reset_dot(0, ASSET_COL)) is None, "dot gone after reset"
        assert_eq(count(g), 0, "the grid is back to all-default")


if __name__ == "__main__":
    sys.exit(run_demo("R965 §5.38 §5.40 — per-row / per-column reset", body))
