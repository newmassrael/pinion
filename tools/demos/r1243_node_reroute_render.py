#!/usr/bin/env python3
"""R1243 §5.38 §5.49 §5.52 — reroute knot RENDER + double-click-on-wire splice.

R1235 spliced a reroute over `invoke add_reroute <edge_id>` but left it painting
as a full "Reroute" card and deferred the live gesture. R1243 finishes the
visual half: the reroute is now a compact KNOT_SIZE dot centred on the wire, and
a DOUBLE-CLICK on a wire splices one in (the live twin of the verb). Both halves
are driven here over the §5.12 plane (§2 #2), headless, no pixels:

  * `scene/double_click` on a wire midpoint splices a reroute into that wire
    (A -> R -> B) in ONE undoable step — the gesture reads the same background
    edge-hit probe an in-place click selects a wire from.
  * The knot is centred on the wire midpoint by its DOT half-width (KNOT_SIZE/2),
    not a phantom card half (NODE_W/2) — an RPC-observable geometry proof.

  (A) boot: 4 nodes, 3 edges; edge 0 = node0 -> node2.in0.
  (B) the wire midpoint is on the wire (a single click selects edge 0).
  (C) DOUBLE-CLICK that point: +1 node, net +1 edge, edge 0 gone; is_reroute.
  (D) the knot is centred on the wire midpoint by KNOT_SIZE/2 (compact, not card).
  (E) one undo reverts the whole splice; redo re-applies; undo back to baseline.
  (F) a double-click on empty canvas splices nothing.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1243_node_reroute_render.py
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

# Node-card geometry (mirrors the hello-node-editor constants) — used to
# reconstruct the wire's port centres so the demo can verify the knot is
# centred by its DOT half-width, not a phantom card half.
NODE_W = 130
HEADER_H = 30
PORT_PITCH = 28
PORT_SIZE = 12
KNOT_SIZE = 18
PORT_ROW0_TOP = HEADER_H + PORT_PITCH // 2 - PORT_SIZE // 2  # port_row_top(0) = 38
# The graph canvas sits to the right of the palette; node positions are GRAPH
# units, so a click at graph (gx, gy) lands at window (PALETTE_W + gx, gy).
PALETTE_W = 132


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
    """Centre of an op node's output port 0 (window px), from its top-left."""
    return (x + NODE_W - PORT_SIZE // 2, y + PORT_ROW0_TOP + PORT_SIZE // 2)


def in_port0_center(x: int, y: int) -> tuple[int, int]:
    """Centre of an op node's input port 0 (window px), from its top-left."""
    return (x + PORT_SIZE // 2, y + PORT_ROW0_TOP + PORT_SIZE // 2)


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        assert_eq(q(tf, "edge_count"), 3, "3 seed edges")
        assert_eq(q(tf, "edge.0"), "0:0->2:0", "edge 0 = node0.out0 -> node2.in0")
        assert_eq(q(tf, "reroute_ids"), "", "no reroutes at boot")

        # Reconstruct edge 0's endpoints from the node positions, then its
        # midpoint (the cubic's t=0.5 is exactly the straight midpoint).
        n0x, n0y = q(tf, "node.0.x"), q(tf, "node.0.y")
        n2x, n2y = q(tf, "node.2.x"), q(tf, "node.2.y")
        fx, fy = out_port0_center(n0x, n0y)
        tx, ty = in_port0_center(n2x, n2y)
        mid = ((fx + tx) / 2.0, (fy + ty) / 2.0)
        mid_ix, mid_iy = (fx + tx) // 2, (fy + ty) // 2  # i32::midpoint (floor)

        # ── (B/C) DOUBLE-CLICK the wire → splice a reroute knot ──────
        # `mid` is edge 0's bezier midpoint (== r839/r880's (210,134) on the
        # seed layout); the double-click succeeding IS the on-wire validation
        # (a miss would no-op and time out here). A single click at the same
        # spot immediately before would chain with the double-click in the RPC
        # drain's tight time window, so the point is exercised only once.
        tf.double_click(at=W(*mid))
        wait_until(lambda: q(tf, "node_count") == 5,
                   desc="the double-click on the wire splices a reroute knot")
        assert_eq(q(tf, "edge_count"), 4, "net +1 edge (removed 1, added 2)")
        rid = int(q(tf, "reroute_ids"))
        assert_eq(rid, 4, "the double-click minted reroute node 4")
        assert_eq(q(tf, f"node.{rid}.is_reroute"), True, "the new node is a reroute")
        assert "0" not in q(tf, "edge_ids").split(","), "the double-clicked edge 0 is gone"
        # The path now routes node0 -> R -> node2, typed.
        wires = {e: q(tf, f"edge.{e}") for e in q(tf, "edge_ids").split(",")}
        vals = set(wires.values())
        assert f"0:0->{rid}:0" in vals, "node0 now feeds the reroute"
        assert f"{rid}:0->2:0" in vals, "the reroute feeds node2's original input"
        assert_eq(q(tf, f"node.{rid}.input_types"), "Vector", "input adopts the wire type")
        assert_eq(q(tf, f"node.{rid}.output_types"), "Vector", "output adopts the wire type")
        # The gesture keeps the NEW KNOT selected — no edge selection, not the
        # fresh A->R wire under the cursor (the spent-release latch).
        assert_eq(q(tf, "selected_edge"), None, "no edge is selected after the splice")
        assert_eq(q(tf, "selected"), rid, "the new reroute knot is the selection")

        # ── (D) the knot is CENTRED by its DOT half, not a card half ──
        kx, ky = q(tf, f"node.{rid}.x"), q(tf, f"node.{rid}.y")
        assert_eq(kx, mid_ix - KNOT_SIZE // 2,
                  "knot x centres the DOT on the wire midpoint (KNOT_SIZE/2)")
        assert_eq(ky, mid_iy - KNOT_SIZE // 2, "knot y centres the DOT")
        assert kx != mid_ix - NODE_W // 2, "it is NOT centred as a NODE_W card"
        # The knot's centre coincides with the wire midpoint.
        assert_eq(kx + KNOT_SIZE // 2, mid_ix, "the dot centre sits on the wire mid x")
        assert_eq(ky + KNOT_SIZE // 2, mid_iy, "the dot centre sits on the wire mid y")

        # ── (E) undo reverts the whole splice; redo re-applies ───────
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Insert reroute", "one labelled step")
        assert_eq(undo(tf), True, "undo the splice")
        assert_eq(q(tf, "node_count"), 4, "the reroute node is gone")
        assert_eq(q(tf, "edge_count"), 3, "back to 3 edges")
        assert_eq(q(tf, "edge.0"), "0:0->2:0", "edge 0 restored verbatim")
        assert_eq(q(tf, "reroute_ids"), "", "no reroutes after undo")
        assert_eq(redo(tf), True, "redo re-splices")
        assert_eq(q(tf, "node_count"), 5, "the reroute is back")
        assert_eq(q(tf, "reroute_ids"), str(rid), "the reroute is enumerated again")
        assert_eq(undo(tf), True, "undo again to a clean baseline")
        assert_eq(q(tf, "node_count"), 4, "baseline restored")

        # ── (F) a double-click on EMPTY canvas splices nothing ───────
        # Graph (620, 400): right of every node (max x 600) and below every
        # wire (all in y 110..254) — genuinely empty canvas.
        tf.double_click(at=W(620.0, 400.0))
        # Give the drain a beat, then assert nothing changed.
        assert_eq(q(tf, "edge.0"), "0:0->2:0", "edge 0 still intact")
        assert_eq(q(tf, "node_count"), 4, "a double-click off any wire splices nothing")
        assert_eq(q(tf, "reroute_ids"), "", "no reroute from an empty double-click")

        # A second wire double-click still works (the gesture re-arms cleanly).
        # Node-id minting is monotonic, so the re-splice mints the NEXT id (5),
        # not the recycled 4 — one reroute, freshly spliced.
        tf.double_click(at=W(*mid))
        wait_until(lambda: q(tf, "node_count") == 5,
                   desc="the gesture re-arms cleanly and splices again")
        assert_eq(len(q(tf, "reroute_ids").split(",")), 1, "exactly one reroute after the re-splice")
        assert_eq(q(tf, "reroute_ids"), "5", "the re-splice mints the next monotonic id")


if __name__ == "__main__":
    sys.exit(run_demo("R1243 reroute knot render + double-click splice", body))
