#!/usr/bin/env python3
"""R1236 §5.38 §5.52 — node-graph DISSOLVE (delete + reconnect).

The inverse of R1235's reroute splice: deleting a reroute knot should not orphan
the wire, it should rejoin the endpoints (the Blueprint "delete + reconnect" /
Alt+Delete). R1236 adds `dissolve_node`: remove a 1-in / 1-out node and bridge
its upstream source straight to its downstream target, in ONE undoable step.
All AI-first over the §5.12 plane (§2 #2), no pixels:

  * `invoke dissolve_node <id>` dissolves node `<id>` (delete + reconnect A -> B),
    returning True; `dissolve_selected` (no arg) dissolves the lone selected node.
  * A dissolve is a no-op (False) for a non-passthrough wiring (zero or many
    edges on either side) or an invalid bridge — the caller falls back to delete.

  (A) boot: 4 nodes, 3 edges.
  (B) splice a reroute into edge 0 (node0 -> R -> node2), then dissolve R:
      the wire node0 -> node2 is rejoined; node + edge counts return to baseline.
  (C) one undo restores the whole hop; redo dissolves again.
  (D) gate: a two-input node (Multiply) and the source / sink nodes do NOT
      dissolve (no unambiguous single hop).

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1236_node_dissolve.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

UNDO = "/node_undo/external"


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def dissolve(tf: RpcSubprocess, node: int) -> Any:
    return tf.invoke("/external/dissolve_node", node)


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        assert_eq(q(tf, "edge_count"), 3, "3 seed edges")

        # ── (B) splice a reroute, then dissolve it ───────────────────
        rid = tf.invoke("/external/add_reroute", 0)
        assert_eq(rid, 4, "reroute minted node 4")
        assert_eq(q(tf, "node_count"), 5, "the hop node0 -> R -> node2 exists")
        # R1241 — the eligibility read (no mutate-to-probe): only the reroute is
        # dissolvable; the read predicts the verb.
        assert_eq(q(tf, f"dissolvable.{rid}"), True, "the reroute reads as dissolvable")
        # ★R1596 — `dissolvable` widened from a SHAPE test ("exactly one wire
        # in and one out") to LOSSLESSNESS. `Document::dissolve` is the general
        # form (Blender's NODE_OT_delete_reconnect), so what is worth asking is
        # no longer *can it* but *does it lose anything* -- and a Multiply
        # routes its output from input 0, so the wire past it is bridged and
        # nothing is cut.
        assert_eq(q(tf, "dissolvable.2"), True, "a Multiply loses nothing either")
        assert_eq(q(tf, "dissolve_severs.2"), "", "its cut list is empty")
        # A SOURCE is the lossy one: nothing flows into it, so its output has no
        # input to be routed from and the wire leaving it dies -- NAMED, where
        # `node_internal_relink` deletes it and returns void.
        assert_eq(q(tf, "dissolvable.0"), False, "a source's dissolve is lossy")
        assert q(tf, "dissolve_severs.0"), "and it names the wire it would cut"
        # The enumeration follows the widened predicate: every node whose
        # dissolve costs nothing, which is now the reroute AND the pass-through
        # compute nodes -- and NOT the sources, whose outgoing wire would die.
        enumerated = q(tf, "dissolvable_ids").split(",")
        assert str(rid) in enumerated, f"the reroute is in it: {enumerated}"
        assert "0" not in enumerated and "1" not in enumerated, (
            f"and the two sources are not: {enumerated}"
        )
        assert_eq(dissolve(tf, rid), True, "the reroute dissolves")
        assert_eq(q(tf, "node_count"), 4, "the reroute node is removed")
        assert_eq(q(tf, "edge_count"), 3, "net -1 edge (removed 2, added 1 bridge)")
        assert str(rid) not in q(tf, "node_ids").split(","), "the reroute is de-enumerated"
        assert_eq(q(tf, "node.0.title"), "Texture", "node0 survives the dissolve")
        assert_eq(q(tf, "node.2.title"), "Multiply", "node2 survives the dissolve")
        # The wire node0.out0 -> node2.in0 is reconnected (a fresh bridge edge id).
        wires = {q(tf, f"edge.{e}") for e in q(tf, "edge_ids").split(",")}
        assert "0:0->2:0" in wires, "node0 -> node2.in0 is bridged back"

        # ── (C) one undo restores the hop; redo dissolves again ──────
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Dissolve node", "one labelled step")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo restores the hop")
        assert_eq(q(tf, "node_count"), 5, "the reroute is back")
        assert_eq(q(tf, "edge_count"), 4, "and its two edges")
        assert_eq(q(tf, f"node.{rid}.title"), "Reroute", "the restored knot is intact")
        assert_eq(q(tf, f"node.{rid}.input_types"), "Vector", "its typed port too")
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo dissolves again")
        assert_eq(q(tf, "node_count"), 4, "back to baseline")
        assert_eq(q(tf, "edge_count"), 3, "edges back to baseline")

        # ── (D) dissolve a second reroute on another edge (edge 1) ───
        rid2 = tf.invoke("/external/add_reroute", 1)
        assert_eq(q(tf, "node_count"), 5, "a reroute on edge 1 (node1 -> node2.in1)")
        assert_eq(dissolve(tf, rid2), True, "it dissolves too")
        assert_eq(q(tf, "node_count"), 4, "removed")
        wires2 = {q(tf, f"edge.{e}") for e in q(tf, "edge_ids").split(",")}
        assert "1:0->2:1" in wires2, "node1 -> node2.in1 is bridged back"

        # ── (E) ★ the gate is LOSSLESSNESS, not a shape ──────────────
        # R1596 — `Document::dissolve` is the general form, so the only thing a
        # dissolve refuses is a node that is not there. What used to be the
        # gate ("exactly one hop") is now a QUESTION the caller asks first, and
        # the answer names the cost.
        assert_eq(q(tf, "node.2.inputs"), 2, "Multiply has two inputs")
        assert_eq(dissolve(tf, 99), False, "an unknown id has no dissolve at all")
        assert_eq(q(tf, "node_count"), 4, "and left the graph intact")
        # A source: lossy, and it says which wire dies -- then the operation is
        # run and exactly that wire is gone, so the prediction is CHECKED.
        cut = q(tf, "dissolve_severs.0")
        assert cut and cut in q(tf, "edge_ids").split(","), f"the named wire exists: {cut}"
        assert_eq(dissolve(tf, 0), True, "a source dissolves -- at a cost it named")
        assert cut not in q(tf, "edge_ids").split(","), "and cut exactly that wire"
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "one undo restores it")
        assert_eq(q(tf, "node_count"), 4, "the graph is back")
        assert_eq(q(tf, "edge_count"), 3, "edge count too")


if __name__ == "__main__":
    sys.exit(run_demo("R1236 node-graph dissolve (delete + reconnect)", body))
