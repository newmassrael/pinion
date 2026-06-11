#!/usr/bin/env python3
"""R837 §5.38 §5.40 §5.50 — editable data grid.

The 2-D generalisation of the R836 property grid: a table where every cell is
editable in place by a type-appropriate control. Built as a pure composition
of a `DataGridExternal` coordinator (the flat typed cell model + 2-D roving
cursor + edit latch) plus ONE shared `TextFieldExternal` inline editor.

2nd consumer of the lifted `pinion_core::cell_value` SSOT (after
hello-property-grid) and 3rd consumer of the `grid_table_nodes` a11y SSOT.

Coordinator slots (`data_grid`, the primary external):
  /external/row_count          -> 4
  /external/col_count          -> 5
  /external/focused_row,_col   -> 2-D roving cursor
  /external/editing_row,_col   -> null | int (cell being text-edited)
  /external/col_name.<c>       -> column title
  /external/col_kind.<c>       -> "bool" | "int" | "float" | "text"
  /external/value.<r>.<c>      -> the typed cell value
  /external/toggle             -> invoke: flip the focused bool cell
  /external/begin              -> invoke: edit the focused cell
  /external/send               -> invoke: composite "<r>_<c>:<Event>" routing

Verified (>= 30 assertions):
  (A) boot taxonomy — 4x5 grid, column kinds, seed values
  (B) 2-D keyboard roving — arrows + Home/End move + clamp on both axes
  (C) bool toggle — Space flips the focused bool; single-click toggles too
  (D) int/float edit via keyboard — gate drops letters, Enter commits
  (E) Escape cancels — value untouched
  (F) programmatic typed set — intervene writes strictly per column kind
  (G) double-click enters edit mode on an editable cell
  (H) paint — cells render; the inline field only while editing
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
    wait_snap,
    wait_until,
)

VIEWPORT = (460, 320)

GRID = "data_grid"
EDIT = "data_grid_edit"
# R896 — the grid's horizontal scroll: the 570px columns outgrow the 370px
# viewport, so the trailing columns (Scale / Active) clip out at rest and the
# grid scrolls sideways to reveal them (the read-only grids' R784 wrap reused).
H_SCROLL = "data_grid_hscroll"


def _focus_grid(tf) -> None:
    tf.request("focus/set", {"tag": GRID})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == GRID,
        timeout=4.0,
        interval=0.03,
        desc="grid owns keyboard focus",
    )


def body() -> None:
    with RpcSubprocess("hello-data-grid", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, GRID) is not None, "grid present"
        assert_eq(tf.query("/external/row_count"), 4, "4 rows")
        assert_eq(tf.query("/external/col_count"), 5, "5 columns")
        assert_eq(tf.query("/external/focused_row"), 0, "cursor row 0")
        assert_eq(tf.query("/external/focused_col"), 0, "cursor col 0")
        assert_eq(tf.query("/external/editing_row"), None, "no cell editing")
        assert_eq(tf.query("/external/col_name.0"), "Asset", "col 0 name")
        assert_eq(tf.query("/external/col_name.2"), "Count", "col 2 name")
        assert_eq(tf.query("/external/col_kind.0"), "text", "Asset is text")
        assert_eq(tf.query("/external/col_kind.2"), "int", "Count is int")
        assert_eq(tf.query("/external/col_kind.3"), "float", "Scale is float")
        assert_eq(tf.query("/external/col_kind.4"), "bool", "Active is bool")
        assert_eq(tf.query("/external/value.0.0"), "Hero", "seed (0,0)")
        assert_eq(tf.query("/external/value.1.2"), 24, "seed (1,2) int")
        assert_eq(tf.query("/external/value.3.3"), 4.0, "seed (3,3) float")
        assert_eq(tf.query("/external/value.2.4"), False, "seed (2,4) bool")

        # ── (B) 2-D keyboard roving (clamped on both axes) ───────────
        _focus_grid(tf)
        tf.key(path=GRID, name="ArrowRight")
        tf.key(path=GRID, name="ArrowDown")
        wait_until(lambda: tf.query("/external/focused_col") == 1 and
                   tf.query("/external/focused_row") == 1, timeout=4.0, interval=0.03,
                   desc="Right+Down -> (1,1)")
        tf.key(path=GRID, name="End")
        wait_until(lambda: tf.query("/external/focused_col") == 4, timeout=4.0,
                   interval=0.03, desc="End -> last column")
        tf.key(path=GRID, name="ArrowRight")
        # Clamp no-op: the dispatch commits before the response.
        assert_eq(tf.query("/external/focused_col"), 4, "Right at last col clamps")
        tf.key(path=GRID, name="Home")
        wait_until(lambda: tf.query("/external/focused_col") == 0, timeout=4.0,
                   interval=0.03, desc="Home -> col 0")
        for _ in range(6):
            tf.key(path=GRID, name="ArrowDown")
        wait_until(lambda: tf.query("/external/focused_row") == 3, timeout=4.0,
                   interval=0.03, desc="ArrowDown clamps at last row")

        # ── (C) bool toggle: Space on focused, then single-click ─────
        tf.intervene("/external/focused_row", 2)
        tf.intervene("/external/focused_col", 4)  # Active bool of row 2 = false
        tf.key(path=GRID, name="Space")
        wait_until(lambda: tf.query("/external/value.2.4") is True, timeout=4.0,
                   interval=0.03, desc="Space toggles the focused bool")
        # Single-click the Active bool of row 0 (true) toggles it off. R896 —
        # the Active column is off-screen at rest, so scroll it into view
        # first (the cell tag is clipped out of the paint scene until then);
        # then scroll back so the rest of the demo runs at the default offset.
        tf.scroll(H_SCROLL, to=(1000, 0))  # clamps to max => reveals Active
        wait_snap(tf, lambda s: find_by_tag(s, f"{GRID}#0_4") is not None,
                  viewport=VIEWPORT, desc="Active column scrolled into view")
        tf.click(path=f"{GRID}#0_4")
        wait_until(lambda: tf.query("/external/value.0.4") is False, timeout=4.0,
                   interval=0.03, desc="single-click toggles a bool cell")
        assert_eq(tf.query("/external/focused_row"), 0, "click focuses the cell")
        assert_eq(tf.query("/external/focused_col"), 4, "click focuses the cell")
        tf.scroll(H_SCROLL, to=(0, 0))
        wait_snap(tf, lambda s: find_by_tag(s, f"{GRID}#0_0") is not None,
                  viewport=VIEWPORT, desc="scrolled back to the leading columns")

        # ── (D) int edit via keyboard (gate drops letters) ──────────
        _focus_grid(tf)
        tf.intervene("/external/focused_row", 1)
        tf.intervene("/external/focused_col", 2)  # Count int = 24
        tf.key(path=GRID, name="Enter")
        wait_until(lambda: tf.query("/external/editing_col") == 2, timeout=4.0,
                   interval=0.03, desc="Enter starts editing the int cell")
        wait_until(lambda: find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), EDIT)
                   is not None, timeout=4.0, interval=0.05, desc="inline field painted")
        wait_until(lambda: tf.request("focus/get").result.get("focused") == EDIT,
                   timeout=4.0, interval=0.03, desc="focus into the field")
        tf.key(path=EDIT, name="Backspace")
        tf.key(path=EDIT, name="Backspace")  # clear "24"
        tf.key(path=EDIT, name="3")
        tf.key(path=EDIT, name="z")  # gated out
        tf.key(path=EDIT, name="6")
        tf.key(path=EDIT, name="Enter")
        wait_until(lambda: tf.query("/external/value.1.2") == 36, timeout=4.0,
                   interval=0.03, desc="int commits 36 (the 'z' was dropped)")
        assert_eq(tf.query("/external/editing_row"), None, "edit mode cleared")

        # ── (E) Escape cancels — value untouched ────────────────────
        _focus_grid(tf)
        tf.intervene("/external/focused_row", 0)
        tf.intervene("/external/focused_col", 0)  # Asset text = "Hero"
        tf.key(path=GRID, name="F2")
        wait_until(lambda: tf.query("/external/editing_row") == 0, timeout=4.0,
                   interval=0.03, desc="F2 starts editing the text cell")
        tf.text("ZZZ", path=EDIT)
        tf.key(path=EDIT, name="Escape")
        wait_until(lambda: tf.query("/external/editing_row") is None, timeout=4.0,
                   interval=0.03, desc="Escape exits edit mode")
        assert_eq(tf.query("/external/value.0.0"), "Hero", "Escape leaves value untouched")

        # ── (F) programmatic typed set (the AI driving path) ─────────
        tf.intervene("/external/value.3.3", -2.5)  # Scale float
        assert_eq(tf.query("/external/value.3.3"), -2.5, "intervene sets a float")
        tf.intervene("/external/value.0.1", "actor")  # Type text
        assert_eq(tf.query("/external/value.0.1"), "actor", "intervene sets text")

        # ── (G) double-click enters edit mode ───────────────────────
        tf.double_click(path=f"{GRID}#2_2")
        wait_until(lambda: tf.query("/external/editing_row") == 2 and
                   tf.query("/external/editing_col") == 2, timeout=4.0, interval=0.03,
                   desc="double-click edits cell (2,2)")
        tf.key(path=EDIT, name="Escape")
        wait_until(lambda: tf.query("/external/editing_row") is None, timeout=4.0,
                   interval=0.03, desc="back to navigation")

        # ── (H) paint: cells render; field only while editing ───────
        # R896 — every cell stays in the paint scene tree (the rows are eager);
        # the h-scroll only clips the trailing columns' on-screen *rects* (so a
        # click on an off-screen cell needs a scroll first, as in section C).
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        for r in range(4):
            for c in range(5):
                assert find_by_tag(snap, f"{GRID}#{r}_{c}") is not None, f"cell ({r},{c}) painted"
        assert find_by_tag(snap, EDIT) is None, "no inline field when not editing"


if __name__ == "__main__":
    sys.exit(run_demo("hello-data-grid R837 §5.38 editable data grid", body))
