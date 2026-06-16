#!/usr/bin/env python3
"""R954 §5.38 — pointer cell-range selection (SelectItems click / Shift+click).

R953 stood up `hello-cell-select` as a SelectItems grid driven by keyboard + RPC,
but a real left-click still routed to the Table coordinator's row-oriented `send`
path (the `SelectRows` model) — so a click did an invisible row-wash and did NOT
update the cell rectangle (the R953-documented smell + deferred pointer axis).

R954 adds a `SelectionBehavior::SelectItems` mode to the coordinator: in that
mode the activate edge (PointerUp) of a click selects the clicked *cell* (a plain
click collapses the rectangle to it, a `Shift`+click extends it from the pinned
anchor — the held modifier rides the composite `send` wire's third segment), and
no row is ever washed. `hello-cell-select` constructs with this behavior, so the
mouse and the keyboard now drive the *same* cell-range model.

An AI agent drives + reads it all over the §5.12 RPC plane (§2 #2):

  (A) a plain click selects the clicked cell and washes NO row (the headline —
      the R953 SelectRows-on-click smell is gone); the overlay appears.
  (B) clicking another cell collapses the selection to it.
  (C) a Shift+click extends the rectangle from the anchor; the count grows.
  (D) a second Shift+click re-extends (anchor stays pinned).
  (E) a plain click collapses again — and still no row is selected.
  (F) the overlay rectangle covers the selected cell (same-snapshot geometry).
  (G) pointer and keyboard share one model: a plain Arrow after a click collapses
      to the new cursor cell.

Run from the workspace root:
    cargo build -p hello-cell-select --release
    python3 tools/demos/r954_cell_pointer_select.py
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


def cell_tag(row: int, col: int) -> str:
    return f"{T}#{row}_{col}"


def click(d: RpcSubprocess, row: int, col: int, *, shift: bool = False) -> None:
    """A real left-click on cell (row, col); the held Shift rides the wire."""
    if shift:
        d.modifiers(shift=True)
    d.click(path=cell_tag(row, col))
    if shift:
        d.modifiers()  # release the held modifier (mirror the key-up)


def body() -> None:
    with RpcSubprocess("hello-cell-select") as d:
        # -- boot: a SelectItems grid with nothing selected ----------------
        assert_eq(q(d, "rows"), 6, "6 data rows")
        assert_eq(q(d, "cols"), 4, "4 columns")
        assert_eq(cell_sel(d), None, "no cell selection at boot")
        assert_eq(q(d, "selected_row"), -1, "no row selected at boot")
        assert not overlay_present(d), "no overlay before any selection"

        # -- (A) a plain click selects the cell and washes NO row ----------
        click(d, 1, 1)
        wait_query(d, "/external/cell_selection", "1,1,1,1", desc="click selects cell (1,1)")
        assert_eq(cell_count(d), 1, "one cell selected")
        assert_eq(q(d, "focused_row"), 1, "the cursor followed the click (row)")
        assert_eq(q(d, "focused_col"), 1, "the cursor followed the click (col)")
        assert_eq(q(d, "selected_row"), -1, "NO row washed — the R953 smell is gone")
        wait_until(lambda: overlay_present(d), desc="the selection overlay is painted")

        # -- (B) clicking another cell collapses the selection to it -------
        click(d, 3, 2)
        wait_query(d, "/external/cell_selection", "3,2,3,2", desc="click collapses to (3,2)")
        assert_eq(cell_count(d), 1, "collapsed to a single cell")
        assert_eq(q(d, "selected_row"), -1, "still no row washed")

        # -- (C) a Shift+click extends the rectangle from the anchor -------
        click(d, 1, 0, shift=True)
        wait_query(d, "/external/cell_selection", "1,0,3,2", desc="Shift+click extends from (3,2)")
        assert_eq(cell_count(d), 9, "3 rows x 3 cols = 9 cells")
        assert_eq(q(d, "focused_row"), 1, "the cursor followed the Shift+click (row)")
        assert_eq(q(d, "focused_col"), 0, "the cursor followed the Shift+click (col)")
        assert_eq(q(d, "selected_row"), -1, "Shift+click washes no row either")

        # -- (D) a second Shift+click re-extends (anchor (3,2) pinned) -----
        click(d, 0, 3, shift=True)
        wait_query(d, "/external/cell_selection", "0,2,3,3", desc="re-extend, anchor pinned at (3,2)")
        assert_eq(cell_count(d), 8, "4 rows x 2 cols = 8 cells")

        # -- (E) a plain click collapses again -----------------------------
        click(d, 2, 2)
        wait_query(d, "/external/cell_selection", "2,2,2,2", desc="plain click collapses to (2,2)")
        assert_eq(cell_count(d), 1, "collapsed to one cell")
        assert_eq(q(d, "selected_row"), -1, "no row washed across the whole sequence")

        # -- (F) the overlay rectangle covers the selected cell ------------
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        rects = abs_rects_of(snap)
        assert CELLSEL_TAG in rects, "the overlay carries an absolute rect"
        ox, oy, ow, oh = rects[CELLSEL_TAG]
        c22 = rects[cell_tag(2, 2)]
        tol = 4  # a few px of slack for the 2px selection border
        assert ox <= c22[0] + tol, f"overlay left {ox} <= cell(2,2) left {c22[0]}"
        assert oy <= c22[1] + tol, f"overlay top {oy} <= cell(2,2) top {c22[1]}"
        assert ox + ow + tol >= c22[0] + c22[2], "overlay right edge reaches cell(2,2)"
        assert oy + oh + tol >= c22[1] + c22[3], "overlay bottom edge reaches cell(2,2)"

        # -- (G) pointer + keyboard share one model ------------------------
        # Give the grid focus, then a plain Arrow collapses to the new cursor.
        assert d.request("focus/set", {"tag": T}).result.get("focused") == T, "grid focused"
        d.key(path=T, name="ArrowRight")
        wait_query(d, "/external/cell_selection", "2,3,2,3", desc="Arrow after click collapses to (2,3)")
        assert_eq(cell_count(d), 1, "one cell after the keyboard collapse")
        assert_eq(q(d, "selected_row"), -1, "keyboard collapse washes no row")


if __name__ == "__main__":
    sys.exit(run_demo("R954 §5.38 — pointer cell-range selection (SelectItems)", body))
