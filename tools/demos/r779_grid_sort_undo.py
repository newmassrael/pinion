#!/usr/bin/env python3
"""R779 §5.40 §5.52 — reversible data-grid sort (undo/redo) E2E.

Drives `hello-grid-sort` via JSON-RPC. R778 made the virtualized grid's column
sort drive a numeric-aware sort proxy; this round makes every sort change
**reversible** through the shared UndoStack — the grid analogue of the 1-D
list's R747 sort -> R748 undoable sort arc. The `GridSortExternal` records each
sort mutation (header click / `invoke cycle_sort` / `intervene sort`) as a
`GridSortEdit` (the third `UndoCommand` consumer, a bespoke peer of
`SortFilterEdit`); the `UndoStackExternal` surfaces the timeline.

  * primary `VirtualSelectExternal` at `vtbl` — source-indexed row selection;
  * extra `GridSortExternal` at `vsort` (with_undo) — sort proxy + recorder;
  * extra `UndoStackExternal` at `vtbl_undo_stack` — the undo/redo surface.

The witness (§2 #7 scene-as-data): the proxy `order` (source_at.*) and the
rendered window walk backward/forward in lockstep with undo/redo, and the
selection (held by source index) is untouched by sort undo (selection ⊥ sort).

  (A) boot — empty timeline, unsorted.
  (B) record — header click + invoke + intervene each push a labelled edit.
  (C) undo/redo — unwind the timeline one step at a time; the sort + the
      rendered order revert/re-apply; labels track the cursor.
  (D) selection ⊥ sort-undo — a selected source row is unaffected by undo.
  (E) clear — `invoke clear` empties the timeline.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-grid-sort"
WIN = (400, 480)
N = 10_000
GRID_TAG = "vtbl"
SORT_TAG = "vsort"
UNDO = "/vtbl_undo_stack/external"
PAUSE = 0.12


def grid_sort(d):
    return d.query("/vsort/external/sort")


def source_at(d, pos: int):
    return d.query(f"/vsort/external/source_at.{pos}")


def selected(d):
    return d.query("/external/selected")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot: empty timeline, unsorted ──────────────────────────
        assert GRID_TAG in abs_rects_of(snap), "grid present at boot"
        assert_eq(grid_sort(tf), "none", "boot is unsorted")
        assert_eq(tf.query(f"{UNDO}/count"), 0, "undo timeline empty at boot")
        assert_eq(tf.query(f"{UNDO}/can_undo"), False, "nothing to undo at boot")
        assert_eq(tf.query(f"{UNDO}/can_redo"), False, "nothing to redo at boot")
        assert_eq(source_at(tf, 0), 0, "unsorted: visual 0 == source 0")

        # ── (B) record: header click + invoke + intervene push edits ────
        tf.click(path="vsort#h0")  # Name ascending
        time.sleep(PAUSE)
        assert_eq(grid_sort(tf), "0:ascending", "header click sorted Name ascending")
        assert_eq(tf.query(f"{UNDO}/count"), 1, "header-click sort recorded one edit")
        assert_eq(tf.query(f"{UNDO}/can_undo"), True, "the sort is undoable")
        assert tf.query(f"{UNDO}/undo_label").startswith("Sort"), "the edit is labelled a Sort"
        asc0_source = source_at(tf, 0)  # Alpha source 0
        assert_eq(asc0_source, 0, "asc Name top is Alpha source 0")

        tf.invoke("/vsort/external/cycle_sort", 0)  # 0:descending
        assert_eq(grid_sort(tf), "0:descending", "invoke cycled to descending")
        tf.intervene("/vsort/external/sort", "1:ascending")  # admin restore
        assert_eq(grid_sort(tf), "1:ascending", "intervene set Score ascending")
        assert_eq(tf.query(f"{UNDO}/count"), 3, "three sort edits on the timeline")
        score0_source = source_at(tf, 0)  # numeric-min Score source

        # ── (C) undo / redo unwind the sort + the rendered order ────────
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the intervene")
        assert_eq(grid_sort(tf), "0:descending", "undo reverted to descending Name")
        assert_eq(tf.query(f"{UNDO}/can_redo"), True, "the undone edit can be redone")
        assert tf.query(f"{UNDO}/redo_label").startswith("Sort"), "redo step is labelled"

        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the cycle")
        assert_eq(grid_sort(tf), "0:ascending", "undo reverted to ascending Name")
        assert_eq(source_at(tf, 0), asc0_source, "the rendered order walked back with the sort")
        snap = tf.snapshot(source="paint", viewport=WIN)
        top_strip = f"{GRID_TAG}_row{asc0_source}"
        assert top_strip in abs_rects_of(snap), "the reverted order's top strip is rendered"

        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the header click")
        assert_eq(grid_sort(tf), "none", "fully unwound → unsorted")
        assert_eq(source_at(tf, 0), 0, "unsorted order restored (visual 0 == source 0)")
        assert_eq(tf.query(f"{UNDO}/can_undo"), False, "timeline fully unwound")

        # redo forward re-applies each edit in order.
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo the header click")
        assert_eq(grid_sort(tf), "0:ascending", "redo re-applied ascending Name")
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo the cycle")
        assert_eq(grid_sort(tf), "0:descending", "redo re-applied descending")
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo the intervene")
        assert_eq(grid_sort(tf), "1:ascending", "redo re-applied Score ascending")
        assert_eq(source_at(tf, 0), score0_source, "rendered order walked forward too")

        # ── (D) selection ⊥ sort-undo ───────────────────────────────────
        tf.intervene("/vsort/external/sort", "none")  # back to identity for a stable click
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert "vtbl#2_0" in abs_rects_of(snap), "cell vtbl#2_0 visible for the click test"
        tf.click(path="vtbl#2_0")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 2, "selected source row 2")
        tf.click(path="vsort#h0")  # sort, recorded
        time.sleep(PAUSE)
        assert_eq(selected(tf), 2, "selection survives the sort")
        tf.invoke(f"{UNDO}/undo", None)  # undo the sort
        assert_eq(selected(tf), 2, "selection is untouched by sort-undo (selection ⊥ sort)")

        # ── (E) clear empties the timeline ──────────────────────────────
        assert tf.query(f"{UNDO}/count") > 0, "timeline non-empty before clear"
        tf.invoke(f"{UNDO}/clear", None)
        assert_eq(tf.query(f"{UNDO}/count"), 0, "clear emptied the undo timeline")
        assert_eq(tf.query(f"{UNDO}/can_undo"), False, "nothing to undo after clear")


if __name__ == "__main__":
    sys.exit(run_demo("R779 §5.40 §5.52 — reversible data-grid sort", body))
