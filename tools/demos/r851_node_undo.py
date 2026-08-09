#!/usr/bin/env python3
"""R851 §5.38 §5.52 — node-graph undo / redo (structural edits).

Drives hello-node-editor over JSON-RPC. R838-R850 made the node graph
authorable (move / connect / delete / add) but every edit was irreversible —
the table-stakes an editor cannot ship without. R851 sits the structural edits
on the `UndoStack` substrate (the undo stack peer): adding a node, deleting a
node together with its incident edges, connecting, and disconnecting each record
a reversible `GraphEdit` delta. `Ctrl+Z` / `Ctrl+Shift+Z` (`Ctrl+Y`) drive it
from the keyboard, and the `UndoStackExternal` at `/node_undo/external` surfaces
the history as data for an AI agent. Stable ids (R841) are the prerequisite: a
redone node keeps its `NodeId`, a restored edge its `EdgeId`.

  (A) boot — the 4-node material graph; the undo history is empty.
  (B) add a node via RPC, then undo (the node + selection revert) and redo (the
      node returns with the SAME stable id) — witnessed in the paint scene.
  (C) connect a wire, then undo / redo the connection.
  (D) delete the central node (3 incident edges); one undo restores the node AND
      every wire it carried, verbatim (stable-id round-trip).
  (E) keyboard — `Ctrl+Z` undoes, `Ctrl+Y` / `Ctrl+Shift+Z` redo.
  (F) a fresh edit after an undo truncates the redo branch (single-branch model);
      a fresh add never reuses an undone node's id (monotonic mint).
  (G) the history is queryable as data (index / count / labels) and `clear`able.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
UNDO = "/node_undo/external"
# PALETTE_W (132) + WIN_W (640), WIN_H (420).
WIN = (132 + 640, 420)
FIRST_DYN = 4  # ids 0..3 are the seed nodes; the first minted id is 4.


def ncount(tf) -> int:
    return tf.query("/external/node_count")


def ecount(tf) -> int:
    return tf.query("/external/edge_count")


def can_undo(tf) -> bool:
    return tf.query(f"{UNDO}/can_undo")


def can_redo(tf) -> bool:
    return tf.query(f"{UNDO}/can_redo")


def undo_label(tf):
    return tf.query(f"{UNDO}/undo_label")


def node_ids(tf) -> list[int]:
    csv = tf.query("/external/node_ids")
    return [int(x) for x in csv.split(",")] if csv else []


def edge_ids(tf) -> list[int]:
    csv = tf.query("/external/edge_ids")
    return [int(x) for x in csv.split(",")] if csv else []


def card_present(tf, node_id: int) -> bool:
    return f"{G}#node_{node_id}" in abs_rects_of(tf.snapshot(source="paint", viewport=WIN))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — clean history ────────────────────────────────
        assert_eq(ncount(tf), 4, "boot: 4 seed nodes")
        assert_eq(ecount(tf), 3, "boot: 3 seed edges")
        assert_eq(can_undo(tf), False, "boot: nothing to undo")
        assert_eq(can_redo(tf), False, "boot: nothing to redo")
        assert_eq(undo_label(tf), None, "boot: no undo label")
        assert_eq(tf.query(f"{UNDO}/index"), 0, "boot: cursor at 0")
        assert_eq(tf.query(f"{UNDO}/count"), 0, "boot: empty history")

        # ── (B) add via RPC → undo → redo (same stable id) ──────────
        new_id = tf.invoke("/external/add_node", "Multiply")
        assert_eq(new_id, FIRST_DYN, "add_node returns the first minted id")
        assert_eq(ncount(tf), 5, "the add grew the graph")
        assert_eq(tf.query("/external/selected"), new_id, "the new node is selected")
        assert_eq(can_undo(tf), True, "the add is journaled")
        assert_eq(undo_label(tf), "Add Multiply", "the next undo is the add")
        wait_until(lambda: card_present(tf, new_id) or None, desc="the added node paints a card")

        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo stepped")
        assert_eq(ncount(tf), 4, "undo removed the node")
        assert_eq(tf.query("/external/selected"), None, "undo reverted the selection")
        assert_eq(can_redo(tf), True, "the add is now redoable")
        wait_until(lambda: (not card_present(tf, new_id)) or None, desc="the card disappeared")

        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo stepped")
        assert_eq(ncount(tf), 5, "redo restored the node")
        assert_eq(tf.query(f"/external/node.{new_id}.title"), "Multiply", "same id, same node")
        assert_eq(tf.query("/external/selected"), new_id, "redo re-selected it")
        assert new_id in node_ids(tf), "the restored id enumerates again"

        # ── (C) connect → undo → redo ───────────────────────────────
        # Wire Texture(0).out0 -> the new Multiply(4).in0 (a free input).
        e_before = ecount(tf)
        assert_eq(tf.invoke("/external/add_edge", f"0,0,{new_id},0"), True, "connect into the new node")
        assert_eq(ecount(tf), e_before + 1, "the wire landed")
        assert_eq(undo_label(tf), "Connect", "the next undo is the connect")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the connect")
        assert_eq(ecount(tf), e_before, "the wire is gone")
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo the connect")
        assert_eq(ecount(tf), e_before + 1, "the wire is back")

        # ── (D) delete the central node + all incident edges ────────
        # Seed node 2 (Multiply) is incident to seed edges 0, 1, 2.
        n_before, ed_before = ncount(tf), ecount(tf)
        assert_eq(tf.invoke("/external/delete_node", 2), True, "delete the central node")
        assert_eq(ncount(tf), n_before - 1, "the node is gone")
        assert_eq(ecount(tf), ed_before - 3, "its three incident edges are gone")
        assert 0 not in edge_ids(tf), "an incident edge id is gone"
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the delete")
        assert_eq(ncount(tf), n_before, "the node is restored")
        assert_eq(ecount(tf), ed_before, "every incident edge is restored")
        assert_eq(tf.query("/external/edge.0"), "0:0->2:0", "a restored edge keeps its id + endpoints")
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo the delete")
        assert_eq(ncount(tf), n_before - 1, "redo removed the node again")

        # ── (E) keyboard Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z ─────────────
        tf.request("focus/set", {"tag": G})
        nc = ncount(tf)
        added = tf.invoke("/external/add_node", "Color")
        assert_eq(ncount(tf), nc + 1, "added a node to drive the keyboard")
        # Ctrl+Z undoes.
        tf.modifiers(ctrl=True)
        tf.key(path=G, name="z")
        tf.modifiers()
        assert_eq(wait_until(lambda: True if ncount(tf) == nc else None, desc="Ctrl+Z undid the add"), True)
        assert added not in node_ids(tf), "Ctrl+Z removed the node"
        # Ctrl+Y redoes.
        tf.modifiers(ctrl=True)
        tf.key(path=G, name="y")
        tf.modifiers()
        assert_eq(wait_until(lambda: True if ncount(tf) == nc + 1 else None, desc="Ctrl+Y redid the add"), True)
        # Ctrl+Z again, then Ctrl+Shift+Z (the redo twin).
        tf.modifiers(ctrl=True)
        tf.key(path=G, name="z")
        tf.modifiers()
        assert_eq(wait_until(lambda: True if ncount(tf) == nc else None, desc="Ctrl+Z undid again"), True)
        tf.modifiers(ctrl=True, shift=True)
        tf.key(path=G, name="Z")
        tf.modifiers()
        assert_eq(
            wait_until(lambda: True if ncount(tf) == nc + 1 else None, desc="Ctrl+Shift+Z redid the add"),
            True,
        )

        # ── (F) redo-branch truncation + monotonic mint ─────────────
        a = tf.invoke("/external/add_node", "Add")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo node a")
        assert_eq(can_redo(tf), True, "a is redoable")
        assert a not in node_ids(tf), "a is currently undone"
        b = tf.invoke("/external/add_node", "Output")  # truncates the redo branch
        assert_eq(can_redo(tf), False, "the fresh edit dropped the redo branch")
        assert b > a, f"a minted id ({b}) never reuses an undone one ({a})"

        # ── (G) history as data + clear ─────────────────────────────
        assert tf.query(f"{UNDO}/count") > 0, "the history holds commands"
        assert tf.query(f"{UNDO}/index") > 0, "the cursor advanced"
        assert_eq(tf.invoke(f"{UNDO}/clear", None), 0, "clear empties the history (cursor 0)")
        assert_eq(can_undo(tf), False, "nothing to undo after clear")
        assert_eq(can_redo(tf), False, "nothing to redo after clear")


if __name__ == "__main__":
    sys.exit(run_demo("R851 §5.38 §5.52 — node-graph undo / redo", body))
