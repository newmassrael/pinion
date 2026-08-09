#!/usr/bin/env python3
"""R872 / R921 §5.27 §5.40 §5.38 — property-grid live name search / filter.

The Details-panel search box: typing in the title-bar search field filters the
visible property rows by name, live (every keystroke), over the R921 property
**tree**. The recursive path-to-match filter (`flat_visible_filtered`, the toolkit
`setRecursiveFilteringEnabled`) keeps any node on a path to a match and reveals
it even inside a collapsed branch, so searching "pos" surfaces the Position
struct's fields under their Transform > Position path. A branch with no match
anywhere on its path is pruned.

Two cooperating wires:
  property_grid_search (extra TextField)         -> the live filter query (text)
  property_grid_tree   (TreeViewIntrospect, RO)  -> the filtered visible flatten:
    /external/row_count       -> visible row count after filtering
    /external/id_at.<pos>     -> a row's node id (leaf "6" / branch cat./struct.)
    /external/label_at.<pos>  -> a row's label
    /external/level_at.<pos>  -> aria-level (depth + 1)
  property_grid        (coordinator) -> /external/toggle_branch (collapse a branch)

Verified (>= 30 assertions):
  (A) boot — search box painted, empty query, the full 28-row tree
  (B) name filter — "pos" reveals Transform > Position > X/Y/Z (5 rows)
  (C) clear via Backspace restores every row
  (D) deep prune — "name" leaves only Identity > Name
  (E) no-match query -> empty flatten
  (F) collapse (no filter) hides a branch's subtree; a filter auto-reveals it
  (G) Escape clears the filter and hands focus back to the grid
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
    wait_until,
)

VIEWPORT = (460, 820)

GRID = "property_grid"
SEARCH = "property_grid_search"
TREE = "property_grid_tree"

TRANSFORM = "cat.Transform"
POSITION = "struct.Position"


def _focus_search(tf) -> None:
    tf.request("focus/set", {"tag": SEARCH})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == SEARCH,
        timeout=4.0, interval=0.03, desc="search box owns keyboard focus",
    )


def _rows(tf) -> int:
    return tf.query(f"/{TREE}/external/row_count")


def _id(tf, pos: int):
    return tf.query(f"/{TREE}/external/id_at.{pos}")


def _label(tf, pos: int):
    return tf.query(f"/{TREE}/external/label_at.{pos}")


def _level(tf, pos: int):
    return tf.query(f"/{TREE}/external/level_at.{pos}")


def _clear(tf, n: int) -> None:
    """Backspace `n` times to clear an `n`-char query (stays focused)."""
    for _ in range(n):
        tf.key(path=SEARCH, name="Backspace")


def body() -> None:
    with RpcSubprocess("hello-property-grid", boot_grace=1.5) as tf:
        # ── (A) boot: search box painted, empty query, full tree ─────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, GRID) is not None, "grid present"
        assert find_by_tag(snap, SEARCH) is not None, "search box painted in the title band"
        assert_eq(_rows(tf), 28, "boot: 6 categories + 2 structs + 1 array + 19 leaves")
        assert_eq(_id(tf, 0), "cat.Identity", "the first row is the Identity category")

        # ── (B) name filter: "pos" -> Transform > Position > X/Y/Z ───
        _focus_search(tf)
        tf.text("pos", path=SEARCH)
        wait_until(lambda: _rows(tf) == 5, timeout=4.0, interval=0.03,
                   desc="'pos' reveals the Transform > Position path + its 3 fields")
        assert_eq(_id(tf, 0), TRANSFORM, "the surviving category is Transform")
        assert_eq(_label(tf, 0), "Transform", "its label is Transform")
        assert_eq(_level(tf, 0), 1, "the category is aria-level 1")
        assert_eq(_id(tf, 1), POSITION, "the Position struct is revealed on the match path")
        assert_eq(_level(tf, 1), 2, "the struct is aria-level 2")
        assert_eq(_id(tf, 2), "6", "Position X (value 6)")
        assert_eq(_level(tf, 2), 3, "a struct field is aria-level 3")
        assert_eq(_id(tf, 3), "7", "Position Y (value 7)")
        assert_eq(_id(tf, 4), "12", "Position Z (value 12)")

        # ── (C) clear restores every row ─────────────────────────────
        _clear(tf, 3)
        wait_until(lambda: _rows(tf) == 28, timeout=4.0, interval=0.03,
                   desc="clearing the query restores all rows")

        # ── (D) deep prune: "name" -> Identity > Name only ───────────
        tf.text("name", path=SEARCH)
        wait_until(lambda: _rows(tf) == 2, timeout=4.0, interval=0.03,
                   desc="'name' leaves Identity > Name")
        assert_eq(_id(tf, 0), "cat.Identity", "surviving category is Identity")
        assert_eq(_id(tf, 1), "0", "Name (value 0)")
        _clear(tf, 4)
        wait_until(lambda: _rows(tf) == 28, timeout=4.0, interval=0.03, desc="cleared again")

        # ── (E) no-match query -> empty flatten ──────────────────────
        tf.text("zzz", path=SEARCH)
        wait_until(lambda: _rows(tf) == 0, timeout=4.0, interval=0.03,
                   desc="no-match query empties the flatten")
        assert_eq(_id(tf, 0), None, "no row at position 0 when empty")
        _clear(tf, 3)
        wait_until(lambda: _rows(tf) == 28, timeout=4.0, interval=0.03, desc="restored after no-match")

        # ── (F) collapse (no filter) hides a subtree; a filter reveals it ─
        # With no filter, collapsing Transform hides its 2 structs + 6 fields.
        assert_eq(tf.invoke(f"/{GRID}/external/toggle_branch", TRANSFORM), False, "Transform collapses")
        wait_until(lambda: _rows(tf) == 20, timeout=4.0, interval=0.03,
                   desc="collapsing Transform hides its 8-row subtree (28 - 8)")
        # The recursive filter auto-reveals matches regardless of collapse state.
        _focus_search(tf)
        tf.text("pos", path=SEARCH)
        wait_until(lambda: _rows(tf) == 5, timeout=4.0, interval=0.03,
                   desc="'pos' reveals the Position path even though Transform is collapsed")
        _clear(tf, 3)
        # Re-expand Transform so the tree is whole again.
        assert_eq(tf.invoke(f"/{GRID}/external/toggle_branch", TRANSFORM), True, "Transform re-expands")
        wait_until(lambda: _rows(tf) == 28, timeout=4.0, interval=0.03, desc="full tree restored")

        # ── (G) Escape clears the filter + returns focus to the grid ──
        tf.text("pos", path=SEARCH)
        wait_until(lambda: _rows(tf) == 5, timeout=4.0, interval=0.03, desc="filtered before Escape")
        tf.key(path=SEARCH, name="Escape")
        wait_until(lambda: _rows(tf) == 28, timeout=4.0, interval=0.03,
                   desc="Escape clears the filter")
        wait_until(lambda: tf.request("focus/get").result.get("focused") == GRID,
                   timeout=4.0, interval=0.03, desc="Escape returns focus to the grid")


if __name__ == "__main__":
    sys.exit(run_demo("hello-property-grid R872 / R921 §5.27 live property search/filter", body))
