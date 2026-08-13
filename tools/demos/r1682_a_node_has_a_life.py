#!/usr/bin/env python3
"""R1682 §5.20 §5.21 — a node keeps its name while its name changes.

Drives `hello-node-lab` over JSON-RPC. The analysis tool's node-graph screen
could make a node and could do nothing else to one: of the reference's thirty
operations, four were a node's own life — delete it, rename it, collapse it,
switch it off — and all four were absent together, exactly as a link's second
half was before the round before this one.

The round's claim is not "four actions exist". It is that **a rename is not a
re-creation**. The reference prototype has no rename, so it copies the node
under the new name and covers the old one with a deletion — and then has to
hand-move ten side tables to compensate. Here the model renames in place, keeps
the node's identity, and refuses a name another node already answers to; every
per-card record on the screen is keyed by that identity, so the list of things a
rename has to carry is empty. That is asserted here rather than described.

Measured on the reference toolkit 6.11.1 offscreen, which is the floor this is
built to beat: an object's identity survives a rename there too, but naming a
second sibling what the first is called is ACCEPTED, both then hold the name,
its by-name lookup answers one of the two and says nothing, and the rename
notification carries only the new name — so a listener keyed by the old one is
told that something changed and not what to un-key.

  (A) boot — the three seats are painted for the selected card, and the wire
      reports both of a card's switches.
  (B) the wire declares the four verbs, and the operation table says which of
      them a pointer can also reach.
  (C) collapse — through the wire and through a real router press; it is a
      LOOK, so the card's painted rectangle is what moves.
  (D) switch off — through both channels; it is what the graph MEANS, and it
      moves nothing a collapse moves.
  (E) rename — the same card answers to the new name, everything hung on it is
      untouched, a taken name is refused, and the node reset puts it back
      WITHOUT deleting the card.
  (F) delete — through a router press, taking its links with it; the last card
      is refused.

>= 30 assertions.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    assert_router_press_moves,
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"
VIEWPORT = (1440, 900)


def q(tf, path):
    return tf.query(f"{EXT}/{path}")


def cards(tf) -> dict:
    return json.loads(q(tf, "cards"))


def rects(tf) -> dict:
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def refused(tf, verb, args) -> str:
    """Invoke something that must be refused, and answer the sentence."""
    try:
        tf.invoke(f"{EXT}/{verb}", args)
    except RpcError as why:
        return str(why)
    raise AssertionError(f"{verb} {args!r} was accepted and had to be refused")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot ────────────────────────────────────────────────
        opening = cards(tf)
        assert_eq(
            sorted(opening),
            sorted(q(tf, "nodes").split(",")),
            "every card the canvas draws reports its switches",
        )
        for name, switch in opening.items():
            assert_eq(switch["collapsed"], False, f"{name} opens full size")
            assert_eq(switch["disabled"], False, f"{name} opens running")

        painted = rects(tf)
        for seat in ("lab.inspector.collapse", "lab.inspector.disable", "lab.inspector.delete"):
            assert seat in painted, f"★ {seat} is on the opening screen"

        # ── (B) the vocabulary is declared, not guessed ─────────────
        spec = json.loads(q(tf, "spec"))
        ops = {op["name"]: op for op in spec["operations"]}
        for name in ("delete a node", "rename a node", "collapse a node", "disable a node"):
            assert_eq(ops[name]["absent"], False, f"{name!r} is answered now")
        # R1683 — the rename had no gesture when this demo was written, because
        # the screen had no text entry at all. It has one now, so the row this
        # demo asserted as the exception is an exception no longer.
        for name in ("delete a node", "collapse a node", "disable a node", "rename a node"):
            assert_eq(ops[name]["gesture"], True, f"{name!r} has a way in for a person")

        # ── (C) collapse is a LOOK ──────────────────────────────────
        tf.invoke(f"{EXT}/select", "P-03")
        assert_eq(q(tf, "selected"), "P-03", "the inspector's subject")
        before = rects(tf)["lab.node.P-03"]
        assert_eq(tf.invoke(f"{EXT}/collapse", "P-03"), "true", "the toggle answers its state")
        assert_eq(cards(tf)["P-03"]["collapsed"], True, "★ and the wire reports it")
        painted = rects(tf)
        after = painted["lab.node.P-03"]
        assert after[3] < before[3], (
            f"★ a collapsed card is DRAWN smaller — {after[3]} < {before[3]} — "
            f"which is the observable a look has"
        )
        assert after[2] < before[2], (
            f"★ and narrower — {after[2]} < {before[2]}"
        )
        # ★★ Everything that FOLLOWS the card follows it. A collapse that moved
        # the box and left the pins where they were would strand every wire
        # into the card, which is the shape of defect this screen keeps
        # producing — a derivation that half-applied.
        for side in ("dial", "accept"):
            tag = f"lab.pin.P-03.{side}"
            assert tag in painted, f"{tag} is still painted"
            px, py, pw, ph = painted[tag]
            assert (
                after[0] - 8 <= px + pw // 2 <= after[0] + after[2] + 8
                and after[1] - 8 <= py + ph // 2 <= after[1] + after[3] + 8
            ), f"★ {tag} moved with the card it belongs to, not to where it was"
        # And it cannot have grown into a neighbour: R1655's invariant, which
        # a floating affordance broke one round ago.
        for tag, box in painted.items():
            # Cards only: `lab.node.<name>` with nothing after it. The parts
            # INSIDE a card (`lab.node.P-03.id`) legitimately overlap it.
            rest = tag.removeprefix("lab.node.")
            if rest == tag or "." in rest or tag == "lab.node.P-03":
                continue
            apart = (
                after[0] + after[2] <= box[0]
                or box[0] + box[2] <= after[0]
                or after[1] + after[3] <= box[1]
                or box[1] + box[3] <= after[1]
            )
            assert apart, f"★ the collapsed card overlaps {tag}"
        assert_eq(cards(tf)["P-03"]["disabled"], False, "and it still runs")

        # The same toggle through a real router press, which is the column a
        # wire-driven test cannot see.
        assert_router_press_moves(
            tf, "lab.inspector.collapse", lambda: q(tf, "cards"), "the card expands again"
        )
        assert_eq(cards(tf)["P-03"]["collapsed"], False, "★ the seat toggles it back")
        assert_eq(rects(tf)["lab.node.P-03"], before, "and the card is its old size")

        # ── (D) switching off is what the graph MEANS ───────────────
        assert_eq(tf.invoke(f"{EXT}/disable", "P-03"), "true", "the toggle answers its state")
        assert_eq(cards(tf)["P-03"]["disabled"], True, "★ the wire reports it")
        assert_eq(
            cards(tf)["P-03"]["collapsed"],
            False,
            "★ and switching off moved nothing a collapse moves — two facts, "
            "kept apart",
        )
        assert_router_press_moves(
            tf, "lab.inspector.disable", lambda: q(tf, "cards"), "the card runs again"
        )
        assert_eq(cards(tf)["P-03"]["disabled"], False, "the seat toggles it back")

        # ── (E) a rename is not a re-creation ───────────────────────
        layout_before = json.loads(q(tf, "layout"))["P-03"]
        frame_before = json.loads(q(tf, "frames"))["P-03"]
        links_before = json.loads(q(tf, "links"))
        form_before = q(tf, "form")
        painted_before = rects(tf)
        card_before = painted_before["lab.node.P-03"]
        digest_before = sorted(
            tag.removeprefix("lab.node.P-03.")
            for tag in painted_before
            if tag.startswith("lab.node.P-03.")
        )

        assert_eq(tf.invoke(f"{EXT}/rename", "P-03,edge-01"), "edge-01", "the new name")
        assert "edge-01" in q(tf, "nodes").split(","), "★ the canvas calls it that now"
        assert "P-03" not in q(tf, "nodes").split(","), "and not the old name"
        assert_eq(q(tf, "selected"), "edge-01", "the selection is the thing that moved")
        assert_eq(q(tf, "form"), form_before, "★ its form is untouched")
        assert_eq(
            json.loads(q(tf, "layout"))["edge-01"],
            layout_before,
            "★ it sits exactly where it sat",
        )
        assert_eq(
            json.loads(q(tf, "frames"))["edge-01"], frame_before, "★ on the host it was on"
        )
        renamed = json.loads(q(tf, "links"))
        assert_eq(
            [link["id"] for link in renamed],
            [link["id"] for link in links_before],
            "★★ no link was re-minted — the reference remakes the node here "
            "and every link identity changes with it",
        )
        assert_eq(
            sum(1 for link in renamed if "edge-01" in (link["from"], link["to"])),
            sum(1 for link in links_before if "P-03" in (link["from"], link["to"])),
            "and the same links still run to it, under the new name",
        )

        # ★★ And the CARD is the same card to look at. Two derivations on this
        # screen found their specification row by the card's current name — its
        # digest lines and its width — so a rename made a card silently redraw
        # itself from a different source and snap to the default width. Nothing
        # was broken enough to fail, which is how it would have stayed.
        painted_after = rects(tf)
        card_after = painted_after["lab.node.edge-01"]
        assert_eq(
            (card_after[2], card_after[3]),
            (card_before[2], card_before[3]),
            "★★ a renamed card is drawn at the size it was drawn at",
        )
        assert_eq(
            sorted(
                tag.removeprefix("lab.node.edge-01.")
                for tag in painted_after
                if tag.startswith("lab.node.edge-01.")
            ),
            digest_before,
            "★★ and shows the same digest lines — they are the specification's, "
            "and a rename does not change which row of it this card came from",
        )

        # A name another card answers to is REFUSED, by the model.
        why = refused(tf, "rename", "edge-01,P-01")
        assert "already called" in why, f"★ the refusal names the collision: {why}"
        assert_eq(q(tf, "selected"), "edge-01", "and nothing moved")
        blank = refused(tf, "rename", "edge-01,   ")
        assert "no name" in blank, f"a blank name is its own refusal: {blank}"

        # The node reset puts the name back and does NOT delete the card —
        # which it would have, when a stray was "a name the specification does
        # not have".
        assert_eq(
            json.loads(q(tf, "changed"))["nodes"], True, "a renamed card is a changed node set"
        )
        count = len(q(tf, "nodes").split(","))
        tf.invoke(f"{EXT}/reset", "nodes")
        assert_eq(
            len(q(tf, "nodes").split(",")),
            count,
            "★★ the card is still on the canvas after the reset",
        )
        assert "P-03" in q(tf, "nodes").split(","), "★ and it is called what it opened as"
        assert_eq(json.loads(q(tf, "changed"))["nodes"], False, "nothing left to put back")

        # ── (F) delete takes its links with it ──────────────────────
        tf.invoke(f"{EXT}/select", "P-03")
        touching = [
            link for link in json.loads(q(tf, "links")) if "P-03" in (link["from"], link["to"])
        ]
        assert touching, "the card this demo deletes has links, or it proves nothing"
        assert_router_press_moves(
            tf, "lab.inspector.delete", lambda: q(tf, "nodes"), "the card goes"
        )
        assert "P-03" not in q(tf, "nodes").split(","), "★ pressed where it is painted"
        assert_eq(
            [link for link in json.loads(q(tf, "links")) if "P-03" in (link["from"], link["to"])],
            [],
            "★ and its links went with it",
        )
        assert "P-03" not in cards(tf), "the switch report has no row for a card that is gone"

        # The last card stays. Deleting down to one is how the refusal is
        # reached, and it is reached through the wire because a demo pressing a
        # seat forty times is a demo about pressing.
        while len(q(tf, "nodes").split(",")) > 1:
            tf.invoke(f"{EXT}/delete_node", q(tf, "nodes").split(",")[0])
        last = q(tf, "nodes")
        why = refused(tf, "delete_node", last)
        assert "last card" in why, f"★ the last card stays, and says why: {why}"
        assert_eq(q(tf, "nodes"), last, "and it really is still there")
        assert_eq(q(tf, "selected"), last, "so the inspector still has a subject")


if __name__ == "__main__":
    sys.exit(run_demo("R1682 §5.21 — a node has a life", body))
