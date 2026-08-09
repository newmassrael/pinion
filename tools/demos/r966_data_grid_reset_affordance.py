#!/usr/bin/env python3
"""R966 §5.38 §5.40 — visible row / column reset affordance on the data grid.

R965 landed `reset_row` / `reset_col` as RPC-only verbs (no shipped UI
affordance — its own honesty note). R966 lands the visible affordance + its
accessible action, so a human (not just the AI) can reset a whole row / column:

  (A) **column-header reset dot**: a column holding ANY modified cell paints an
      accent reset dot at its header cell's trailing edge, tagged
      `data_grid#resetcol<col>`. A click resets that whole column to its column
      default. The dot is a child on top of the header's sort target, so a dot
      click resets while a click elsewhere on the header still sorts.
  (B) **row reset dot**: a row holding ANY modified cell paints a reset dot in
      its leading handle gutter (the row-header column the header's blank
      leading cell aligns over), tagged `data_grid#resetrow<row>`. A click
      resets that whole row; a press on the grip glyph still arms a drag.
  (C) **AI-first read peers**: `col_modified.<col>` / `row_modified.<row>` (the
      1-D aggregate of `modified.<row>.<col>`); a dot's presence == its query.
  (D) the reset is POINTER + RPC accessible (this demo). R980 additionally made
      the cell + column reset AT-reachable (a reset `button` AccessNode child of
      the gridcell / columnheader, an AT Click routed through the same `send`
      wire) — see tools/demos/r980_access_reset.py; R982 made the ROW reset
      AT-reachable too via a `rowheader` host (tools/demos/r982_data_grid_row_reset.py).

The Asset column (col 0, leftmost) header dot + the row-0 dot (in the leading
gutter) are fully inside the viewport, so they are exercised by POINTER click;
the off-screen Count column uses the reset_col / reset_row RPC peer (R960's
clip-gated-column pattern).

Every seed cell starts away from the empty column defaults, so every column +
row boots modified (the engine's "a customized instance shows its overrides").

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r966_data_grid_reset_affordance.py

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
VIEWPORT = (460, 348)  # the example's live window (so snapshot == hit-test scene)

ASSET_COL = 0  # Text, leftmost — fully inside the viewport, pointer-tested
COUNT_COL = 2  # Int, off-screen at boot — exercised via the reset_col RPC peer


def col_dot(col: int) -> str:
    """The column-header reset-dot click target — `data_grid#resetcol<col>`."""
    return f"{GRID}#resetcol{col}"


def row_dot(row: int) -> str:
    """The per-row reset-dot click target — `data_grid#resetrow<row>`."""
    return f"{GRID}#resetrow{row}"


def dot_present(tf: RpcSubprocess, tag: str) -> bool:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, tag) is not None


def body() -> None:
    with RpcSubprocess(EXAMPLE, request_timeout=12.0) as tf:
        wait_snap(
            tf,
            lambda s: find_by_tag(s, f"{GRID}#0_{ASSET_COL}") is not None,
            viewport=VIEWPORT,
            desc="grid painted",
        )

        # ── (A/C) every column + row boots modified (seed != column defaults), so
        # every header + row reset dot paints; the query is the dot's read peer.
        for col in range(6):
            assert_eq(tf.query(f"{EXT}/col_modified.{col}"), True, f"col {col} boots modified")
        for row in range(4):
            assert_eq(tf.query(f"{EXT}/row_modified.{row}"), True, f"row {row} boots modified")
        assert dot_present(tf, col_dot(ASSET_COL)), "the Asset column shows a header reset dot"
        assert dot_present(tf, col_dot(COUNT_COL)), "the Count column shows a header reset dot"
        assert dot_present(tf, row_dot(0)), "row 0 shows a reset dot in its handle gutter"

        # ── (A) click the in-viewport Asset header dot -> the whole column resets
        assert_eq(tf.query(f"{EXT}/value.0.{ASSET_COL}"), "Hero", "seed row-0 Asset is Hero")
        tf.click(path=col_dot(ASSET_COL))
        wait_query(tf, f"{EXT}/col_modified.{ASSET_COL}", False, desc="header dot click reset the Asset column")
        for row in range(4):
            assert_eq(tf.query(f"{EXT}/value.{row}.{ASSET_COL}"), "", f"Asset row {row} is now the column default")
        wait_snap(
            tf,
            lambda s: find_by_tag(s, col_dot(ASSET_COL)) is None,
            viewport=VIEWPORT,
            desc="the column reset dot vanished once the column is clean",
        )

        # ── (B) row 0 keeps its dot while cols 1..5 stay modified; click resets it
        assert_eq(tf.query(f"{EXT}/row_modified.0"), True, "row 0 still modified after the column reset")
        assert dot_present(tf, row_dot(0)), "row 0's reset dot persists while the row has modified cells"
        tf.click(path=row_dot(0))
        wait_query(tf, f"{EXT}/row_modified.0", False, desc="row dot click reset the whole row")
        wait_snap(
            tf,
            lambda s: find_by_tag(s, row_dot(0)) is None,
            viewport=VIEWPORT,
            desc="the row reset dot vanished once the row is clean",
        )

        # ── (C) the off-screen Count column via the reset_col RPC peer
        assert_eq(tf.query(f"{EXT}/col_modified.{COUNT_COL}"), True, "the Count column is still modified")
        cleared = tf.invoke(f"{EXT}/reset_col", COUNT_COL)
        assert isinstance(cleared, int) and cleared > 0, f"reset_col cleared the Count cells (got {cleared})"
        wait_query(tf, f"{EXT}/col_modified.{COUNT_COL}", False, desc="RPC reset_col cleared the Count column")
        for row in range(4):
            assert_eq(tf.query(f"{EXT}/value.{row}.{COUNT_COL}"), 0, f"Count row {row} is the column default 0")

        # ── (C) the row reset RPC peer + idempotent no-op semantics
        assert_eq(tf.query(f"{EXT}/row_modified.1"), True, "row 1 still has modified cells")
        cleared = tf.invoke(f"{EXT}/reset_row", 1)
        assert isinstance(cleared, int) and cleared > 0, f"reset_row cleared row 1 (got {cleared})"
        wait_query(tf, f"{EXT}/row_modified.1", False, desc="RPC reset_row cleared row 1")
        assert_eq(tf.invoke(f"{EXT}/reset_row", 1), 0, "an already-clean row is a 0 no-op")
        assert_eq(tf.invoke(f"{EXT}/reset_col", COUNT_COL), 0, "an already-clean column is a 0 no-op")

        # ── out-of-range read peers are graceful (false, never an error)
        assert_eq(tf.query(f"{EXT}/col_modified.99"), False, "an out-of-range column is not modified")
        assert_eq(tf.query(f"{EXT}/row_modified.99"), False, "an out-of-range row is not modified")

        # ── reset_all clears the rest; every row + column reset dot is gone
        remaining = tf.query(f"{EXT}/modified_count")
        assert isinstance(remaining, int) and remaining > 0, f"cells remain to clear (got {remaining})"
        assert_eq(tf.invoke(f"{EXT}/reset_all", None), remaining, "reset_all clears the remaining cells")
        wait_query(tf, f"{EXT}/modified_count", 0, desc="every cell now sits at its column default")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        for col in range(6):
            assert find_by_tag(snap, col_dot(col)) is None, f"no column reset dot remains at col {col}"
        for row in range(4):
            assert find_by_tag(snap, row_dot(row)) is None, f"no row reset dot remains at row {row}"


if __name__ == "__main__":
    sys.exit(run_demo("R966 §5.38 §5.40 — data-grid row/column reset affordance", body))
