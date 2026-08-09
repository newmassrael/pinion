#!/usr/bin/env python3
"""R1383 §5.38 §5.52 — node-graph layered auto-layout, over RPC.

Every professional node editor (the engine Blueprint, the DCC, Substance Designer,
another procedural DCC) can *arrange* / *tidy* a tangled graph into a clean left-to-right
layered layout with one command. R1383 adds the AI-first peer of that button:

    invoke auto_layout        (no args -> Bool: did anything move?)

It lays out the WHOLE graph (no selection needed) into columns so data flows
forward — a source's column is left of its consumer's — with a crossing-reduced
vertical order inside each column, as ONE discrete undo step through the same
`apply_node_moves` SSOT align / distribute / nudge use. The pure geometry is a
textbook Sugiyama pass (cycle-broken, longest-path layered, barycenter-ordered,
deterministic — it reads only the node set + heights + edges, never positions),
so `query node.<id>.{x,y}` reads the tidy result back with no pixels (§2 #7) and
an AI agent drives + verifies the whole round-trip over the §5.12 plane (§2 #2).

The seed graph is `Texture(0) x Color(1) -> Multiply(2) -> Output(3)` (ids
0..=3, 3 edges); NODE_W is a fixed 130 and the layout advances 130 + 60 = 190
graph units per column (LAYOUT_PITCH).

  (A) boot taxonomy — 4 nodes / 3 edges / the graph canvas present.
  (B) tidy a scrambled graph — sources parked far right / sink far left; after
      auto_layout the two sources share the layer-0 column, Multiply is one
      column right, Output one further, and the stacked sources do not overlap.
  (C) idempotent — a second pass over the tidy graph moves nothing (Bool false),
      every position unchanged.
  (D) undo — a whole re-layout is exactly ONE undo step; undo restores, redo
      re-applies.
  (E) a wider graph — grow an `Add` consumer of Multiply fed by a fresh Texture
      source (RPC add_node / add_edge); auto_layout now builds a THREE-node
      layer-0 column (the three sources, pairwise non-overlapping) and a
      two-node consumer column, with every edge — old and new — flowing forward.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1383_node_auto_layout.py
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
    wait_query,
)

VIEWPORT = (132 + 640, 420)
G = "node_graph"
UNDO = "/node_undo/external"
CARD_H = 68  # a 1-port source card's height (HEADER_H 30 + 1*PORT_PITCH 28 + BODY_PAD 10)
PITCH = 190  # NODE_W (130) + LAYER_GAP (60)


def nx(tf, nid: int) -> int:
    return tf.query(f"/external/node.{nid}.x")


def ny(tf, nid: int) -> int:
    return tf.query(f"/external/node.{nid}.y")


def place(tf, nid: int, x: int, y: int) -> None:
    tf.intervene(f"/external/node.{nid}.x", x)
    tf.intervene(f"/external/node.{nid}.y", y)
    wait_query(tf, f"/external/node.{nid}.x", x, desc=f"node {nid} x parked")
    wait_query(tf, f"/external/node.{nid}.y", y, desc=f"node {nid} y parked")


def auto_layout(tf) -> bool:
    return tf.invoke("/external/auto_layout", None)


def ucount(tf) -> int:
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
        assert_eq(tf.query("/external/node_count"), 4, "4 seed nodes")
        assert_eq(tf.query("/external/edge_count"), 3, "3 seed edges")

        # ── (B) tidy a deliberately scrambled graph ──────────────────
        # Reverse the natural order: sink far left, sources far right.
        place(tf, 3, 40, 60)  # Output
        place(tf, 2, 240, 300)  # Multiply
        place(tf, 1, 500, 60)  # Color
        place(tf, 0, 520, 260)  # Texture
        assert_eq(auto_layout(tf), True, "auto_layout rearranged the scrambled graph")

        # Columns: Texture(0) x Color(1) [layer 0] -> Multiply(2) -> Output(3).
        assert_eq(nx(tf, 0), nx(tf, 1), "the two sources share the layer-0 column")
        assert nx(tf, 0) < nx(tf, 2), "a source is left of its Multiply consumer"
        assert nx(tf, 1) < nx(tf, 2), "the other source is too"
        assert nx(tf, 2) < nx(tf, 3), "Multiply is left of Output"
        assert_eq(nx(tf, 2) - nx(tf, 0), PITCH, "one column advances exactly NODE_W + LAYER_GAP")
        assert_eq(nx(tf, 3) - nx(tf, 2), PITCH, "and the next column too")
        # The stacked sources do not overlap (>= a card height apart in y).
        assert abs(ny(tf, 0) - ny(tf, 1)) >= CARD_H, "the source pair is stacked, not overlapping"

        # ── (C) idempotent — a second pass moves nothing ─────────────
        tidy = {nid: (nx(tf, nid), ny(tf, nid)) for nid in (0, 1, 2, 3)}
        assert_eq(auto_layout(tf), False, "a second auto_layout over the tidy graph is a no-op")
        for nid, (x, y) in tidy.items():
            assert_eq(nx(tf, nid), x, f"node {nid} x unchanged by the idempotent pass")
            assert_eq(ny(tf, nid), y, f"node {nid} y unchanged by the idempotent pass")

        # ── (D) undo — one discrete step; undo restores, redo re-applies ─
        # Re-scramble every node, then a single auto_layout must be ONE step
        # whose undo restores all four scrambled positions verbatim.
        scrambled = {3: (60, 40), 2: (300, 240), 1: (60, 500), 0: (260, 520)}
        for nid, (x, y) in scrambled.items():
            place(tf, nid, x, y)
        before = ucount(tf)
        assert_eq(auto_layout(tf), True, "auto_layout tidied the re-scrambled graph")
        assert_eq(ucount(tf), before + 1, "a whole re-layout is exactly ONE undo step")
        laid0 = (nx(tf, 0), ny(tf, 0))
        assert laid0 != scrambled[0], "node 0 left its scrambled spot"
        assert_eq(undo(tf), True, "one undo reverses the whole re-layout")
        for nid, (x, y) in scrambled.items():
            wait_query(tf, f"/external/node.{nid}.x", x, desc=f"undo restored node {nid} x")
            wait_query(tf, f"/external/node.{nid}.y", y, desc=f"undo restored node {nid} y")
        assert_eq(redo(tf), True, "redo re-applies the re-layout")
        wait_query(tf, "/external/node.0.x", laid0[0], desc="redo re-laid node 0")

        # ── (E) a wider graph — a 3-node column + a 2-node column ────
        # Add an `Add` consumer of Multiply, fed also by a fresh Texture source,
        # so layer 0 gains a third source and layer 2 gains a second consumer.
        add4 = tf.invoke("/external/add_node", "Add")
        assert_eq(add4, 4, "add_node mints the first dynamic id")
        tex5 = tf.invoke("/external/add_node", "Texture")
        assert_eq(tex5, 5, "the second added node id")
        assert_eq(tf.invoke("/external/add_edge", f"2,0,{add4},0"), True, "Multiply -> Add.in0")
        assert_eq(tf.invoke("/external/add_edge", f"{tex5},0,{add4},1"), True, "Texture -> Add.in1")
        assert_eq(tf.query("/external/node_count"), 6, "6 nodes now")
        assert_eq(tf.query("/external/edge_count"), 5, "5 edges now")

        assert_eq(auto_layout(tf), True, "auto_layout tidies the widened graph")
        # Layer 0 = {0, 1, 5}: three sources sharing one column.
        assert_eq(nx(tf, 0), nx(tf, 1), "sources 0 and 1 share the layer-0 column")
        assert_eq(nx(tf, 0), nx(tf, 5), "the added source 5 joins the same column")
        col0 = sorted((ny(tf, 0), ny(tf, 1), ny(tf, 5)))
        assert col0[1] - col0[0] >= CARD_H, "the lower source pair does not overlap"
        assert col0[2] - col0[1] >= CARD_H, "nor the upper pair"
        # Layer 2 = {3, 4}: Output and Add both consume Multiply.
        assert_eq(nx(tf, 3), nx(tf, 4), "Output and Add share the consumer column")
        assert nx(tf, 2) < nx(tf, 3), "Multiply is left of both its consumers"
        # Every edge — old and new — flows strictly forward.
        for (frm, to) in [(0, 2), (1, 2), (2, 3), (2, 4), (5, 4)]:
            assert nx(tf, frm) < nx(tf, to), f"edge {frm}->{to} flows forward"


if __name__ == "__main__":
    sys.exit(run_demo("R1383 node-graph layered auto-layout", body))
