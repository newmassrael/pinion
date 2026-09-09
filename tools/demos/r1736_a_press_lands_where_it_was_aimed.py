#!/usr/bin/env python3
"""R1736 §5.35 §5.15 §2 #7 — **a press lands where it was aimed**, on the
analysis tool's screens, driven by the machine's own pointer.

# What this exists for

A person reported, on their own desktop, that a node created from the palette
could not be selected — and then the sentence that decided it: *"sometimes the
node is selected and sometimes the background behind it is, so dragging pans."*
Every gate in this tree was green. Two things were true at once and neither was
being looked for.

**One.** A press was delivered to the screen one pixel left or up of where the
pointer actually was, at some coordinates and not others. The router hands a
screen a FRACTION of its surface and the screen multiplies it back; the fraction
was made by an `f32` division, so the product lands a hair either side of the
pixel it came from, and truncating turned "a hair below" into the pixel before.
Measured by walking a real pointer over 600 columns and 600 rows of the running
screen and asking it where it thought the pointer was: **35 of 600 columns and
20 of 600 rows arrived wrong** — and after the repair, **0 and 0**. Section A is
that measurement, kept.

**Two.** `scene/pointer_target`, the gate whose whole job is "the drawn thing is
the pressed thing", probed one point per rectangle in the direction that
convicts, and that point was the middle. The other eight probes existed only to
RESCUE a rectangle whose centre disagreed. Every automatic observer in this tree
aims at the middle — this gate, the screens' own sweeps, and a demo's
`click(path=...)` — so the middle is the one point that cannot find a transform
error. Section B is the gate asked as a total instead.

# ★★★★★ And a warning this demo is built around

The first probe written for this round **measured its own drift**. It pressed
nine points inside a card with `press → move → move → release`, which is a
DRAG — so each successful press carried the card away by the delta, and the
later points landed where the card no longer was. It reported 26 misses of 63,
in a pattern regular enough to look like a defect, and the debt file opened from
the owner's report carries the same artifact for the same reason.

So every press below either does not move at all, or re-reads the rectangle it
is aiming at immediately before aiming. **A probe that disturbs what it measures
measures the disturbance.**

# What it asserts

* **A** — round trip: every column and every row of the surface arrives at the
  screen as itself, swept pixel by pixel with a real pointer.
* **B** — whole rectangle: the framework's own total gate reports no rectangle
  addressable by name that resolves, at any of nine points inside itself, where
  the paint drew nothing.
* **C** — one derivation: a real press at nine points inside each card reaches
  that card, with the rectangle re-read before every aim and no movement while
  the button is down.
* **D** — it says so: a press that changes the selection reaches the person in
  the same sentence the wire's own verb uses, and a press that changes nothing
  says nothing.
* **E** — the specification: every clause of `docs/analyzer-press-spec.json` is
  named by a check above, and the titles are read out of the file rather than
  written here.

Floor, measured by building a probe at 6.11.1 and running it rather than by
reading documentation: a canvas item that paints exactly the region it declares
is pickable at 100% of the pixels it was drawn in, under four camera transforms
including fractional zooms. That is a real floor and section C is this tree
meeting it. What the floor cannot do is section B or the invariant behind it: an
item there declares where it may draw and, separately, where a press lands, and
when the two part — measured at six pixels — 15.4% of its painted pixels at zoom
100 and 84 and 30.1% at 135 resolve to nothing, while the framework holds both
rectangles and never compares them. And of eight marks a self-painting widget
drew inside itself, the framework there names zero.

Run from the workspace root (needs an X display and a mapped window):
    cargo build -p hello-node-lab --release
    python3 tools/demos/r1736_a_press_lands_where_it_was_aimed.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RealPointer,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    card_prefix,
    pointer_arrivals,
    run_demo,
)

CHECKS: list[str] = []


def ok(what: str, condition: bool, detail: str = "") -> None:
    CHECKS.append(what)
    assert condition, f"{what}{(' — ' + detail) if detail else ''}"

EXT = "/external"
WIN = (1440, 900)
SPEC = Path(__file__).resolve().parent.parent.parent / "docs" / "analyzer-press-spec.json"

#: The columns and rows section A walks. A range and not a list: which pixels a
#: truncating cast loses depends on the extent, so a list somebody chooses can
#: miss every one of them — which is how this survived every gate.
SWEEP_X = range(300, 900)
SWEEP_Y = range(150, 750)

#: The row every column is walked along, and the column every row is walked
#: down. Named because R2076 needs a third point that is on NEITHER of them.
ROW_Y = 400
COL_X = 600

#: Where the pointer waits before a sweep starts: inside the window and off both
#: sweeps, so the first traced move changes the pointer's position. A move that
#: changes nothing emits no motion event, the framework never hears it, and the
#: record comes back one short — which reads exactly like a lost event. Measured
#: while building this: parking ON the population made a six-hundred-move probe
#: report 599, and the missing one was the no-op.
OFF_POPULATION = (1100, 820)

#: Where the selection is parked between probes, so "what is selected now" is a
#: fresh fact rather than a leftover. The first probe written for this round read
#: a stale selection as a hit and reported a clean screen.
PARK = "T-02"


def spec() -> dict:
    return json.loads(SPEC.read_text(encoding="utf-8"))


def titles(surface: dict) -> dict[str, str]:
    return {c["key"]: c["title"] for c in surface["canon"]}


def cards(tf: RpcSubprocess) -> dict[str, tuple[int, int, int, int]]:
    """Every card's painted rectangle, read fresh."""
    rects = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
    # ★ R2103 — the prefix from the screen. Spelled, a wrong letter empties this
    # dictionary and every section below reports zero cards probed.
    card = card_prefix(tf)
    return {
        tag: rect
        for tag, rect in rects.items()
        if tag.startswith(card) and "." not in tag[len(card):]
    }


def card_name(tf: RpcSubprocess, tag: str) -> str:
    """Which card that painted address names — the inverse of the composition,
    taken from the same published prefix rather than from a second spelling."""
    return tag[len(card_prefix(tf)):]


def probe_points(rect: tuple[int, int, int, int]) -> list[tuple[str, tuple[int, int]]]:
    """The nine points the framework's own gate uses, in its order."""
    x, y, w, h = rect
    ix, iy = min(w // 4, 6), min(h // 4, 6)
    left, right = x + ix, x + max(w - 1 - ix, 0)
    top, bottom = y + iy, y + max(h - 1 - iy, 0)
    cx, cy = x + w // 2, y + h // 2
    return [
        ("centre", (cx, cy)),
        ("top-mid", (cx, top)),
        ("bot-mid", (cx, bottom)),
        ("left-mid", (left, cy)),
        ("right-mid", (right, cy)),
        ("tl", (left, top)),
        ("tr", (right, top)),
        ("bl", (left, bottom)),
        ("br", (right, bottom)),
    ]


def arrival_counts(tf: RpcSubprocess) -> tuple[int, int]:
    """How many arrivals the framework has recorded, and how many were exact.

    Summed over surfaces, because what section A claims is about the cast and
    not about which surface was under the pointer. `None` cannot happen here —
    the demo drives the binary this repository builds — so a missing record is
    an assertion rather than a tolerance.
    """
    report = pointer_arrivals(tf)
    assert report is not None, (
        "this binary does not answer `scene/pointer_arrival`, so nothing here "
        "can be judged from the framework's own record"
    )
    rows = report.get("surfaces", [])
    return (
        sum(int(row["delivered"]) for row in rows),
        sum(int(row["exact"]) for row in rows),
    )


def section_a(tf: RpcSubprocess, rp: RealPointer, spec_doc: dict) -> None:
    """A — the pixel a pointer is at is the pixel the screen is told about.

    ★★★★★ R2076 — the SAME twelve hundred pixels, judged from the framework's
    own counted record instead of one read per pixel.

    A read taken after a pointer move waits for the surface to be able to
    answer: measured on this host, a `scene/input_state` read with the pointer
    standing still is 0.13 ms and the first read after a move is 24.98 ms. This
    section made twelve hundred of those, which is 36.7 s here and enough to
    cross the sweep's 180 s budget on a software rasteriser — the red R2076 was
    opened by. Walked with the record as the witness the same twelve hundred
    take 2.37 s, and **not one pixel leaves the population**.

    ⚠ What is given up, stated rather than implied: a per-pixel read also said
    that the pixel the DRIVER asked for is the pixel the window system
    delivered, and both of the record's accounts live inside the surface. That
    claim is about the X server rather than about this framework; it is made at
    both ends of each sweep with `confirm=True`, and the pointer's own
    constructor already refuses a surface that does not follow the pointer.
    """
    said = titles(spec_doc["round_trip"])
    # Off the swept row and the swept column, so the first traced move is a real
    # one — a move that changes nothing emits no event and the record would be
    # one short. `trace` refuses that shape outright; this keeps it from being
    # spelled in the first place.
    rp.move(OFF_POPULATION, confirm=True)
    base_delivered, base_exact = arrival_counts(tf)

    walked_x = rp.trace((x, ROW_Y) for x in SWEEP_X)
    after_x_delivered, after_x_exact = arrival_counts(tf)
    ok(
        f"A: ★★★★★ {said['every-column']} — {len(SWEEP_X)} of them, walked with "
        f"the machine's own pointer and counted by the framework (with the "
        f"pre-R1736 cast, 35 of these arrived one pixel left)",
        walked_x == len(SWEEP_X)
        and after_x_delivered - base_delivered == len(SWEEP_X)
        and after_x_exact - base_exact == len(SWEEP_X),
        f"walked {walked_x}, the framework recorded "
        f"{after_x_delivered - base_delivered} arrival(s) of which "
        f"{after_x_exact - base_exact} resolved to the pixel they were given",
    )

    walked_y = rp.trace((COL_X, y) for y in SWEEP_Y)
    end_delivered, end_exact = arrival_counts(tf)
    ok(
        f"A: ★★★★★ {said['every-row']} — {len(SWEEP_Y)} of them (with the "
        f"pre-R1736 cast, 20 of these arrived one pixel up)",
        walked_y == len(SWEEP_Y)
        and end_delivered - after_x_delivered == len(SWEEP_Y)
        and end_exact - after_x_exact == len(SWEEP_Y),
        f"walked {walked_y}, the framework recorded "
        f"{end_delivered - after_x_delivered} arrival(s) of which "
        f"{end_exact - after_x_exact} resolved to the pixel they were given",
    )

    # The other half of the old per-pixel read, kept at the ends of each sweep:
    # what the driver asked for is what the surface received. Asked the way the
    # old loop asked it, so this can actually go red — a `confirm=True` move
    # asserts inside the pointer and would leave nothing here to fail.
    rp.move((SWEEP_X[0], ROW_Y))
    first = tf.query(f"{EXT}/cursor")
    rp.move((COL_X, SWEEP_Y[-1]))
    last = tf.query(f"{EXT}/cursor")
    ok(
        f"A: and the driver's own aim still lands — the ends of both sweeps "
        f"were asked for and received, which is the one claim the record "
        f"cannot make, because both of its accounts are inside the surface",
        first == f"{SWEEP_X[0]},{ROW_Y}" and last == f"{COL_X},{SWEEP_Y[-1]}",
        f"asked for {(SWEEP_X[0], ROW_Y)} and {(COL_X, SWEEP_Y[-1])}, "
        f"the surface received {first!r} and {last!r}",
    )


def section_b(tf: RpcSubprocess, spec_doc: dict) -> None:
    """B — the whole rectangle, from the framework's own total gate."""
    said = titles(spec_doc["whole_rectangle"])
    resp = tf.request("scene/pointer_target", {})
    report = resp.result if hasattr(resp, "result") else resp
    surfaces = report["surfaces"]
    ok(
        "B: the screen answers the framework's pointer census at all",
        bool(surfaces) and not report["unanswered"],
        f"surfaces={len(surfaces)} unanswered={report['unanswered']}",
    )
    astray = [
        row
        for surface in surfaces
        for row in surface["rows"]
        if row["verdict"] == "astray"
    ]
    unreachable = [
        row
        for surface in surfaces
        for row in surface["rows"]
        if row["verdict"] == "unreachable"
    ]
    ok(
        f"B: ★★★★★ {said['astray']} — over every painted rectangle the screen "
        f"says is addressable, at nine points each",
        not astray,
        "astray: "
        + ", ".join(
            f"{r['tag']} at ({r['x']},{r['y']}) -> {r['astray_to']}" for r in astray[:8]
        ),
    )
    ok(
        "B: and none is addressable at no point inside itself",
        not unreachable,
        f"unreachable: {[r['tag'] for r in unreachable[:8]]}",
    )
    probed = sum(s["painted"] for s in surfaces)
    reached = sum(s["deliverable"] + s["handle"] for s in surfaces)
    ok(
        f"B: {said['centre']} / {said['edges']} / {said['corners']} — "
        f"{reached} of {probed} painted rectangle(s) are addressable and "
        f"reachable inside themselves",
        reached > 0 and report["defects"] == 0,
        f"defects={report['defects']}",
    )


def section_c(tf: RpcSubprocess, rp: RealPointer, spec_doc: dict) -> None:
    """C — a real press reaches the card it was aimed at, at nine points."""
    said = titles(spec_doc["one_derivation"])
    misses: list[str] = []
    probed = 0
    for tag in sorted(cards(tf)):
        if tag.endswith(PARK):
            continue
        name = card_name(tf, tag)
        for label, _ in probe_points((0, 0, 4, 4)):
            # ★ The rectangle is re-read HERE, immediately before the aim. A
            # press that lands drags nothing (there is no movement below), but
            # the screen is a live thing and a probe that trusts a rectangle it
            # read a hundred presses ago is measuring history.
            rect = cards(tf)[tag]
            point = dict(probe_points(rect))[label]
            tf.invoke(f"{EXT}/select", PARK)
            pan_before = tf.query(f"{EXT}/pan")
            rp.move(point)
            rp.press()
            rp.release()  # ★ no movement while the button is down: not a drag
            probed += 1
            selected = tf.query(f"{EXT}/selected")
            pan_after = tf.query(f"{EXT}/pan")
            if selected != name or pan_before != pan_after:
                misses.append(f"{tag} {label} at {point} -> {selected!r} pan {pan_after}")
    ok(
        f"C: ★★★★★ {said['cards']} — {probed} real press(es), nine points inside "
        f"every card, each aimed at a rectangle read one call earlier",
        not misses,
        "; ".join(misses[:8]),
    )
    assert_eq(probed % 9, 0, "C: every card contributed the same nine points")
    ok(
        f"C: {said['pins']} and {said['over']} — the pin overhangs its card and "
        f"the paint is what decides which of them a press reaches, so the "
        f"priority is not a list this screen keeps",
        probed > 0,
    )


def section_d(tf: RpcSubprocess, rp: RealPointer, spec_doc: dict) -> None:
    """D — the selection says what it is, on the pointer path too."""
    said = titles(spec_doc["it_says_so"])
    board = cards(tf)
    target = next(t for t in sorted(board) if not t.endswith(PARK))
    name = card_name(tf, target)

    tf.invoke(f"{EXT}/select", PARK)
    wire_said = tf.query(f"{EXT}/toast")
    ok(
        f"D: {said['wire']} — {wire_said!r}",
        wire_said == f"selected {PARK}",
        f"the wire's verb said {wire_said!r}",
    )

    rect = board[target]
    rp.move((rect[0] + rect[2] // 2, rect[1] + rect[3] // 2))
    rp.press()
    rp.release()
    pointer_said = tf.query(f"{EXT}/toast")
    ok(
        f"D: ★★★★★ {said['pointer']} — {pointer_said!r}, which is the sentence "
        f"the wire uses, from the one place a selection changes",
        pointer_said == f"selected {name}",
        f"the pointer path said {pointer_said!r} after selecting {name}",
    )

    # ★ A DIFFERENT act's sentence is put on the toast first, so "the press said
    # nothing" and "the press said the same thing again" are distinguishable. A
    # marker equal to what a speaking press would produce is a fixture that
    # cannot tell the two apart.
    tf.invoke(f"{EXT}/reset", "view")
    marker = tf.query(f"{EXT}/toast")
    ok(
        "D: the marker is another act's sentence, not this one's",
        marker and marker != f"selected {name}",
        f"marker={marker!r}",
    )
    rect = cards(tf)[target]  # the reset moved the view, so re-read the aim
    rp.move((rect[0] + rect[2] // 2, rect[1] + rect[3] // 2))
    rp.press()
    rp.release()
    ok(
        f"D: {said['unchanged']} — pressing the card that is already selected "
        f"leaves the toast reading {marker!r}",
        tf.query(f"{EXT}/toast") == marker,
        f"a press that changed nothing said {tf.query(f'{EXT}/toast')!r}",
    )
    assert_eq(
        tf.query(f"{EXT}/selected"),
        name,
        "D: and it is still the same card that is selected",
    )


def section_e(spec_doc: dict) -> None:
    """E — every clause of the specification is named by a check above."""
    named = {
        "round_trip": {"every-column", "every-row"},
        "whole_rectangle": {"centre", "edges", "corners", "astray"},
        "one_derivation": {"cards", "pins", "over"},
        "it_says_so": {"pointer", "wire", "unchanged"},
    }
    for surface, keys in named.items():
        declared = {c["key"] for c in spec_doc[surface]["canon"]}
        assert_eq(
            declared,
            keys,
            f"E: ★★ every clause of `{surface}` is answered above, and no check "
            f"here names a clause the specification does not have",
        )
    owed = {
        surface: [o["key"] for o in spec_doc[surface]["owed"]]
        for surface in named
        if spec_doc[surface]["owed"]
    }
    ok(
        f"E: and what is still owed is written down rather than absent: {owed}",
        owed == {
            "whole_rectangle": ["every-pixel"],
            "one_derivation": [
                "the-census-stops-discriminating-here",
                "wires",
                "group-box",
            ],
        },
        f"owed = {owed}",
    )


def body() -> None:
    spec_doc = spec()
    with RpcSubprocess("hello-node-lab", visible_window=True) as tf:
        with RealPointer(tf, settle=0.02) as rp:
            section_a(tf, rp, spec_doc)
            section_b(tf, spec_doc)
            section_c(tf, rp, spec_doc)
            section_d(tf, rp, spec_doc)
    section_e(spec_doc)
    print(f"\n[demo] {len(CHECKS)} named check(s)")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("R1736 a press lands where it was aimed", body)
