#!/usr/bin/env python3
"""R935 §5.51 scene-outliner drag-to-reparent — `hello-tree-reparent`.

The hierarchical move a flat reorder (hello-dnd / hello-tab-reorder) cannot do:
drag a tree node ONTO another to make it that node's child (reparent), or
BETWEEN two rows to reorder among siblings. Cycle prevention rejects a drop
into the node's own subtree.

Proven as DATA through `/external/*` introspection (§2 #7) — no pixels needed
beyond the row rects the drag is aimed at:

  - boot: the visible flattening (row_count / id_at / depth_at) and every
    node's parent (parent_of) match the declared tree.
  - drag "Key Light" ONTO "Props" → parent_of(light) becomes "props" and Props
    auto-expands (its hidden child becomes visible); the move is observable as
    a parent change, not a pixel diff.
  - drag to reorder siblings (Before / After) reorders within a parent.
  - drag a child onto the root area promotes it to a top-level node.
  - drag a parent onto its own descendant is REJECTED (no change) — the cycle
    guard.

The drag is driven by the existing `scene/drag` RPC (press row centre → move to
the target row at a vertical fraction selecting before/onto/after → release):
no new RPC method. ZERO-FLAKE: each assertion polls observed introspection
state (`wait_query`), never a fixed sleep.

Run from the workspace root:
    cargo build -p hello-tree-reparent --release
    python3 tools/demos/r935_tree_reparent.py
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
)

EXAMPLE = "hello-tree-reparent"
VIEWPORT = (340, 460)
TAG = "outliner"

# Vertical fractions of the target row selecting the drop zone (the binding
# splits the row height into thirds along y_rel).
BEFORE = 0.12
ONTO = 0.5
AFTER = 0.88


def row_tag(node_id: str) -> str:
    return f"{TAG}#{node_id}"


def parent_of(tf, node_id: str):
    return tf.query(f"/external/parent_of.{node_id}")


def row_count(tf) -> int:
    return tf.query("/external/row_count")


def id_at(tf, n: int):
    return tf.query(f"/external/id_at.{n}")


def visible_ids(tf) -> list:
    return [id_at(tf, i) for i in range(row_count(tf))]


def row_rects(tf) -> dict:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return abs_rects_of(snap)


def drag_onto(tf, src_id: str, dst_id: str, frac_y: float) -> None:
    """Drag the row of `src_id` onto the row of `dst_id`, releasing at vertical
    fraction `frac_y` (selecting before / onto / after)."""
    rects = row_rects(tf)
    sx, sy, sw, sh = rects[row_tag(src_id)]
    dx, dy, dw, dh = rects[row_tag(dst_id)]
    frm = (float(sx + sw // 2), float(sy + sh // 2))
    to = (float(dx + dw // 2), float(dy + int(dh * frac_y)))
    tf.drag(from_at=frm, to_at=to, steps=8)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot structure ──────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, TAG) is not None, "outliner root present at boot"
        boot_order = ["world", "terrain", "player", "camera", "light", "props", "ui", "hud"]
        assert_eq(row_count(tf), len(boot_order), "boot visible row count")
        assert_eq(visible_ids(tf), boot_order, "boot visible flattening")
        for node in boot_order:
            assert find_by_tag(snap, row_tag(node)) is not None, f"row {node} painted at boot"
        # Declared parentage (root nodes report "" — present, no parent).
        assert_eq(parent_of(tf, "world"), "", "world is a root")
        assert_eq(parent_of(tf, "terrain"), "world", "terrain under world")
        assert_eq(parent_of(tf, "camera"), "player", "camera under player")
        assert_eq(parent_of(tf, "light"), "player", "light under player")
        assert_eq(parent_of(tf, "crate"), "props", "crate under props (hidden, collapsed)")
        assert_eq(parent_of(tf, "hud"), "ui", "hud under ui")
        assert_eq(parent_of(tf, "ghost"), None, "absent node → Null")
        assert tf.query("/external/preview") is None, "no drag preview at rest"
        # `crate` is parented under the collapsed `props`, so it is not visible.
        assert "crate" not in visible_ids(tf), "collapsed branch hides its child"

        # ── (B) reparent: drag Key Light ONTO Props ─────────────────
        drag_onto(tf, "light", "props", ONTO)
        wait_query(
            tf, "/external/parent_of.light", "props",
            desc="Key Light reparented under Props",
        )
        # Props auto-expanded on the Onto-drop, so its children are now visible.
        assert_eq(parent_of(tf, "crate"), "props", "Props' original child still under it")
        ids = visible_ids(tf)
        assert "crate" in ids, "Props auto-expanded → its children are visible"
        assert "light" in ids, "the moved node is visible under its new parent"
        # light is no longer a child of player.
        player_kids = [n for n in ids if parent_of(tf, n) == "player"]
        assert player_kids == ["camera"], f"player now has only camera, got {player_kids}"
        assert tf.query("/external/preview") is None, "preview cleared after drop"

        # ── (C) reorder among siblings: Before / After ──────────────
        # Move "terrain" AFTER "player" (both children of world) → world's
        # visible children reorder to player-block, then terrain.
        drag_onto(tf, "terrain", "player", AFTER)
        wait_query(
            tf, "/external/parent_of.terrain", "world",
            desc="terrain stays a child of world after the reorder",
        )
        ids = visible_ids(tf)
        # world's direct children in visible order: player before terrain now.
        assert ids.index("player") < ids.index("terrain"), "terrain reordered after the player block"

        # Move "terrain" BEFORE "player" again → terrain leads.
        drag_onto(tf, "terrain", "player", BEFORE)
        wait_query(
            tf, "/external/parent_of.terrain", "world",
            desc="terrain still under world after the second reorder",
        )
        ids = visible_ids(tf)
        assert ids.index("terrain") < ids.index("player"), "terrain reordered before player"

        # ── (D) promote a child to the root level ───────────────────
        # Drag "camera" AFTER "ui" (a root node) → camera becomes a root.
        drag_onto(tf, "camera", "ui", AFTER)
        wait_query(
            tf, "/external/parent_of.camera", "",
            desc="camera promoted to a root-level node",
        )
        assert_eq(parent_of(tf, "camera"), "", "camera is now a root sibling")

        # ── (E) cycle prevention: drop a parent into its own subtree ─
        # "props" currently holds "light". Dragging "props" ONTO "light"
        # (its own child) must be rejected — the tree is unchanged.
        before_ids = visible_ids(tf)
        before_parent = parent_of(tf, "light")
        drag_onto(tf, "props", "light", ONTO)
        # The drag completes but the commit is a no-op; poll until the drag
        # state settles, then assert nothing moved.
        wait_query(tf, "/external/preview", None, desc="drag session ended")
        assert_eq(parent_of(tf, "light"), before_parent, "light still under props (cycle rejected)")
        assert_eq(visible_ids(tf), before_ids, "tree unchanged after a cycle drop")
        # And "props" did not become its own descendant's child.
        assert parent_of(tf, "props") != "light", "props was not parented under its child"


if __name__ == "__main__":
    sys.exit(run_demo("R935 scene-outliner drag-to-reparent", body))
