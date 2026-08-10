#!/usr/bin/env python3
"""R1632 — a port count is a property of the node, not of its kind.

The engine reference gives a node editor ten separate commands for a node whose
port count is its own: `AddExecutionPin`, `InsertExecutionPinBefore` / `After`,
`RemoveExecutionPin`, `AddOptionPin`, `RemoveOptionPin`,
`SoundCueGraph::AddInput` / `DeleteInput`, `AnimGraph::AddBlendListPin` /
`RemoveBlendListPin` — each one hand-written per node class. The DCC has four
more (`socket_items::` add / remove-by-index / remove-active / move).

`pinion_node_graph` is those fourteen as **two operations and a position**, and
this demo drives them through ONE verb over the wire.

What each check discriminates:

* **The port list is read from the PICTURE.** An item edit that computed the
  right model and never reached the canvas is the failure this is written
  against, so every port name here comes from a painted label.
* **An item is a PAIR.** `Layers` contributes a `Layer` and an `Opacity` per
  item — the engine's blend-list shape. A re-index that shifted by one port
  would pass with single-port items and fail here.
* **The fixed port PAST the run moves too.** `Gain` sits after the layers, so
  its own address depends on how many there are. Nothing on the node says so,
  and the reference ships this case unhandled:
  `//@TODO: ANIMREFACTOR: Need to handle moving pins below up correctly`.
* **Removal says what it cost.** The wire it cut and the value it handed back
  are on the wire. `RemoveExecutionPin` answers `void` after `BreakAllPinLinks`.
* **A move loses nothing.** A permutation has somewhere for every address to
  go, so `severed:` and `discarded:` empty is a claim about the arithmetic.
* **Names are derived.** After a removal the ordinals are compact with no
  renaming pass, and past 26 — the reference caps arity at `'Z' - 'A'` because
  its pins are *named* for the letters.
* **The composite still computes.** The picture and the model agreeing is not
  enough; the evaluated colour has to follow the layers to their new places.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1632_a_port_count_belongs_to_the_node.py
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
    texts_of,
)

#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"

#: The example's paint-tag prefix.
VIEW_TAG = "nodegroups"

#: `hello-node-groups`'s seed, mirrored rather than imported.
BASE, BLEND, LEVEL, MIX, FADE, OUT = 0, 1, 2, 3, 4, 5


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def fields(reply: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for piece in str(reply).split("|"):
        key, _, value = piece.partition(":")
        out[key] = value
    return out


def item(tf: RpcSubprocess, spec: str) -> dict[str, str]:
    return fields(inv(tf, "item", spec))


def refused(tf: RpcSubprocess, verb: str, spec: str) -> None:
    try:
        inv(tf, verb, spec)
    except Exception:  # noqa: BLE001 — the refusal is the assertion
        return
    raise AssertionError(f"{verb} {spec!r} should have been refused")


def painted_ports(tf: RpcSubprocess, node: int, side: str) -> list[str]:
    """Every input (or output) label of `node`, read off the painted scene.

    From the picture rather than the model: the resolved name of a variadic
    port is derived when the signature is resolved, so what an author actually
    reads is the only place worth asserting it.
    """
    snap = tf.snapshot(source="paint", viewport=[1100, 760])
    names: list[str] = []
    index = 0
    while True:
        found = find_by_tag(snap, f"{VIEW_TAG}.pinlabel.{node}.{side}.{index}")
        if found is None:
            return names
        text = texts_of(found)
        assert text, f"pinlabel {node}.{side}.{index} paints no text"
        names.append(text[0])
        index += 1


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── 1. a fresh variadic node arrives at its declared minimum ────
        layers = int(str(inv(tf, "add", "layers")))
        assert_eq(str(inv(tf, "select", str(layers))), "1", "selected the stack")
        tf.tick(0.016)
        assert_eq(
            painted_ports(tf, layers, "in"),
            ["Base", "Layer 0", "Opacity 0", "Gain"],
            "one layer, because that is the floor the kind declared",
        )
        print("[demo] a fresh Layers node paints its declared minimum")

        # ── 2. adding an item adds a PAIR, and Gain moves by two ────────
        added = item(tf, "add:in:1")
        assert_eq(added["items"], "2", "two layers now")
        assert_eq(
            added["added"],
            "in3,in4",
            "★ ONE item, TWO ports — the reference's blend-list shape",
        )
        assert "in3>in5" in added["moved"], (
            f"★ `Gain` moved by TWO, not by one: {added['moved']}"
        )
        assert_eq(added["severed"], "", "and an insert cuts nothing")
        tf.tick(0.016)
        assert_eq(
            painted_ports(tf, layers, "in"),
            ["Base", "Layer 0", "Opacity 0", "Layer 1", "Opacity 1", "Gain"],
            "and the canvas says the same",
        )
        print(f"[demo] add:in:1 -> added {added['added']}, moved {added['moved']}")

        # ── 3. wire every port, so a re-index has something to get wrong ─
        # The seed's three sources feed the stack: two colours and an amount.
        # Each source emits a DIFFERENT value, so "the wires moved correctly"
        # is a statement about which colour arrives where.
        inv(tf, "connect", f"{BASE}.0>{layers}.0")
        inv(tf, "connect", f"{BLEND}.0>{layers}.1")
        inv(tf, "connect", f"{LEVEL}.0>{layers}.2")
        inv(tf, "connect", f"{BASE}.0>{layers}.3")
        inv(tf, "connect", f"{LEVEL}.0>{layers}.5")
        wired = int(str(tf.query(f"{EXT}/links")))
        assert wired >= 5, f"the stack is wired: {wired}"
        inv(tf, "set_value", f"{layers}.in4=90")
        print("[demo] every port of the stack is wired or authored")

        # ── 4. a THIRD layer, inserted BEFORE the second ────────────────
        before = item(tf, "add:in:1")
        assert_eq(before["items"], "3")
        assert "in3>in5" in before["moved"], (
            f"the layer that was at item 1 moved to item 2: {before['moved']}"
        )
        assert "in5>in7" in before["moved"], f"and Gain with it: {before['moved']}"
        assert_eq(before["severed"], "", "still lossless")
        tf.tick(0.016)
        assert_eq(
            painted_ports(tf, layers, "in"),
            [
                "Base",
                "Layer 0",
                "Opacity 0",
                "Layer 1",
                "Opacity 1",
                "Layer 2",
                "Opacity 2",
                "Gain",
            ],
        )
        print("[demo] insert-before pushes the item that was there along")
        # ★ And the value authored on the second layer's opacity rode two
        # places up with it. Read HERE and not only after the removal, because
        # the two edits happen to bring it back to the index it started at —
        # a fixture that only looked at the end would pass an implementation
        # that never moved it at all.
        assert "in6=90" in str(inv(tf, "port_values", str(layers))), (
            "the authored opacity moved from in4 to in6 with its item"
        )

        # ── 5. a removal NAMES the wires it cut and the values it kept ──
        removed = item(tf, "remove:in:0")
        assert_eq(removed["items"], "2")
        assert removed["severed"], (
            "★ the wires the removal cut are on the wire, where the "
            f"reference's command answers void: {removed}"
        )
        assert_eq(
            len(removed["severed"].split(",")),
            2,
            f"an item of two ports takes two wires: {removed['severed']}",
        )
        assert "in7>in5" in removed["moved"], (
            f"★ and `Gain` came back down by two: {removed['moved']}"
        )
        tf.tick(0.016)
        assert_eq(
            painted_ports(tf, layers, "in"),
            ["Base", "Layer 0", "Opacity 0", "Layer 1", "Opacity 1", "Gain"],
            "the names are compact again, with no renaming pass — they are "
            "derived from the ordinal every time the signature resolves",
        )
        print(f"[demo] remove:in:0 -> severed {removed['severed']}")

        # ── 6. the authored value followed its port ─────────────────────
        # `in4` held 90 before two ports were removed from below it, so it is
        # `in2` now. A re-index that moved links and not values would leave the
        # 90 addressing a port that belongs to another layer.
        values = str(inv(tf, "port_values", str(layers)))
        assert "authored:in4=90" in values, (
            f"★ and back down to in4 when the item below it went: {values}"
        )
        print(f"[demo] the authored value rode along: {values}")

        # ── 7. a move loses nothing at all ──────────────────────────────
        moved = item(tf, "move:in:0:1")
        assert_eq(moved["severed"], "", "★ a permutation severs nothing")
        assert_eq(moved["discarded"], "", "and discards nothing")
        assert_eq(moved["items"], "2", "and is the same length")
        assert moved["moved"], f"while every address did move: {moved}"
        back = item(tf, "move:in:1:0")
        assert_eq(back["severed"], "")
        assert_eq(
            str(inv(tf, "port_values", str(layers))),
            values,
            "and moving back is the identity, which a shift-by-one is not",
        )
        print("[demo] a move is a permutation: nothing severed, nothing discarded")

        # ── 8. an item may be NAMED, and the rest keep their ordinals ───
        item(tf, "add:in:0:Underlay")
        tf.tick(0.016)
        assert_eq(
            painted_ports(tf, layers, "in"),
            [
                "Base",
                "Underlay Layer",
                "Underlay Opacity",
                "Layer 1",
                "Opacity 1",
                "Layer 2",
                "Opacity 2",
                "Gain",
            ],
            "★ the authored name reaches both of its item's ports, and the "
            "unnamed items renumber around it",
        )
        print("[demo] a named item names its ports; the others keep ordinals")

        # ── 9. the composite still computes, and follows the layers ─────
        # The picture and the model agreeing is not enough: the node's own
        # output has to be the composite of the layers where they now are.
        # ★ Distinct colours and distinct opacities first. Two layers that
        # composite to the same thing whatever their order would make this
        # check pass on an implementation that never reordered anything —
        # which is exactly what an earlier draft of this fixture did.
        for port, value in (
            ("in1", "10,20,30"),
            ("in2", "50"),
            ("in3", "200,10,10"),
            ("in4", "25"),
        ):
            inv(tf, "set_value", f"{layers}.{port}={value}")
        composite = fields(str(inv(tf, "port_values", str(layers))))["carries"]
        assert "out0=" in composite, f"the stack computes: {composite}"
        was = composite.split("out0=")[1]
        item(tf, "move:in:0:1")
        after_move = fields(str(inv(tf, "port_values", str(layers))))["carries"]
        assert after_move.split("out0=")[1] != was, (
            "★ reordering the layers changes the composite — the evaluator "
            f"reads the run the NODE has: {was} -> {after_move}"
        )
        item(tf, "move:in:1:0")
        assert_eq(
            fields(str(inv(tf, "port_values", str(layers))))["carries"].split("out0=")[1],
            was,
            "and moving back restores it exactly",
        )
        assert_eq(str(tf.query(f"{EXT}/valid")), "ok", "valid after every edit")
        print(f"[demo] the composite follows the layers: {was}")

        # ── 10. the declaration is what refuses, not a menu ─────────────
        for _ in range(3):
            item(tf, "add:in:0")
        refused(tf, "item", "add:in:0")  # the ceiling of six
        while True:
            reply = item(tf, "remove:in:0")
            if reply["items"] == "1":
                break
        refused(tf, "item", "remove:in:0")  # the floor of one
        refused(tf, "item", "add:out:0")  # the other side has no run
        refused(tf, "item", "add:sideways:0")
        refused(tf, "item", "shuffle:in:0")
        refused(tf, "item", "remove:in:9")
        print("[demo] six spellings outside the declaration refused")

        # ── 11. a kind with no run has no items at all ──────────────────
        inv(tf, "select", str(MIX))
        refused(tf, "item", "add:in:0")
        print("[demo] a fixed-arity kind refuses an item edit")

        # ── 12. past the alphabet, which the reference cannot reach ─────
        inv(tf, "select", str(layers))
        # Six is this kind's own ceiling, so the unbounded case is asserted on
        # the crate's side; what the wire shows is that the ceiling is the
        # DECLARATION's and is reported as such rather than as a silent no-op.
        grown = item(tf, "add:in:1")
        assert_eq(grown["items"], "2")
        assert_eq(
            painted_ports(tf, layers, "in")[3:5],
            ["Layer 1", "Opacity 1"],
            "the ordinal is applied at resolve time, so it is never stale",
        )

        # ── 13. and the two verbs are DECLARED on the surface ───────────
        # ★ Only half-checkable from here, and saying which half is the point:
        # an `External`'s declared action list does not reach the wire at all
        # (measured this round — it is in neither snapshot source, and there is
        # no `scene/schema`), so what a client can observe is that the declared
        # name is ACCEPTED and an undeclared one is not. The gap is registered
        # rather than papered over.
        assert_eq(item(tf, "add:in:1")["items"], "3", "`item` is accepted")
        refused(tf, "kern", "in:0")
        print("[demo] item answers where an undeclared verb does not")

if __name__ == "__main__":
    run_demo("R1632 — a port count belongs to the node", body)
