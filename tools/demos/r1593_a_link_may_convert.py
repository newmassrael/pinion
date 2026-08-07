#!/usr/bin/env python3
"""R1593 §5.38 §5.52 §2 #7 — a link may convert.

A node system's type relation is **directed**. A scalar feeds a colour by
broadcasting; a colour never narrows back into a scalar. Until this round
`pinion-node-graph` decided a wire with `source.ty != sink.ty`, and `!=` is
symmetric — so the crate's own doc, which told a coercing taxonomy to "model
that by making the coercion part of equality", was asking for something no
equality relation can be. Making the two types equal would have admitted the
narrowing too.

So the relation is declared by the taxonomy, once, **as the conversion itself**
(`NodeKind::conversion`). That single declaration decides four things: whether a
wire is accepted, what arrives along it, which input a bypassed node routes to
which output, and whether a document that arrived from a file still type-checks.

What this script checks, and why each check discriminates:

* **The relation is asymmetric.** The same document accepts `Amount -> Colour`
  and refuses `Colour -> Amount`. No `PartialEq` implementation can produce
  those two answers together, which is the proof the old contract could not
  express this application's own lattice.
* **The question is answerable before the wire exists.** `conversion` takes the
  same argument spelling `connect` does, so "may I?" and "do it" name the wire
  the same way, and it answers with both types as well as the verdict.
* **PAST BLENDER (1): legality and the conversion are ONE declaration.** Blender
  keeps them apart in three places that can disagree — `validate_link` (a
  `bNodeTreeType` C function pointer that says whether a wire may exist),
  `DataTypeConversions` (a global `Map<(from, to), ConversionFunctions>` holding
  the actual conversion) and `get_internal_link_type_priority` (a static
  socket-type table used when a node is muted). Asserted here by driving the
  wire and reading the value that arrives: acceptance entails carriage.
* **PAST BLENDER (2): the crossing is ASKABLE at all.** `validate_link` is a
  function pointer reached through `ntree.typeinfo`; Blender exposes no accessor
  for "would this wire be legal, and would the value change". Here it is a
  method and a wire read.
* **PAST BLENDER (3): showing an implicit conversion costs nothing.** Blender
  makes the fact visible by materialising a whole `implicit_conversion` node
  into the tree (`register_node_type_implicit_conversion`, `node_common.cc`), so
  seeing it changes the graph you are looking at. Here the node count is
  asserted unchanged and the wire is drawn dotted.
* **PAST BLENDER (4): a bypassed node routes by the SAME relation.** Blender
  answers that from its third table, unrelated to either of the other two, so
  what a muted node passes through there can disagree with what a wire in the
  same position would have carried.
* **The rule for a bypass is one sentence: the identity as far as the signature
  allows, and it changes a value only when it has to.** A direct crossing beats
  a converting one even at the output's own index, and a converting route is
  reported as not the identity.
* **The three wire looks are ORDERED, not merged.** A muted wire carries no
  value, so there is nothing for it to convert; a muted converting wire is drawn
  as muted.
* **A saved document is re-checked against the same relation.** A broadcast
  survives a reload without being called a mismatch.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1593_a_link_may_convert.py
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

#: Link ids, in the order the seed wires them.
L_BASE, L_BLEND, L_LEVEL, L_MIX, L_FADE = 0, 1, 2, 3, 4

#: mix(200,60,60 @ 75% with 40,90,220 @ 25%) — what the seeded Mix answers.
SEEDED = "160,67,100"

#: The seeded Level is 25%, and the broadcast is `25 * 255 / 100`.
BROADCAST_OF_25 = "63,63,63"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def fields(reply: str) -> dict[str, str]:
    """`"routes:0<-0|dropped:|identity:true"` -> a dict."""
    out: dict[str, str] = {}
    for piece in str(reply).split("|"):
        key, _, value = piece.partition(":")
        out[key] = value
    return out


def through(tf: RpcSubprocess, node: int) -> dict[str, str]:
    return fields(inv(tf, "passthrough", str(node)))


def conversions(tf: RpcSubprocess) -> dict[str, str]:
    """`"0=direct,2=converted"` -> a dict keyed by link id."""
    raw = str(q(tf, "link_conversions"))
    out: dict[str, str] = {}
    for piece in raw.split(","):
        if not piece:
            continue
        key, _, value = piece.partition("=")
        out[key] = value
    return out


#: `Dash::DOTTED` / `Dash::DASHED` as they arrive over the wire — the two
#: rhythms `pinion_core::style::Dash` ships, read rather than assumed.
DOTTED = {"on": 1, "off": 3, "offset": 0, "period": 4}
DASHED = {"on": 6, "off": 4, "offset": 0, "period": 10}


def wire_dash(tf: RpcSubprocess, link: int):
    """The stroke rhythm the PAINT gave `nodegroups.wire.<link>`."""
    found: list = []

    def walk(node) -> None:
        if isinstance(node, dict):
            if node.get("tag") == f"nodegroups.wire.{link}":
                found.append(node["style"]["stroke"]["dash"])
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    walk(tf.snapshot(source="paint"))
    assert len(found) == 1, f"exactly one wire.{link} in the paint, got {len(found)}"
    return found[0]


def refused(tf: RpcSubprocess, path: str, args) -> None:
    """Run a verb that must be refused."""
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

        # ── (A) the relation, asked before any wire is made ──────────────────
        assert_eq(q(tf, "nodes"), 6, "A: the seeded material")
        assert_eq(q(tf, "links"), 5, "A: and its five wires")
        assert_eq(
            inv(tf, "conversion", f"{BASE}.0>{MIX}.0"),
            "colour->colour direct",
            "A: like feeds like, unchanged",
        )
        assert_eq(
            inv(tf, "conversion", f"{LEVEL}.0>{FADE}.0"),
            "amount->colour converted",
            "A: PAST BLENDER — the conversion is ASKABLE, and it says the value "
            "would be CHANGED. Blender's validate_link is a C function pointer "
            "on the tree type with no accessor, and whether the value would "
            "change lives in a different table again",
        )
        assert_eq(
            inv(tf, "conversion", f"{BASE}.0>{MIX}.2"),
            "colour->amount refused",
            "A: and the narrowing is refused",
        )
        assert_eq(
            inv(tf, "conversion", f"{LEVEL}.0>{MIX}.2"),
            "amount->amount direct",
            "A: the seeded Factor wire, unchanged",
        )
        # "There is no such port" is a different answer from "no value may go
        # there", so it is a refusal of the READ rather than a third verdict.
        refused(tf, "conversion", f"{LEVEL}.9>{MIX}.0")
        refused(tf, "conversion", f"99.0>{MIX}.0")

        # ── (B) the same document answers the mirrored pair differently ──────
        # This is the headline. `connect`'s gate WAS `source.ty != sink.ty`, and
        # `!=` is symmetric, so no equality relation can give both of these.
        assert_eq(
            inv(tf, "connect", f"{LEVEL}.0>{FADE}.0"),
            "linked, displaced 3",
            "B: an amount broadcasts into a colour input, and the wire it "
            "replaced is named",
        )
        refused(tf, "connect", f"{BASE}.0>{MIX}.2")
        refusal = str(q(tf, "last_refusal"))
        assert "Colour" in refusal, f"B: the refusal names the source type: {refusal!r}"
        assert "Amount" in refusal, f"B: and the destination type: {refusal!r}"

        # ── (C) acceptance entails carriage: ONE declaration ─────────────────
        assert_eq(
            inv(tf, "node_value", str(FADE)),
            BROADCAST_OF_25,
            "C: PAST BLENDER — the value ARRIVED converted. Legality and the "
            "conversion are one declaration, so a wire this crate accepts is a "
            "wire it can carry a value along; Blender's shader validate_link "
            "returns true for pairs its conversion table has no entry for",
        )
        assert_eq(
            inv(tf, "node_value", str(OUT)),
            "",
            "C: the sink has no outputs of its own",
        )
        assert_eq(
            q(tf, "valid"),
            "ok",
            "C: and the document is still well typed with a broadcast in it",
        )
        assert_eq(
            q(tf, "nodes"),
            6,
            "C: PAST BLENDER — showing the conversion cost NO node. Blender "
            "materialises an implicit_conversion node into the tree, so seeing "
            "the fact changes the graph you are looking at",
        )

        # ── (D) which wires convert, read off the model ──────────────────────
        marks = conversions(tf)
        assert_eq(marks[str(L_BASE)], "direct", "D: the Base wire is unchanged")
        assert_eq(marks[str(L_LEVEL)], "direct", "D: so is the Factor wire")
        assert_eq(marks["5"], "converted", "D: and the new wire converts")
        assert_eq(len(marks), 5, "D: five wires, one of them converting")

        # ── (E) the wire that converts is DRAWN differently ──────────────────
        assert_eq(wire_dash(tf, L_BASE), None, "E: a direct wire is solid")
        assert_eq(
            wire_dash(tf, 5),
            DOTTED,
            "E: and a converting one is dotted — a third arm of the stroke "
            "vocabulary R1575 opened, so a reader tells the three apart "
            "without a legend and a colour-blind reader tells them apart at all",
        )

        # ── (F) a bypassed node routes by the SAME relation ──────────────────
        assert_eq(inv(tf, "add", "tint"), "6", "F: a Tint — amount in, colour out")
        assert_eq(
            inv(tf, "connect", f"{LEVEL}.0>6.0"),
            "linked",
            "F: fed the same 25%",
        )
        assert_eq(
            inv(tf, "node_value", "6"),
            "50,50,50",
            "F: computing, a Tint DOUBLES its amount",
        )
        tint = through(tf, 6)
        assert_eq(
            tint["routes"],
            "0<-0",
            "F: PAST BLENDER — its only pass-through goes through the lattice's "
            "broadcast. Blender answers this from a THIRD table "
            "(get_internal_link_type_priority), unrelated to the one that "
            "validates a wire and to the one that converts a value",
        )
        assert_eq(tint["converting"], "0", "F: and the routing SAYS the value changes")
        assert_eq(
            tint["identity"],
            "false",
            "F: same index, but what leaves is not what arrived",
        )
        assert_eq(tint["dropped"], "", "F: nothing is dropped")
        assert_eq(inv(tf, "bypass", "6"), "6=true", "F: take it out of the meaning")
        assert_eq(
            inv(tf, "node_value", "6"),
            BROADCAST_OF_25,
            "F: bypassed, the amount crosses through the DECLARED conversion — "
            "a different number from the one the node computes, so this cannot "
            "be confused with the node having run",
        )
        assert_eq(inv(tf, "bypass", "6"), "6=false", "F: and back")

        # ── (G) a direct crossing beats a converting one ─────────────────────
        # `Glaze` is (Strength: Amount, Colour: Colour) -> Colour. The output's
        # OWN index holds the amount, which could reach it by CONVERTING; index
        # 1 holds the colour, which reaches it unchanged. A rule that ranked
        # position above the value would route 0<-0 here — which is exactly the
        # counterfactual, and why the assertion is made on this shape rather
        # than on Fade, where both rules give the same answer.
        assert_eq(inv(tf, "add", "glaze"), "7", "G: a Glaze")
        glaze = through(tf, 7)
        assert_eq(
            glaze["routes"],
            "0<-1",
            "G: the value that survives wins over the port that shares the "
            "output's index",
        )
        assert_eq(glaze["converting"], "", "G: so nothing on this node converts")
        assert_eq(glaze["unreached"], "0", "G: and the Strength reaches nothing")
        assert_eq(
            glaze["identity"],
            "false",
            "G: the value does not leave by the port it arrived on",
        )
        # Driven end to end, so the routing is not merely reported but USED.
        assert_eq(
            inv(tf, "connect", f"{BASE}.0>7.1"), "linked", "G: feed it a colour"
        )
        assert_eq(inv(tf, "node_value", "7"), "200,60,60", "G: strength 0 keeps it")
        assert_eq(inv(tf, "bypass", "7"), "7=true", "G: bypass it")
        assert_eq(
            inv(tf, "node_value", "7"),
            "200,60,60",
            "G: the COLOUR passed through, not the grey of the strength — "
            "which is what the preference decides",
        )
        assert_eq(inv(tf, "bypass", "7"), "7=false", "G: and back")

        fade = through(tf, FADE)
        assert_eq(fade["routes"], "0<-0", "G: Fade is unchanged by any of this")
        assert_eq(fade["unreached"], "1", "G: and its Factor still reaches nothing")

        # ── (H) the three looks are ordered, not merged ──────────────────────
        assert_eq(q(tf, "muted_links"), "", "H: no wire is muted yet")
        assert_eq(inv(tf, "mute_link", "5"), "muted", "H: stop the converting wire")
        assert_eq(q(tf, "muted_links"), "5", "H: it says so")
        assert_eq(
            conversions(tf)["5"],
            "converted",
            "H: the crossing is still what it is — muting stops a value, it "
            "does not change what the two types are",
        )
        assert_eq(
            inv(tf, "node_value", str(FADE)),
            "0,0,0",
            "H: and with nothing arriving, the port falls back to its default",
        )
        assert_eq(
            wire_dash(tf, 5),
            DASHED,
            "H: the wire is drawn MUTED, not converting. The two facts are "
            "ordered rather than merged: a wire carrying nothing has nothing "
            "to convert, so drawing it as converting would say something false",
        )
        assert_eq(inv(tf, "mute_link", "5"), "unmuted", "H: let it through again")
        assert_eq(inv(tf, "node_value", str(FADE)), BROADCAST_OF_25, "H: restored")
        assert_eq(wire_dash(tf, 5), DOTTED, "H: and dotted again")

        # ── (I) the relation survives a group boundary ───────────────────────
        assert_eq(inv(tf, "select", "4"), "1", "I: select the Fade")
        assert_eq(inv(tf, "group", "Grey"), "1:8|unframed:", "I: collapse it")
        group = 8
        assert_eq(
            inv(tf, "interface", "1"),
            "in:Colour:colour|out:Colour:colour",
            "I: the derived interface takes the CONSUMER's type",
        )
        converting = [k for k, v in conversions(tf).items() if v == "converted"]
        assert_eq(
            len(converting),
            1,
            "I: so the conversion stayed on ONE wire OUTSIDE the group — read "
            "off the model rather than argued for. Addressed by counting "
            "rather than by a link id, because grouping mints fresh ids",
        )
        assert_eq(
            inv(tf, "node_value", str(group)),
            BROADCAST_OF_25,
            "I: the group computes what the node computed — the broadcast "
            "crossed the derived boundary unchanged in meaning",
        )
        assert_eq(q(tf, "valid"), "ok", "I: and the document is still well typed")

        # ── (J) the palette names itself ─────────────────────────────────────
        refused(tf, "add", "nope")
        message = str(q(tf, "last_refusal"))
        assert "glaze" in message, f"J: the refusal names the real palette: {message!r}"
        assert "cap" in message, f"J: including the one it used to omit: {message!r}"


if __name__ == "__main__":
    run_demo("r1593_a_link_may_convert", body)
