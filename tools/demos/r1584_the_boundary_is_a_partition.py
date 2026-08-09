#!/usr/bin/env python3
"""R1584 §5.38 §5.52 §2 #7 — a group's boundary is a partition, and it moves.

`group` CREATES a partition: these nodes are inside, the rest are outside, and
the interface is derived from what crosses. `ungroup` destroys one. Neither can
MOVE one, and moving one is what an editor does all day — this node belongs in
the group after all, that one does not. `Document::group_insert` and
`Document::group_separate` are the two directions, and `hello-node-groups`
supplies only WHERE the boundary is: pointed at for the inward move, and the
edit path's own last step for the outward one, which is where the DCC reads it
from too.

What this script checks, and why each check discriminates:

* **A node that changes sides keeps what the graph computes.** The value at the
  group's output is asserted before and after, in both directions, and the
  round trip in and back out is byte-identical. That is the whole claim, and it
  is the one a structural assertion cannot make.
* **PAST the DCC (1): SEPARATE RECONNECTS.** Measured at `8cf50599`,
  `node_group_separate_selected` copies the selected nodes into the parent tree
  and, for the Move arm, deletes them from the group. It touches the interface
  not at all — so the group is left holding sockets that reach nothing and the
  separated nodes arrive wired only to each other, and the value that used to
  flow through them is simply gone. Here the value survives, and the crate's
  own tests hold the DCC's rule as a helper and assert the divergence.
* **PAST the DCC (2): one value is still one port.** `build_node_set_interface`
  walks only the sockets of the nodes being moved and never consults the
  group's existing interface, so a producer that already feeds this instance
  gets a SECOND socket for the same value. Here it re-uses the port.
* **PAST the DCC (3): a port that stops describing a crossing is REMOVED**, and
  the removal is named. The DCC's insert only ever appends.
* **PAST the DCC (4): the blast radius is REPORTED.** A definition is shared, so
  an edit through one instance changes all of them; `node_group_insert_exec`
  does it silently. Here `others` counts them and `severed` names every link
  that died AND the tree it was in — a link id means nothing without one.
* **PAST the DCC (5): the fork is an ARM OF THE OPERATION.** the DCC reaches it
  in two steps and another vocabulary (a node group is an ID datablock, so you
  make it single-user first). Here `fork` is one word at the call, and the
  other instance is asserted unchanged to the byte.
* **A move that would make the group reach itself is refused**, in both
  directions, naming the walk — and the document is unchanged afterwards.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1584_the_boundary_is_a_partition.py
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

#: `hello-node-groups`'s seed, mirrored rather than imported — a demo that read
#: the fixture out of the code under test could not catch it changing.
BASE, BLEND, LEVEL, MIX, FADE, OUT = 0, 1, 2, 3, 4, 5

#: mix(200,60,60 @ 75% with 40,90,220 @ 25%) — what the seeded Mix answers.
SEEDED = "160,67,100"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def fields(reply: str) -> dict[str, str]:
    """`"def:1|moved:1|others:0"` -> a dict. The move report's own form."""
    out: dict[str, str] = {}
    for piece in str(reply).split("|"):
        key, _, value = piece.partition(":")
        out[key] = value
    return out


def refused(tf: RpcSubprocess, path: str, args) -> str:
    """Run a verb that must be refused, and answer the recorded sentence.

    Read back through the READ channel rather than off the error frame, because
    the point is that the application can SHOW it.
    """
    try:
        inv(tf, path, args)
    except AssertionError:
        pass
    except Exception:  # noqa: BLE001 - any refusal shape is fine here
        pass
    sentence = q(tf, "last_refusal")
    assert sentence, f"{path}({args!r}) was expected to be refused and record why"
    return str(sentence)


def collapse_the_mix(tf: RpcSubprocess) -> None:
    """Put the seeded Mix in a group called `Tint`, leaving instance node 6."""
    assert_eq(inv(tf, "select", str(MIX)), "1", "the mix alone")
    assert_eq(
        inv(tf, "group", "Tint"),
        "1:6|unframed:",
        "definition 1, instance node 6, and nothing left a frame behind (R1589)",
    )


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) the seed, and a group to move a boundary about ──────────────
        assert_eq(q(tf, "nodes"), 6, "A: the seeded material")
        assert_eq(q(tf, "last_move"), "", "A: no boundary has moved yet")
        collapse_the_mix(tf)
        assert_eq(
            inv(tf, "interface", "1"),
            "in:Base:colour,Blend:colour,Factor:amount|out:Colour:colour",
            "A: three values cross in, one crosses out",
        )
        assert_eq(inv(tf, "node_value", "6"), SEEDED, "A: and it evaluates")

        # ── (B) inward: the port whose feed moved inside is REMOVED ─────────
        assert_eq(inv(tf, "select", str(BASE)), "1", "B: take the base swatch")
        moved = fields(inv(tf, "group_insert", "6"))
        assert_eq(moved["def"], "1", "B: the definition it went into")
        assert_eq(moved["forked_from"], "-", "B: nothing was copied")
        assert_eq(moved["moved"], "1", "B: one node changed sides")
        assert_eq(
            moved["unexposed"],
            "in0:Base",
            "B: PAST the DCC — the value now comes from inside, so the port "
            "that carried it is gone rather than left describing nothing. "
            "the DCC's insert only ever appends",
        )
        assert_eq(moved["exposed"], "", "B: and nothing new crosses")
        assert_eq(moved["others"], "0", "B: this definition has one instance")
        assert_eq(
            inv(tf, "interface", "1"),
            "in:Blend:colour,Factor:amount|out:Colour:colour",
            "B: two values cross in now",
        )
        assert_eq(q(tf, "nodes"), 5, "B: the swatch left the host tree")
        assert_eq(
            inv(tf, "node_value", "6"),
            SEEDED,
            "B: THE CLAIM — the graph computes what it computed",
        )
        assert_eq(q(tf, "valid"), "ok", "B: and every invariant still holds")

        # ── (C) outward: the same node comes back, still wired ──────────────
        assert_eq(inv(tf, "enter", "6"), "1", "C: step inside the group")
        assert_eq(q(tf, "depth"), 1, "C: the path knows where it is")
        assert_eq(inv(tf, "node_kind", "6"), "Swatch", "C: the swatch is in here")
        assert_eq(inv(tf, "select", "6"), "1", "C: take it back out")
        back = fields(inv(tf, "group_separate", ""))
        assert_eq(back["moved"], "1", "C: one node changed sides")
        assert_eq(
            back["exposed"],
            "in2:Base",
            "C: it feeds the group from outside now, so a port describes that "
            "— APPENDED, at index 2. A round trip restores the graph's meaning "
            "and NOT the port order, and that is forced rather than sloppy: "
            "other instances address ports by index, so putting this one back "
            "at 0 would silently rewire every one of them",
        )
        assert_eq(back["unexposed"], "", "C: and nothing stopped crossing")
        assert_eq(q(tf, "depth"), 0, "C: the user is where their nodes are")
        assert_eq(q(tf, "nodes"), 6, "C: the swatch is back in the host tree")
        assert_eq(
            q(tf, "selection"),
            "7",
            "C: and it is RENUMBERED. Ids are unique within a tree and nowhere "
            "else, so a node that changes trees cannot keep its own",
        )
        assert_eq(
            inv(tf, "interface", "1"),
            "in:Blend:colour,Factor:amount,Base:colour|out:Colour:colour",
            "C: the same three values cross, in the order the moves left them",
        )
        assert_eq(
            inv(tf, "node_value", "6"),
            SEEDED,
            "C: THE CLAIM, the other way — a node that leaves a group keeps "
            "its wiring. The DCC's Separate/Move copies the nodes out, deletes "
            "them from the group and reconnects nothing, so this value is lost",
        )
        assert_eq(q(tf, "valid"), "ok", "C: the round trip broke nothing")

        # ── (D) one value is one port, even across a boundary move ──────────
        assert_eq(inv(tf, "add", "fade"), "8", "D: a second fade")
        assert_eq(inv(tf, "connect", "7.0>8.0"), "linked", "D: fed by the base")
        assert_eq(inv(tf, "select", "8"), "1", "D: move it in")
        shared = fields(inv(tf, "group_insert", "6"))
        assert_eq(
            shared["exposed"],
            "",
            "D: PAST the DCC — the base already crosses at port 0, so the "
            "moved node reads THAT port. build_node_set_interface walks only "
            "the moved node's sockets and would add a second one for the "
            "same value",
        )
        assert_eq(shared["unexposed"], "", "D: and no port stopped describing one")
        assert_eq(
            inv(tf, "interface", "1"),
            "in:Blend:colour,Factor:amount,Base:colour|out:Colour:colour",
            "D: the interface is unchanged — three in, one out",
        )
        assert_eq(q(tf, "valid"), "ok", "D: still coherent")

        # ── (E) outward, with values crossing BOTH ways ─────────────────────
        # The case the claim is really about: the node coming out is fed from
        # the group's input AND feeds what stays behind. Three ports stop
        # describing a crossing, one starts, and the arithmetic does not move.
        assert_eq(inv(tf, "reset", ""), "reset", "E: back to the seed")
        assert_eq(inv(tf, "select", f"{MIX},{FADE}"), "2", "E: the mix and the fade")
        assert_eq(
            inv(tf, "group", "Chain"),
            "1:6|unframed:",
            "E: definition 1, instance 6, nothing unframed (R1589)",
        )
        assert_eq(
            inv(tf, "interface", "1"),
            "in:Base:colour,Blend:colour,Factor:amount|out:Colour:colour",
            "E: three values in, one out",
        )
        chained = inv(tf, "node_value", "6")
        assert_eq(
            chained,
            SEEDED,
            "E: a concrete value, so the comparison after the move cannot pass "
            "by both sides being nothing",
        )
        assert_eq(inv(tf, "enter", "6"), "1", "E: step inside")
        assert_eq(inv(tf, "select", str(MIX)), "1", "E: take the mix back out")
        split = fields(inv(tf, "group_separate", ""))
        assert_eq(
            split["unexposed"],
            "in2:Factor in1:Blend in0:Base",
            "E: all three inputs stop crossing, because the node that consumed "
            "them left",
        )
        assert_eq(
            split["exposed"],
            "in0:Colour",
            "E: and the value it sends back in is one that crosses now",
        )
        assert_eq(q(tf, "nodes"), 6, "E: the mix is out in the host tree")
        assert_eq(
            inv(tf, "node_value", "6"),
            chained,
            "E: THE CLAIM at full strength — the moved node is fed by what fed "
            "the group and feeds what the group fed, so the result is the same "
            "value. The DCC's Separate/Move reconnects NOTHING: the sources "
            "would be left dangling and this would answer with nothing at all",
        )
        assert_eq(q(tf, "valid"), "ok", "E: and the document is coherent")

        # ── (F) a shared definition: the blast radius is NAMED ──────────────
        assert_eq(inv(tf, "reset", ""), "reset", "F: back to the seed")
        collapse_the_mix(tf)
        assert_eq(inv(tf, "instantiate", "1"), "7", "F: a second instance")
        assert_eq(inv(tf, "add", "swatch"), "8", "F: with its own sources")
        assert_eq(inv(tf, "add", "level"), "9", "F: and its own factor")
        assert_eq(inv(tf, "connect", "8.0>7.0"), "linked", "F: wire the grey in")
        assert_eq(inv(tf, "connect", "9.0>7.2"), "linked", "F: and the factor")
        before = inv(tf, "node_value", "7")
        assert_eq(inv(tf, "instances", "1"), 2, "F: two instances of one definition")

        assert_eq(inv(tf, "select", str(BASE)), "1", "F: move the base in through 6")
        cost = fields(inv(tf, "group_insert", "6"))
        assert_eq(
            cost["others"],
            "1",
            "F: PAST the DCC — a definition is SHARED, so this edit reached "
            "an instance the user did not name. node_group_insert_exec "
            "appends to the shared group and reports nothing",
        )
        assert_eq(
            cost["severed"],
            "t0:node 7.0",
            "F: and the link that died is named WITH ITS TREE — a link id is "
            "unique inside one tree and meaningless outside it",
        )
        after = inv(tf, "node_value", "7")
        assert after != before, (
            f"F: the other instance really did change ({before} -> {after}), "
            "which is why the report exists"
        )
        assert_eq(q(tf, "valid"), "ok", "F: and the document is still coherent")

        # ── (G) the same move, forked: nobody else moves ────────────────────
        assert_eq(inv(tf, "reset", ""), "reset", "G: back to the seed")
        collapse_the_mix(tf)
        assert_eq(inv(tf, "instantiate", "1"), "7", "G: a second instance again")
        assert_eq(inv(tf, "add", "swatch"), "8", "G: with its own sources")
        assert_eq(inv(tf, "add", "level"), "9", "G: and its own factor")
        assert_eq(inv(tf, "connect", "8.0>7.0"), "linked", "G: wire the grey in")
        assert_eq(inv(tf, "connect", "9.0>7.2"), "linked", "G: and the factor")
        untouched = inv(tf, "node_value", "7")
        assert_eq(inv(tf, "select", str(BASE)), "1", "G: the same move")
        forked = fields(inv(tf, "group_insert", "6,fork"))
        assert_eq(forked["forked_from"], "1", "G: the definition it was copied from")
        assert forked["def"] != "1", f"G: and a new one to edit: {forked['def']}"
        assert_eq(forked["others"], "0", "G: zero by construction, after a fork")
        assert_eq(forked["severed"], "", "G: so nothing died anywhere")
        assert_eq(
            inv(tf, "interface", "1"),
            "in:Base:colour,Blend:colour,Factor:amount|out:Colour:colour",
            "G: the original definition is exactly as it was",
        )
        assert_eq(inv(tf, "instances", "1"), 1, "G: with the other instance on it")
        assert_eq(
            inv(tf, "node_value", "7"),
            untouched,
            "G: PAST the DCC — the instance nobody named is unchanged to the "
            "byte. The DCC needs a separate make-single-user step first",
        )
        assert_eq(inv(tf, "node_value", "6"), SEEDED, "G: and the edited one still holds")
        assert_eq(q(tf, "valid"), "ok", "G: both definitions are coherent")

        # ── (H) the refusals, each naming what it is about ──────────────────
        assert_eq(inv(tf, "reset", ""), "reset", "H: back to the seed")
        collapse_the_mix(tf)
        assert_eq(inv(tf, "select", "6"), "1", "H: select the group itself")
        assert "cannot be moved into itself" in refused(tf, "group_insert", "6"), (
            "H: a group cannot be moved inside itself"
        )
        assert "not inside a group" in refused(tf, "group_separate", ""), (
            "H: separate needs a tree above, which is the DCC's own rule "
            "(tree_get(snode, 1)); the refusal says so"
        )
        assert_eq(q(tf, "valid"), "ok", "H: no refusal left a mark")

        # A walk that leaves the group and comes back to the node moving in:
        # the group already feeds `fade`, so give `fade` a consumer and try to
        # move THAT in. The DCC's own test for this is one hop deep
        # (`node_group_make_test_selected`), and this case is one where a
        # one-hop rule and a reachability rule agree; R1577 covers the two-hop
        # case where they do not.
        assert_eq(inv(tf, "add", "fade"), "7", "H: fade feeds a second fade")
        assert_eq(inv(tf, "connect", "4.0>7.0"), "linked", "H: two hops out")
        assert_eq(q(tf, "nodes"), 7, "H: the graph is acyclic as it stands")
        assert_eq(inv(tf, "select", "7"), "1", "H: move the far one in")
        cycle = refused(tf, "group_insert", "6")
        assert "cycle" in cycle and "4" in cycle, (
            f"H: the walk is NAMED, not merely refused: {cycle!r}"
        )
        assert_eq(q(tf, "nodes"), 7, "H: and the refusal moved nothing")
        assert_eq(q(tf, "valid"), "ok", "H: the document is untouched")


if __name__ == "__main__":
    run_demo("r1584_the_boundary_is_a_partition", body)
