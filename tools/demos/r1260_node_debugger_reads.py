#!/usr/bin/env python3
"""R1260 §5.38 §2 #7 — node-graph DEBUGGER reads (per-input resolved values + cycle localisation).

A session audit found the eval introspection incomplete for the stated
visual-scripting-debugger use: to learn what actually flowed into a node an AI
had to find the edge, read the source's `value`, and re-apply the coercion by
hand; and a `null` value was un-localisable (`eval.acyclic` is one whole-graph
bool). R1260 adds two pure §2 #7 reads:

  * `query node.<id>.resolved_input.<port>` — the value that ACTUALLY resolves
    into an input port: a wired source's output COERCED to the port type (so the
    `Float->Vector` broadcast is visible), else the R899 pin default, `null` on a
    cycle upstream.
  * `query eval.cycle_nodes` — the ids of the nodes ON a cycle (not merely
    downstream), so a `null` points at the exact knot to break.

Driven over §5.12, headless, no pixels:
  (A) boot: resolved_input reads the wired grey; no cycle.
  (B) authoring a source propagates through resolved_input.
  (C) a Scalar (Float) wired to a Vector input: resolved_input shows the broadcast.
  (D) a 2-cycle -> eval.cycle_nodes localises it; the cycle nodes' value +
      resolved_input read null; downstream nodes are NOT listed.
  (E) breaking the cycle clears cycle_nodes.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1260_node_debugger_reads.py
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


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def inv(tf: RpcSubprocess, verb: str, args: Any) -> Any:
    return tf.invoke(f"/external/{verb}", args)


def rgb(v: Any) -> tuple[int, int, int]:
    return (v["r"], v["g"], v["b"])


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot: resolved_input reads the wired grey; no cycle ───
        assert_eq(rgb(q(tf, "node.2.resolved_input.0")), (0x80, 0x80, 0x80), "Multiply.in0 <- Texture grey")
        assert_eq(rgb(q(tf, "node.2.resolved_input.1")), (0x80, 0x80, 0x80), "Multiply.in1 <- Color grey")
        assert_eq(q(tf, "eval.cycle_nodes"), "", "a DAG has no cycle nodes")
        assert_eq(q(tf, "eval.acyclic"), True, "seed graph is a DAG")
        assert_eq(rgb(q(tf, "node.2.value")), (64, 64, 64), "Multiply(grey, grey) = 64")

        # ── (B) authoring a source propagates through resolved_input ──
        tf.intervene("/external/node.0.value", "#ff0000")  # Texture -> red
        assert_eq(rgb(q(tf, "node.0.value")), (255, 0, 0), "Texture authored red")
        assert_eq(rgb(q(tf, "node.2.resolved_input.0")), (255, 0, 0), "Multiply.in0 now sees red (the wire)")
        assert_eq(rgb(q(tf, "node.2.resolved_input.1")), (0x80, 0x80, 0x80), "in1 unchanged (Color still grey)")
        # The resolved inputs explain the node's value: mul(red, grey) = (128,0,0).
        assert_eq(rgb(q(tf, "node.2.value")), (128, 0, 0), "value follows from the resolved inputs")
        # detail.resolved_input mirrors the selected node.
        tf.intervene("/external/selected_ids", "2")
        assert_eq(rgb(q(tf, "detail.resolved_input.0")), (255, 0, 0), "detail.resolved_input mirrors node 2")

        # ── (C) a Float source broadcasts into a Vector input ────────
        scalar = inv(tf, "add_node", "Scalar")
        add = inv(tf, "add_node", "Add")
        assert_eq((scalar, add), (4, 5), "Scalar=4, Add=5")
        assert_eq(inv(tf, "add_edge", f"{scalar},0,{add},0"), True, "Scalar.out (Float) -> Add.in0 (Vector)")
        # in0 = broadcast(0.0) = black; the debugger sees the coercion, not the raw Float.
        assert_eq(rgb(q(tf, "node.5.resolved_input.0")), (0, 0, 0), "resolved_input shows the Float->Vector broadcast")
        assert_eq(rgb(q(tf, "node.5.resolved_input.1")), (0x80, 0x80, 0x80), "in1 is the unwired grey default")

        # ── (D) ★ a cycle is UNREACHABLE, by both paths ──────────────
        # R1596 — this section used to build a 2-cycle with two `add_edge`
        # calls and then read the localisation back. It cannot: `connect`
        # refuses any wire that would close a cycle and names the path, and
        # `set_graph` runs `Document::validate`, which refuses a blob carrying
        # one. So the editor can no longer hold a cyclic graph AT ALL -- where
        # before it accepted the wire and detected the cycle afterwards.
        #
        # The localisation itself (`Document::cycle_nodes`) is proven where a
        # peer's document can actually be constructed: the crate's own tests and
        # this example's, which build one through the wire form. What is
        # assertable HERE is the guarantee, which is the stronger fact.
        a = inv(tf, "add_node", "Add")
        b = inv(tf, "add_node", "Add")
        assert_eq((a, b), (6, 7), "two Adds")
        assert_eq(inv(tf, "add_edge", f"{a},0,{b},0"), True, "6 -> 7")
        edges_before = q(tf, "edge_ids")
        assert_eq(
            inv(tf, "add_edge", f"{b},0,{a},0"), False,
            "★ the wire that would close the cycle is REFUSED"
        )
        assert_eq(q(tf, "edge_ids"), edges_before, "and nothing was wired")
        assert_eq(q(tf, "eval.acyclic"), True, "so the graph is still a DAG")
        assert_eq(q(tf, "eval.cycle_nodes"), "", "and nobody is on a cycle")

        # The other reads are unaffected by the two loose nodes.
        assert_eq(q(tf, "node.6.resolved_input.0"), q(tf, "node.6.input_default.0"),
                  "an unwired input resolves its own default")
        # Terminal = Multiply(red, grey) = (128,0,0); the loose pair doesn't touch it.
        assert_eq(rgb(q(tf, "eval.output")), (128, 0, 0), "the loose pair leaves the terminal intact")

        # ── (E) deleting one still clears its consumer's wire ────────
        assert_eq(inv(tf, "delete_node", a), True, "delete node 6 (and its incident edges)")
        assert_eq(q(tf, "eval.cycle_nodes"), "", "cycle_nodes stays empty")
        assert_eq(q(tf, "eval.acyclic"), True, "the graph is a DAG")
        # Node 7 lost its wire from 6, so in0 falls back to its grey default —
        # resolvable again (not null).
        assert_eq(rgb(q(tf, "node.7.resolved_input.0")), (0x80, 0x80, 0x80), "node 7 in0 is its grey default again")


if __name__ == "__main__":
    sys.exit(run_demo("r1260_node_debugger_reads", body))
