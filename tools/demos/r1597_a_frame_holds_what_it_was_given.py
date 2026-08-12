#!/usr/bin/env python3
"""R1597 §5.38 §5.52 §2 #2 — a frame holds what it was GIVEN, not what it covers.

This is the round that finished moving `hello-node-editor` onto
`pinion-node-graph`, and the migration changed what four wire contracts MEAN. A
change of meaning is worth nothing if only unit tests see it, so this drives
every one of them over the live surface.

What this script checks, and why each check discriminates:

* **Membership is a FACT, not a rectangle test.** The editor re-derived "what is
  in this frame" from geometry on every read, so widening the box silently
  adopted nodes and shrinking it abandoned them — what the frame *said* it held
  changed with nobody having edited membership. It is `Node::parent` now (R1589,
  the DCC's `node::parent`), so a resize changes the box and nothing else. The
  script widens a frame until it geometrically covers every node, ASSERTS that
  it does, and only then asserts `contains` did not move — the covering is
  checked first, so the claim cannot pass by accident.
* **PAST the DCC: `attach` / `detach` are answers, not just acts.** Both are
  census operators (`attach` / `detach`) whose model layer landed
  at R1589 with no gesture to reach it. Each answers the CSV of nodes whose frame
  actually changed; the DCC's operators report only whether the operator ran, so
  "it moved three nodes" and "it moved none" are the same answer there.
* **PAST the DCC: containment reads from both ends and at both depths.**
  `frame.<id>.contains` (one level), `frame.<id>.contents` (transitively — what
  a drag carries) and `node.<id>.parent`. The DCC exposes `node.parent` to
  Python and has **no** accessor for a frame's children at all, so its own UI
  walks every node in the tree comparing pointers.
* **`detach` is ONE LEVEL, proven against a NEST.** A node in `Outer > Inner`
  lands in `Outer`, so detaching composes and the all-the-way form is a repeat.
  `detach` clears the parent outright, so only the second is reachable
  there.
* **A dissolve says what it would COST before you run it.** `dissolvable.<id>`
  widened from a shape test ("exactly one wire in and one out") to
  LOSSLESSNESS, and `dissolve_severs.<id>` names the wires it would cut — then
  the script runs the dissolve and asserts exactly those wires went, so the
  prediction is checked against the operation rather than trusted.
  `node_internal_relink` deletes those links and returns `void`, so nothing in
  the DCC can be asked this: the user finds out by doing it.
* **A cycle can no longer be AUTHORED.** The editor's own gate blocked only a
  direct self-loop, so two `add_edge` calls could build a 2-cycle. `connect`
  refuses any wire that would close one, visible here as `add_edge -> false`
  with the edge list unchanged.
* **An undo never re-issues an id.** A snapshot undo restores the mint counters
  with everything else, so the node added after an undo would take the undone
  node's id — and an agent addressing by id would silently be talking about a
  different node.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1597_a_frame_holds_what_it_was_given.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_no_such_member,
    run_demo,
)

#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"
#: R851 — the undo history is its own coordinator-only extra External.
UNDO = "/node_undo/external"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def csv(tf: RpcSubprocess, path: str) -> list[int]:
    raw = str(q(tf, path))
    return [int(n) for n in raw.split(",")] if raw else []


def select(tf: RpcSubprocess, nodes: list[int]) -> None:
    tf.intervene(f"{EXT}/selected_ids", ",".join(str(n) for n in nodes))


def origin(tf: RpcSubprocess, node: int) -> tuple[int, int]:
    """A node's top-left in GRAPH units, read through the selection alias."""
    select(tf, [node])
    return int(q(tf, "detail.x")), int(q(tf, "detail.y"))


def refused(tf: RpcSubprocess, path: str) -> str:
    """The refusal text for a read that must not answer at all."""
    try:
        q(tf, path)
    except Exception as err:  # noqa: BLE001 — the message is the assertion
        return str(err)
    raise AssertionError(f"query {path!r} was expected to be refused")


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # -- (A) framing a selection RECORDS what it holds -------------------
        every = csv(tf, "node_ids")
        assert len(every) >= 4, f"A: the seeded graph: {every}"
        where = {node: origin(tf, node) for node in every}
        select(tf, every[:2])
        frame = int(inv(tf, "add_frame", None))
        assert frame not in every, f"A: a frame is a fresh node: {frame}"
        assert_eq(csv(tf, f"frame.{frame}.contains"), every[:2], "A: it holds the selection")
        assert_eq(q(tf, "frame_count"), 1, "A: and there is one frame")
        # Read it from the MEMBER's end too -- the DCC has only this direction.
        assert_eq(q(tf, f"node.{every[0]}.parent"), frame, "A: the member names its frame")
        assert q(tf, f"node.{every[2]}.parent") is None, "A: an outsider names none"

        # -- (B) * a resize does NOT move membership -------------------------
        held = csv(tf, f"frame.{frame}.contains")
        was = (int(q(tf, f"frame.{frame}.w")), int(q(tf, f"frame.{frame}.h")))
        # Asked for more than the world has; the clamp decides, so the assertion
        # reads the answer back rather than repeating the request.
        tf.intervene(f"{EXT}/frame.{frame}.x", 0)
        tf.intervene(f"{EXT}/frame.{frame}.y", 0)
        tf.intervene(f"{EXT}/frame.{frame}.w", 100000)
        tf.intervene(f"{EXT}/frame.{frame}.h", 100000)
        now = (int(q(tf, f"frame.{frame}.w")), int(q(tf, f"frame.{frame}.h")))
        assert now[0] > was[0] and now[1] > was[1], f"B: the box grew: {was} -> {now}"
        # The box now geometrically COVERS every node -- which is exactly the
        # condition the old rectangle test read as membership. Asserting the
        # covering FIRST is what stops the next assertion passing by accident.
        fx, fy = int(q(tf, f"frame.{frame}.x")), int(q(tf, f"frame.{frame}.y"))
        for outsider in every[2:]:
            ox, oy = where[outsider]
            assert (
                fx <= ox <= fx + now[0] and fy <= oy <= fy + now[1]
            ), f"B: node {outsider} at ({ox},{oy}) must be inside ({fx},{fy})+{now}"
        assert_eq(
            csv(tf, f"frame.{frame}.contains"),
            held,
            "B: * and what it HOLDS did not move -- a rectangle test would say all of them",
        )
        for outsider in every[2:]:
            assert q(tf, f"node.{outsider}.parent") is None, "B: nobody was adopted"

        # -- (C) attach is the ACT that joins, and it names who moved --------
        select(tf, every[2:])
        moved = [int(n) for n in str(inv(tf, "attach", None)).split(",")]
        assert_eq(moved, every[2:], "C: attach names who joined")
        assert_eq(
            csv(tf, f"frame.{frame}.contains"), every, "C: and now the frame holds all of them"
        )
        # Running it again moves nobody, and can SAY so.
        assert_eq(str(inv(tf, "attach", None)), "", "C: a second attach is a no-op it can name")

        # -- (D) detach is one level, proven against a NEST ------------------
        select(tf, [every[0]])
        inner = int(inv(tf, "add_frame", None))
        assert_eq(q(tf, f"node.{every[0]}.parent"), inner, "D: the inner frame took it")
        assert_eq(q(tf, f"frame.{inner}.parent"), frame, "D: and sits inside the outer one")
        assert_eq(csv(tf, f"frame.{inner}.contains"), [every[0]], "D: inner holds the node")
        assert every[0] not in csv(tf, f"frame.{frame}.contains"), "D: outer no longer does"
        assert every[0] in csv(tf, f"frame.{frame}.contents"), "D: but still CONTAINS it"
        # One level out lands in the OUTER frame, not on the canvas.
        select(tf, [every[0]])
        assert_eq(str(inv(tf, "detach", None)), str(every[0]), "D: detach names who left")
        assert_eq(
            q(tf, f"node.{every[0]}.parent"),
            frame,
            "D: * one level -- the DCC's detach clears the parent outright",
        )
        # Repeat and it reaches the canvas, which is what composing means.
        select(tf, [every[0]])
        inv(tf, "detach", None)
        assert q(tf, f"node.{every[0]}.parent") is None, "D: twice reaches the canvas"
        assert_eq(str(inv(tf, "detach", None)), "", "D: and a third time moves nobody")

        # -- (E) a dissolve says what it would COST --------------------------
        # The sink: nothing flows past it, so dissolving it cuts nothing.
        sink = every[3]
        assert q(tf, f"dissolvable.{sink}") is True, "E: a sink's dissolve is lossless"
        assert_eq(str(q(tf, f"dissolve_severs.{sink}")), "", "E: and cuts nothing")
        # A source: its outgoing wire has no upstream to be bridged from.
        source = every[0]
        assert q(tf, f"dissolvable.{source}") is False, "E: a source's dissolve is lossy"
        cut = str(q(tf, f"dissolve_severs.{source}"))
        assert cut, f"E: * and it NAMES the wire it would cut: {cut!r}"
        # The prediction IS the operation: run it and exactly that wire is gone.
        assert int(cut) in csv(tf, "edge_ids"), "E: the named wire is there first"
        assert inv(tf, "dissolve_node", source) is True, "E: the dissolve runs"
        assert int(cut) not in csv(tf, "edge_ids"), "E: and cut exactly what it said"
        tf.invoke(f"{UNDO}/undo", None)
        assert int(cut) in csv(tf, "edge_ids"), "E: one undo restores the hop"
        # An id with no node has no dissolve at all -- a REFUSAL, which is a
        # different fact from "it would cut nothing".
        #
        # ★ R1670 — and since R1667 it is a refusal that NAMES the id it could
        # not find, under the code that means "the family is right, this member
        # is not". The old assertion looked for the word `Unknown`, which was
        # the collapsed answer's variant name; asserting on the arm and on the
        # id is the fact, where the word was the encoding.
        assert_no_such_member(
            lambda: q(tf, "dissolve_severs.9999"), saying="9999"
        )

        # -- (F) a cycle cannot be authored ----------------------------------
        a = int(inv(tf, "add_node", "Add"))
        b = int(inv(tf, "add_node", "Add"))
        assert inv(tf, "add_edge", f"{a},0,{b},0") is True, "F: the forward wire lands"
        edges_before = csv(tf, "edge_ids")
        assert (
            inv(tf, "add_edge", f"{b},0,{a},0") is False
        ), "F: * the wire that would close the cycle is REFUSED"
        assert_eq(csv(tf, "edge_ids"), edges_before, "F: and nothing was wired")
        assert_eq(q(tf, "eval.acyclic"), True, "F: so the graph is still a DAG")
        assert_eq(str(q(tf, "eval.cycle_nodes")), "", "F: with nobody on a cycle")

        # -- (G) an undo never re-issues an id -------------------------------
        doomed = int(inv(tf, "add_node", "Add"))
        tf.invoke(f"{UNDO}/undo", None)
        assert doomed not in csv(tf, "node_ids"), "G: the node is gone"
        again = int(inv(tf, "add_node", "Add"))
        assert (
            again > doomed
        ), f"G: * {again} must be past {doomed} -- an id an agent held must not be reused"


if __name__ == "__main__":
    run_demo("r1597_a_frame_holds_what_it_was_given", body)
