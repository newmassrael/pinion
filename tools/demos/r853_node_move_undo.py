#!/usr/bin/env python3
"""R853 §5.38 §5.52 — node-graph move undo (drag = one step, nudge coalesces).

Drives hello-node-editor over JSON-RPC. R851 made the *structural* edits
reversible but left node **moves** un-journaled (an honest gap: redoing an add
restored the node at its add-time position). R853 closes it — every move is a
reversible `MoveNodeCmd` on the same `UndoStack`:

  * a capture **drag** records ONE move at gesture end (before = grab position,
    after = release position) — not one per cursor frame;
  * a keyboard **nudge burst** coalesces to one undo step (the long-deferred
    `UndoCommand::merge` hook, now with a real consumer);
  * an RPC `intervene .x` then `.y` on the same node coalesce to one move.

  (A) drag a node, undo reverts the whole drag, redo re-applies it.
  (B) three Arrow nudges = ONE coalesced undo step.
  (C) intervene x then y = ONE coalesced move; one undo reverts both axes.
  (D) a move after an add is a separate, redoable step (the R851 wrinkle fixed).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
UNDO = "/node_undo/external"
WIN = (132 + 640, 420)


def pos(tf, nid: int) -> tuple[int, int]:
    return (tf.query(f"/external/node.{nid}.x"), tf.query(f"/external/node.{nid}.y"))


def count(tf) -> int:
    return tf.query(f"{UNDO}/count")


def undo(tf) -> bool:
    return tf.invoke(f"{UNDO}/undo", None)


def redo(tf) -> bool:
    return tf.invoke(f"{UNDO}/redo", None)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        assert_eq(count(tf), 0, "boot: empty undo history")

        # ── (A) drag a node -> one undo step ─────────────────────────
        before = pos(tf, 0)
        # Drag node 0's card toward the lower-right of the canvas.
        tf.drag(from_path=f"{G}#node_0", to_at=(132 + 360, 300), steps=8)
        moved = wait_until(
            lambda: pos(tf, 0) if pos(tf, 0) != before else None,
            desc="the drag moved node 0",
        )
        assert moved != before, "node 0 moved under the drag"
        assert_eq(count(tf), 1, "the whole drag is exactly one undo step")
        assert_eq(undo(tf), True, "undo the drag")
        assert_eq(pos(tf, 0), before, "undo reverted the drag to the grab position")
        assert_eq(redo(tf), True, "redo the drag")
        assert_eq(pos(tf, 0), moved, "redo re-applied the drag")
        # Leave the drag redone (cursor at the top, no redo branch) for (B).

        # ── (B) keyboard nudge burst -> one coalesced step ───────────
        tf.request("focus/set", {"tag": G})
        tf.intervene("/external/selected", 0)
        bx, by = pos(tf, 0)
        tf.key(path=G, name="ArrowRight")  # first nudge establishes the step
        c_base = count(tf)
        tf.key(path=G, name="ArrowRight")
        tf.key(path=G, name="ArrowRight")
        after = wait_until(
            lambda: pos(tf, 0) if pos(tf, 0)[0] >= bx + 3 * 12 else None,
            desc="the nudges moved node 0 right by 3 steps",
        )
        assert_eq(after, (bx + 36, by), "3 nudges moved 36px in x, none in y")
        assert_eq(count(tf), c_base, "nudges after the first coalesce (count unchanged)")
        assert_eq(undo(tf), True, "one undo reverts the whole burst")
        assert_eq(pos(tf, 0), (bx, by), "node 0 back to the pre-burst position")

        # ── (C) intervene x then y -> one coalesced move ─────────────
        cx, cy = pos(tf, 0)
        tf.intervene("/external/node.0.x", 260)  # first move establishes the step
        c_base = count(tf)
        tf.intervene("/external/node.0.y", 190)
        assert_eq(pos(tf, 0), (260, 190), "both axes set via RPC")
        assert_eq(count(tf), c_base, "y coalesced into the x move (count unchanged)")
        assert_eq(undo(tf), True, "one undo reverts both axes")
        assert_eq(pos(tf, 0), (cx, cy), "node 0 fully restored")

        # ── (D) a move after an add is a separate, redoable step ─────
        new_id = tf.invoke("/external/add_node", "Multiply")  # structural step
        c_add = count(tf)
        add_pos = pos(tf, new_id)
        tf.request("focus/set", {"tag": G})
        tf.intervene("/external/selected", new_id)
        tf.key(path=G, name="ArrowDown")
        moved_pos = wait_until(
            lambda: pos(tf, new_id) if pos(tf, new_id) != add_pos else None,
            desc="the new node nudged",
        )
        assert moved_pos != add_pos, "the new node moved"
        assert_eq(count(tf), c_add + 1, "the move is a separate step, not folded into the add")
        assert_eq(undo(tf), True, "undo the move")
        assert_eq(pos(tf, new_id), add_pos, "node back at its add-time position")
        assert_eq(redo(tf), True, "redo the move")
        assert_eq(pos(tf, new_id), moved_pos, "redo restores the moved position (R851 wrinkle fixed)")


if __name__ == "__main__":
    sys.exit(run_demo("R853 §5.38 §5.52 — node-graph move undo", body))
