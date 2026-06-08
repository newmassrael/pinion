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

R841 makes node/edge identity stable: nodes and edges are addressed by stable
ids (`node.<id>` / `edge.<id>`), enumerated via `node_ids` / `edge_ids`. Delete
drops only the target + its incident edges — survivors keep their ids (no
reindex), so an edge / selection reference survives an unrelated delete.

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


def _ids(tf, which: str) -> list[int]:
    """Current stable node / edge id list (R841 — addressing is by id, and the
    id space is sparse after deletes)."""
    csv = tf.query(f"/external/{which}")
    return [int(x) for x in csv.split(",")] if csv else []


def _edge_conns(tf) -> dict[int, str]:
    """Map each live edge id → its "from:port->to:port" connection string."""
    return {eid: tf.query(f"/external/edge.{eid}") for eid in _ids(tf, "edge_ids")}


def _edge_id_of(tf, conn: str) -> int | None:
    """The stable id of the edge with connection string `conn`, if present."""
    return next((eid for eid, c in _edge_conns(tf).items() if c == conn), None)


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
        # R841 stable-id enumeration handles.
        assert_eq(tf.query("/external/node_ids"), "0,1,2,3", "node id space")
        assert_eq(tf.query("/external/edge_ids"), "0,1,2", "edge id space")

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

        # ── (D) RPC edge edit — validation + remove (by stable id) ───
        assert_eq(tf.invoke("/external/add_edge", "2,0,2,0"), False, "self-loop rejected")
        assert_eq(tf.invoke("/external/add_edge", "0,9,3,0"), False, "bad port rejected")
        # New wire into Output's single input replaces the existing wire there
        # (input takes one wire) — count stays 3, the new edge mints a fresh id.
        assert_eq(tf.invoke("/external/add_edge", "0,0,3,0"), True, "valid edge added")
        assert_eq(tf.query("/external/edge_count"), 3, "input single-wire dedup keeps 3")
        conns = set(_edge_conns(tf).values())
        assert "0:0->3:0" in conns, f"Output input rewired ({conns})"
        assert "2:0->3:0" not in conns, "old Multiply->Output wire replaced"
        new_eid = _edge_id_of(tf, "0:0->3:0")
        assert new_eid is not None, "new edge id resolvable"
        assert_eq(tf.invoke("/external/remove_edge", new_eid), True, "remove that edge by id")
        assert_eq(tf.query("/external/edge_count"), 2, "now 2 edges")
        assert_eq(tf.invoke("/external/remove_edge", 999), False, "unknown edge id rejected")

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
        conns = set(_edge_conns(tf).values())
        assert "2:0->3:0" in conns, f"Multiply->Output wire present ({conns})"

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

        # ── (H) delete_node by stable id; survivors keep their ids ───
        node_ids = _ids(tf, "node_ids")  # node 0 was deleted in (G) → sparse
        before = len(node_ids)
        assert_eq(tf.invoke("/external/delete_node", 999), False, "unknown node id rejected")
        victim = node_ids[0]
        assert_eq(tf.invoke("/external/delete_node", victim), True, f"delete node id {victim}")
        assert_eq(tf.query("/external/node_count"), before - 1, "node removed")
        assert victim not in _ids(tf, "node_ids"), "deleted id not reused"

        # ── (I) paint: nodes, ports, and bezier edges all render ─────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        for nid in _ids(tf, "node_ids"):
            assert find_by_tag(snap, f"{G}#node_{nid}") is not None, f"node {nid} card painted"
        edge_ids = _ids(tf, "edge_ids")
        assert edge_ids, "at least one edge remains"
        assert find_by_tag(snap, f"{G}#edge_{edge_ids[0]}") is not None, "a bezier edge painted"
        time.sleep(PAUSE)


if __name__ == "__main__":
    sys.exit(run_demo("hello-node-editor R838 §5.38 node-graph editor", body))
