#!/usr/bin/env python3
"""R953 §5.38 §5.40 — the dedicated cell-range-selection grid (anchor + extent).

R952 added the spreadsheet / Qt `QTableView` `SelectItems` model to the `Table`
coordinator (a rectangle of cells selected by anchor + extent) but bolted it
onto `hello-table`, which already drove the `SelectRows` model (one whole row
washed). Two selection models in one grid is a no-mode smell — a real editor
picks one (Excel selects cells, a list selects rows). R953 splits the cell-range
model into this dedicated `hello-cell-select` grid (a small numeric spreadsheet)
and reverts `hello-table` to pure row selection.

This grid's *only* selection model is the cell rectangle:
  * `select-cell "r,c"` starts a single-cell selection at the cursor;
  * `extend-cell "r,c"` grows the rectangle from the pinned anchor;
  * a plain Arrow collapses the rectangle to the new cursor cell, a `Shift`+Arrow
    extends it; `Escape` clears it;
  * the rectangle is `bbox(anchor, cursor)` in data coords, painted as a bordered
    accent overlay (always contiguous — the grid never sorts, so R952's
    sorted-view overlay-omit never applies here).

An AI agent drives + reads the whole thing over the §5.12 RPC plane (§2 #2):

  (A) `select-cell` then `extend-cell` -> the rectangle grows; the cursor follows
      the extent; the overlay rectangle appears (the headline).
  (B) `extend-cell` toward the top-left normalizes the bounds (anchor pinned).
  (C) the painted overlay rectangle covers exactly the selected cells (a
      same-snapshot geometry check -- no cross-process timing).
  (D) a plain Arrow collapses the selection to a single cell at the new cursor.
  (E) a `Shift`+Arrow extends the rectangle from the anchor.
  (F) `Escape` clears it (overlay gone), the cursor staying put.
  (G) `Home` / `End` jump within the row, each a fresh single-cell selection.
  (H) `clear-cell-selection` drops it; out-of-range select is a no-op (`false`).

Run from the workspace root:
    cargo build -p hello-cell-select --release
    python3 tools/demos/r953_cell_select.py
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

T = "grid"
VIEWPORT = (540, 360)
CELLSEL_TAG = "grid_cellsel"


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
    with RpcSubprocess("hello-cell-select") as d:
        # -- boot: a 6x4 spreadsheet with no cell selection ----------------
        assert_eq(q(d, "rows"), 6, "6 data rows")
        assert_eq(q(d, "cols"), 4, "4 columns")
        assert_eq(cell_sel(d), None, "no cell selection at boot")
        assert_eq(cell_count(d), 0, "empty selection counts zero cells")
        assert not overlay_present(d), "no selection overlay before any selection"
        assert d.request("focus/set", {"tag": T}).result.get("focused") == T, "grid focused"

        # -- (A) select-cell then extend-cell -- the rectangle grows -------
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

        # -- (B) extend toward the top-left normalizes the bounds ----------
        assert d.invoke("/external/extend-cell", "0,0") is True, "extend toward top-left"
        wait_query(d, "/external/cell_selection", "0,0,1,1", desc="bounds normalize (anchor 1,1)")
        assert_eq(cell_count(d), 4, "2x2 rectangle")

        # -- (C) the overlay rectangle covers the selected cells -----------
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        rects = abs_rects_of(snap)
        assert CELLSEL_TAG in rects, "the overlay carries an absolute rect"
        ox, oy, ow, oh = rects[CELLSEL_TAG]
        # Same-snapshot geometry (deterministic): the overlay's box contains the
        # union of the corner cells (0,0) and (1,1). A few px of slack for the
        # 2px selection border the overlay draws around the cells.
        c00 = rects["grid#0_0"]
        c11 = rects["grid#1_1"]
        tol = 4
        assert ox <= c00[0] + tol, f"overlay left {ox} <= cell(0,0) left {c00[0]}"
        assert oy <= c00[1] + tol, f"overlay top {oy} <= cell(0,0) top {c00[1]}"
        assert ox + ow + tol >= c11[0] + c11[2], "overlay right edge reaches cell(1,1)"
        assert oy + oh + tol >= c11[1] + c11[3], "overlay bottom edge reaches cell(1,1)"

        # -- (D) a plain Arrow collapses to a single cell at the cursor ----
        # Cursor is at (0,0) after the last extend; ArrowRight -> (0,1), and a
        # plain move collapses the selection to that one cell.
        d.key(path=T, name="ArrowRight")
        wait_query(d, "/external/cell_selection", "0,1,0,1", desc="plain Arrow collapses to (0,1)")
        assert_eq(cell_count(d), 1, "collapsed to a single cell")

        # -- (E) a Shift+Arrow extends the rectangle from the anchor -------
        d.modifiers(shift=True)  # held-modifier cache; the next key press reads it
        d.key(path=T, name="ArrowDown")
        d.modifiers()  # release
        wait_query(d, "/external/cell_selection", "0,1,1,1", desc="Shift+Arrow extends to (1,1)")
        assert_eq(cell_count(d), 2, "the rectangle grew to two cells")

        # -- (F) Escape clears the selection, the cursor staying put -------
        d.key(path=T, name="Escape")
        wait_query(d, "/external/cell_selection", None, desc="Escape cleared the selection")
        assert_eq(cell_count(d), 0, "cleared selection counts zero")
        wait_until(lambda: not overlay_present(d), desc="the overlay is gone after Escape")
        assert_eq(q(d, "focused_row"), 1, "Escape kept the cursor row")
        assert_eq(q(d, "focused_col"), 1, "Escape kept the cursor col")

        # -- (G) Home / End are fresh single-cell selections within the row -
        d.key(path=T, name="End")
        wait_query(d, "/external/cell_selection", "1,3,1,3", desc="End selects the last column cell")
        assert_eq(q(d, "focused_col"), 3, "the cursor jumped to the last column")
        d.key(path=T, name="Home")
        wait_query(d, "/external/cell_selection", "1,0,1,0", desc="Home selects the first column cell")
        assert_eq(q(d, "focused_col"), 0, "the cursor jumped to the first column")

        # -- (H) clear, and out-of-range is a no-op ------------------------
        assert d.invoke("/external/clear-cell-selection", None) is True, "clear handled"
        wait_query(d, "/external/cell_selection", None, desc="selection cleared")
        assert_eq(cell_count(d), 0, "cleared selection counts zero")
        assert d.invoke("/external/select-cell", "9,9") is False, "out-of-range select is a no-op (false)"
        assert_eq(cell_sel(d), None, "out-of-range select changed nothing")


if __name__ == "__main__":
    sys.exit(run_demo("R953 §5.38 — dedicated cell range selection grid", body))
