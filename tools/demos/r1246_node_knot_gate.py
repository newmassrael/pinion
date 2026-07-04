#!/usr/bin/env python3
"""R1246 §5.38 §5.49 §5.52 — a reroute KNOT double-click is a no-op; begin_edit
refuses a card edit on a knot (paint==a11y root fix).

Audit-clearance of R1243/R1245. R1245 bound double-click-on-a-knot to a
destructive dissolve — an invented, non-standard footgun that duplicated the
standard Alt+Delete; and it only patched the DOUBLE-CLICK route to `begin_rename`,
leaving F2 and `invoke begin_rename` still arming a card-title editor on a knot
that the compact-dot paint never shows (an a11y textbox with no painted peer).
R1246 fixes both at the root: `begin_edit` refuses a CARD-surface edit on a
reroute (covering double-click / F2 / RPC), so a knot double-click is a clean
no-op. Dissolve stays on the standard `invoke dissolve_node` / Alt+Delete.

The seed graph: 4 nodes, 3 edges; edge 0 = node0.out0 -> node2.in0.

  (A) boot; splice a reroute into edge 0 (rid = 4).
  (B) DOUBLE-CLICK the knot: NO-OP — no dissolve, still a reroute, no rename armed.
  (C) begin_rename on the knot is refused via the RPC verb (by id AND the Null =
      selection form the F2 key drives) — `renaming` stays null (no phantom editor).
  (D) dissolve via the STANDARD path (`invoke dissolve_node`) still works.
  (E) a COMPUTE node's double-click still opens its title rename.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1246_node_knot_gate.py
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

NODE_W = 130
HEADER_H = 30
PORT_PITCH = 28
PORT_SIZE = 12
PORT_ROW0_TOP = HEADER_H + PORT_PITCH // 2 - PORT_SIZE // 2  # 38
PALETTE_W = 132


def W(gx: float, gy: float) -> tuple[float, float]:
    return (PALETTE_W + gx, gy)


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def inv(tf: RpcSubprocess, verb: str, arg: Any = None) -> Any:
    return tf.invoke(f"/external/{verb}", arg)


def out_port0_center(x: int, y: int) -> tuple[int, int]:
    return (x + NODE_W - PORT_SIZE // 2, y + PORT_ROW0_TOP + PORT_SIZE // 2)


def in_port0_center(x: int, y: int) -> tuple[int, int]:
    return (x + PORT_SIZE // 2, y + PORT_ROW0_TOP + PORT_SIZE // 2)


def wires(tf: RpcSubprocess) -> set[str]:
    ids = q(tf, "edge_ids")
    return {q(tf, f"edge.{e}") for e in ids.split(",")} if ids else set()


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot + splice a reroute into edge 0 ─────────────────
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        assert_eq(q(tf, "edge.0"), "0:0->2:0", "edge 0 = node0.out0 -> node2.in0")
        n0x, n0y = q(tf, "node.0.x"), q(tf, "node.0.y")
        n2x, n2y = q(tf, "node.2.x"), q(tf, "node.2.y")
        fx, fy = out_port0_center(n0x, n0y)
        tx, ty = in_port0_center(n2x, n2y)
        mid = ((fx + tx) / 2.0, (fy + ty) / 2.0)  # the knot sits here

        rid = inv(tf, "add_reroute", 0)
        assert_eq(rid, 4, "the reroute mints node id 4")
        assert_eq(q(tf, "node_count"), 5, "the knot is spliced in")
        assert_eq(q(tf, "edge_count"), 4, "net +1 edge (removed 1, added 2)")
        assert_eq(q(tf, f"node.{rid}.is_reroute"), True, "node 4 is a reroute")
        assert_eq(q(tf, f"node.{rid}.inputs"), 1, "the knot has one input")
        assert_eq(q(tf, f"node.{rid}.outputs"), 1, "the knot has one output")
        assert_eq(q(tf, "renaming"), None, "no rename in flight")

        # ── (B) DOUBLE-CLICK the knot -> NO-OP ──────────────────────
        tf.double_click(at=W(*mid))
        # No structural change and no editor armed. Give the drain a beat via a
        # follow-up query round; the count MUST stay 5 (no dissolve).
        assert_eq(q(tf, "node_count"), 5, "the knot double-click dissolves nothing")
        assert_eq(q(tf, "edge_count"), 4, "no edges changed by the no-op double-click")
        assert_eq(q(tf, "reroute_ids"), str(rid), "the knot is still there")
        assert_eq(q(tf, "renaming"), None, "no phantom rename armed on the knot")
        assert_eq(q(tf, "editing"), None, "no inline editor of any kind armed")
        assert_eq(q(tf, f"node.{rid}.is_reroute"), True, "still a reroute after double-click")

        # ── (C) begin_rename on the knot is refused (every route) ───
        assert_eq(inv(tf, "begin_rename", rid), False, "invoke begin_rename <knot> refused")
        assert_eq(q(tf, "renaming"), None, "no editor armed by the id route")
        # The knot is the selection (add_reroute selects it); the F2 key drives
        # `begin_rename` with Null = the selected node.
        assert_eq(q(tf, "selected"), rid, "the knot is selected")
        assert_eq(inv(tf, "begin_rename", None), False, "F2 (begin_rename Null) on the knot refused")
        assert_eq(q(tf, "renaming"), None, "no editor armed by the F2 route")

        # ── (D) dissolve via the STANDARD verb still works ──────────
        assert_eq(inv(tf, "dissolve_node", rid), True, "invoke dissolve_node removes the knot")
        assert_eq(q(tf, "node_count"), 4, "the knot is gone")
        assert_eq(q(tf, "edge_count"), 3, "net -1 edge (removed 2, added 1 bridge)")
        assert_eq(q(tf, "reroute_ids"), "", "no reroutes remain")
        assert "0:0->2:0" in wires(tf), "node0 -> node2.in0 reconnected by the dissolve"
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Dissolve node", "one labelled dissolve step")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo restores the hop")
        assert_eq(q(tf, "node_count"), 5, "the knot is back")
        assert_eq(q(tf, "reroute_ids"), str(rid), "the reroute is enumerated again")

        # ── (E) a COMPUTE node's double-click still renames ─────────
        # Dissolve the knot again for a clean state, then rename node 2.
        assert_eq(inv(tf, "dissolve_node", rid), True, "re-dissolve for the rename check")
        cx = q(tf, "node.2.x") + NODE_W // 2
        cy = q(tf, "node.2.y") + (HEADER_H + 2 * PORT_PITCH + 10) // 2
        tf.double_click(at=W(cx, cy))
        wait_until(lambda: q(tf, "renaming") is not None,
                   desc="a compute node's double-click opens the title rename")
        assert_eq(q(tf, "node_count"), 4, "renaming a compute node dissolves nothing")


if __name__ == "__main__":
    sys.exit(run_demo("R1246 knot double-click no-op + begin_edit card gate", body))
