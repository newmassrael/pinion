#!/usr/bin/env python3
"""R937 §5.38 §5.52 — data-grid row drag-to-reorder.

Drives hello-data-grid over JSON-RPC. R930-R932 made the editable grid dynamic
(add / remove rows) + undoable, but the rows could not be MANUALLY reordered —
a core DCC interaction (reorder layers / list items). R937 adds it as the 3rd
consumer of the R742 drag-session hooks (after hello-dnd vertical / hello-tab-
reorder horizontal), driving the move through the SAME `GridUndoCtx` funnel every
mutator uses (so a reorder re-anchors the cursor + cancels the edit latch, R930.1):

  * a leading drag-handle column (a grip glyph) arms a row drag-to-reorder ONLY in
    the plain source view (no sort / filter / group), where the visual order IS the
    source order — so a manual position is unambiguous (Qt / Excel disable reorder
    under a sort proxy); the handle is then a blank spacer + `begin_drag` returns
    `None`;
  * the handle press and a numeric cell-scrub press arm mutually-exclusive
    gestures, disambiguated by the router (a drag session takes precedence over the
    capture lock), so a cell scrub still works untouched;
  * `move_row "from,to"` is the AI-first reorder primary (the move_elem peer); the
    drag is its pointer twin; Alt+Arrow is the keyboard twin — all three journal
    ONE `MoveRowEdit` (symmetric undo: from/to physical indices, redo / undo swap).

  (A) boot — the plain view: reorder enabled, grips painted, no drag in flight.
  (B) `move_row` (RPC) reorders the source model; the moved row follows the cursor;
      undo restores the order + cursor, redo re-applies (one symmetric step).
  (C) a pointer drag of a row's handle onto another row reorders it (the GUI twin);
      the drag preview clears at release.
  (D) a sort makes manual order meaningless: reorder is disabled (no grip, the gate
      reads false); clearing the sort re-enables it.

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r937_data_grid_reorder.py

>= 35 assertions.
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
)

EXAMPLE = "hello-data-grid"
GRID = "data_grid"
UNDO = "/data_grid_undo/external"
VIEWPORT = (460, 380)


def q(tf, path: str):
    return tf.query(f"/external/{path}")


def inv(tf, verb: str, arg):
    return tf.invoke(f"/external/{verb}", arg)


def painted(tf, tag: str) -> bool:
    return find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), tag) is not None


def drag_handle_onto(tf, src_row: int, dst_row: int, frac_y: float) -> None:
    """Press the grip handle of `src_row` and release over `dst_row` at vertical
    fraction `frac_y` (top half inserts before, bottom half after)."""
    rects = abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))
    sx, sy, sw, sh = rects[f"{GRID}#d{src_row}"]
    dx, dy, dw, dh = rects[f"{GRID}#d{dst_row}"]
    frm = (float(sx + sw // 2), float(sy + sh // 2))
    to = (float(dx + dw // 2), float(dy + int(dh * frac_y)))
    tf.drag(from_at=frm, to_at=to, steps=8)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — the plain reorder view ───────────────────────
        assert_eq(q(tf, "row_count"), 4, "4 seed rows")
        assert_eq(q(tf, "col_count"), 5, "5 data columns (the handle is not one)")
        assert_eq(q(tf, "reorder_enabled"), True, "the plain view enables reorder")
        assert_eq(q(tf, "drag_preview"), None, "no drag in flight at boot")
        # Boot source rows: 0 Hero, 1 Tree, 2 Coin, 3 Boss (Asset column).
        assert_eq(q(tf, "value.0.0"), "Hero", "boot row 0")
        assert_eq(q(tf, "value.1.0"), "Tree", "boot row 1")
        assert_eq(q(tf, "value.2.0"), "Coin", "boot row 2")
        assert_eq(q(tf, "value.3.0"), "Boss", "boot row 3")
        for r in range(4):
            assert painted(tf, f"{GRID}#d{r}"), f"row {r} grip handle painted"
        assert not painted(tf, "dg_drop_line"), "no drop line at rest"

        # ── (B) RPC move_row — the AI-first reorder primary ─────────
        assert_eq(inv(tf, "move_row", "0,2"), True, "move_row reorders the source")
        assert_eq(q(tf, "value.0.0"), "Tree", "rows shifted up")
        assert_eq(q(tf, "value.1.0"), "Coin", "rows shifted up")
        assert_eq(q(tf, "value.2.0"), "Hero", "Hero moved to index 2")
        assert_eq(q(tf, "value.3.0"), "Boss", "the tail is unchanged")
        assert_eq(q(tf, "focused_row"), 2, "the moved row follows the cursor")
        assert_eq(inv(tf, "move_row", "1,1"), False, "from == to is a no-op")
        assert_eq(inv(tf, "move_row", "0,9"), False, "out-of-range is a no-op")
        # One symmetric undo step: undo restores order + cursor, redo re-applies.
        tf.invoke(f"{UNDO}/undo", None)
        wait_query(tf, "/external/value.0.0", "Hero", desc="undo restored the boot order")
        assert_eq(q(tf, "value.2.0"), "Coin", "undo restored every row")
        tf.invoke(f"{UNDO}/redo", None)
        wait_query(tf, "/external/value.2.0", "Hero", desc="redo re-moved Hero to index 2")
        # Back to the boot order for the drag test.
        tf.invoke(f"{UNDO}/undo", None)
        wait_query(tf, "/external/value.0.0", "Hero", desc="back to the boot order")

        # ── (C) pointer drag — the GUI twin of move_row ─────────────
        # Press Hero's handle (row 0), release over row 2's bottom half (insert
        # after 2 → gap 3 → resting index 2): [Hero, Tree, Coin, Boss] -> [Tree,
        # Coin, Hero, Boss].
        drag_handle_onto(tf, 0, 2, 0.8)
        wait_query(tf, "/external/value.2.0", "Hero", desc="the drag moved Hero after row 2")
        assert_eq(q(tf, "value.0.0"), "Tree", "the drag reordered the source")
        assert_eq(q(tf, "value.1.0"), "Coin", "the drag reordered the source")
        assert_eq(q(tf, "value.3.0"), "Boss", "the tail stayed put")
        assert_eq(q(tf, "focused_row"), 2, "the dragged row follows the cursor")
        assert_eq(q(tf, "drag_preview"), None, "the drag preview clears at release")
        assert not painted(tf, "dg_drop_line"), "no drop line at rest after release"

        # ── (D) a sort disables reorder ─────────────────────────────
        inv(tf, "cycle_sort", 0)  # sort by the Asset column (ascending)
        assert_eq(q(tf, "reorder_enabled"), False, "a sort disables manual reorder")
        assert_eq(q(tf, "drag_preview"), None, "no drag possible under a sort")
        assert not painted(tf, f"{GRID}#d0"), "the grip handle is not painted under a sort"
        assert not painted(tf, f"{GRID}#d1"), "no grip on any row under a sort"
        # Clearing the sort (cycle asc -> desc -> unsorted) re-enables it.
        inv(tf, "cycle_sort", 0)
        inv(tf, "cycle_sort", 0)
        wait_query(tf, "/external/reorder_enabled", True, desc="clearing the sort re-enables reorder")
        assert painted(tf, f"{GRID}#d0"), "the grip handle returns when reorder re-enables"


if __name__ == "__main__":
    sys.exit(run_demo("R937 §5.38 §5.52 — data-grid row drag-to-reorder", body))
