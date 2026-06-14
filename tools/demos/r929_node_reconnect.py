#!/usr/bin/env python3
"""R929 §5.52 §5.38 — node-graph edge reconnection, over RPC.

A node editor's canonical edit is re-wiring: grab a connected input and drop
its wire on a different input, keeping the source. R929 adds the AI-first peer
of that gesture — `invoke reconnect_edge "edge,to_node,to_port"` — as ONE
atomic undo step (the old edge removed + a fresh edge `same-source -> new-input`
added in a single `Ctrl+Z`), validated through the same `validate_connection`
SSOT as `add_edge` (self-loop / typed-port / single-wire-into-input). An AI
agent drives and reads the whole round-trip over the §5.12 RPC plane (§2 #2).

The seed graph is `Texture x Color -> Multiply -> Output` (all `Vector`
ports), edges 0=`0:0->2:0`, 1=`1:0->2:1`, 2=`2:0->3:0`.

  (A) boot taxonomy — 4 nodes / 3 edges / empty undo history.
  (B) free Multiply.in1 (remove edge 1) so a clean reconnect has somewhere
      to land.
  (C) reconnect edge 0's target 2:0 -> 2:1 — source preserved, edge count
      unchanged (a rewire, not an add), the wire mints a fresh id.
  (D) one undo restores the original wiring; redo re-wires it.
  (E) rejects — self-loop, unknown edge id; a re-drop on the same input is a
      no-op success that adds no undo step.
  (F) reconnect onto an OCCUPIED input displaces the resident wire; one undo
      restores BOTH.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r929_node_reconnect.py
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

VIEWPORT = (132 + 640, 420)
G = "node_graph"
UNDO = "/node_undo/external"


def edge_count(tf) -> int:
    return tf.query("/external/edge_count")


def edge_ids(tf) -> list[int]:
    csv = tf.query("/external/edge_ids")
    return [int(x) for x in csv.split(",")] if csv else []


def conns(tf) -> dict[int, str]:
    """Live edge id -> "from:fp->to:tp" connection string."""
    return {eid: tf.query(f"/external/edge.{eid}") for eid in edge_ids(tf)}


def edge_id_of(tf, conn: str) -> int | None:
    """The stable id of the edge with connection `conn` (robust to id minting)."""
    return next((eid for eid, c in conns(tf).items() if c == conn), None)


def has_conn(tf, conn: str) -> bool:
    return conn in conns(tf).values()


def reconnect(tf, edge: int, to_node: int, to_port: int) -> bool:
    return tf.invoke("/external/reconnect_edge", f"{edge},{to_node},{to_port}")


def remove_edge(tf, edge: int) -> bool:
    return tf.invoke("/external/remove_edge", edge)


def add_edge(tf, spec: str) -> bool:
    return tf.invoke("/external/add_edge", spec)


def count(tf) -> int:
    return tf.query(f"{UNDO}/count")


def undo(tf) -> bool:
    return tf.invoke(f"{UNDO}/undo", None)


def redo(tf) -> bool:
    return tf.invoke(f"{UNDO}/redo", None)


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, G) is not None, "graph canvas present"
        assert_eq(tf.query("/external/node_count"), 4, "4 nodes")
        assert_eq(edge_count(tf), 3, "3 seed edges")
        assert_eq(tf.query("/external/edge.0"), "0:0->2:0", "Texture -> Multiply.in0")
        assert_eq(tf.query("/external/edge.1"), "1:0->2:1", "Color -> Multiply.in1")
        assert_eq(count(tf), 0, "empty undo history at boot")

        # ── (B) free Multiply.in1 so a clean reconnect can land there ─
        assert_eq(remove_edge(tf, 1), True, "disconnect edge 1 to free Multiply.in1")
        assert_eq(edge_count(tf), 2, "two edges after the disconnect")
        assert_eq(count(tf), 1, "the disconnect is one undo step")

        # ── (C) reconnect edge 0's target 2:0 -> 2:1 (the headline) ───
        assert_eq(reconnect(tf, 0, 2, 1), True, "reconnect edge 0 to Multiply.in1")
        assert_eq(edge_count(tf), 2, "a rewire keeps the edge count (not an add)")
        assert not has_conn(tf, "0:0->2:0"), "the old target is gone"
        assert has_conn(tf, "0:0->2:1"), "the wire now lands on Multiply.in1, source preserved"
        assert edge_id_of(tf, "0:0->2:1") != 0, "the reconnected wire minted a fresh edge id"
        assert_eq(count(tf), 2, "the reconnect added exactly one undo step")

        # ── (D) undo restores the original wiring; redo re-wires it ───
        assert_eq(undo(tf), True, "undo the reconnect")
        assert has_conn(tf, "0:0->2:0"), "one undo restored the original target"
        assert not has_conn(tf, "0:0->2:1"), "the reconnected wire is gone"
        assert_eq(tf.query("/external/edge.0"), "0:0->2:0", "edge 0 restored verbatim (stable id)")
        assert_eq(redo(tf), True, "redo re-wires it")
        assert has_conn(tf, "0:0->2:1"), "redo re-applied the reconnect"

        # ── (E) rejects + same-target no-op ──────────────────────────
        eid = edge_id_of(tf, "0:0->2:1")
        assert eid is not None, "the live reconnected edge id"
        steps = count(tf)
        assert_eq(reconnect(tf, eid, 0, 0), False, "self-loop (target on the source node) rejected")
        assert_eq(reconnect(tf, 999, 2, 0), False, "unknown edge id rejected")
        assert has_conn(tf, "0:0->2:1"), "a rejected reconnect leaves the wire unchanged"
        assert_eq(reconnect(tf, eid, 2, 1), True, "re-drop on its own input is a no-op success")
        assert_eq(count(tf), steps, "the no-op reconnect added no undo step")
        assert has_conn(tf, "0:0->2:1"), "still the same wire after the no-op"

        # ── (F) reconnect onto an OCCUPIED input displaces the resident ─
        # Re-occupy Multiply.in0 with Color (node 1), then reconnect the node-0
        # wire onto it -> displaces the Color wire; one undo restores both.
        assert_eq(add_edge(tf, "1,0,2,0"), True, "Color -> Multiply.in0 (re-occupy in0)")
        assert_eq(edge_count(tf), 3, "three edges before the displacing reconnect")
        eid = edge_id_of(tf, "0:0->2:1")
        assert_eq(reconnect(tf, eid, 2, 0), True, "reconnect node 0's wire onto the occupied in0")
        assert has_conn(tf, "0:0->2:0"), "node 0's wire moved to Multiply.in0"
        assert not has_conn(tf, "1:0->2:0"), "the resident Color wire was displaced"
        assert_eq(edge_count(tf), 2, "one removed-target + one displaced, one added")
        assert_eq(undo(tf), True, "one undo reverses the whole displacing reconnect")
        assert has_conn(tf, "0:0->2:1"), "node 0's wire restored to Multiply.in1"
        assert has_conn(tf, "1:0->2:0"), "the displaced Color wire is restored too"
        assert_eq(edge_count(tf), 3, "both wires are back")


if __name__ == "__main__":
    sys.exit(run_demo("R929 node-graph edge reconnection", body))
