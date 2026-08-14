#!/usr/bin/env python3
"""R1688 §5.20 §5.21 — the canvas is pointed at the whole graph, and at the
first thing wrong with it.

Drives `hello-node-lab` over JSON-RPC, in a real window, with a real pointer.

The screen's operation table has carried `fit the graph to the view` and `go to
the first problem` as **absent on both channels** since R1677 — no verb, no
gesture — and they are the last two. With this round the absence count reaches
zero for the first time.

Neither is difficult, and that is the argument for the column existing: a census
of what is ON the screen is blind by construction to what the screen cannot DO,
so an operation that is missing paints nothing and no forward check can see the
hole. Eleven rounds of green sweeps sat over these two.

What is not a screen detail is where the arithmetic went. There are two node
canvases in this tree and both had written the same affine, the same union fold
and the same clamp by hand; `hello-node-editor` frames its graph with a copy of
exactly this and `hello-node-lab` was about to write a fourth coordinate
conversion of its own. So it is `pinion-node-graph::view` now — `Camera` (one
projection with its own inverse), `ZoomRange` (validated where it is built),
`Margin` (canvas units or screen pixels, said out loud, because the reference
uses the first and the DCC idiom uses the second and they are different scales
for one graph) and `Fit`, whose answer says **whether it fitted**. Both canvases
call it; the editor's own viewport demos are part of this round's local gate.

  (A) the table declares both, on both channels, and NOTHING is absent.
  (B) the zoom pill is the reference's: minus, the read-out, plus, fit — one
      row, in that order, no overlaps, and the read-out is the control that
      puts the view back.
  (C) both seats answer a press at their CORNERS, not just at the middle.
  (D) fit — the zoom moves, and every card and every frame is inside the canvas
      afterwards, judged from the painted scene.
  (E) fit is idempotent, and does not depend on where you were looking.
  (F) the jump — the selection lands on the card the FIRST gate line names, and
      the toast is that line.
  (G) it reveals a card that is off screen, and moves nothing when it is not.
  (H) with the gate clear it says so instead of pretending.
  (I) a zoom step keeps the middle of the canvas still.
  (J) the seats are named buttons, and the read-out's name contains its reading.
  (K) the wire and the seats are the same act.

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
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"

# ★ Read from the screen rather than written here. The toolbar is what sets this
# screen's minimum window width, and this round moved it — a demo carrying its
# own copy of that number is the second-copy failure the whole specification
# table exists to prevent, one level up.
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


def press(tf, tag):
    box = rects(tf)[tag]
    tf.click(at=(box[0] + box[2] // 2, box[1] + box[3] // 2))


def resolves(tf, at) -> str:
    """What a press at that pixel would be, asked of the screen rather than
    computed here."""
    return tf.invoke(f"{EXT}/point", f"{at[0]},{at[1]}")


def camera(tf) -> tuple[int, tuple[int, int]]:
    """Where the canvas is pointed, as the wire states it."""
    x, y = (int(v) for v in q(tf, "pan").split(","))
    return int(q(tf, "zoom")), (x, y)


def access(tf):
    resp = tf.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return {node["tag"]: node for node in resp.result["nodes"] if node.get("tag")}


def graph_boxes(tf) -> dict[str, tuple[int, int, int, int]]:
    """Every card and every host frame, as PAINTED."""
    painted = rects(tf)
    spec = json.loads(q(tf, "spec"))
    out = {}
    for name in q(tf, "nodes").split(","):
        tag = f"lab.node.{name}"
        if tag in painted:
            out[name] = painted[tag]
    for frame in spec["frames"]:
        tag = f"lab.frame.{frame['name']}"
        if tag in painted:
            out[frame["name"]] = painted[tag]
    return out


def empty_spot(tf, canvas) -> tuple[int, int]:
    """A pixel of bare canvas, ASKED of the screen rather than assumed.

    A drag is a pan only when it starts on nothing: begun on a card it places
    the card, and begun on a frame's tab it moves the frame and its members. A
    demo that picked a corner and hoped would be driving a different gesture on
    the day the graph moved.
    """
    for dx, dy in ((8, 8), (8, 40), (40, 8), (8, 80), (80, 8), (200, 8)):
        at = (canvas[0] + dx, canvas[1] + dy)
        if resolves(tf, at) == "canvas":
            return at
    raise AssertionError("no bare canvas to start a pan on")


def inside(a, b) -> bool:
    return (
        a[0] >= b[0]
        and a[1] >= b[1]
        and a[0] + a[2] <= b[0] + b[2]
        and a[1] + a[3] <= b[1] + b[3]
    )


def overlaps(a, b) -> bool:
    return (
        a[0] < b[0] + b[2]
        and b[0] < a[0] + a[2]
        and a[1] < b[1] + b[3]
        and b[1] < a[1] + a[3]
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) the declaration ─────────────────────────────────────
        spec = json.loads(q(tf, "spec"))
        rows = {row["name"]: row for row in spec["operations"]}
        for name, verb in (
            ("fit the graph to the view", "fit"),
            ("go to the first problem", "go_to_problem"),
        ):
            op = rows[name]
            assert_eq(op["gesture"], True, f"★ '{name}' has a way in for a person")
            assert_eq(op["verb"][0], verb, f"★ and a verb an agent can call: {op}")
            assert_eq(
                op["absent"],
                False,
                f"so '{name}' is answered on both channels — it was absent on "
                "both since R1677",
            )
        absent = [row["name"] for row in spec["operations"] if row["absent"]]
        assert_eq(
            absent,
            [],
            "★★★ and NOTHING in the table is absent any more — the first time "
            "this count has been zero",
        )
        assert len(spec["operations"]) >= 30, spec["operations"]

        # ── (B) the pill is the reference's ─────────────────────────
        painted = rects(tf)
        pill = [
            painted[tag]
            for tag in (
                "lab.toolbar.zoom.out",
                "lab.reset.view",
                "lab.toolbar.zoom.in",
                "lab.toolbar.fit",
            )
        ]
        for tag in ("lab.toolbar.fit", "lab.reset.view", "lab.toolbar.gate"):
            assert tag in painted, f"{tag} is painted"
        for n in range(len(pill) - 1):
            assert pill[n][0] + pill[n][2] <= pill[n + 1][0], (
                "★ minus, the read-out, plus, fit — in that order and not "
                f"overlapping: {pill}"
            )
            assert_eq(
                pill[n][1] + pill[n][3] // 2,
                pill[n + 1][1] + pill[n + 1][3] // 2,
                "and on one row",
            )
        assert inside(painted["lab.toolbar.zoom"], painted["lab.reset.view"]), (
            "★★ the read-out is the view reset's own caption — the reference "
            "makes the percentage the button that puts the view back, and this "
            f"screen had a number that could not be pressed: {painted['lab.toolbar.zoom']} "
            f"in {painted['lab.reset.view']}"
        )
        assert not overlaps(painted["lab.toolbar.gate"], painted["lab.toolbar.zoom.out"]), (
            "the launch chip is in the other cluster"
        )

        # ── (C) pressable at the CORNERS ────────────────────────────
        # A probe aimed at a centre cannot see an error smaller than half a
        # control, which is how a 1 px transform defect survived two rounds
        # (R1684). The corners are where it shows.
        for tag, want in (
            ("lab.toolbar.fit", "fit"),
            ("lab.toolbar.gate", "problem"),
            ("lab.reset.view", "reset:view"),
        ):
            box = painted[tag]
            for dx, dy in (
                (0, 0),
                (box[2] - 1, 0),
                (0, box[3] - 1),
                (box[2] - 1, box[3] - 1),
            ):
                at = (box[0] + dx, box[1] + dy)
                assert_eq(
                    resolves(tf, at),
                    want,
                    f"★ {tag} answers at its corner {at}",
                )

        # ── (D) fit ─────────────────────────────────────────────────
        canvas = painted["lab.canvas"]
        opened_at = camera(tf)
        assert_eq(opened_at[0], spec["zoom"], "the screen opens at the declared zoom")
        press(tf, "lab.toolbar.fit")
        framed = camera(tf)
        assert framed != opened_at, f"the view moved: {opened_at} -> {framed}"
        said = q(tf, "toast")
        assert said.startswith("the whole graph"), said
        boxes = graph_boxes(tf)
        assert len(boxes) >= len(spec["nodes"]) + len(spec["frames"]), boxes
        for name, box in boxes.items():
            assert inside(box, canvas), (
                f"★★ {name} is painted {box} and the canvas is {canvas} — a fit "
                "that leaves part of the graph off screen is the one thing this "
                "operation must not do"
            )
        # And CENTRED, which containment alone would not catch: a fit that
        # pinned the graph to a corner satisfies every check above.
        left = min(box[0] for box in boxes.values())
        right = max(box[0] + box[2] for box in boxes.values())
        assert abs((left - canvas[0]) - (canvas[0] + canvas[2] - right)) <= 2, (
            f"the gutters are {left - canvas[0]} and "
            f"{canvas[0] + canvas[2] - right}"
        )

        # ── (E) idempotent, and not a function of the view ──────────
        press(tf, "lab.toolbar.fit")
        assert_eq(
            camera(tf),
            framed,
            "★★ framing a framed graph answers the same camera — the reference's "
            "own advice is to call its fit twice, which is this defect seen from "
            "the other side",
        )
        tf.invoke(f"{EXT}/zoom_by", "25")
        tf.drag(
            from_at=(canvas[0] + canvas[2] // 2, canvas[1] + canvas[3] // 2),
            to_at=(canvas[0] + canvas[2] // 2 - 180, canvas[1] + canvas[3] // 2 + 90),
        )
        assert camera(tf) != framed, "the view really did move away"
        press(tf, "lab.toolbar.fit")
        assert_eq(
            camera(tf),
            framed,
            "★★★ and it is a function of the GRAPH, not of where you were "
            "looking when you asked",
        )

        # ── (F) the jump ────────────────────────────────────────────
        gate = json.loads(q(tf, "gate"))
        assert gate, "the opening graph has findings for the chip to lead to"
        first = gate[0]["sentence"]
        who = first.split(" ")[0]
        assert q(tf, "selected") != who, (
            f"★ the first finding is not on the card the screen is on: "
            f"{first!r} against {q(tf, 'selected')}"
        )
        press(tf, "lab.toolbar.gate")
        assert_eq(q(tf, "selected"), who, "★★ the jump lands on that card")
        assert_eq(q(tf, "toast"), first, "and says which finding it took you to")

        # ── (G) it reveals, and only when it has to ─────────────────
        before = camera(tf)
        press(tf, "lab.toolbar.gate")
        assert_eq(
            camera(tf),
            before,
            "★★ everything is on screen, so a second press moves nothing — a "
            "reveal that re-centred would throw away a view somebody chose",
        )
        # Now drag the graph off to the right and ask again.
        spot = empty_spot(tf, canvas)
        tf.drag(from_at=spot, to_at=(canvas[0] + canvas[2] - 4, spot[1]))
        assert camera(tf) != before, "the pan moved"
        # ★ Off screen shows up as the card not being PAINTED at all: the canvas
        # clips, so a card panned out of it leaves no mark and therefore no tag.
        # That is a stronger statement than a rectangle outside the pane, and it
        # is the framework's own answer rather than this file's arithmetic.
        gone = graph_boxes(tf)
        assert who not in gone or not inside(gone[who], canvas), (
            f"★ and {who} really is off screen now, or the reveal below would "
            f"be proving nothing: {gone.get(who)} against {canvas}"
        )
        press(tf, "lab.toolbar.gate")
        moved = graph_boxes(tf)[who]
        assert inside(moved, canvas), (
            f"★★★ the card the person was sent to is on screen: {moved} in "
            f"{canvas}. The reference only moves the selection, so a graph "
            "panned away leaves them told about something they cannot see"
        )

        # ── (H) with the gate clear, it says so ─────────────────────
        # The opening graph's findings are cards with nowhere to listen and
        # peers with discovery on. Repairing them is what a person would do, and
        # the chip has to stop offering a journey there is none of.
        #
        # ★ Driven off the gate's OWN list rather than a list written here: the
        # findings are derived from the forms and a demo that named them would
        # be asserting a graph rather than a behaviour. It also proves the jump's
        # own precondition moves — each repair takes the FIRST line away.
        press(tf, "lab.toolbar.fit")
        repaired = 0
        for port in range(7470, 7490):
            lines = json.loads(q(tf, "gate"))
            if not lines:
                break
            sentence = lines[0]["sentence"]
            tf.invoke(f"{EXT}/select", sentence.split(" ")[0])
            if "discovery" in sentence:
                tf.invoke(f"{EXT}/set_field", "discovery.multicast=false")
            else:
                tf.invoke(f"{EXT}/set_field", f"listen.endpoints=tcp/0.0.0.0:{port}")
            repaired += 1
        assert repaired >= 2, f"{repaired} finding(s) were repaired"
        assert_eq(
            json.loads(q(tf, "gate")),
            [],
            "★ every finding is repaired through the wire",
        )
        settled = camera(tf)
        said = tf.invoke(f"{EXT}/go_to_problem", "")
        assert "nothing to go to" in said, said
        assert_eq(camera(tf), settled, "and nothing moved")
        assert_eq(
            access(tf)["lab.toolbar.gate"]["name"],
            "gate passed, nothing to go to",
            "★★ and the chip SAYS there is nothing — a reader told only 'go to "
            "the first problem' has not been told whether there is one",
        )
        tf.invoke(f"{EXT}/reset", "fields")

        # ── (I) a zoom step keeps the middle still ──────────────────
        # The reference zooms about the viewport centre; this screen changed the
        # scale and left the pan, which anchors at the canvas ORIGIN — so zooming
        # out from a graph you had panned to walked it off the corner.
        tf.drag(
            from_at=(canvas[0] + canvas[2] // 2, canvas[1] + canvas[3] // 2),
            to_at=(canvas[0] + canvas[2] // 2 - 140, canvas[1] + canvas[3] // 2 + 70),
        )
        mid = (canvas[2] / 2, canvas[3] / 2)

        def under_the_middle() -> tuple[float, float]:
            zoom, pan = camera(tf)
            return ((mid[0] - pan[0]) / (zoom / 100), (mid[1] - pan[1]) / (zoom / 100))

        held = under_the_middle()
        for tag in ("lab.toolbar.zoom.in", "lab.toolbar.zoom.in", "lab.toolbar.zoom.out"):
            press(tf, tag)
            now = under_the_middle()
            assert abs(now[0] - held[0]) <= 3 and abs(now[1] - held[1]) <= 3, (
                f"★★ the canvas point under the middle moved from {held} to "
                f"{now} at {q(tf, 'zoom')}%"
            )

        # ── (J) named buttons ───────────────────────────────────────
        nodes = access(tf)
        for tag, name in (
            ("lab.toolbar.fit", "fit the graph to the view"),
            ("lab.toolbar.zoom.in", "zoom in"),
        ):
            assert_eq(nodes[tag]["role"], "button", f"{tag} announces as a button")
            assert_eq(nodes[tag]["name"], name, f"{tag} says what it does")
        reading = f"{q(tf, 'zoom')}%"
        assert reading in nodes["lab.reset.view"]["name"], (
            "★★ the read-out's accessible name CONTAINS its visible one: a "
            f"button labelled {reading} whose name was only 'reset the view' is "
            f"the label-in-name failure — {nodes['lab.reset.view']['name']!r}"
        )
        assert "reset" in nodes["lab.reset.view"]["name"], (
            "and still says what pressing it does"
        )
        assert nodes["lab.toolbar.gate"]["name"].startswith("gate"), nodes[
            "lab.toolbar.gate"
        ]["name"]

        # ── (K) the wire and the seats are the same act ─────────────
        press(tf, "lab.toolbar.fit")
        by_seat = (camera(tf), q(tf, "toast"))
        tf.invoke(f"{EXT}/zoom_by", "300")
        by_wire_said = tf.invoke(f"{EXT}/fit", "")
        assert_eq(
            (camera(tf), q(tf, "toast")),
            by_seat,
            "★ the verb and the seat point the canvas the same way",
        )
        assert_eq(by_wire_said, by_seat[1], "and answer the same sentence")

        tf.invoke(f"{EXT}/select", "R-01")
        by_wire = tf.invoke(f"{EXT}/go_to_problem", "")
        landed = q(tf, "selected")
        tf.invoke(f"{EXT}/select", "R-01")
        press(tf, "lab.toolbar.gate")
        assert_eq(q(tf, "selected"), landed, "and the jump likewise")
        assert_eq(q(tf, "toast"), by_wire, "with the same sentence")


if __name__ == "__main__":
    run_demo("R1688 §5.21 — where the canvas is pointed", body)
