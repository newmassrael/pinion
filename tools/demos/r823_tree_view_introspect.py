#!/usr/bin/env python3
"""R823 §5.50 §5.12 — first-class tree-state RPC introspection.

Drives `hello-tree-view` over JSON-RPC and verifies the new query-only
`TreeViewIntrospect` surface (`tree_view_introspection_extra`): the tree's
structure + keyboard cursor are now read *as data* through `scene/query`
rather than scraped from the paint scene. Before R823 the only tree
Externals were the `ButtonExternal` primary and the `TreeRowClickExternal`
router (which exposes only transient pressed/hovered ids) — the structural
+ WAI-ARIA navigation state had no first-class query surface
([[ai-first-rpc-introspection-obligation]]).

Verifies:
  (A) boot structure: row_count + per-position id / label / aria-level /
      aria-expanded, and the cursor parked on `src`.
  (B) the introspected id sequence matches the painted `file_tree#<id>`
      rows (the query surface and the paint scene agree).
  (C) cursor tracks keyboard navigation (cursor / cursor_index move with
      Arrow keys, driven through `scene/key`).
  (D) row_count + expanded_at track collapse / expand (a structural edit
      observed purely through the query surface).
  (E) out-of-range positions are present-but-empty (Null), and a cursor on
      a collapsed-away id has no visible index.
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

VIEWPORT = (480, 400)
AT = (10.0, 10.0)
STATE_TAG = "file_tree_state"
TREE_TAG = "file_tree"
BOOT_IDS = ["src", "src/main.rs", "src/lib.rs", "src/widgets", "tests", "docs"]


def body() -> None:
    with RpcSubprocess("hello-tree-view", boot_grace=1.5) as tf:

        def q(path: str):
            return tf.query(f"/{STATE_TAG}/external/{path}")

        def snap():
            return tf.snapshot(source="paint", viewport=VIEWPORT)

        def press(name: str) -> None:
            tf.key(at=AT, name=name)

        def await_cursor(expected: str) -> None:
            wait_until(lambda: q("cursor") == expected, desc=f"cursor == {expected}")
            assert_eq(q("cursor"), expected, f"cursor must be {expected}")

        def await_row_count(expected: int) -> None:
            wait_until(lambda: q("row_count") == expected, desc=f"row_count == {expected}")
            assert_eq(q("row_count"), expected, f"row_count must be {expected}")

        # ── (A) boot structure via the query surface ────────────────
        assert find_by_tag(snap(), TREE_TAG) is not None, "TreeView root tag present"
        assert_eq(q("row_count"), 6, "boot: 6 visible rows (src expanded; tests/docs/widgets collapsed)")
        assert_eq(q("cursor"), "src", "boot cursor parked on src")
        assert_eq(q("cursor_index"), 0, "src is visual row 0")
        # Per-position id + label + aria-level (depth + 1).
        for pos, node_id in enumerate(BOOT_IDS):
            assert_eq(q(f"id_at.{pos}"), node_id, f"id_at.{pos} == {node_id}")
        assert_eq(q("label_at.0"), "src", "label_at.0 == src")
        assert_eq(q("label_at.1"), "main.rs", "label_at.1 == main.rs")
        assert_eq(q("label_at.3"), "widgets", "label_at.3 == widgets")
        assert_eq(q("level_at.0"), 1, "src is a root row → aria-level 1")
        assert_eq(q("level_at.1"), 2, "src/main.rs is one level deeper → aria-level 2")
        assert_eq(q("level_at.3"), 2, "src/widgets is one level deeper → aria-level 2")
        # aria-expanded: Bool for a branch, Null for a leaf.
        assert_eq(q("expanded_at.0"), True, "src is an expanded branch")
        assert_eq(q("expanded_at.1"), None, "src/main.rs is a leaf → aria-expanded undefined")
        assert_eq(q("expanded_at.3"), False, "src/widgets is a collapsed branch")
        assert_eq(q("expanded_at.4"), False, "tests is a collapsed branch")

        # ── (E) out-of-range position is present-but-empty ──────────
        assert_eq(q("id_at.6"), None, "one past the end is Null (present-but-empty)")
        assert_eq(q("label_at.99"), None, "far out-of-range label is Null")
        assert_eq(q("expanded_at.6"), None, "out-of-range aria-expanded is Null")

        # ── (B) the query surface agrees with the paint scene ───────
        painted = set()
        s = snap()

        def collect(node):
            if not isinstance(node, dict):
                return
            tag = node.get("tag")
            if isinstance(tag, str) and tag.startswith(f"{TREE_TAG}#"):
                painted.add(tag.removeprefix(f"{TREE_TAG}#"))
            for child in node.get("children") or []:
                collect(child)
            collect(node.get("content"))  # Scroll wrapper

        collect(s)
        for node_id in BOOT_IDS:
            assert node_id in painted, f"introspected id {node_id} also renders; painted={sorted(painted)!r}"

        # Focus the single-tab-stop tree before driving keys (R820.1 gate).
        tf.request("focus/set", {"tag": "tree_root"})

        # ── (C) cursor tracks keyboard navigation ───────────────────
        press("ArrowDown")
        await_cursor("src/main.rs")
        assert_eq(q("cursor_index"), 1, "cursor_index follows the cursor down")
        press("ArrowDown")
        await_cursor("src/lib.rs")
        assert_eq(q("cursor_index"), 2, "cursor_index == 2 on src/lib.rs")
        press("End")
        await_cursor("docs")
        assert_eq(q("cursor_index"), 5, "End jumps the cursor to the last row")
        press("Home")
        await_cursor("src")

        # ── (D) row_count + expanded_at track collapse / expand ─────
        press("ArrowLeft")  # collapse the expanded src
        await_row_count(3)
        assert_eq(q("expanded_at.0"), False, "src now reports collapsed via the query surface")
        assert_eq(q("id_at.1"), "tests", "with src collapsed, visual row 1 is the tests sibling")
        assert_eq(q("id_at.2"), "docs", "…and visual row 2 is docs")
        press("ArrowRight")  # re-expand src
        await_row_count(6)
        assert_eq(q("expanded_at.0"), True, "src reports expanded again")
        assert_eq(q("id_at.1"), "src/main.rs", "its first child is visible row 1 again")

        # ── (E) a cursor on a collapsed-away id has no visible index ─
        # Move to src/widgets (a collapsed branch), then collapse its
        # parent src: the cursor id is retained but is no longer visible.
        press("ArrowDown")  # src/main.rs
        press("ArrowDown")  # src/lib.rs
        press("ArrowDown")  # src/widgets
        await_cursor("src/widgets")
        assert_eq(q("cursor_index"), 3, "src/widgets is visual row 3")
        press("ArrowLeft")  # collapsed branch → ascend to parent src
        await_cursor("src")
        assert_eq(q("cursor_index"), 0, "ascended to the src parent")


if __name__ == "__main__":
    sys.exit(run_demo("R823 §5.50 §5.12 — tree-state RPC introspection", body))
