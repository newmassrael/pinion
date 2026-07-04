#!/usr/bin/env python3
"""R1244 §5.38 §5.52 — data-grid paste AUTO-GROWS rows, over RPC.

R1237 pasted a TSV block at the cursor but CLIPPED any row that overran the last
visible row. R1244 finishes it: the row model is dynamic (R930 add_row), so a
paste that overruns GROWS the grid — each overrun line appends a fresh row and
lands in it, the spreadsheet-paste convention. The COLUMN schema is fixed
([`NCOLS`]), so cells past the right edge still clip. The whole paste — grown
rows AND their cells — is ONE undo step. All AI-first over the §5.12 plane.

The seed table is 4 rows x 6 cols (Asset / Type / Count / Scale / Active / Tint);
Count is col 2 (Int), Scale is col 3 (Float), Tint is col 5 (Color).

  (A) boot — 4 rows.
  (B) a 3-row block at the last row GROWS the grid to 6 rows; all cells land.
  (C) the grown rows are typed by their column (Int / Float) like any row.
  (D) ONE undo reverts the whole paste — the grown rows AND the anchor edits.
  (E) redo re-grows and re-writes; a 2nd undo settles back to the 4-row baseline.
  (F) columns still CLIP — a cell past the last column is dropped, no growth.
  (G) an in-range block (no overrun) still writes in place, growing nothing.

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r1244_data_grid_paste_grow.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

UNDO = "/data_grid_undo/external"


def q(tf, path: str):
    return tf.query(f"/external/{path}")


def inv(tf, verb: str, arg=None):
    return tf.invoke(f"/external/{verb}", arg)


def cursor(tf, row: int, col: int) -> None:
    tf.intervene("/external/focused_row", row)
    tf.intervene("/external/focused_col", col)


def body() -> None:
    with RpcSubprocess("hello-data-grid", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        assert_eq(q(tf, "row_count"), 4, "4 seed rows")
        assert_eq(q(tf, "col_kind.2"), "int", "col 2 (Count) is int")
        assert_eq(q(tf, "col_kind.3"), "float", "col 3 (Scale) is float")
        base_anchor = q(tf, "value.3.2")  # the last row's Count, before any paste

        # ── (B) a 3-row block at the last row grows to 6 rows ────────
        cursor(tf, 3, 2)  # last row, Count column
        assert_eq(inv(tf, "paste", "11\t1.5\n22\t2.5\n33\t3.5"), 6, "all six cells land")
        assert_eq(q(tf, "row_count"), 6, "the grid grew from 4 to 6 rows")

        # ── (C) every landed row is typed by its column ──────────────
        assert_eq(q(tf, "value.3.2"), 11, "anchor row Count = 11")
        assert_eq(q(tf, "value.3.3"), 1.5, "anchor row Scale = 1.5")
        assert_eq(q(tf, "value.4.2"), 22, "grown row 4 Count = 22")
        assert_eq(q(tf, "value.4.3"), 2.5, "grown row 4 Scale = 2.5")
        assert_eq(q(tf, "value.5.2"), 33, "grown row 5 Count = 33")
        assert_eq(q(tf, "value.5.3"), 3.5, "grown row 5 Scale = 3.5")
        # A column NOT in the block keeps the appended row's typed default.
        assert_eq(q(tf, "value.5.0"), "", "grown row's Asset is the empty default")

        # ── (D) ONE undo reverts the whole paste (rows + cells) ──────
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Paste", "the whole paste is one step")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "one undo")
        assert_eq(q(tf, "row_count"), 4, "the two grown rows are gone (row 4/5 removed)")
        assert_eq(q(tf, "value.3.2"), base_anchor, "the anchor row's Count reverted too")

        # ── (E) redo re-grows; a 2nd undo settles to baseline ────────
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo re-applies")
        assert_eq(q(tf, "row_count"), 6, "redo re-grows to 6 rows")
        assert_eq(q(tf, "value.5.2"), 33, "redo re-writes the grown cell")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo back to baseline")
        assert_eq(q(tf, "row_count"), 4, "back to the 4-row baseline")

        # ── (F) columns still CLIP (fixed schema) ────────────────────
        cursor(tf, 0, 5)  # last column (Tint / Color)
        assert_eq(inv(tf, "paste", "#a1b2c3\tCLIP"), 1, "only the colour lands; the extra column clips")
        assert_eq(q(tf, "row_count"), 4, "a column overrun never grows rows")
        tf.invoke(f"{UNDO}/undo", None)  # revert the colour before the last case

        # ── (G) an in-range block writes in place, growing nothing ───
        cursor(tf, 0, 2)  # room for 2 rows below (rows 0,1)
        assert_eq(inv(tf, "paste", "7\n8"), 2, "both rows are in range")
        assert_eq(q(tf, "row_count"), 4, "an in-range paste grows nothing")
        assert_eq(q(tf, "value.0.2"), 7, "row 0 Count = 7")
        assert_eq(q(tf, "value.1.2"), 8, "row 1 Count = 8")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the in-range paste")
        assert_eq(q(tf, "value.0.2"), 1, "row 0 Count restored")
        assert_eq(q(tf, "value.1.2"), 24, "row 1 Count restored (both in-range rows revert)")

        # ── (H) R1247 — an all-unparseable overrun line grows NO phantom row ──
        # A text label over the Int column (the real spreadsheet case): the
        # anchor lands 55, the overrun "Total" parses to nothing, so growth is
        # per LANDED row, not per overrun line — no phantom empty row.
        cursor(tf, 3, 2)  # last row, Int column
        assert_eq(inv(tf, "paste", "55\nTotal"), 1, "only the anchor cell lands")
        assert_eq(q(tf, "row_count"), 4, "the unparseable overrun grew NO phantom row")
        assert_eq(q(tf, "value.3.2"), 55, "the anchor row got 55")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the anchor write")
        assert_eq(q(tf, "value.3.2"), base_anchor, "the anchor Count restored")


if __name__ == "__main__":
    sys.exit(run_demo("R1244 data-grid paste auto-grows rows", body))
