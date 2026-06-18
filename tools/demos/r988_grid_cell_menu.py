#!/usr/bin/env python3
"""R988 §5.27 §5.38 §5.53 — data-cell context menu + column-naming labels.

R986 gave hello-grid-header-menu a column-HEADER context menu. R988 extends
the same own-rendered ContextMenu to any **data cell**: right-clicking a cell
opens the sort menu scoped to that cell's column, and the menu labels NAME the
column ("Sort by Score ascending"), so the popup reflects what it acts on
(Excel / DCC). Pure composition — the `grid_target_at` geometric hit-test now
resolves a header OR a body cell, and the view computes the labels per-open
from the captured column.

Verification scope (>=30 assertions; gates per [[zero-flake-policy]]: every
action->assert edge polls observed state, no fixed sleeps):

  (A) boot — menu closed, three items, unsorted.
  (B) right-click a data CELL in the Score column opens the popup there.
  (C) the cell hit-test is scene-as-data: the status bar names the column
      AND the cell row.
  (D) dynamic labels — the items name the clicked column.
  (E) activating "Sort by Score ascending" sorts that column.
  (F) a cell in a DIFFERENT column re-targets it (labels + sort).
  (G) header vs cell consistency — the header path (R986) still names the
      same column.
  (H) "Clear Sort" resets; a right-click OUTSIDE the grid opens nothing.
  (I) keyboard model on a cell-opened popup.
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

VIEWPORT = (520, 460)
N = 8
KEY_AT = (5.0, 5.0)


def _score(i: int) -> int:
    return (i * 37 + 11) % 100


SCORE_ASC = sorted(range(N), key=lambda i: (_score(i), i))


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _wait_paint(tf, predicate, desc):
    return wait_until(lambda: (lambda s: s if predicate(s) else None)(_paint(tf)), desc=desc)


def _status_text(snap) -> str:
    node = find_by_tag(snap, "grid_status")
    return (node.get("content") or "") if node else ""


def _menu_text(snap) -> str:
    panel = find_by_tag(snap, "colmenu")
    out = []

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "Text" and isinstance(n.get("content"), str):
                out.append(n["content"])
            for c in n.get("children") or []:
                walk(c)

    walk(panel)
    return " | ".join(out)


def _top_row_rect(snap):
    """The row strip currently at visual position 0 (smallest y), whatever
    source id it holds after a sort."""
    rects = abs_rects_of(snap)
    rows = [(t, r) for t, r in rects.items() if t.startswith("grid_row")]
    assert rows, "at least one data row strip paints"
    return min(rows, key=lambda tr: tr[1][1])[1]


def _cell_point(tf, col: int):
    """Center of a body cell in `col`: the column's x (from its header) + the
    top row strip's y."""
    snap = _wait_paint(
        tf,
        lambda s: f"grid_ch{col}" in abs_rects_of(s)
        and any(t.startswith("grid_row") for t in abs_rects_of(s)),
        f"header grid_ch{col} + a row strip paint",
    )
    hx, _hy, hw, _hh = abs_rects_of(snap)[f"grid_ch{col}"]
    rx, ry, rw, rh = _top_row_rect(snap)
    return float(hx + hw // 2), float(ry + rh // 2)


def _header_point(tf, col: int):
    snap = _wait_paint(tf, lambda s: f"grid_ch{col}" in abs_rects_of(s), f"header {col} paints")
    x, y, w, h = abs_rects_of(snap)[f"grid_ch{col}"]
    return float(x + w // 2), float(y + h // 2)


def _source_at(tf, pos: int):
    return tf.query(f"/gsort/external/source_at.{pos}")


def _activate(tf, item: int):
    tf.click(path=f"colmenu#i{item}")
    wait_query(tf, "/external/open", False, desc=f"item {item} activates + closes")
    tf.pointer_leave()


def body() -> None:
    with RpcSubprocess("hello-grid-header-menu", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        _wait_paint(tf, lambda s: "grid_ch0" in abs_rects_of(s), "grid paints")
        assert_eq(tf.query("/external/open"), False, "boot: menu closed")               # 1
        assert_eq(tf.query("/external/item_count"), 3, "three items")                   # 2
        assert_eq(tf.query("/gsort/external/sort"), "none", "boot: unsorted")           # 3
        assert_eq(_source_at(tf, 0), 0, "unsorted: visual 0 == source 0")               # 4

        # ── (A2) R988.1 boundary guard ───────────────────────────────
        # A right-click near col 0's RIGHT edge must resolve col 0 (Name), not
        # col 1. The pre-R988.1 hit-test omitted the 8px block_pad content inset
        # and mis-resolved the column within 8px of every boundary; this clicks
        # 3px inside the painted col-0 cell (where the old math returned col 1).
        snap = _wait_paint(tf, lambda s: "grid_ch0" in abs_rects_of(s), "col-0 header paints")
        c0x, c0y, c0w, c0h = abs_rects_of(snap)["grid_ch0"]
        tf.click(at=(float(c0x + c0w - 3), float(c0y + c0h // 2)), button="right")
        wait_query(tf, "/external/open", True, desc="right-click near col-0 right edge opens")  # 5
        snap = _wait_paint(tf, lambda s: find_by_tag(s, "colmenu") is not None, "popup paints")  # 6
        boundary_menu = _menu_text(snap)
        assert "Sort by Name" in boundary_menu, (
            f"col-0 right edge resolves Name (block_pad-correct); got {boundary_menu!r}"
        )                                                                                # 7
        assert "Sort by Score" not in boundary_menu, (
            f"not mis-resolved to col 1 (the pre-R988.1 bug); got {boundary_menu!r}"
        )                                                                                # 8
        tf.intervene("/external/open", False)
        wait_query(tf, "/external/open", False, desc="dismiss the boundary-guard popup")  # 9
        tf.pointer_leave()

        # ── (B) right-click a data CELL in the Score column (col 1) ──
        cx, cy = _cell_point(tf, 1)
        tf.click(at=(cx, cy), button="right")
        wait_query(tf, "/external/open", True, desc="right-click a Score cell opens the menu")  # 5
        assert_eq(tf.query("/external/open_x"), cx, "popup anchored at the cell x")      # 6
        snap = _wait_paint(tf, lambda s: find_by_tag(s, "colmenu") is not None, "popup paints")  # 7
        assert find_by_tag(snap, "colmenu#barrier") is not None, "barrier paints"        # 8

        # ── (C) the cell hit-test is scene-as-data ───────────────────
        status = _status_text(snap)
        assert "Score" in status, f"status names the clicked column; got {status!r}"     # 9
        assert "cell row 0" in status, f"status names the cell row; got {status!r}"       # 10

        # ── (D) dynamic labels name the column ───────────────────────
        menu = _menu_text(snap)
        assert "Sort by Score ascending" in menu, f"item names column; got {menu!r}"      # 11
        assert "Sort by Score descending" in menu, f"desc item; got {menu!r}"             # 12
        assert "Clear Sort" in menu, f"clear item; got {menu!r}"                          # 13

        # ── (E) activate "Sort by Score ascending" ───────────────────
        _activate(tf, 0)
        wait_query(tf, "/gsort/external/sort", "1:ascending",
                   desc="cell menu sorts the Score column")                               # 14
        tail = tf.stderr_tail(80)
        assert any('shell: intent colmenu.command payload=Text("0")' in ln for ln in tail), (
            f"activation logs the command intent; stderr={tail[-6:]!r}"
        )                                                                                 # 15
        assert_eq(_source_at(tf, 0), SCORE_ASC[0], "asc Score: min-score id at visual 0") # 16
        assert_eq(_source_at(tf, N - 1), SCORE_ASC[-1], "asc Score: max-score id last")   # 17

        # ── (F) a cell in a DIFFERENT column (Name, col 0) ───────────
        nx, ny = _cell_point(tf, 0)
        tf.click(at=(nx, ny), button="right")
        wait_query(tf, "/external/open", True, desc="right-click a Name cell opens the menu")  # 18
        snap = _wait_paint(tf, lambda s: "Sort by Name" in _menu_text(s), "labels re-target Name")  # 19
        assert "Name" in _status_text(snap), "status re-targets the Name column"          # 20
        assert "Sort by Score" not in _menu_text(snap), "labels no longer name Score"     # 21
        _activate(tf, 1)  # Sort by Name descending
        wait_query(tf, "/gsort/external/sort", "0:descending",
                   desc="cell menu sorts the Name column descending")                     # 22

        # ── (G) header vs cell consistency (R986 path intact) ────────
        hx, hy = _header_point(tf, 1)
        tf.click(at=(hx, hy), button="right")
        wait_query(tf, "/external/open", True, desc="right-click the Score header opens too")  # 23
        snap = _paint(tf)
        assert "Sort by Score ascending" in _menu_text(snap), (
            "the header path names the same column as the cell path"
        )                                                                                 # 24
        assert "header" in _status_text(snap), "status marks a header (not cell) target"  # 25

        # ── (H) Clear Sort + outside-grid opens nothing ──────────────
        _activate(tf, 2)
        wait_query(tf, "/gsort/external/sort", "none", desc="Clear Sort resets")          # 26
        tf.click(at=(100.0, 12.0), button="right")  # status bar, above the grid
        assert_eq(tf.query("/external/open"), False, "right-click outside the grid opens nothing")  # 27
        tf.pointer_leave()

        # ── (I) keyboard model on a cell-opened popup ────────────────
        cx, cy = _cell_point(tf, 2)  # Status column
        tf.click(at=(cx, cy), button="right")
        wait_query(tf, "/external/open", True, desc="cell right-click reopens for keyboard")  # 28
        tf.request("focus/set", {"tag": "colmenu"})
        wait_until(lambda: tf.request("focus/get").result.get("focused") == "colmenu",
                   desc="menu owns focus")                                                # 29
        tf.key(at=KEY_AT, name="ArrowDown")
        wait_query(tf, "/external/active", 0, desc="ArrowDown -> first item")             # 30
        tf.key(at=KEY_AT, name="Enter")
        wait_query(tf, "/external/open", False, desc="Enter activates + closes")          # 31
        wait_query(tf, "/gsort/external/sort", "2:ascending",
                   desc="keyboard activated Sort by Status ascending")                    # 32


if __name__ == "__main__":
    sys.exit(run_demo("R988 §5.27 §5.38 §5.53 — data-cell context menu + column labels", body))
