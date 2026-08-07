#!/usr/bin/env python3
"""R1598 §5.38 §5.52 §2 #2 — a node changes what it IS, not which node it is.

Blender's `NODE_OT_swap_node` (`scripts/startup/bl_operators/node.py`) creates a
NEW node and deletes the old one, so the swapped node's identity dies with it and
every reference to it dies too: a selection, a saved layout, an undo record, an
agent holding the id. `Document::set_kind` changes the body in place, so the id
survives -- which is what makes a swap an edit rather than a replace-and-hope.

What this script checks, and why each check discriminates:

* **PAST BLENDER: the id survives, and the script proves it by ADDRESSING the
  node by id across the swap.** It reads `node.<id>.op` before and after and
  asserts the id names the same node with a different body, then checks the
  position and the label came through -- the fields Blender's new node would
  have started blank.
* **What survives is one derivation, and it is reported.** The verb answers
  `"<carried>|<severed>|<discarded>"`: which ports moved where, which wires
  died, which authored values died. Blender reports NONE of it -- it drops what
  does not fit inside three swallowed exceptions (`except IndexError: pass`,
  `except KeyError: pass`, `except (AttributeError, KeyError, TypeError): pass`)
  plus a silent `tree.links.remove` for a link that turned out invalid, so "the
  swap worked" and "the swap worked and cost you two wires" are the same
  outcome there.
* **A lossless swap is checked against the wires, not just its own report.**
  Multiply -> Add shares (A, B) -> Out, so the script asserts the edge count is
  unchanged AND the report says nothing was lost -- either alone could lie.
* **A lossy swap NAMES the wire, and the naming is checked against the graph.**
  Swapping to a one-input sink drops a port; the script asserts the named wire
  was in the edge list before and is gone after, so the report is verified
  against the operation rather than trusted.
* **The graph is left sound.** `set_graph` round-trips the swapped document,
  which runs `Document::validate` -- so a swap that left a dangling wire or a
  stray authored value would be caught here even if the report looked right.
* **A structural body is refused by NAME.** A comment frame is a node, and a
  frame with a signature would be linkable; the refusal says so rather than
  answering false.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1598_a_node_changes_what_it_is.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
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


def cost(tf: RpcSubprocess, node: int, kind: str) -> tuple[str, list[int], str]:
    """`(carried, severed edge ids, discarded)` for one swap."""
    answer = str(inv(tf, "swap_node", f"{node},{kind}"))
    carried, severed, discarded = answer.split("|")
    return carried, [int(n) for n in severed.split(",")] if severed else [], discarded


def refused(tf: RpcSubprocess, args: str) -> str:
    try:
        inv(tf, "swap_node", args)
    except Exception as err:  # noqa: BLE001 — the message is the assertion
        return str(err)
    raise AssertionError(f"swap_node({args!r}) was expected to be refused")


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # -- (A) a lossless swap keeps the node AND every wire ---------------
        every = csv(tf, "node_ids")
        assert len(every) >= 4, f"A: the seeded graph: {every}"
        # The Multiply: two wired inputs, one wired output.
        target = 2
        assert_eq(q(tf, f"node.{target}.op"), "Multiply", "A: the seed's compute node")
        tf.intervene(f"{EXT}/node.{target}.title", "keep me")
        tf.intervene(f"{EXT}/node.{target}.x", 123)
        edges_before = csv(tf, "edge_ids")

        carried, severed, discarded = cost(tf, target, "Add")
        assert_eq(carried, "in0>in0,in1>in1,out0>out0", "A: every port carried")
        assert_eq(severed, [], "A: no wire died")
        assert_eq(discarded, "", "A: no value died")
        assert_eq(csv(tf, "edge_ids"), edges_before, "A: and the graph agrees")

        # * The id names the SAME node, with a different body.
        assert_eq(q(tf, f"node.{target}.op"), "Add", "A: * same id, new body")
        assert_eq(q(tf, f"node.{target}.title"), "keep me", "A: the rename survived")
        assert_eq(q(tf, f"node.{target}.x"), 123, "A: and the position")
        assert q(tf, f"node.{target}.h") is not None, "A: it is a real node"

        # One undoable step, and it goes back.
        assert_eq(q(tf, "eval.acyclic"), True, "A: still a DAG")
        tf.invoke(f"{UNDO}/undo", None)
        assert_eq(q(tf, f"node.{target}.op"), "Multiply", "A: undo restores the kind")
        assert_eq(q(tf, f"node.{target}.title"), "keep me", "A: rename predates the swap")

        # -- (B) a lossy swap NAMES the wire, verified against the graph -----
        # `Output` is a sink: one input, no output at all. Swapping the Multiply
        # for it drops the second input and the whole output side.
        before = csv(tf, "edge_ids")
        carried, severed, discarded = cost(tf, target, "Output")
        assert_eq(carried, "in0>in0", "B: only the first input has anywhere to go")
        assert severed, f"B: * and the wires that died are NAMED: {severed}"
        for wire in severed:
            assert wire in before, f"B: {wire} was in the graph first"
        after = csv(tf, "edge_ids")
        for wire in severed:
            assert wire not in after, f"B: and {wire} is gone now"
        assert_eq(
            sorted(set(before) - set(after)),
            sorted(severed),
            "B: * exactly the named wires went -- the report is checked, not trusted",
        )

        # The document is SOUND: round-tripping it runs Document::validate, so a
        # dangling wire or a stray authored value would be caught right here.
        blob = q(tf, "serialized")
        assert inv(tf, "set_graph", blob) is True, "B: the swapped graph validates"

        # -- (C) an authored value that loses its port is named too ----------
        # Author on the Lerp's Float factor, then swap for a kind without one.
        lerp = int(inv(tf, "add_node", "Lerp"))
        tf.intervene(f"{EXT}/node.{lerp}.input_default.2", 0.5)
        assert_eq(q(tf, f"node.{lerp}.input_default.2"), 0.5, "C: the value is authored")
        carried, severed, discarded = cost(tf, lerp, "Add")
        assert "in2" in discarded, f"C: * the value that died is NAMED: {discarded!r}"
        assert_eq(q(tf, f"node.{lerp}.op"), "Add", "C: and the node is still the node")

        # -- (D) a structural body is refused BY NAME ------------------------
        tf.intervene(f"{EXT}/selected_ids", str(every[0]))
        frame = int(inv(tf, "add_frame", None))
        assert_eq(
            inv(tf, "swap_node", f"{frame},Add"),
            None,
            "D: a frame's body is the crate's, not the application's",
        )
        reason = refused(tf, f"{every[0]},Nope")
        assert "not a node kind" in reason, f"D: {reason!r}"
        reason = refused(tf, "nonsense")
        assert "malformed argument" in reason, f"D: {reason!r}"


if __name__ == "__main__":
    run_demo("r1598_a_node_changes_what_it_is", body)
