#!/usr/bin/env python3
"""R1235 §5.38 §5.52 — node-graph REROUTE node (wire splice).

A dense graph reads better when a long wire is bent around obstacles. R1235 adds
the Blueprint / material-editor "reroute" (knot): splice a typed 1-in / 1-out
passthrough into an edge so it routes A -> R -> B instead of A -> B, then drag R
to route the wire. All AI-first over the §5.12 plane (§2 #2), no pixels:

  * `invoke add_reroute <edge_id>` removes the edge, drops a reroute node at the
    wire midpoint, and re-wires A -> R -> B in ONE undoable step; returns the new
    node id (`Null` for an unknown edge).
  * The reroute adopts the wire's PortType on BOTH ports, so A -> R and R -> B
    are assignable exactly when A -> B was (the splice never weakens type-safety).

  (A) boot: 4 nodes, 3 edges; edge 0 = node0 -> node2.in0 (a Vector wire).
  (B) splice a reroute into edge 0: +1 node, net +1 edge, edge 0 gone.
  (C) the path now routes node0 -> R -> node2; R is a typed Vector passthrough.
  (D) one undo removes the whole reroute and restores edge 0; redo re-splices.
  (E) type adoption: a Scalar -> Lerp FLOAT wire reroutes to a Float passthrough.
  (F) rejects: an unknown edge id is Null; the verb is schema-declared.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1235_node_reroute.py
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
    wait_until,
)

UNDO = "/node_undo/external"


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def add_node(tf: RpcSubprocess, kind: str) -> Any:
    return tf.invoke("/external/add_node", kind)


def add_reroute(tf: RpcSubprocess, edge: int) -> Any:
    return tf.invoke("/external/add_reroute", edge)


def undo(tf: RpcSubprocess) -> bool:
    return tf.invoke(f"{UNDO}/undo", None)


def redo(tf: RpcSubprocess) -> bool:
    return tf.invoke(f"{UNDO}/redo", None)


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        assert_eq(q(tf, "edge_count"), 3, "3 seed edges")
        assert_eq(q(tf, "edge.0"), "0:0->2:0", "edge 0 = node0.out0 -> node2.in0")

        # ── (B) splice a reroute into edge 0 ─────────────────────────
        rid = add_reroute(tf, 0)
        assert_eq(rid, 4, "the reroute mints node id 4")
        assert_eq(q(tf, "node_count"), 5, "one new node (the reroute)")
        assert_eq(q(tf, "edge_count"), 4, "net +1 edge (removed 1, added 2)")
        assert str(rid) in q(tf, "node_ids").split(","), "reroute is enumerated"
        # The original edge id is gone from the enumeration.
        assert "0" not in q(tf, "edge_ids").split(","), "edge 0 removed"

        # ── (C) the path routes node0 -> R -> node2, typed Vector ────
        edges = {eid: q(tf, f"edge.{eid}") for eid in q(tf, "edge_ids").split(",")}
        wires = set(edges.values())
        assert f"0:0->{rid}:0" in wires, "node0 now feeds the reroute"
        assert f"{rid}:0->2:0" in wires, "the reroute feeds node2's original input"
        assert_eq(q(tf, f"node.{rid}.title"), "Reroute", "titled Reroute")
        assert_eq(q(tf, f"node.{rid}.inputs"), 1, "exactly one input port")
        assert_eq(q(tf, f"node.{rid}.outputs"), 1, "exactly one output port")
        assert_eq(q(tf, f"node.{rid}.input_types"), "Vector", "input adopts wire type")
        assert_eq(q(tf, f"node.{rid}.output_types"), "Vector", "output adopts wire type")

        # ── (D) one undo reverts the whole splice; redo re-applies ───
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Insert reroute", "one labelled step")
        assert_eq(undo(tf), True, "undo the splice")
        assert_eq(q(tf, "node_count"), 4, "the reroute node is gone")
        assert_eq(q(tf, "edge_count"), 3, "back to 3 edges")
        assert_eq(q(tf, "edge.0"), "0:0->2:0", "edge 0 restored verbatim")
        assert_eq(redo(tf), True, "redo re-splices")
        assert_eq(q(tf, "node_count"), 5, "the reroute is back")
        assert_eq(undo(tf), True, "undo again to a clean baseline")
        assert_eq(q(tf, "node_count"), 4, "baseline restored")

        # ── (E) type adoption: a FLOAT wire reroutes to a Float knot ──
        scalar = add_node(tf, "Scalar")   # Float source
        lerp = add_node(tf, "Lerp")       # port 2 is a Float factor input
        wait_until(lambda: True if q(tf, "node_count") == 6 else None,
                   desc="Scalar + Lerp added")
        assert_eq(
            tf.invoke("/external/add_edge", f"{scalar},0,{lerp},2"),
            True,
            "Float -> Float wired",
        )
        float_eid = next(
            int(e) for e in q(tf, "edge_ids").split(",")
            if q(tf, f"edge.{e}").startswith(f"{scalar}:0->")
        )
        frid = add_reroute(tf, float_eid)
        assert isinstance(frid, int), "the float wire splices"
        assert_eq(q(tf, f"node.{frid}.title"), "Reroute", "the float knot is titled")
        assert_eq(q(tf, f"node.{frid}.input_types"), "Float", "Float wire -> Float in")
        assert_eq(q(tf, f"node.{frid}.output_types"), "Float", "Float wire -> Float out")
        fwires = {q(tf, f"edge.{e}") for e in q(tf, "edge_ids").split(",")}
        assert f"{scalar}:0->{frid}:0" in fwires, "Scalar now feeds the float knot"
        assert f"{frid}:0->{lerp}:2" in fwires, "the float knot feeds Lerp's factor"

        # ── (F) rejects ──────────────────────────────────────────────
        assert_eq(add_reroute(tf, 99), None, "an unknown edge id splices nothing")


if __name__ == "__main__":
    sys.exit(run_demo("R1235 node-graph reroute (wire splice)", body))
