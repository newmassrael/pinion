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

        # ── (D) a 2-cycle: localise it, and the members read null ────
        a = inv(tf, "add_node", "Add")
        b = inv(tf, "add_node", "Add")
        assert_eq((a, b), (6, 7), "two Adds for the cycle")
        assert_eq(inv(tf, "add_edge", f"{a},0,{b},0"), True, "6 -> 7")
        assert_eq(inv(tf, "add_edge", f"{b},0,{a},0"), True, "7 -> 6 closes the cycle")
        assert_eq(q(tf, "eval.acyclic"), False, "no longer a DAG")
        assert_eq(q(tf, "eval.cycle_nodes"), f"{a},{b}", "cycle_nodes localises EXACTLY the two knots")
        assert_eq(q(tf, "node.6.value"), None, "a cycle node's value is null")
        assert_eq(q(tf, "node.7.value"), None, "its partner is null too")
        assert_eq(q(tf, "node.6.resolved_input.0"), None, "and its resolved input is null")
        assert_eq(q(tf, "node.7.resolved_input.0"), None, "the partner's too")
        # The other nodes are NOT falsely reported as on the cycle.
        assert "0" not in q(tf, "eval.cycle_nodes").split(","), "the Texture source is not on the cycle"
        assert "5" not in q(tf, "eval.cycle_nodes").split(","), "the disconnected Add(5) is not on the cycle"
        # Terminal = Multiply(red, grey) = (128,0,0); the disconnected cycle doesn't touch it.
        assert_eq(rgb(q(tf, "eval.output")), (128, 0, 0), "the disconnected cycle leaves the terminal intact")

        # ── (E) breaking the cycle clears the localisation ───────────
        assert_eq(inv(tf, "delete_node", a), True, "delete a cycle node (and its incident edges)")
        assert_eq(q(tf, "eval.cycle_nodes"), "", "cycle_nodes is empty again")
        assert_eq(q(tf, "eval.acyclic"), True, "the graph is a DAG again")
        # Node 7 lost its wire from 6, so in0 falls back to its grey default —
        # resolvable again (not null).
        assert_eq(rgb(q(tf, "node.7.resolved_input.0")), (0x80, 0x80, 0x80), "node 7 in0 is its grey default again")


if __name__ == "__main__":
    sys.exit(run_demo("r1260_node_debugger_reads", body))
