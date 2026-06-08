#!/usr/bin/env python3
"""R838 §5.38 §5.40 §5.51 — node-graph editor substrate.

A canvas of titled nodes (each with input / output ports) connected by
cubic-bezier edges — the visual-scripting / material-graph form factor and the
self-hosted editor's blueprint-panel seed. Pure composition of one
`NodeGraphExternal` coordinator over existing substrate: the R51.34 capture
lock (node drag-to-move), the R742 drag substrate (port drag-to-connect), the
R721 vector-path nodes (bezier edges), `absolute_position` (free placement),
and `Owner::cache` Signals (the shared model).

Coordinator slots (`node_graph`, the primary external):
  /external/node_count             -> 4
  /external/edge_count             -> 3
  /external/selected               -> null | int
  /external/node.<i>.title         -> node title
  /external/node.<i>.{x,y}         -> canvas position (intervene moves it)
  /external/node.<i>.{inputs,outputs} -> port arity (read-only)
  /external/edge.<i>               -> "from_node:from_port->to_node:to_port"
  /external/add_edge               -> invoke "fn,fp,tn,tp" (string)
  /external/remove_edge            -> invoke <edge index> (int)
  /external/delete_node            -> invoke <node index> (int)
  /external/delete_selected        -> invoke (null)
  /external/nudge                  -> invoke "dx,dy" (string)

R839 adds edge selection: click a wire (point-to-bezier hit-test via the
R51.34 capture-seed click coords) to select it, Delete to remove it; node and
edge selection are mutually exclusive.

Verified (>= 30 assertions):
  (A) boot taxonomy — 4 nodes / 3 edges, titles, port arity, seed wiring
  (A2) click-select a wire (R839 bezier hit-test) + delete_selected + restore
  (B) RPC node move — intervene node.<i>.{x,y}; off-canvas clamps
  (C) selection — click selects; empty-canvas click + intervene clear it
  (D) RPC edge edit — add_edge validates (self-loop / range / input dedup),
      remove_edge by index
  (E) live node drag — scene/drag a node body moves it (R51.34 capture)
  (F) live edge connect — scene/drag an output port onto an input port adds
      an edge (R742 drag substrate)
  (G) keyboard — arrows nudge the selected node, Delete removes it
  (H) delete_node — incident edges drop, survivors reindex
  (I) paint — nodes, ports, and bezier edges all render
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (640, 420)
PAUSE = 0.10

G = "node_graph"


def _focus_graph(tf) -> None:
    tf.request("focus/set", {"tag": G})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == G,
        timeout=4.0,
        interval=0.03,
        desc="graph owns keyboard focus",
    )


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, G) is not None, "graph canvas present"
        assert_eq(tf.query("/external/node_count"), 4, "4 nodes")
        assert_eq(tf.query("/external/edge_count"), 3, "3 edges")
        assert_eq(tf.query("/external/selected"), None, "nothing selected")
        assert_eq(tf.query("/external/selected_edge"), None, "no edge selected")
        assert_eq(tf.query("/external/node.0.title"), "Texture", "node 0 title")
        assert_eq(tf.query("/external/node.2.title"), "Multiply", "node 2 title")
        assert_eq(tf.query("/external/node.2.inputs"), 2, "Multiply has 2 inputs")
        assert_eq(tf.query("/external/node.2.outputs"), 1, "Multiply has 1 output")
        assert_eq(tf.query("/external/node.3.outputs"), 0, "Output is a sink")
        assert_eq(tf.query("/external/edge.0"), "0:0->2:0", "Texture -> Multiply.in0")
        assert_eq(tf.query("/external/edge.1"), "1:0->2:1", "Color -> Multiply.in1")
        assert_eq(tf.query("/external/edge.2"), "2:0->3:0", "Multiply -> Output.in0")

        # ── (A2) click-select a wire (R839 bezier hit-test) + delete ─
        # Edge 0 (Texture.out0 -> Multiply.in0) bows through ~(210, 134),
        # open space between the two cards. Clicking there selects the wire.
        tf.click(at=(210.0, 134.0))
        wait_until(lambda: tf.query("/external/selected_edge") == 0, timeout=4.0,
                   interval=0.03, desc="clicking the wire selects edge 0")
        assert_eq(tf.query("/external/selected"), None, "node selection cleared (mutual exclusion)")
        # delete_selected removes the selected edge (the Delete-key path).
        assert_eq(tf.invoke("/external/delete_selected", None), True, "delete selected edge")
        wait_until(lambda: tf.query("/external/edge_count") == 2, timeout=4.0,
                   interval=0.03, desc="wire removed")
        assert_eq(tf.query("/external/selected_edge"), None, "edge selection cleared")
        # Restore the wire so later sections see the seed topology.
        assert_eq(tf.invoke("/external/add_edge", "0,0,2,0"), True, "re-add the wire")
        wait_until(lambda: tf.query("/external/edge_count") == 3, timeout=4.0,
                   interval=0.03, desc="3 edges again")
        # A click on empty canvas clears any selection.
        tf.click(at=(615.0, 400.0))
        wait_until(lambda: tf.query("/external/selected_edge") is None
                   and tf.query("/external/selected") is None, timeout=4.0,
                   interval=0.03, desc="empty-canvas click clears selection")

        # ── (B) RPC node move (the AI-first path) + clamp ────────────
        tf.intervene("/external/node.0.x", 180)
        tf.intervene("/external/node.0.y", 90)
        assert_eq(tf.query("/external/node.0.x"), 180, "node 0 moved x")
        assert_eq(tf.query("/external/node.0.y"), 90, "node 0 moved y")
        tf.intervene("/external/node.0.x", 99999)  # off-canvas
        clamped_x = tf.query("/external/node.0.x")
        assert clamped_x < 640, f"off-canvas x clamps into the window ({clamped_x})"
        tf.intervene("/external/node.0.x", 40)  # restore

        # ── (C) selection: click selects, empty click / intervene clear ─
        tf.click(path=f"{G}#node_2")
        wait_until(lambda: tf.query("/external/selected") == 2, timeout=4.0,
                   interval=0.03, desc="click selects node 2")
        tf.click(at=(615, 400))  # empty canvas corner
        wait_until(lambda: tf.query("/external/selected") is None, timeout=4.0,
                   interval=0.03, desc="empty-canvas click deselects")
        tf.intervene("/external/selected", 1)
        assert_eq(tf.query("/external/selected"), 1, "intervene selects node 1")
        tf.intervene("/external/selected", None)
        assert_eq(tf.query("/external/selected"), None, "intervene clears selection")

        # ── (D) RPC edge edit — validation + remove ──────────────────
        assert_eq(tf.invoke("/external/add_edge", "2,0,2,0"), False, "self-loop rejected")
        assert_eq(tf.invoke("/external/add_edge", "0,9,3,0"), False, "bad port rejected")
        # New wire into Output's single input replaces edge 2's target (input
        # takes one wire), so the count stays 3.
        assert_eq(tf.invoke("/external/add_edge", "0,0,3,0"), True, "valid edge added")
        assert_eq(tf.query("/external/edge_count"), 3, "input single-wire dedup keeps 3")
        assert_eq(tf.query("/external/edge.2"), "0:0->3:0", "Output input rewired")
        assert_eq(tf.invoke("/external/remove_edge", 2), True, "remove edge 2")
        assert_eq(tf.query("/external/edge_count"), 2, "now 2 edges")
        assert_eq(tf.invoke("/external/remove_edge", 9), False, "out-of-range remove rejected")

        # ── (E) live node drag (R51.34 capture lock) ─────────────────
        x_before = tf.query("/external/node.1.x")
        y_before = tf.query("/external/node.1.y")
        tf.drag(from_path=f"{G}#node_1", to_at=(330.0, 330.0), steps=10)
        wait_until(lambda: tf.query("/external/node.1.x") != x_before
                   or tf.query("/external/node.1.y") != y_before,
                   timeout=4.0, interval=0.05, desc="node 1 moved under a live drag")
        assert tf.query("/external/node.1.x") > x_before + 40, "node dragged right"
        assert tf.query("/external/node.1.y") > y_before + 40, "node dragged down"

        # ── (F) live edge connect (R742 drag substrate) ──────────────
        # Output's input was left unwired by (D). Reconnect Multiply.out0 ->
        # Output.in0 by dragging the output port onto the input port
        # (edge_count 2 -> 3). Nodes 2 / 3 are at their seed positions (only
        # node 1 moved in (E)), so the seeded port geometry holds.
        tf.drag(from_path=f"{G}#oport_2_0", to_path=f"{G}#iport_3_0", steps=12)
        wait_until(lambda: tf.query("/external/edge_count") == 3, timeout=4.0,
                   interval=0.05, desc="port drag created an edge")
        # The new edge connects Multiply (2) output 0 to Output (3) input 0.
        edges = [tf.query(f"/external/edge.{i}") for i in range(3)]
        assert "2:0->3:0" in edges, f"Multiply->Output wire present ({edges})"

        # ── (G) keyboard nudge + delete the selected node ────────────
        _focus_graph(tf)
        tf.intervene("/external/selected", 0)
        nx = tf.query("/external/node.0.x")
        tf.key(path=G, name="ArrowRight")
        wait_until(lambda: tf.query("/external/node.0.x") == nx + 12, timeout=4.0,
                   interval=0.03, desc="ArrowRight nudges selected node by 12")
        tf.key(path=G, name="ArrowDown")
        ny = tf.query("/external/node.0.y")
        assert ny is not None, "node still present after nudge"
        # Delete removes the selected node + its incident edges.
        tf.key(path=G, name="Delete")
        wait_until(lambda: tf.query("/external/node_count") == 3, timeout=4.0,
                   interval=0.03, desc="Delete removes the selected node")
        assert_eq(tf.query("/external/selected"), None, "selection cleared after delete")

        # ── (H) delete_node reindex (RPC) ────────────────────────────
        before = tf.query("/external/node_count")
        assert_eq(tf.invoke("/external/delete_node", before), False, "out-of-range delete rejected")
        assert_eq(tf.invoke("/external/delete_node", 0), True, "delete node 0")
        assert_eq(tf.query("/external/node_count"), before - 1, "node removed")

        # ── (I) paint: nodes, ports, and bezier edges all render ─────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        nodes_now = tf.query("/external/node_count")
        for i in range(nodes_now):
            assert find_by_tag(snap, f"{G}#node_{i}") is not None, f"node {i} card painted"
        edges_now = tf.query("/external/edge_count")
        assert edges_now >= 1, "at least one edge remains"
        assert find_by_tag(snap, f"{G}#edge_0") is not None, "edge 0 bezier painted"
        time.sleep(PAUSE)


if __name__ == "__main__":
    sys.exit(run_demo("hello-node-editor R838 §5.38 node-graph editor", body))
