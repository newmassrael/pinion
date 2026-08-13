#!/usr/bin/env python3
"""R824 §5.50 §5.12 — tree-state RPC introspection on the *virtualized* tree.

Drives `hello-virtual-tree` (a 10 000-node tree, 496 rows visible at boot,
only a ~dozen-row window ever painted) over JSON-RPC and verifies the R823
`tree_view_introspection_extra` surface as its 2nd consumer — and, crucially,
that the query surface is **window-independent**: for a virtualized tree only
a window of rows is ever a scene node, so `scene/query` is the *only* way an
AI reads the full structure (`row_count` over the whole flattening, plus
`id_at` / `label_at` / `level_at` / `expanded_at` / `cursor` for positions far
outside the painted window). Paint-scraping fundamentally cannot reach them.

Verifies:
  (A) boot: full 496-row count + cursor on s0, while only a small window paints.
  (B) window-independence: rows at flat positions 100 / 200 / 400 / 495 are
      queryable by id / label / level / aria-expanded yet are NOT painted.
  (C) child rows (depth-1 leaves) report aria-level 2 + undefined aria-expanded.
  (D) row_count + expanded_at track a collapse / expand driven by scene/key.
  (E) the cursor is introspectable even when navigated far off the window (End).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    unclipped_rects_of,
    assert_eq,
    run_demo,
    wait_until,
)

VIEWPORT = (480, 520)
AT = (10.0, 10.0)
STATE_TAG = "vtree_state"
TREE_TAG = "vtree"
ROOT_TAG = "vtree_root"
# 100 sections x (1 + 99) nodes; sections 0..3 expanded at boot.
# Boot flattening: 4 expanded sections (1 + 99 each) + 96 collapsed = 496.
BOOT_ROWS = 4 * (1 + 99) + 96  # 496


def body() -> None:
    with RpcSubprocess("hello-virtual-tree", boot_grace=1.5) as tf:

        def q(path: str):
            return tf.query(f"/{STATE_TAG}/external/{path}")

        def snap():
            return tf.snapshot(source="paint", viewport=VIEWPORT)

        def press(name: str) -> None:
            tf.key(at=AT, name=name)

        def painted_ids() -> set[str]:
            ids: set[str] = set()

            def walk(node):
                if not isinstance(node, dict):
                    return
                tag = node.get("tag")
                if isinstance(tag, str) and tag.startswith(f"{TREE_TAG}#"):
                    ids.add(tag.removeprefix(f"{TREE_TAG}#"))
                for child in node.get("children") or []:
                    walk(child)
                walk(node.get("content"))  # Scroll wrapper

            walk(snap())
            return ids

        def await_cursor(expected: str) -> None:
            wait_until(lambda: q("cursor") == expected, desc=f"cursor == {expected}")
            assert_eq(q("cursor"), expected, f"cursor must be {expected}")

        def await_row_count(expected: int) -> None:
            wait_until(lambda: q("row_count") == expected, desc=f"row_count == {expected}")
            assert_eq(q("row_count"), expected, f"row_count must be {expected}")

        # ── (A) boot: full structure via query while only a window paints ──
        assert ROOT_TAG in unclipped_rects_of(snap()), "focusable virtual tree root present"
        assert_eq(q("row_count"), BOOT_ROWS, "full 496-row flattening reported")
        assert_eq(q("cursor"), "s0", "cursor parked on the first section")
        assert_eq(q("cursor_index"), 0, "s0 is flat row 0")
        boot_painted = painted_ids()
        assert 0 < len(boot_painted) < 40, f"only a window paints, got {len(boot_painted)} of {BOOT_ROWS}"
        assert "s0" in boot_painted, "the cursor section is in the painted window"
        # boot root structure
        assert_eq(q("id_at.0"), "s0", "id_at.0 == s0")
        assert_eq(q("label_at.0"), "Section 000", "label_at.0 == Section 000")
        assert_eq(q("level_at.0"), 1, "a section is a root row → aria-level 1")
        assert_eq(q("expanded_at.0"), True, "s0 boots expanded")

        # ── (B) window-independence: far off-window rows are queryable ─────
        # Flat positions: s1@100, s2@200, s3@300, s4@400, s5@401, s99@495.
        assert_eq(q("id_at.100"), "s1", "row 100 is the 2nd section (after s0 + 99 children)")
        assert_eq(q("label_at.100"), "Section 001", "…labelled Section 001")
        assert_eq(q("expanded_at.100"), True, "s1 boots expanded (s < 4)")
        assert_eq(q("id_at.200"), "s2", "row 200 is s2")
        assert_eq(q("id_at.300"), "s3", "row 300 is s3")
        assert_eq(q("id_at.400"), "s4", "row 400 is the first collapsed section")
        assert_eq(q("label_at.400"), "Section 004", "…labelled Section 004")
        assert_eq(q("level_at.400"), 1, "s4 is a section → aria-level 1")
        assert_eq(q("expanded_at.400"), False, "s4 is collapsed (s >= 4)")
        assert_eq(q("id_at.401"), "s5", "collapsed sections are contiguous → s5 follows s4")
        assert_eq(q("id_at.495"), "s99", "row 495 is the last section")
        assert_eq(q("id_at.496"), None, "one past the end is Null (present-but-empty)")
        # The decisive point: these rows are NOT painted, yet are fully queryable.
        for off in ("s1", "s2", "s4", "s99"):
            assert off not in boot_painted, f"{off} is off-window (not painted) but was queryable"

        # ── (C) child rows: aria-level 2 + undefined aria-expanded ────────
        assert_eq(q("id_at.1"), "s0-i0", "s0's first child is flat row 1")
        assert_eq(q("label_at.1"), "Item 000-0000", "…labelled Item 000-0000")
        assert_eq(q("level_at.1"), 2, "a child is one level deeper → aria-level 2")
        assert_eq(q("expanded_at.1"), None, "a leaf's aria-expanded is undefined (Null)")

        # Focus the single-tab-stop tree before driving keys (R820.1 gate).
        tf.request("focus/set", {"tag": "vtree_root"})

        # ── (D) row_count + expanded_at track a key-driven collapse/expand ─
        press("ArrowLeft")  # collapse the expanded s0 (cursor sits on it)
        await_row_count(BOOT_ROWS - 99)  # 496 - 99 children = 397
        assert_eq(q("expanded_at.0"), False, "s0 reports collapsed via the query surface")
        assert_eq(q("id_at.1"), "s1", "with s0 collapsed, flat row 1 is the s1 sibling")
        press("ArrowRight")  # re-expand s0
        await_row_count(BOOT_ROWS)
        assert_eq(q("expanded_at.0"), True, "s0 reports expanded again")
        assert_eq(q("id_at.1"), "s0-i0", "its first child is flat row 1 again")

        # ── (E) cursor is introspectable when navigated far off-window ─────
        press("End")
        await_cursor("s99")  # jumps the cursor to the last of 496 rows
        assert_eq(q("cursor_index"), BOOT_ROWS - 1, "End parks the cursor on flat row 495")


if __name__ == "__main__":
    sys.exit(run_demo("R824 §5.50 §5.12 — virtual tree-state introspection", body))
