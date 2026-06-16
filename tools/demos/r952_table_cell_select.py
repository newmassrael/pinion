#!/usr/bin/env python3
"""R952 §5.38 §5.40 — cell range selection in the data grid (anchor + extent).

The `Table` widget had a 2-D roving cursor (`focused_row` / `focused_col`) and
a per-row selection (`SelectRows`), but no **cell range selection** — the
spreadsheet / Qt `QTableView` `SelectItems` model where a rectangle of cells is
selected by anchor + extent. R952 adds it: `select-cell` starts a single-cell
selection at the cursor, `extend-cell` grows the rectangle to the cursor, and
the selection is the bounding box of the anchor and the roving cursor (data
coords, so a sort repositions the cells without remapping). `hello-table` drives
it from the keyboard — a plain Arrow collapses the selection to the new cell, a
`Shift`+Arrow extends the rectangle — and paints it as a bordered accent overlay.

An AI agent drives + reads the whole thing over the §5.12 RPC plane (§2 #2):

  (A) `select-cell` then `extend-cell` → the selected rectangle grows; the
      cursor follows the extent; the overlay rectangle appears (the headline).
  (B) `extend-cell` toward the top-left normalizes the bounds (anchor pinned).
  (C) the painted overlay rectangle covers exactly the selected cells (a
      same-snapshot geometry check — no cross-process timing).
  (D) a plain Arrow collapses the selection to a single cell at the new cursor.
  (E) a `Shift`+Arrow extends the rectangle from the anchor.
  (F) `clear-cell-selection` drops it (overlay gone); out-of-range select is a
      no-op (`false`), not an error.

Run from the workspace root:
    cargo build -p hello-table --release
    python3 tools/demos/r952_table_cell_select.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_until,
)

T = "table"
VIEWPORT = (540, 360)
CELLSEL_TAG = "table_cellsel"


def q(d: RpcSubprocess, slot: str):
    return d.query(f"/external/{slot}")


def cell_sel(d: RpcSubprocess):
    return q(d, "cell_selection")


def cell_count(d: RpcSubprocess) -> int:
    return q(d, "cell_selection_count")


def overlay_present(d: RpcSubprocess) -> bool:
    snap = d.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, CELLSEL_TAG) is not None


def body() -> None:
    with RpcSubprocess("hello-table") as d:
        # ── boot: a 6x4 grid with no cell selection ───────────────────
        assert_eq(q(d, "rows"), 6, "6 data rows")
        assert_eq(q(d, "cols"), 4, "4 columns")
        assert_eq(cell_sel(d), None, "no cell selection at boot")
        assert_eq(cell_count(d), 0, "empty selection counts zero cells")
        assert not overlay_present(d), "no selection overlay before any selection"
        assert d.request("focus/set", {"tag": T}).result.get("focused") == T, "grid focused"

        # ── (A) select-cell then extend-cell — the rectangle grows ────
        assert d.invoke("/external/select-cell", "1,1") is True, "select-cell handled"
        wait_query(d, "/external/cell_selection", "1,1,1,1", desc="single-cell selection at (1,1)")
        assert_eq(cell_count(d), 1, "one cell selected")
        assert_eq(q(d, "focused_row"), 1, "the cursor row followed select-cell")
        assert_eq(q(d, "focused_col"), 1, "the cursor col followed select-cell")
        wait_until(lambda: overlay_present(d), desc="the selection overlay is painted")

        assert d.invoke("/external/extend-cell", "3,2") is True, "extend-cell handled"
        wait_query(d, "/external/cell_selection", "1,1,3,2", desc="rectangle (1,1)->(3,2)")
        assert_eq(cell_count(d), 6, "3 rows x 2 cols = 6 cells")
        assert_eq(q(d, "focused_row"), 3, "the cursor followed the extent (row)")
        assert_eq(q(d, "focused_col"), 2, "the cursor followed the extent (col)")

        # ── (B) extend toward the top-left normalizes the bounds ──────
        assert d.invoke("/external/extend-cell", "0,0") is True, "extend toward top-left"
        wait_query(d, "/external/cell_selection", "0,0,1,1", desc="bounds normalize (anchor 1,1)")
        assert_eq(cell_count(d), 4, "2x2 rectangle")

        # ── (C) the overlay rectangle covers the selected cells ───────
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        rects = abs_rects_of(snap)
        assert CELLSEL_TAG in rects, "the overlay carries an absolute rect"
        ox, oy, ow, oh = rects[CELLSEL_TAG]
        # Same-snapshot geometry (deterministic): the overlay's box contains the
        # union of the corner cells (0,0) and (1,1). A few px of slack for the
        # 2px selection border the overlay draws around the cells.
        c00 = rects["table#0_0"]
        c11 = rects["table#1_1"]
        tol = 4
        assert ox <= c00[0] + tol, f"overlay left {ox} <= cell(0,0) left {c00[0]}"
        assert oy <= c00[1] + tol, f"overlay top {oy} <= cell(0,0) top {c00[1]}"
        assert ox + ow + tol >= c11[0] + c11[2], "overlay right edge reaches cell(1,1)"
        assert oy + oh + tol >= c11[1] + c11[3], "overlay bottom edge reaches cell(1,1)"

        # ── (D) a plain Arrow collapses to a single cell at the cursor ─
        # Cursor is at (0,0) after the last extend; ArrowRight -> (0,1), and a
        # plain move collapses the selection to that one cell.
        d.key(path=T, name="ArrowRight")
        wait_query(d, "/external/cell_selection", "0,1,0,1", desc="plain Arrow collapses to (0,1)")
        assert_eq(cell_count(d), 1, "collapsed to a single cell")

        # ── (E) a Shift+Arrow extends the rectangle from the anchor ───
        d.modifiers(shift=True)  # held-modifier cache; the next key press reads it
        d.key(path=T, name="ArrowDown")
        d.modifiers()  # release
        wait_query(d, "/external/cell_selection", "0,1,1,1", desc="Shift+Arrow extends to (1,1)")
        assert_eq(cell_count(d), 2, "the rectangle grew to two cells")

        # ── (F) clear, and out-of-range is a no-op ────────────────────
        assert d.invoke("/external/clear-cell-selection", None) is True, "clear handled"
        wait_query(d, "/external/cell_selection", None, desc="selection cleared")
        assert_eq(cell_count(d), 0, "cleared selection counts zero")
        wait_until(lambda: not overlay_present(d), desc="the overlay is gone after clear")
        assert d.invoke("/external/select-cell", "9,9") is False, "out-of-range select is a no-op (false)"
        assert_eq(cell_sel(d), None, "out-of-range select changed nothing")


if __name__ == "__main__":
    sys.exit(run_demo("R952 §5.38 — data-grid cell range selection", body))
