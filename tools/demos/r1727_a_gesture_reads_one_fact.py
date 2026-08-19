#!/usr/bin/env python3
"""R1727 §5.35 §5.15 §2 #2 §2 #7 — **a gesture reads one fact, and the harness
can make a real one.**

# What this demo exists for, and who found it

R1726 shipped a dashboard whose drag was green through the wire and wrong under
a hand. The owner found three defects by dragging cards with a mouse — a drop
preview that covered the widget beneath it, a drop that landed in the wrong cell
after a scroll, and a missing cursor label — and then asked the question this
round is the answer to: *why are you asking me instead of driving it yourself?*

Measured on the analysis tool's dashboard, one press from the same pixel to the
same pixel, eight interpolated moves, three ways of delivering them:

    a real X11 pointer                 loss@0,4   latency@0,5   topology@0,6
    eight `scene/drag` calls           loss@0,4   latency@0,5   topology@0,6
    ONE `scene/drag` with steps=8      loss@0,10  latency@0,7   topology@0,11

The third is the harness's own multi-step march, and it is the one every drag
assertion in this tree was made through. It differs because it delivers the
moves with no frame between them, and the board's `pointer_move` was reading a
FRACTION taken over the last painted rect and multiplying it by a row count read
from the LIVE model — which the drag had already grown. Two facts, true at two
different times. A hand supplies the frame, so the wrong reading hid behind the
right answer.

`pinion_core::PointerReading` carries the rectangle the fraction was taken over,
so `px()` is `cursor − rect.origin` whatever has happened to the model since.
This demo asserts the three deliveries now agree, and drives the last one with
the machine's own pointer.

# Floor, measured by building a probe against 6.11.1 and running it

Its move event carries widget-local **pixels**, so this failure mode does not
arise there — on that axis the floor was above us, and `px()` is the interface
it has. What it does not have is the other half. Across the six classes a
scrolling, tabular, split or sliding surface is built from there — 441 declared
properties and 274 declared methods — **not one** names the extent a paint used,
and none names whether an event was synthesised or came from the window system.
In the same probe run its live height had already reached 512 while its last
paint had used 320, and nothing reports the 320. A test there also cannot read
the scene as data mid-gesture at all; here that is one `scene/snapshot` with the
button still down, which is section C.

Run from the workspace root (a real pointer needs a display):
    cargo build --release -p hello-tile-dashboard -p hello-node-lab \\
        -p hello-data-grid -p hello-node-editor
    DISPLAY=:97 python3 tools/demos/r1727_a_gesture_reads_one_fact.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RealPointer,
    RealPointerUnavailable,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    assert_gesture_reads_one_fact,
    run_demo,
)

CHECKS: list[str] = []

#: How many real-pointer sessions actually opened. Zero means this host could
#: not drive one, and the coverage line at the end says so rather than letting a
#: shorter run read as a pass.
REAL_POINTER_RUNS = 0

#: The card and the empty board cell the measurement above used, in the
#: dashboard's logical pixels: the centre of `loss`, and two rows down-left.
FROM = (564.0, 137.0)
TO = (150.0, 340.0)
STEPS = 8

#: What all three deliveries must agree on.
SETTLED = (
    "throughput@0,0+12x1 latency@0,5+6x1 loss@0,4+6x1 "
    "topology@0,6+4x2 alarms@4,6+8x1"
)


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def tiles(app: RpcSubprocess) -> str:
    return app.snapshot()["introspect"]["tiles"]


def cards(app: RpcSubprocess) -> dict:
    rects = abs_rects_of(app.snapshot(source="paint", viewport=(760, 420)))
    return {
        t: r
        for t, r in rects.items()
        if t.startswith("dashboard#card") and not t.endswith(".label")
    }


def real_pointer(app: RpcSubprocess) -> "RealPointer | None":
    """A driver, or `None` with a LOUD line saying the host has no pointer.

    ★ The absence is announced, never inferred into a pass. Missing `xdotool`
    on a runner is an infrastructure fact, the same class the push gate's CI
    read fails open on — but a check that silently stopped happening is exactly
    the failure this round exists to end, so it always prints.
    """
    global REAL_POINTER_RUNS
    try:
        pointer = RealPointer(app)
    except RealPointerUnavailable as exc:
        print(f"[real-pointer] UNAVAILABLE — this section is not driven: {exc}")
        return None
    REAL_POINTER_RUNS += 1
    return pointer


def body() -> None:  # noqa: PLR0915 - one narrative, read top to bottom
    # ── (A) the wire agrees with itself ──────────────────────────────────
    banner("A — the same gesture, delivered two ways, answers once")
    settled = assert_gesture_reads_one_fact(
        "hello-tile-dashboard",
        from_at=FROM,
        to_at=TO,
        steps=STEPS,
        read=tiles,
        label="dashboard card drag",
    )
    ok("A: batched and per-move deliveries leave the same board", True)
    assert_eq(
        settled,
        SETTLED,
        "A: and it is the arrangement a real pointer produced. Before R1727 the "
        "batched delivery put the dragged card on row 10 and every displaced "
        "card three rows past where a hand leaves it",
    )

    # ── (B) and so does the machine's own pointer ────────────────────────
    banner("B — a real X11 pointer lands the same board")
    with RpcSubprocess("hello-tile-dashboard", visible_window=True) as app:
        pointer = real_pointer(app)
        if pointer is not None:
            with pointer:
                before = tiles(app)
                ok("B: the board starts from the seed arrangement", "loss@6,1" in before)
                pointer.drag(from_at=FROM, to_at=TO, steps=STEPS)
                assert_eq(
                    tiles(app),
                    SETTLED,
                    "B: a hand and the wire leave the same board. This is the "
                    "first demo in this tree to drive the machine's own pointer",
                )
                ok("B: and the drag really let go", not app.snapshot()["introspect"]["dragging"])

    # ── (C) read the scene while the button is DOWN ──────────────────────
    banner("C — the scene is readable mid-gesture, with the button held")
    with RpcSubprocess("hello-tile-dashboard", visible_window=True) as app:
        pointer = real_pointer(app)
        if pointer is not None:
            with pointer:
                held: dict = {}

                def look(_p: RealPointer) -> None:
                    held["tiles"] = tiles(app)
                    held["dragging"] = app.snapshot()["introspect"]["dragging"]
                    held["cards"] = cards(app)

                pointer.drag(from_at=FROM, to_at=TO, steps=STEPS, hold=look)

                assert_eq(
                    held["dragging"],
                    "loss",
                    "C: mid-gesture the board names what is being held — read "
                    "with the button still down, which is the capability R1726 "
                    "had to ask a person for",
                )
                ok(
                    "C: the held card has already reached its destination row",
                    "loss@0,4" in held["tiles"],
                )
                ok(
                    "C: and every card the drag displaced is readable there too",
                    "latency@0,5" in held["tiles"] and "topology@0,6" in held["tiles"],
                )
                ok(
                    "C: the paint is readable mid-gesture as well",
                    len(held["cards"]) == 5,
                )

    # ── (D) screen A: the same gesture on the node lab ───────────────────
    #
    # Screen A is the standing priority, and it is where the reference
    # specification lives as a VALUE: the screen publishes the table it was
    # built against, so this section asserts the placement gesture against the
    # specification rather than against numbers written down here.
    banner("D — screen A (node graph lab): a real pointer moves a node card")
    with RpcSubprocess("hello-node-lab", visible_window=True, boot_grace=1.5) as app:
        spec = json.loads(app.query("/external/spec"))
        ok("D: the screen publishes its own specification", "design" in spec)
        assert_eq(
            spec["design"],
            [1625, 900],
            "D: and the specification states the design surface the reference "
            "screen is laid out on",
        )
        nodes_before = app.query("/external/nodes")
        links_before = app.query("/external/links")
        verdict_before = json.loads(app.query("/external/verdict"))
        ok(
            f"D: the graph opens with {len(nodes_before.split(','))} nodes",
            len(nodes_before.split(",")) >= 6,
        )
        ok(
            "D: and the launch gate has already judged it",
            "may_launch" in verdict_before,
        )

        rects = abs_rects_of(app.snapshot(source="paint"))
        node_tags = sorted(
            t for t in rects if t.startswith("lab.node.") and t.count(".") == 2
        )
        ok(f"D: the screen paints {len(node_tags)} node cards", len(node_tags) >= 6)
        target = node_tags[0]
        start = rects[target]
        grab = (start[0] + start[2] / 2, start[1] + 12)
        drop = (grab[0] + 90, grab[1] + 70)

        pointer = real_pointer(app)
        if pointer is not None:
            with pointer:
                mid: dict = {}

                def look_at_the_graph(_p: RealPointer) -> None:
                    mid["rect"] = abs_rects_of(app.snapshot(source="paint"))[target]
                    mid["nodes"] = app.query("/external/nodes")

                pointer.drag(from_at=grab, to_at=drop, steps=6, hold=look_at_the_graph)

                ok(
                    "D: the card is already displaced while the button is DOWN",
                    mid["rect"][0] > start[0] and mid["rect"][1] > start[1],
                )
                assert_eq(
                    sorted(mid["nodes"].split(",")),
                    sorted(nodes_before.split(",")),
                    "D: and a placement mid-gesture adds and removes nothing — "
                    "a free canvas moves what you hold, it does not edit the graph",
                )
                # ★ The ORDER did move, and that is R1726's rule showing through
                # a real gesture for the first time: what you hold goes to the
                # front, and this list is the paint order. Asserted rather than
                # tolerated — the first draft of this section compared the
                # strings and failed, which is the check doing its job.
                ok(
                    "D: and the held card has been raised to the front of the "
                    "paint order (R1726, seen here under a real pointer)",
                    mid["nodes"].split(",")[-1] == target.rsplit(".", 1)[-1],
                )

                moved = abs_rects_of(app.snapshot(source="paint"))[target]
                ok(
                    f"D: {target} moved under a real pointer "
                    f"({start[0]},{start[1]} -> {moved[0]},{moved[1]})",
                    (moved[0], moved[1]) != (start[0], start[1]),
                )
                ok(
                    "D: and it went the way the pointer went",
                    moved[0] > start[0] and moved[1] > start[1],
                )
                assert_eq(
                    app.query("/external/links"),
                    links_before,
                    "D: the topology is untouched by a placement",
                )
                assert_eq(
                    json.loads(app.query("/external/verdict")),
                    verdict_before,
                    "D: so the launch gate's verdict does not move either — the "
                    "reference screen separates WHERE a node is drawn from WHAT "
                    "the graph is, and so does this one",
                )

    # ── (F) the three reference screens each answer a real pointer ───────
    #
    # The integration pass. A press driven by the machine's own pointer at the
    # centre of a painted rectangle has to change the screen's own published
    # state — the property `scene/pointer_reach` can only check statically, and
    # the one R1649 shipped a whole shell without.
    banner("F — screen A, B and C each answer a real press")
    for example, boot, tag_of, read, label in (
        (
            "hello-node-lab",
            1.5,
            lambda r: sorted(
                t for t in r if t.startswith("lab.node.") and t.count(".") == 2
            )[1],
            lambda a: a.query("/external/selected"),
            "screen A: pressing a node card selects it",
        ),
        (
            "hello-packet-view",
            1.2,
            lambda r: "pv.list.row.2",
            lambda a: str(a.query("/external/selected_row")),
            "screen B: pressing a capture row selects it",
        ),
        (
            "hello-tile-dashboard",
            1.0,
            lambda r: "dashboard#card.alarms",
            lambda a: a.query("/external/current"),
            "screen C: pressing a board card makes it current",
        ),
    ):
        with RpcSubprocess(example, visible_window=True, boot_grace=boot) as app:
            rects = abs_rects_of(app.snapshot(source="paint"))
            tag = tag_of(rects)
            ok(f"F: {example} paints {tag}", tag in rects)
            r = rects[tag]
            before = read(app)
            pointer = real_pointer(app)
            if pointer is None:
                continue
            with pointer:
                pointer.move((r[0] + r[2] / 2, r[1] + r[3] / 2), confirm=True)
                ok(f"F: the surface received the real pointer at {tag}", True)
                pointer.press()
                pointer.release()
                after = read(app)
                ok(
                    f"F: {label} ({before!r} -> {after!r})",
                    after != before,
                )

    # ── (E) the class, swept ─────────────────────────────────────────────
    #
    # ★★★★★ The gate the class debt `paint-and-gesture-read-two-facts` never
    # had. Every surface whose captured drag CHANGES what it is measured
    # against gets the same two deliveries, and has to answer once.
    banner("E — every captured drag in the analysis tool answers one fact")

    def painted(prefix: str):
        """What the screen LOOKS like — the reading that needs no schema."""

        def read(app: RpcSubprocess) -> str:
            rects = abs_rects_of(app.snapshot(source="paint"))
            return " ".join(
                f"{t}@{r[0]},{r[1]}" for t, r in sorted(rects.items()) if t.startswith(prefix)
            )

        return read

    def a_card_and_somewhere_to_put_it(example: str, prefix: str, depth: int):
        """Grab the first painted card's title bar; drop it down and right."""
        with RpcSubprocess(example, boot_grace=1.5) as probe:
            rects = abs_rects_of(probe.snapshot(source="paint"))
            tags = sorted(t for t in rects if t.startswith(prefix) and t.count(".") == depth)
            assert tags, f"{example} painted no {prefix}* card to drag"
            r = rects[tags[0]]
            grab = (r[0] + r[2] / 2, r[1] + 12)
            return grab, (grab[0] + 90, grab[1] + 70)

    swept = 0
    for example, prefix, depth, label in (
        ("hello-node-lab", "lab.node.", 2, "node lab: a node dragged across the canvas"),
        ("hello-node-editor", "node_", 0, "node editor: the same gesture on the other graph"),
    ):
        start, end = a_card_and_somewhere_to_put_it(example, prefix, depth)
        assert_gesture_reads_one_fact(
            example,
            from_at=start,
            to_at=end,
            steps=6,
            read=painted(prefix),
            label=label,
        )
        swept += 1
        ok(f"E: {label}", True)
    ok(f"E: {swept} further captured-drag surfaces swept", swept == 2)

    # ★★★★★ The demo's own coverage, said out loud — because a section that
    # quietly does not run is the exact shape this round exists to end. Four of
    # the six sections need a real pointer; on a host without one they are
    # skipped, and without this the only evidence would be a smaller number
    # nobody was comparing against anything.
    print(f"\n{len(CHECKS)} named check(s):")
    for line in CHECKS:
        print(f"  - {line}")
    driven = [c for c in CHECKS if c[0] in "BCDF"]
    if REAL_POINTER_RUNS == 0:
        print(
            f"[coverage] NO REAL POINTER on this host: {len(CHECKS)} checks ran, "
            "and every one of them came from the wire. The real-pointer sections "
            "(B, C, D, F) contributed nothing."
        )
    else:
        assert len(driven) >= 12, (
            f"the real pointer ran {REAL_POINTER_RUNS} time(s) but only "
            f"{len(driven)} check(s) came from it — a section stopped "
            "contributing without saying so"
        )
        print(
            f"[coverage] {REAL_POINTER_RUNS} real-pointer session(s) contributed "
            f"{len(driven)} of {len(CHECKS)} named checks."
        )


if __name__ == "__main__":
    run_demo("r1727 a gesture reads one fact", body)
