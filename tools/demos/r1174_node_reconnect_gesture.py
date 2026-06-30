#!/usr/bin/env python3
"""R1174 §5.52 §5.38 §5.49 — node-graph edge reconnect, the live DRAG gesture.

R929 shipped the AI-first reconnect *verb* (`invoke reconnect_edge`) but
deferred the human-editor *gesture* — grabbing a wired input port and dragging
its wire loose onto another input. R1174 closes that honest gap: a press on a
*wired* input arms a reconnect drag that reuses the same R742 `begin_drag` /
`drag_release` substrate the output-port connect uses, with the grabbed edge id
riding in the live `Preview` so the loose end commits through the SAME
`reconnect_edge` SSOT as the verb (one atomic `Ctrl+Z`, validated through
`validate_connection`). So reconnect is now "a live drag AND the AI path are one
source of truth", like node move and edge connect.

This demo drives the *gesture* over `scene/drag` (a real press → march →
release arc through the `InputRouter`), and reads the whole round-trip back over
the §5.12 RPC plane (§2 #2) — the gesture and its outcome are both AI-observable.

The seed graph is `Texture x Color -> Multiply -> Output` (all `Vector` ports),
edges 0=`0:0->2:0`, 1=`1:0->2:1`, 2=`2:0->3:0`.

  (A) boot taxonomy — 4 nodes / 3 edges / empty undo history.
  (B) free Multiply.in1 (remove edge 1) so a clean reconnect has somewhere to
      land.
  (C) GESTURE — drag Multiply.in0's wire onto the freed Multiply.in1: source
      preserved, edge count unchanged (a rewire, not an add), a fresh edge id,
      one undo step labelled "Reconnect" (identical to the verb).
  (D) one undo restores the original wiring; redo re-wires it.
  (E) an UNWIRED input has no edge to grab — dragging it arms nothing (inert).
  (F) dropping the reconnect drag on a non-input (an OUTPUT port) cancels — the
      wiring and the undo history are untouched.
  (G) re-dropping a wire on its OWN input is a no-op success (no fresh id, no
      undo step).
  (H) GESTURE onto an OCCUPIED input displaces the resident wire; one undo
      restores BOTH (the same atomic delta the verb records).

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1174_node_reconnect_gesture.py
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
    wait_until,
)

VIEWPORT = (132 + 640, 420)
G = "node_graph"
UNDO = "/node_undo/external"


def edge_count(tf) -> int:
    return tf.query("/external/edge_count")


def edge_ids(tf) -> list[int]:
    csv = tf.query("/external/edge_ids")
    return [int(x) for x in csv.split(",")] if csv else []


def conns(tf) -> dict[int, str]:
    """Live edge id -> "from:fp->to:tp" connection string."""
    return {eid: tf.query(f"/external/edge.{eid}") for eid in edge_ids(tf)}


def edge_id_of(tf, conn: str) -> int | None:
    """The stable id of the edge with connection `conn` (robust to id minting)."""
    return next((eid for eid, c in conns(tf).items() if c == conn), None)


def has_conn(tf, conn: str) -> bool:
    return conn in conns(tf).values()


def remove_edge(tf, edge: int) -> bool:
    return tf.invoke("/external/remove_edge", edge)


def add_edge(tf, spec: str) -> bool:
    return tf.invoke("/external/add_edge", spec)


def count(tf) -> int:
    return tf.query(f"{UNDO}/count")


def undo(tf) -> bool:
    return tf.invoke(f"{UNDO}/undo", None)


def redo(tf) -> bool:
    return tf.invoke(f"{UNDO}/redo", None)


def reconnect_drag(tf, from_port: str, to_port: str, steps: int = 12) -> None:
    """Drag the wire at input `from_port` onto `to_port` (port tags, e.g.
    "iport_2_0" or "oport_0_0") — a real press/march/release through the router."""
    tf.drag(from_path=f"{G}#{from_port}", to_path=f"{G}#{to_port}", steps=steps)


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, G) is not None, "graph canvas present"
        assert_eq(tf.query("/external/node_count"), 4, "4 nodes")
        assert_eq(edge_count(tf), 3, "3 seed edges")
        assert_eq(tf.query("/external/edge.0"), "0:0->2:0", "Texture -> Multiply.in0")
        assert_eq(tf.query("/external/edge.1"), "1:0->2:1", "Color -> Multiply.in1")
        assert_eq(count(tf), 0, "empty undo history at boot")

        # ── (B) free Multiply.in1 so a clean reconnect can land there ─
        assert_eq(remove_edge(tf, 1), True, "disconnect edge 1 to free Multiply.in1")
        assert_eq(edge_count(tf), 2, "two edges after the disconnect")
        assert_eq(count(tf), 1, "the disconnect is one undo step")

        # ── (C) the headline GESTURE — drag Multiply.in0's wire to in1 ─
        assert has_conn(tf, "0:0->2:0"), "Multiply.in0 is wired (grab target)"
        reconnect_drag(tf, "iport_2_0", "iport_2_1")
        wait_until(lambda: has_conn(tf, "0:0->2:1"), timeout=4.0,
                   desc="the dragged wire lands on Multiply.in1")
        assert_eq(edge_count(tf), 2, "a rewire keeps the edge count (not an add)")
        assert not has_conn(tf, "0:0->2:0"), "the wire left Multiply.in0"
        assert has_conn(tf, "0:0->2:1"), "source preserved, target moved to in1"
        assert edge_id_of(tf, "0:0->2:1") != 0, "the reconnected wire minted a fresh id"
        assert_eq(count(tf), 2, "the gesture added exactly one undo step")
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Reconnect",
                  "the gesture journals the same 'Reconnect' step the verb does")

        # ── (D) undo restores the original wiring; redo re-wires it ───
        assert_eq(undo(tf), True, "undo the gesture")
        assert has_conn(tf, "0:0->2:0"), "one undo restored the original target"
        assert not has_conn(tf, "0:0->2:1"), "the reconnected wire is gone"
        assert_eq(tf.query("/external/edge.0"), "0:0->2:0", "edge 0 restored verbatim (stable id)")
        assert_eq(redo(tf), True, "redo re-wires it")
        assert has_conn(tf, "0:0->2:1"), "redo re-applied the reconnect"
        # State now: 0:0->2:1 and 2:0->3:0; Multiply.in0 is unwired.

        # ── (E) an UNWIRED input arms no drag (inert, not a reconnect) ─
        assert not has_conn(tf, "0:0->2:0"), "Multiply.in0 is now unwired"
        before = conns(tf)
        steps = count(tf)
        reconnect_drag(tf, "iport_2_0", "iport_3_0")  # grab an empty input
        assert_eq(conns(tf), before, "dragging an unwired input changes no wiring")
        assert_eq(count(tf), steps, "an inert input drag adds no undo step")

        # ── (F) dropping on a non-input (an OUTPUT port) cancels ───────
        before = conns(tf)
        steps = count(tf)
        reconnect_drag(tf, "iport_2_1", "oport_0_0")  # drop on Texture's output
        assert_eq(conns(tf), before, "a drop off any input port leaves the wire untouched")
        assert_eq(count(tf), steps, "a cancelled reconnect adds no undo step")

        # ── (G) re-dropping a wire on its OWN input is a no-op success ─
        eid = edge_id_of(tf, "0:0->2:1")
        assert eid is not None, "the live reconnected edge id"
        steps = count(tf)
        reconnect_drag(tf, "iport_2_1", "iport_2_1")  # same input
        assert has_conn(tf, "0:0->2:1"), "still the same wire after the no-op"
        assert_eq(edge_id_of(tf, "0:0->2:1"), eid, "a same-input no-op mints no fresh id")
        assert_eq(count(tf), steps, "the no-op reconnect added no undo step")

        # ── (H) GESTURE onto an OCCUPIED input displaces the resident ──
        # Re-occupy Multiply.in0 with Color (node 1), then drag the in1 wire onto
        # it -> displaces the Color wire; one undo restores both.
        assert_eq(add_edge(tf, "1,0,2,0"), True, "Color -> Multiply.in0 (re-occupy in0)")
        assert_eq(edge_count(tf), 3, "three edges before the displacing gesture")
        reconnect_drag(tf, "iport_2_1", "iport_2_0")
        wait_until(lambda: has_conn(tf, "0:0->2:0"), timeout=4.0,
                   desc="node 0 wire moved to Multiply.in0")
        assert not has_conn(tf, "1:0->2:0"), "the resident Color wire was displaced"
        assert not has_conn(tf, "0:0->2:1"), "the dragged wire left Multiply.in1"
        assert_eq(edge_count(tf), 2, "one removed-target + one displaced, one added")
        assert_eq(undo(tf), True, "one undo reverses the whole displacing gesture")
        assert has_conn(tf, "0:0->2:1"), "node 0's wire restored to Multiply.in1"
        assert has_conn(tf, "1:0->2:0"), "the displaced Color wire is restored too"
        assert_eq(edge_count(tf), 3, "both wires are back")


if __name__ == "__main__":
    sys.exit(run_demo("R1174 node-graph reconnect gesture", body))
