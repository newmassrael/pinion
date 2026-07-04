#!/usr/bin/env python3
"""R1245 §5.38 §5.49 §5.52 — double-click a reroute KNOT to DISSOLVE it.

R1243 made a double-click ON A WIRE splice a reroute knot (A -> R -> B). R1245
completes the symmetry: a double-click ON THE KNOT dissolves it (remove +
reconnect the wire straight through, A -> B) — the inverse gesture, reusing the
R1236 `dissolve_node`. A knot has no title to rename, so this is its natural
double-click action; a compute node's double-click still opens its title rename.
This also clears an R1243 latent bug: routing a knot to `begin_rename` armed an
editor the compact-knot paint never shows. All AI-first over the §5.12 plane.

The seed graph: 4 nodes, 3 edges; edge 0 = node0.out0 -> node2.in0.

  (A) boot; splice a reroute into edge 0 (rid = 4), so the wire routes 0 -> R -> 2.
  (B) DOUBLE-CLICK the knot (at the wire midpoint it sits on): it dissolves —
      the knot + its two edges go, node0 -> node2 is reconnected directly.
  (C) one undo restores the whole hop (the knot + both edges); redo re-dissolves.
  (D) a COMPUTE node's double-click still opens its title rename (not a dissolve).

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1245_node_knot_dissolve.py
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

# Node-card geometry (mirrors the hello-node-editor constants).
NODE_W = 130
HEADER_H = 30
PORT_PITCH = 28
PORT_SIZE = 12
PORT_ROW0_TOP = HEADER_H + PORT_PITCH // 2 - PORT_SIZE // 2  # port_row_top(0) = 38
PALETTE_W = 132  # the canvas x-offset inside the window


def W(gx: float, gy: float) -> tuple[float, float]:
    """Graph units -> window-absolute logical px (zoom 1, pan 0)."""
    return (PALETTE_W + gx, gy)


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def undo(tf: RpcSubprocess) -> bool:
    return tf.invoke(f"{UNDO}/undo", None)


def redo(tf: RpcSubprocess) -> bool:
    return tf.invoke(f"{UNDO}/redo", None)


def out_port0_center(x: int, y: int) -> tuple[int, int]:
    return (x + NODE_W - PORT_SIZE // 2, y + PORT_ROW0_TOP + PORT_SIZE // 2)


def in_port0_center(x: int, y: int) -> tuple[int, int]:
    return (x + PORT_SIZE // 2, y + PORT_ROW0_TOP + PORT_SIZE // 2)


def wire0_conns(tf: RpcSubprocess) -> set[str]:
    """The set of `<from>:<port>-><to>:<port>` wires currently in the graph."""
    ids = q(tf, "edge_ids")
    return {q(tf, f"edge.{e}") for e in ids.split(",")} if ids else set()


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot + splice a reroute into edge 0 ─────────────────
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        assert_eq(q(tf, "edge_count"), 3, "3 seed edges")
        assert_eq(q(tf, "edge.0"), "0:0->2:0", "edge 0 = node0.out0 -> node2.in0")
        assert_eq(q(tf, "reroute_ids"), "", "no reroutes at boot")

        n0x, n0y = q(tf, "node.0.x"), q(tf, "node.0.y")
        n2x, n2y = q(tf, "node.2.x"), q(tf, "node.2.y")
        fx, fy = out_port0_center(n0x, n0y)
        tx, ty = in_port0_center(n2x, n2y)
        mid = ((fx + tx) / 2.0, (fy + ty) / 2.0)  # the knot sits here

        rid = tf.invoke("/external/add_reroute", 0)
        assert_eq(rid, 4, "the reroute mints node id 4")
        assert_eq(q(tf, "node_count"), 5, "the knot is spliced in")
        assert_eq(q(tf, "edge_count"), 4, "net +1 edge (removed 1, added 2)")
        assert_eq(q(tf, f"node.{rid}.is_reroute"), True, "node 4 is a reroute")
        assert_eq(q(tf, "reroute_ids"), str(rid), "the knot is enumerated")
        assert "0" not in q(tf, "edge_ids").split(","), "the original edge 0 was removed by the splice"
        wires = wire0_conns(tf)
        assert f"0:0->{rid}:0" in wires, "node0 now feeds the knot"
        assert f"{rid}:0->2:0" in wires, "the knot feeds node2's original input"
        # The knot's dot centres on the wire midpoint (R1243): x = mid - KNOT/2.
        assert_eq(q(tf, f"node.{rid}.x") + 9, int(mid[0]), "the knot centres on the wire mid x")
        assert_eq(q(tf, f"node.{rid}.y") + 9, int(mid[1]), "the knot centres on the wire mid y")

        # ── (B) DOUBLE-CLICK the knot -> dissolve + reconnect ───────
        tf.double_click(at=W(*mid))
        wait_until(lambda: q(tf, "node_count") == 4,
                   desc="the knot double-click dissolves it")
        assert_eq(q(tf, "edge_count"), 3, "net -1 edge (removed 2, added 1 bridge)")
        assert_eq(q(tf, "reroute_ids"), "", "no reroutes remain — the knot is gone")
        # node0 -> node2.in0 is reconnected directly (the bridge).
        assert "0:0->2:0" in wire0_conns(tf), "node0 -> node2.in0 reconnected"

        # ── (C) one undo restores the whole hop; redo re-dissolves ──
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Dissolve node", "one labelled step")
        assert_eq(undo(tf), True, "undo the dissolve")
        assert_eq(q(tf, "node_count"), 5, "the knot is back")
        assert_eq(q(tf, "reroute_ids"), str(rid), "the reroute is enumerated again")
        assert f"0:0->{rid}:0" in wire0_conns(tf), "the 0 -> R hop is restored"
        assert_eq(redo(tf), True, "redo re-dissolves")
        assert_eq(q(tf, "node_count"), 4, "the knot is gone again")
        assert "0:0->2:0" in wire0_conns(tf), "and the wire is bridged again"

        # ── (D) a COMPUTE node's double-click still renames ─────────
        # Restore the knot first (so the graph is non-trivial), then rename a
        # compute node — its double-click opens the title editor, not a dissolve.
        assert_eq(undo(tf), True, "undo back to the knot state")
        assert_eq(q(tf, "node_count"), 5, "knot restored for the rename check")
        assert_eq(q(tf, "renaming"), None, "no rename in flight yet")
        cx = q(tf, "node.2.x") + NODE_W // 2
        cy = q(tf, "node.2.y") + (HEADER_H + 2 * PORT_PITCH + 10) // 2
        tf.double_click(at=W(cx, cy))
        wait_until(lambda: q(tf, "renaming") is not None,
                   desc="a compute node's double-click opens the title rename")
        assert_eq(q(tf, "node_count"), 5, "renaming a compute node dissolves nothing")
        assert_eq(q(tf, "node.2.is_reroute"), False, "node 2 is not a reroute")


if __name__ == "__main__":
    sys.exit(run_demo("R1245 double-click a reroute knot to dissolve it", body))
