#!/usr/bin/env python3
"""R891 §5.40 — column filter folded onto the EDITABLE data grid.

Drives `hello-data-grid` via JSON-RPC. The R886 grid gained the read-only
catalog's column-SORT axis; R891 folds the FILTER axis the same way, through
the cross-grid `GridFilter` / `grid_filter_str` wire vocabulary the read-only
`hello-grid-filter` / `hello-virtual-sort` already speak (the R874-class
documented-future-axis landing — the R837 docstring deferred the filter fold
pending the proxy substrate rounds, which have since landed).

The fold's payoff invariant (the spreadsheet / the toolkit sort filter proxy model): every grid
state is SOURCE-keyed — cursor, edit latch, cell addressing — and only the
paint / a11y row sequence + arrow navigation read the derived view order. So
committing an edit that flips a row out of an active filter drops the row on
the very next paint AND re-anchors the now-hidden source-keyed cursor onto the
visible row that takes its screen slot (never the silent navigation teleport
the sort fold left as a documented note). The filter match is by the cell's
TYPED value (`CellValue::matches_filter`, the value-not-label peer of
`sort_cmp`), so a bool filters by `true`/`false`, not the `On`/`Off` label.

Wire surface (all on /external = the grid coordinator), read/write symmetric:
  query  filter            -> "none" | "<col>=<value>"
  query  view_len          -> rows passing the filter (NROWS when unfiltered)
  intervene filter         -> decode = inverse of the query encoding; Null clears
  invoke set_filter <s>    -> filter (or Null to clear); returns the new view_len
  query  source_at.<pos>   -> source row painted at visual position <pos>

Scope (exact count = 34 checks — in-body assert_eq + wait gates; the
_focus_grid focus wait is setup, not a check):
  (A) boot: unfiltered, view_len = NROWS               [5]
  (B) set_filter shrinks the view, reports view_len     [6]
  (C) filter composes with sort (filter-then-sort)      [3]
  (D) clearing restores the full grid                   [3]
  (E) intervene filter round-trip read/write + Null     [4]
  (F) keyboard: filter-change re-anchors the cursor +
      arrow nav skips filtered rows                     [5]
  (G) edit-while-filtered: commit drops the row + the
      cursor re-anchors to its screen slot              [5]
  (H) status bar + painted rows track the view          [3]
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_query,
    wait_snap,
    wait_until,
)

VIEWPORT = (460, 348)

GRID = "data_grid"

# Type column (col 1) source values: sprite, mesh, sprite, mesh.
# Count column (col 2) source values: 1, 24, 99, 1.


def order(tf) -> list:
    return [tf.query(f"/external/source_at.{p}") for p in range(4)]


def _focus_grid(tf) -> None:
    tf.request("focus/set", {"tag": GRID})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == GRID,
        timeout=4.0,
        interval=0.03,
        desc="grid owns keyboard focus",
    )


def _walk(node, out) -> None:
    """Collect (tag, text) pairs across the scene tree."""
    if isinstance(node, dict):
        if node.get("type") == "Text" and isinstance(node.get("content"), str):
            out["texts"].append(node["content"])
        tag = node.get("tag")
        if isinstance(tag, str):
            out["tags"].append(tag)
        for v in node.values():
            _walk(v, out)
    elif isinstance(node, list):
        for v in node:
            _walk(v, out)


def _scan(snap) -> dict:
    out: dict = {"texts": [], "tags": []}
    _walk(snap, out)
    return out


def _row_tags(snap) -> list:
    return [t for t in _scan(snap)["tags"] if t.startswith("dg_row")]


def body() -> None:
    with RpcSubprocess("hello-data-grid", boot_grace=1.5) as tf:
        # ── (A) boot: unfiltered, full view ─────────────────────────
        assert_eq(tf.query("/external/filter"), "none", "boot: unfiltered")
        assert_eq(tf.query("/external/view_len"), 4, "boot: view_len = NROWS")
        assert_eq(order(tf), [0, 1, 2, 3], "boot: identity order")
        assert_eq(tf.query("/external/value.0.2"), 1, "Hero Count = 1")
        assert_eq(tf.query("/external/focused_row"), 0, "cursor at source 0")

        # ── (B) set_filter shrinks the view, reports view_len ───────
        assert_eq(tf.invoke("/external/set_filter", "1=mesh"), 2,
                  "set_filter returns the new view_len in one round-trip")
        wait_query(tf, "/external/view_len", 2, desc="two rows carry Type=mesh")
        assert_eq(tf.query("/external/filter"), "1=mesh", "filter readout")
        assert_eq(tf.query("/external/source_at.0"), 1, "Tree first")
        assert_eq(tf.query("/external/source_at.1"), 3, "Boss second")
        assert_eq(tf.query("/external/source_at.2"), None, "view shrank to 2")

        # ── (C) filter composes with sort (filter-then-sort) ────────
        tf.invoke("/external/cycle_sort", 2)  # Count ascending over survivors
        wait_query(tf, "/external/sort", "2:ascending", desc="sort the survivors")
        assert_eq(tf.query("/external/view_len"), 2, "filter survives the sort")
        assert_eq(tf.query("/external/source_at.0"), 3, "Boss (Count 1) sorts first")
        assert_eq(tf.query("/external/source_at.1"), 1, "Tree (Count 24) second")

        # ── (D) clearing restores the full grid ─────────────────────
        tf.request("scene/intervene", {"path": "/external/sort", "value": "none"})
        assert_eq(tf.invoke("/external/set_filter", None), 4,
                  "clearing the filter restores all rows")
        wait_query(tf, "/external/filter", "none", desc="filter cleared")
        assert_eq(order(tf), [0, 1, 2, 3], "full identity order again")

        # ── (E) intervene filter round-trip + Null clear ────────────
        tf.request("scene/intervene", {"path": "/external/filter", "value": "1=sprite"})
        wait_query(tf, "/external/filter", "1=sprite",
                   desc="intervene decode = inverse of query encode")
        assert_eq(tf.query("/external/view_len"), 2, "Hero + Coin are sprites")
        tf.request("scene/intervene", {"path": "/external/filter", "value": None})
        wait_query(tf, "/external/filter", "none", desc="Null clears the filter")

        # ── (F) keyboard: re-anchor on filter, arrow nav skips ──────
        _focus_grid(tf)
        tf.request("scene/intervene", {"path": "/external/focused_row", "value": 0})
        # Hero (row 0) is a sprite; Type=mesh excludes it, so the cursor
        # re-anchors to the visible row at its prior slot (Tree, source 1).
        tf.invoke("/external/set_filter", "1=mesh")
        wait_query(tf, "/external/focused_row", 1,
                   desc="filter re-anchors the hidden cursor to a visible row")
        # mesh rows = [Tree(1), Boss(3)]; ArrowDown walks only those.
        tf.key(path=GRID, name="ArrowDown")
        wait_query(tf, "/external/focused_row", 3, desc="ArrowDown skips filtered rows")
        tf.key(path=GRID, name="ArrowDown")
        wait_query(tf, "/external/focused_row", 3, desc="clamps at the last visible row")
        tf.invoke("/external/set_filter", None)

        # ── (G) edit-while-filtered: commit drops the row, cursor
        #        re-anchors to the row that takes its screen slot ─────
        tf.request("scene/intervene", {"path": "/external/sort", "value": "none"})
        # Count=1 keeps Hero(0) + Boss(3).
        assert_eq(tf.invoke("/external/set_filter", "2=1"), 2, "two rows have Count=1")
        tf.request("scene/intervene", {"path": "/external/focused_row", "value": 0})
        tf.request("scene/intervene", {"path": "/external/focused_col", "value": 2})
        tf.invoke("/external/begin", None)
        wait_query(tf, "/external/editing_row", 0, desc="editing source row 0 (Hero Count)")
        # Editor seeds with "1"; clear it then type 5 so Hero's Count leaves
        # the Count=1 facet.
        tf.key(path="data_grid_edit", name="Backspace")
        tf.key(path="data_grid_edit", name="5")
        tf.key(path="data_grid_edit", name="Enter")
        wait_query(tf, "/external/value.0.2", 5, desc="commit writes the source cell")
        assert_eq(tf.query("/external/view_len"), 1, "Hero dropped from the filtered view")
        assert_eq(tf.query("/external/source_at.0"), 3, "only Boss remains")
        assert_eq(tf.query("/external/focused_row"), 3,
                  "source-keyed cursor re-anchored from the filtered-out row to Boss")
        assert_eq(tf.query("/external/editing_row"), None, "edit latch closed")

        # ── (H) the status bar + painted rows track the view ────────
        tf.invoke("/external/set_filter", "1=mesh")
        snap = wait_snap(
            tf,
            lambda s: "filter 1=mesh · showing 2 of 4" in _scan(s)["texts"],
            viewport=VIEWPORT,
            desc="status bar reads the active filter + view size",
        )
        assert_eq(len(_row_tags(snap)), 2, "exactly two data rows painted under the filter")
        tf.invoke("/external/set_filter", None)
        snap = wait_snap(
            tf,
            lambda s: "filter none · showing 4 of 4" in _scan(s)["texts"],
            viewport=VIEWPORT,
            desc="cleared status restores the full count",
        )
        assert_eq(len(_row_tags(snap)), 4, "all four rows painted once cleared")


if __name__ == "__main__":
    sys.exit(run_demo("R891 §5.40 — editable data-grid column filter", body))
