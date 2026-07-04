#!/usr/bin/env python3
"""R1237 §5.38 §5.52 — data-grid block PASTE (TSV), over RPC.

R1222 gave the cell-range COPY (read half). R1237 adds the write half for the
editable grid: paste a TSV block anchored at the cursor. Rows split on `\\n`,
cells on `\\t`; the block fills the cells from the cursor down-and-right, each
parsed through its COLUMN's type + clamped, the whole paste ONE undo step (the
grid's first `begin_macro` transaction). The AI-first primary path (§2 #2) — an
agent supplies the block directly, no OS clipboard needed. Crucially the paste
follows the active sort / filter / group (visible order), never raw source order.

The seed table is 4 rows x 6 cols (Asset / Type / Count / Scale / Active / Tint);
Count is col 2 (Int), Scale is col 3 (Float).

  (A) boot — 4 rows, the seed values.
  (B) paste a 2x2 block at (row 0, col 2): the Int + Float cells take it; the
      verb returns the count of cells written (4).
  (C) the whole block is ONE undo step ("Paste"); undo restores every cell.
  (D) clip — a block overrunning the last row writes only the in-range cells,
      and never adds rows.
  (E) skip — a value that does not parse for the column's type is skipped (the
      cell keeps its prior value); an empty paste is a no-op.
  (F) visible order — under an active sort, the block lands on the VISUAL rows
      (`source_at.<pos>`), not source rows 0,1.

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r1237_data_grid_paste.py
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
        assert_eq(q(tf, "value.0.2"), 1, "row0 Count seed")
        assert_eq(q(tf, "value.1.2"), 24, "row1 Count seed")

        # ── (B) paste a 2x2 block at (row 0, col 2) ──────────────────
        cursor(tf, 0, 2)
        assert_eq(inv(tf, "paste", "42\t1.5\n7\t9.5"), 4, "four cells written")
        assert_eq(q(tf, "value.0.2"), 42, "row0 Count = 42")
        assert_eq(q(tf, "value.0.3"), 1.5, "row0 Scale = 1.5")
        assert_eq(q(tf, "value.1.2"), 7, "row1 Count = 7")
        assert_eq(q(tf, "value.1.3"), 9.5, "row1 Scale = 9.5")
        assert_eq(q(tf, "value.2.2"), 99, "a cell below the block is unchanged")
        assert_eq(q(tf, "value.0.0"), "Hero", "a cell left of the block is unchanged")

        # ── (C) one undo reverts the whole block; redo re-applies ────
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Paste", "the block is one step")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the paste")
        assert_eq(q(tf, "value.0.2"), 1, "row0 Count restored")
        assert_eq(q(tf, "value.1.2"), 24, "row1 Count restored")
        assert_eq(q(tf, "value.1.3"), 2.5, "row1 Scale restored")
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo the paste")
        assert_eq(q(tf, "value.1.2"), 7, "row1 Count re-applied")
        tf.invoke(f"{UNDO}/undo", None)  # settle back to baseline

        # ── (D) clip an overrunning block ────────────────────────────
        cursor(tf, 3, 2)  # last row
        assert_eq(inv(tf, "paste", "55\n66"), 1, "only the in-range row lands")
        assert_eq(q(tf, "value.3.2"), 55, "the last row got 55")
        assert_eq(q(tf, "row_count"), 4, "the overrun added no rows")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the clipped paste")
        assert_eq(q(tf, "value.3.2"), 1, "the last row's Count restored")

        # ── (E) skip unparseable + empty paste ───────────────────────
        cursor(tf, 0, 2)
        assert_eq(inv(tf, "paste", "abc"), 0, "'abc' does not parse into Int")
        assert_eq(q(tf, "value.0.2"), 1, "the Int cell keeps its value")
        assert_eq(inv(tf, "paste", ""), 0, "an empty paste writes nothing")

        # ── (F) the paste follows the active sort (visible order) ────
        inv(tf, "cycle_sort", 0)  # sort col 0 (Asset) ascending
        s0 = q(tf, "source_at.0")  # source row now shown at visual position 0
        s1 = q(tf, "source_at.1")
        assert s0 != 0, "the sort reorders (visual row 0 is not source row 0)"
        cursor(tf, s0, 0)  # cursor at visual row 0
        assert_eq(inv(tf, "paste", "Alpha\nBeta"), 2, "two Asset cells written")
        assert_eq(q(tf, f"value.{s0}.0"), "Alpha", "visual row 0 got Alpha")
        assert_eq(q(tf, f"value.{s1}.0"), "Beta", "visual row 1 got Beta (not source 1)")
        assert_eq(q(tf, "row_count"), 4, "the sorted paste added no rows")


if __name__ == "__main__":
    sys.exit(run_demo("R1237 data-grid block paste (TSV)", body))
