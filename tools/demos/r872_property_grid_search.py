#!/usr/bin/env python3
"""R872 §5.27 §5.40 §5.38 — property-grid live name search / filter.

The Details-panel search box: typing in the title-bar search field filters the
visible property rows by name, live (every keystroke), composing with the R871
category grouping — the R844 filter -> group proxy chain, here with a
name-search filter feeding `use_group_order_with_source`. A category whose every
member is filtered out contributes no header.

Two cooperating wires:
  property_grid_search (extra TextField) -> the live filter query (its text)
  property_grid_cat    (GroupOrderExternal) -> the grouped/filtered flatten

Group-by proxy slots (`property_grid_cat`), reflecting the *filtered* flatten:
  /external/visible_len           -> headers + visible data rows after filtering
  /external/kind_at.<pos>         -> "header" | "data"
  /external/source_at.<pos>       -> a data row's source index (null on header)
  /external/label_at.<pos>        -> a header's category label
  /external/member_count_at.<pos> -> member count (over the base order)
  /external/toggle_group          -> invoke(int): collapse/expand a category

Verified (>= 30 assertions):
  (A) boot — search box painted, empty query, all 17 rows
  (B) name filter — "pos" narrows to Transform's 2 Pos rows + its header
  (C) clear via Backspace restores every row
  (D) category narrowing — "name" leaves only Identity + Name
  (E) no-match query -> empty flatten (no headers, no rows)
  (F) filter + collapse compose — collapse a filtered category to its header
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
CAT = "property_grid_cat"

TRANSFORM = 4  # the Transform category id (Pos X / Pos Y)


def _focus_search(tf) -> None:
    tf.request("focus/set", {"tag": SEARCH})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == SEARCH,
        timeout=4.0, interval=0.03, desc="search box owns keyboard focus",
    )


def _vlen(tf) -> int:
    return tf.query(f"/{CAT}/external/visible_len")


def _clear(tf, n: int) -> None:
    """Backspace `n` times to clear an `n`-char query (stays focused)."""
    for _ in range(n):
        tf.key(path=SEARCH, name="Backspace")


def body() -> None:
    with RpcSubprocess("hello-property-grid", boot_grace=1.5) as tf:
        # ── (A) boot: search box painted, empty query, all rows ──────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, GRID) is not None, "grid present"
        assert find_by_tag(snap, SEARCH) is not None, "search box painted in the title band"
        assert_eq(tf.query(f"/{CAT}/external/group_count"), 5, "5 categories")
        assert_eq(_vlen(tf), 17, "boot: 5 headers + 12 data, no filter")

        # ── (B) name filter: "pos" -> Transform header + Pos X / Pos Y ─
        _focus_search(tf)
        tf.text("pos", path=SEARCH)
        wait_until(lambda: _vlen(tf) == 3, timeout=4.0, interval=0.03,
                   desc="'pos' narrows to the Transform header + 2 Pos rows")
        assert_eq(tf.query(f"/{CAT}/external/kind_at.0"), "header", "row 0 is the category header")
        assert_eq(tf.query(f"/{CAT}/external/label_at.0"), "Transform", "the surviving header is Transform")
        assert_eq(tf.query(f"/{CAT}/external/member_count_at.0"), 2, "Transform has 2 members")
        assert_eq(tf.query(f"/{CAT}/external/kind_at.1"), "data", "row 1 is a data row")
        assert_eq(tf.query(f"/{CAT}/external/source_at.1"), 6, "Pos X (source 6)")
        assert_eq(tf.query(f"/{CAT}/external/source_at.2"), 7, "Pos Y (source 7)")

        # ── (C) clear restores every row ─────────────────────────────
        _clear(tf, 3)
        wait_until(lambda: _vlen(tf) == 17, timeout=4.0, interval=0.03,
                   desc="clearing the query restores all rows")

        # ── (D) category narrowing: "name" -> Identity + Name only ────
        tf.text("name", path=SEARCH)
        wait_until(lambda: _vlen(tf) == 2, timeout=4.0, interval=0.03,
                   desc="'name' leaves Identity header + Name")
        assert_eq(tf.query(f"/{CAT}/external/label_at.0"), "Identity", "surviving header is Identity")
        assert_eq(tf.query(f"/{CAT}/external/source_at.1"), 0, "Name (source 0)")
        _clear(tf, 4)
        wait_until(lambda: _vlen(tf) == 17, timeout=4.0, interval=0.03, desc="cleared again")

        # ── (E) no-match query -> empty flatten ──────────────────────
        tf.text("zzz", path=SEARCH)
        wait_until(lambda: _vlen(tf) == 0, timeout=4.0, interval=0.03,
                   desc="no-match query empties the flatten (no headers)")
        assert_eq(tf.query(f"/{CAT}/external/kind_at.0"), None, "no row at position 0 when empty")
        _clear(tf, 3)
        wait_until(lambda: _vlen(tf) == 17, timeout=4.0, interval=0.03, desc="restored after no-match")

        # ── (F) filter + collapse compose ────────────────────────────
        tf.text("pos", path=SEARCH)
        wait_until(lambda: _vlen(tf) == 3, timeout=4.0, interval=0.03, desc="'pos' -> 3 rows")
        # Collapse the (filtered) Transform category -> only its header remains.
        assert_eq(tf.invoke(f"/{CAT}/external/toggle_group", TRANSFORM), 1,
                  "collapse the filtered Transform -> header only")
        assert_eq(tf.query(f"/{CAT}/external/kind_at.0"), "header", "the lone row is the header")
        assert_eq(tf.invoke(f"/{CAT}/external/toggle_group", TRANSFORM), 3, "re-expand -> 3 rows")

        # ── (G) Escape clears the filter + returns focus to the grid ──
        assert _vlen(tf) == 3, "still filtered before Escape"
        tf.key(path=SEARCH, name="Escape")
        wait_until(lambda: _vlen(tf) == 17, timeout=4.0, interval=0.03,
                   desc="Escape clears the filter")
        wait_until(lambda: tf.request("focus/get").result.get("focused") == GRID,
                   timeout=4.0, interval=0.03, desc="Escape returns focus to the grid")


if __name__ == "__main__":
    sys.exit(run_demo("hello-property-grid R872 §5.27 live property search/filter", body))
