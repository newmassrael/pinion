#!/usr/bin/env python3
"""R1594 §5.38 §5.52 §2 #7 — a socket's value is authored on the node.

A port's **type** and **name** come from the kind, so every node of a kind shares
them. Its **value** does not: two `Swatch` nodes are two different colours, and
the number a user typed into an unwired input belongs to that input and to no
other node's. `pinion-node-graph` had nowhere to put that, because `Port::default`
is derived from the kind — so this example had to carry each source's constant
as a payload inside its own taxonomy (`Swatch([i64; 3])`), where nothing could
ever edit it. The DCC keeps exactly what was missing, as
`node socket::default_value`, per socket per node.

`Node::values` is that, and the rule the evaluator applies is **one sentence
covering both sides**: an authored value is what a port carries when nothing
else supplies one. For an input that means no link; for an output it means the
kind computed nothing there — which is what makes a source node's constant this
same mechanism instead of a second one.

What this script checks, and why each check discriminates:

* **Two nodes of one kind hold two values.** The seeded material has two
  `Swatch`es of different colours, and after this round `Op::Swatch` is a UNIT
  variant: the taxonomy got *smaller* because the mechanism moved to where it
  belongs.
* **The three-step fallback, in order.** A port carries the link, else the value
  authored on the node, else the kind's own declared resting value — driven over
  the wire in that order, and back again.
* **A link HIDES an authored value rather than discarding it.** Unwire and it is
  still there.
* **PAST the DCC (1): a source's constant is a rule, not per-node-type code.**
  the DCC's Value node reaches its own output socket's `default_value` in
  `node_shader_value.cc`, and nothing generic does; a node type that forgets
  simply has no constant.
* **PAST the DCC (2): authoring is gated by the signature.** the DCC writes a
  socket's `default_value` through RNA with no such check. Here a port that does
  not exist is refused *by name*, with the arity.
* **PAST the DCC (3): the type is checked.** the DCC gets this free because a
  socket's authored value is a different C struct per socket type; here the
  taxonomy answers `value_type` and the crate refuses a colour on an amount
  port.
* **A copy carries what was authored on it** — through duplicate and through a
  group collapse, which moves the node to another tree.
* **A value that outlived its port is REPORTED.** Shrink a definition's
  interface under an instance that had authored a value on the port that went
  away, and `validate` names it.
* **A bypassed node carries its routing, not its authored outputs**, because
  R1586 NAMES the outputs no input can feed and filling one in would make that
  a lie.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1594_a_value_is_authored_on_the_node.py
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
    out: dict[str, str] = {}
    for piece in str(reply).split("|"):
        key, _, value = piece.partition(":")
        out[key] = value
    return out


def ports(tf: RpcSubprocess, node: int) -> dict[str, str]:
    return fields(inv(tf, "port_values", str(node)))


def refused(tf: RpcSubprocess, path: str, args) -> None:
    try:
        inv(tf, path, args)
    except AssertionError:
        pass
    except Exception:  # noqa: BLE001 - any refusal shape is fine here
        pass
    else:
        raise AssertionError(f"{path}({args!r}) was expected to be refused")


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) two nodes of one kind, two values ───────────────────────────
        assert_eq(q(tf, "nodes"), 6, "A: the seeded material")
        assert_eq(inv(tf, "node_kind", str(BASE)), "Swatch", "A: a Swatch")
        assert_eq(inv(tf, "node_kind", str(BLEND)), "Swatch", "A: and another")
        assert_eq(
            ports(tf, BASE)["authored"],
            "out0=200,60,60",
            "A: the value is on the NODE's own port",
        )
        assert_eq(
            ports(tf, BLEND)["authored"],
            "out0=40,90,220",
            "A: and the other node of the same kind holds a different one — "
            "which a payload inside the taxonomy could never express, because "
            "a kind is shared and a node is not",
        )
        assert_eq(inv(tf, "node_value", str(BASE)), "200,60,60", "A: it emits it")
        assert_eq(inv(tf, "node_value", str(BLEND)), "40,90,220")
        assert_eq(inv(tf, "node_value", str(MIX)), SEEDED, "A: and the mix is unchanged")
        assert_eq(q(tf, "valid"), "ok")

        # ── (B) a fresh source rests where its KIND says ────────────────────
        assert_eq(inv(tf, "add", "swatch"), "6", "B: a fresh Swatch")
        assert_eq(
            ports(tf, 6)["authored"], "", "B: with nothing authored on it"
        )
        assert_eq(
            inv(tf, "node_value", "6"),
            "128,128,128",
            "B: PAST the DCC — it rests where the KIND declares, by the same "
            "rule an input's pin default is reached. The DCC's Value node gets "
            "its constant from per-node C code reading its own output socket, "
            "so a node type that forgets simply has none",
        )

        # ── (C) the three-step fallback, in order ───────────────────────────
        # `Fade`'s Factor input: kind's default, then the node's, then a link.
        assert_eq(
            ports(tf, FADE)["carries"],
            f"in0={SEEDED},in1=0,out0={SEEDED}",
            "C: the Factor rests at the kind's declared 0",
        )
        assert_eq(
            inv(tf, "set_value", f"{FADE}.in1=40"),
            "authored",
            "C: author 40 on THIS node's Factor",
        )
        assert_eq(ports(tf, FADE)["authored"], "in1=40")
        assert_eq(
            ports(tf, FADE)["carries"],
            "in0=160,67,100,in1=40,out0=139,83,103",
            "C: the authored value beats the kind's, and the node fades",
        )
        assert_eq(
            inv(tf, "connect", f"{LEVEL}.0>{FADE}.1"),
            "linked",
            "C: now wire the Factor",
        )
        assert_eq(
            ports(tf, FADE)["carries"].split(",in1=")[1].split(",")[0],
            "25",
            "C: a link beats both",
        )
        assert_eq(
            ports(tf, FADE)["authored"],
            "in1=40",
            "C: and the authored value is HIDDEN, not discarded",
        )
        # Stop the link rather than removing it: R1586's rule is that a muted
        # link makes the port fall back "exactly as if nothing were wired", so
        # this asserts the two derivations compose — and the wire is still
        # there, which is what makes "hidden, not discarded" checkable twice.
        link = int(str(q(tf, "links"))) - 1
        assert_eq(inv(tf, "mute_link", str(link)), "muted", "C: stop the link")
        assert_eq(
            ports(tf, FADE)["carries"].split(",in1=")[1].split(",")[0],
            "40",
            "C: and the authored value is reached again, with the wire still "
            "in place",
        )
        assert_eq(q(tf, "muted_links"), str(link), "C: still wired, still muted")
        assert_eq(
            inv(tf, "clear_value", f"{FADE}.in1"),
            "cleared 40",
            "C: clearing answers what it took away",
        )
        # The CARRIED value is 0 either way, so asserting it cannot tell a real
        # clear from writing the kind's default over the port. What separates
        # them is whether anything is still authored — and that is the whole
        # claim, because a written-over port stops tracking the kind.
        assert_eq(
            ports(tf, FADE)["authored"],
            "",
            "C: nothing is authored any more — clearing is not writing the "
            "default over it, so a later change to the kind reaches this node",
        )
        assert_eq(
            ports(tf, FADE)["carries"].split(",in1=")[1].split(",")[0],
            "0",
            "C: and the port is back to the kind's own",
        )
        assert_eq(
            inv(tf, "clear_value", f"{FADE}.in1"),
            "nothing was authored",
            "C: and clearing nothing says so",
        )

        # ── (D) authoring is gated, twice ───────────────────────────────────
        refused(tf, "set_value", f"{LEVEL}.in0=5")
        assert "no port in0" in str(q(tf, "last_refusal")), q(tf, "last_refusal")
        assert "has 0" in str(q(tf, "last_refusal")), (
            "D: PAST the DCC — a port that is not there is refused BY NAME, "
            "with the arity. The DCC writes a socket's default_value through "
            "RNA with no such gate"
        )
        refused(tf, "set_value", f"{LEVEL}.out0=1,2,3")
        refusal = str(q(tf, "last_refusal"))
        assert "Amount" in refusal and "Colour" in refusal, (
            f"D: PAST the DCC — the TYPE is checked and both are named: "
            f"{refusal!r}. The DCC gets this free from a different C struct "
            f"per socket type; here the taxonomy answers value_type"
        )
        refused(tf, "set_value", "99.out0=1")
        refused(tf, "set_value", f"{BASE}.sideways=1")
        refused(tf, "set_value", f"{BASE}.out0=nope")
        refused(tf, "set_value", f"{BASE}.out0")
        assert_eq(q(tf, "valid"), "ok", "D: and no refusal wrote anything")
        assert_eq(ports(tf, BASE)["authored"], "out0=200,60,60", "D: unchanged")

        # ── (E) a copy carries what was authored on it ──────────────────────
        assert_eq(inv(tf, "select", str(BASE)), "1", "E: select the base swatch")
        assert_eq(
            inv(tf, "duplicate", "60,60,drop,share"),
            "nodes:1|links:0|added:|reused:|reattached:0|unattached:|reframed:|unframed:",
            "E: duplicate it beside itself",
        )
        assert_eq(
            ports(tf, 7)["authored"],
            "out0=200,60,60",
            "E: a duplicated Swatch that came out grey is the defect "
            "`adopt_from` exists to prevent",
        )
        assert_eq(inv(tf, "node_value", "7"), "200,60,60")

        # ── (F) and across a group boundary ─────────────────────────────────
        assert_eq(inv(tf, "select", str(BASE)), "1", "F: select it again")
        assert_eq(inv(tf, "group", "Held"), "1:8|unframed:", "F: collapse it")
        assert_eq(
            inv(tf, "node_value", "8"),
            "200,60,60",
            "F: the definition emits what the node did — the value travelled "
            "to another tree with it",
        )
        assert_eq(inv(tf, "node_value", str(MIX)), SEEDED, "F: nothing moved")
        assert_eq(q(tf, "valid"), "ok")

        # ── (G) a value that outlived its port is reported ──────────────────
        assert_eq(inv(tf, "expose", "1"), "0", "G: give the definition an input")
        assert_eq(
            inv(tf, "set_value", "8.in0=7"),
            "authored",
            "G: author on the instance's new port",
        )
        assert_eq(q(tf, "valid"), "ok", "G: legal so far")
        assert_eq(inv(tf, "unexpose", "1.0"), "0", "G: take the port away")
        assert "StrayPortValue" in str(q(tf, "valid")), (
            f"G: the value outlived its port and validate NAMES it: "
            f"{q(tf, 'valid')!r}"
        )
        assert_eq(
            inv(tf, "clear_value", "8.in0"),
            "cleared 7",
            "G: and it can be taken back off",
        )
        assert_eq(q(tf, "valid"), "ok", "G: which restores the document")

        # ── (H) a bypassed node carries its routing, not its outputs ────────
        assert_eq(inv(tf, "bypass", str(BLEND)), f"{BLEND}=true", "H: bypass a source")
        assert_eq(
            inv(tf, "node_value", str(BLEND)),
            "null",
            "H: a source has no inputs, so a bypassed one passes nothing — and "
            "its authored value does NOT fill the gap, because R1586 names "
            "that output as dropped and filling it would make that a lie",
        )
        assert_eq(
            fields(inv(tf, "passthrough", str(BLEND)))["dropped"],
            "0",
            "H: named, as R1586 promised",
        )
        assert_eq(inv(tf, "bypass", str(BLEND)), f"{BLEND}=false", "H: and back")
        assert_eq(inv(tf, "node_value", str(BLEND)), "40,90,220", "H: restored")


if __name__ == "__main__":
    run_demo("r1594_a_value_is_authored_on_the_node", body)
