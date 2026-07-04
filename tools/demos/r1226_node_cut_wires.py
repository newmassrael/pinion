#!/usr/bin/env python3
"""R1226 §5.38 §5.52 — node-graph wire KNIFE (`cut_wires`).

The node editor could disconnect wires one at a time (`remove_edge` / grab-a-wire
+ Delete) but had no fast multi-wire cut — the Blueprint / material-editor knife
that slices through a bundle of wires in one stroke. R1226 adds it as the §2
AI-first primary path (`cut_wires "x1,y1,x2,y2"`, graph units): every wire the
straight cut segment crosses is deleted as ONE undoable step, and the verb
returns the CSV of cut edge ids (mirrors `edge_ids`). The knife is a held
mid-drag the atomic `scene/drag` cannot snapshot (the R1114 §2 #2 note), so the
verb-first path is the canonical one; a live canvas stroke funnels through it.

The geometry reuses the SAME `edge_curve` + `cubic_at` sampling the wire
hit-test reads (no paint/geometry divergence), tested with a robust CLRS
segment-intersection so a knife through a sampled vertex still cuts.

Boot material graph (4 nodes / 3 wires):
    Texture.out --e0--> Multiply.in0        (left column x<=170 -> Multiply x=250)
    Color.out   --e1--> Multiply.in1
    Multiply.out--e2--> Output.in           (Multiply x>=374 -> Output x=470)

  (A) boot: 3 wires, clean undo history.
  (B) a vertical knife at x=200 cuts the two wires INTO Multiply (e0,e1), not e2.
  (C) ONE undo restores both cut wires; redo re-cuts them.
  (D) a vertical knife at x=420 cuts only e2 (past Multiply); e0/e1 intact.
  (E) a stroke in empty space cuts nothing (no undo entry); nodes never removed.
  (F) a malformed / non-string spec is a typed error, never a silent empty cut.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1226_node_cut_wires.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
)

UNDO = "/node_undo/external"


def edge_count(tf: RpcSubprocess) -> int:
    return tf.query("/external/edge_count")


def node_count(tf: RpcSubprocess) -> int:
    return tf.query("/external/node_count")


def edge_ids(tf: RpcSubprocess) -> list[int]:
    csv = tf.query("/external/edge_ids")
    return [int(x) for x in csv.split(",")] if csv else []


def conn(tf: RpcSubprocess, eid: int) -> Any:
    return tf.query(f"/external/edge.{eid}")


def cut(tf: RpcSubprocess, spec: str) -> str:
    return tf.invoke("/external/cut_wires", spec)


def undo_depth(tf: RpcSubprocess) -> int:
    """The undoable depth (`index`): 0 = nothing to undo. (Distinct from
    `count`, the total entries, which a QUndoStack keeps for redo.)"""
    return tf.query(f"{UNDO}/index")


def undo(tf: RpcSubprocess) -> bool:
    return tf.invoke(f"{UNDO}/undo", None)


def redo(tf: RpcSubprocess) -> bool:
    return tf.invoke(f"{UNDO}/redo", None)


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        assert_eq(edge_count(tf), 3, "3 seed wires")
        assert_eq(node_count(tf), 4, "4 seed nodes")
        assert_eq(sorted(edge_ids(tf)), [0, 1, 2], "stable ids 0,1,2")
        assert_eq(undo_depth(tf), 0, "clean undo history at boot")
        # Capture the connection strings so the survivors can be checked by wiring.
        e2_conn = conn(tf, 2)

        # ── (B) knife at x=200 cuts the two wires INTO Multiply ──────
        assert_eq(cut(tf, "200,20,200,380"), "0,1", "cut returns the cut-edge CSV")
        assert_eq(edge_count(tf), 1, "two wires gone")
        assert_eq(sorted(edge_ids(tf)), [2], "only Multiply -> Output survives")
        assert_eq(conn(tf, 2), e2_conn, "the surviving wire is unchanged")
        assert_eq(undo_depth(tf), 1, "the whole cut is ONE undo step")
        assert_eq(node_count(tf), 4, "a cut removes wires, never nodes")

        # ── (C) one undo restores both; redo re-cuts ────────────────
        assert_eq(undo(tf), True, "undo the cut")
        assert_eq(edge_count(tf), 3, "both cut wires restored in one step")
        assert_eq(sorted(edge_ids(tf)), [0, 1, 2], "with their original stable ids")
        assert_eq(undo_depth(tf), 0, "history back to clean")
        assert_eq(redo(tf), True, "redo re-cuts")
        assert_eq(edge_count(tf), 1, "back to one wire")
        assert_eq(undo(tf), True, "undo again for the next section")
        assert_eq(edge_count(tf), 3, "restored")

        # ── (D) knife at x=420 cuts only e2 (past Multiply) ─────────
        assert_eq(cut(tf, "420,20,420,380"), "2", "only the Multiply -> Output wire")
        assert_eq(edge_count(tf), 2, "two wires remain")
        assert_eq(sorted(edge_ids(tf)), [0, 1], "e0 + e1 intact")
        assert_eq(undo(tf), True, "restore for the miss test")
        assert_eq(edge_count(tf), 3, "restored")

        # ── (E) a stroke in empty space cuts nothing ────────────────
        assert_eq(cut(tf, "600,480,700,500"), "", "empty-space stroke crosses no wire")
        assert_eq(edge_count(tf), 3, "graph unchanged")
        assert_eq(undo_depth(tf), 0, "a no-op cut records no undo entry")
        # A stroke down a single node's body column (x=90, inside Texture) — still
        # crosses no wire (wires leave from the port edges).
        assert_eq(cut(tf, "90,20,90,380"), "", "a stroke inside a node cuts nothing")
        assert_eq(edge_count(tf), 3, "still unchanged")

        # ── (F) rejects ─────────────────────────────────────────────
        for bad in ("bad", "1,2,3", "1,2,3,4,5"):
            rejected = False
            try:
                cut(tf, bad)
            except RpcError:
                rejected = True
            assert rejected, f"a malformed spec {bad!r} is a typed Rejected error"
        type_err = False
        try:
            tf.invoke("/external/cut_wires", 5)
        except RpcError:
            type_err = True
        assert type_err, "a non-string arg is a typed TypeMismatch"


if __name__ == "__main__":
    sys.exit(run_demo("R1226 node-graph wire knife (cut_wires)", body))
