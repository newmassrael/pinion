#!/usr/bin/env python3
"""R1261 §5.38 — node-graph paint scales (O(nodes·edges) cross-scans removed).

A measure-first perf pass: the node paint cross-referenced nodes<->edges with
LINEAR scans — view_node_cards rescanned every edge per node for its wired-input
set, and view_edges linear-found both endpoint nodes per edge — so the paint was
O(nodes·edges), a frame-time cost the self-hosted editor can't afford at large-
graph scale. R1261 precomputes the lookup indices ONCE (O((n+e)·log n)); the
scene output is byte-identical (all existing paint tests still pass). This demo
drives the substrate at a scale well past the 4-node seed graph and verifies it
still builds, wires, and EVALUATES correctly end-to-end:

  (A) build a chain of Add nodes off a Color source, each wired to the previous.
  (B) the graph stays a DAG; the terminal evaluates through the whole chain.
  (C) mid-chain resolved_input + per-node value read correctly at scale.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1261_node_paint_scale.py
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

CHAIN = 24  # Add nodes chained off the seed Color source


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def inv(tf: RpcSubprocess, verb: str, args: Any) -> Any:
    return tf.invoke(f"/external/{verb}", args)


def rgb(v: Any) -> tuple[int, int, int]:
    return (v["r"], v["g"], v["b"])


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        # ── (A) grow the graph: a chain of Add nodes off the Color source (1) ──
        prev = 1  # the seed Color source (grey), a Vector output
        first_add = None
        for i in range(CHAIN):
            nid = inv(tf, "add_node", "Add")
            assert_eq(nid, 4 + i, f"add_node minted node {4 + i}")
            # Wire the previous node's output into this Add's in0.
            assert_eq(inv(tf, "add_edge", f"{prev},0,{nid},0"), True, f"wire {prev} -> {nid}.in0")
            if first_add is None:
                first_add = nid
            prev = nid
        last = prev
        assert_eq(q(tf, "node_count"), 4 + CHAIN, "the graph grew by the whole chain")
        assert_eq(q(tf, "edge_count"), 3 + CHAIN, "one new wire per chained node")

        # ── (B) still a DAG; it evaluates through the whole chain ─────
        assert_eq(q(tf, "eval.acyclic"), True, "a deep chain is still a DAG")
        assert_eq(q(tf, "eval.cycle_nodes"), "", "no cycles")
        # Each Add sums its wired in0 with its grey (0x80) in1 default, saturating;
        # after the first couple of links the chain pins white. The terminal (the
        # seed Output <- Multiply, untouched) is unaffected by the new branch.
        assert_eq(rgb(q(tf, f"node.{last}.value")), (255, 255, 255), "the chain end saturates to white")
        assert_eq(q(tf, f"node.{last}.op"), "Add", "the chain end is an Add")
        assert_eq(rgb(q(tf, "eval.output")), (64, 64, 64), "the seed terminal is untouched")

        # ── (C) per-node reads are correct at scale ──────────────────
        assert_eq(q(tf, f"node.{first_add}.op"), "Add", "the first chained node is an Add")
        assert_eq(q(tf, f"node.{first_add}.is_source"), False, "an Add is not a source")
        # first_add.in0 is wired from the Color source (grey); in1 is its default.
        assert_eq(rgb(q(tf, f"node.{first_add}.resolved_input.0")), (0x80, 0x80, 0x80), "in0 <- Color grey")
        assert_eq(rgb(q(tf, f"node.{first_add}.resolved_input.1")), (0x80, 0x80, 0x80), "in1 grey default")
        # A mid-chain node is wired (its in0 default is hidden) and evaluates white.
        mid = first_add + CHAIN // 2
        assert_eq(q(tf, f"node.{mid}.op"), "Add", "mid-chain is an Add")
        assert_eq(rgb(q(tf, f"node.{mid}.value")), (255, 255, 255), "mid-chain already saturated")
        assert_eq(q(tf, f"node.{mid}.inputs"), 2, "an Add has 2 input ports")
        assert_eq(q(tf, f"node.{mid}.outputs"), 1, "and 1 output port")
        assert_eq(q(tf, f"node.{mid}.is_reroute"), False, "a chained Add is not a reroute")
        assert_eq(rgb(q(tf, f"node.{mid}.resolved_input.0")), (255, 255, 255), "mid.in0 <- prev (white)")
        assert_eq(rgb(q(tf, f"node.{mid}.resolved_input.1")), (0x80, 0x80, 0x80), "mid.in1 grey default")
        # detail.* mirrors a selected chain node at scale.
        tf.intervene("/external/selected_ids", str(mid))
        assert_eq(q(tf, "detail.op"), "Add", "detail.op mirrors the selected chain node")
        assert_eq(rgb(q(tf, "detail.value")), (255, 255, 255), "detail.value mirrors it")
        # The chain's edges enumerate + read back.
        assert_eq(q(tf, "reroute_ids"), "", "no reroutes in the chain")
        eids = q(tf, "edge_ids").split(",")
        assert_eq(len(eids), 3 + CHAIN, "edge_ids enumerates every wire")
        # The seed sources are still authorable at scale.
        assert_eq(q(tf, "node.0.is_source"), True, "Texture still a source")
        assert_eq(q(tf, "node.1.is_source"), True, "Color still a source")
        # The full node id set is intact (no drops from the paint index change).
        ids = q(tf, "node_ids").split(",")
        assert_eq(len(ids), 4 + CHAIN, "node_ids enumerates the whole graph")

        # The seed graph still round-trips its serialized snapshot after the growth.
        blob = q(tf, "serialized")
        assert_eq(inv(tf, "set_graph", blob), True, "the grown graph re-loads (round-trip valid)")
        assert_eq(q(tf, "node_count"), 4 + CHAIN, "count preserved through the round-trip")


if __name__ == "__main__":
    sys.exit(run_demo("r1261_node_paint_scale", body))
