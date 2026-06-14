#!/usr/bin/env python3
"""R930 §5.38 — data-grid dynamic rows (add / remove), over RPC.

A table that cannot grow is a toy table. R930 makes the editable data grid
*dynamic-length*: the row count is derived from the flat cell model
(`model.len() / NCOLS`, not a `const`), so `invoke add_row` / `remove_row`
grow and shrink the table at runtime and the very next paint re-derives the
sort / filter / group order over the new model — the model is the one SSOT,
so an appended row edits and sorts exactly like a seeded one, with no parallel
machinery and no separate row-count to keep in sync. An AI agent drives and
reads the whole round-trip over the §5.12 RPC plane (§2 #2).

The seed table is 4 rows x 5 columns (Asset / Tag / Count / Scale / Active).

  (A) boot taxonomy — 4 rows, the seed values, the "of 4" status.
  (B) add_row — appends a typed-empty row, grows the count, moves the cursor
      onto it; the appended row edits like any seeded cell.
  (C) the added row participates in sort (Count 0 leads an ascending sort).
  (D) remove_row — drops a source row; indices above it shift down.
  (E) rejects — out-of-range row; the last remaining row is never removed.

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r930_data_grid_rows.py
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

VIEWPORT = (460, 320)
GRID = "data_grid"


def q(tf, path: str):
    return tf.query(f"/external/{path}")


def inv(tf, verb: str, arg=None):
    return tf.invoke(f"/external/{verb}", arg)


def body() -> None:
    with RpcSubprocess("hello-data-grid", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy ────────────────────────────────────────
        assert find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), GRID) is not None, "grid present"
        assert_eq(q(tf, "row_count"), 4, "4 seed rows")
        assert_eq(q(tf, "col_count"), 5, "5 columns")
        assert_eq(q(tf, "col_name.2"), "Count", "col 2 is Count")
        assert_eq(q(tf, "col_kind.2"), "int", "Count is an int column")
        assert_eq(q(tf, "col_kind.4"), "bool", "Active is a bool column")
        assert_eq(q(tf, "value.0.0"), "Hero", "row 0 Asset")
        assert_eq(q(tf, "value.1.0"), "Tree", "row 1 Asset")
        assert_eq(q(tf, "value.3.0"), "Boss", "row 3 Asset")

        # ── (B) add_row appends a typed-empty row + moves the cursor ──
        assert_eq(inv(tf, "add_row"), 4, "add_row returns the new source index")
        assert_eq(q(tf, "row_count"), 5, "one row added")
        assert_eq(q(tf, "value.4.0"), "", "new Asset cell is the empty Text default")
        assert_eq(q(tf, "value.4.2"), 0, "new Count cell is the Int default 0")
        assert_eq(q(tf, "value.4.3"), 0.0, "new Scale cell is the Float default 0.0")
        assert_eq(q(tf, "value.4.4"), False, "new Active cell is the Bool default false")
        assert_eq(q(tf, "focused_row"), 4, "the cursor moved onto the new row")
        # The appended row edits exactly like a seeded cell (no parallel path).
        tf.intervene("/external/value.4.0", "Sword")
        assert_eq(q(tf, "value.4.0"), "Sword", "the appended row edits like a seeded one")
        tf.intervene("/external/value.4.4", True)
        assert_eq(q(tf, "value.4.4"), True, "and toggles its bool")

        # ── (C) the added row participates in sort ───────────────────
        # New row's Count is the default 0 — below every seed Count (1/24/99/1),
        # so an ascending Count sort puts it first.
        inv(tf, "cycle_sort", 2)  # unsorted -> ascending by Count (col 2)
        assert_eq(q(tf, "visible_len"), 5, "all five rows are visible (no filter)")
        assert_eq(q(tf, "source_at.0"), 4, "the added row (Count 0) sorts to the front")
        inv(tf, "cycle_sort", 2)  # ascending -> descending
        inv(tf, "cycle_sort", 2)  # descending -> unsorted (clear for the remove phase)
        assert_eq(q(tf, "source_at.0"), 0, "back to source order (Hero first)")

        # ── (D) remove_row drops a source row + shifts indices down ──
        # Source rows now: 0 Hero, 1 Tree, 2 Coin, 3 Boss, 4 Sword.
        assert_eq(inv(tf, "remove_row", 1), True, "remove source row 1 (Tree)")
        assert_eq(q(tf, "row_count"), 4, "one row removed")
        assert_eq(q(tf, "value.1.0"), "Coin", "old row 2 (Coin) shifted down to index 1")
        assert_eq(q(tf, "value.2.0"), "Boss", "and Boss to index 2")
        assert_eq(q(tf, "value.3.0"), "Sword", "and the added Sword to index 3")

        # ── (E) rejects — out-of-range + keep at least one row ───────
        assert_eq(inv(tf, "remove_row", 99), False, "out-of-range row rejected")
        assert_eq(q(tf, "row_count"), 4, "rejected remove changed nothing")
        for _ in range(3):
            assert_eq(inv(tf, "remove_row", 0), True, "drain a row")
        assert_eq(q(tf, "row_count"), 1, "down to a single row")
        assert_eq(inv(tf, "remove_row", 0), False, "the last remaining row is never removed")
        assert_eq(q(tf, "row_count"), 1, "still one row")
        assert_eq(q(tf, "focused_row"), 0, "the cursor stays clamped into the single surviving row")


if __name__ == "__main__":
    sys.exit(run_demo("R930 data-grid dynamic rows", body))
