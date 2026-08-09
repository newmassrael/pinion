#!/usr/bin/env python3
"""R1589 §5.38 §5.52 §2 #7 — a node can belong to a frame, and belonging is a
forest.

A node editor has two kinds of containment and they are not the same kind. A
**group** contains a graph: its members live in another tree, they compute, and
the boundary is a signature — R1577 built that. A **frame** contains a region of
*canvas*: its members stay exactly where they are, they compute exactly as
before, and the boundary means nothing to the evaluator at all. Without it the
commonest thing anyone does to a large graph — fence off eight nodes and call
them "decode" — has nowhere to be recorded. A census of the DCC reference tree at
`8cf50599` names eight operators for it (`attach`, `detach`, `join`,
`join_named`, `join_nodes`, `parent_set`, `translate_attach`,
`translate_attach_remove_on_cancel`) and this crate had **no concept of it at
all**.

What this script checks, and why each check discriminates:

* **The relation is one field and the invariant is a FOREST.** A container must
  be a frame, and nothing may contain itself. Both are asserted by driving the
  refusals over the wire and reading the graph back afterwards.
* **PAST the DCC (1): the cycle is REFUSED, and the refusal names the chain.**
  the DCC declares the same `node *parent` and states both rules as
  `BLI_assert` inside `node_attach_node` — compiled out of the build it ships.
  Worse, its own `parent_set` calls `node_detach_node` FIRST, so by the
  time the assert runs the chain it would have walked is already cleared: the
  guard cannot fire even in a debug build. Nothing there then terminates, since
  `node_is_parent_and_child` and `get_sorted_node_parents` both walk `parent` to
  `nullptr`.
* **PAST the DCC (2): deleting a frame hands its members to the frame ABOVE.**
  the DCC's `node_unlink_attached` clears every child's parent outright, so
  deleting the middle frame of `Outer > Inner > node` puts the node on the
  canvas even though `Outer` is still there and still contains where it was.
  Only the containment the deletion destroyed is destroyed here, and what moved
  is reported.
* **PAST the DCC (3): a duplicate lands back inside its frame.** the DCC's copy
  path looks the parent up in the copy map, does not find it because the frame
  was not selected, and calls `node_detach_node` — recording nothing. Here the
  fragment NAMES what it was cut from and the insertion puts it back, reporting
  `reframed`.
* **PAST the DCC (4): a frame id is not a name, so a paste into another tree
  cannot join a frame by number.** The same integer elsewhere is an unrelated
  node. This is the hazard `BKE_main_merge` walks into by matching datablocks on
  their NAME; the answer here is the same one R1578 gave for definitions.
* **PAST the DCC (5): the forest is ENUMERABLE.** `frames` answers what contains
  what, in one read. The DCC has one pointer per node and no accessor for the
  relation, so it exists only as something a caller reassembles.
* **One derivation, not one per gesture.** Framing, unframing and dragging all
  act on the selection's *outermost* members. The DCC computes that three
  times — `node_join_attach_recursive` and `node_detach_recursive`, two
  recursive functions over two structs with identical fields, plus a third pass
  in the transform code.
* **The evaluator cannot see it.** Every containment gesture is driven and the
  computed value is asserted unchanged, which is what makes a frame a fact about
  the canvas rather than about the graph.
* **The picture is derived from the model** (§2 #7): the fence's rect is read
  out of the PAINT scene and asserted to contain the cards it fences, and to
  move exactly as far as a drag moved them.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1589_a_frame_is_a_forest.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
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


def fields(reply: str, sep: str = " ") -> dict[str, str]:
    """`"frame:true parent:6 ancestry:6"` -> a dict."""
    out: dict[str, str] = {}
    for piece in str(reply).split(sep):
        if not piece:
            continue
        key, _, value = piece.partition(":")
        out[key] = value
    return out


def where(tf: RpcSubprocess, node: int) -> dict[str, str]:
    return fields(inv(tf, "containment", str(node)))


def value(tf: RpcSubprocess, node: int) -> str:
    return str(inv(tf, "node_value", str(node)))


def refused(tf: RpcSubprocess, path: str, args) -> str:
    """Run a verb that must be refused, and answer the recorded reason."""
    try:
        inv(tf, path, args)
    except Exception:  # noqa: BLE001 - any refusal shape is fine; the reason is read back
        pass
    else:
        raise AssertionError(f"{path}({args!r}) was expected to be refused")
    return str(q(tf, "last_refusal"))


def rect_of(tf: RpcSubprocess, tag: str) -> tuple[int, int, int, int]:
    """A painted node's window-absolute rect, from the PAINT scene."""
    rects = abs_rects_of(tf.snapshot(source="paint", viewport=(1180, 760)))
    assert tag in rects, f"{tag} is not painted (have {sorted(rects)[:8]}...)"
    return rects[tag]


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) before anything is framed ───────────────────────────────────
        assert_eq(q(tf, "nodes"), 6, "A: the seeded material")
        assert_eq(q(tf, "frames"), "", "A: the forest is empty")
        assert_eq(value(tf, MIX), SEEDED, "A: and this is what it computes")
        base = where(tf, BASE)
        assert_eq(base["frame"], "false", "A: an ordinary node is not a frame")
        assert_eq(base["parent"], "-", "A: and it is on the canvas")
        assert_eq(base["ancestry"], "", "A: with nothing above it")

        # ── (B) framing acts on the selection ───────────────────────────────
        inv(tf, "select", f"{BASE},{BLEND},{LEVEL}")
        made = fields(inv(tf, "frame", "sources"), ":")
        frame = int(list(made)[0])
        assert_eq(q(tf, "nodes"), 7, "B: the frame is a node")
        assert_eq(
            q(tf, "frames"),
            f"{frame}={BASE},{BLEND},{LEVEL}",
            "B: PAST the DCC — the forest is enumerable in one read, where "
            "`node::parent` is one pointer per node and the relation has no "
            "accessor at all",
        )
        assert_eq(
            inv(tf, "node_kind", str(frame)),
            "frame",
            "B: and its body is the crate's own structural arm, not a taxonomy "
            "the application had to supply",
        )
        assert_eq(where(tf, BASE)["parent"], str(frame), "B: the members moved in")
        assert_eq(
            value(tf, MIX),
            SEEDED,
            "B: and the evaluator cannot see any of it — a frame is a fact "
            "about the canvas, which is why it is a separate BODY rather than a "
            "flag beside `bypassed`",
        )
        assert_eq(q(tf, "valid"), "ok", "B: the forest satisfies the invariants")

        # ── (C) a frame has no ports, so nothing can be wired to it ─────────
        assert_eq(
            inv(tf, "node_ports", str(frame)),
            "in:|out:",
            "C: an empty signature, so `connect` refuses either end of a wire "
            "to it with the ordinary arity refusal rather than an arm someone "
            "had to remember",
        )
        reason = refused(tf, "connect", f"{BASE}.0>{frame}.0")
        assert "port 0 of 0" in reason, f"C: {reason!r} names the arity"

        # ── (D) nesting: a frame made inside a frame stays inside it ────────
        inv(tf, "select", f"{BASE},{BLEND}")
        inner = int(list(fields(inv(tf, "frame", "pair"), ":"))[0])
        assert_eq(
            where(tf, inner)["parent"],
            str(frame),
            "D: the new fence landed inside what already contained all of the "
            "selection, so framing part of a pipeline does not lift it out",
        )
        assert_eq(where(tf, BASE)["ancestry"], f"{frame},{inner}", "D: outermost first")
        assert_eq(where(tf, frame)["members"], f"{LEVEL},{inner}", "D: direct members")
        assert_eq(
            where(tf, frame)["contents"],
            f"{BASE},{BLEND},{LEVEL},{inner}",
            "D: contents is transitive",
        )

        # ── (E) the outermost derivation, once, for every gesture ───────────
        inv(tf, "select", f"{inner},{BASE}")
        moved = str(inv(tf, "frame", "again"))
        third = int(moved.split(":")[0])
        assert_eq(
            moved.split(":")[1],
            str(inner),
            "E: only the OUTERMOST selected node is attached — `base` keeps the "
            "container that is itself moving. The DCC computes this three "
            "times: node_join_attach_recursive, node_detach_recursive (two "
            "recursive functions over two structs with identical fields) and "
            "again in the transform code",
        )
        assert_eq(
            where(tf, BASE)["ancestry"],
            f"{frame},{third},{inner}",
            "E: so `base` gained a level without being touched",
        )

        # ── (F) the cycle the DCC's own guard cannot catch ──────────────────
        reason = refused(tf, "reparent", f"{frame}>{inner}")
        assert "inside itself" in reason, f"F: {reason!r}"
        assert f"{frame} contains" in reason, (
            f"F: PAST the DCC — the refusal NAMES the chain: {reason!r}. "
            "the DCC asserts this and its own parent_set detaches "
            "first, clearing the very chain the assert walks"
        )
        assert_eq(
            where(tf, inner)["parent"],
            str(third),
            "F: refused means unchanged",
        )
        reason = refused(tf, "reparent", f"{inner}>{inner}")
        assert "cannot be inside itself" in reason, f"F: {reason!r}"
        reason = refused(tf, "reparent", f"{BASE}>{MIX}")
        assert "is not a frame" in reason, (
            f"F: PAST the DCC — the DCC states this one as BLI_assert too: {reason!r}"
        )
        assert_eq(q(tf, "valid"), "ok", "F: and the document is still a forest")

        # ── (G) unframing leaves ONE level ──────────────────────────────────
        inv(tf, "select", str(BASE))
        assert_eq(str(inv(tf, "unframe", "")), str(BASE), "G: one node moved")
        assert_eq(
            where(tf, BASE)["ancestry"],
            f"{frame},{third}",
            "G: PAST the DCC — out of `inner`, still inside the two fences the "
            "gesture did not touch. detach clears the parent outright, "
            "so only the all-the-way form is reachable there",
        )
        inv(tf, "reparent", f"{BASE}>-")
        assert_eq(where(tf, BASE)["ancestry"], "", "G: and the all-the-way form is here too")
        inv(tf, "reparent", f"{BASE}>{inner}")
        assert_eq(where(tf, BASE)["parent"], str(inner), "G: put back for what follows")

        # ── (H) the picture is derived from the model (§2 #7) ───────────────
        fence = rect_of(tf, f"nodegroups.frame.{inner}")
        card = rect_of(tf, f"nodegroups.node.{BASE}")
        assert fence[0] < card[0] and fence[1] < card[1], (
            f"H: the fence starts above and left of what it fences: {fence} vs {card}"
        )
        assert fence[0] + fence[2] > card[0] + card[2], f"H: and ends past it: {fence}"
        outer_fence = rect_of(tf, f"nodegroups.frame.{frame}")
        assert outer_fence[0] < fence[0], (
            f"H: an outer fence stands OFF the inner one rather than coinciding "
            f"with it: {outer_fence} vs {fence}"
        )

        # ── (I) moving a frame moves what it contains ───────────────────────
        before_card = card
        before_fence = fence
        before_outside = rect_of(tf, f"nodegroups.node.{MIX}")
        carried = str(inv(tf, "nudge", f"{inner}:40:0")).split(",")
        assert str(inner) == carried[0], f"I: the frame itself comes first: {carried}"
        assert str(BASE) in carried, f"I: and its contents come along: {carried}"
        after_card = rect_of(tf, f"nodegroups.node.{BASE}")
        after_fence = rect_of(tf, f"nodegroups.frame.{inner}")
        assert_eq(after_card[0] - before_card[0], 40, "I: the card moved with the fence")
        assert_eq(after_fence[0] - before_fence[0], 40, "I: by exactly the same amount")
        assert_eq(
            rect_of(tf, f"nodegroups.node.{MIX}")[0],
            before_outside[0],
            "I: and a node outside the fence did not move — read from a rect "
            "captured BEFORE the drag, so this can fail",
        )
        assert_eq(value(tf, MIX), SEEDED, "I: a drag changes no value")

        # ── (J) a duplicate lands back inside its frame ─────────────────────
        inv(tf, "select", str(BASE))
        out = fields(str(inv(tf, "duplicate", "60,60")), "|")
        assert_eq(out["nodes"], "1", "J: one copy")
        copy = int(str(q(tf, "selection")).split(",")[0])
        assert_eq(
            out["reframed"],
            str(copy),
            "J: PAST the DCC — the copy went back inside the fence its original "
            "is in. The DCC's copy path looks the parent up in the copy map, "
            "does not find it because the frame was not selected, and calls "
            "node_detach_node with no record anywhere",
        )
        assert_eq(out["unframed"], "", "J: nothing was left behind")
        assert_eq(where(tf, copy)["parent"], str(inner), "J: read back off the graph")

        # ── (K) a collapse leaves the instance where the selection was ──────
        inv(tf, "select", f"{BASE},{copy}")
        made = str(inv(tf, "group", "Sources"))
        instance = int(made.split("|")[0].split(":")[1])
        assert_eq(
            where(tf, instance)["parent"],
            str(inner),
            "K: a pipeline stage collapsed into a group stays in its pipeline",
        )
        assert_eq(
            fields(made, "|")["unframed"],
            f"{BASE}<{inner},{copy}<{inner}",
            "K: and the nodes that went into the definition are NAMED as having "
            "left it, because a host-tree frame id means nothing in there",
        )
        assert_eq(q(tf, "valid"), "ok", "K: the forest is intact on both sides")

        # ── (L) an inline puts them back inside the instance's frame ────────
        out_nodes = str(inv(tf, "ungroup", str(instance)))
        assert "2 nodes" in out_nodes, f"L: both came back: {out_nodes}"
        back = [int(n) for n in str(q(tf, "selection")).split(",")]
        for node in back:
            assert_eq(
                where(tf, node)["parent"],
                str(inner),
                "L: grafted onto whatever contained the instance. The DCC "
                "assigns the group node's parent to EVERY copied node, which "
                "overwrites the relationships its own copy step just recreated",
            )
        assert_eq(q(tf, "valid"), "ok", "L: still a forest")

        # ── (M) deleting a frame hands its members up one level ─────────────
        deep = where(tf, back[0])["ancestry"].split(",")
        assert len(deep) >= 2, f"M: the fixture needs nesting: {deep}"
        inv(tf, "dissolve", str(inner))
        assert_eq(
            where(tf, back[0])["ancestry"],
            ",".join(deep[:-1]),
            "M: PAST the DCC — only the containment the deletion destroyed is "
            "destroyed. node_unlink_attached clears the child's parent "
            "outright, putting it on the canvas while its outer fence is still "
            "there",
        )
        assert_eq(q(tf, "valid"), "ok", "M: and no member names a node that is gone")
        assert_eq(value(tf, MIX), SEEDED, "M: through all of it, one value")

        # ── (N) a frame id is not a name ────────────────────────────────────
        # Cut a node that is inside a frame, so the fragment RECORDS the frame
        # it left, and carry it into a different tree.
        inv(tf, "select", str(back[0]))
        inv(tf, "copy", "")
        held = str(q(tf, "clipboard"))
        assert held.startswith("1n"), f"N: one node is held: {held}"
        assert int(q(tf, "clipboard_bytes")) > 0, "N: and it serializes"
        left = where(tf, back[0])
        assert left["parent"] != "-", f"N: the copied node is in a fence: {left}"
        home = int(left["parent"])

        inv(tf, "select", str(MIX))
        stage = str(inv(tf, "group", "Stage"))
        inv(tf, "enter", stage.split("|")[0].split(":")[1])
        assert_eq(q(tf, "depth"), 1, "N: inside the definition now")
        assert int(q(tf, "nodes")) >= 3, "N: which has the collapsed node and its interface"
        # Make the trap REAL: nest fences in here until one of them wears the
        # very number the fragment's record names. Without this the check would
        # pass because that id happens not to exist, which proves nothing.
        inv(tf, "select", str(MIX))
        for _ in range(32):
            born = int(str(inv(tf, "frame", "trap")).split(":")[0])
            if born >= home:
                break
            inv(tf, "select", str(born))
        assert_eq(
            str(inv(tf, "containment", str(home))).split(" ")[0],
            "frame:true",
            "N: the destination tree now has a FRAME wearing that exact number",
        )
        # ★ §C asserted the empty signature in the ROOT tree, whose interface is
        # empty — so a frame wrongly wearing its TREE's ports answers identically
        # there and that check discriminates nothing. R1590 found it by
        # counterfactual (D-CF-11) after R1589's own CF-9 had found the same hole
        # in the crate test and the sweep stopped at that one artifact. Here the
        # definition HAS interface ports, so this can fail.
        assert "|" in str(inv(tf, "interface", "1")), "N: the definition has ports"
        assert_eq(
            inv(tf, "node_ports", str(born)),
            "in:|out:",
            "N: and a frame still has no ports OF ITS OWN",
        )

        pasted = fields(str(inv(tf, "paste", "0,0")), "|")
        assert_eq(
            pasted["reframed"],
            "",
            "N: PAST the DCC — pasted into ANOTHER tree, the copy joins no "
            "frame even though the number it remembers names a frame right "
            "here. That is the hazard BKE_main_merge walks into by matching "
            "datablocks on their NAME",
        )
        assert_eq(
            pasted["unframed"],
            f"{str(q(tf, 'selection'))}<{home}",
            "N: and what it could not rejoin is named, with the frame it left",
        )
        assert_eq(q(tf, "valid"), "ok", "N: both trees are still forests")


if __name__ == "__main__":
    run_demo("r1589_a_frame_is_a_forest", body)
