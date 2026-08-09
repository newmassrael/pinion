#!/usr/bin/env python3
"""R886 §5.40 — column sort folded onto the EDITABLE data grid.

Drives `hello-data-grid` via JSON-RPC. The R837 grid gains the read-only
catalog's column-sort axis through the same `cycle_col_sort` /
`grid_order_by` / `cell_cmp` SSOT and the cross-grid `grid_sort_str` wire
vocabulary — the R874-class documented-future-axis landing (the R837
docstring deferred the fold pending the proxy substrate rounds, which have
since landed).

The fold's payoff invariant (the spreadsheet / the toolkit sort filter proxy model): every grid
state is SOURCE-keyed — cursor, edit latch, cell addressing — and only the
paint / a11y row sequence + arrow navigation read the derived visual order.
So committing an edit that changes the active sort key moves the row on the
very next paint while the cursor follows the source row.

Wire surface (all on /external = the grid coordinator):
  query  sort               -> "none" | "<col>:ascending" | "<col>:descending"
  query  source_at.<pos>    -> source row painted at visual position <pos>
  intervene sort            -> decode = inverse of the query encoding
  invoke cycle_sort <col>   -> the header-click RPC shortcut
  send "h<col>:PointerUp"   -> the composite header-click wire form

Scope (exact count = 33 checks — in-body assert_eq + wait gates; the
_focus_grid focus wait is setup, not a check):
  (A) boot: unsorted, identity order            [5]
  (B) header CLICK cycles asc -> desc -> none   [8]
  (C) arrow nav walks the visual order          [5]
  (D) edit-while-sorted: commit re-sorts live,
      cursor + value follow the source row      [6]
  (E) intervene sort round-trip + clamp         [3]
  (F) sorted header paints the direction glyph  [3]
  (G) cycle_sort invoke returns the new key     [3]
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

VIEWPORT = (460, 320)

GRID = "data_grid"

# Count-column (col 2) source values: 1, 24, 99, 1 -> stable ascending
# order [0, 3, 1, 2], descending [2, 1, 0, 3].
ASC = [0, 3, 1, 2]
DESC = [2, 1, 0, 3]


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


def _walk_texts(node, out) -> None:
    if isinstance(node, dict):
        if node.get("type") == "Text" and isinstance(node.get("content"), str):
            out.append(node["content"])
        for v in node.values():
            _walk_texts(v, out)
    elif isinstance(node, list):
        for v in node:
            _walk_texts(v, out)


def body() -> None:
    with RpcSubprocess("hello-data-grid", boot_grace=1.5) as tf:
        # ── (A) boot: unsorted identity ─────────────────────────────
        assert_eq(tf.query("/external/sort"), "none", "boot: unsorted")
        assert_eq(order(tf), [0, 1, 2, 3], "boot: identity order")
        assert_eq(tf.query("/external/value.0.2"), 1, "Hero Count = 1")
        assert_eq(tf.query("/external/value.2.2"), 99, "Coin Count = 99")
        assert_eq(tf.query("/external/focused_row"), 0, "cursor at source 0")

        # ── (B) clicking the Count header cycles the sort ───────────
        tf.click(path=f"{GRID}#h2")
        wait_query(tf, "/external/sort", "2:ascending",
                   desc="header click sorts ascending")
        assert_eq(order(tf), ASC, "stable ascending permutation")
        tf.click(path=f"{GRID}#h2")
        wait_query(tf, "/external/sort", "2:descending",
                   desc="second click flips to descending")
        assert_eq(order(tf), DESC, "descending permutation")
        tf.click(path=f"{GRID}#h2")
        wait_query(tf, "/external/sort", "none",
                   desc="third click returns to source order")
        assert_eq(order(tf), [0, 1, 2, 3], "unsorted = identity again")
        # A different column jumps straight to ascending.
        tf.click(path=f"{GRID}#h0")
        wait_query(tf, "/external/sort", "0:ascending",
                   desc="different header jumps to ascending")
        assert_eq(tf.query("/external/source_at.0"), 3,
                  "Asset ascending: Boss first")
        tf.request("scene/intervene",
                   {"path": "/external/sort", "value": "none"})

        # ── (C) arrow navigation walks the VISUAL order ─────────────
        _focus_grid(tf)
        tf.invoke("/external/cycle_sort", 2)
        wait_query(tf, "/external/sort", "2:ascending", desc="sorted for nav")
        tf.request("scene/intervene", {"path": "/external/focused_row", "value": 0})
        tf.key(path=GRID, name="ArrowDown")
        wait_query(tf, "/external/focused_row", 3,
                   desc="ArrowDown lands on source 3 (visual position 1)")
        tf.key(path=GRID, name="ArrowDown")
        wait_query(tf, "/external/focused_row", 1,
                   desc="next visual row is source 1")
        tf.key(path=GRID, name="ArrowUp")
        wait_query(tf, "/external/focused_row", 3, desc="ArrowUp steps back")
        tf.key(path=GRID, name="ArrowUp")
        wait_query(tf, "/external/focused_row", 0, desc="back at the top")

        # ── (D) edit-while-sorted: live re-sort, cursor follows ─────
        tf.request("scene/intervene", {"path": "/external/focused_col", "value": 2})
        tf.invoke("/external/begin", None)
        wait_query(tf, "/external/editing_row", 0, desc="editing source row 0")
        # The editor seeds with the current value ("1"); clear it first so
        # the typed digits replace rather than append.
        tf.key(path="data_grid_edit", name="Backspace")
        for ch in "500":
            tf.key(path="data_grid_edit", name=ch)
        tf.key(path="data_grid_edit", name="Enter")
        wait_query(tf, "/external/value.0.2", 500,
                   desc="commit writes the source cell")
        assert_eq(tf.query("/external/source_at.3"), 0,
                  "edited row re-sorted to the visual bottom")
        assert_eq(order(tf), [3, 1, 2, 0], "full re-sorted permutation")
        assert_eq(tf.query("/external/focused_row"), 0,
                  "source-keyed cursor follows the moved row")
        assert_eq(tf.query("/external/editing_row"), None, "edit latch closed")

        # ── (E) intervene round-trip + out-of-range clamp ───────────
        tf.request("scene/intervene",
                   {"path": "/external/sort", "value": "3:descending"})
        wait_query(tf, "/external/sort", "3:descending",
                   desc="intervene decode = inverse of query encode")
        assert_eq(tf.query("/external/source_at.0"), 3,
                  "Scale descending: Boss (4.0) first")
        tf.request("scene/intervene",
                   {"path": "/external/sort", "value": "9:ascending"})
        wait_query(tf, "/external/sort", "none",
                   desc="out-of-range column clamps to unsorted")

        # ── (F) the sorted header paints the direction glyph ────────
        # R1548.1 — asked of the header CELL, not of a concatenated string.
        # Until R1547.1 this binding painted `"{label}{glyph}"` as ONE text
        # node, which is what made `startswith("Count") and "▲" in t` a legal
        # reading; R1547.1 split them, because a label is the header's content
        # (and its accessible name) while the sort glyph is presentational, and
        # the joined node announced "Asset ▲" to a screen reader. The
        # cell-scoped question is the one that survives the split — and it says
        # something the string never did: that the glyph is inside the Count
        # column's OWN cell, rather than anywhere in a string that happens to
        # start with "Count".
        tf.invoke("/external/cycle_sort", 2)
        snap = wait_snap(
            tf,
            lambda s: {"Count", "▲"} <= _header_cell_texts(s, 2),
            viewport=VIEWPORT,
            desc="ascending glyph lands on the Count header",
        )
        texts = _snap_texts(snap)
        assert_eq(sum(1 for t in texts if "▲" in t), 1,
                  "exactly one header carries the glyph")
        assert_eq(any("▼" in t for t in texts), False,
                  "no descending glyph while ascending")

        # ── (G) cycle_sort returns the new key (1-RT outcome) ───────
        r = tf.invoke("/external/cycle_sort", 2)
        assert_eq(r, "2:descending", "invoke returns the post-cycle key")
        r = tf.invoke("/external/cycle_sort", 2)
        assert_eq(r, "none", "third cycle returns unsorted")
        # R886.1 — out-of-range col is the family's silent no-op (the
        # GridSortExternal contract), returning the unchanged key.
        tf.invoke("/external/cycle_sort", 1)
        assert_eq(tf.invoke("/external/cycle_sort", 9), "1:ascending",
                  "out-of-range cycle_sort is a no-op returning the key")


def _snap_texts(snap) -> list:
    out: list = []
    _walk_texts(snap, out)
    return out


def _header_cell_texts(snap, col: int) -> set:
    """R1548.1 — every text painted inside column `col`'s header cell.

    The cell is the composite `"{GRID}#h{col}"` container the sort click
    routes through, so this addresses the header by the tag the framework
    publishes rather than by a position in a flattened text list. Stripped,
    because the glyph node is padded (`" ▲"`) and the padding is a paint
    detail, not part of what the header says.
    """
    found: list = []
    _walk_node(snap, f"{GRID}#h{col}", found)
    out: set = set()
    for cell in found:
        texts: list = []
        _walk_texts(cell, texts)
        out.update(t.strip() for t in texts)
    return out


def _walk_node(node, tag: str, out: list) -> None:
    if isinstance(node, dict):
        if node.get("tag") == tag:
            out.append(node)
        for v in node.values():
            _walk_node(v, tag, out)
    elif isinstance(node, list):
        for v in node:
            _walk_node(v, tag, out)


if __name__ == "__main__":
    sys.exit(run_demo("R886 §5.40 — editable data-grid column sort", body))
