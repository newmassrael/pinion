#!/usr/bin/env python3
"""R1390 §5.38 §5.52 — node-graph force-directed (organic) layout, over RPC.

A pro node editor offers TWO auto-arrange modes: a layered "tidy" for a DAG
(data flows forward across columns — R1383's `auto_layout`) and an *organic*
relaxation for a cyclic or undirected topology (yEd "organic", Graphviz `neato`,
Gephi ForceAtlas). R1390 adds the AI-first peer of that second button:

    invoke force_layout       (no args -> Bool: did anything move?)

It relaxes the WHOLE graph (no selection needed) into a compact, symmetric
cluster: every node repels every other by an inverse-distance electrical force,
every edge springs its endpoints together, annealed over a fixed iteration count
to a low-energy rest state — as ONE discrete undo step through the same
`apply_node_moves` SSOT align / distribute / auto_layout use. The pure geometry
is a textbook Fruchterman-Reingold pass, grid-seeded and fixed-iteration so it is
deterministic (it reads only the node set + edges, never positions), so
`query node.<id>.{x,y}` reads the relaxed result back with no pixels (§2 #7) and
an AI agent drives + verifies the whole round-trip over the §5.12 plane (§2 #2).

The seed graph is `Texture(0) x Color(1) -> Multiply(2) -> Output(3)` (ids 0..=3,
3 edges 0->2 / 1->2 / 2->3). The organic layout keeps a wired pair near the ideal
edge length while pushing an unwired pair apart, so a directly-wired pair ends
TIGHTER than a two-hop pair.

  (A) boot taxonomy — 4 nodes / 3 edges / the graph canvas present.
  (B) relax a scrambled graph — the two wired pairs (0-2, 2-3) end tighter than
      the two-hop pair (0..3), and no two nodes coincide (repulsion separates).
  (C) idempotent — a second pass over the relaxed graph moves nothing (Bool
      false), every position unchanged.
  (D) undo — a whole relaxation is exactly ONE undo step; undo restores, redo
      re-applies.
  (E) position-independent shape — the organic layout reads only structure, so
      relaxing from two different scrambled positions yields the SAME shape
      (identical node-to-node offsets), even though the anchor differs.
  (F) coexists with the layered mode — auto_layout re-columns the same graph
      (a source shares its layer-0 column); force_layout then returns to the same
      organic shape, so the two arrangement modes are independent and each is
      deterministic-from-structure.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1390_node_force_layout.py
"""

from __future__ import annotations

import math
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


def node_dist(tf, a: int, b: int) -> float:
    return math.hypot(nx(tf, a) - nx(tf, b), ny(tf, a) - ny(tf, b))


def offsets(tf, ids: tuple[int, ...]) -> dict[int, tuple[int, int]]:
    """Every node's position relative to node `ids[0]` — the anchor-free shape."""
    ox, oy = nx(tf, ids[0]), ny(tf, ids[0])
    return {nid: (nx(tf, nid) - ox, ny(tf, nid) - oy) for nid in ids}


def place(tf, nid: int, x: int, y: int) -> None:
    tf.intervene(f"/external/node.{nid}.x", x)
    tf.intervene(f"/external/node.{nid}.y", y)
    wait_query(tf, f"/external/node.{nid}.x", x, desc=f"node {nid} x parked")
    wait_query(tf, f"/external/node.{nid}.y", y, desc=f"node {nid} y parked")


def force_layout(tf) -> bool:
    return tf.invoke("/external/force_layout", None)


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
        ids = (0, 1, 2, 3)

        # ── (A) boot taxonomy ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, G) is not None, "graph canvas present"
        assert_eq(tf.query("/external/node_count"), 4, "4 seed nodes")
        assert_eq(tf.query("/external/edge_count"), 3, "3 seed edges")

        # ── (B) relax a deliberately scrambled graph ─────────────────
        place(tf, 3, 40, 60)  # Output
        place(tf, 2, 240, 300)  # Multiply
        place(tf, 1, 500, 60)  # Color
        place(tf, 0, 520, 260)  # Texture
        assert_eq(force_layout(tf), True, "force_layout relaxed the scrambled graph")

        # Every node stays on the world surface (0 <= x,y <= 2048).
        for nid in ids:
            assert 0 <= nx(tf, nid) <= 2048, f"node {nid} x on the world surface"
            assert 0 <= ny(tf, nid) <= 2048, f"node {nid} y on the world surface"

        # FR property: a directly-wired pair settles tighter than a two-hop pair.
        # Edges 0->2, 1->2, 2->3, so 0 and 3 are two hops apart.
        d02 = node_dist(tf, 0, 2)  # wired
        d23 = node_dist(tf, 2, 3)  # wired
        d03 = node_dist(tf, 0, 3)  # two hops
        assert d02 < d03, f"wired 0-2 ({d02:.0f}) tighter than two-hop 0..3 ({d03:.0f})"
        assert d23 < d03, f"wired 2-3 ({d23:.0f}) tighter than two-hop 0..3 ({d03:.0f})"

        # Repulsion keeps every pair from coinciding.
        for i in ids:
            for j in ids:
                if i < j:
                    assert (nx(tf, i), ny(tf, i)) != (nx(tf, j), ny(tf, j)), (
                        f"nodes {i} and {j} never coincide"
                    )

        # ── (C) idempotent — a second pass moves nothing ─────────────
        relaxed = {nid: (nx(tf, nid), ny(tf, nid)) for nid in ids}
        assert_eq(force_layout(tf), False, "a second force_layout is a no-op (idempotent)")
        for nid, (x, y) in relaxed.items():
            assert_eq((nx(tf, nid), ny(tf, nid)), (x, y), f"node {nid} unchanged by the idempotent pass")

        # ── (D) undo — one discrete step; undo restores, redo re-applies ─
        scrambled = {3: (60, 40), 2: (300, 240), 1: (60, 500), 0: (260, 520)}
        for nid, (x, y) in scrambled.items():
            place(tf, nid, x, y)
        before = ucount(tf)
        assert_eq(force_layout(tf), True, "force_layout relaxed the re-scrambled graph")
        assert_eq(ucount(tf), before + 1, "a whole relaxation is exactly ONE undo step")
        laid0 = (nx(tf, 0), ny(tf, 0))
        assert laid0 != scrambled[0], "node 0 left its scrambled spot"
        assert_eq(undo(tf), True, "one undo reverses the whole relaxation")
        for nid, (x, y) in scrambled.items():
            wait_query(tf, f"/external/node.{nid}.x", x, desc=f"undo restored node {nid} x")
            wait_query(tf, f"/external/node.{nid}.y", y, desc=f"undo restored node {nid} y")
        assert_eq(redo(tf), True, "redo re-applies the relaxation")
        wait_query(tf, "/external/node.0.x", laid0[0], desc="redo re-laid node 0 x")

        # ── (E) the organic shape is position-independent ────────────
        # Scramble to config A, relax, record the anchor-free shape.
        for nid, (x, y) in {0: (500, 40), 1: (40, 300), 2: (300, 40), 3: (520, 300)}.items():
            place(tf, nid, x, y)
        assert_eq(force_layout(tf), True, "force_layout relaxed config A")
        shape_a = offsets(tf, ids)
        # Scramble to a DIFFERENT config B (a different bounding box), relax again.
        for nid, (x, y) in {0: (80, 400), 1: (600, 120), 2: (200, 260), 3: (440, 80)}.items():
            place(tf, nid, x, y)
        assert_eq(force_layout(tf), True, "force_layout relaxed config B")
        shape_b = offsets(tf, ids)
        for nid in ids:
            assert_eq(shape_b[nid], shape_a[nid], f"node {nid} offset is position-independent")

        # ── (F) coexists with the layered mode; each is deterministic ─
        # auto_layout re-columns the graph (the two sources share layer 0)...
        assert_eq(auto_layout(tf), True, "auto_layout re-columns the organic graph")
        assert_eq(nx(tf, 0), nx(tf, 1), "the two sources share the layered layer-0 column")
        assert nx(tf, 0) < nx(tf, 2), "a source is left of its Multiply consumer (layered)"
        # ...and force_layout returns to the SAME organic shape, whatever the
        # intermediate arrangement — proof the modes are independent.
        assert_eq(force_layout(tf), True, "force_layout re-relaxes the columned graph")
        shape_c = offsets(tf, ids)
        for nid in ids:
            assert_eq(shape_c[nid], shape_a[nid], f"node {nid} returns to the same organic offset")


if __name__ == "__main__":
    sys.exit(run_demo("R1390 node-graph force-directed (organic) layout", body))
