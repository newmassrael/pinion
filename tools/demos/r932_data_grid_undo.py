#!/usr/bin/env python3
"""R932 §5.38 §5.52 — data-grid undo / redo (cell + row edits).

Drives hello-data-grid over JSON-RPC. R837-R931 made the editable data grid
authorable (typed cell edits, numeric scrub, dynamic add / remove rows) but
every edit was irreversible — the table-stakes a DCC / IDE grid cannot ship
without (the R930 / R931 carry: "data-grid has no undo stack"). R932 sits the
cell and row mutations on the SAME `UndoStack` substrate the node-graph editor
uses (a 2nd-widget consumer, not a new primitive): every discrete cell write
(RPC `value`, keyboard inline commit, bool toggle), a whole numeric scrub drag
(ONE step at release), and each `add_row` / `remove_row` records a reversible
granular command (the cell / row before-after, never a whole-model snapshot).
`Ctrl+Z` / `Ctrl+Shift+Z` (`Ctrl+Y`) drive it from the keyboard, the
`UndoStackExternal` at `/data_grid_undo/external` surfaces the history as data
for an AI agent, and a `dg_undo` status line paints the next undo / redo labels.

  (A) boot — the 4x5 seed table; the undo history is empty; the status reads
      "undo: none / redo: none".
  (B) cell edit (RPC value) — one undo step; undo restores the value, redo
      re-applies it; the status mirrors the label.
  (C) bool toggle — one undo step ("Toggle cell"); undo reverses it.
  (D) numeric scrub is ONE undo step for the whole drag (not one per frame).
  (E) add_row — undo drops the row, redo re-adds it.
  (F) remove_row — undo re-inserts the WHOLE row verbatim (every cell), cursor
      back; redo drops it again.
  (G) view-state (sort / filter) is NOT journaled — honest scope.
  (H) keyboard — Ctrl+Z undoes, Ctrl+Y / Ctrl+Shift+Z redo.
  (I) the history is queryable as data and `clear`able.

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r932_data_grid_undo.py
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
    wait_snap,
    wait_until,
)

EXAMPLE = "hello-data-grid"
GRID = "data_grid"
H_SCROLL = "data_grid_hscroll"
UNDO = "/data_grid_undo/external"
UNDO_STATUS = "dg_undo"
VIEWPORT = (460, 380)


def q(tf, path: str):
    return tf.query(f"/external/{path}")


def inv(tf, verb: str, arg=None):
    return tf.invoke(f"/external/{verb}", arg)


def uq(tf, slot: str):
    return tf.query(f"{UNDO}/{slot}")


def uinv(tf, verb: str):
    return tf.invoke(f"{UNDO}/{verb}", None)


def snap(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def status_text(tf) -> str:
    node = find_by_tag(snap(tf), UNDO_STATUS)
    assert node is not None, "dg_undo status node present"
    return node.get("content") or ""


def cell_on_screen(s, tag: str) -> bool:
    """The h-scroll clips a cell's on-screen rect, so a scrub press needs the
    rect to overlap the horizontal viewport (the r914 ZERO-FLAKE rule)."""
    node = find_by_tag(s, H_SCROLL)
    rects = abs_rects_of(s)
    if node is None or tag not in rects:
        return False
    vp = node.get("viewport") or {}
    vp_x, vp_w = int(vp.get("x", -1)), int(vp.get("w", -1))
    x, _, w, _ = rects[tag]
    return x < vp_x + vp_w and x + w > vp_x


def scrub(tf, row: int, col: int, dx: int) -> None:
    """Press the centre of cell `(row, col)` and drag the captured cursor `dx`
    logical px right (the deterministic `scene/drag` arc = press + moves +
    release through the capture lock, the r914 helper)."""
    x, y, w, h = abs_rects_of(snap(tf))[f"{GRID}#{row}_{col}"]
    cx, cy = x + w // 2, y + h // 2
    tf.drag(from_at=(float(cx), float(cy)), to_at=(float(cx + dx), float(cy)), steps=10)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — clean history, status mirror ──────────────────
        assert find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), GRID) is not None, "grid present"
        assert_eq(q(tf, "row_count"), 4, "4 seed rows")
        assert_eq(uq(tf, "can_undo"), False, "boot: nothing to undo")
        assert_eq(uq(tf, "can_redo"), False, "boot: nothing to redo")
        assert_eq(uq(tf, "count"), 0, "boot: empty history")
        assert_eq(uq(tf, "index"), 0, "boot: cursor at 0")
        assert_eq(uq(tf, "undo_label"), None, "boot: no undo label")
        wait_until(
            lambda: status_text(tf) == "undo: none · redo: none" or None,
            desc="boot status reads 'undo: none / redo: none'",
        )

        # ── (B) cell edit (RPC value) → undo → redo ─────────────────
        # Tree's Count (value.1.2) boots at 24; write 7.
        assert_eq(q(tf, "value.1.2"), 24, "Tree Count boots 24")
        tf.intervene("/external/value.1.2", 7)
        assert_eq(q(tf, "value.1.2"), 7, "the write landed")
        assert_eq(uq(tf, "can_undo"), True, "the edit is journaled")
        assert_eq(uq(tf, "count"), 1, "one history step")
        assert_eq(uq(tf, "undo_label"), "Edit cell", "the next undo is the cell edit")
        wait_until(
            lambda: status_text(tf) == "undo: Edit cell · redo: none" or None,
            desc="the status mirrors the pending undo label",
        )
        assert_eq(uinv(tf, "undo"), True, "undo stepped")
        assert_eq(q(tf, "value.1.2"), 24, "undo restored the original 24")
        assert_eq(uq(tf, "can_redo"), True, "the edit is now redoable")
        assert_eq(uinv(tf, "redo"), True, "redo stepped")
        assert_eq(q(tf, "value.1.2"), 7, "redo re-applied 7")
        assert_eq(uinv(tf, "clear"), 0, "clear the history for the next section")

        # ── (C) bool toggle is one undo step ────────────────────────
        # Hero's Active (value.0.4) boots true.
        assert_eq(q(tf, "value.0.4"), True, "Hero Active boots true")
        tf.intervene("/external/focused_row", 0)
        tf.intervene("/external/focused_col", 4)
        assert_eq(inv(tf, "toggle"), True, "toggle the focused bool")
        assert_eq(q(tf, "value.0.4"), False, "Active flipped to false")
        assert_eq(uq(tf, "undo_label"), "Toggle cell", "the undo is the toggle")
        assert_eq(uinv(tf, "undo"), True, "undo the toggle")
        assert_eq(q(tf, "value.0.4"), True, "toggle reversed")
        assert_eq(uinv(tf, "clear"), 0, "clear for the next section")

        # ── (D) a numeric scrub drag is ONE undo step ───────────────
        # Reveal the Count column (h-scroll clips it at boot), then drag its
        # row-0 cell right as one gesture (press + moves + release).
        tf.scroll(H_SCROLL, to=(1000, 0))  # clamps to max => Count/Scale on-screen
        wait_snap(tf, lambda s: cell_on_screen(s, f"{GRID}#0_2"),
                  viewport=VIEWPORT, desc="Count column scrolled on-screen")
        base = q(tf, "value.0.2")
        scrub(tf, 0, 2, 80)  # +80px well past the click dead zone
        scrubbed = q(tf, "value.0.2")
        assert scrubbed > base, f"the scrub raised Count from {base} to {scrubbed}"
        assert_eq(uq(tf, "count"), 1, "the whole drag is ONE undo step, not one per frame")
        assert_eq(uq(tf, "undo_label"), "Scrub cell", "the step is the scrub")
        assert_eq(uinv(tf, "undo"), True, "undo the scrub")
        assert_eq(q(tf, "value.0.2"), base, "undo restored the press value in one step")
        assert_eq(uinv(tf, "clear"), 0, "clear for the next section")
        tf.scroll(H_SCROLL, to=(0, 0))  # restore the horizontal scroll

        # ── (E) add_row → undo → redo ───────────────────────────────
        assert_eq(inv(tf, "add_row"), 4, "add_row returns the new index")
        assert_eq(q(tf, "row_count"), 5, "one row added")
        assert_eq(uq(tf, "undo_label"), "Add row", "the undo is the add")
        assert_eq(uinv(tf, "undo"), True, "undo the add")
        assert_eq(q(tf, "row_count"), 4, "undo dropped the row")
        assert_eq(uinv(tf, "redo"), True, "redo the add")
        assert_eq(q(tf, "row_count"), 5, "redo re-added the row")
        assert_eq(uinv(tf, "clear"), 0, "clear for the next section")

        # ── (F) remove_row → undo restores the WHOLE row verbatim ───
        # Source rows: 0 Hero, 1 Tree(Count 7 from B's redo), 2 Coin, 3 Boss, 4 (added).
        assert_eq(q(tf, "value.1.0"), "Tree", "row 1 is Tree")
        assert_eq(q(tf, "value.1.2"), 7, "Tree's Count is 7 (from the B edit)")
        tf.intervene("/external/focused_row", 1)
        assert_eq(inv(tf, "remove_row", 1), True, "remove source row 1 (Tree)")
        assert_eq(q(tf, "value.1.0"), "Coin", "Coin shifted into the freed slot")
        assert_eq(uq(tf, "undo_label"), "Remove row", "the undo is the remove")
        assert_eq(uinv(tf, "undo"), True, "undo the remove")
        assert_eq(q(tf, "value.1.0"), "Tree", "Tree's name re-inserted")
        assert_eq(q(tf, "value.1.2"), 7, "and its Count — the whole row, not just the name")
        assert_eq(q(tf, "focused_row"), 1, "cursor restored to the re-inserted row")
        assert_eq(uinv(tf, "clear"), 0, "clear for the next section")

        # ── (G) view-state is NOT journaled (honest scope) ──────────
        inv(tf, "cycle_sort", 2)  # sort by Count
        inv(tf, "set_filter", "4=true")  # filter Active==true
        assert_eq(uq(tf, "count"), 0, "sort / filter are view state, not undoable edits")
        inv(tf, "set_filter", None)  # clear the filter for the keyboard section
        inv(tf, "cycle_sort", 2)
        inv(tf, "cycle_sort", 2)  # back to unsorted

        # ── (H) keyboard Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z ─────────────
        tf.request("focus/set", {"tag": GRID})
        tf.intervene("/external/value.0.2", 42)
        assert_eq(q(tf, "value.0.2"), 42, "a fresh edit to drive the keyboard")
        # Ctrl+Z undoes.
        tf.modifiers(ctrl=True)
        tf.key(path=GRID, name="z")
        tf.modifiers()
        assert_eq(wait_until(lambda: True if q(tf, "value.0.2") == base else None,
                             desc="Ctrl+Z undid the edit"), True)
        # Ctrl+Y redoes.
        tf.modifiers(ctrl=True)
        tf.key(path=GRID, name="y")
        tf.modifiers()
        assert_eq(wait_until(lambda: True if q(tf, "value.0.2") == 42 else None,
                             desc="Ctrl+Y redid the edit"), True)
        # Ctrl+Z again, then Ctrl+Shift+Z (the redo twin).
        tf.modifiers(ctrl=True)
        tf.key(path=GRID, name="z")
        tf.modifiers()
        assert_eq(wait_until(lambda: True if q(tf, "value.0.2") == base else None,
                             desc="Ctrl+Z undid again"), True)
        tf.modifiers(ctrl=True, shift=True)
        tf.key(path=GRID, name="Z")
        tf.modifiers()
        assert_eq(wait_until(lambda: True if q(tf, "value.0.2") == 42 else None,
                             desc="Ctrl+Shift+Z redid the edit"), True)

        # ── (I) history as data + clear ─────────────────────────────
        assert uq(tf, "count") > 0, "the history holds commands"
        assert uq(tf, "index") > 0, "the cursor advanced"
        assert_eq(uinv(tf, "clear"), 0, "clear empties the history (cursor 0)")
        assert_eq(uq(tf, "can_undo"), False, "nothing to undo after clear")
        assert_eq(uq(tf, "can_redo"), False, "nothing to redo after clear")
        wait_until(
            lambda: status_text(tf) == "undo: none · redo: none" or None,
            desc="the status returns to 'none / none' after clear",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R932 §5.38 §5.52 — data-grid undo / redo", body))
