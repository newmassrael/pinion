#!/usr/bin/env python3
"""R986 §5.27 §5.38 §5.53 — data-grid column-header context menu E2E.

Drives the new `hello-grid-header-menu` binding via JSON-RPC. A
right-click on a sortable column header opens a pinion own-rendered
`ContextMenu` (Sort Ascending / Sort Descending / Clear Sort) anchored at
the header; activating an item applies that sort to the **right-clicked**
column. Pure composition: R772 `ContextMenuExternal` (the primary, so the
WAI-ARIA keyboard model + R715 dismiss barrier come for free) + R887
secondary-click wire + R778 `GridSortExternal` (the extra sort proxy the
menu and a plain left-click both drive). No framework change.

The captured-target pattern (the toolkit `exec` / web `event.target`): the
binding records the right-clicked column in a reactive Signal at
`apply_secondary_click` time (geometric header hit-test) and reads it back
in the `update` reducer when the `"command"` intent fires. The status bar
surfaces the captured column as scene-as-data (§2 #7).

Verification scope (>=30 assertions; gates per [[zero-flake-policy]]:
every action->assert edge is wait_query / wait_until on observed state,
no fixed sleeps):

  (A) boot — menu closed (no panel/barrier), item_count 3, grid headers
      paint, sort proxy reports unsorted source order.
  (B) right-click header col 1 (Score) opens the popup at the press point
      over the universal arc; the captured column shows as scene-as-data.
  (C) activate "Sort Ascending" -> col 1 ascending; the command intent
      logs; the order is by Score (min id at visual 0).
  (D) re-open + "Sort Descending" -> col 1 descending (max id at 0).
  (E) re-open + "Clear Sort" -> back to source order.
  (F) a DIFFERENT column (Name, col 0) sorts independently.
  (G) right-click OFF the header band opens nothing (header-only menu).
  (H) WAI-ARIA keyboard model: focus the menu, Arrow nav, Enter activates.
  (I) the R715 click-outside barrier dismisses.
  (J) a plain left-click on a header still cycles its sort (the proxy
      path is unaffected by the new right-click surface).
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
# R988 — the labels name the right-clicked column; section (B) right-clicks
# the Score header, so the items read "Sort by Score ...".
MENU_ITEMS = ["Sort by Score ascending", "Sort by Score descending", "Clear Sort"]
# Cursor target for keyboard injection: top-left, over the barrier, clear
# of any popup item (mirrors r772's KEY_AT).
KEY_AT = (5.0, 5.0)


def _score(i: int) -> int:
    return (i * 37 + 11) % 100


# Source ids ordered by ascending Score (2-digit, so lexical == numeric).
SCORE_ASC = sorted(range(N), key=lambda i: (_score(i), i))


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _wait_paint(tf, predicate, desc):
    return wait_until(
        lambda: (lambda s: s if predicate(s) else None)(_paint(tf)),
        desc=desc,
    )


def _status_text(snap) -> str:
    node = find_by_tag(snap, "grid_status")
    if node is None:
        return ""
    return node.get("content") or ""


def _header_center(tf, col: int):
    """Center of the painted header cell for `col` (the geometric anchor a
    right-click must map back to that column)."""
    snap = _wait_paint(
        tf,
        lambda s: f"grid_ch{col}" in abs_rects_of(s),
        f"header grid_ch{col} paints",
    )
    x, y, w, h = abs_rects_of(snap)[f"grid_ch{col}"]
    return float(x + w // 2), float(y + h // 2)


def _source_at(tf, pos: int):
    return tf.query(f"/gsort/external/source_at.{pos}")


def _open_menu_on_header(tf, col: int):
    """Right-click the col header and wait for the popup to open."""
    cx, cy = _header_center(tf, col)
    tf.click(at=(cx, cy), button="right")
    wait_query(tf, "/external/open", True, desc=f"right-click header {col} opens menu")
    return cx, cy


def _activate(tf, item: int):
    """Left-click a popup item, waiting for the popup to close."""
    tf.click(path=f"colmenu#i{item}")
    wait_query(tf, "/external/open", False, desc=f"item {item} activates + closes")
    tf.pointer_leave()


def body() -> None:
    with RpcSubprocess("hello-grid-header-menu", boot_grace=1.5) as tf:
        # ── (A) boot: menu closed, grid + sort proxy live ───────────────
        snap = _wait_paint(tf, lambda s: "grid_ch0" in abs_rects_of(s),
                           "grid headers paint at boot")
        assert find_by_tag(snap, "colmenu") is None, "no popup panel while closed"      # 1
        assert find_by_tag(snap, "colmenu#barrier") is None, "no barrier while closed"  # 2
        assert_eq(tf.query("/external/open"), False, "boot: menu closed")               # 3
        assert_eq(tf.query("/external/item_count"), 3, "three command items")           # 4
        assert_eq(tf.query("/gsort/external/cols"), 3, "proxy knows the column count")  # 5
        assert_eq(tf.query("/gsort/external/count"), N, "proxy knows the row count")    # 6
        assert_eq(tf.query("/gsort/external/sort"), "none", "boot: unsorted")           # 7
        assert_eq(_source_at(tf, 0), 0, "unsorted: visual 0 == source 0")               # 8

        # ── (B) right-click header col 1 (Score) opens at the point ─────
        cx, cy = _open_menu_on_header(tf, 1)
        assert_eq(tf.query("/external/open_x"), cx, "popup anchored at the press x")    # 9
        assert_eq(tf.query("/external/open_y"), cy, "popup anchored at the press y")    # 10
        assert_eq(tf.query("/external/active"), None, "pointer open pre-highlights nothing")  # 11
        snap = _wait_paint(tf, lambda s: find_by_tag(s, "colmenu") is not None,
                           "popup paints after right-click")                            # 12
        assert find_by_tag(snap, "colmenu#barrier") is not None, "barrier paints"       # 13
        panel_text = " ".join(
            n.get("content") or ""
            for n in _iter_text(find_by_tag(snap, "colmenu"))
        )
        for item in MENU_ITEMS:
            assert item in panel_text, f"menu item {item!r} renders; got {panel_text!r}"  # 14,15,16
        # The captured column is scene-as-data on the status bar.
        assert "target Score (header)" in _status_text(snap), (
            f"status surfaces captured column; got {_status_text(snap)!r}"
        )                                                                               # 17

        # ── (C) activate "Sort Ascending" -> col 1 ascending ────────────
        _activate(tf, 0)
        wait_query(tf, "/gsort/external/sort", "1:ascending",
                   desc="menu Sort Ascending applies to col 1")                         # 18
        tail = tf.stderr_tail(80)
        assert any('shell: intent colmenu.command payload=Text("0")' in ln for ln in tail), (
            f"item activation logs the command intent; stderr={tail[-6:]!r}"
        )                                                                               # 19
        assert_eq(_source_at(tf, 0), SCORE_ASC[0], "asc Score: min-score id at visual 0")  # 20
        assert_eq(_source_at(tf, N - 1), SCORE_ASC[-1], "asc Score: max-score id last")    # 21

        # ── (D) re-open header 1 + "Sort Descending" ────────────────────
        _open_menu_on_header(tf, 1)
        _activate(tf, 1)
        wait_query(tf, "/gsort/external/sort", "1:descending",
                   desc="menu Sort Descending applies to col 1")                        # 22
        assert_eq(_source_at(tf, 0), SCORE_ASC[-1], "desc Score: max-score id at visual 0")  # 23

        # ── (E) re-open + "Clear Sort" -> source order ──────────────────
        _open_menu_on_header(tf, 1)
        _activate(tf, 2)
        wait_query(tf, "/gsort/external/sort", "none", desc="menu Clear Sort resets")   # 24
        assert_eq(_source_at(tf, 0), 0, "cleared: visual 0 == source 0")                # 25

        # ── (F) a DIFFERENT column (Name, col 0) sorts independently ────
        _open_menu_on_header(tf, 0)
        snap = _paint(tf)
        assert "target Name (header)" in _status_text(snap), (
            f"re-targeted to col 0; got {_status_text(snap)!r}"
        )                                                                               # 26
        _activate(tf, 0)
        wait_query(tf, "/gsort/external/sort", "0:ascending",
                   desc="Name column sorts ascending")                                  # 27
        # Name asc: "Alpha0" before "Alpha5" (lexical) -> source 0 then 5.
        assert_eq(_source_at(tf, 0), 0, "Name asc: Alpha0 at visual 0")                 # 28
        assert_eq(_source_at(tf, 1), 5, "Name asc: Alpha5 at visual 1")                 # 29
        # Reset for the remaining sections.
        _open_menu_on_header(tf, 0)
        _activate(tf, 2)
        wait_query(tf, "/gsort/external/sort", "none", desc="reset before keyboard")    # 30

        # ── (G) right-click OUTSIDE the grid opens nothing ──────────────
        # R988 — a right-click on a data cell now opens the menu too, so the
        # "opens nothing" case targets the status bar (above the grid).
        tf.click(at=(100.0, 12.0), button="right")  # the status bar, above the header
        # No state edge to await; the press-edge one-shot is synchronous on
        # the dispatch the rejecting override returns false for, so a direct
        # read is race-free (no repaint was scheduled).
        assert_eq(tf.query("/external/open"), False,
                  "right-click outside the grid (status bar) opens no menu")            # 31
        tf.pointer_leave()

        # ── (H) WAI-ARIA §3.5 keyboard model on the opened popup ────────
        _open_menu_on_header(tf, 2)  # Status column
        tf.request("focus/set", {"tag": "colmenu"})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == "colmenu",
            desc="menu owns focus",
        )                                                                               # 32
        tf.key(at=KEY_AT, name="ArrowDown")
        wait_query(tf, "/external/active", 0, desc="ArrowDown -> first item")           # 33
        tf.key(at=KEY_AT, name="ArrowUp")
        wait_query(tf, "/external/active", 2, desc="ArrowUp wraps -> last item")        # 34
        tf.key(at=KEY_AT, name="Home")
        wait_query(tf, "/external/active", 0, desc="Home -> first item")                # 35
        tf.key(at=KEY_AT, name="Enter")
        wait_query(tf, "/external/open", False, desc="Enter activates + closes")        # 36
        wait_query(tf, "/gsort/external/sort", "2:ascending",
                   desc="keyboard Enter applied Sort Ascending to col 2")               # 37
        # Reset.
        _open_menu_on_header(tf, 2)
        _activate(tf, 2)
        wait_query(tf, "/gsort/external/sort", "none", desc="reset before barrier")     # 38

        # ── (I) the R715 click-outside barrier dismisses ────────────────
        _open_menu_on_header(tf, 1)
        tf.pointer_leave()
        tf.click(at=(5.0, 5.0))  # left-click outside -> barrier
        wait_query(tf, "/external/open", False,
                   desc="left-click outside (barrier) dismisses")                       # 39
        _wait_paint(tf, lambda s: find_by_tag(s, "colmenu") is None,
                    "panel unpaints after dismiss")                                     # 40

        # ── (J) a plain left-click on a header still cycles its sort ────
        # The new right-click surface left the proxy's own left-click path
        # intact (gsort#h<col> -> cycle_sort).
        lx, ly = _header_center(tf, 1)
        tf.click(at=(lx, ly))  # left-click cycles col 1: none -> ascending
        wait_query(tf, "/gsort/external/sort", "1:ascending",
                   desc="left-click header cycles its sort (proxy path intact)")        # 41
        assert_eq(tf.query("/external/open"), False,
                  "a left-click on a header opens no context menu")                     # 42


def _iter_text(node):
    """Yield every Text node under `node` (depth-first)."""
    out = []

    def walk(n):
        if isinstance(n, dict):
            if n.get("type") == "Text":
                out.append(n)
            for c in n.get("children") or []:
                walk(c)

    walk(node)
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R986 §5.27 §5.38 §5.53 — data-grid column-header context menu", body))
