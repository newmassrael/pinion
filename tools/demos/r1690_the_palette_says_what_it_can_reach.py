#!/usr/bin/env python3
"""R1690 §5.20 §5.21 — the palette says how much of the option surface it can
author, and every string on that surface has a shape or is named as having none.

Drives `hello-node-lab` over JSON-RPC, in a real window, with a real pointer.

The reference tool publishes **four** self-censuses. Two of them this screen
already mirrors — its operation table (R1677) and its save partition (R1689) —
and this round is the other two:

* how much of the configuration the palette can reach, as two numbers, and
* how much of the string half of that configuration is pinned down.

Neither number is written anywhere. Both are computed from the palette against a
declared option surface every time they are asked, which is the only version of
a coverage meter worth painting: drop a field and the figure falls with nobody
editing it. (D) is that property, driven.

The half the reference does not have is the one that found a defect. A reach
count answers "is the key offered" and stops, so it is blind to a key offered at
a shape the target refuses — the row is there, the value goes in, and the node
does not come up. This screen had exactly that: its node identifier is read by a
parser and was typed as **free text** for its whole life, and three of its own
opening values (`t1`, `t2`, `q1`) were outside the alphabet that parser accepts.
The launch gate found them the first time the shape came from the surface.

  (A) both meters answer on the wire, as numbers rather than as a sentence.
  (B) the pill on screen says exactly what the wire says — a rendering of its
      source, not a second count.
  (C) the surface is larger than the palette, in both directions, so the meter
      has room to report a loss and is not `n/n`.
  (D) narrowing the palette lowers the figure, driven through the screen; and
      taking a ROW off the screen does not, because the chip offers it back.
  (E) every string leaf is in exactly one of three classes and all three are
      populated — the partition is total and not a tautology.
  (F) the identifier's shape is enforced: a value outside its alphabet closes
      the launch gate and the refusal says what was wanted.
  (G) ...and it is enforced on both channels, from the wire and from a typed
      key, with the same sentence.
  (H) the tri-state: a half-typed address is not yet acceptable and is not
      refused either, which is what lets a person type one.
  (I) the meter stands with no card selected, because it is a fact about the
      tool.
  (J) the graph the screen opens with satisfies its own declared shapes — the
      regression gate for the three bad identifiers.
  (K) the gate panel is bounded by the canvas and says what it is not showing.

>= 30 assertions.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    texts_of,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"

_DESIGN: tuple[int, int] | None = None


def viewport(tf) -> tuple[int, int]:
    global _DESIGN
    if _DESIGN is None:
        design = json.loads(q(tf, "spec"))["design"]
        _DESIGN = (design[0], design[1])
    return _DESIGN


def q(tf, path):
    return tf.query(f"{EXT}/{path}")


def rects(tf):
    return abs_rects_of(tf.snapshot(source="paint", viewport=viewport(tf)))


def find_restorable(tf) -> tuple[str, str]:
    """The first `(card, row)` where taking the row off is UNDOABLE.

    ★★★★★ R1850 — asked, not named. This file used to drive a key written into
    its own source, and R1842 rewrote the option surface from the target's own
    declaration: that key stopped being one the screen offers back, so the
    round trip below had no chip to press and the gate went red reporting a
    defect that was its own copy going stale. A gate that must NAME a key
    carries a duplicate of something the screen owns.

    Two conditions, and both are measured rather than assumed:

    * the row is **offerable** — `catalogue.restorable` is the screen's own
      answer to "which of these come back if you take them off";
    * the row is **written**, not worked out from the canvas. A derived row is
      disowned rather than removed, so it never leaves the form and the claim
      below would be about a different act.
    """
    for card in [c for c in q(tf, "nodes").split(",") if c]:
        tf.invoke(f"{EXT}/select", card)
        rows = {f["key"]: f for f in json.loads(q(tf, "form"))}
        for key in json.loads(q(tf, "catalogue"))["restorable"]:
            if rows[key].get("source") is None:
                return card, key
    raise AssertionError(
        "no card has a written row the screen can offer back — the round trip "
        "this section is about is unreachable, which is a finding and not a "
        "reason to skip"
    )


def press(tf, tag):
    box = rects(tf)[tag]
    tf.click(at=(box[0] + box[2] // 2, box[1] + box[3] // 2))


def run_of(tf, tag: str) -> str:
    """What the run tagged `tag` says, read out of the painted scene."""
    node = find_by_tag(tf.snapshot(source="paint", viewport=viewport(tf)), tag)
    assert node is not None, f"{tag} is painted"
    said = texts_of(node)
    assert said, f"{tag} is a painted run"
    return said[0]


def reach(tf) -> dict:
    return json.loads(q(tf, "reach"))


def strings(tf) -> dict:
    return json.loads(q(tf, "strings"))


def fraction(text: str) -> tuple[int, int]:
    hit, total = text.split("/")
    return int(hit), int(total)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        spec = json.loads(q(tf, "spec"))

        # ── (A) both meters answer, as numbers ──────────────────────
        r = reach(tf)
        s = strings(tf)
        for key in ("sections", "leaves", "sound", "complete", "leaves_missing"):
            assert key in r, f"the reach slot carries {key}: {sorted(r)}"
        for key in ("pinned", "choices", "formats", "free"):
            assert key in s, f"the string slot carries {key}: {sorted(s)}"
        print(f"[A] reach {r['sections']} sections, {r['leaves']} leaves; {s['pinned']} strings pinned")

        # ── (B) the pill is a rendering of those numbers ────────────
        painted = run_of(tf, "lab.inspector.reach.text")
        assert_eq(
            painted,
            f"sections {r['sections']} · leaves {r['leaves']} · strings {s['pinned']}",
            "★★ the pill says what the wire says — one computation, two "
            "renderings, so a second count behind the paint fails here",
        )

        # ── (C) the surface is larger than the palette ──────────────
        hit_leaves, all_leaves = fraction(r["leaves"])
        hit_fields, all_fields = fraction(r["sections"])
        assert 0 < hit_leaves < all_leaves, (
            f"leaves {r['leaves']}: a surface the palette already covers whole "
            f"cannot show a regression, and one it does not reach at all is not "
            f"a tool"
        )
        assert 0 < hit_fields < all_fields, f"sections {r['sections']}"
        assert all_leaves >= 30, f"a five-leaf surface measures nothing: {all_leaves}"
        assert_eq(
            len(r["leaves_missing"]),
            all_leaves - hit_leaves,
            "and the ones it does not reach are NAMED, not just counted",
        )
        assert_eq(r["complete"], False, "so the palette is honestly partial")
        print(
            f"[C] {all_leaves} leaves across {all_fields} sections; the palette "
            f"reaches {hit_leaves}, and names the {len(r['leaves_missing'])} it does not"
        )

        # ── (D) the figure falls when the palette narrows... ────────
        #      ...and does NOT when a row merely leaves the screen.
        # ★ R1850 — on the card that HAS such a row, found by asking rather
        # than by naming one. Only a row that is written (not worked out from
        # the wires) is removed rather than disowned, and only one the screen
        # can offer back makes the round trip; the pair is not on every card.
        node, restorable_row = find_restorable(tf)
        tf.invoke(f"{EXT}/select", node)
        before = reach(tf)["leaves"]
        # ★★★★★ R1850 — the row is ASKED FOR, not named. This block used to
        # drive `transport.link.tx.batch_size`, and R1842 rewrote the option
        # surface from the target's own declaration: that key stopped being one
        # the catalogue re-offers, so the chip below was genuinely absent and
        # this gate went red reporting a defect that was its own copy going
        # stale. A gate that must name a key carries a duplicate of something
        # the form owns. `catalogue.restorable` is the screen's answer to "which
        # of the rows in front of you come back if you take them off", which is
        # exactly the precondition this claim needs.
        row = restorable_row
        tf.invoke(f"{EXT}/remove_field", row)
        keys = {r["key"] for r in json.loads(q(tf, "form"))}
        assert row not in keys, f"the row really left the screen: {sorted(keys)}"
        assert row in set(json.loads(q(tf, "catalogue"))["offered"]), (
            "★ and having left, it is now OFFERED — which is the half that "
            "makes the reach below stay put"
        )
        assert_eq(
            reach(tf)["leaves"],
            before,
            "★★★ and the reach did not move: the chip offers the key back, so "
            "the TOOL can still author it — a meter that fell here would be "
            "reporting the session rather than the tool",
        )
        # ⚠ R1850 — through the VERB, where this used to press the chip. The
        # claim this section makes is about the METER, and the chip's own
        # reachability is `r1686`'s subject. Saying so matters because the
        # reason it changed is a finding: since R1842 grew the option surface
        # from 53 paths to 111 the panel is long enough that the chip is a
        # scroll away, so a press here would have been asserting the scroll
        # position rather than the figure.
        tf.invoke(f"{EXT}/add_field", row)
        assert row in {r["key"] for r in json.loads(q(tf, "form"))}, (
            "the row is back on the form"
        )
        assert_eq(reach(tf)["leaves"], before, "putting it back changes nothing either")
        # ⚠ R1850 — and the selection goes BACK. This section had to move to a
        # card that has a restorable written row, and section (F) below drives
        # `id` and compares against a verdict captured while the ORIGINAL card
        # was selected. Leaving the selection here made that comparison read
        # two different cards and report six warnings against four — the exact
        # mistake R1842 made in `r1684`, repeated here by the round repairing
        # it. A section that moves the selection puts it back.
        tf.invoke(f"{EXT}/select", spec["selected_node"])

        # ── (E) the string surface is partitioned, and uses all of it ─
        classes = [s["choices"], s["formats"], s["free"]]
        pinned, total = fraction(s["pinned"])
        assert_eq(
            sum(len(c) for c in classes),
            total,
            "★ the three classes are the whole of the string surface",
        )
        assert_eq(len(s["choices"]) + len(s["formats"]), pinned, "and pinned is two of them")
        for name, members in zip(("choices", "formats", "free"), classes):
            assert members, (
                f"★ {name} is empty — a partition where everything lands in one "
                f"class measures nothing"
            )
        overlap = set(s["choices"]) & set(s["formats"]) | set(s["formats"]) & set(s["free"])
        assert_eq(overlap, set(), "and no leaf is in two classes")
        assert "listen.endpoints" in s["formats"], (
            f"★ a list of formatted strings IS a formatted string surface — a "
            f"meter reading only scalars would miss this screen's two most "
            f"important strings: {s['formats']}"
        )
        print(
            f"[E] {total} string leaf/leaves: {len(s['choices'])} word sets, "
            f"{len(s['formats'])} with a shape, {len(s['free'])} free"
        )

        # ── (F) the shape is enforced, and the refusal says what for ─
        assert_eq(reach(tf)["sound"], True, "the palette offers every key at its declared shape")
        opening_verdict = q(tf, "verdict")
        tf.invoke(f"{EXT}/set_field", "id=zz")
        blocked = q(tf, "verdict")
        assert blocked != opening_verdict, f"a value outside the alphabet moved the gate: {blocked}"
        gate = q(tf, "gate")
        assert "hexadecimal" in gate, (
            f"★★ and the refusal says what was WANTED, derived from the "
            f"declaration rather than written beside it: {gate}"
        )
        assert "id" in gate, f"and which key it is about: {gate}"

        # ── (G) both channels refuse it the same way ────────────────
        tf.invoke(f"{EXT}/set_field", "id=a1")
        assert_eq(q(tf, "verdict"), opening_verdict, "a good value opens it again")
        by_wire = q(tf, "gate")
        tf.invoke(f"{EXT}/set_field", "id=zz")
        from_wire_gate = q(tf, "gate")
        tf.invoke(f"{EXT}/set_field", "id=a1")
        assert_eq(q(tf, "gate"), by_wire, "and closes and opens repeatably")
        assert "hexadecimal" in from_wire_gate, from_wire_gate

        # ── (H) the tri-state ───────────────────────────────────────
        #      A half-typed address is not acceptable and is not refused. That
        #      is the state a two-answer validator cannot hold, and without it a
        #      field must reject the third keystroke of a sixteen-character
        #      value.
        tf.invoke(f"{EXT}/set_field", "listen.endpoints=tcp/0.0.0.0:7447")
        good = q(tf, "verdict")
        tf.invoke(f"{EXT}/set_field", "listen.endpoints=sctp/0.0.0.0:7447")
        assert q(tf, "verdict") != good, "a transport this screen has no pin for is refused"
        assert "tcp" in q(tf, "gate"), f"and the refusal lists the ones it has: {q(tf, 'gate')}"
        tf.invoke(f"{EXT}/set_field", "listen.endpoints=tcp/0.0.0.0:7447")
        assert_eq(q(tf, "verdict"), good, "and the good value comes back")

        # ── (I) the meter is the tool's, not the selection's ────────
        #      Driven across every card, because the roles have different
        #      opening rows and a meter derived from the selected form would
        #      move between them. The no-card case is the same claim at its
        #      limit and is held in process, where a selection can be cleared —
        #      this screen has no verb for that and inventing one to make a demo
        #      reachable would be the wrong direction.
        readings = set()
        for name in q(tf, "nodes").split(","):
            tf.invoke(f"{EXT}/select", name)
            readings.add(run_of(tf, "lab.inspector.reach.text"))
        assert_eq(
            len(readings),
            1,
            f"★ the pill reads the same on every card — it is a fact about the "
            f"palette, and the roles' forms differ: {sorted(readings)}",
        )
        tf.invoke(f"{EXT}/select", spec["selected_node"])

        # ── (J) the opening graph satisfies its own shapes ──────────
        #      The regression gate for the three identifiers that did not.
        bad = []
        for name in q(tf, "nodes").split(","):
            tf.invoke(f"{EXT}/select", name)
            row = next(
                (f for f in json.loads(q(tf, "form")) if f["key"] == "id"),
                None,
            )
            assert row is not None, f"{name} has an identifier row"
            if not all(c in "0123456789abcdef" for c in row["value"]):
                bad.append((name, row["value"]))
        assert_eq(
            bad,
            [],
            "★★★★ every card the screen opens with holds an identifier the "
            "target would accept — three of them did not until the shape came "
            "from the option surface, and nothing could say so while the row "
            "was free text",
        )
        print(f"[J] all {len(q(tf, 'nodes').split(','))} opening identifiers satisfy their declared shape")

        # ── (K) the gate panel is bounded ───────────────────────────
        #      Enough bad values and the panel used to be placed above the top
        #      of the canvas. It is bounded by the canvas now and counts what it
        #      cannot show, because the verdict is derived from ALL of them.
        canvas = rects(tf)["lab.canvas"] if "lab.canvas" in rects(tf) else None
        for name in q(tf, "nodes").split(","):
            tf.invoke(f"{EXT}/select", name)
            tf.invoke(f"{EXT}/set_field", "id=zz")
        painted_now = rects(tf)
        gate_box = painted_now["lab.gate"]
        assert gate_box[1] >= 0 and gate_box[3] > 0, f"the panel has a place: {gate_box}"
        if canvas is not None:
            assert gate_box[1] >= canvas[1], (
                f"★★ the panel stays inside the canvas with every card wrong: "
                f"gate {gate_box} vs canvas {canvas}"
            )
        lines = [t for t in painted_now if t.startswith("lab.gate.line.")]
        assert lines, "and it is showing problems"
        if "lab.gate.more" in painted_now:
            said = run_of(tf, "lab.gate.more")
            assert "more" in said, said
            print(f"[K] the gate panel shows {len(lines)} line(s) and says: {said!r}")
        else:
            print(f"[K] the gate panel shows all {len(lines)} line(s) inside the canvas")


if __name__ == "__main__":
    run_demo("R1690 §5.21 — the palette says what it can reach", body)
