#!/usr/bin/env python3
"""R1372 §5.38 §5.52 — data-grid cell-range COPY (TSV), over RPC.

R1237 gave the editable grid a block PASTE (write half). R1372 adds the COPY
half so the copy/paste pair is symmetric: a rectangular cell-range selection
(the spreadsheet / Qt SelectItems model the Table widget got in R952) plus a
`copy` that serializes the selection to the SAME TSV a `paste` consumes. The
range endpoints are the SOURCE-keyed anchor + cursor; the selected rectangle is
their bounding box over the VISIBLE order, so it is always one contiguous screen
block and the copy reads top-to-bottom exactly as a paste writes.

Wire (the cross-grid cell-selection vocabulary — the Table widget /
hello-cell-select speak it identically):
  invoke  select-cell "r,c" / extend-cell "r,c" / clear-cell-selection
  query   cell_selection ("p0,c0,p1,c1" VISIBLE positions / null) /
          cell_selection_count / cell_selection_tsv /
          copy_tsv (what Ctrl+C copies: range TSV or the lone focused cell)

The seed table is 4 rows x 6 cols (Asset / Type / Count / Scale / Active / Tint);
Asset is col 0 (Text: Hero / Tree / Coin / Boss), Count is col 2 (Int: 1/24/99/1).

  (A) boot — no range: the reads are null / 0 / null.
  (B) select + extend — a 3x2 rectangle; cell_selection / count / the TSV block.
  (C) copy — the `copy` action == cell_selection_tsv (one funnel); with no range
      `copy` yields the lone focused cell.
  (D) copy -> paste ROUND-TRIP — copy a column, paste it below, the cells match
      (the symmetry payoff, all via the same TSV string).
  (E) clear + out-of-range / malformed guards.
  (F) visible order — under an active sort the rectangle is VISIBLE positions and
      the copy reads the visual rows, never raw source order.

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r1372_data_grid_copy.py
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
        # ── (A) boot — no range ──────────────────────────────────────
        assert_eq(q(tf, "row_count"), 4, "4 seed rows")
        assert_eq(q(tf, "value.0.0"), "Hero", "row0 Asset seed")
        assert_eq(q(tf, "value.2.0"), "Coin", "row2 Asset seed")
        assert_eq(q(tf, "cell_selection"), None, "no range at boot")
        assert_eq(q(tf, "cell_selection_count"), 0, "no cells selected")
        assert_eq(q(tf, "cell_selection_tsv"), None, "no TSV without a range")

        # ── (B) select (0,0) + extend to (2,1) => a 3x2 rectangle ────
        assert_eq(inv(tf, "select-cell", "0,0"), True, "start the range at (0,0)")
        assert_eq(q(tf, "cell_selection"), "0,0,0,0", "a fresh selection is one cell")
        assert_eq(q(tf, "cell_selection_count"), 1, "one cell so far")
        assert_eq(inv(tf, "extend-cell", "2,1"), True, "extend to (2,1)")
        assert_eq(q(tf, "cell_selection"), "0,0,2,1", "the 3x2 rectangle")
        assert_eq(q(tf, "cell_selection_count"), 6, "3 rows x 2 cols = 6 cells")
        tsv = q(tf, "cell_selection_tsv")
        assert tsv.count("\n") == 2, f"3 rows -> 2 newlines, got {tsv!r}"
        for line in tsv.split("\n"):
            assert line.count("\t") == 1, f"2 cols -> 1 tab per row, got {line!r}"
        assert tsv.startswith("Hero\t"), f"row 0 asset is Hero, got {tsv!r}"
        assert "\nTree\t" in tsv, "row 1 asset is Tree"
        assert "\nCoin\t" in tsv, "row 2 asset is Coin"

        # ── (C) copy_tsv — a READ (CQS), not an action; lone-cell fallback ─
        assert_eq(q(tf, "copy_tsv"), tsv, "copy_tsv == cell_selection_tsv (one funnel)")
        assert_eq(inv(tf, "clear-cell-selection"), True, "drop the range")
        assert_eq(q(tf, "cell_selection"), None, "range cleared")
        cursor(tf, 1, 0)  # lone cursor on Tree
        assert_eq(q(tf, "copy_tsv"), "Tree", "with no range, copy_tsv is the lone cell")

        # ── (D) copy -> paste ROUND-TRIP (the symmetry payoff) ───────
        # Copy the Asset column rows 0..1 ("Hero\nTree") then paste it at row 2,
        # so rows 2 and 3 Asset become Hero and Tree — all via the same string.
        inv(tf, "select-cell", "0,0")
        inv(tf, "extend-cell", "1,0")
        col_copy = q(tf, "copy_tsv")
        assert_eq(col_copy, "Hero\nTree", "copied the Asset column top pair")
        cursor(tf, 2, 0)
        assert_eq(inv(tf, "paste", col_copy), 2, "the copied block pastes 2 cells")
        assert_eq(q(tf, "value.2.0"), "Hero", "row 2 Asset now Hero (pasted)")
        assert_eq(q(tf, "value.3.0"), "Tree", "row 3 Asset now Tree (pasted)")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the round-trip paste")
        assert_eq(q(tf, "value.2.0"), "Coin", "row 2 Asset restored")
        assert_eq(q(tf, "value.3.0"), "Boss", "row 3 Asset restored")

        # ── (E) guards — out-of-range no-op, malformed reject ────────
        inv(tf, "clear-cell-selection")
        assert_eq(inv(tf, "select-cell", "99,0"), False, "out-of-range select is a no-op")
        assert_eq(q(tf, "cell_selection"), None, "no range from an out-of-range select")

        # ── (F) the selection + copy follow the active sort ──────────
        inv(tf, "cycle_sort", 0)  # sort col 0 (Asset) ascending: Boss/Coin/Hero/Tree
        s0 = q(tf, "source_at.0")  # source row shown at visual position 0
        s1 = q(tf, "source_at.1")
        assert s0 != 0, "the sort reorders (visual row 0 is not source row 0)"
        assert_eq(inv(tf, "select-cell", f"{s0},0"), True, "anchor at visual row 0's source")
        assert_eq(inv(tf, "extend-cell", f"{s1},0"), True, "extend to visual row 1's source")
        # The rectangle reads in VISIBLE positions (0..1), not the source rows.
        assert_eq(q(tf, "cell_selection"), "0,0,1,0", "bounds are visible positions")
        # copy_tsv reads the VISUAL order: sorted Asset column top pair = Boss, Coin.
        assert_eq(q(tf, "copy_tsv"), "Boss\nCoin", "copy_tsv reads the sorted visual order")


if __name__ == "__main__":
    sys.exit(run_demo("R1372 data-grid cell-range copy (TSV)", body))
