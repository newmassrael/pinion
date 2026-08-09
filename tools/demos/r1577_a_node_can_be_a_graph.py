#!/usr/bin/env python3
"""R1577 §5.38 §5.52 §2 #7 — a node can BE a graph, and the framework owns it.

`hello-node-editor` is nine thousand lines and owns its own graph model, its
own edit machinery and its own evaluator — so an application that wanted a
node system had to copy them, and copying a model is a fork, because the
invariants go with it and then drift. It also has **no groups at all**: the
single largest capability a node editor has was inexpressible, because a flat
`Vec<Node>` cannot hold a node that is a graph.

R1577 puts the mechanism in the framework (`pinion-node-graph`, on top of
`pinion-graph::group`) and leaves the taxonomy to the application.
`hello-node-groups` is that application: five material ops, a way to draw a
box, and nothing else. Its length is the argument.

What this script checks, and why each check discriminates:

* **The interface is DERIVED, not authored.** Grouping one `Mix` yields
  exactly the three values that crossed in and the one that crossed out, each
  named from its INTERNAL end — the end that survives inside the definition.
  Nobody typed those names.
* **PAST the DCC (1): a two-hop bypass is REFUSED, and the refusal names the
  wires.** the DCC's `node_group_make_test_selected` tests a one-hop
  approximation — no unselected node may have both an input from the selection
  and an output to it — so `swatch -> mix -> fade -> output`, grouped as
  `{swatch, output}`, PASSES there and produces a cyclic tree; the cycle is
  found later by a separate pass that flags links rather than by anything that
  could have refused. Measured at `8cf50599`: `space_node/node_group.cc` does
  not contain the substring `cycle` at all. Here it is a reachability test and
  the walk is reported.
* **PAST the DCC (2): recursion names the CHAIN.** `node_group_poll` reports
  one flat sentence — "Nesting a node group inside of itself is not allowed" —
  for a direct self-nest and for one four deep, so the definitions that
  actually carry the recursion are never named.
* **A definition is RE-USABLE, and two instances do not share a value.** The
  same definition, instanced twice and fed differently, reads two different
  colours off one wire. A memo keyed by `(tree, node)` rather than by INSTANCE
  gets this wrong, and it is the single easiest thing to get wrong when adding
  groups to an evaluator that did not have them.
* **The interface is ONE statement.** Exposing a port reaches every instance
  at once; unexposing one drops its links inside the definition AND at every
  instance, and slides the higher ports down — bookkeeping a hand-rolled
  editor has to remember in every tree.
* **The edit path is data**, and it is PRUNED when the group it descends
  through is inlined, rather than naming a node that is gone.
* **Nothing the substrate does breaks its own rules**: `validate` is asked
  after every structural edit, not once at the end.
* **It reaches assistive technology.** Where the user is editing is exactly
  the state an AT user cannot infer from a drawing they cannot see.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1577_a_node_can_be_a_graph.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
)

VIEWPORT = (900, 560)
#: The view's scene tag, which every painted node's tag is prefixed with.
VIEW = "nodegroups"
#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"

#: `hello-node-groups`'s seed, mirrored rather than imported — a demo that read
#: the fixture out of the code under test could not catch it changing.
BASE, BLEND, LEVEL, MIX, FADE, OUT = 0, 1, 2, 3, 4, 5

#: mix(200,60,60 @ 75% with 40,90,220 @ 25%), then fade at 0% -> unchanged.
SEEDED = "160,67,100"
#: The same definition fed a 50% level instead of the seeded 25%.
AT_HALF = "120,75,140"
#: …and with the Base input unexposed, so it falls back to its port default.
NO_BASE = "10,22,55"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def refused(tf: RpcSubprocess, path: str, args) -> str:
    """Run a verb that must be refused, and answer the recorded sentence.

    The sentence is read back through the READ channel rather than off the
    error frame, because the point is that the application can SHOW it — a
    refusal a user never sees is a refusal that did not help.
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


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) it is a node system, and the framework built it ─────────────
        assert_eq(q(tf, "trees"), 1, "A: one tree, no definitions yet")
        assert_eq(q(tf, "nodes"), 6, "A: the seeded material")
        assert_eq(q(tf, "links"), 5, "A: wired end to end")
        assert_eq(q(tf, "valid"), "ok", "A: and it satisfies its own invariants")
        assert_eq(q(tf, "depth"), 0, "A: the user is at the root")
        assert_eq(q(tf, "path"), "Material", "A: which is the breadcrumb")
        assert_eq(inv(tf, "node_kind", str(MIX)), "Mix", "A: node 3 is the mix")
        assert_eq(
            inv(tf, "node_ports", str(MIX)),
            "in:Base:colour,Blend:colour,Factor:amount|out:Colour:colour",
            "A: whose signature comes from the taxonomy",
        )
        assert_eq(inv(tf, "node_value", str(MIX)), SEEDED, "A: and it evaluates")

        # ── (B) PAST the DCC: the two-hop bypass the DCC's rule accepts ─────
        assert_eq(inv(tf, "select", f"{BASE},{OUT}"), "2", "B: select the ends")
        bypass = refused(tf, "group", "Bad")
        assert "cycle" in bypass, f"B: the refusal says what it is: {bypass!r}"
        assert f"{BASE} -> {MIX} -> {FADE} -> {OUT}" in bypass, (
            "B: and NAMES the walk that leaves the selection and returns. No "
            "unselected node here has both an input from the selection and an "
            "output to it, so the DCC's one-hop test accepts this selection "
            f"and builds a cyclic tree: {bypass!r}"
        )
        assert_eq(q(tf, "trees"), 1, "B: a refused collapse built no definition")
        assert_eq(q(tf, "nodes"), 6, "B: and moved no node")
        assert_eq(q(tf, "valid"), "ok", "B: the document is untouched")

        # ── (C) the interface is DERIVED from what crosses ──────────────────
        assert_eq(inv(tf, "select", str(MIX)), "1", "C: select the mix alone")
        made = str(inv(tf, "group", "Blend"))
        # R1589 appended the containments the collapse could not carry, so the
        # pair is the first field rather than the whole reply.
        definition, instance = (int(p) for p in made.split("|")[0].split(":"))
        assert_eq(definition, 1, "C: the first definition")
        assert_eq(q(tf, "trees"), 2, "C: the document gained a tree")
        assert_eq(
            inv(tf, "interface", str(definition)),
            "in:Base:colour,Blend:colour,Factor:amount|out:Colour:colour",
            "C: three values crossed in and one out, and each port is named "
            "from its INTERNAL end — nobody typed these",
        )
        assert_eq(
            inv(tf, "node_kind", str(instance)),
            f"group:{definition}",
            "C: what is left behind is an instance",
        )
        assert_eq(q(tf, "last_refusal"), "", "C: and it was not refused")
        assert_eq(q(tf, "valid"), "ok", "C: invariants hold after the collapse")

        # ── (D) collapsing did not change what the material computes ────────
        assert_eq(
            inv(tf, "node_value", str(instance)),
            SEEDED,
            "D: the group answers exactly what the mix answered",
        )

        # ── (E) a definition is RE-USABLE, and instances are independent ────
        assert_eq(inv(tf, "instances", str(definition)), 1, "E: one instance")
        again = int(inv(tf, "instantiate", str(definition)))
        assert_eq(inv(tf, "instances", str(definition)), 2, "E: now two")
        half = int(inv(tf, "add", "level"))
        for wire in (f"{BASE}.0>{again}.0", f"{BLEND}.0>{again}.1", f"{half}.0>{again}.2"):
            assert_eq(inv(tf, "connect", wire), "linked", f"E: {wire}")
        assert_eq(
            inv(tf, "node_value", str(again)),
            AT_HALF,
            "E: the second instance is fed a 50% level and says so",
        )
        assert_eq(
            inv(tf, "node_value", str(instance)),
            SEEDED,
            "E: and the FIRST is unchanged. A memo keyed by (tree, node) "
            "rather than by instance hands this one the other's answer",
        )
        assert_eq(q(tf, "valid"), "ok", "E: still valid")

        # ── (F) the edit path is data ───────────────────────────────────────
        assert_eq(inv(tf, "enter", str(instance)), str(definition), "F: descend")
        assert_eq(q(tf, "depth"), 1, "F: one level down")
        assert_eq(q(tf, "path"), "Material/Blend", "F: the breadcrumb says where")
        assert_eq(q(tf, "current_tree"), definition, "F: editing the definition")
        assert_eq(
            q(tf, "nodes"),
            3,
            "F: the mix, plus the two nodes that ARE the interface seen from "
            "inside — the framework put them there",
        )

        # ── (G) PAST the DCC: recursion names the chain ─────────────────────
        recursion = refused(tf, "instantiate", str(definition))
        assert "nest a group inside itself" in recursion, f"G: {recursion!r}"
        assert str(definition) in recursion, (
            "G: and the chain is NAMED. The DCC reports one flat sentence for "
            f"a direct self-nest and for one four groups deep: {recursion!r}"
        )
        assert_eq(q(tf, "nodes"), 3, "G: nothing was placed")
        assert_eq(inv(tf, "exit", ""), "0", "G: back up")
        assert_eq(q(tf, "depth"), 0, "G: at the root again")

        # ── (H) the interface is ONE statement ──────────────────────────────
        assert_eq(inv(tf, "expose", str(definition)), "3", "H: a fourth input")
        for node in (instance, again):
            ports = str(inv(tf, "node_ports", str(node)))
            assert ports.count(":colour") + ports.count(":amount") == 5, (
                f"H: instance {node} gained the port at once: {ports!r}"
            )
        dropped = int(inv(tf, "unexpose", f"{definition}.0"))
        assert_eq(
            dropped,
            3,
            "H: dropping the Base port took its link inside the definition AND "
            "one at each of the two instances — bookkeeping a hand-rolled "
            "editor has to remember in every tree",
        )
        assert_eq(
            inv(tf, "interface", str(definition)),
            "in:Blend:colour,Factor:amount,Extra:amount|out:Colour:colour",
            "H: and the higher ports slid down",
        )
        assert_eq(
            inv(tf, "node_value", str(instance)),
            NO_BASE,
            "H: so Base falls back to its port default, and the value moves",
        )
        assert_eq(q(tf, "valid"), "ok", "H: through all of it, still valid")

        # ── (I) inlining, and the path that pointed into it ─────────────────
        assert_eq(inv(tf, "enter", str(instance)), str(definition), "I: descend again")
        assert_eq(q(tf, "depth"), 1, "I: inside")
        # Addressed by TREE and node: an agent acts on the document, not on the
        # view, so it can inline the very instance the user has descended
        # through — which is the situation an edit path has to survive.
        inlined = str(inv(tf, "ungroup", f"0.{instance}"))
        assert "in use" in inlined, (
            f"I: the definition is kept — the other instance uses it: {inlined!r}"
        )
        assert_eq(q(tf, "depth"), 0, "I: the path was PRUNED rather than left "
                  "naming a node that is gone")
        assert_eq(q(tf, "path"), "Material", "I: back at the root")
        assert_eq(inv(tf, "instances", str(definition)), 1, "I: one instance left")
        assert_eq(q(tf, "valid"), "ok", "I: and the document is consistent")

        # ── (J) it is on screen, and it reaches assistive technology ────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert snap, "J: the window paints"
        crumb = find_by_tag(snap, f"{VIEW}.breadcrumb")
        assert crumb is not None, "J: the breadcrumb is painted"
        node_card = find_by_tag(snap, f"{VIEW}.node.{again}")
        assert node_card is not None, "J: the surviving instance is drawn"
        acc = tf.request("scene/access", {}).result or {}
        surface = access_node_by_tag(acc, VIEW)
        assert surface is not None, "J: the graph is an AT node"
        name = str(surface.get("value") or "")
        assert "editing Material" in name, f"J: which says where the user is: {name!r}"
        assert "1 definitions" in name, f"J: and what the library holds: {name!r}"


if __name__ == "__main__":
    run_demo("r1577_a_node_can_be_a_graph", body)
