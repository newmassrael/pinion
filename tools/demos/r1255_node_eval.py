#!/usr/bin/env python3
"""R1255 §5.38 §5.52 §2 #3 #7 — node-graph dataflow EVALUATION (the Phase-C entry).

The node editor was an *authoring* substrate (typed nodes, edges, R899 pin
defaults) with no compute. R1255 makes the authored graph *evaluate*: each node
carries a first-class `NodeOp` compute identity (keyed off, NOT the rewritable
title — the R1242 identity lesson), and a topological, cycle-safe, memoised pass
resolves each input (a wired source's output, else its pin default), coerces it
to the port type (`Float -> Vector` scalar-broadcast), and applies the op. It is
pure derived introspection over the §5.12 plane (§2 #7), no pixels, no mutation:

  * `query node.<id>.value`  — a node's evaluated output (a `Float` float, a
    `Vector` a `{hex,r,g,b,a}` object, `null` on a cycle).
  * `query eval.output`      — the terminal value at the `Output` sink.
  * `query eval.acyclic`     — the DAG check (so `null` = cycle vs absent node).

Authoring drives the result — `intervene`-ing a pin default or wiring a source
re-computes the reads. Phases:

  (A) boot: the seed `Texture x Color -> Multiply -> Output` graph evaluates.
  (B) a fresh unwired `Add` computes over its two grey pin defaults.
  (C) `intervene`-ing those pin defaults re-computes the node's value.
  (D) wiring `Add -> Output` (displacing the old wire) moves the terminal.
  (E) a `Scalar` (`Float`) wired into a `Vector` input broadcasts (coercion),
      and the wire overrides the retained pin default.
  (F) a 2-node cycle is uncomputable (`null`) and flips `eval.acyclic` false;
      a self-loop wire is structurally rejected; a disconnected cycle leaves
      the terminal intact.
  (G) deleting a cycle node restores the DAG and makes the survivor computable.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1255_node_eval.py
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

GREY = (0x80, 0x80, 0x80)


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def inv(tf: RpcSubprocess, verb: str, args: Any) -> Any:
    return tf.invoke(f"/external/{verb}", args)


def rgb(v: Any) -> tuple[int, int, int]:
    """The (r, g, b) triple of a `Vector` (`Color`) value read."""
    return (v["r"], v["g"], v["b"])


def node_rgb(tf: RpcSubprocess, node: int) -> tuple[int, int, int]:
    return rgb(q(tf, f"node.{node}.value"))


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) the seed graph evaluates ─────────────────────────────
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        assert_eq(q(tf, "edge_count"), 3, "3 seed edges")
        assert_eq(q(tf, "eval.acyclic"), True, "the seed graph is a DAG")
        # Sources are their Vector constant (mid-grey); Multiply blends
        # grey*grey/255 = 64; the Output sink reports its resolved input.
        assert_eq(node_rgb(tf, 0), GREY, "Texture source = grey constant")
        assert_eq(node_rgb(tf, 1), GREY, "Color source = grey constant")
        assert_eq(node_rgb(tf, 2), (64, 64, 64), "Multiply(grey, grey) = grey*grey/255")
        assert_eq(node_rgb(tf, 3), (64, 64, 64), "Output.value = the Multiply result")
        assert_eq(rgb(q(tf, "eval.output")), (64, 64, 64), "eval.output = the terminal")
        # The rename-stable compute identity (R1256) — the AI reads `op` to know
        # what a node computes; Texture vs Color (both ()->Vector) are otherwise
        # structurally identical, and title is rewritable.
        assert_eq(q(tf, "node.0.op"), "Texture", "node 0 op = Texture")
        assert_eq(q(tf, "node.1.op"), "Color", "node 1 op = Color (same signature as Texture)")
        assert_eq(q(tf, "node.2.op"), "Multiply", "node 2 op = Multiply")

        # ── (B) a fresh unwired Add computes over its pin defaults ────
        add = inv(tf, "add_node", "Add")
        assert_eq(add, 4, "add_node minted node 4")
        assert_eq(q(tf, "node_count"), 5, "one node added")
        assert_eq(q(tf, "node.4.op"), "Add", "op=Add (not derivable from its (V,V)->V signature)")
        # Both inputs unconnected -> both grey defaults; 128+128 saturates at 255.
        assert_eq(node_rgb(tf, 4), (255, 255, 255), "Add(grey, grey) saturates to white")

        # ── (C) intervene the pin defaults -> re-compute ─────────────
        tf.intervene("/external/node.4.input_default.0", "#300000")
        tf.intervene("/external/node.4.input_default.1", "#003000")
        assert_eq(node_rgb(tf, 4), (48, 48, 0), "Add(#300000, #003000) = #303000")
        tf.intervene("/external/node.4.input_default.0", "#ff0000")
        assert_eq(node_rgb(tf, 4), (255, 48, 0), "changing in0's default re-computes")

        # ── (D) wire Add -> Output: the terminal moves ───────────────
        # Output.in0 is already wired (edge 2 from Multiply); the one-wire rule
        # displaces it, so edge_count stays 3 (+1 added, -1 displaced).
        assert_eq(inv(tf, "add_edge", "4,0,3,0"), True, "Add -> Output.in0")
        assert_eq(q(tf, "edge_count"), 3, "the new wire displaced the old one")
        assert_eq(node_rgb(tf, 3), (255, 48, 0), "Output now reports Add's value")
        assert_eq(rgb(q(tf, "eval.output")), (255, 48, 0), "eval.output followed the rewire")
        assert_eq(q(tf, "eval.acyclic"), True, "still a DAG")

        # ── (E) Scalar (Float) -> Vector input broadcasts ────────────
        scal = inv(tf, "add_node", "Scalar")
        assert_eq(scal, 5, "add_node minted the Scalar (node 5)")
        assert_eq(q(tf, "node.5.value"), 0.0, "Scalar source = its Float constant 0.0")
        # Float -> Vector is the one lattice coercion; the wire is accepted and
        # the scalar broadcasts (0.0 -> black), overriding in0's #ff0000 default.
        assert_eq(inv(tf, "add_edge", "5,0,4,0"), True, "Scalar.out (Float) -> Add.in0 (Vector)")
        assert_eq(node_rgb(tf, 4), (0, 48, 0), "in0 = broadcast(0.0)=black; in1 = #003000 default")
        assert_eq(rgb(q(tf, "eval.output")), (0, 48, 0), "terminal follows the broadcast")

        # ── (F) a cycle is uncomputable + detected ───────────────────
        a = inv(tf, "add_node", "Add")
        b = inv(tf, "add_node", "Add")
        assert_eq((a, b), (6, 7), "two Add nodes for the cycle (6, 7)")
        assert_eq(inv(tf, "add_edge", "6,0,6,0"), False, "a self-loop wire is rejected")
        assert_eq(inv(tf, "add_edge", "6,0,7,0"), True, "6 -> 7")
        # ★R1596 — a cycle can no longer be AUTHORED. The editor's own gate
        # blocked only a DIRECT self-loop, so two `add_edge` calls could build a
        # two-hop cycle; `Document::connect` refuses any wire that would close
        # one and names the path, so the whole class is unreachable by editing
        # and only a blob from a peer can carry one.
        edges_before = q(tf, "edge_ids")
        assert_eq(inv(tf, "add_edge", "7,0,6,0"), False, "the closing wire is REFUSED")
        assert_eq(q(tf, "edge_ids"), edges_before, "and nothing was wired")
        assert_eq(q(tf, "eval.acyclic"), True, "so the graph is still a DAG")
        assert_eq(str(q(tf, "eval.cycle_nodes")), "", "with nobody on a cycle")
        # The terminal is untouched by the two loose nodes either way.
        assert_eq(rgb(q(tf, "eval.output")), (0, 48, 0), "the disconnected pair leaves eval.output intact")

        # ── (G) delete one of them -> the other computes over its defaults ──
        assert_eq(inv(tf, "delete_node", 6), True, "delete node 6 (and its incident edges)")
        assert_eq(q(tf, "eval.acyclic"), True, "still a DAG")
        # Node 7 lost its wired in0 -> falls back to its grey default.
        assert_eq(node_rgb(tf, 7), (255, 255, 255), "the survivor computes over its defaults again")


if __name__ == "__main__":
    sys.exit(run_demo("r1255_node_eval", body))
