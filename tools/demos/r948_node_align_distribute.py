#!/usr/bin/env python3
"""R948 §5.52 §5.38 — node-graph align / distribute, over RPC.

Every professional node editor (Unreal Blueprint, Blender, Figma, draw.io) can
align a multi-node selection to a common edge / centre and distribute it with
even spacing. R948 adds the AI-first peer of an align toolbar:

  invoke align_{left,center_h,right,top,center_v,bottom}   (no args -> Bool)
  invoke distribute_{h,v}                                  (no args -> Bool)

Each snaps / spaces the *selected* nodes and journals ONE discrete (non-
coalescing) undo step through the same `MoveNodesCmd` the drag / nudge use, so
a single `Ctrl+Z` reverts a whole align. The result is pure scene-as-data:
`query node.<id>.{x,y}` reads it back with no pixels (§2 #7). An AI agent drives
and verifies the whole round-trip over the §5.12 RPC plane (§2 #2).

The seed graph is `Texture x Color -> Multiply -> Output` (node ids 0..=3);
nodes 0 / 1 / 3 are 1-port (short) cards, node 2 (Multiply) is a 2-port (tall)
card. NODE_W is a fixed 130 (so a node's centre_x = x + 65).

  (A) boot taxonomy — 4 nodes / 3 edges / the graph canvas present.
  (B) horizontal align — left / right / centre_h snap x to the bbox left /
      right / mid, leaving y untouched.
  (C) vertical align — top / bottom / centre_v snap y (equal-height members,
      so y -> min / max / midpoint), leaving x untouched.
  (D) distribute — three uneven members; the extremes stay fixed and the
      middle's centre lands evenly between them (h and v).
  (E) guards — align needs >=2 selected, distribute needs >=3 (else a no-op
      `false` that journals nothing).
  (F) undo — an align is exactly one step; one undo restores, redo re-applies.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r948_node_align_distribute.py
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


def nx(tf, nid: int) -> int:
    return tf.query(f"/external/node.{nid}.x")


def ny(tf, nid: int) -> int:
    return tf.query(f"/external/node.{nid}.y")


def place(tf, nid: int, x: int, y: int) -> None:
    """Move node `nid` to `(x, y)` via the state-write channel, confirming."""
    tf.intervene(f"/external/node.{nid}.x", x)
    tf.intervene(f"/external/node.{nid}.y", y)
    wait_query(tf, f"/external/node.{nid}.x", x, desc=f"node {nid} x placed")
    wait_query(tf, f"/external/node.{nid}.y", y, desc=f"node {nid} y placed")


def select(tf, ids: list[int]) -> None:
    csv = ",".join(str(i) for i in ids)
    tf.intervene("/external/selected_ids", csv)
    wait_query(tf, "/external/selected_ids", csv, desc=f"selection = {csv}")


def layout(tf, verb: str) -> bool:
    return tf.invoke(f"/external/{verb}", None)


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
        assert_eq(tf.query("/external/edge_count"), 3, "3 seed edges")

        # ── (B) horizontal align (left / right / centre_h) ───────────
        # Left edges 100 / 300 / 200; NODE_W=130 -> right edges 230 / 430 /
        # 330 -> bbox left=100, right=430.
        def place_h_row() -> None:
            place(tf, 0, 100, 60)
            place(tf, 1, 300, 160)
            place(tf, 2, 200, 260)
            select(tf, [0, 1, 2])

        place_h_row()
        assert_eq(layout(tf, "align_left"), True, "align_left moved the selection")
        for nid in (0, 1, 2):
            wait_query(tf, f"/external/node.{nid}.x", 100, desc=f"node {nid} x -> bbox left")
        assert_eq(ny(tf, 1), 160, "a horizontal align leaves y untouched")

        place_h_row()
        assert_eq(layout(tf, "align_right"), True, "align_right moved the selection")
        for nid in (0, 1, 2):
            wait_query(tf, f"/external/node.{nid}.x", 300, desc=f"node {nid} x -> right-w (430-130)")

        place_h_row()
        assert_eq(layout(tf, "align_center_h"), True, "align_center_h moved the selection")
        for nid in (0, 1, 2):
            # midpoint(100, 430) - NODE_W/2 = 265 - 65 = 200.
            wait_query(tf, f"/external/node.{nid}.x", 200, desc=f"node {nid} x -> bbox centre")

        # ── (C) vertical align (equal-height members 0 / 1 / 3) ──────
        def place_v_col() -> None:
            place(tf, 0, 40, 80)
            place(tf, 1, 40, 200)
            place(tf, 3, 40, 400)
            select(tf, [0, 1, 3])

        place_v_col()
        assert_eq(layout(tf, "align_top"), True, "align_top moved the selection")
        for nid in (0, 1, 3):
            wait_query(tf, f"/external/node.{nid}.y", 80, desc=f"node {nid} y -> bbox top")
        assert_eq(nx(tf, 1), 40, "a vertical align leaves x untouched")

        place_v_col()
        assert_eq(layout(tf, "align_bottom"), True, "align_bottom moved the selection")
        for nid in (0, 1, 3):
            # equal heights -> bottom-align is y -> max (400).
            wait_query(tf, f"/external/node.{nid}.y", 400, desc=f"node {nid} y -> bbox bottom")

        place_v_col()
        assert_eq(layout(tf, "align_center_v"), True, "align_center_v moved the selection")
        for nid in (0, 1, 3):
            # equal heights -> centre_v is y -> midpoint(80, 400) = 240.
            wait_query(tf, f"/external/node.{nid}.y", 240, desc=f"node {nid} y -> bbox centre")

        # ── (D) distribute (extremes fixed, middle evenly spaced) ────
        # Centres (x+65) 165 / 215 / 565 -> distribute_h fixes 165 & 565,
        # middle -> midpoint(165, 565) = 365 -> x = 300.
        place(tf, 0, 100, 50)
        place(tf, 1, 150, 50)
        place(tf, 2, 500, 50)
        select(tf, [0, 1, 2])
        assert_eq(layout(tf, "distribute_h"), True, "distribute_h moved the middle")
        wait_query(tf, "/external/node.0.x", 100, desc="distribute_h: left extreme fixed")
        wait_query(tf, "/external/node.2.x", 500, desc="distribute_h: right extreme fixed")
        wait_query(tf, "/external/node.1.x", 300, desc="distribute_h: middle centre evenly spaced")

        # Vertical, equal-height members 0 / 1 / 3: y 100 / 150 / 500 ->
        # distribute_v fixes 100 & 500, middle -> 300.
        place(tf, 0, 40, 100)
        place(tf, 1, 40, 150)
        place(tf, 3, 40, 500)
        select(tf, [0, 1, 3])
        assert_eq(layout(tf, "distribute_v"), True, "distribute_v moved the middle")
        wait_query(tf, "/external/node.0.y", 100, desc="distribute_v: top extreme fixed")
        wait_query(tf, "/external/node.3.y", 500, desc="distribute_v: bottom extreme fixed")
        wait_query(tf, "/external/node.1.y", 300, desc="distribute_v: middle centre evenly spaced")

        # ── (E) guards — align needs >=2, distribute needs >=3 ───────
        select(tf, [0])
        steps = count(tf)
        assert_eq(layout(tf, "align_left"), False, "one node has nothing to align to")
        select(tf, [0, 1])
        assert_eq(layout(tf, "distribute_h"), False, "two nodes have no middle to space")
        assert_eq(count(tf), steps, "a too-small align / distribute journals nothing")

        # ── (F) undo — one discrete step; undo restores, redo re-applies ─
        place(tf, 0, 100, 50)
        place(tf, 1, 300, 50)
        place(tf, 2, 200, 50)
        select(tf, [0, 1, 2])
        before = count(tf)
        assert_eq(layout(tf, "align_left"), True, "align_left for the undo round-trip")
        assert_eq(count(tf), before + 1, "an align is exactly ONE undo step")
        wait_query(tf, "/external/node.1.x", 100, desc="aligned before the undo")
        assert_eq(undo(tf), True, "one undo reverses the whole align")
        wait_query(tf, "/external/node.1.x", 300, desc="undo restored the pre-align x")
        wait_query(tf, "/external/node.2.x", 200, desc="undo restored every member")
        assert_eq(redo(tf), True, "redo re-applies the align")
        wait_query(tf, "/external/node.1.x", 100, desc="redo re-aligned the selection")


if __name__ == "__main__":
    sys.exit(run_demo("R948 node-graph align / distribute", body))
